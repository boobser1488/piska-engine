//! Preferences → Code Editor: шрифт, отступы, перенос, пробелы, линейка, парные скобки,
//! правила сохранения, проверка при наборе и после сохранения, раскладка клавиш.
//!
//! Хранятся в общем файле настроек строками `code_<имя>: значение` (см. `prefs.rs`).

use crate::i18n::tr_s;

use crate::settings::SettingsUi;
use crate::state::Editor;

/// Раскладка клавиш вкладки Code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keymap {
    /// Сочетания Visual Studio (Ctrl+D — дублировать строку, Alt+↑ / ↓, Ctrl+K Ctrl+C…).
    #[default]
    VisualStudio,
    /// Только то, что умеет само текстовое поле, без сочетаний правки строк.
    Plain,
}

/// Настройки вкладки Code.
#[derive(Clone, Debug, PartialEq)]
pub struct CodePrefs {
    pub font_size: f32,
    /// Отступ для файлов, у которых его не видно по тексту (и нет `.editorconfig`).
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub word_wrap: bool,
    pub show_whitespace: bool,
    /// Вертикальная линейка на этой колонке; 0 — нет.
    pub ruler: usize,
    pub auto_close: bool,
    pub trim_trailing: bool,
    pub final_newline: bool,
    pub format_on_save: bool,
    pub check_on_type: bool,
    /// Пауза после правки перед мгновенной проверкой, мс.
    pub check_delay_ms: u32,
    pub check_on_save: bool,
    pub keymap: Keymap,
}

impl Default for CodePrefs {
    fn default() -> Self {
        Self {
            font_size: 12.0,
            tab_size: 4,
            insert_spaces: true,
            word_wrap: false,
            show_whitespace: false,
            ruler: 0,
            auto_close: true,
            trim_trailing: false,
            final_newline: false,
            format_on_save: false,
            check_on_type: true,
            check_delay_ms: 300,
            check_on_save: true,
            keymap: Keymap::VisualStudio,
        }
    }
}

impl CodePrefs {
    /// Одна запись файла настроек (`key` — с префиксом `code_`). Незнакомое пропускается.
    pub(crate) fn set(&mut self, key: &str, value: &str) {
        let flag = value == "true";
        let num = |v: &str| v.trim().parse::<f64>().ok();
        match key {
            "code_font_size" => self.font_size = num(value).map_or(self.font_size, |v| (v as f32).clamp(8.0, 32.0)),
            "code_tab_size" => self.tab_size = num(value).map_or(self.tab_size, |v| (v as usize).clamp(1, 16)),
            "code_insert_spaces" => self.insert_spaces = flag,
            "code_word_wrap" => self.word_wrap = flag,
            "code_show_whitespace" => self.show_whitespace = flag,
            "code_ruler" => self.ruler = num(value).map_or(self.ruler, |v| (v as usize).min(400)),
            "code_auto_close" => self.auto_close = flag,
            "code_trim_trailing" => self.trim_trailing = flag,
            "code_final_newline" => self.final_newline = flag,
            "code_format_on_save" => self.format_on_save = flag,
            "code_check_on_type" => self.check_on_type = flag,
            "code_check_delay_ms" => self.check_delay_ms = num(value).map_or(self.check_delay_ms, |v| (v as u32).clamp(50, 5000)),
            "code_check_on_save" => self.check_on_save = flag,
            "code_keymap" => self.keymap = if value.trim() == "plain" { Keymap::Plain } else { Keymap::VisualStudio },
            _ => {}
        }
    }

    /// Строки файла настроек.
    pub(crate) fn to_text(&self) -> String {
        let keymap = match self.keymap {
            Keymap::VisualStudio => "visual-studio",
            Keymap::Plain => "plain",
        };
        let rows = [
            ("code_font_size", self.font_size.to_string()),
            ("code_tab_size", self.tab_size.to_string()),
            ("code_insert_spaces", self.insert_spaces.to_string()),
            ("code_word_wrap", self.word_wrap.to_string()),
            ("code_show_whitespace", self.show_whitespace.to_string()),
            ("code_ruler", self.ruler.to_string()),
            ("code_auto_close", self.auto_close.to_string()),
            ("code_trim_trailing", self.trim_trailing.to_string()),
            ("code_final_newline", self.final_newline.to_string()),
            ("code_format_on_save", self.format_on_save.to_string()),
            ("code_check_on_type", self.check_on_type.to_string()),
            ("code_check_delay_ms", self.check_delay_ms.to_string()),
            ("code_check_on_save", self.check_on_save.to_string()),
            ("code_keymap", keymap.to_string()),
        ];
        rows.iter().map(|(k, v)| format!("    {k}: {v},\n")).collect()
    }
}

impl Editor {
    /// Перенести настройки Code Editor в состояние вкладки.
    pub(crate) fn apply_code_prefs(&mut self) {
        let prefs = &self.prefs.code;
        let checks = &mut self.code.checks.settings;
        checks.on_type = prefs.check_on_type;
        checks.delay = prefs.check_delay_ms as f64 / 1000.0;
        checks.on_save = prefs.check_on_save;
        self.code.save_trim = prefs.trim_trailing;
        self.code.save_final_newline = prefs.final_newline;
        self.code.format_on_save = prefs.format_on_save;
    }
}

/// Страница Preferences → Code Editor.
pub(crate) fn page(s: &mut SettingsUi<'_>, editor: &mut Editor) {
    let defaults = CodePrefs::default();
    let prefs = &mut editor.prefs.code;
    let mut changed = false;
    s.group(tr_s("Text"));
    changed |= s.number(tr_s("Font size"), tr_s("Size of the monospace font in the Code tab (Ctrl+wheel zooms it too)."), &mut prefs.font_size, defaults.font_size, 8.0..=32.0, 0.1, tr_s(" pt"));
    changed |= s.number(tr_s("Tab size"), tr_s("Indent width for files whose own indentation cannot be seen (an `.editorconfig` and the file itself come first)."), &mut prefs.tab_size, defaults.tab_size, 1..=16, 1.0, "");
    changed |= s.toggle(tr_s("Insert spaces"), tr_s("Tab inserts spaces rather than a tab character in such files."), &mut prefs.insert_spaces, defaults.insert_spaces);
    changed |= s.toggle(tr_s("Word wrap"), tr_s("Long lines wrap at the window edge (Alt+Z)."), &mut prefs.word_wrap, defaults.word_wrap);
    changed |= s.toggle(tr_s("Show whitespace"), tr_s("Spaces and tabs are drawn as faint dots and arrows."), &mut prefs.show_whitespace, defaults.show_whitespace);
    changed |= s.number(tr_s("Column ruler"), tr_s("A vertical line at this column; 0 — none."), &mut prefs.ruler, defaults.ruler, 0..=400, 1.0, "");
    s.group(tr_s("Typing"));
    changed |= s.toggle(tr_s("Auto-close brackets and quotes"), tr_s("Typing ( [ { or a quote inserts the pair; typing the closing one steps over it."), &mut prefs.auto_close, defaults.auto_close);
    changed |= s.combo(
        tr_s("Keymap"),
        tr_s("Visual Studio: Ctrl+D duplicates a line, Alt+Up / Down moves it, Ctrl+K Ctrl+C comments… Plain: only what the text field does."),
        &mut prefs.keymap,
        defaults.keymap,
        &[(Keymap::VisualStudio, tr_s("Visual Studio")), (Keymap::Plain, tr_s("Plain"))],
    );
    s.group(tr_s("Saving"));
    changed |= s.toggle(tr_s("Trim trailing whitespace"), tr_s("Spaces at line ends are removed when a file is saved."), &mut prefs.trim_trailing, defaults.trim_trailing);
    changed |= s.toggle(tr_s("Format on save"), tr_s("Rust files go through rustfmt, C++ through clang-format (if installed), Scenario is re-indented before saving."), &mut prefs.format_on_save, defaults.format_on_save);
    changed |= s.toggle(tr_s("Insert final newline"), tr_s("A file always ends with a line break when saved."), &mut prefs.final_newline, defaults.final_newline);
    s.group(tr_s("Checks"));
    changed |= s.toggle(tr_s("Check while typing"), tr_s("Syntax errors are underlined a moment after you stop typing (Rust, C++, Scenario, RON, JSON, TOML, WGSL)."), &mut prefs.check_on_type, defaults.check_on_type);
    changed |= s.number(tr_s("Check delay"), tr_s("How long to wait after the last key press before checking."), &mut prefs.check_delay_ms, defaults.check_delay_ms, 50..=5000, 5.0, tr_s(" ms"));
    changed |= s.toggle(tr_s("Check on save (cargo check)"), tr_s("Saving a Rust file runs `cargo check` for the project in the background; its errors appear in the Error List."), &mut prefs.check_on_save, defaults.check_on_save);
    if changed {
        editor.apply_code_prefs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_prefs_round_trip() {
        let prefs = CodePrefs { font_size: 14.0, ruler: 100, keymap: Keymap::Plain, check_delay_ms: 500, trim_trailing: true, ..CodePrefs::default() };
        let mut back = CodePrefs::default();
        for line in prefs.to_text().lines() {
            let (k, v) = line.split_once(':').unwrap();
            back.set(k.trim(), v.trim().trim_end_matches(','));
        }
        assert_eq!(back, prefs);
    }
}
