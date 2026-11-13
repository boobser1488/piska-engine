//! Замеры времени кадра: куда ушло время последнего [`App::tick`](crate::app::App::tick).
//!
//! Замер дешёвый (два чтения часов на хук подсистемы), поэтому включён всегда.
//! В браузере (`wasm32-unknown-unknown`) `std::time::Instant` нет — там все
//! длительности равны нулю, а код движка не меняется.

/// Секундомер, который собирается под любую платформу.
#[derive(Clone, Copy, Debug)]
pub struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

impl Stopwatch {
    pub fn start() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
        }
    }

    /// Миллисекунд с запуска (в браузере — 0).
    pub fn ms(&self) -> f32 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start.elapsed().as_secs_f32() * 1000.0
        }
        #[cfg(target_arch = "wasm32")]
        {
            0.0
        }
    }

    /// Миллисекунд с запуска; секундомер перезапускается.
    pub fn lap(&mut self) -> f32 {
        let ms = self.ms();
        *self = Self::start();
        ms
    }
}

/// Куда ушло время одного кадра `App::tick`, в миллисекундах.
/// Берётся через [`App::profile`](crate::app::App::profile).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickProfile {
    /// Весь `tick`.
    pub total_ms: f32,
    /// Хуки компонентов: `on_start`, `on_fixed_update`, `on_update`, `on_late_update`,
    /// доставка сообщений, таймеры и твины.
    pub scripts_ms: f32,
    /// Все хуки каждой подсистемы за кадр — в порядке подключения: `("physics3d", 1.2)`.
    pub subsystems: Vec<(&'static str, f32)>,
    /// Конец кадра: уничтожение объектов, смена сцены, события.
    pub end_frame_ms: f32,
    /// Сколько фиксированных шагов сделано в этом кадре.
    pub fixed_steps: u32,
}

impl TickProfile {
    /// Время подсистемы по имени (0, если такой нет).
    pub fn subsystem_ms(&self, name: &str) -> f32 {
        self.subsystems
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, ms)| ms)
            .sum()
    }
}
