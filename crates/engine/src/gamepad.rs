//! Геймпады: подсистема опрашивает `gilrs` (десктоп) или Gamepad API браузера (веб)
//! и пишет состояние в `world.input`.
//!
//! Нет геймпада, система не поддерживается или `gilrs` не запустился — подсистема молча
//! ничего не делает: игра по-прежнему работает с клавиатурой и мышью. Читать геймпады
//! нужно через `Input` (`input.axis("Horizontal")`, `input.gamepad_button_down(..)`), а не
//! через этот модуль.

use engine_core::app::Subsystem;
use engine_core::world::World;

/// Подсистема ввода с геймпадов. Добавляется в `engine::app()`.
pub struct GamepadSubsystem {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    backend: Option<backend::Backend>,
    #[cfg(target_arch = "wasm32")]
    backend: Option<web_backend::Backend>,
}

impl Default for GamepadSubsystem {
    fn default() -> Self {
        Self::new()
    }
}

impl GamepadSubsystem {
    pub fn new() -> Self {
        Self {
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", target_arch = "wasm32"))]
            backend: None,
        }
    }
}

impl Subsystem for GamepadSubsystem {
    fn name(&self) -> &'static str {
        "gamepad"
    }

    fn init(&mut self, world: &mut World) {
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            self.backend = backend::Backend::new(world);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = world;
            self.backend = web_backend::Backend::new();
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos", target_arch = "wasm32")))]
        let _ = world;
    }

    /// События геймпадов разбираются в начале кадра, до скриптов: `on_update` и
    /// `on_fixed_update` этого кадра уже видят свежее состояние.
    fn pre_update(&mut self, world: &mut World) {
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", target_arch = "wasm32"))]
        if let Some(backend) = &mut self.backend {
            backend.pump(world);
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos", target_arch = "wasm32")))]
        let _ = world;
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod backend {
    use engine_core::input::{GamepadAxis, GamepadButton, GamepadId};
    use engine_core::world::World;
    use gilrs::{Axis, Button, EventType, Gilrs};

    pub struct Backend {
        gilrs: Gilrs,
    }

    impl Backend {
        /// `None`, если `gilrs` не смог стартовать (нет прав на устройства, драйвера...).
        pub fn new(world: &mut World) -> Option<Self> {
            let gilrs = match Gilrs::new() {
                Ok(gilrs) => gilrs,
                Err(error) => {
                    log::info!("gamepad input is unavailable: {error}");
                    return None;
                }
            };
            // Геймпады, подключённые до запуска игры, событий «Connected» не присылают.
            for (id, pad) in gilrs.gamepads() {
                world.input.connect_gamepad(pad_id(id), pad.name());
                log::info!("gamepad connected: {}", pad.name());
            }
            Some(Self { gilrs })
        }

        pub fn pump(&mut self, world: &mut World) {
            while let Some(event) = self.gilrs.next_event() {
                let pad = pad_id(event.id);
                let input = &mut world.input;
                match event.event {
                    EventType::Connected => {
                        let name = self.gilrs.gamepad(event.id).name().to_string();
                        log::info!("gamepad connected: {name}");
                        input.connect_gamepad(pad, &name);
                    }
                    EventType::Disconnected | EventType::Dropped => {
                        log::info!("gamepad {} disconnected", pad.0);
                        input.disconnect_gamepad(pad);
                    }
                    EventType::ButtonPressed(button, _) => set_button(input, pad, button, true),
                    EventType::ButtonReleased(button, _) => set_button(input, pad, button, false),
                    // Триггеры приходят как кнопки с плавным значением.
                    EventType::ButtonChanged(button, value, _) => match button {
                        Button::LeftTrigger2 => input.set_gamepad_axis(pad, GamepadAxis::LeftTrigger, value),
                        Button::RightTrigger2 => input.set_gamepad_axis(pad, GamepadAxis::RightTrigger, value),
                        _ => {}
                    },
                    EventType::AxisChanged(axis, value, _) => match axis {
                        Axis::LeftStickX => input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, value),
                        Axis::LeftStickY => input.set_gamepad_axis(pad, GamepadAxis::LeftStickY, value),
                        Axis::RightStickX => input.set_gamepad_axis(pad, GamepadAxis::RightStickX, value),
                        Axis::RightStickY => input.set_gamepad_axis(pad, GamepadAxis::RightStickY, value),
                        // На части платформ крестовина приходит осями.
                        Axis::DPadX => {
                            input.set_gamepad_button(pad, GamepadButton::DPadLeft, value < -0.5);
                            input.set_gamepad_button(pad, GamepadButton::DPadRight, value > 0.5);
                        }
                        Axis::DPadY => {
                            input.set_gamepad_button(pad, GamepadButton::DPadDown, value < -0.5);
                            input.set_gamepad_button(pad, GamepadButton::DPadUp, value > 0.5);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    fn pad_id(id: gilrs::GamepadId) -> GamepadId {
        GamepadId(usize::from(id) as u32)
    }

    fn set_button(input: &mut engine_core::input::Input, pad: GamepadId, button: Button, down: bool) {
        let mapped = match button {
            Button::South => GamepadButton::South,
            Button::East => GamepadButton::East,
            Button::West => GamepadButton::West,
            Button::North => GamepadButton::North,
            Button::LeftTrigger => GamepadButton::LeftBumper,
            Button::RightTrigger => GamepadButton::RightBumper,
            Button::LeftTrigger2 => GamepadButton::LeftTrigger,
            Button::RightTrigger2 => GamepadButton::RightTrigger,
            Button::Select => GamepadButton::Select,
            Button::Start => GamepadButton::Start,
            Button::Mode => GamepadButton::Mode,
            Button::LeftThumb => GamepadButton::LeftStick,
            Button::RightThumb => GamepadButton::RightStick,
            Button::DPadUp => GamepadButton::DPadUp,
            Button::DPadDown => GamepadButton::DPadDown,
            Button::DPadLeft => GamepadButton::DPadLeft,
            Button::DPadRight => GamepadButton::DPadRight,
            _ => return,
        };
        input.set_gamepad_button(pad, mapped, down);
    }
}

/// Браузер: Gamepad API (`navigator.getGamepads()`). Событий нет — состояние
/// опрашивается каждый кадр и сравнивается с прошлым (это делает `Input`: повторная
/// установка того же значения ничего не меняет).
#[cfg(target_arch = "wasm32")]
mod web_backend {
    use std::collections::HashMap;

    use engine_core::input::{GamepadAxis, GamepadButton, GamepadId, Input};
    use engine_core::world::World;
    use wasm_bindgen::JsCast;

    /// Кнопки «стандартной» раскладки W3C (`mapping == "standard"`) по номерам.
    const BUTTONS: [GamepadButton; 17] = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::West,
        GamepadButton::North,
        GamepadButton::LeftBumper,
        GamepadButton::RightBumper,
        GamepadButton::LeftTrigger,
        GamepadButton::RightTrigger,
        GamepadButton::Select,
        GamepadButton::Start,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
        GamepadButton::Mode,
    ];
    /// Номера кнопок-триггеров в стандартной раскладке: у них плавное значение.
    const LEFT_TRIGGER: u32 = 6;
    const RIGHT_TRIGGER: u32 = 7;

    pub struct Backend {
        /// Подключённые геймпады: номер браузера → имя.
        known: HashMap<u32, String>,
    }

    impl Backend {
        /// `None`, если нет окна браузера (например, в веб-воркере).
        pub fn new() -> Option<Self> {
            web_sys::window()?;
            Some(Self { known: HashMap::new() })
        }

        pub fn pump(&mut self, world: &mut World) {
            let Some(window) = web_sys::window() else {
                return;
            };
            // Браузер может запретить доступ (политика страницы) — тогда геймпадов нет.
            let Ok(pads) = window.navigator().get_gamepads() else {
                return;
            };
            let input = &mut world.input;
            let mut seen: Vec<u32> = Vec::new();
            for value in pads.iter() {
                // Пустые места массива — `null`.
                let Ok(pad) = value.dyn_into::<web_sys::Gamepad>() else {
                    continue;
                };
                if !pad.connected() {
                    continue;
                }
                let index = pad.index();
                seen.push(index);
                let id = GamepadId(index);
                if !self.known.contains_key(&index) {
                    let name = pad.id();
                    if pad.mapping() != web_sys::GamepadMappingType::Standard {
                        log::warn!("gamepad `{name}` has no standard mapping in this browser; buttons may be mixed up");
                    }
                    log::info!("gamepad connected: {name}");
                    input.connect_gamepad(id, &name);
                    self.known.insert(index, name);
                }
                read_pad(input, id, &pad);
            }
            let gone: Vec<u32> = self.known.keys().copied().filter(|index| !seen.contains(index)).collect();
            for index in gone {
                self.known.remove(&index);
                log::info!("gamepad {index} disconnected");
                input.disconnect_gamepad(GamepadId(index));
            }
        }
    }

    fn read_pad(input: &mut Input, id: GamepadId, pad: &web_sys::Gamepad) {
        let buttons = pad.buttons();
        let button = |index: u32| buttons.get(index).dyn_into::<web_sys::GamepadButton>().ok();
        for (index, mapped) in BUTTONS.iter().enumerate() {
            if let Some(state) = button(index as u32) {
                input.set_gamepad_button(id, *mapped, state.pressed());
            }
        }
        if let Some(state) = button(LEFT_TRIGGER) {
            input.set_gamepad_axis(id, GamepadAxis::LeftTrigger, state.value() as f32);
        }
        if let Some(state) = button(RIGHT_TRIGGER) {
            input.set_gamepad_axis(id, GamepadAxis::RightTrigger, state.value() as f32);
        }
        let axes = pad.axes();
        let axis = |index: u32| axes.get(index).as_f64().unwrap_or(0.0) as f32;
        // В браузере ось Y стиков направлена вниз, в `Input` — вверх.
        input.set_gamepad_axis(id, GamepadAxis::LeftStickX, axis(0));
        input.set_gamepad_axis(id, GamepadAxis::LeftStickY, -axis(1));
        input.set_gamepad_axis(id, GamepadAxis::RightStickX, axis(2));
        input.set_gamepad_axis(id, GamepadAxis::RightStickY, -axis(3));
    }
}
