//! Реестр команд редактора и их горячих клавиш.
//!
//! Каждая команда — строка таблицы [`COMMANDS`]: постоянное имя (`save`, `tool-move`),
//! заголовок, раздел и сочетание по умолчанию. Пользователь переназначает сочетания на
//! странице Preferences → Shortcuts; переназначения хранятся в настройках редактора
//! ([`crate::Preferences::shortcuts`], строки `shortcut.<имя>: Ctrl+Shift+S`).
//! Обработка клавиш ([`Editor::dispatch_shortcuts`]), подписи в меню и подсказки
//! кнопок берут сочетание только отсюда, поэтому показывают *текущее* назначение.

use std::collections::BTreeMap;

use egui::{Key, Modifiers};

use crate::dock::{LayoutAction, Panel};
use crate::state::{Action, Editor, Mode, Tool, ViewTab};

/// Сочетание клавиш: клавиша и модификаторы (Ctrl — это и Cmd на macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyBinding {
    /// Клавиша без модификаторов.
    pub const fn key(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: false }
    }

    /// Ctrl + клавиша.
    pub const fn ctrl(key: Key) -> Self {
        Self { key, ctrl: true, shift: false, alt: false }
    }

    /// Ctrl + Shift + клавиша.
    pub const fn ctrl_shift(key: Key) -> Self {
        Self { key, ctrl: true, shift: true, alt: false }
    }

    const fn with(key: Key, ctrl: bool, shift: bool, alt: bool) -> Self {
        Self { key, ctrl, shift, alt }
    }

    /// Сочетание нажатия `key` с модификаторами `modifiers` (как его видит egui).
    pub fn from_event(key: Key, modifiers: Modifiers) -> Self {
        Self { key, ctrl: modifiers.command || modifiers.ctrl, shift: modifiers.shift, alt: modifiers.alt }
    }

    /// Модификаторы в виде egui (Ctrl — `command`, как `Modifiers::COMMAND`).
    pub fn modifiers(&self) -> Modifiers {
        Modifiers { alt: self.alt, ctrl: false, shift: self.shift, mac_cmd: false, command: self.ctrl }
    }

    fn modifier_count(&self) -> usize {
        usize::from(self.ctrl) + usize::from(self.shift) + usize::from(self.alt)
    }

    /// Подпись для меню и подсказок: `Ctrl+Shift+S`, `Del`, `Ctrl+,`.
    pub fn text(&self) -> String {
        self.compose(key_label(self.key))
    }

    /// Запись в файл настроек: только имена клавиш, без `,` и `:` (`Ctrl+Comma`).
    pub fn storage(&self) -> String {
        self.compose(self.key.name())
    }

    fn compose(&self, key: &str) -> String {
        let mut text = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.shift, "Shift+"), (self.alt, "Alt+")] {
            if on {
                text.push_str(name);
            }
        }
        text.push_str(key);
        text
    }

    /// Разобрать `Ctrl+Shift+S`, `Alt+Shift+N`, `Del`, `Ctrl+,` или `Ctrl+Comma`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut rest = text.trim();
        let mut binding = Self::key(Key::Space);
        loop {
            let lower = rest.to_ascii_lowercase();
            let prefix = ["ctrl+", "cmd+", "command+", "shift+", "alt+", "option+"]
                .into_iter()
                .find(|p| lower.starts_with(p) && rest.len() > p.len());
            let Some(prefix) = prefix else { break };
            match prefix {
                "shift+" => binding.shift = true,
                "alt+" | "option+" => binding.alt = true,
                _ => binding.ctrl = true,
            }
            rest = &rest[prefix.len()..];
        }
        binding.key = parse_key(rest)?;
        Some(binding)
    }

    /// Нажатие совпадает точно (Shift и Alt — как в сочетании).
    fn matches_exact(&self, modifiers: Modifiers) -> bool {
        modifiers.matches_exact(self.modifiers())
    }

    /// Нажатие совпадает «логически»: лишние Shift / Alt не мешают (как `consume_key`).
    fn matches_logically(&self, modifiers: Modifiers) -> bool {
        modifiers.matches_logically(self.modifiers())
    }
}

impl std::fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

/// Короткое имя клавиши для подписи.
fn key_label(key: Key) -> &'static str {
    match key {
        Key::Delete => "Del",
        Key::Escape => "Esc",
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Minus => key.name(),
        _ => key.symbol_or_name(),
    }
}

fn parse_key(text: &str) -> Option<Key> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match text.to_ascii_lowercase().as_str() {
        "del" => return Some(Key::Delete),
        "esc" => return Some(Key::Escape),
        "-" => return Some(Key::Minus),
        _ => {}
    }
    if let Some(key) = Key::from_name(text) {
        return Some(key);
    }
    let upper = text.to_ascii_uppercase();
    if let Some(key) = Key::from_name(&upper) {
        return Some(key);
    }
    Key::ALL.iter().copied().find(|k| k.symbol_or_name() == text || k.name().eq_ignore_ascii_case(text))
}

/// Когда команда слушает клавиатуру.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum When {
    /// Всегда, даже когда печатают в поле (Ctrl+S, Ctrl+P).
    Always,
    /// Когда не печатают в текстовом поле (Ctrl+Z нужен самому полю).
    NotTyping,
    /// Ещё и окно Project под курсором не забрало клавиши файлового менеджера.
    Scene,
    /// Ещё и только в режиме Edit: одиночные буквы в Play принадлежат игре.
    EditMode,
    /// Обрабатывает док (окно под курсором) — см. [`Editor::consume_command`].
    Dock,
}

/// Команда редактора.
pub struct CommandDef {
    /// Постоянное имя: ключ в файле настроек и в тестах.
    pub id: &'static str,
    /// Заголовок в списке Shortcuts.
    pub title: &'static str,
    /// Раздел списка (File, Edit, Play, Tools, Window…).
    pub category: &'static str,
    /// Сочетание по умолчанию (`None` — команда без сочетания, его можно назначить).
    pub default: Option<KeyBinding>,
    pub(crate) when: When,
}

const fn cmd(id: &'static str, title: &'static str, category: &'static str, default: Option<KeyBinding>, when: When) -> CommandDef {
    CommandDef { id, title, category, default, when }
}

const N: Key = Key::N; // «N» встречается в трёх сочетаниях ниже

/// Все команды редактора с горячими клавишами, в порядке страницы Shortcuts.
pub const COMMANDS: &[CommandDef] = &[
    // --- File ---
    cmd("new-scene", "New Scene", "File", Some(KeyBinding::ctrl(N)), When::Always),
    cmd("save", "Save", "File", Some(KeyBinding::ctrl(Key::S)), When::Always),
    cmd("save-as", "Save As…", "File", Some(KeyBinding::ctrl_shift(Key::S)), When::Always),
    cmd("preferences", "Preferences…", "File", Some(KeyBinding::ctrl(Key::Comma)), When::Always),
    cmd("project-settings", "Project Settings…", "File", None, When::Always),
    cmd("quit", "Quit", "File", Some(KeyBinding::ctrl(Key::Q)), When::Always),
    // --- Edit ---
    cmd("undo", "Undo", "Edit", Some(KeyBinding::ctrl(Key::Z)), When::NotTyping),
    cmd("redo", "Redo", "Edit", Some(KeyBinding::ctrl(Key::Y)), When::NotTyping),
    cmd("redo-alt", "Redo (second shortcut)", "Edit", Some(KeyBinding::ctrl_shift(Key::Z)), When::NotTyping),
    cmd("duplicate", "Duplicate", "Edit", Some(KeyBinding::ctrl(Key::D)), When::Scene),
    cmd("rename", "Rename", "Edit", Some(KeyBinding::key(Key::F2)), When::EditMode),
    cmd("delete", "Delete", "Edit", Some(KeyBinding::key(Key::Delete)), When::EditMode),
    cmd("frame-selected", "Frame Selected", "Edit", Some(KeyBinding::key(Key::F)), When::EditMode),
    cmd("select-none", "Select None", "Edit", Some(KeyBinding::key(Key::Escape)), When::EditMode),
    // --- GameObject ---
    cmd("create-empty", "Create Empty", "GameObject", Some(KeyBinding::ctrl_shift(N)), When::Always),
    cmd("create-empty-child", "Create Empty Child", "GameObject", Some(KeyBinding::with(N, false, true, true)), When::Always),
    // --- Play ---
    cmd("play", "Play / Stop", "Play", Some(KeyBinding::ctrl(Key::P)), When::Always),
    cmd("pause", "Pause / Resume", "Play", Some(KeyBinding::ctrl_shift(Key::P)), When::Always),
    cmd("step", "Step", "Play", Some(KeyBinding::with(Key::P, true, false, true)), When::Always),
    // --- Tools ---
    cmd("tool-move", "Move Tool", "Tools", Some(KeyBinding::key(Key::W)), When::EditMode),
    cmd("tool-rotate", "Rotate Tool", "Tools", Some(KeyBinding::key(Key::E)), When::EditMode),
    cmd("tool-scale", "Scale Tool", "Tools", Some(KeyBinding::key(Key::R)), When::EditMode),
    cmd("toggle-snap", "Snap to Grid", "Tools", None, When::EditMode),
    cmd("toggle-local", "Local / World Axes", "Tools", None, When::EditMode),
    // --- View ---
    cmd("view-3d", "2D / 3D View", "View", None, When::Scene),
    cmd("view-grid", "Show Grid", "View", None, When::Scene),
    cmd("view-gizmos", "Show Gizmos", "View", None, When::Scene),
    // --- Assets ---
    cmd("refresh", "Refresh Assets", "Assets", Some(KeyBinding::ctrl(Key::R)), When::Always),
    cmd("build-code", "Build Code", "Assets", Some(KeyBinding::ctrl(Key::B)), When::Always),
    // --- Window ---
    cmd("window-scene", "Scene", "Window", Some(KeyBinding::ctrl(Key::Num1)), When::Always),
    cmd("window-game", "Game", "Window", Some(KeyBinding::ctrl(Key::Num2)), When::Always),
    cmd("window-code", "Code", "Window", Some(KeyBinding::ctrl(Key::Num3)), When::Always),
    cmd("window-hierarchy", "Hierarchy", "Window", Some(KeyBinding::ctrl(Key::Num4)), When::Always),
    cmd("window-project", "Project", "Window", Some(KeyBinding::ctrl(Key::Num5)), When::Always),
    cmd("window-console", "Console", "Window", Some(KeyBinding::ctrl(Key::Num6)), When::Always),
    cmd("window-profiler", "Profiler", "Window", Some(KeyBinding::ctrl(Key::Num7)), When::Always),
    cmd("window-inspector", "Inspector", "Window", Some(KeyBinding::ctrl(Key::Num8)), When::Always),
    cmd("maximize-panel", "Maximize / Restore Window", "Window", Some(KeyBinding::with(Key::Space, false, true, false)), When::Dock),
];

/// Команда по имени.
pub fn command(id: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Окно, которое открывает команда `window-*`.
pub(crate) fn window_command(panel: Panel) -> &'static str {
    match panel {
        Panel::Scene => "window-scene",
        Panel::Game => "window-game",
        Panel::Code => "window-code",
        Panel::Hierarchy => "window-hierarchy",
        Panel::Project => "window-project",
        Panel::Console => "window-console",
        Panel::Profiler => "window-profiler",
        Panel::Inspector => "window-inspector",
    }
}

/// Назначенные пользователем сочетания (только отличия от умолчаний).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    /// Имя команды → сочетание (`None` — сочетание снято).
    overrides: BTreeMap<String, Option<KeyBinding>>,
}

impl Keymap {
    /// Текущее сочетание команды `id`.
    pub fn binding(&self, id: &str) -> Option<KeyBinding> {
        match self.overrides.get(id) {
            Some(binding) => *binding,
            None => command(id).and_then(|c| c.default),
        }
    }

    /// Подпись текущего сочетания (пусто — без сочетания).
    pub fn text(&self, id: &str) -> String {
        self.binding(id).map(|b| b.text()).unwrap_or_default()
    }

    /// `text` в скобках после подсказки: `Play (Ctrl+P)`; без сочетания — только `text`.
    pub fn tip(&self, text: &str, id: &str) -> String {
        match self.binding(id) {
            Some(binding) => format!("{text} ({})", binding.text()),
            None => text.to_string(),
        }
    }

    /// Назначить сочетание (`None` — снять). Совпадение с умолчанием убирает запись.
    pub fn set(&mut self, id: &str, binding: Option<KeyBinding>) {
        let Some(def) = command(id) else { return };
        if def.default == binding {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.to_string(), binding);
        }
    }

    /// Вернуть сочетание по умолчанию.
    pub fn reset(&mut self, id: &str) {
        self.overrides.remove(id);
    }

    pub fn reset_all(&mut self) {
        self.overrides.clear();
    }

    /// Сочетание команды не меняли.
    pub fn is_default(&self, id: &str) -> bool {
        !self.overrides.contains_key(id)
    }

    /// Другая команда, у которой уже есть сочетание `binding`.
    pub fn conflict(&self, id: &str, binding: KeyBinding) -> Option<&'static CommandDef> {
        COMMANDS.iter().find(|c| c.id != id && self.binding(c.id) == Some(binding))
    }

    /// Строки файла настроек: `    shortcut.save: Ctrl+Shift+S,` (`none` — снято).
    pub(crate) fn to_text(&self) -> String {
        let mut text = String::new();
        for (id, binding) in &self.overrides {
            let value = binding.map_or_else(|| "none".to_string(), |b| b.storage());
            text.push_str(&format!("    shortcut.{id}: {value},\n"));
        }
        text
    }

    /// Одна запись файла настроек (`id` без `shortcut.`). Незнакомое пропускается.
    pub(crate) fn parse_entry(&mut self, id: &str, value: &str) {
        if command(id).is_none() {
            return;
        }
        let value = value.trim();
        if value.eq_ignore_ascii_case("none") || value.is_empty() {
            self.set(id, None);
        } else if let Some(binding) = KeyBinding::parse(value) {
            self.set(id, Some(binding));
        }
    }
}

/// Текущие сочетания в данных контекста egui: подписи в меню модулей, у которых нет
/// доступа к настройкам (док, иерархия, вид Scene).
pub(crate) fn publish(ctx: &egui::Context, keymap: &Keymap) {
    let id = egui::Id::new("piska_keymap");
    let same = ctx.data(|d| d.get_temp::<Keymap>(id)).is_some_and(|k| k == *keymap);
    if !same {
        ctx.data_mut(|d| d.insert_temp(id, keymap.clone()));
    }
}

/// Подпись текущего сочетания команды `id` для меню (пусто — без сочетания).
pub(crate) fn label(ctx: &egui::Context, id: &str) -> String {
    let keymap = ctx.data(|d| d.get_temp::<Keymap>(egui::Id::new("piska_keymap"))).unwrap_or_default();
    keymap.text(id)
}

/// Состояние страницы Shortcuts: какая команда ждёт нажатия, последняя сработавшая.
#[derive(Default)]
pub(crate) struct ShortcutState {
    /// Команда, которой назначают сочетание: следующее нажатие станет её сочетанием.
    pub listening: Option<&'static str>,
    /// Последняя команда, запущенная клавишами (для тестов и строки состояния).
    pub last: Option<&'static str>,
}

impl Editor {
    /// Текущее сочетание команды `id` (см. [`COMMANDS`]).
    pub fn shortcut(&self, id: &str) -> Option<KeyBinding> {
        self.prefs.shortcuts.binding(id)
    }

    /// Подпись текущего сочетания команды `id` — то, что показывают меню.
    pub fn shortcut_text(&self, id: &str) -> String {
        self.prefs.shortcuts.text(id)
    }

    /// Назначить команде `id` сочетание (`None` — снять). Конфликты не проверяются:
    /// это делает страница Shortcuts.
    pub fn set_shortcut(&mut self, id: &str, binding: Option<KeyBinding>) {
        self.prefs.shortcuts.set(id, binding);
    }

    /// Команда, которую последней запустили клавиши.
    pub fn last_shortcut_command(&self) -> Option<&'static str> {
        self.settings.keys.last
    }

    /// Какая команда ждёт нажатия на странице Shortcuts.
    pub fn listening_shortcut(&self) -> Option<&'static str> {
        self.settings.keys.listening
    }

    /// Начать (или отменить, `None`) запись сочетания для команды `id`.
    pub fn listen_shortcut(&mut self, id: Option<&str>) {
        self.settings.keys.listening = id.and_then(command).map(|c| c.id);
    }

    /// Выполнить команду `id`, как если бы нажали её клавиши. `false` — нет такой команды.
    pub fn run_command(&mut self, id: &str) -> bool {
        let selection = self.selection;
        let editing = self.mode == Mode::Edit;
        match id {
            "new-scene" => self.actions.push(Action::NewScene),
            // Во вкладке Code Ctrl+S сохраняет файл, а не сцену.
            "save" => self.actions.push(if self.view_tab == ViewTab::Code { Action::SaveCode } else { Action::Save }),
            "save-as" => self.open_save_as(),
            "preferences" => self.preferences_open = !self.preferences_open,
            "project-settings" => self.script_order.open = !self.script_order.open,
            "quit" => self.actions.push(Action::Quit),
            "undo" => self.actions.push(Action::Undo),
            "redo" | "redo-alt" => self.actions.push(Action::Redo),
            "duplicate" => self.actions.extend(selection.map(Action::Duplicate)),
            "rename" => {
                if let Some((id, name)) = selection.and_then(|id| self.app.world.scene.get(id).map(|o| (id, o.name.clone()))) {
                    self.hierarchy.begin_rename(id, &name);
                }
            }
            "delete" => self.actions.extend(selection.map(Action::Delete)),
            "frame-selected" => self.actions.push(Action::FrameSelected),
            "select-none" => {
                if selection.is_some() {
                    self.actions.push(Action::Select(None));
                }
            }
            "create-empty" => self.actions.push(Action::CreateEmpty { parent: None }),
            "create-empty-child" => self.actions.push(Action::CreateEmpty { parent: selection }),
            "play" => self.actions.push(if editing { Action::Play } else { Action::Stop }),
            "pause" => self.actions.push(Action::Pause),
            "step" => self.actions.push(Action::Step),
            "tool-move" => self.tool = Tool::Move,
            "tool-rotate" => self.tool = Tool::Rotate,
            "tool-scale" => self.tool = Tool::Scale,
            "toggle-snap" => self.snap = !self.snap,
            "toggle-local" => self.gizmo_local = !self.gizmo_local,
            "view-3d" => {
                let enabled = !self.scene_3d;
                self.set_scene_3d(enabled);
            }
            "view-grid" => self.show_grid = !self.show_grid,
            "view-gizmos" => self.show_gizmos = !self.show_gizmos,
            "refresh" => self.actions.push(Action::RefreshAssets),
            "build-code" => self.actions.push(Action::Build),
            "maximize-panel" => {
                let panel = self.maximized_panel().unwrap_or(Panel::Scene);
                self.actions.push(Action::Layout(LayoutAction::ToggleMaximize(panel)));
            }
            other => match Panel::ALL.into_iter().find(|p| window_command(*p) == other) {
                Some(panel) => self.actions.push(Action::Layout(LayoutAction::Focus(panel))),
                None => return false,
            },
        }
        true
    }

    /// Команда `id` активна в этом кадре (контекст клавиатуры).
    fn command_active(&self, ctx: &egui::Context, when: When, typing: bool) -> bool {
        match when {
            When::Always => true,
            When::NotTyping => !typing,
            When::Scene => !typing && !self.project.keys_owned(ctx),
            When::EditMode => !typing && !self.project.keys_owned(ctx) && self.mode == Mode::Edit,
            When::Dock => false,
        }
    }

    /// Горячие клавиши кадра: запись сочетания на странице Shortcuts, затем команды.
    pub(crate) fn dispatch_shortcuts(&mut self, ctx: &egui::Context) {
        publish(ctx, &self.prefs.shortcuts);
        if self.settings.keys.listening.is_some() {
            self.capture_listened_key(ctx);
            return;
        }
        if self.dialog.is_some() || self.dock.save_dialog.is_some() || self.project.modal_open() || self.settings_modal_open() {
            return;
        }
        let typing = ctx.wants_keyboard_input();
        let presses: Vec<egui::Event> = ctx.input(|i| {
            i.events.iter().filter(|e| matches!(e, egui::Event::Key { pressed: true, .. })).cloned().collect()
        });
        for event in presses {
            let egui::Event::Key { key, modifiers, .. } = &event else { continue };
            let Some(id) = self.match_command(ctx, *key, *modifiers, typing) else { continue };
            ctx.input_mut(|i| {
                if let Some(at) = i.events.iter().position(|e| *e == event) {
                    i.events.remove(at);
                }
            });
            self.settings.keys.last = Some(id);
            self.run_command(id);
        }
    }

    /// Команда для нажатия: сначала точное совпадение модификаторов, затем «логическое»
    /// (лишние Shift / Alt), из них — с большим числом модификаторов.
    fn match_command(&self, ctx: &egui::Context, key: Key, modifiers: Modifiers, typing: bool) -> Option<&'static str> {
        let candidates: Vec<(&'static CommandDef, KeyBinding)> = COMMANDS
            .iter()
            .filter_map(|c| self.prefs.shortcuts.binding(c.id).filter(|b| b.key == key).map(|b| (c, b)))
            .filter(|(c, _)| self.command_active(ctx, c.when, typing))
            .collect();
        candidates
            .iter()
            .find(|(_, b)| b.matches_exact(modifiers))
            .or_else(|| {
                candidates
                    .iter()
                    .filter(|(_, b)| b.matches_logically(modifiers))
                    .max_by_key(|(_, b)| b.modifier_count())
            })
            .map(|(c, _)| c.id)
    }

    /// Нажатие команды `id`, которую обрабатывает не реестр, а сам модуль (док):
    /// поглотить нажатие её текущего сочетания. `true` — нажали.
    pub(crate) fn consume_command(&mut self, ctx: &egui::Context, id: &'static str) -> bool {
        if self.settings.keys.listening.is_some() {
            return false;
        }
        let Some(binding) = self.prefs.shortcuts.binding(id) else { return false };
        let pressed = ctx.input_mut(|i| i.consume_key(binding.modifiers(), binding.key));
        if pressed {
            self.settings.keys.last = Some(id);
        }
        pressed
    }

    /// Режим записи: первое нажатие клавиши становится сочетанием команды. Esc без
    /// модификаторов отменяет запись. Занятое сочетание спрашивает, заменить ли.
    fn capture_listened_key(&mut self, ctx: &egui::Context) {
        let Some(id) = self.settings.keys.listening else { return };
        if !self.preferences_open {
            self.settings.keys.listening = None;
            return;
        }
        let pressed = ctx.input_mut(|i| {
            let at = i.events.iter().position(|e| matches!(e, egui::Event::Key { pressed: true, repeat: false, .. }))?;
            match i.events.remove(at) {
                egui::Event::Key { key, modifiers, .. } => Some(KeyBinding::from_event(key, modifiers)),
                _ => None,
            }
        });
        let Some(binding) = pressed else { return };
        self.settings.keys.listening = None;
        if binding == KeyBinding::key(Key::Escape) {
            return;
        }
        self.assign_shortcut(id, binding);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_print_and_parse_back() {
        for def in COMMANDS {
            let Some(binding) = def.default else { continue };
            assert_eq!(KeyBinding::parse(&binding.text()), Some(binding), "{}", def.id);
            assert_eq!(KeyBinding::parse(&binding.storage()), Some(binding), "{}", def.id);
            assert!(!binding.storage().contains([',', ':']), "{}", def.id);
        }
        assert_eq!(KeyBinding::ctrl_shift(Key::S).text(), "Ctrl+Shift+S");
        assert_eq!(KeyBinding::ctrl(Key::Comma).text(), "Ctrl+,");
        assert_eq!(KeyBinding::ctrl(Key::Comma).storage(), "Ctrl+Comma");
        assert_eq!(KeyBinding::key(Key::Delete).text(), "Del");
        assert_eq!(KeyBinding::parse("ctrl+k"), Some(KeyBinding::ctrl(Key::K)));
        assert_eq!(KeyBinding::parse("Ctrl+"), None);
    }

    #[test]
    fn command_ids_are_unique_and_defaults_do_not_clash() {
        for (i, a) in COMMANDS.iter().enumerate() {
            for b in &COMMANDS[i + 1..] {
                assert_ne!(a.id, b.id);
                if a.default.is_some() {
                    assert_ne!(a.default, b.default, "{} and {} share a default", a.id, b.id);
                }
            }
        }
        assert!(Keymap::default().conflict("save", KeyBinding::ctrl(Key::S)).is_none());
        assert_eq!(Keymap::default().conflict("quit", KeyBinding::ctrl(Key::S)).map(|c| c.id), Some("save"));
    }

    #[test]
    fn keymap_keeps_only_differences_and_round_trips() {
        let mut keymap = Keymap::default();
        keymap.set("save", Some(KeyBinding::ctrl(Key::S)));
        assert!(keymap.is_default("save"));
        keymap.set("play", Some(KeyBinding::key(Key::F5)));
        keymap.set("quit", None);
        assert_eq!(keymap.text("play"), "F5");
        assert_eq!(keymap.text("quit"), "");
        let mut back = Keymap::default();
        for line in keymap.to_text().lines() {
            let (key, value) = line.split_once(':').unwrap();
            back.parse_entry(key.trim().trim_start_matches("shortcut."), value.trim().trim_end_matches(','));
        }
        assert_eq!(back, keymap);
    }
}
