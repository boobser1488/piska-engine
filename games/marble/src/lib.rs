//! «Шаровой завод» — 3D-игра на Piska Engine: катите стальной шар по цехам заброшенного
//! завода в небе.
//!
//! Игра — образец того, как устроен проект: **содержимое лежит в ассетах** (сцены
//! `assets/scenes/`, префабы `assets/prefabs/`, интерфейс `assets/ui/`, материалы
//! `assets/materials/`, модели `assets/models/`, звуки `assets/audio/`, данные
//! `assets/data/`, настройки `assets/project.settings`), а **код — это скрипты**
//! (`src/scripts/`, по одному компоненту в файле: механизмы цехов, шар, камера, менеджер
//! игры). Здесь ничего не строится из кода: `build()` подключает ассеты, регистрирует
//! скрипты и загружает сцену меню. Всё остальное — объекты в сценах: откройте любую в
//! редакторе (`cargo run -p marble --features editor --bin marble-editor`).
//!
//! Игра целиком на Rust (C++ по умолчанию выключен), поэтому собирается и для браузера.
//! Сцены и префабы пишет инструмент `examples/bake_assets.rs` (`tools/bake/`); он нужен
//! только для перегенерации и в игре не используется.

/// Ядро без компонентов: данные из ассетов, состояние, поток игры.
pub mod game;
/// Скрипты на Rust (редактор: Assets → New Rust Script).
pub mod scripts;

use engine::prelude::*;

pub use game::data::{CourseInfo, Route, Waypoint, WpKind, COUNT};
pub use game::state::{medal_for, Action, Boot, GameState, Options, Progress, RunResult, Screen};
pub use game::view::{BallStatus, ViewState, BALL_RADIUS, GRAVITY};

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Marble Factory";

/// Зарегистрировать компоненты игры.
pub fn register(app: &mut App) {
    scripts::register(app);
}

/// Собрать игру с заданными сохранениями и режимом запуска: ассеты, скрипты, ресурсы,
/// сцена меню. Объекты не создаются — они в `assets/scenes/menu.scene`.
pub fn build_with(save: SaveData, boot: Boot) -> Result<App, EngineError> {
    build_in(save, boot, engine::assets_dir!())
}

/// То же, но с каталогом ассетов `assets` (копия проекта после правок в редакторе — в тестах).
pub fn build_in(save: SaveData, boot: Boot, assets: impl AsRef<std::path::Path>) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    #[cfg(feature = "cpp-example")]
    engine::add_static_cpp(&mut app, engine::cpp_scripts!())?;

    let state = GameState::new(&save, boot);
    app.world.resources.insert(save);
    app.world.resources.insert(state);
    app.world.resources.insert(ViewState::default());
    app.world.resources.insert(BallStatus::default());
    app.world.resources.insert(game::view::RollVoice::default());

    // Настройки проекта (ввод, гравитация, слои, окно) читаются отсюда же.
    engine::mount_assets(&mut app, assets)?;
    engine::load_scene(&mut app, "scenes/menu.scene")?;
    Ok(app)
}

/// Режим запуска из окружения — для скриншотов и ручной проверки (не в браузере).
///
/// `MARBLE_SCREEN` — `menu`, `select`, `options`, `c1`…`c8`, `pause`, `results`, `lose`;
/// `MARBLE_AT` — доля маршрута (0..1), куда поставить шар; `MARBLE_DEMO=1` — примерный
/// прогресс для экрана выбора.
fn boot_from_env() -> Boot {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Boot {
            screen: std::env::var("MARBLE_SCREEN").ok(),
            at: std::env::var("MARBLE_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            demo_progress: std::env::var("MARBLE_DEMO").is_ok_and(|v| v != "0"),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        Boot::default()
    }
}

/// Собрать игру: сохранения — там, где они живут на этой платформе.
pub fn build() -> Result<App, EngineError> {
    build_with(engine::save_data(GAME_NAME), boot_from_env())
}

engine::game_main!(build, WindowConfig::titled("Шаровой завод").with_size(1280, 720));
