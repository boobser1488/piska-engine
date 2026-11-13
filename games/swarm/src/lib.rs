//! «Рой» — аркадный шутер на выживание в круглой арене на Piska Engine.
//!
//! Одинокий дрон-ремонтник защищает реактор от волн взбесившихся машин: 20 волн,
//! 4 босса, 18 улучшений, бесконечный режим. Логика — в [`sim`] (без сцены и рендера),
//! отображение — в [`view`], интерфейс — в [`ui`], ход игры — в [`flow`].
//!
//! Управление: WASD / левый стик — движение; мышь / правый стик / стрелки — прицел и
//! огонь; Пробел / RB — рывок; Esc / Start — пауза; на тачскрине — два экранных стика.

pub mod atlas;
pub mod boss;
pub mod bot;
pub mod data;
pub mod debug;
pub mod flow;
pub mod grid;
pub mod profile;
pub mod rng;
pub mod sim;
pub mod sound;
pub mod ui;
pub mod view;

/// Скрипты на Rust, созданные в редакторе (New Rust Script).
pub mod scripts;

use engine::prelude::*;

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Piska Swarm";

/// Зарегистрировать компоненты игры (сцена ссылается на них по имени).
pub fn register(app: &mut App) {
    scripts::register(app);
    app.register::<flow::Director>();
}

/// Привязки ввода: движение WASD / левый стик, прицел стрелками / правый стик, рывок, огонь.
fn bind_input(input: &mut Input) {
    input.set_axis_bindings(
        "Horizontal",
        AxisBindings::pair(Key::A, Key::D).also(GamepadButton::DPadLeft, GamepadButton::DPadRight).with_analog(GamepadAxis::LeftStickX),
    );
    input.set_axis_bindings(
        "Vertical",
        AxisBindings::pair(Key::S, Key::W).also(GamepadButton::DPadDown, GamepadButton::DPadUp).with_analog(GamepadAxis::LeftStickY),
    );
    input.set_axis_bindings("AimX", AxisBindings::pair(Key::Left, Key::Right).with_analog(GamepadAxis::RightStickX));
    input.set_axis_bindings("AimY", AxisBindings::pair(Key::Down, Key::Up).with_analog(GamepadAxis::RightStickY));
    input.set_button_bindings("Dash", vec![Key::Space.into(), Key::LShift.into(), GamepadButton::RightBumper.into(), GamepadButton::LeftBumper.into(), GamepadButton::South.into()]);
    input.set_button_bindings("Fire", vec![Binding::Mouse(MouseButton::Left), GamepadButton::RightTrigger.into(), GamepadButton::LeftTrigger.into()]);
}

/// Собрать игру с заданным хранилищем сохранений (тесты передают `SaveData::in_memory()`).
pub fn build_with(save: SaveData) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    app.world.resources.insert(save);
    bind_input(&mut app.world.input);
    engine::mount_assets(&mut app, engine::assets_dir!())?;
    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        sound::register(audio);
    }
    engine::load_scene(&mut app, "scenes/arena.scene")?;
    Ok(app)
}

/// Собрать игру: сохранения — там, где они живут на этой платформе; зерно случайности — своё
/// на каждый запуск.
pub fn build() -> Result<App, EngineError> {
    let mut app = build_with(engine::save_data(GAME_NAME))?;
    app.world.resources.insert(Random::from_entropy());
    Ok(app)
}

engine::game_main!(build, WindowConfig::titled("Рой").with_size(1280, 720));
