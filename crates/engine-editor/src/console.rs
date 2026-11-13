//! Окно Console: лог движка, игры и сборки.
//!
//! Фильтры по уровню со счётчиками, поиск, схлопывание повторов, автопрокрутка.
//! Щелчок по строке показывает сообщение целиком; если в нём есть `файл:строка`
//! (ошибка компилятора), файл открывается во вкладке Code на этой строке.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine_core::logging::{self, Level, LogRecord};

use crate::icons::{self, Icon};
use crate::state::{Action, Editor};
use crate::theme;

pub(crate) struct ConsoleState {
    records: Vec<LogRecord>,
    next_seq: u64,
    pub show_info: bool,
    pub show_warn: bool,
    pub show_error: bool,
    pub filter: String,
    /// Одинаковые сообщения — одной строкой со счётчиком.
    pub collapse: bool,
    /// Держать список прокрученным к последней записи.
    pub auto_scroll: bool,
    /// Выбранная запись (её `seq`).
    pub selected: Option<u64>,
    /// Error Pause: новая ошибка в режиме Play ставит игру на паузу (как в Unity).
    pub error_pause: bool,
    /// Сколько ошибок уже видели — чтобы заметить новую.
    errors_seen: usize,
    /// Сколько последних записей держать (Preferences → Console & Diagnostics).
    max: usize,
}

impl Default for ConsoleState {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            next_seq: 0,
            show_info: true,
            show_warn: true,
            show_error: true,
            filter: String::new(),
            collapse: false,
            auto_scroll: true,
            selected: None,
            error_pause: false,
            errors_seen: 0,
            max: logging::CAPACITY,
        }
    }
}

pub(crate) fn level_style(level: Level) -> (Icon, Color32) {
    match level {
        Level::Error => (Icon::Error, theme::ERROR),
        Level::Warn => (Icon::Warn, theme::WARN),
        _ => (Icon::Info, theme::TEXT_MUTED),
    }
}

impl ConsoleState {
    /// Забрать новые записи из лога движка. `true` — что-то пришло.
    pub fn poll(&mut self) -> bool {
        let fresh = logging::records_since(self.next_seq);
        let Some(last) = fresh.last() else {
            return false;
        };
        self.next_seq = last.seq + 1;
        self.records.extend(fresh);
        self.trim();
        true
    }

    /// Держать не больше `max` последних записей (не больше, чем хранит лог движка).
    pub fn set_max(&mut self, max: usize) {
        self.max = max.clamp(1, logging::CAPACITY);
        self.trim();
    }

    /// Сколько записей держит окно.
    pub fn shown(&self) -> usize {
        self.records.len()
    }

    fn trim(&mut self) {
        let overflow = self.records.len().saturating_sub(self.max);
        self.records.drain(..overflow);
    }

    /// Пришли ли ошибки с прошлого вызова (для Error Pause).
    pub fn take_new_errors(&mut self) -> bool {
        let errors = self.count(Level::Error);
        let fresh = errors > self.errors_seen;
        self.errors_seen = errors;
        fresh
    }

    pub fn last(&self) -> Option<&LogRecord> {
        self.records.last()
    }

    /// Сколько записей уровня `level` (Info включает всё, что ниже).
    pub fn count(&self, level: Level) -> usize {
        self.records
            .iter()
            .filter(|record| match level {
                Level::Error | Level::Warn => record.level == level,
                _ => record.level >= Level::Info,
            })
            .count()
    }

    pub(crate) fn clear(&mut self) {
        self.records.clear();
        self.selected = None;
        self.errors_seen = 0;
    }

    /// Видимые строки: (индекс записи, сколько раз повторилась).
    fn visible(&self) -> Vec<(usize, usize)> {
        let filter = self.filter.trim().to_lowercase();
        let passes = |record: &LogRecord| {
            let level_ok = match record.level {
                Level::Error => self.show_error,
                Level::Warn => self.show_warn,
                _ => self.show_info,
            };
            level_ok
                && (filter.is_empty()
                    || record.message.to_lowercase().contains(&filter)
                    || record.target.to_lowercase().contains(&filter))
        };
        let mut rows: Vec<(usize, usize)> = Vec::new();
        if self.collapse {
            let mut seen: HashMap<(Level, &str), usize> = HashMap::new();
            for (index, record) in self.records.iter().enumerate().filter(|(_, r)| passes(r)) {
                match seen.get(&(record.level, record.message.as_str())) {
                    Some(row) => rows[*row].1 += 1,
                    None => {
                        seen.insert((record.level, record.message.as_str()), rows.len());
                        rows.push((index, 1));
                    }
                }
            }
        } else {
            rows = self
                .records
                .iter()
                .enumerate()
                .filter(|(_, r)| passes(r))
                .map(|(index, _)| (index, 1))
                .collect();
        }
        rows
    }
}

/// Место в исходнике из сообщения компилятора: `src/player.rs:12:5`, `gameplay.cpp(40)`.
pub(crate) fn source_location(message: &str) -> Option<(String, usize)> {
    for token in message.split_whitespace() {
        let token = token.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '\'' | '"' | '`'));
        let lower = token.to_ascii_lowercase();
        let extensions = [".rs:", ".cpp:", ".hpp:", ".h:", ".cc:", ".c:", ".wgsl:", ".scenario:", ".cpp(", ".h(", ".hpp("];
        let Some(at) = extensions.iter().filter_map(|ext| lower.find(ext).map(|i| i + ext.len() - 1)).min() else {
            continue;
        };
        let path = &token[..at];
        let digits: String = token[at + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(line) = digits.parse::<usize>() {
            return Some((path.to_string(), line));
        }
    }
    None
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    let errors = editor.console.count(Level::Error);
    let warnings = editor.console.count(Level::Warn);
    let infos = editor.console.count(Level::Info);
    {
        let console = &mut editor.console;
        theme::toolbar_strip(ui, |ui| {
            if theme::icon_text_button(ui, Icon::Trash, "Clear").on_hover_text("Remove all messages").clicked() {
                console.clear();
            }
            theme::toolbar_separator(ui);
            theme::toggle_button(
                ui,
                &mut console.collapse,
                Icon::Collapse,
                "Collapse",
                "Show identical messages once, with a count",
            );
            theme::toggle_button(
                ui,
                &mut console.error_pause,
                Icon::Pause,
                "Error Pause",
                "Pause the game when an error is logged in Play mode",
            );
            theme::toggle_button(
                ui,
                &mut console.auto_scroll,
                Icon::ArrowDown,
                "Auto-scroll",
                "Keep the newest message in view",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                theme::pill_toggle(ui, &mut console.show_error, Icon::Error, theme::ERROR, &errors.to_string(), "Show errors");
                theme::pill_toggle(ui, &mut console.show_warn, Icon::Warn, theme::WARN, &warnings.to_string(), "Show warnings");
                theme::pill_toggle(ui, &mut console.show_info, Icon::Info, theme::INFO, &infos.to_string(), "Show messages");
                ui.add_space(theme::SP_S);
                theme::search_field(ui, &mut console.filter, "Search messages", 240.0);
            });
        });
    }

    let rows = editor.console.visible();
    let selected = editor
        .console
        .selected
        .and_then(|seq| editor.console.records.iter().find(|r| r.seq == seq))
        .cloned();

    // Подробности выбранной записи — внизу окна.
    if let Some(record) = &selected {
        egui::TopBottomPanel::bottom("console_details")
            .resizable(true)
            .default_height(110.0)
            .height_range(64.0..=400.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(theme::SP_M, theme::SP_S + 2.0)),
            )
            .show_inside(ui, |ui| details(editor, ui, record));
    }

    if rows.is_empty() {
        let (title, hint) = if editor.console.records.is_empty() {
            ("No messages", "Logs from the engine, your game and builds appear here")
        } else {
            ("Nothing matches", "Change the search text or turn the level filters back on")
        };
        theme::empty_state(ui, Icon::Console, title, hint);
        return;
    }

    let console = &mut editor.console;
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("console_scroll")
        .auto_shrink([false, false]);
    if console.auto_scroll {
        scroll = scroll.stick_to_bottom(true);
    }
    let mut clicked = None;
    let mut opened = None;
    scroll.show_rows(ui, theme::ROW, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in range {
            let (index, count) = rows[row];
            let record = &console.records[index];
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click());
            let is_selected = console.selected == Some(record.seq);
            theme::list_row(ui.painter(), rect, is_selected, true, response.hovered());
            ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::mix(theme::BG_PANEL, theme::LINE, 0.5)));
            let (icon, color) = level_style(record.level);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
                icon,
                color,
            );
            // Справа: источник и счётчик повторов.
            let mut right = rect.right() - theme::SP_M;
            if count > 1 {
                let galley = ui
                    .painter()
                    .layout_no_wrap(count.to_string(), egui::FontId::proportional(10.5), theme::TEXT_STRONG);
                let pill = Rect::from_min_size(
                    pos2(right - galley.size().x - 8.0, rect.center().y - 7.0),
                    vec2((galley.size().x + 8.0).max(16.0), 14.0),
                );
                ui.painter().rect_filled(pill, 7.0, theme::BG_ACTIVE);
                ui.painter().galley(pill.center() - galley.size() * 0.5, galley, theme::TEXT_STRONG);
                right = pill.left() - theme::SP_M;
            }
            let target = ui
                .painter()
                .layout_no_wrap(record.target.clone(), theme::caption(), theme::TEXT_DISABLED);
            let target_width = target.size().x.min(220.0);
            let target_pos = pos2(right - target_width, rect.center().y - target.size().y * 0.5);
            ui.painter()
                .with_clip_rect(Rect::from_min_size(target_pos, vec2(target_width, rect.height())))
                .galley(target_pos, target, theme::TEXT_DISABLED);
            right = target_pos.x - theme::SP_L;

            let first_line = record.message.lines().next().unwrap_or_default();
            let more = record.message.lines().nth(1).is_some();
            let text_color = match record.level {
                Level::Error => theme::mix(theme::TEXT, theme::ERROR, 0.3),
                Level::Warn => theme::mix(theme::TEXT, theme::WARN, 0.2),
                _ => theme::TEXT,
            };
            let font = if record.target == "build" { theme::mono() } else { theme::body() };
            let text = if more { format!("{first_line}  …") } else { first_line.to_string() };
            let clip = Rect::from_min_max(pos2(rect.left() + 26.0, rect.top()), pos2(right, rect.bottom()));
            ui.painter().with_clip_rect(clip).text(
                pos2(clip.left(), rect.center().y),
                Align2::LEFT_CENTER,
                text,
                font,
                text_color,
            );
            if response.clicked() {
                clicked = Some(record.seq);
            }
            if response.double_clicked() {
                opened = source_location(&record.message);
            }
        }
    });
    if let Some(seq) = clicked {
        console.selected = if console.selected == Some(seq) { None } else { Some(seq) };
    }
    // Двойной щелчок по ошибке компилятора — сразу к строке в исходнике.
    if let Some((path, line)) = opened {
        if let Some(file) = resolve_source(editor, &path) {
            editor.actions.push(Action::OpenCode(file.clone()));
            editor.code.goto_line(&file, line);
        }
    }
}

fn details(editor: &mut Editor, ui: &mut egui::Ui, record: &LogRecord) {
    let (icon, color) = level_style(record.level);
    let location = source_location(&record.message).and_then(|(path, line)| {
        let file = resolve_source(editor, &path)?;
        Some((file, line))
    });
    ui.horizontal(|ui| {
        icons::inline_small(ui, icon, color);
        ui.label(egui::RichText::new(format!("{}", record.level)).font(theme::strong(theme::FONT_CAPTION)).color(theme::TEXT));
        ui.label(egui::RichText::new(&record.target).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Close, "Close details").clicked() {
                editor.console.selected = None;
            }
            if theme::icon_text_button(ui, Icon::Copy, "Copy").clicked() {
                ui.ctx().copy_text(record.message.clone());
            }
            if let Some((file, line)) = &location {
                let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if theme::icon_text_button(ui, Icon::Code, &format!("Open {name}:{line}"))
                    .on_hover_text("Open the file in the Code tab at this line")
                    .clicked()
                {
                    editor.actions.push(Action::OpenCode(file.clone()));
                    editor.code.goto_line(file, *line);
                }
            }
        });
    });
    ui.add_space(theme::SP_S);
    egui::ScrollArea::vertical()
        .id_salt("console_details_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&record.message).font(theme::mono()).color(theme::TEXT))
                    .wrap()
                    .selectable(true),
            );
        });
    let rect = ui.max_rect();
    ui.painter()
        .hline(rect.x_range().expand(theme::SP_M), rect.top() - theme::SP_S - 2.0, Stroke::new(1.0_f32, theme::LINE));
}

/// Найти файл из сообщения компилятора: как есть, в каталогах кода, рядом с ними.
fn resolve_source(editor: &Editor, path: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate);
    }
    let mut roots: Vec<PathBuf> = editor.config.code_dirs.clone();
    roots.extend(editor.config.code_dirs.iter().filter_map(|dir| dir.parent().map(PathBuf::from)));
    roots.extend(editor.config.build_commands.iter().filter_map(|c| c.dir.clone()));
    // Сценарии (`scenarios/intro.scenario:3:5`) — относительно каталога ассетов.
    if let Some(db) = editor.app.world.resources.get::<engine_assets::AssetDatabase>() {
        roots.push(db.root().to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    roots
        .into_iter()
        .map(|root| root.join(path))
        .find(|file| file.is_file())
        .or_else(|| {
            // По имени файла среди известных исходников.
            let name = candidate.file_name()?;
            editor
                .code
                .files
                .iter()
                .find(|(_, file)| file.file_name() == Some(name))
                .map(|(_, file)| file.clone())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_locations_are_found() {
        assert_eq!(
            source_location("  --> examples/platformer/scripts.rs:42:17"),
            Some(("examples/platformer/scripts.rs".to_string(), 42))
        );
        assert_eq!(
            source_location("C:/game/gameplay.cpp(40): error C2065: 'x': undeclared identifier"),
            Some(("C:/game/gameplay.cpp".to_string(), 40))
        );
        assert_eq!(
            source_location("gameplay.cpp:12:3: error: expected ';'"),
            Some(("gameplay.cpp".to_string(), 12))
        );
        assert_eq!(source_location("play: `Level 1`"), None);
        // Ошибки сценариев Scenario.
        assert_eq!(
            source_location("scenarios/intro.scenario:12:5: `prnt` is not defined"),
            Some(("scenarios/intro.scenario".to_string(), 12))
        );
    }
}
