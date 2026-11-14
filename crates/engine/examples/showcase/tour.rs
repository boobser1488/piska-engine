//! Компоненты сцен витрины (вращение крыльев мельницы и чаек, мерцание костра, качка
//! лодки) и облёт сцен камерой в оконном режиме.

use engine::prelude::*;

use super::geo::{sstep, ssstep};

component! {
    /// Вращение вокруг локальной оси с постоянной скоростью (крылья мельницы, круги чаек).
    pub struct Spin {
        /// Ось в локальных координатах объекта.
        pub axis: Vec3 = Vec3::Y,
        /// Градусов в секунду.
        pub speed: f32 = 30.0,
    }
}

impl Component for Spin {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let axis = self.axis.try_normalize().unwrap_or(Vec3::Y);
        let turn = Quat::from_axis_angle(axis, self.speed.to_radians() * dt);
        let t = ctx.transform();
        t.rotation = (t.rotation * turn).normalize();
    }
}

component! {
    /// Мерцание точечного света (костёр): яркость «дышит» вокруг исходной.
    pub struct Flicker {
        /// Размах: 0.2 — ±20 % яркости.
        pub amount: f32 = 0.2,
        /// Скорость мерцания.
        pub speed: f32 = 8.0,
        /// Исходная яркость (запоминается на первом кадре; 0 — ещё не запомнена).
        pub base: f32 = 0.0,
        pub time: f32 = 0.0,
    }
}

impl Component for Flicker {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.time += dt * self.speed;
        let t = self.time;
        let wave = (t.sin() * 0.5 + (t * 2.37 + 1.3).sin() * 0.3 + (t * 5.11 + 0.7).sin() * 0.2).clamp(-1.0, 1.0);
        if let Some(light) = ctx.get_mut::<Light>() {
            if self.base <= 0.0 {
                self.base = light.intensity;
            }
            light.intensity = self.base * (1.0 + wave * self.amount);
        }
    }
}

component! {
    /// Покачивание на воде: вверх-вниз и крен.
    pub struct Bob {
        /// Амплитуда по высоте, м.
        pub height: f32 = 0.1,
        /// Амплитуда крена, градусы.
        pub roll: f32 = 3.0,
        pub speed: f32 = 1.0,
        pub time: f32 = 0.0,
        /// Исходные позиция и курс (запоминаются на первом кадре).
        pub origin: Vec3 = Vec3::ZERO,
        pub yaw: f32 = 0.0,
        pub started: bool = false,
    }
}

impl Component for Bob {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if !self.started {
            self.started = true;
            self.origin = ctx.transform().position;
            self.yaw = ctx.transform().euler_deg().y;
        }
        self.time += dt * self.speed;
        let t = self.time;
        let tr = ctx.transform();
        tr.position = self.origin + Vec3::Y * (t * 1.3).sin() * self.height;
        tr.set_euler_deg(Vec3::new((t * 0.9).sin() * self.roll * 0.5, self.yaw, (t * 1.1 + 0.5).sin() * self.roll));
    }
}

/// Зарегистрировать компоненты витрины (нужно до загрузки сцен).
pub fn register(app: &mut App) {
    app.register::<Spin>();
    app.register::<Flicker>();
    app.register::<Bob>();
}

/// Камеры-закладки сцены: (имя, позиция, поворот) в порядке иерархии `Shots`.
pub fn shots(world: &World) -> Vec<(String, GameObjectId, Vec3, Quat)> {
    let scene = &world.scene;
    let Some(root) = scene.find_by_name("Shots") else { return Vec::new() };
    let Some(object) = scene.get(root) else { return Vec::new() };
    object
        .children()
        .iter()
        .filter_map(|&id| {
            let o = scene.get(id)?;
            Some((o.name.clone(), id, o.transform.position, o.transform.rotation))
        })
        .collect()
}

/// Включить камеру-закладку `name`, выключить остальные. `false` — такой нет.
pub fn activate_shot(world: &mut World, name: &str) -> bool {
    let list = shots(world);
    if !list.iter().any(|(n, ..)| n == name) {
        return false;
    }
    for (n, id, ..) in list {
        world.scene.set_active(id, n == name);
    }
    true
}

/// Облёт: камера плавно перелетает между закладками сцены, после последней — затемнение
/// и следующая сцена. Цифры — сразу к сцене, пробел — пауза, Esc — выход.
pub struct Tour {
    pub scenes: Vec<String>,
    pub index: usize,
    /// Секунд на перелёт между закладками.
    pub leg: f32,
    time: f32,
    paused: bool,
    loaded: bool,
    path: Vec<(Vec3, Quat)>,
    camera: Option<GameObjectId>,
    exposure: f32,
}

impl Tour {
    pub fn new(scenes: Vec<String>) -> Self {
        Self { scenes, index: 0, leg: 7.0, time: 0.0, paused: false, loaded: false, path: Vec::new(), camera: None, exposure: 1.0 }
    }

    fn load(&mut self, world: &mut World) {
        let path = &self.scenes[self.index];
        let Some(db) = world.resources.get::<AssetDatabase>().cloned() else { return };
        let text = db
            .handle::<engine::assets::SceneAsset>(path)
            .and_then(|h| db.read_text(h.guid()));
        match text {
            Ok(text) => {
                if let Err(error) = world.load_scene_str(&text, path) {
                    log::error!("showcase: cannot load `{path}`: {error}");
                }
            }
            Err(error) => log::error!("showcase: `{path}`: {error}. Run `cargo run --example showcase -- --generate` first"),
        }
        let list = shots(world);
        self.path = list.iter().map(|(_, _, p, r)| (*p, *r)).collect();
        self.camera = list.iter().find(|(n, ..)| n == "hero").map(|(_, id, ..)| *id);
        for (n, id, ..) in &list {
            world.scene.set_active(*id, n == "hero");
        }
        self.exposure = 1.0;
        world.scene.visit_with::<Environment>(|_, env| self.exposure = env.exposure);
        self.time = 0.0;
        self.loaded = true;
    }
}

impl Subsystem for Tour {
    fn name(&self) -> &'static str {
        "showcase tour"
    }

    fn pre_update(&mut self, world: &mut World) {
        let digits = [Key::Digit1, Key::Digit2, Key::Digit3, Key::Digit4, Key::Digit5];
        let mut jump = None;
        for (i, key) in digits.into_iter().enumerate() {
            if i < self.scenes.len() && world.input.key_down(key) {
                jump = Some(i);
            }
        }
        if world.input.key_down(Key::Escape) {
            world.request_quit();
        }
        if world.input.key_down(Key::Space) {
            self.paused = !self.paused;
        }
        if let Some(i) = jump {
            self.index = i;
            self.loaded = false;
        }
        if !self.loaded {
            self.load(world);
        }
        let dt = world.time.unscaled_delta.clamp(0.0, 0.1);
        if !self.paused {
            self.time += dt;
        }
        let legs = self.path.len().saturating_sub(1).max(1) as f32;
        let total = legs * self.leg;
        if self.time > total + 1.0 {
            self.index = (self.index + 1) % self.scenes.len();
            self.load(world);
            return;
        }
        // Затемнение: 1 с в начале и в конце сцены.
        let fade = sstep(0.0, 1.0, self.time).min(sstep(total + 1.0, total, self.time));
        let exposure = self.exposure * fade.max(0.02);
        world.scene.for_each_with_mut::<Environment>(|_, env| env.exposure = exposure);
        let (Some(camera), false) = (self.camera, self.path.is_empty()) else { return };
        let t = (self.time / self.leg).clamp(0.0, legs);
        let k = (t.floor() as usize).min(self.path.len().saturating_sub(2));
        let s = ssstep(0.0, 1.0, t - k as f32);
        let (a, b) = (self.path[k], self.path[(k + 1).min(self.path.len() - 1)]);
        if let Some(object) = world.scene.get_mut(camera) {
            // Дуга вверх посередине перелёта — камера не проходит сквозь предметы.
            let lift = (s * std::f32::consts::PI).sin() * a.0.distance(b.0) * 0.12;
            object.transform.position = a.0.lerp(b.0, s) + Vec3::Y * lift;
            object.transform.rotation = a.1.slerp(b.1, s);
        }
    }
}
