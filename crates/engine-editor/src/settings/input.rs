//! Страница Input: карта действий ввода (как Input Manager в Unity).
//!
//! Сверху — список действий (кнопки и оси): выбрать, добавить, удалить. Ниже — выбранное
//! действие: имя, вид, привязки (клавиши, мышь, кнопки геймпада, стики «как кнопка»),
//! для осей — списки «минус» / «плюс», аналоговые оси с инверсией и мёртвая зона.
//! Привязку можно выбрать из меню или «услышать»: Listen ждёт следующее нажатие клавиши,
//! кнопки мыши или геймпада (геймпад — через `world.input`, то есть пока игра запущена).
//!
//! Карта хранится в `project.settings` (раздел `input`) и сразу применяется к миру;
//! удалённые и переименованные действия убираются из `world.input` здесь же.

use egui::{pos2, vec2, Rect, Sense};
use engine_core::input::{Binding, GamepadAxis, GamepadButton, Key, MouseButton, DEFAULT_DEAD_ZONE};
use engine_core::settings::{ActionKind, InputAction, InputSettings};

use super::rows::{hairline, WIDE_W};
use super::{SettingsScope, SettingsUi};
use crate::icons::{self, Icon};
use crate::state::Editor;
use crate::theme;

/// Какой список привязок действия: источники кнопки или «минус» / «плюс» оси.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputBindingList {
    /// Источники нажатия действия-кнопки.
    Bindings,
    /// Источники −1 действия-оси.
    Negative,
    /// Источники +1 действия-оси.
    Positive,
}

/// Ожидание нажатия для привязки (кнопка Listen).
#[derive(Clone, Debug)]
struct Listen {
    action: String,
    list: InputBindingList,
    /// Заменить привязку с этим номером (`None` — добавить новую).
    replace: Option<usize>,
    /// Первый кадр пропускается: в нём пришёл щелчок, который начал ожидание.
    armed: bool,
    /// Модификаторы прошлого кадра (Shift / Ctrl / Alt не приходят событиями клавиш).
    modifiers: egui::Modifiers,
}

/// Состояние страницы между кадрами.
#[derive(Default)]
pub(crate) struct InputPageState {
    /// Имя выбранного действия.
    selected: Option<String>,
    /// Поле «новое действие».
    new_name: String,
    /// Правка имени: (действие, текст поля).
    rename: Option<(String, String)>,
    listen: Option<Listen>,
    /// Привязали кнопку мыши: её отпускание не должно нажать то, что под курсором.
    swallow_release: bool,
}

/// Что сделать со списком привязок после кадра.
enum ListOp {
    Remove(usize),
    Listen(Option<usize>),
    Add(Binding),
    /// Ожидание закончилось: привязка или отмена (`None`).
    Heard(Option<Binding>),
}

/// Что сделать со списком действий после кадра.
enum ActionOp {
    Select(String),
    Remove(String),
    Add(String, ActionKind),
}

impl InputBindingList {
    fn key(self) -> &'static str {
        match self {
            InputBindingList::Bindings => "bindings",
            InputBindingList::Negative => "negative",
            InputBindingList::Positive => "positive",
        }
    }

    fn texts(self, action: &InputAction) -> &Vec<String> {
        match self {
            InputBindingList::Bindings => &action.bindings,
            InputBindingList::Negative => &action.negative,
            InputBindingList::Positive => &action.positive,
        }
    }

    fn texts_mut(self, action: &mut InputAction) -> &mut Vec<String> {
        match self {
            InputBindingList::Bindings => &mut action.bindings,
            InputBindingList::Negative => &mut action.negative,
            InputBindingList::Positive => &mut action.positive,
        }
    }
}

// ---------------------------------------------------------------------------
// Идентификаторы и доступ для тестов
// ---------------------------------------------------------------------------

impl Editor {
    /// Строка действия `name` в списке действий (щелчок выбирает).
    pub fn input_action_row_id(name: &str) -> egui::Id {
        egui::Id::new(("settings_input_action", name.to_string()))
    }

    /// Кнопка удаления действия `name`.
    pub fn input_remove_action_id(name: &str) -> egui::Id {
        egui::Id::new(("settings_input_remove_action", name.to_string()))
    }

    /// Поле имени нового действия.
    pub fn input_new_action_field_id() -> egui::Id {
        egui::Id::new("settings_input_new_action")
    }

    /// Кнопка «Add Button» / «Add Axis» под списком действий.
    pub fn input_add_action_id(kind: ActionKind) -> egui::Id {
        egui::Id::new(("settings_input_add_action", kind == ActionKind::Axis))
    }

    /// Поле имени выбранного действия (переименование — по Enter или уходу из поля).
    pub fn input_rename_field_id() -> egui::Id {
        egui::Id::new("settings_input_rename")
    }

    /// Привязка номер `index` в списке `list` выбранного действия: щелчок ждёт новое
    /// нажатие и заменяет её.
    pub fn input_binding_id(list: InputBindingList, index: usize) -> egui::Id {
        egui::Id::new(("settings_input_binding", list.key(), index))
    }

    /// Кнопка удаления привязки номер `index` в списке `list`.
    pub fn input_remove_binding_id(list: InputBindingList, index: usize) -> egui::Id {
        egui::Id::new(("settings_input_remove_binding", list.key(), index))
    }

    /// Кнопка Listen списка `list`: ждёт нажатие и добавляет привязку.
    pub fn input_listen_id(list: InputBindingList) -> egui::Id {
        egui::Id::new(("settings_input_listen", list.key()))
    }

    /// Кнопка «Revert» выбранного действия (вернуть его привязки по умолчанию).
    pub fn input_revert_action_id() -> egui::Id {
        egui::Id::new("settings_input_revert_action")
    }

    /// Действие, выбранное на странице Input.
    pub fn selected_input_action(&self) -> Option<String> {
        self.settings.input.selected.clone()
    }

    /// Выбрать действие на странице Input (как щелчок по строке).
    pub fn select_input_action(&mut self, name: &str) {
        self.settings.input.selected = Some(name.to_string());
        self.settings.input.listen = None;
    }

    /// Страница Input ждёт нажатие (Listen).
    pub fn is_listening_for_input(&self) -> bool {
        self.settings.input.listen.is_some()
    }
}

// ---------------------------------------------------------------------------
// Названия
// ---------------------------------------------------------------------------

/// «LeftBumper» → «Left Bumper».
fn spaced(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn key_name(key: Key) -> String {
    let debug = format!("{key:?}");
    match key {
        Key::Left | Key::Right | Key::Up | Key::Down => format!("{debug} Arrow"),
        Key::LShift => "Left Shift".into(),
        Key::RShift => "Right Shift".into(),
        Key::LCtrl => "Left Ctrl".into(),
        Key::RCtrl => "Right Ctrl".into(),
        Key::LAlt => "Left Alt".into(),
        Key::RAlt => "Right Alt".into(),
        _ => match debug.strip_prefix("Digit") {
            Some(digit) => digit.to_string(),
            None => spaced(&debug),
        },
    }
}

fn pad_button_name(button: GamepadButton) -> String {
    let face = match button {
        GamepadButton::South => " (A / Cross)",
        GamepadButton::East => " (B / Circle)",
        GamepadButton::West => " (X / Square)",
        GamepadButton::North => " (Y / Triangle)",
        _ => "",
    };
    let name = format!("{button:?}").replace("DPad", "D-Pad ");
    format!("{}{face}", spaced(&name).replace("D- Pad ", "D-Pad ").replace("  ", " "))
}

fn pad_axis_direction(axis: GamepadAxis, positive: bool) -> String {
    let base = spaced(&format!("{axis:?}"));
    match axis {
        GamepadAxis::LeftStickX | GamepadAxis::RightStickX => {
            format!("{} {}", base.trim_end_matches(" X"), if positive { "Right" } else { "Left" })
        }
        GamepadAxis::LeftStickY | GamepadAxis::RightStickY => {
            format!("{} {}", base.trim_end_matches(" Y"), if positive { "Up" } else { "Down" })
        }
        GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger => {
            if positive {
                format!("{base} (pressed)")
            } else {
                format!("{base} (released side)")
            }
        }
    }
}

/// Источник и его название для человека: («Key», «Space»).
pub(crate) fn binding_label(binding: Binding) -> (&'static str, String) {
    match binding {
        Binding::Key(key) => ("Key", key_name(key)),
        Binding::Mouse(button) => ("Mouse", format!("{} Button", spaced(&format!("{button:?}")))),
        Binding::Pad(button) => ("Pad", pad_button_name(button)),
        Binding::PadAxis { axis, positive } => ("Stick", pad_axis_direction(axis, positive)),
    }
}

/// Название строки из файла (незнакомая строка — как есть, с пометкой).
fn text_label(text: &str) -> (&'static str, String, bool) {
    match Binding::parse(text) {
        Some(binding) => {
            let (kind, name) = binding_label(binding);
            (kind, name, true)
        }
        None => ("?", format!("{text} (unknown)"), false),
    }
}

/// Краткое описание привязок действия для списка действий.
fn summary(action: &InputAction) -> String {
    let names = |texts: &[String]| texts.iter().map(|t| text_label(t).1).collect::<Vec<_>>().join(", ");
    match action.kind {
        ActionKind::Button => {
            if action.bindings.is_empty() {
                "no bindings".to_string()
            } else {
                names(&action.bindings)
            }
        }
        ActionKind::Axis => {
            let mut parts = Vec::new();
            if !action.negative.is_empty() || !action.positive.is_empty() {
                parts.push(format!("− {}  + {}", names(&action.negative), names(&action.positive)));
            }
            if !action.analog.is_empty() {
                parts.push(action.analog.iter().map(|a| analog_label(a)).collect::<Vec<_>>().join(", "));
            }
            if parts.is_empty() {
                "no bindings".to_string()
            } else {
                parts.join("  ·  ")
            }
        }
    }
}

/// Разбор аналоговой оси из файла: (ось, инверсия).
fn parse_analog(text: &str) -> Option<(GamepadAxis, bool)> {
    let (invert, name) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    GamepadAxis::ALL.into_iter().find(|a| format!("{a:?}") == name).map(|a| (a, invert))
}

fn analog_text(axis: GamepadAxis, invert: bool) -> String {
    format!("{}{axis:?}", if invert { "-" } else { "" })
}

fn analog_label(text: &str) -> String {
    match parse_analog(text) {
        Some((axis, invert)) => format!("{}{}", spaced(&format!("{axis:?}")), if invert { " (inverted)" } else { "" }),
        None => format!("{text} (unknown)"),
    }
}

/// Проверить имя действия: не пустое и не занятое (кроме самого действия `except`).
fn check_name(map: &InputSettings, name: &str, except: Option<&str>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("An action needs a name".to_string());
    }
    if map.actions.iter().any(|a| a.name == name && Some(a.name.as_str()) != except) {
        return Err(format!("An action named “{name}” already exists: names must be unique"));
    }
    Ok(name.to_string())
}

/// Действие `name` по умолчанию: встроенное или пустое того же вида.
fn default_action(defaults: &InputSettings, name: &str, kind: ActionKind) -> InputAction {
    defaults.find(name).cloned().unwrap_or_else(|| InputAction {
        name: name.to_string(),
        kind,
        dead_zone: DEFAULT_DEAD_ZONE,
        ..InputAction::default()
    })
}

// ---------------------------------------------------------------------------
// Ожидание нажатия
// ---------------------------------------------------------------------------

/// Услышать нажатие этого кадра: `Some(Some(привязка))`, `Some(None)` — Esc (отмена),
/// `None` — ещё ждём.
fn hear(ui: &egui::Ui, listen: &mut Listen, pad: Option<Binding>) -> Option<Option<Binding>> {
    let (events, modifiers) = ui.input(|i| (i.events.clone(), i.modifiers));
    if !listen.armed {
        listen.armed = true;
        listen.modifiers = modifiers;
        return None;
    }
    let mut heard = None;
    for event in &events {
        match event {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => return Some(None),
            egui::Event::Key { key, pressed: true, repeat: false, .. } => {
                if let Some(key) = crate::ui::map_key(*key) {
                    heard = Some(Binding::Key(key));
                    break;
                }
            }
            egui::Event::PointerButton { button, pressed: true, .. } => {
                let button = match button {
                    egui::PointerButton::Primary => MouseButton::Left,
                    egui::PointerButton::Secondary => MouseButton::Right,
                    egui::PointerButton::Middle => MouseButton::Middle,
                    _ => continue,
                };
                heard = Some(Binding::Mouse(button));
                break;
            }
            _ => {}
        }
    }
    // Shift / Ctrl / Alt сами по себе событий клавиш не дают — только смену модификаторов.
    if heard.is_none() {
        let was = listen.modifiers;
        heard = if modifiers.shift && !was.shift {
            Some(Binding::Key(Key::LShift))
        } else if modifiers.ctrl && !was.ctrl {
            Some(Binding::Key(Key::LCtrl))
        } else if modifiers.alt && !was.alt {
            Some(Binding::Key(Key::LAlt))
        } else {
            None
        };
    }
    listen.modifiers = modifiers;
    heard.or(pad).map(Some)
}

/// Держать клавиатуру у ожидающей строки: Tab, стрелки и Esc не уводят фокус, а
/// сочетания клавиш редактора не срабатывают (редактор видит «пользователь печатает»).
fn hold_keyboard(ui: &egui::Ui, id: egui::Id) {
    ui.memory_mut(|m| {
        m.request_focus(id);
        m.set_focus_lock_filter(
            id,
            egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
        );
    });
}

// ---------------------------------------------------------------------------
// Меню выбора привязки
// ---------------------------------------------------------------------------

fn binding_menu(ui: &mut egui::Ui) -> Option<Binding> {
    let mut picked = None;
    let mut keys = |ui: &mut egui::Ui, icon: Icon, title: &str, range: std::ops::Range<usize>| {
        theme::submenu(ui, icon, title, |ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for key in &Key::ALL[range] {
                    if theme::item(&key_name(*key)).show(ui) {
                        picked = Some(Binding::Key(*key));
                    }
                }
            });
        });
    };
    theme::menu_section(ui, "Keyboard");
    keys(ui, Icon::Text, "Letters", 0..26);
    keys(ui, Icon::Text, "Digits", 26..36);
    keys(ui, Icon::Move, "Arrows and editing", 36..51);
    keys(ui, Icon::ArrowUp, "Modifiers", 51..57);
    keys(ui, Icon::List, "Function keys", 57..69);
    theme::menu_separator(ui);
    theme::menu_section(ui, "Mouse");
    for button in MouseButton::ALL {
        let (_, name) = binding_label(Binding::Mouse(button));
        if theme::item(&name).show(ui) {
            picked = Some(Binding::Mouse(button));
        }
    }
    theme::menu_separator(ui);
    theme::menu_section(ui, "Gamepad");
    theme::submenu(ui, Icon::Target, "Buttons", |ui| {
        for button in GamepadButton::ALL {
            if theme::item(&pad_button_name(button)).show(ui) {
                picked = Some(Binding::Pad(button));
            }
        }
    });
    theme::submenu(ui, Icon::Move, "Sticks and triggers", |ui| {
        for axis in GamepadAxis::ALL {
            let directions: &[bool] = match axis {
                GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger => &[true],
                _ => &[false, true],
            };
            for positive in directions {
                if theme::item(&pad_axis_direction(axis, *positive)).show(ui) {
                    picked = Some(Binding::PadAxis { axis, positive: *positive });
                }
            }
        }
    });
    picked
}

// ---------------------------------------------------------------------------
// Списки
// ---------------------------------------------------------------------------

/// Квадратная кнопка-иконка в строке списка (удалить).
fn row_icon_button(ui: &mut egui::Ui, rect: Rect, id: egui::Id, icon: Icon, tip: &str) -> bool {
    let click = ui.interact(rect, id, Sense::click());
    if click.hovered() {
        ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
    }
    icons::paint(ui.painter(), rect.shrink(5.0), icon, if click.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
    click.on_hover_text(tip).clicked()
}

/// Подпись вида источника (Key, Mouse, Pad) — маленький серый тег слева.
fn kind_tag(ui: &egui::Ui, rect: Rect, text: &str, known: bool) -> f32 {
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::caption(), theme::TEXT_MUTED);
    let tag = Rect::from_min_size(pos2(rect.left(), rect.center().y - 8.0), vec2(44.0, 16.0));
    ui.painter().rect_filled(tag, theme::R_S, if known { theme::BG_FIELD } else { theme::mix(theme::BG_FIELD, theme::WARN, 0.35) });
    ui.painter().galley(tag.center() - galley.size() * 0.5, galley, theme::TEXT_MUTED);
    tag.right()
}

/// Список привязок с кнопками Listen и Add. `listen` — ожидание, если оно относится к
/// этому списку; `pad` — нажатие геймпада этого кадра.
fn binding_list(
    ui: &mut egui::Ui,
    list: InputBindingList,
    texts: &[String],
    listen: Option<&mut Listen>,
    pad: Option<Binding>,
) -> Option<ListOp> {
    let mut op = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut listening_index = listen.as_ref().map(|l| l.replace);
    if let Some(l) = listen {
        if let Some(heard) = hear(ui, l, pad) {
            op = Some(ListOp::Heard(heard));
            // Ожидание кончилось: клавиатура снова у редактора.
            listening_index = None;
            ui.memory_mut(|m| {
                if let Some(focused) = m.focused() {
                    m.surrender_focus(focused);
                }
            });
        }
    }
    // Услышанное нажатие (Space, Enter) не должно ещё и «нажать» строку под фокусом.
    let heard = op.is_some();
    let prompt = "Press a key, click a mouse button or use the gamepad…  (Esc cancels)";
    let width = ui.available_width();
    for (index, text) in texts.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(vec2(width, theme::ROW + 2.0), Sense::hover());
        let id = Editor::input_binding_id(list, index);
        let click = ui.interact(Rect::from_min_max(rect.min, pos2(rect.right() - 26.0, rect.bottom())), id, Sense::click());
        let waiting = listening_index == Some(Some(index));
        if waiting {
            ui.painter().rect(rect.shrink2(vec2(0.0, 1.0)), theme::R_S, theme::accent_soft(), egui::Stroke::new(1.0_f32, theme::accent()));
            hold_keyboard(ui, id);
        } else if click.hovered() {
            ui.painter().rect_filled(rect, 0.0, theme::hover_fill());
        }
        let (kind, name, known) = text_label(text);
        let x = kind_tag(ui, rect.shrink2(vec2(theme::SP_S, 0.0)), kind, known);
        let (shown, color) = if waiting { (prompt.to_string(), theme::TEXT_STRONG) } else { (name, if known { theme::TEXT } else { theme::WARN }) };
        let galley = ui.painter().layout(shown, theme::body(), color, (rect.right() - 36.0 - x).max(10.0));
        ui.painter().galley(pos2(x + theme::SP_M, rect.center().y - galley.size().y * 0.5), galley, color);
        if click.on_hover_text("Click, then press the new key, mouse button or gamepad input").clicked() && !heard {
            op = Some(ListOp::Listen(Some(index)));
        }
        let remove = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(20.0, 20.0));
        if row_icon_button(ui, remove, Editor::input_remove_binding_id(list, index), Icon::Close, "Remove this binding") && !heard {
            op = Some(ListOp::Remove(index));
        }
        hairline(ui);
    }
    if texts.is_empty() && listening_index != Some(None) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, theme::ROW), Sense::hover());
        ui.painter().text(
            rect.left_center() + vec2(theme::SP_S, 0.0),
            egui::Align2::LEFT_CENTER,
            "No bindings yet",
            theme::caption(),
            theme::TEXT_DISABLED,
        );
    }
    if listening_index == Some(None) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, theme::ROW + 2.0), Sense::hover());
        let id = Editor::input_listen_id(list).with("waiting");
        let _ = ui.interact(rect, id, Sense::click());
        ui.painter().rect(rect.shrink2(vec2(0.0, 1.0)), theme::R_S, theme::accent_soft(), egui::Stroke::new(1.0_f32, theme::accent()));
        ui.painter().text(rect.left_center() + vec2(theme::SP_M, 0.0), egui::Align2::LEFT_CENTER, prompt, theme::body(), theme::TEXT_STRONG);
        hold_keyboard(ui, id);
    }
    ui.add_space(theme::SP_S);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SP_S;
        let listen_button = theme::icon_text_button(ui, Icon::Target, "Listen");
        let listen_button = ui
            .interact(listen_button.rect, Editor::input_listen_id(list), Sense::click())
            .on_hover_text("Add a binding: press the key, mouse button or gamepad input it should react to");
        if listen_button.clicked() && !heard {
            op = Some(ListOp::Listen(None));
        }
        if let Some(Some(binding)) = theme::dropdown_button(ui, "Add", "Pick a key, mouse button or gamepad input from a list", binding_menu) {
            if !heard {
                op = Some(ListOp::Add(binding));
            }
        }
    });
    op
}

/// Список аналоговых осей: ось, инверсия, удалить; кнопка добавления.
fn analog_list(ui: &mut egui::Ui, analog: &mut Vec<String>) -> bool {
    let before = analog.clone();
    ui.spacing_mut().item_spacing.y = 0.0;
    let width = ui.available_width();
    let mut remove = None;
    for (index, text) in analog.iter_mut().enumerate() {
        let (rect, _) = ui.allocate_exact_size(vec2(width, theme::ROW + 4.0), Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(rect.min + vec2(theme::SP_S, 0.0), pos2(rect.right() - 28.0, rect.bottom())))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = theme::SP_M;
        let (mut axis, mut invert) = parse_analog(text).unwrap_or((GamepadAxis::LeftStickX, false));
        egui::ComboBox::from_id_salt(("settings_input_analog", index))
            .selected_text(spaced(&format!("{axis:?}")))
            .width(WIDE_W * 0.75)
            .show_ui(&mut child, |ui| {
                for option in GamepadAxis::ALL {
                    ui.selectable_value(&mut axis, option, spaced(&format!("{option:?}")));
                }
            });
        child.checkbox(&mut invert, "Invert").on_hover_text("Flip the sign: for example, stick down gives +1");
        let new = analog_text(axis, invert);
        if parse_analog(text).is_some() && *text != new {
            *text = new;
        }
        let x = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(20.0, 20.0));
        if row_icon_button(ui, x, egui::Id::new(("settings_input_remove_analog", index)), Icon::Close, "Remove this axis") {
            remove = Some(index);
        }
        hairline(ui);
    }
    if let Some(index) = remove {
        analog.remove(index);
    }
    ui.add_space(theme::SP_S);
    let add = theme::icon_text_button(ui, Icon::Plus, "Add Axis");
    if add.on_hover_text("Read a gamepad stick or trigger as this axis").clicked() {
        let unused = GamepadAxis::ALL
            .into_iter()
            .find(|a| !analog.iter().any(|t| parse_analog(t).is_some_and(|(b, _)| b == *a)))
            .unwrap_or(GamepadAxis::LeftStickX);
        analog.push(analog_text(unused, false));
    }
    *analog != before
}

// ---------------------------------------------------------------------------
// Страница
// ---------------------------------------------------------------------------

pub(super) fn input(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = InputSettings::default();
    let mut map = editor.project_draft_mut().input.clone();
    let before = map.clone();
    // Действия, которые надо убрать из `world.input` (удалённые и старые имена).
    let mut gone: Vec<String> = Vec::new();
    let mut error: Option<String> = None;

    let state = &mut editor.settings.input;
    let selected = state
        .selected
        .clone()
        .filter(|name| map.find(name).is_some())
        .or_else(|| map.actions.first().map(|a| a.name.clone()));
    state.selected = selected.clone();
    if state.listen.as_ref().is_some_and(|l| Some(&l.action) != selected.as_ref()) {
        state.listen = None;
    }
    // Нажатие геймпада видно только через `world.input` (пока игра запущена).
    let pad = editor
        .app
        .world
        .input
        .last_pressed_binding()
        .filter(|b| matches!(b, Binding::Pad(_) | Binding::PadAxis { .. }));

    // --- Список действий ---
    s.group("Actions");
    let state = &mut editor.settings.input;
    let row = s.block(
        "Action list",
        "Read them in code: ctx.input().button(\"Jump\"), ctx.input().axis(\"Horizontal\"). Click an action to edit it.",
        map != defaults,
        |ui| {
            // Только что привязали кнопку мыши: щелчки игнорируются, пока её не отпустят.
            if state.swallow_release && !ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
                state.swallow_release = false;
            }
            let mut op = None;
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            for action in &map.actions {
                let (rect, _) = ui.allocate_exact_size(vec2(width, theme::ROW + 4.0), Sense::hover());
                let click = ui.interact(Rect::from_min_max(rect.min, pos2(rect.right() - 26.0, rect.bottom())), Editor::input_action_row_id(&action.name), Sense::click());
                let is_selected = selected.as_deref() == Some(action.name.as_str());
                theme::list_row(ui.painter(), rect, is_selected, true, click.hovered());
                let name = ui.painter().layout_no_wrap(action.name.clone(), theme::strong(theme::FONT_BODY), theme::TEXT_STRONG);
                let name_w = name.size().x.max(110.0);
                ui.painter().galley(pos2(rect.left() + theme::SP_M, rect.center().y - name.size().y * 0.5), name, theme::TEXT_STRONG);
                let kind = match action.kind {
                    ActionKind::Button => "Button",
                    ActionKind::Axis => "Axis",
                };
                let tag_x = rect.left() + theme::SP_M + name_w + theme::SP_M;
                kind_tag(ui, Rect::from_min_max(pos2(tag_x, rect.top()), rect.max), kind, true);
                let text_x = tag_x + 44.0 + theme::SP_M;
                // Одна строка с многоточием.
                let mut job = egui::text::LayoutJob::simple_singleline(summary(action), theme::caption(), theme::TEXT_MUTED);
                job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - 34.0 - text_x).max(10.0));
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(pos2(text_x, rect.center().y - galley.size().y * 0.5), galley, theme::TEXT_MUTED);
                if click.clicked() {
                    op = Some(ActionOp::Select(action.name.clone()));
                }
                let remove = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(20.0, 20.0));
                if row_icon_button(ui, remove, Editor::input_remove_action_id(&action.name), Icon::Trash, "Remove this action") {
                    op = Some(ActionOp::Remove(action.name.clone()));
                }
                hairline(ui);
            }
            if map.actions.is_empty() {
                theme::empty_state(ui, Icon::List, "No input actions", "Add one below, or press Reset page for the built-in ones.");
            }
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::SP_S;
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.new_name)
                        .id(Editor::input_new_action_field_id())
                        .hint_text(egui::RichText::new("New action name").color(theme::TEXT_DISABLED))
                        .vertical_align(egui::Align::Center)
                        .min_size(vec2(0.0, theme::FIELD_H))
                        .desired_width(WIDE_W),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let named = !state.new_name.trim().is_empty();
                for (kind, label, tip) in [
                    (ActionKind::Button, "Add Button", "A button action: pressed or not (Jump, Fire)"),
                    (ActionKind::Axis, "Add Axis", "An axis action from −1 to 1 (Horizontal, Throttle)"),
                ] {
                    let button = ui.add_enabled_ui(named, |ui| theme::icon_text_button(ui, Icon::Plus, label)).inner;
                    let button = ui.interact(button.rect, Editor::input_add_action_id(kind), Sense::click()).on_hover_text(tip);
                    let pressed = button.clicked() || (enter && kind == ActionKind::Button);
                    if pressed && named {
                        op = Some(ActionOp::Add(state.new_name.clone(), kind));
                    }
                }
            });
            op
        },
    );
    if row.reset {
        gone.extend(map.actions.iter().map(|a| a.name.clone()));
        map = defaults.clone();
        if row.inner.is_none() {
            // Проход Reset: страница целиком возвращается к встроенным действиям.
            finish(editor, map, &before, gone, None);
            return;
        }
    }
    let swallow = editor.settings.input.swallow_release;
    match row.inner.flatten() {
        Some(ActionOp::Select(name)) if !swallow => editor.select_input_action(&name),
        Some(ActionOp::Remove(name)) if !swallow => {
            map.actions.retain(|a| a.name != name);
            gone.push(name);
        }
        Some(ActionOp::Add(name, kind)) => match check_name(&map, &name, None) {
            Ok(name) => {
                map.actions.push(InputAction { name: name.clone(), kind, dead_zone: DEFAULT_DEAD_ZONE, ..InputAction::default() });
                editor.settings.input.new_name.clear();
                editor.select_input_action(&name);
            }
            Err(e) => error = Some(e),
        },
        _ => {}
    }

    // Действия, которые задаёт только код игры (их нет в карте).
    let mut code_only: Vec<String> = editor
        .app
        .world
        .input
        .button_names()
        .into_iter()
        .chain(editor.app.world.input.axis_names())
        .filter(|n| map.find(n).is_none() && !gone.iter().any(|g| g == n))
        .map(str::to_string)
        .collect();
    code_only.sort();
    code_only.dedup();
    if !code_only.is_empty() {
        s.note(&format!(
            "Defined only in the game's code: {}. Code that binds an action after start-up wins over this map.",
            code_only.join(", ")
        ));
    }

    // --- Выбранное действие ---
    let Some(name) = editor.settings.input.selected.clone().filter(|n| map.find(n).is_some()) else {
        finish(editor, map, &before, gone, error);
        return;
    };
    let index = map.actions.iter().position(|a| a.name == name).expect("checked above");
    let kind = map.actions[index].kind;
    let action_default = default_action(&defaults, &name, kind);

    s.group(&format!("{} “{name}”", if kind == ActionKind::Axis { "Axis" } else { "Button" }));
    // Имя.
    let state = &mut editor.settings.input;
    let shown = match &state.rename {
        Some((action, text)) if *action == name => text.clone(),
        _ => name.clone(),
    };
    let row = s.row("Name", "What code asks for. Must be unique; renaming keeps the bindings.", false, |ui| {
        let mut text = shown.clone();
        let edit = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id(Editor::input_rename_field_id())
                .vertical_align(egui::Align::Center)
                .min_size(vec2(0.0, theme::FIELD_H))
                .desired_width(WIDE_W),
        );
        if edit.changed() {
            state.rename = Some((name.clone(), text.clone()));
        }
        edit.lost_focus().then_some(text)
    });
    if let Some(Some(text)) = row.inner {
        editor.settings.input.rename = None;
        if text.trim() != name {
            match check_name(&map, &text, Some(&name)) {
                Ok(new) => {
                    map.actions[index].name = new.clone();
                    gone.push(name.clone());
                    editor.settings.input.selected = Some(new);
                    finish(editor, map, &before, gone, error);
                    return;
                }
                Err(e) => error = Some(e),
            }
        }
    }

    // Вид.
    let row = s.row("Type", "Button: pressed or not. Axis: a value from −1 to 1 from two sides and gamepad sticks.", kind != action_default.kind, |ui| {
        let mut value = kind;
        theme::segmented(
            ui,
            &mut value,
            &[
                (ActionKind::Button, "Button", "Pressed or not: input.button(name)"),
                (ActionKind::Axis, "Axis", "From −1 to 1: input.axis(name)"),
            ],
        )
        .then_some(value)
    });
    let new_kind = if row.reset { Some(action_default.kind) } else { row.inner.flatten() };
    if let Some(new_kind) = new_kind.filter(|k| *k != kind) {
        let action = &mut map.actions[index];
        match new_kind {
            ActionKind::Axis => action.positive = std::mem::take(&mut action.bindings),
            ActionKind::Button => {
                let mut all = std::mem::take(&mut action.negative);
                all.append(&mut action.positive);
                all.dedup();
                action.bindings = all;
                action.analog.clear();
            }
        }
        action.kind = new_kind;
        editor.settings.input.listen = None;
    }
    let kind = map.actions[index].kind;

    // Привязки.
    let lists: &[(InputBindingList, &str, &str)] = match kind {
        ActionKind::Button => &[(InputBindingList::Bindings, "Bindings", "Any of these presses the button.")],
        ActionKind::Axis => &[
            (InputBindingList::Negative, "Negative", "Any of these gives −1 (left, down, back)."),
            (InputBindingList::Positive, "Positive", "Any of these gives +1 (right, up, forward)."),
        ],
    };
    for (list, label, description) in lists {
        let texts = list.texts(&map.actions[index]).clone();
        let default_texts = list.texts(&action_default).clone();
        let state = &mut editor.settings.input;
        let listen = state.listen.as_mut().filter(|l| l.list == *list && l.action == name);
        let row = s.block(label, description, texts != default_texts, |ui| binding_list(ui, *list, &texts, listen, pad));
        if row.reset {
            *list.texts_mut(&mut map.actions[index]) = default_texts.clone();
        }
        let state = &mut editor.settings.input;
        match row.inner.flatten() {
            Some(ListOp::Remove(i)) if !state.swallow_release => {
                list.texts_mut(&mut map.actions[index]).remove(i);
                state.listen = None;
            }
            Some(ListOp::Listen(replace)) if !state.swallow_release => {
                state.listen = Some(Listen { action: name.clone(), list: *list, replace, armed: false, modifiers: egui::Modifiers::NONE });
            }
            Some(ListOp::Add(binding)) => {
                let texts = list.texts_mut(&mut map.actions[index]);
                if !texts.contains(&binding.to_text()) {
                    texts.push(binding.to_text());
                }
            }
            Some(ListOp::Heard(heard)) => {
                let replace = state.listen.take().and_then(|l| l.replace);
                if let Some(binding) = heard {
                    state.swallow_release = matches!(binding, Binding::Mouse(_));
                    put_binding(list.texts_mut(&mut map.actions[index]), replace, binding);
                }
            }
            _ => {}
        }
    }

    if kind == ActionKind::Axis {
        let row = s.block(
            "Analog axes",
            "Gamepad sticks and triggers read smoothly; added to the keys above and kept within −1…1.",
            map.actions[index].analog != action_default.analog,
            |ui| {
                let mut analog = map.actions[index].analog.clone();
                analog_list(ui, &mut analog).then_some(analog)
            },
        );
        if row.reset {
            map.actions[index].analog = action_default.analog.clone();
        } else if let Some(Some(analog)) = row.inner {
            map.actions[index].analog = analog;
        }
        s.number(
            "Dead zone",
            "Stick movement smaller than this counts as zero; the rest is stretched to the full range.",
            &mut map.actions[index].dead_zone,
            action_default.dead_zone,
            0.0..=0.95,
            0.01,
            "",
        );
    }

    let differs = map.actions[index] != action_default;
    let built_in = defaults.find(&name).is_some();
    let row = s.row(
        "Revert action",
        if built_in { "Give this built-in action its default bindings back." } else { "Remove every binding of this action." },
        false,
        |ui| {
            let button = ui.add_enabled_ui(differs, |ui| theme::button(ui, if built_in { "Revert to default" } else { "Clear bindings" })).inner;
            ui.interact(button.rect, Editor::input_revert_action_id(), Sense::click()).clicked() && differs
        },
    );
    if row.inner == Some(true) {
        map.actions[index] = action_default;
        editor.settings.input.listen = None;
    }

    finish(editor, map, &before, gone, error);
}

/// Положить услышанную привязку: заменить номер `replace` или добавить (без повторов).
fn put_binding(texts: &mut Vec<String>, replace: Option<usize>, binding: Binding) {
    let text = binding.to_text();
    match replace {
        Some(i) if i < texts.len() => {
            if texts.iter().enumerate().any(|(j, t)| j != i && *t == text) {
                texts.remove(i);
            } else {
                texts[i] = text;
            }
        }
        _ => {
            if !texts.contains(&text) {
                texts.push(text);
            }
        }
    }
}

/// Записать карту в черновик, убрать исчезнувшие действия из мира, показать ошибку.
fn finish(editor: &mut Editor, map: InputSettings, before: &InputSettings, gone: Vec<String>, error: Option<String>) {
    for name in gone {
        if map.find(&name).is_none() {
            editor.app.world.input.remove_action(&name);
        }
    }
    if map != *before {
        editor.project_draft_mut().input = map;
    }
    if let Some(error) = error {
        editor.settings.status[SettingsScope::Project.index()] = Some((error, true));
    }
}
