//! Ввод в стиле Unity Input Manager: клавиши, мышь, геймпады и именованные действия.
//!
//! Ядро не знает про окно и про железо: состояние сюда пишет бэкенд (`engine-render` через
//! winit, `engine::gamepad` через gilrs) или тест (`world.input.set_key(Key::Space, true)`,
//! `world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, 1.0)`).
//!
//! # Действия
//!
//! Игровой код читает не клавиши, а **действия** — именованные кнопки и оси:
//! `input.axis("Horizontal")`, `input.button_down("Jump")`. Что к ним привязано, решает
//! [`Binding`]: клавиша, кнопка мыши, кнопка геймпада или отклонение стика/триггера.
//! Привязки меняются во время игры (экран переназначения управления), см.
//! [`Input::set_button_bindings`], [`Input::axis_bindings_mut`], [`Input::last_pressed_binding`].
//!
//! # Фиксированный шаг
//!
//! «Нажато в этом кадре» (`key_down`, `button_down`, `mouse_button_down`,
//! `gamepad_button_down`…) внутри `on_fixed_update` означает «нажато с прошлого
//! фиксированного шага». Так короткое нажатие не пропадает, даже если в кадре не было ни
//! одного шага физики. Это верно для всех источников.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::math::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    Left, Right, Up, Down,
    Space, Enter, Escape, Tab, Backspace,
    Delete, Insert, Home, End, PageUp, PageDown,
    LShift, RShift, LCtrl, RCtrl, LAlt, RAlt,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
}

impl Key {
    /// Все клавиши — для разбора и перебора (экран переназначения).
    pub const ALL: [Key; 69] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K,
        Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V,
        Key::W, Key::X, Key::Y, Key::Z,
        Key::Digit0, Key::Digit1, Key::Digit2, Key::Digit3, Key::Digit4,
        Key::Digit5, Key::Digit6, Key::Digit7, Key::Digit8, Key::Digit9,
        Key::Left, Key::Right, Key::Up, Key::Down,
        Key::Space, Key::Enter, Key::Escape, Key::Tab, Key::Backspace,
        Key::Delete, Key::Insert, Key::Home, Key::End, Key::PageUp, Key::PageDown,
        Key::LShift, Key::RShift, Key::LCtrl, Key::RCtrl, Key::LAlt, Key::RAlt,
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6,
        Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub const ALL: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];
}

/// Номер геймпада, как его выдал бэкенд (для тестов — любое число). Стабилен, пока
/// геймпад подключён.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GamepadId(pub u32);

/// Кнопки геймпада по расположению (как у Xbox / PlayStation):
/// `South` — A / ✕, `East` — B / ○, `West` — X / □, `North` — Y / △.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    South,
    East,
    West,
    North,
    /// L1 / LB.
    LeftBumper,
    /// R1 / RB.
    RightBumper,
    /// L2 / LT как кнопка (нажат до упора или по порогу бэкенда). Плавное значение — [`GamepadAxis::LeftTrigger`].
    LeftTrigger,
    /// R2 / RT как кнопка.
    RightTrigger,
    Select,
    Start,
    /// Кнопка Xbox / PS / Home.
    Mode,
    /// Нажатие левого стика.
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl GamepadButton {
    pub const ALL: [GamepadButton; 17] = [
        GamepadButton::South, GamepadButton::East, GamepadButton::West, GamepadButton::North,
        GamepadButton::LeftBumper, GamepadButton::RightBumper,
        GamepadButton::LeftTrigger, GamepadButton::RightTrigger,
        GamepadButton::Select, GamepadButton::Start, GamepadButton::Mode,
        GamepadButton::LeftStick, GamepadButton::RightStick,
        GamepadButton::DPadUp, GamepadButton::DPadDown, GamepadButton::DPadLeft, GamepadButton::DPadRight,
    ];
}

/// Аналоговые оси геймпада. Стики: −1..1, **вверх и вправо — плюс**. Триггеры: 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
}

impl GamepadAxis {
    pub const ALL: [GamepadAxis; 6] = [
        GamepadAxis::LeftStickX, GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX, GamepadAxis::RightStickY,
        GamepadAxis::LeftTrigger, GamepadAxis::RightTrigger,
    ];
}

/// Источник сигнала, который можно привязать к действию.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(Key),
    Mouse(MouseButton),
    /// Кнопка геймпада (любого из подключённых).
    Pad(GamepadButton),
    /// Стик или триггер как кнопка: «нажато», когда ось отклонена в сторону `positive`
    /// дальше порога ([`Input::set_axis_press_threshold`], по умолчанию 0.5).
    PadAxis { axis: GamepadAxis, positive: bool },
}

impl Binding {
    /// Строка для файла настроек: `Key:Space`, `Mouse:Left`, `Pad:South`, `PadAxis:LeftStickX+`.
    pub fn to_text(self) -> String {
        match self {
            Binding::Key(k) => format!("Key:{k:?}"),
            Binding::Mouse(m) => format!("Mouse:{m:?}"),
            Binding::Pad(p) => format!("Pad:{p:?}"),
            Binding::PadAxis { axis, positive } => {
                format!("PadAxis:{axis:?}{}", if positive { '+' } else { '-' })
            }
        }
    }

    /// Обратно из [`to_text`](Self::to_text); `None` для незнакомой строки.
    pub fn parse(text: &str) -> Option<Binding> {
        let (kind, rest) = text.split_once(':')?;
        match kind {
            "Key" => Key::ALL.into_iter().find(|k| format!("{k:?}") == rest).map(Binding::Key),
            "Mouse" => MouseButton::ALL
                .into_iter()
                .find(|m| format!("{m:?}") == rest)
                .map(Binding::Mouse),
            "Pad" => GamepadButton::ALL
                .into_iter()
                .find(|p| format!("{p:?}") == rest)
                .map(Binding::Pad),
            "PadAxis" => {
                let positive = match rest.chars().last()? {
                    '+' => true,
                    '-' => false,
                    _ => return None,
                };
                let name = &rest[..rest.len() - 1];
                GamepadAxis::ALL
                    .into_iter()
                    .find(|a| format!("{a:?}") == name)
                    .map(|axis| Binding::PadAxis { axis, positive })
            }
            _ => None,
        }
    }
}

impl From<Key> for Binding {
    fn from(key: Key) -> Self {
        Binding::Key(key)
    }
}

impl From<MouseButton> for Binding {
    fn from(button: MouseButton) -> Self {
        Binding::Mouse(button)
    }
}

impl From<GamepadButton> for Binding {
    fn from(button: GamepadButton) -> Self {
        Binding::Pad(button)
    }
}

/// Аналоговая привязка оси действия к оси геймпада.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnalogBinding {
    pub axis: GamepadAxis,
    /// Перевернуть знак (например, ось Y стика → «вниз — плюс»).
    pub invert: bool,
}

impl AnalogBinding {
    pub fn new(axis: GamepadAxis) -> Self {
        Self { axis, invert: false }
    }

    pub fn inverted(axis: GamepadAxis) -> Self {
        Self { axis, invert: true }
    }
}

/// Мёртвая зона по умолчанию: отклонения стика меньше 0.2 считаются нулём.
pub const DEFAULT_DEAD_ZONE: f32 = 0.2;

/// Привязки оси действия: «минус»-источники, «плюс»-источники и аналоговые оси геймпада.
/// Значение оси: цифровая часть (плюс − минус) + аналоговая (с мёртвой зоной), в пределах −1..1.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisBindings {
    pub negative: Vec<Binding>,
    pub positive: Vec<Binding>,
    pub analog: Vec<AnalogBinding>,
    /// Мёртвая зона аналоговых осей: `|v| <= dead_zone` → 0, дальше значение растягивается
    /// на весь диапазон (без «скачка» на границе зоны).
    pub dead_zone: f32,
}

impl Default for AxisBindings {
    fn default() -> Self {
        Self {
            negative: Vec::new(),
            positive: Vec::new(),
            analog: Vec::new(),
            dead_zone: DEFAULT_DEAD_ZONE,
        }
    }
}

impl AxisBindings {
    /// Пара источников «минус / плюс», например `AxisBindings::keys(Key::A, Key::D)`.
    pub fn pair(negative: impl Into<Binding>, positive: impl Into<Binding>) -> Self {
        Self {
            negative: vec![negative.into()],
            positive: vec![positive.into()],
            ..Self::default()
        }
    }

    pub fn with_analog(mut self, axis: GamepadAxis) -> Self {
        self.analog.push(AnalogBinding::new(axis));
        self
    }

    pub fn with_dead_zone(mut self, dead_zone: f32) -> Self {
        self.dead_zone = dead_zone.clamp(0.0, 0.99);
        self
    }

    pub fn also(mut self, negative: impl Into<Binding>, positive: impl Into<Binding>) -> Self {
        self.negative.push(negative.into());
        self.positive.push(positive.into());
        self
    }
}

/// Мёртвая зона с растяжением: `0` внутри зоны, дальше линейно до `±1`.
pub fn apply_dead_zone(value: f32, dead_zone: f32) -> f32 {
    let magnitude = value.abs();
    if magnitude <= dead_zone {
        0.0
    } else {
        value.signum() * ((magnitude - dead_zone) / (1.0 - dead_zone).max(1e-6)).min(1.0)
    }
}

/// Фаза касания экрана (тачскрин, Android / Web).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    /// Палец коснулся экрана в этом кадре.
    Began,
    /// Палец на экране (двигается или стоит).
    Moved,
    /// Палец отпущен в этом кадре; в следующем касание исчезнет из списка.
    Ended,
}

/// Одно касание экрана.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
    /// Номер пальца: не меняется, пока палец на экране.
    pub id: u64,
    /// Позиция в пикселях окна, (0, 0) — левый верхний угол.
    pub position: Vec2,
    /// Смещение с прошлого кадра.
    pub delta: Vec2,
    pub phase: TouchPhase,
}

/// Что «нажимается» у геймпада: кнопка или направление оси.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PadInput {
    Button(GamepadButton),
    AxisDir(GamepadAxis, bool),
}

#[derive(Clone, Debug)]
struct PadState {
    name: String,
    axes: HashMap<GamepadAxis, f32>,
}

/// Состояние ввода на текущий кадр.
#[derive(Clone, Debug)]
pub struct Input {
    held: HashSet<Key>,
    pressed: HashSet<Key>,
    released: HashSet<Key>,
    mouse_held: HashSet<MouseButton>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_released: HashSet<MouseButton>,
    mouse_position: Vec2,
    mouse_delta: Vec2,
    scroll: Vec2,
    text: String,
    touches: Vec<Touch>,
    screen_size: Vec2,
    axes: HashMap<String, AxisBindings>,
    buttons: HashMap<String, Vec<Binding>>,
    /// То же, что `pressed`/`released`, но копится до ближайшего фиксированного шага:
    /// в кадре может не быть ни одного шага, а нажатие терять нельзя.
    fixed_pressed: HashSet<Key>,
    fixed_released: HashSet<Key>,
    fixed_mouse_pressed: HashSet<MouseButton>,
    fixed_mouse_released: HashSet<MouseButton>,
    /// Идёт `on_fixed_update`: «нажато в этом кадре» читается из накопленного.
    in_fixed_step: bool,
    pads: BTreeMap<GamepadId, PadState>,
    pad_held: HashSet<(GamepadId, PadInput)>,
    pad_pressed: HashSet<(GamepadId, PadInput)>,
    pad_released: HashSet<(GamepadId, PadInput)>,
    fixed_pad_pressed: HashSet<(GamepadId, PadInput)>,
    fixed_pad_released: HashSet<(GamepadId, PadInput)>,
    axis_press_threshold: f32,
    /// Геймпады «не у игры» (в редакторе фокус не на окне Game): кнопки отпущены,
    /// стики в нуле, новые события не доходят.
    pads_suspended: bool,
    /// Игра просит захватить курсор (`set_cursor_captured(true)`).
    cursor_capture_requested: bool,
    /// Хозяин (окно, вкладка браузера, вид Game редактора) курсор действительно захватил.
    cursor_captured: bool,
}

impl Default for Input {
    /// Привязки по умолчанию — как в новом проекте Unity, плюс геймпад:
    /// `Horizontal` (A/D, ←/→, крестовина, левый стик X), `Vertical` (S/W, ↓/↑, крестовина,
    /// левый стик Y), `Jump` (Space, South), `Fire1` (LCtrl, West), `Submit` (Enter, South),
    /// `Cancel` (Escape, East).
    fn default() -> Self {
        let mut input = Self {
            held: HashSet::new(),
            pressed: HashSet::new(),
            released: HashSet::new(),
            mouse_held: HashSet::new(),
            mouse_pressed: HashSet::new(),
            mouse_released: HashSet::new(),
            mouse_position: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            scroll: Vec2::ZERO,
            text: String::new(),
            touches: Vec::new(),
            screen_size: Vec2::new(1280.0, 720.0),
            axes: HashMap::new(),
            buttons: HashMap::new(),
            fixed_pressed: HashSet::new(),
            fixed_released: HashSet::new(),
            fixed_mouse_pressed: HashSet::new(),
            fixed_mouse_released: HashSet::new(),
            in_fixed_step: false,
            pads: BTreeMap::new(),
            pad_held: HashSet::new(),
            pad_pressed: HashSet::new(),
            pad_released: HashSet::new(),
            fixed_pad_pressed: HashSet::new(),
            fixed_pad_released: HashSet::new(),
            axis_press_threshold: 0.5,
            pads_suspended: false,
            cursor_capture_requested: false,
            cursor_captured: false,
        };
        input.set_axis_bindings(
            "Horizontal",
            AxisBindings::pair(Key::A, Key::D)
                .also(Key::Left, Key::Right)
                .also(GamepadButton::DPadLeft, GamepadButton::DPadRight)
                .with_analog(GamepadAxis::LeftStickX),
        );
        input.set_axis_bindings(
            "Vertical",
            AxisBindings::pair(Key::S, Key::W)
                .also(Key::Down, Key::Up)
                .also(GamepadButton::DPadDown, GamepadButton::DPadUp)
                .with_analog(GamepadAxis::LeftStickY),
        );
        input.set_button_bindings("Jump", vec![Key::Space.into(), GamepadButton::South.into()]);
        input.set_button_bindings("Fire1", vec![Key::LCtrl.into(), GamepadButton::West.into()]);
        input.set_button_bindings("Submit", vec![Key::Enter.into(), GamepadButton::South.into()]);
        input.set_button_bindings("Cancel", vec![Key::Escape.into(), GamepadButton::East.into()]);
        input
    }
}

impl Input {
    // ----- чтение: клавиши и мышь -----

    /// Unity: `GetKey` — клавиша удерживается.
    pub fn key(&self, key: Key) -> bool {
        self.held.contains(&key)
    }

    /// Unity: `GetKeyDown` — нажата в этом кадре. В `on_fixed_update` — нажата с
    /// прошлого фиксированного шага, так что там её можно читать напрямую.
    pub fn key_down(&self, key: Key) -> bool {
        if self.in_fixed_step {
            self.fixed_pressed.contains(&key)
        } else {
            self.pressed.contains(&key)
        }
    }

    /// Unity: `GetKeyUp` — отпущена в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn key_up(&self, key: Key) -> bool {
        if self.in_fixed_step {
            self.fixed_released.contains(&key)
        } else {
            self.released.contains(&key)
        }
    }

    pub fn mouse_button(&self, button: MouseButton) -> bool {
        self.mouse_held.contains(&button)
    }

    pub fn mouse_button_down(&self, button: MouseButton) -> bool {
        if self.in_fixed_step {
            self.fixed_mouse_pressed.contains(&button)
        } else {
            self.mouse_pressed.contains(&button)
        }
    }

    /// Кнопка мыши отпущена в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn mouse_button_up(&self, button: MouseButton) -> bool {
        if self.in_fixed_step {
            self.fixed_mouse_released.contains(&button)
        } else {
            self.mouse_released.contains(&button)
        }
    }

    /// Позиция курсора в пикселях окна, (0, 0) — левый верхний угол.
    pub fn mouse_position(&self) -> Vec2 {
        self.mouse_position
    }

    /// Смещение курсора за кадр в пикселях (Unity: оси `Mouse X` / `Mouse Y`).
    pub fn mouse_delta(&self) -> Vec2 {
        self.mouse_delta
    }

    /// Прокрутка колеса за кадр в «строках»: `y > 0` — от себя (Unity: `mouseScrollDelta`).
    pub fn scroll_delta(&self) -> Vec2 {
        self.scroll
    }

    /// Текст, набранный за кадр (с учётом раскладки) — для полей ввода.
    pub fn text_input(&self) -> &str {
        &self.text
    }

    /// Касания экрана в этом кадре (Unity: `Input.touches`).
    pub fn touches(&self) -> &[Touch] {
        &self.touches
    }

    /// Размер окна (или вида Game в редакторе) в пикселях — в тех же единицах,
    /// что `mouse_position` и позиции касаний.
    pub fn screen_size(&self) -> Vec2 {
        self.screen_size
    }

    // ----- чтение: геймпады -----

    /// Подключённые геймпады по возрастанию номера.
    pub fn gamepads(&self) -> Vec<GamepadId> {
        self.pads.keys().copied().collect()
    }

    pub fn gamepad_count(&self) -> usize {
        self.pads.len()
    }

    pub fn is_gamepad_connected(&self, pad: GamepadId) -> bool {
        self.pads.contains_key(&pad)
    }

    /// Название модели, если бэкенд его сообщил.
    pub fn gamepad_name(&self, pad: GamepadId) -> Option<&str> {
        self.pads.get(&pad).map(|p| p.name.as_str())
    }

    /// Кнопка конкретного геймпада удерживается.
    pub fn gamepad_button(&self, pad: GamepadId, button: GamepadButton) -> bool {
        self.pad_held.contains(&(pad, PadInput::Button(button)))
    }

    /// Нажата в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn gamepad_button_down(&self, pad: GamepadId, button: GamepadButton) -> bool {
        self.pad_edge(true, pad, PadInput::Button(button))
    }

    /// Отпущена в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn gamepad_button_up(&self, pad: GamepadId, button: GamepadButton) -> bool {
        self.pad_edge(false, pad, PadInput::Button(button))
    }

    /// Сырое значение оси конкретного геймпада (без мёртвой зоны). Нет геймпада — 0.
    pub fn gamepad_axis(&self, pad: GamepadId, axis: GamepadAxis) -> f32 {
        self.pads
            .get(&pad)
            .and_then(|p| p.axes.get(&axis))
            .copied()
            .unwrap_or(0.0)
    }

    /// Левый стик как вектор с круговой мёртвой зоной (длина ≤ 1; вверх — плюс по Y).
    pub fn gamepad_left_stick(&self, pad: GamepadId, dead_zone: f32) -> Vec2 {
        radial_dead_zone(
            Vec2::new(
                self.gamepad_axis(pad, GamepadAxis::LeftStickX),
                self.gamepad_axis(pad, GamepadAxis::LeftStickY),
            ),
            dead_zone,
        )
    }

    pub fn gamepad_right_stick(&self, pad: GamepadId, dead_zone: f32) -> Vec2 {
        radial_dead_zone(
            Vec2::new(
                self.gamepad_axis(pad, GamepadAxis::RightStickX),
                self.gamepad_axis(pad, GamepadAxis::RightStickY),
            ),
            dead_zone,
        )
    }

    /// Кнопка удерживается на любом из геймпадов.
    pub fn any_gamepad_button(&self, button: GamepadButton) -> bool {
        self.pad_held.iter().any(|(_, i)| *i == PadInput::Button(button))
    }

    /// Нажата в этом кадре на любом из геймпадов.
    pub fn any_gamepad_button_down(&self, button: GamepadButton) -> bool {
        self.pad_pressed_set()
            .iter()
            .any(|(_, i)| *i == PadInput::Button(button))
    }

    /// Значение оси с наибольшим отклонением среди всех геймпадов (сырое).
    pub fn any_gamepad_axis(&self, axis: GamepadAxis) -> f32 {
        self.pads
            .values()
            .filter_map(|p| p.axes.get(&axis).copied())
            .fold(0.0, |best: f32, v| if v.abs() > best.abs() { v } else { best })
    }

    // ----- чтение: действия -----

    /// Unity: `GetAxisRaw` для оси-действия. Цифровые источники дают ровно −1/0/1,
    /// аналоговые — плавное значение с мёртвой зоной; сумма ограничена −1..1.
    /// Неизвестная ось даёт 0 и предупреждение в лог.
    pub fn axis(&self, name: &str) -> f32 {
        let Some(axis) = self.axes.get(name) else {
            log::warn!("input axis `{name}` is not bound; use `input.bind_axis(..)`");
            return 0.0;
        };
        let any = |list: &[Binding]| list.iter().any(|b| self.binding_held(*b));
        let digital = any(&axis.positive) as i32 - any(&axis.negative) as i32;
        let analog: f32 = axis
            .analog
            .iter()
            .map(|a| {
                let sign = if a.invert { -1.0 } else { 1.0 };
                apply_dead_zone(self.any_gamepad_axis(a.axis) * sign, axis.dead_zone)
            })
            .sum();
        (digital as f32 + analog).clamp(-1.0, 1.0)
    }

    /// Unity: `GetButton` — удерживается любой из привязанных источников.
    pub fn button(&self, name: &str) -> bool {
        self.button_bindings_or_warn(name).iter().any(|b| self.binding_held(*b))
    }

    /// Unity: `GetButtonDown` — нажат в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn button_down(&self, name: &str) -> bool {
        self.button_bindings_or_warn(name)
            .iter()
            .any(|b| self.binding_pressed(*b))
    }

    /// Unity: `GetButtonUp`.
    pub fn button_up(&self, name: &str) -> bool {
        self.button_bindings_or_warn(name)
            .iter()
            .any(|b| self.binding_released(*b))
    }

    /// Аналоговая «сила» кнопки 0..=1: цифровые источники дают 0 или 1, ось (триггер)
    /// — своё отклонение. Берётся максимум.
    pub fn button_value(&self, name: &str) -> f32 {
        self.button_bindings_or_warn(name)
            .iter()
            .map(|b| self.binding_value(*b))
            .fold(0.0, f32::max)
    }

    /// Состояние любого источника.
    pub fn binding_held(&self, binding: Binding) -> bool {
        match binding {
            Binding::Key(k) => self.key(k),
            Binding::Mouse(m) => self.mouse_button(m),
            Binding::Pad(p) => self.any_gamepad_button(p),
            Binding::PadAxis { axis, positive } => self
                .pad_held
                .iter()
                .any(|(_, i)| *i == PadInput::AxisDir(axis, positive)),
        }
    }

    /// Нажат в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn binding_pressed(&self, binding: Binding) -> bool {
        match binding {
            Binding::Key(k) => self.key_down(k),
            Binding::Mouse(m) => self.mouse_button_down(m),
            Binding::Pad(p) => self.any_pad_edge(true, PadInput::Button(p)),
            Binding::PadAxis { axis, positive } => {
                self.any_pad_edge(true, PadInput::AxisDir(axis, positive))
            }
        }
    }

    /// Отпущен в этом кадре (в `on_fixed_update` — с прошлого шага).
    pub fn binding_released(&self, binding: Binding) -> bool {
        match binding {
            Binding::Key(k) => self.key_up(k),
            Binding::Mouse(m) => self.mouse_button_up(m),
            Binding::Pad(p) => self.any_pad_edge(false, PadInput::Button(p)),
            Binding::PadAxis { axis, positive } => {
                self.any_pad_edge(false, PadInput::AxisDir(axis, positive))
            }
        }
    }

    /// Сила источника 0..=1.
    pub fn binding_value(&self, binding: Binding) -> f32 {
        match binding {
            Binding::PadAxis { axis, positive } => {
                let v = self.any_gamepad_axis(axis);
                (if positive { v } else { -v }).clamp(0.0, 1.0)
            }
            other => self.binding_held(other) as i32 as f32,
        }
    }

    /// Первый источник, нажатый в этом кадре: клавиша, кнопка мыши, кнопка геймпада или
    /// стик/триггер, отклонённый за порог. Для экрана «нажмите новую кнопку»: вызывайте
    /// каждый кадр, пока не вернёт `Some`. `Escape` и кнопки мыши не исключаются —
    /// решайте сами, что считать отменой.
    pub fn last_pressed_binding(&self) -> Option<Binding> {
        let mut keys: Vec<Key> = self.pressed.iter().copied().collect();
        keys.sort_by_key(|k| Key::ALL.iter().position(|x| x == k));
        if let Some(k) = keys.first() {
            return Some(Binding::Key(*k));
        }
        if let Some(m) = MouseButton::ALL.into_iter().find(|m| self.mouse_pressed.contains(m)) {
            return Some(Binding::Mouse(m));
        }
        let mut pads: Vec<&(GamepadId, PadInput)> = self.pad_pressed.iter().collect();
        pads.sort_by_key(|(id, _)| *id);
        pads.first().map(|(_, input)| match *input {
            PadInput::Button(b) => Binding::Pad(b),
            PadInput::AxisDir(axis, positive) => Binding::PadAxis { axis, positive },
        })
    }

    fn pad_pressed_set(&self) -> &HashSet<(GamepadId, PadInput)> {
        if self.in_fixed_step {
            &self.fixed_pad_pressed
        } else {
            &self.pad_pressed
        }
    }

    fn pad_edge(&self, pressed: bool, pad: GamepadId, input: PadInput) -> bool {
        let set = match (pressed, self.in_fixed_step) {
            (true, false) => &self.pad_pressed,
            (true, true) => &self.fixed_pad_pressed,
            (false, false) => &self.pad_released,
            (false, true) => &self.fixed_pad_released,
        };
        set.contains(&(pad, input))
    }

    fn any_pad_edge(&self, pressed: bool, input: PadInput) -> bool {
        let set = match (pressed, self.in_fixed_step) {
            (true, false) => &self.pad_pressed,
            (true, true) => &self.fixed_pad_pressed,
            (false, false) => &self.pad_released,
            (false, true) => &self.fixed_pad_released,
        };
        set.iter().any(|(_, i)| *i == input)
    }

    fn button_bindings_or_warn(&self, name: &str) -> &[Binding] {
        match self.buttons.get(name) {
            Some(bindings) => bindings,
            None => {
                log::warn!("input button `{name}` is not bound; use `input.bind_button(..)`");
                &[]
            }
        }
    }

    // ----- настройка действий -----

    /// Привязать клавиши к оси (как в Unity Input Manager): любая из `negative` даёт −1,
    /// любая из `positive` — +1. Заменяет только **клавиатурные** привязки этой оси:
    /// геймпад и мышь, если они были привязаны, остаются. Полная замена —
    /// [`set_axis_bindings`](Self::set_axis_bindings).
    pub fn bind_axis(&mut self, name: &str, negative: &[Key], positive: &[Key]) {
        let axis = self.axes.entry(name.to_string()).or_default();
        axis.negative.retain(|b| !matches!(b, Binding::Key(_)));
        axis.positive.retain(|b| !matches!(b, Binding::Key(_)));
        axis.negative.extend(negative.iter().copied().map(Binding::Key));
        axis.positive.extend(positive.iter().copied().map(Binding::Key));
    }

    /// Привязать клавиши к кнопке. Заменяет только клавиатурные привязки; остальные
    /// источники остаются (см. [`bind_axis`](Self::bind_axis)).
    pub fn bind_button(&mut self, name: &str, keys: &[Key]) {
        let list = self.buttons.entry(name.to_string()).or_default();
        list.retain(|b| !matches!(b, Binding::Key(_)));
        list.extend(keys.iter().copied().map(Binding::Key));
    }

    /// Полностью заменить привязки кнопки.
    pub fn set_button_bindings(&mut self, name: &str, bindings: Vec<Binding>) {
        self.buttons.insert(name.to_string(), bindings);
    }

    pub fn add_button_binding(&mut self, name: &str, binding: Binding) {
        let list = self.buttons.entry(name.to_string()).or_default();
        if !list.contains(&binding) {
            list.push(binding);
        }
    }

    /// Убрать один источник. `false`, если его не было.
    pub fn remove_button_binding(&mut self, name: &str, binding: Binding) -> bool {
        match self.buttons.get_mut(name) {
            Some(list) => {
                let before = list.len();
                list.retain(|b| *b != binding);
                list.len() != before
            }
            None => false,
        }
    }

    /// Заменить один источник другим на том же месте (переназначение клавиши).
    /// Если `old` не найден, `new` просто добавляется.
    pub fn rebind_button(&mut self, name: &str, old: Binding, new: Binding) {
        let list = self.buttons.entry(name.to_string()).or_default();
        match list.iter().position(|b| *b == old) {
            Some(i) => list[i] = new,
            None => list.push(new),
        }
        let mut seen = HashSet::new();
        list.retain(|b| seen.insert(*b));
    }

    pub fn button_bindings(&self, name: &str) -> &[Binding] {
        self.buttons.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn has_button(&self, name: &str) -> bool {
        self.buttons.contains_key(name)
    }

    pub fn button_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.buttons.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Полностью заменить привязки оси.
    pub fn set_axis_bindings(&mut self, name: &str, bindings: AxisBindings) {
        self.axes.insert(name.to_string(), bindings);
    }

    pub fn axis_bindings(&self, name: &str) -> Option<&AxisBindings> {
        self.axes.get(name)
    }

    /// Привязки оси для правки на месте; ось создаётся пустой, если её не было.
    pub fn axis_bindings_mut(&mut self, name: &str) -> &mut AxisBindings {
        self.axes.entry(name.to_string()).or_default()
    }

    pub fn has_axis(&self, name: &str) -> bool {
        self.axes.contains_key(name)
    }

    pub fn axis_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.axes.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Удалить кнопку и ось с этим именем.
    pub fn remove_action(&mut self, name: &str) {
        self.buttons.remove(name);
        self.axes.remove(name);
    }

    /// Порог (0..1), с которого отклонённый стик или триггер считается «нажатой кнопкой»
    /// ([`Binding::PadAxis`]). По умолчанию 0.5; отпускание — ниже 70% порога, чтобы
    /// дрожание стика не давало серию нажатий.
    pub fn set_axis_press_threshold(&mut self, threshold: f32) {
        self.axis_press_threshold = threshold.clamp(0.05, 0.99);
    }

    // ----- запись (для бэкенда окна и тестов) -----

    pub fn set_key(&mut self, key: Key, down: bool) {
        if down {
            if self.held.insert(key) {
                self.pressed.insert(key);
                self.fixed_pressed.insert(key);
            }
        } else if self.held.remove(&key) {
            self.released.insert(key);
            self.fixed_released.insert(key);
        }
    }

    pub fn set_mouse_button(&mut self, button: MouseButton, down: bool) {
        if down {
            if self.mouse_held.insert(button) {
                self.mouse_pressed.insert(button);
                self.fixed_mouse_pressed.insert(button);
            }
        } else if self.mouse_held.remove(&button) {
            self.mouse_released.insert(button);
            self.fixed_mouse_released.insert(button);
        }
    }

    /// Новая позиция курсора; смещение за кадр копится в `mouse_delta`. Пока курсор
    /// захвачен ([`Input::is_cursor_captured`]), смещение сюда не идёт: его приносят сырые
    /// события мыши через [`Input::add_mouse_delta`] (иначе движение посчиталось бы дважды).
    pub fn set_mouse_position(&mut self, position: Vec2) {
        if !self.cursor_captured {
            self.mouse_delta += position - self.mouse_position;
        }
        self.mouse_position = position;
    }

    // ----- захват курсора (взгляд мышью в играх от первого лица) -----

    /// Попросить захватить курсор: он прячется и не упирается в край окна, а
    /// [`Input::mouse_delta`] приносит сырое относительное движение мыши. `false` —
    /// отпустить. Это **просьба**: применяет её хозяин игры (окно — в конце кадра, в
    /// браузере — Pointer Lock, редактор — только пока вид Game в фокусе и игра идёт).
    /// Сбывшийся захват видно по [`Input::is_cursor_captured`]. Потеря фокуса,
    /// сворачивание, пауза или остановка в редакторе ([`Input::release_all`]) снимают и
    /// захват, и просьбу: игра узнаёт об этом по `is_cursor_captured() == false` и
    /// просит снова, когда игрок вернётся (например, по клику «Продолжить»).
    pub fn set_cursor_captured(&mut self, captured: bool) {
        self.cursor_capture_requested = captured;
    }

    /// Игра просит захватить курсор (последний вызов [`Input::set_cursor_captured`]).
    pub fn cursor_capture_requested(&self) -> bool {
        self.cursor_capture_requested
    }

    /// Курсор сейчас захвачен хозяином игры.
    pub fn is_cursor_captured(&self) -> bool {
        self.cursor_captured
    }

    /// Для хозяина игры (бэкенд окна, редактор, тесты): захват применён (`true`) или
    /// снят (`false`). Хозяин сравнивает [`Input::cursor_capture_requested`] с этим
    /// состоянием, применяет разницу и сообщает итог сюда. Отказ (платформа не умеет,
    /// вид Game не в фокусе) — просто не ставить `true`.
    pub fn set_cursor_capture_state(&mut self, captured: bool) {
        self.cursor_captured = captured;
    }

    /// Добавить смещение мыши без перемещения курсора (захваченный курсор, сырые события).
    pub fn add_mouse_delta(&mut self, delta: Vec2) {
        self.mouse_delta += delta;
    }

    /// Добавить прокрутку колеса за этот кадр.
    pub fn add_scroll(&mut self, delta: Vec2) {
        self.scroll += delta;
    }

    /// Добавить набранный текст (управляющие символы отбрасываются).
    pub fn add_text(&mut self, text: &str) {
        self.text.extend(text.chars().filter(|c| !c.is_control()));
    }

    /// Обновить касание. Первое касание заодно ведёт себя как левая кнопка мыши,
    /// поэтому интерфейс и игры «под мышь» работают на тачскрине без правок.
    pub fn set_touch(&mut self, id: u64, position: Vec2, phase: TouchPhase) {
        let primary = self.touches.first().is_none_or(|t| t.id == id);
        match self.touches.iter_mut().find(|t| t.id == id) {
            Some(touch) => {
                touch.delta += position - touch.position;
                touch.position = position;
                // «Began» держится до конца кадра, даже если палец уже сдвинулся.
                if !(touch.phase == TouchPhase::Began && phase == TouchPhase::Moved) {
                    touch.phase = phase;
                }
            }
            None => self.touches.push(Touch {
                id,
                position,
                delta: Vec2::ZERO,
                phase,
            }),
        }
        if primary {
            if phase == TouchPhase::Began {
                // Палец появился в новом месте: это не движение мыши.
                self.mouse_position = position;
            } else {
                self.set_mouse_position(position);
            }
            self.set_mouse_button(MouseButton::Left, phase != TouchPhase::Ended);
        }
    }

    /// Размер окна в пикселях. Пишет бэкенд окна и редактор (размер вида Game).
    pub fn set_screen_size(&mut self, size: Vec2) {
        self.screen_size = size.max(Vec2::ONE);
    }

    /// Геймпад подключён. Бэкенд зовёт это при подключении; `set_gamepad_*` для
    /// неизвестного номера подключают его сами (с названием «Gamepad»).
    pub fn connect_gamepad(&mut self, pad: GamepadId, name: &str) {
        self.pads
            .entry(pad)
            .and_modify(|p| p.name = name.to_string())
            .or_insert_with(|| PadState {
                name: name.to_string(),
                axes: HashMap::new(),
            });
    }

    /// Геймпад отключён: все его нажатые кнопки отпускаются (с событием «отпущено»).
    pub fn disconnect_gamepad(&mut self, pad: GamepadId) {
        if self.pads.remove(&pad).is_none() {
            return;
        }
        let held: Vec<_> = self.pad_held.iter().filter(|(id, _)| *id == pad).copied().collect();
        for (_, input) in held {
            self.pad_set(pad, input, false);
        }
    }

    pub fn set_gamepad_button(&mut self, pad: GamepadId, button: GamepadButton, down: bool) {
        self.ensure_pad(pad);
        if self.pads_suspended {
            return;
        }
        self.pad_set(pad, PadInput::Button(button), down);
    }

    /// Отдать геймпады игре или забрать (`true` — забрать). Редактор забирает их, пока
    /// фокус клавиатуры не на окне Game: всё нажатое отпускается (с событием «отпущено»),
    /// стики встают в ноль, а события до возврата не доходят — ничего не «залипает».
    pub fn set_gamepads_suspended(&mut self, suspended: bool) {
        if self.pads_suspended == suspended {
            return;
        }
        if suspended {
            let held: Vec<_> = self.pad_held.iter().copied().collect();
            for (pad, input) in held {
                self.pad_set(pad, input, false);
            }
            for state in self.pads.values_mut() {
                state.axes.clear();
            }
        }
        self.pads_suspended = suspended;
    }

    /// Геймпады забраны у игры ([`Input::set_gamepads_suspended`]).
    pub fn gamepads_suspended(&self) -> bool {
        self.pads_suspended
    }

    /// Значение оси геймпада (стики −1..1, триггеры 0..1; вверх и вправо — плюс).
    pub fn set_gamepad_axis(&mut self, pad: GamepadId, axis: GamepadAxis, value: f32) {
        self.ensure_pad(pad);
        if self.pads_suspended {
            return;
        }
        let value = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
        if let Some(state) = self.pads.get_mut(&pad) {
            state.axes.insert(axis, value);
        }
        let threshold = self.axis_press_threshold;
        for positive in [true, false] {
            let directed = if positive { value } else { -value };
            let held = self.pad_held.contains(&(pad, PadInput::AxisDir(axis, positive)));
            let now = if held {
                directed > threshold * 0.7
            } else {
                directed >= threshold
            };
            self.pad_set(pad, PadInput::AxisDir(axis, positive), now);
        }
    }

    fn ensure_pad(&mut self, pad: GamepadId) {
        if !self.pads.contains_key(&pad) {
            self.connect_gamepad(pad, "Gamepad");
        }
    }

    fn pad_set(&mut self, pad: GamepadId, input: PadInput, down: bool) {
        let key = (pad, input);
        if down {
            if self.pad_held.insert(key) {
                self.pad_pressed.insert(key);
                self.fixed_pad_pressed.insert(key);
            }
        } else if self.pad_held.remove(&key) {
            self.pad_released.insert(key);
            self.fixed_pad_released.insert(key);
        }
    }

    /// Отпустить всё разом — когда окно игры теряет фокус или редактор выходит из Play.
    /// Геймпады остаются подключёнными и сохраняют положение стиков: бэкенд присылает
    /// только изменения.
    pub fn release_all(&mut self) {
        // Курсор отпускается вместе с кнопками; хозяин игры вернёт его системе.
        self.cursor_capture_requested = false;
        self.held.clear();
        self.pressed.clear();
        self.released.clear();
        self.mouse_held.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = Vec2::ZERO;
        self.text.clear();
        self.touches.clear();
        self.fixed_pressed.clear();
        self.fixed_released.clear();
        self.fixed_mouse_pressed.clear();
        self.fixed_mouse_released.clear();
        self.pad_pressed.clear();
        self.pad_released.clear();
        self.fixed_pad_pressed.clear();
        self.fixed_pad_released.clear();
    }

    /// Начало фиксированного шага. Вызывает `App::tick`.
    pub fn begin_fixed_step(&mut self) {
        self.in_fixed_step = true;
    }

    /// Конец фиксированного шага: накопленные нажатия обработаны.
    pub fn end_fixed_step(&mut self) {
        self.in_fixed_step = false;
        self.fixed_pressed.clear();
        self.fixed_released.clear();
        self.fixed_mouse_pressed.clear();
        self.fixed_mouse_released.clear();
        self.fixed_pad_pressed.clear();
        self.fixed_pad_released.clear();
    }

    /// Сбросить «нажато/отпущено в этом кадре». Вызывает `App::tick` в конце кадра.
    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
        self.pad_pressed.clear();
        self.pad_released.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = Vec2::ZERO;
        self.text.clear();
        self.touches.retain(|t| t.phase != TouchPhase::Ended);
        for touch in &mut self.touches {
            touch.delta = Vec2::ZERO;
            touch.phase = TouchPhase::Moved;
        }
    }
}

/// Круговая мёртвая зона для стика: направление сохраняется, длина растягивается на 0..1.
fn radial_dead_zone(v: Vec2, dead_zone: f32) -> Vec2 {
    let length = v.length();
    if length <= dead_zone {
        Vec2::ZERO
    } else {
        v / length * ((length - dead_zone) / (1.0 - dead_zone).max(1e-6)).min(1.0)
    }
}
