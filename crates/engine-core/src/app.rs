//! App — главный цикл. Один вызов [`App::tick`] = один кадр.

use crate::component::Component;
use crate::profile::{Stopwatch, TickProfile};
use crate::reflect::TypeName;
use crate::scene::ObjectBuilder;
use crate::world::World;

/// Подсистема движка: физика, аудио, C++-плагины, сетевой код.
/// Подсистемы вызываются в порядке добавления.
#[allow(unused_variables)]
pub trait Subsystem: 'static {
    fn name(&self) -> &'static str;
    /// Один раз перед первым кадром.
    fn init(&mut self, world: &mut World) {}
    /// В начале кадра, до скриптов.
    fn pre_update(&mut self, world: &mut World) {}
    /// Перед `on_fixed_update` скриптов: здесь физика возвращает в `Transform`
    /// точные (не сглаженные) позы тел.
    fn before_fixed_update(&mut self, world: &mut World) {}
    /// На каждом фиксированном шаге, сразу после `on_fixed_update` скриптов (здесь шагает физика).
    fn fixed_update(&mut self, world: &mut World, dt: f32) {}
    /// Раз в кадр после всех фиксированных шагов, перед `on_update`. `alpha` (0..=1) —
    /// где «сейчас» находится между предпоследним и последним шагом физики; по нему
    /// физика сглаживает позы для отрисовки.
    fn interpolate(&mut self, world: &mut World, alpha: f32) {}
    /// После `on_late_update` скриптов.
    fn post_update(&mut self, world: &mut World) {}
    /// Игра стоит (редактор в режиме Edit или на паузе), но подсистема может
    /// заняться фоновыми делами: например, подхватить пересобранный плагин.
    fn idle(&mut self, world: &mut World) {}
}

/// Приложение: мир + подсистемы + накопитель фиксированного шага.
pub struct App {
    pub world: World,
    subsystems: Vec<Box<dyn Subsystem>>,
    accumulator: f32,
    initialized: bool,
    profile: TickProfile,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Не больше стольких фиксированных шагов за кадр: при «подвисании» игра
    /// замедлится, а не уйдёт в спираль смерти.
    const MAX_FIXED_STEPS: u32 = 5;

    /// «Голое» приложение без рендера и физики — для тестов и серверов.
    pub fn new() -> Self {
        Self {
            world: World::new(),
            subsystems: Vec::new(),
            accumulator: 0.0,
            initialized: false,
            profile: TickProfile::default(),
        }
    }

    /// Куда ушло время последнего [`tick`](Self::tick): скрипты, каждая подсистема,
    /// конец кадра. На этом стоят профайлер редактора и пример `stress`.
    pub fn profile(&self) -> &TickProfile {
        &self.profile
    }

    /// Зарегистрировать тип компонента, чтобы его можно было загружать из сцен.
    pub fn register<T: Component + Default + TypeName>(&mut self) -> &mut Self {
        self.world.registry.register::<T>();
        self
    }

    pub fn add_subsystem(&mut self, subsystem: impl Subsystem) -> &mut Self {
        self.subsystems.push(Box::new(subsystem));
        self
    }

    /// Создать объект в текущей сцене: `app.spawn("Player").with(Mover::default());`
    pub fn spawn(&mut self, name: impl Into<String>) -> ObjectBuilder<'_> {
        self.world.scene.build(name)
    }

    /// Один кадр длительностью `dt` секунд реального времени.
    pub fn tick(&mut self, dt: f32) {
        let frame_clock = Stopwatch::start();
        // Время каждой подсистемы копится в том же векторе, что и в прошлом кадре:
        // замер не выделяет память.
        let profile = &mut self.profile;
        profile.subsystems.clear();
        profile.subsystems.extend(self.subsystems.iter().map(|s| (s.name(), 0.0)));
        profile.scripts_ms = 0.0;
        profile.fixed_steps = 0;
        let times = &mut profile.subsystems;
        let mut scripts_ms = 0.0;
        // Вызвать хук у всех подсистем, засекая время каждой.
        macro_rules! each_subsystem {
            (|$s:ident| $call:expr) => {
                for (index, $s) in self.subsystems.iter_mut().enumerate() {
                    let clock = Stopwatch::start();
                    $call;
                    times[index].1 += clock.ms();
                }
            };
        }
        macro_rules! scripts {
            ($call:expr) => {{
                let clock = Stopwatch::start();
                $call;
                scripts_ms += clock.ms();
            }};
        }
        let world = &mut self.world;

        if !self.initialized {
            self.initialized = true;
            for subsystem in &mut self.subsystems {
                subsystem.init(world);
            }
        }

        // Нулевой, отрицательный или «битый» масштаб не должны ломать цикл: пауза — это 0.
        let scale = if world.time.time_scale.is_finite() {
            world.time.time_scale.max(0.0)
        } else {
            1.0
        };
        let real_dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 }.min(world.time.max_delta.max(0.0));
        let dt = real_dt * scale;
        world.time.delta = dt;
        world.time.elapsed += dt as f64;
        world.time.unscaled_delta = real_dt;
        world.time.unscaled_elapsed += real_dt as f64;
        world.time.frame += 1;

        each_subsystem!(|subsystem| subsystem.pre_update(world));
        scripts!(world.start_pending());

        // Фиксированный шаг: скрипты → физика.
        //
        // Симуляция идёт «на шаг вперёд»: шагаем, пока она не покроет текущий момент,
        // а на экран попадает смесь двух последних шагов (`fixed_alpha`). Поэтому на
        // 60 Гц виден результат шага этого же кадра (нет лишнего кадра задержки), а на
        // 75 и 144 Гц движение остаётся ровным, хотя шаги случаются не в каждом кадре.
        let fixed = world.time.fixed_delta;
        self.accumulator = (self.accumulator + dt).min(fixed * Self::MAX_FIXED_STEPS as f32);
        while self.accumulator > 1e-6 {
            self.accumulator -= fixed;
            profile.fixed_steps += 1;
            each_subsystem!(|subsystem| subsystem.before_fixed_update(world));
            // Нажатия копятся до первого шага: короткое нажатие не теряется.
            world.input.begin_fixed_step();
            scripts!(world.fixed_update(fixed));
            each_subsystem!(|subsystem| subsystem.fixed_update(world, fixed));
            scripts!(world.flush_messages());
            world.input.end_fixed_step();
        }
        world.time.fixed_alpha = (1.0 + self.accumulator / fixed).clamp(0.0, 1.0);
        each_subsystem!(|subsystem| subsystem.interpolate(world, world.time.fixed_alpha));

        // Таймеры и твины — прямо перед скриптами, поэтому `on_update` видит их результат.
        scripts!(world.run_scheduled());

        scripts!(world.update(dt));
        scripts!(world.late_update(dt));

        each_subsystem!(|subsystem| subsystem.post_update(world));
        let end_clock = Stopwatch::start();
        world.end_frame();
        profile.end_frame_ms = end_clock.ms();
        profile.scripts_ms = scripts_ms;
        profile.total_ms = frame_clock.ms();
    }

    /// Кадр без игры: редактор зовёт это вместо [`tick`](Self::tick), пока игра стоит.
    pub fn idle(&mut self) {
        for subsystem in &mut self.subsystems {
            subsystem.idle(&mut self.world);
        }
    }

    /// Прогнать `frames` кадров без окна. Основа тестов геймплея и CI.
    pub fn run_headless(&mut self, frames: u32, dt: f32) {
        for _ in 0..frames {
            self.tick(dt);
        }
    }
}
