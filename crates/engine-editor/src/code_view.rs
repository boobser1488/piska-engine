//! Как вкладка Code показывает диагностики (как Visual Studio / VS Code): волнистое
//! подчёркивание точного диапазона (красное — ошибка, янтарное — предупреждение, синее —
//! сообщение; устаревшие — бледнее), значки в колонке номеров строк, отметки на полосе
//! обзора справа (диагностики, совпадения поиска, закладки, строка курсора),
//! подсказка при наведении и меню быстрых исправлений.

use crate::i18n::tr_s;
use std::ops::Range;
use std::sync::Arc;

use egui::epaint::text::cursor::PCursor;
use egui::{pos2, vec2, Color32, Galley, Pos2, Rect, Sense, Shape, Stroke};

use crate::code_diagnostics::{Diagnostic, LineIndex, QuickFix, Severity};
use crate::icons::{self, Icon};
use crate::theme;

/// Цвет важности.
pub(crate) fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Error => theme::ERROR,
        Severity::Warning => theme::WARN,
        Severity::Info => theme::INFO,
    }
}

pub(crate) fn severity_icon(severity: Severity) -> Icon {
    match severity {
        Severity::Error => Icon::Error,
        Severity::Warning => Icon::Warn,
        Severity::Info => Icon::Info,
    }
}

/// Разложенный текст редактора и где он на экране.
pub(crate) struct Laid<'a> {
    pub galley: &'a Arc<Galley>,
    pub origin: Pos2,
    pub text: &'a str,
    pub index: &'a LineIndex,
}

impl Laid<'_> {
    /// Строки галереи совпадают со строками текста (перенос выключен).
    fn unwrapped(&self) -> bool {
        self.galley.rows.len() == self.index.lines()
    }

    /// Прямоугольник символа (высота строки) по байту `offset`, в координатах экрана.
    pub fn char_rect(&self, offset: usize) -> Rect {
        let (line, column) = self.index.line_col(self.text, offset);
        let rect = if self.unwrapped() {
            let row = &self.galley.rows[line.min(self.galley.rows.len() - 1)];
            let x = row.x_offset(column);
            Rect::from_min_max(pos2(x, row.min_y()), pos2(x, row.max_y()))
        } else {
            self.galley.pos_from_pcursor(PCursor { paragraph: line, offset: column, prefer_next_row: false })
        };
        rect.translate(self.origin.to_vec2())
    }

    /// Байт под точкой экрана.
    pub fn offset_at(&self, pos: Pos2) -> usize {
        let cursor = self.galley.cursor_from_pos(pos - self.origin);
        self.index.offset(self.text, cursor.pcursor.paragraph, cursor.pcursor.offset)
    }

    /// Строки (с нуля), которые видны в `clip`.
    pub fn visible_lines(&self, clip: Rect) -> Range<usize> {
        let rows = &self.galley.rows;
        if rows.is_empty() {
            return 0..0;
        }
        if self.unwrapped() {
            let h = rows[0].height().max(1.0);
            let first = ((clip.top() - self.origin.y) / h).floor().max(0.0) as usize;
            let last = ((clip.bottom() - self.origin.y) / h).ceil().max(0.0) as usize + 1;
            return first.min(rows.len())..last.min(rows.len());
        }
        let top = self.galley.cursor_from_pos(vec2(0.0, clip.top() - self.origin.y)).pcursor.paragraph;
        let bottom = self.galley.cursor_from_pos(vec2(0.0, clip.bottom() - self.origin.y)).pcursor.paragraph;
        top..bottom + 1
    }

    /// Верх и низ строки текста `line` на экране (первый ряд строки при переносе).
    pub fn line_span(&self, line: usize) -> (f32, f32) {
        let rect = self.char_rect(self.index.line_start(line));
        (rect.top(), rect.bottom())
    }
}

/// Волнистая линия под `x0..x1` у низа строки.
fn wave(painter: &egui::Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    let x1 = x1.max(x0 + 4.0);
    let step = 2.0;
    let mut points = Vec::with_capacity(((x1 - x0) / step) as usize + 2);
    let mut x = x0;
    let mut up = true;
    while x <= x1 {
        points.push(pos2(x, if up { y - 1.5 } else { y + 0.5 }));
        x += step;
        up = !up;
    }
    painter.add(Shape::line(points, Stroke::new(1.0, color)));
}

/// Подчеркнуть диагностики, видимые в `clip`.
pub(crate) fn paint_squiggles(painter: &egui::Painter, laid: &Laid<'_>, diagnostics: &[&Diagnostic], clip: Rect) {
    let visible = laid.visible_lines(clip);
    // Сначала менее важные: ошибка рисуется поверх предупреждения на том же месте.
    let mut order: Vec<&&Diagnostic> = diagnostics.iter().collect();
    order.sort_by_key(|d| std::cmp::Reverse(d.severity));
    for d in order {
        let first = laid.index.line_of(d.range.start);
        let last = laid.index.line_of(d.range.end.max(d.range.start));
        if last < visible.start || first >= visible.end {
            continue;
        }
        let mut color = severity_color(d.severity);
        if d.stale {
            color = color.gamma_multiply(0.45);
        }
        for line in first.max(visible.start)..=last.min(visible.end.saturating_sub(1)) {
            let start = if line == first { d.range.start } else { laid.index.line_start(line) };
            let end = if line == last { d.range.end } else { laid.index.line_end(laid.text, line) };
            let a = laid.char_rect(start);
            let b = laid.char_rect(end.max(start));
            let x1 = if b.top() > a.top() + 1.0 { a.left() + 8.0 } else { b.left() };
            wave(painter, a.left(), x1.max(a.left() + 6.0), a.bottom() - 1.0, color);
        }
    }
}

/// Значки диагностик в колонке номеров строк: самый важный на строке.
pub(crate) fn paint_gutter_marks(painter: &egui::Painter, laid: &Laid<'_>, diagnostics: &[&Diagnostic], x: f32, clip: Rect) {
    let visible = laid.visible_lines(clip);
    let mut worst: std::collections::BTreeMap<usize, (Severity, bool)> = std::collections::BTreeMap::new();
    for d in diagnostics {
        let line = laid.index.line_of(d.range.start);
        if visible.contains(&line) {
            let entry = worst.entry(line).or_insert((d.severity, d.stale));
            if d.severity < entry.0 {
                *entry = (d.severity, d.stale);
            }
        }
    }
    for (line, (severity, stale)) in worst {
        let (top, bottom) = laid.line_span(line);
        let center = pos2(x, (top + bottom) * 0.5);
        let mut color = severity_color(severity);
        if stale {
            color = color.gamma_multiply(0.45);
        }
        icons::paint(painter, Rect::from_center_size(center, vec2(theme::ICON_S, theme::ICON_S)), severity_icon(severity), color);
    }
}

/// Отметка на полосе обзора.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RulerMark {
    pub line: usize,
    pub color: Color32,
    /// Ширина в долях полосы: диагностики шире совпадений поиска.
    pub lane: Lane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    /// Левая треть: закладки.
    Left,
    /// Середина: совпадения поиска.
    Center,
    /// Правая треть: диагностики.
    Right,
    /// Во всю ширину: строка курсора.
    Full,
}

/// Полоса обзора справа от текста. Щелчок — строка, к которой перейти.
pub(crate) fn overview_ruler(ui: &mut egui::Ui, rect: Rect, lines: usize, marks: &[RulerMark], view: Range<usize>, id: egui::Id) -> Option<usize> {
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::BG_PANEL);
    painter.vline(rect.left(), rect.y_range(), Stroke::new(1.0, theme::LINE));
    let lines = lines.max(1) as f32;
    let y_of = |line: usize| rect.top() + 2.0 + (rect.height() - 4.0) * (line as f32 / lines);
    // Видимая часть текста.
    let (a, b) = (y_of(view.start), y_of(view.end.max(view.start + 1)));
    painter.rect_filled(Rect::from_min_max(pos2(rect.left() + 1.0, a), pos2(rect.right(), b.max(a + 4.0))), 0.0, theme::BG_HOVER.gamma_multiply(0.6));
    let w = rect.width() - 1.0;
    for mark in marks {
        let (x0, x1) = match mark.lane {
            Lane::Left => (rect.left() + 1.0, rect.left() + 1.0 + w / 3.0),
            Lane::Center => (rect.left() + 1.0 + w / 3.0, rect.left() + 1.0 + w * 2.0 / 3.0),
            Lane::Right => (rect.left() + 1.0 + w * 2.0 / 3.0 - 1.0, rect.right()),
            Lane::Full => (rect.left() + 1.0, rect.right()),
        };
        let y = y_of(mark.line);
        let h = if mark.lane == Lane::Full { 1.0 } else { 3.0 };
        painter.rect_filled(Rect::from_min_max(pos2(x0, y - h * 0.5), pos2(x1, y + h * 0.5)), 0.0, mark.color);
    }
    let response = response.on_hover_text(tr_s("Overview: errors, warnings, find matches and bookmarks of the whole file. Click to jump"));
    if response.clicked() || response.dragged() {
        let pos = response.interact_pointer_pos()?;
        let t = ((pos.y - rect.top() - 2.0) / (rect.height() - 4.0)).clamp(0.0, 1.0);
        return Some((t * lines) as usize);
    }
    None
}

/// Подсказка диагностики (как при наведении в VS): значок, код и текст, подсказка,
/// заметки и «Quick Fix…».
pub(crate) fn diagnostic_tooltip(ui: &mut egui::Ui, d: &Diagnostic, fix_keys: &str) {
    ui.set_max_width(520.0);
    ui.horizontal(|ui| {
        let mut color = severity_color(d.severity);
        if d.stale {
            color = color.gamma_multiply(0.6);
        }
        icons::inline(ui, severity_icon(d.severity), color);
        if let Some(code) = &d.code {
            ui.label(egui::RichText::new(code).font(theme::mono()).color(color));
        }
        ui.label(egui::RichText::new(crate::code_checks::origin_label(d.origin)).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
    });
    ui.label(egui::RichText::new(&d.message).color(theme::TEXT_STRONG));
    if let Some(hint) = &d.hint {
        ui.label(egui::RichText::new(crate::i18n::trf!("hint: {hint}", hint = hint)).color(theme::TEXT));
    }
    for note in d.notes.iter().take(6) {
        ui.label(egui::RichText::new(format!("• {note}")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
    }
    if d.stale {
        ui.label(egui::RichText::new(tr_s("The text changed since this check; save to check again.")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
    }
    if !d.fixes.is_empty() {
        ui.horizontal(|ui| {
            icons::inline(ui, Icon::Bulb, theme::WARN);
            ui.label(egui::RichText::new(crate::i18n::trf!("Quick Fix… ({keys})", keys = fix_keys)).color(theme::accent()));
        });
    }
}

/// Открытое меню быстрых исправлений.
#[derive(Clone, Debug)]
pub(crate) struct QuickFixMenu {
    pub path: std::path::PathBuf,
    pub at: Pos2,
    pub fixes: Vec<QuickFix>,
    pub selected: usize,
}

/// Нарисовать меню быстрых исправлений. Возвращает выбранное исправление; `Err(())` —
/// меню закрыли.
pub(crate) fn quick_fix_menu(ctx: &egui::Context, menu: &mut QuickFixMenu) -> Result<Option<QuickFix>, ()> {
    let mut chosen = None;
    let mut close = false;
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    let n = menu.fixes.len().max(1);
    if up {
        menu.selected = (menu.selected + n - 1) % n;
    }
    if down {
        menu.selected = (menu.selected + 1) % n;
    }
    if enter {
        chosen = menu.fixes.get(menu.selected).cloned();
    }
    if escape {
        close = true;
    }
    let area = egui::Area::new(egui::Id::new("code_quick_fix"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(theme::BG_RAISED).stroke(Stroke::new(1.0, theme::LINE_STRONG)).show(ui, |ui| {
                theme::menu_body(ui);
                ui.set_min_width(260.0);
                for (k, fix) in menu.fixes.iter().enumerate() {
                    let selected = k == menu.selected;
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(260.0), theme::MENU_ROW), Sense::click());
                    if selected || response.hovered() {
                        ui.painter().rect_filled(rect, theme::R_S, theme::selection_fill());
                    }
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + theme::SP_6, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)), Icon::Bulb, theme::WARN);
                    ui.painter().text(pos2(rect.left() + theme::SP_6 * 2.0, rect.center().y), egui::Align2::LEFT_CENTER, &fix.title, theme::body(), theme::TEXT);
                    if response.clicked() {
                        chosen = Some(fix.clone());
                    }
                }
            });
        });
    if area.response.clicked_elsewhere() {
        close = true;
    }
    if chosen.is_some() {
        return Ok(chosen);
    }
    if close {
        return Err(());
    }
    Ok(None)
}
