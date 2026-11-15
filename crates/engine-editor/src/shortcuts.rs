//! Реестр команд редактора и их горячих клавиш.
//!
//! Каждая команда — строка таблицы [`COMMANDS`]: постоянное имя (`save`, `tool-move`),
//! заголовок, раздел и сочетание по умолчанию. Пользователь переназначает сочетания на
//! странице Preferences → Shortcuts; переназначения хранятся в настройках редактора
//! ([`crate::Preferences::shortcuts`], строки `shortcut.<имя>: Ctrl+Shift+S`).
//! Обработка клавиш ([`Editor::dispatch_shortcuts`]), подписи в меню и подсказки
//! кнопок берут сочетание только отсюда, поэтому показывают *текущее* назначение.

use crate::i18n::n_;
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
    /// Ещё и только пока ввод не у игры: в Play с фокусом на окне Game одиночные буквы
    /// принадлежат игре.
    EditMode,
    /// Обрабатывает док (окно под курсором) — см. [`Editor::consume_command`].
    Dock,
    /// Во вкладке Code или в Error List (и пока печатают в тексте кода). У такой команды
    /// приоритет над общей с тем же сочетанием.
    Code,
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
    /// Первое нажатие сочетания из двух (`Ctrl+K` в `Ctrl+K, Ctrl+C`), как в Visual Studio.
    pub prefix: Option<KeyBinding>,
    pub(crate) when: When,
}

const fn cmd(id: &'static str, title: &'static str, category: &'static str, default: Option<KeyBinding>, when: When) -> CommandDef {
    CommandDef { id, title, category, default, prefix: None, when }
}

/// Команда с сочетанием из двух нажатий: `prefix`, затем `key`.
const fn chord(id: &'static str, title: &'static str, category: &'static str, prefix: KeyBinding, key: KeyBinding, when: When) -> CommandDef {
    CommandDef { id, title, category, default: Some(key), prefix: Some(prefix), when }
}

const fn alt(key: Key) -> KeyBinding {
    KeyBinding::with(key, false, false, true)
}

const fn shift_alt(key: Key) -> KeyBinding {
    KeyBinding::with(key, false, true, true)
}

const CTRL_K: KeyBinding = KeyBinding::ctrl(Key::K);
const CTRL_M: KeyBinding = KeyBinding::ctrl(Key::M);

const N: Key = Key::N; // «N» встречается в трёх сочетаниях ниже

/// Все команды редактора с горячими клавишами, в порядке страницы Shortcuts.
pub const COMMANDS: &[CommandDef] = &[
    // --- File ---
    cmd("new-scene", n_("New Scene"), n_("File"), Some(KeyBinding::ctrl(N)), When::Always),
    cmd("open-scene", n_("Open Scene"), n_("File"), Some(KeyBinding::ctrl(Key::O)), When::Always),
    cmd("save", n_("Save"), n_("File"), Some(KeyBinding::ctrl(Key::S)), When::Always),
    cmd("save-as", n_("Save As…"), n_("File"), Some(KeyBinding::ctrl_shift(Key::S)), When::Always),
    cmd("preferences", n_("Preferences…"), n_("File"), Some(KeyBinding::ctrl(Key::Comma)), When::Always),
    cmd("project-settings", n_("Project Settings…"), n_("File"), None, When::Always),
    cmd("quit", n_("Quit"), n_("File"), Some(KeyBinding::ctrl(Key::Q)), When::Always),
    // --- Edit ---
    cmd("undo", n_("Undo"), n_("Edit"), Some(KeyBinding::ctrl(Key::Z)), When::NotTyping),
    cmd("redo", n_("Redo"), n_("Edit"), Some(KeyBinding::ctrl(Key::Y)), When::NotTyping),
    cmd("redo-alt", n_("Redo (second shortcut)"), n_("Edit"), Some(KeyBinding::ctrl_shift(Key::Z)), When::NotTyping),
    cmd("duplicate", n_("Duplicate"), n_("Edit"), Some(KeyBinding::ctrl(Key::D)), When::Scene),
    cmd("rename", n_("Rename"), n_("Edit"), Some(KeyBinding::key(Key::F2)), When::EditMode),
    cmd("delete", n_("Delete"), n_("Edit"), Some(KeyBinding::key(Key::Delete)), When::EditMode),
    cmd("frame-selected", n_("Frame Selected"), n_("Edit"), Some(KeyBinding::key(Key::F)), When::EditMode),
    cmd("select-none", n_("Select None"), n_("Edit"), Some(KeyBinding::key(Key::Escape)), When::EditMode),
    cmd("select-all", n_("Select All"), n_("Edit"), Some(KeyBinding::ctrl(Key::A)), When::Scene),
    cmd("copy", n_("Copy"), n_("Edit"), Some(KeyBinding::ctrl(Key::C)), When::Scene),
    cmd("cut", n_("Cut"), n_("Edit"), Some(KeyBinding::ctrl(Key::X)), When::Scene),
    cmd("paste", n_("Paste"), n_("Edit"), Some(KeyBinding::ctrl(Key::V)), When::Scene),
    cmd("paste-child", n_("Paste as Child"), n_("Edit"), Some(KeyBinding::ctrl_shift(Key::V)), When::Scene),
    cmd("group", n_("Group Selection"), n_("Edit"), Some(KeyBinding::ctrl(Key::G)), When::Scene),
    cmd("hide", n_("Deactivate Selection"), n_("Edit"), Some(KeyBinding::key(Key::H)), When::EditMode),
    cmd("unhide", n_("Activate Selection"), n_("Edit"), Some(KeyBinding::with(Key::H, false, true, false)), When::EditMode),
    // --- GameObject ---
    cmd("create-empty", n_("Create Empty"), "GameObject", Some(KeyBinding::ctrl_shift(N)), When::Always),
    cmd("create-empty-child", n_("Create Empty Child"), "GameObject", Some(KeyBinding::with(N, false, true, true)), When::Always),
    // --- Play ---
    cmd("play", n_("Play / Stop"), n_("Play"), Some(KeyBinding::ctrl(Key::P)), When::Always),
    cmd("pause", n_("Pause / Resume"), n_("Play"), Some(KeyBinding::ctrl_shift(Key::P)), When::Always),
    cmd("step", n_("Step"), n_("Play"), Some(KeyBinding::with(Key::P, true, false, true)), When::Always),
    // --- Tools ---
    cmd("tool-move", n_("Move Tool"), n_("Tools"), Some(KeyBinding::key(Key::W)), When::EditMode),
    cmd("tool-rotate", n_("Rotate Tool"), n_("Tools"), Some(KeyBinding::key(Key::E)), When::EditMode),
    cmd("tool-scale", n_("Scale Tool"), n_("Tools"), Some(KeyBinding::key(Key::R)), When::EditMode),
    cmd("toggle-snap", n_("Snap to Grid"), n_("Tools"), None, When::EditMode),
    cmd("toggle-local", n_("Local / World Axes"), n_("Tools"), None, When::EditMode),
    cmd("toggle-pivot", n_("Pivot / Center"), n_("Tools"), Some(KeyBinding::key(Key::Z)), When::EditMode),
    // --- View ---
    cmd("view-3d", n_("2D / 3D View"), n_("View"), Some(KeyBinding::key(Key::Num2)), When::EditMode),
    cmd("view-grid", n_("Show Grid"), n_("View"), None, When::Scene),
    cmd("view-gizmos", n_("Show Gizmos"), n_("View"), None, When::Scene),
    // --- Assets ---
    cmd("refresh", n_("Refresh Assets"), n_("Assets"), Some(KeyBinding::ctrl(Key::R)), When::Always),
    cmd("build-code", n_("Build Code"), n_("Assets"), Some(KeyBinding::ctrl(Key::B)), When::Always),
    // --- Window ---
    cmd("window-scene", n_("Scene"), n_("Window"), Some(KeyBinding::ctrl(Key::Num1)), When::Always),
    cmd("window-game", n_("Game"), n_("Window"), Some(KeyBinding::ctrl(Key::Num2)), When::Always),
    cmd("window-code", n_("Code"), n_("Window"), Some(KeyBinding::ctrl(Key::Num3)), When::Always),
    cmd("window-hierarchy", n_("Hierarchy"), n_("Window"), Some(KeyBinding::ctrl(Key::Num4)), When::Always),
    cmd("window-project", n_("Project"), n_("Window"), Some(KeyBinding::ctrl(Key::Num5)), When::Always),
    cmd("window-console", n_("Console"), n_("Window"), Some(KeyBinding::ctrl(Key::Num6)), When::Always),
    cmd("window-error-list", n_("Error List"), n_("Window"), Some(KeyBinding::ctrl(Key::Num9)), When::Always),
    cmd("window-find-results", n_("Find Results"), n_("Window"), None, When::Always),
    cmd("window-profiler", n_("Profiler"), n_("Window"), Some(KeyBinding::ctrl(Key::Num7)), When::Always),
    cmd("window-inspector", n_("Inspector"), n_("Window"), Some(KeyBinding::ctrl(Key::Num8)), When::Always),
    cmd("maximize-panel", n_("Maximize / Restore Window"), n_("Window"), Some(KeyBinding::with(Key::Space, false, true, false)), When::Dock),
    // --- Code Editor ---
    cmd("check-code", n_("Check Code (cargo check)"), n_("Code Editor"), Some(KeyBinding::key(Key::F7)), When::Code),
    cmd("next-error", n_("Next Error"), n_("Code Editor"), Some(KeyBinding::key(Key::F8)), When::Code),
    cmd("prev-error", n_("Previous Error"), n_("Code Editor"), Some(KeyBinding::with(Key::F8, false, true, false)), When::Code),
    cmd("quick-fix", n_("Quick Fix"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Period)), When::Code),
    cmd("quick-fix-alt", n_("Quick Fix (second shortcut)"), n_("Code Editor"), Some(KeyBinding::with(Key::Enter, false, false, true)), When::Code),
    cmd("find-next", n_("Find Next"), n_("Code Editor"), Some(KeyBinding::key(Key::F3)), When::Code),
    cmd("find-prev", n_("Find Previous"), n_("Code Editor"), Some(KeyBinding::with(Key::F3, false, true, false)), When::Code),
    cmd("find-selected", n_("Find Selected"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::F3)), When::Code),
    cmd("go-to-file", n_("Go to File"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::P)), When::Code),
    cmd("go-to-file-alt", n_("Go to File (second shortcut)"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Comma)), When::Code),
    cmd("go-to-symbol", n_("Go to Symbol in File"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::O)), When::Code),
    cmd("go-to-symbol-alt", n_("Go to Symbol in File (second shortcut)"), n_("Code Editor"), Some(alt(Key::Backslash)), When::Code),
    cmd("go-to-project-symbol", n_("Go to Symbol in Project"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::T)), When::Code),
    cmd("go-to-definition", n_("Go to Definition"), n_("Code Editor"), Some(KeyBinding::key(Key::F12)), When::Code),
    cmd("peek-definition", n_("Peek Definition"), n_("Code Editor"), Some(alt(Key::F12)), When::Code),
    cmd("navigate-back", n_("Navigate Back"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Minus)), When::Code),
    cmd("navigate-forward", n_("Navigate Forward"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::Minus)), When::Code),
    cmd("next-tab", n_("Next Tab (recently used)"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Tab)), When::Code),
    cmd("reopen-tab", n_("Reopen Closed Tab"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::T)), When::Code),
    cmd("find-in-files", n_("Find in Files"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::F)), When::Code),
    cmd("find-references", n_("Find All References"), n_("Code Editor"), Some(KeyBinding::with(Key::F12, false, true, false)), When::Code),
    cmd("complete", n_("Show Completions"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Space)), When::Code),
    cmd("parameter-info", n_("Parameter Info"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::Space)), When::Code),
    chord("format-document", n_("Format Document"), n_("Code Editor"), CTRL_K, KeyBinding::ctrl(Key::D), When::Code),
    cmd("rename-symbol", n_("Rename Symbol"), n_("Code Editor"), Some(KeyBinding::key(Key::F2)), When::Code),
    chord("rename-symbol-alt", n_("Rename Symbol (second shortcut)"), n_("Code Editor"), KeyBinding::ctrl(Key::R), KeyBinding::ctrl(Key::R), When::Code),
    cmd("duplicate-line", n_("Duplicate Line"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::D)), When::Code),
    cmd("delete-line", n_("Delete Line"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::K)), When::Code),
    cmd("cut-line", n_("Cut Line"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::L)), When::Code),
    cmd("move-line-up", n_("Move Line Up"), n_("Code Editor"), Some(alt(Key::ArrowUp)), When::Code),
    cmd("move-line-down", n_("Move Line Down"), n_("Code Editor"), Some(alt(Key::ArrowDown)), When::Code),
    cmd("copy-line-up", n_("Copy Line Up"), n_("Code Editor"), Some(shift_alt(Key::ArrowUp)), When::Code),
    cmd("copy-line-down", n_("Copy Line Down"), n_("Code Editor"), Some(shift_alt(Key::ArrowDown)), When::Code),
    cmd("join-lines", n_("Join Lines"), n_("Code Editor"), None, When::Code),
    cmd("sort-lines", n_("Sort Lines"), n_("Code Editor"), None, When::Code),
    cmd("toggle-comment", n_("Toggle Line Comment"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Slash)), When::Code),
    chord("comment-lines", n_("Comment Selection"), n_("Code Editor"), CTRL_K, KeyBinding::ctrl(Key::C), When::Code),
    chord("uncomment-lines", n_("Uncomment Selection"), n_("Code Editor"), CTRL_K, KeyBinding::ctrl(Key::U), When::Code),
    cmd("block-comment", n_("Toggle Block Comment"), n_("Code Editor"), Some(shift_alt(Key::A)), When::Code),
    cmd("upper-case", n_("Make Uppercase"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::U)), When::Code),
    cmd("lower-case", n_("Make Lowercase"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::U)), When::Code),
    cmd("go-to-bracket", n_("Go to Matching Bracket"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::CloseBracket)), When::Code),
    cmd("expand-selection", n_("Expand Selection"), n_("Code Editor"), Some(shift_alt(Key::ArrowRight)), When::Code),
    cmd("shrink-selection", n_("Shrink Selection"), n_("Code Editor"), Some(shift_alt(Key::ArrowLeft)), When::Code),
    chord("toggle-bookmark", n_("Toggle Bookmark"), n_("Code Editor"), CTRL_K, KeyBinding::ctrl(Key::K), When::Code),
    chord("next-bookmark", n_("Next Bookmark"), n_("Code Editor"), CTRL_K, KeyBinding::ctrl(Key::N), When::Code),
    chord("prev-bookmark", n_("Previous Bookmark"), n_("Code Editor"), CTRL_K, KeyBinding::ctrl(Key::P), When::Code),
    cmd("word-wrap", n_("Toggle Word Wrap"), n_("Code Editor"), Some(alt(Key::Z)), When::Code),
    cmd("code-zoom-reset", n_("Reset Code Zoom"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::Num0)), When::Code),
    // Несколько курсоров и выделение столбцом (Alt+щелчок, Shift+Alt+мышь — в самом виджете).
    cmd("add-caret-above", n_("Add Caret Above"), n_("Code Editor"), Some(KeyBinding::with(Key::ArrowUp, true, false, true)), When::Code),
    cmd("add-caret-below", n_("Add Caret Below"), n_("Code Editor"), Some(KeyBinding::with(Key::ArrowDown, true, false, true)), When::Code),
    cmd("box-select-left", n_("Column Select Left"), n_("Code Editor"), Some(KeyBinding::with(Key::ArrowLeft, true, true, true)), When::Code),
    cmd("box-select-right", n_("Column Select Right"), n_("Code Editor"), Some(KeyBinding::with(Key::ArrowRight, true, true, true)), When::Code),
    cmd("box-select-up", n_("Column Select Up"), n_("Code Editor"), Some(KeyBinding::with(Key::ArrowUp, true, true, true)), When::Code),
    cmd("box-select-down", n_("Column Select Down"), n_("Code Editor"), Some(KeyBinding::with(Key::ArrowDown, true, true, true)), When::Code),
    cmd("add-next-occurrence", n_("Add Next Occurrence"), n_("Code Editor"), Some(shift_alt(Key::Period)), When::Code),
    cmd("select-all-occurrences", n_("Select All Occurrences"), n_("Code Editor"), Some(shift_alt(Key::Semicolon)), When::Code),
    cmd("select-all-occurrences-alt", n_("Select All Occurrences (second shortcut)"), n_("Code Editor"), Some(KeyBinding::ctrl_shift(Key::L)), When::Code),
    cmd("close-tab", n_("Close Tab"), n_("Code Editor"), Some(KeyBinding::ctrl(Key::W)), When::Code),
    cmd("code-new-file", n_("New File in Code Folder"), n_("Code Editor"), Some(KeyBinding::ctrl(N)), When::Code),
    // Свёртка блоков (Ctrl+M, …), как в Visual Studio.
    chord("toggle-outlining", n_("Toggle Outlining Expansion"), n_("Code Editor"), CTRL_M, KeyBinding::ctrl(Key::M), When::Code),
    chord("collapse-to-definitions", n_("Collapse to Definitions"), n_("Code Editor"), CTRL_M, KeyBinding::ctrl(Key::O), When::Code),
    chord("toggle-all-outlining", n_("Toggle All Outlining"), n_("Code Editor"), CTRL_M, KeyBinding::ctrl(Key::L), When::Code),
    chord("stop-outlining", n_("Stop Outlining"), n_("Code Editor"), CTRL_M, KeyBinding::ctrl(Key::P), When::Code),
    // --- Help ---
    cmd("shortcuts-help", n_("Keyboard Shortcuts"), n_("Help"), Some(KeyBinding::key(Key::F1)), When::Always),
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
        Panel::ErrorList => "window-error-list",
        Panel::FindResults => "window-find-results",
        Panel::Profiler => "window-profiler",
        Panel::Inspector => "window-inspector",
    }
}

/// Назначенные пользователем сочетания (только отличия от умолчаний).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    /// Имя команды → сочетание (`None` — сочетание снято).
    overrides: BTreeMap<String, Option<KeyBinding>>,
    /// Переназначенные сочетания из двух нажатий: первое нажатие (второе — в `overrides`).
    chords: BTreeMap<String, KeyBinding>,
}

impl Keymap {
    /// Текущее сочетание команды `id`.
    pub fn binding(&self, id: &str) -> Option<KeyBinding> {
        match self.overrides.get(id) {
            Some(binding) => *binding,
            None => command(id).and_then(|c| c.default),
        }
    }

    /// Первое нажатие сочетания из двух (`None` — сочетание из одного нажатия).
    pub fn prefix(&self, id: &str) -> Option<KeyBinding> {
        match self.overrides.get(id) {
            Some(_) => self.chords.get(id).copied(),
            None => command(id).and_then(|c| c.prefix),
        }
    }

    /// Подпись текущего сочетания (пусто — без сочетания): `Ctrl+S`, `Ctrl+K, Ctrl+C`.
    pub fn text(&self, id: &str) -> String {
        match (self.prefix(id), self.binding(id)) {
            (Some(prefix), Some(key)) => format!("{}, {}", prefix.text(), key.text()),
            (_, key) => key.map(|b| b.text()).unwrap_or_default(),
        }
    }

    /// Назначить сочетание из двух нажатий.
    pub fn set_chord(&mut self, id: &str, prefix: KeyBinding, key: KeyBinding) {
        let Some(def) = command(id) else { return };
        if def.prefix == Some(prefix) && def.default == Some(key) {
            self.reset(id);
        } else {
            self.overrides.insert(id.to_string(), Some(key));
            self.chords.insert(id.to_string(), prefix);
        }
    }

    /// `text` в скобках после подсказки: `Play (Ctrl+P)`; без сочетания — только `text`.
    pub fn tip(&self, text: &str, id: &str) -> String {
        match self.binding(id) {
            Some(_) => format!("{text} ({})", self.text(id)),
            None => text.to_string(),
        }
    }

    /// Назначить сочетание (`None` — снять). Совпадение с умолчанием убирает запись.
    pub fn set(&mut self, id: &str, binding: Option<KeyBinding>) {
        let Some(def) = command(id) else { return };
        self.chords.remove(id);
        if def.default == binding && def.prefix.is_none() {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.to_string(), binding);
        }
    }

    /// Вернуть сочетание по умолчанию.
    pub fn reset(&mut self, id: &str) {
        self.overrides.remove(id);
        self.chords.remove(id);
    }

    pub fn reset_all(&mut self) {
        self.overrides.clear();
        self.chords.clear();
    }

    /// Сочетание команды не меняли.
    pub fn is_default(&self, id: &str) -> bool {
        !self.overrides.contains_key(id)
    }

    /// Другая команда, у которой уже есть сочетание `binding`.
    pub fn conflict(&self, id: &str, binding: KeyBinding) -> Option<&'static CommandDef> {
        COMMANDS.iter().find(|c| c.id != id && self.binding(c.id) == Some(binding) && self.prefix(c.id).is_none())
    }

    /// Строки файла настроек: `    shortcut.save: Ctrl+Shift+S,` (`none` — снято).
    pub(crate) fn to_text(&self) -> String {
        let mut text = String::new();
        for (id, binding) in &self.overrides {
            let value = match (self.chords.get(id), binding) {
                (Some(prefix), Some(key)) => format!("{} {}", prefix.storage(), key.storage()),
                _ => binding.map_or_else(|| "none".to_string(), |b| b.storage()),
            };
            text.push_str(&format!("    shortcut.{id}: {value},\n", id = id, value = value));
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
        } else if let Some((first, second)) = value.split_once(' ').filter(|(_, b)| !b.trim().is_empty()) {
            if let (Some(prefix), Some(key)) = (KeyBinding::parse(first), KeyBinding::parse(second)) {
                self.set_chord(id, prefix, key);
            }
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
    /// Нажато первое нажатие сочетания из двух (`Ctrl+K`): ждём второе.
    pub chord: Option<KeyBinding>,
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
            "new-scene" => self.show_new_scene_dialog(true),
            "open-scene" => self.show_scene_picker(true),
            // Во вкладке Code Ctrl+S сохраняет файл, а не сцену.
            "save" => self.actions.push(if self.view_tab == ViewTab::Code { Action::SaveCode } else { Action::Save }),
            "save-as" => self.open_save_as(),
            "preferences" => self.preferences_open = !self.preferences_open,
            "project-settings" => self.script_order.open = !self.script_order.open,
            "quit" => self.actions.push(Action::Quit),
            "undo" => self.actions.push(Action::Undo),
            "redo" | "redo-alt" => self.actions.push(Action::Redo),
            "duplicate" => self.actions.push(Action::DuplicateSelection),
            "rename" => {
                if let Some((id, name)) = selection.and_then(|id| self.app.world.scene.get(id).map(|o| (id, o.name.clone()))) {
                    self.hierarchy.begin_rename(id, &name);
                }
            }
            "delete" => self.actions.push(Action::DeleteSelection),
            "select-all" => self.actions.push(Action::SelectAll),
            "copy" => self.actions.push(Action::CopySelection),
            "cut" => self.actions.push(Action::CutSelection),
            "paste" => self.actions.push(Action::Paste { as_child: false }),
            "paste-child" => self.actions.push(Action::Paste { as_child: true }),
            "group" => self.actions.push(Action::GroupSelection),
            "hide" => self.actions.push(Action::SetSelectionActive(false)),
            "unhide" => self.actions.push(Action::SetSelectionActive(true)),
            "toggle-pivot" => self.pivot_center = !self.pivot_center,
            "shortcuts-help" => self.shortcuts_help_open = !self.shortcuts_help_open,
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
            "check-code" => {
                crate::code_checks::request_compiler(self, None, 0.0);
            }
            "next-error" => crate::error_list::step(self, true),
            "prev-error" => crate::error_list::step(self, false),
            "quick-fix" | "quick-fix-alt" => self.code.commands.push("quick-fix"),
            "word-wrap" => self.prefs.code.word_wrap = !self.prefs.code.word_wrap,
            "find-next" => self.code.commands.push("find-next"),
            "complete" => self.code.commands.push("complete"),
            "parameter-info" => self.code.commands.push("parameter-info"),
            "go-to-file" | "go-to-file-alt" => self.open_picker(crate::code_nav::PickerKind::Files),
            "go-to-symbol" | "go-to-symbol-alt" => self.open_picker(crate::code_nav::PickerKind::FileSymbols),
            "go-to-project-symbol" => self.open_picker(crate::code_nav::PickerKind::ProjectSymbols),
            "go-to-definition" => self.go_to_definition(false),
            "find-in-files" => self.open_find_in_files(),
            "format-document" => self.code.commands.push("format-document"),
            // F2 в дереве файлов — переименовать файл, в тексте — символ.
            "rename-symbol" | "rename-symbol-alt" if self.code.tree.focused => {
                if let Some(path) = self.code.tree.selected.last().cloned() {
                    self.code.tree_commands.push(crate::code_tree::TreeCmd::BeginRename(path));
                }
            }
            "rename-symbol" | "rename-symbol-alt" => self.begin_code_rename(),
            "close-tab" => {
                let active = self.code.active;
                if !self.code.documents.is_empty() {
                    self.code.request_close(&[active]);
                }
            }
            "code-new-file" => {
                let folder = crate::code_tree::selected_folder(self);
                if let Some(folder) = folder {
                    self.code.tree_commands.push(crate::code_tree::TreeCmd::New(crate::code_tree::NewKind::TextFile, folder));
                }
            }
            "find-references" => self.find_all_references(),
            "peek-definition" => self.go_to_definition(true),
            "navigate-back" => self.navigate_history(true),
            "navigate-forward" => self.navigate_history(false),
            "next-tab" => self.switch_tab_mru(),
            "reopen-tab" => self.reopen_closed_tab(),
            "find-prev" => self.code.commands.push("find-prev"),
            "find-selected" => self.code.commands.push("find-selected"),
            "code-zoom-reset" => self.prefs.code.font_size = crate::code_prefs::CodePrefs::default().font_size,
            "select-all-occurrences-alt" => self.code.commands.push("select-all-occurrences"),
            other if crate::code_keys::CARET_COMMANDS.contains(&other) => {
                let id = COMMANDS.iter().find(|c| c.id == other).map_or("", |c| c.id);
                self.code.commands.push(id);
            }
            other if crate::code_keys::TEXT_COMMANDS.contains(&other) => {
                if self.prefs.code.keymap == crate::code_prefs::Keymap::VisualStudio {
                    let id = COMMANDS.iter().find(|c| c.id == other).map_or("", |c| c.id);
                    self.code.commands.push(id);
                }
            }
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
            // Во вкладке Code Ctrl+G / Ctrl+C… — её собственные (перейти к строке, текст).
            When::Scene => {
                !typing && !self.project.keys_owned(ctx) && self.focused_panel() != Some(crate::dock::Panel::Code)
            }
            // Пока правой кнопкой летают по виду Scene, W / A / S / D / Q / E — полёт, а не
            // инструменты.
            When::EditMode => {
                !typing
                    && !self.project.keys_owned(ctx)
                    && !self.game_has_input_focus()
                    && !self.viewport.flying
                    && !self.hierarchy_typing(ctx.input(|i| i.time))
            }
            When::Dock => false,
            When::Code => {
                let panel = self.focused_panel();
                panel == Some(Panel::Code) || panel == Some(Panel::ErrorList)
            }
        }
    }

    /// Горячие клавиши кадра: запись сочетания на странице Shortcuts, затем команды.
    pub(crate) fn dispatch_shortcuts(&mut self, ctx: &egui::Context) {
        publish(ctx, &self.prefs.shortcuts);
        if self.settings.keys.listening.is_some() {
            self.capture_listened_key(ctx);
            return;
        }
        if self.dialog.is_some()
            || self.code.close_ask.is_some()
            || self.dock.save_dialog.is_some()
            || self.project.modal_open()
            || self.settings_modal_open()
            || self.scene_picker.open
            || self.scene_picker.new_scene
        {
            return;
        }
        let typing = ctx.wants_keyboard_input();
        let presses: Vec<egui::Event> = ctx.input(|i| {
            i.events.iter().filter(|e| matches!(e, egui::Event::Key { pressed: true, .. })).cloned().collect()
        });
        // Открыто всплывающее окно (Build, Undo History…) — Esc закрывает его, а не снимает
        // выделение (окно ловит Esc само, когда рисуется).
        let window_open = crate::window_kit::tool_window_was_open(ctx);
        for event in presses {
            let egui::Event::Key { key, modifiers, .. } = &event else { continue };
            if window_open && *key == egui::Key::Escape && modifiers.is_none() {
                continue;
            }
            let pressed = KeyBinding::from_event(*key, *modifiers);
            let id = match self.settings.keys.chord.take() {
                // Второе нажатие сочетания из двух: команда или ничего (нажатие поглощается).
                Some(prefix) => COMMANDS
                    .iter()
                    .filter(|c| self.prefs.shortcuts.prefix(c.id) == Some(prefix) && self.command_active(ctx, c.when, typing))
                    .find(|c| self.prefs.shortcuts.binding(c.id).is_some_and(|b| b.key == *key && b.matches_exact(*modifiers)))
                    .map(|c| c.id)
                    .or(Some("")),
                None => {
                    let starts_chord = COMMANDS.iter().any(|c| {
                        self.prefs.shortcuts.prefix(c.id).is_some_and(|p| p == pressed) && self.command_active(ctx, c.when, typing)
                    });
                    if starts_chord {
                        self.settings.keys.chord = Some(pressed);
                        Some("")
                    } else {
                        self.match_command(ctx, *key, *modifiers, typing)
                    }
                }
            };
            let Some(id) = id else { continue };
            ctx.input_mut(|i| {
                if let Some(at) = i.events.iter().position(|e| *e == event) {
                    i.events.remove(at);
                }
            });
            if id.is_empty() {
                continue;
            }
            self.settings.keys.last = Some(id);
            self.run_command(id);
        }
    }

    /// Команда для нажатия: сначала точное совпадение модификаторов, затем «логическое»
    /// (лишние Shift / Alt), из них — с большим числом модификаторов.
    fn match_command(&self, ctx: &egui::Context, key: Key, modifiers: Modifiers, typing: bool) -> Option<&'static str> {
        let candidates: Vec<(&'static CommandDef, KeyBinding)> = COMMANDS
            .iter()
            .filter(|c| self.prefs.shortcuts.prefix(c.id).is_none())
            .filter_map(|c| self.prefs.shortcuts.binding(c.id).filter(|b| b.key == key).map(|b| (c, b)))
            .filter(|(c, _)| self.command_active(ctx, c.when, typing))
            .collect();
        // Команды вкладки Code — раньше общих с тем же сочетанием (как в Visual Studio).
        let mut candidates = candidates;
        candidates.sort_by_key(|(c, _)| c.when != When::Code);
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
                // Сочетание команды вкладки Code может совпадать с общим: в Code главнее она
                // (как в Visual Studio Ctrl+D, Ctrl+P…). Внутри одной области — не может.
                let same_scope = (a.when == When::Code) == (b.when == When::Code);
                if a.default.is_some() && same_scope {
                    assert_ne!((a.prefix, a.default), (b.prefix, b.default), "{} and {} share a default", a.id, b.id);
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
