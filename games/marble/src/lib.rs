//! «Шаровой завод» — 3D-игра на Piska Engine: катите стальной шар по цехам заброшенного
//! завода в небе. Ядро (шар, камера, меню, правила) написано на Rust, механизмы цехов —
//! платформы, метлы, конвейеры, вентиляторы, прессы, бамперы, хрупкие плиты — на C++
//! (`cpp/*.cpp`, статически слинкованы через `engine-cpp-build`).
//!
//! Игра рассчитана на Windows, Linux и Android: C++ в браузер не собирается
//! (`docs/api/export.md`, раздел 7).
//!
//! Одна функция [`build`] собирает приложение; `engine::game_main!` делает из неё
//! программу. Тесты зовут [`build_with`] с сохранениями в памяти.

/// Скрипты на Rust, созданные в редакторе (New Rust Script).
pub mod scripts;

use engine::prelude::*;

pub mod ball;
pub mod courses;
pub mod game;
pub mod kit;
pub mod sounds;
pub mod ui;

pub use ball::{Ball, BallStatus, CameraRig, ViewState, GRAVITY};
pub use game::{Action, Boot, GameManager, GameState, Options, Progress, Screen};

/// Имя игры: каталог сохранений на десктопе.
pub const GAME_NAME: &str = "Marble Factory";

/// Зарегистрировать компоненты игры на Rust. Механизмы на C++ регистрирует
/// `engine::add_static_cpp` в [`build_with`].
pub fn register(app: &mut App) {
    scripts::register(app);
    app.register::<Ball>().register::<CameraRig>().register::<GameManager>().register::<kit::SeesawRig>();
}

/// Собрать игру с заданными сохранениями и режимом запуска.
pub fn build_with(save: SaveData, boot: Boot) -> Result<App, EngineError> {
    let mut app = engine::app();
    // Механизмы цехов на C++ линкуются в крейт статически (build.rs → engine-cpp-build).
    engine::add_static_cpp(&mut app, engine::cpp_scripts!())?;
    register(&mut app);

    // Ввод: пауза — Esc или Start. Остальное (оси, прыжок) — стандартные действия движка.
    app.world.input.set_button_bindings("Pause", vec![Key::Escape.into(), GamepadButton::Start.into()]);

    let state = GameState::new(&save, boot);
    app.world.resources.insert(save);
    app.world.resources.insert(state);
    app.world.resources.insert(ViewState::default());
    app.world.resources.insert(BallStatus::default());
    app.world.resources.insert(ball::RollVoice::default());

    engine::mount_assets(&mut app, engine::assets_dir!())?;

    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        sounds::register(audio);
        audio.add_clip(sounds::MUSIC_MENU, sounds::music_menu());
        audio.add_clip(sounds::MUSIC_FACTORY_A, sounds::music_factory_a());
        audio.add_clip(sounds::MUSIC_FACTORY_B, sounds::music_factory_b());
    }

    // Камера с «ушами», солнце, атмосфера и менеджер игры. Всё остальное — цех, шар и
    // интерфейс — создаёт менеджер.
    app.spawn("Camera")
        .tag("MainCamera")
        .at3(0.0, 8.0, 14.0)
        .with(Camera { background: Background::Sky, far: 1800.0, ..Camera::perspective(62.0) })
        .with(CameraRig { orbit: true, ..CameraRig::default() })
        .with(AudioListener::default());
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.62, 0.0))
        .with(Light { color: Color::rgb(1.0, 0.86, 0.68), ..Light::directional(4.6) });
    app.spawn("Environment").with(Environment {
        ambient: 0.32,
        fog_density: 0.0004,
        exposure: 1.0,
        bloom: 0.1,
        vignette: 0.2,
        shadow_distance: 80.0,
    });
    app.spawn("Game").with(GameManager::default());
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
