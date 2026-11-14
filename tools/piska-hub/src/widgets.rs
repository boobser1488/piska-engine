//! Виджеты хаба поверх визуальной системы редактора: значки-метки, миниатюры,
//! картинки шаблонов, навигация, полоса хода, компактные кнопки.

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};
use engine_editor::kit::icons::{self, Icon};
use engine_editor::kit::controls::{self, Kind};
use engine_editor::kit::theme;

use crate::templates::Template;

/// Тон метки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Ok,
    Warn,
    Error,
}

impl Tone {
    fn colors(self) -> (Color32, Color32, Color32) {
        // (фон, рамка, текст)
        let tinted = |c: Color32| (theme::mix(theme::BG_PANEL, c, 0.16), theme::mix(theme::BG_PANEL, c, 0.45), theme::mix(c, Color32::WHITE, 0.25));
        match self {
            Tone::Neutral => (theme::BG_RAISED, theme::LINE_STRONG, theme::TEXT_MUTED),
            Tone::Accent => tinted(theme::accent()),
            Tone::Ok => tinted(theme::OK),
            Tone::Warn => tinted(theme::WARN),
            Tone::Error => tinted(theme::ERROR),
        }
    }
}

/// Шрифт меток.
fn badge_font() -> FontId {
    theme::strong(10.5)
}

/// Ширина метки.
pub fn badge_width(painter: &egui::Painter, text: &str) -> f32 {
    painter.layout_no_wrap(text.to_string(), badge_font(), Color32::WHITE).size().x + 10.0
}

/// Нарисовать метку (`2D`, `C++`, `Missing`) с левым краем в `left`, по центру `y`.
/// Возвращает её прямоугольник.
pub fn paint_badge(painter: &egui::Painter, left: f32, y: f32, text: &str, tone: Tone) -> Rect {
    let (fill, stroke, color) = tone.colors();
    let galley = painter.layout_no_wrap(text.to_string(), badge_font(), color);
    let rect = Rect::from_min_size(pos2(left, y - 9.0), vec2(galley.size().x + 10.0, 18.0));
    painter.rect(rect, theme::R_S, fill, Stroke::new(1.0_f32, stroke));
    painter.galley(pos2(rect.left() + 5.0, rect.center().y - galley.size().y * 0.5), galley, color);
    rect
}

/// Метка в строке раскладки: занимает своё место (в отличие от `paint_badge`).
pub fn paint_badge_inline(ui: &mut Ui, text: &str, tone: Tone) {
    let width = badge_width(ui.painter(), text);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    paint_badge(ui.painter(), rect.left(), rect.center().y, text, tone);
}

/// Текст в одну строку, обрезанный многоточием по ширине.
pub fn paint_elided(painter: &egui::Painter, pos: Pos2, text: &str, font: FontId, color: Color32, max_width: f32, align: Align2) -> Rect {
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat {
            font_id: font,
            color,
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping {
        max_width: max_width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// Подпись в одну строку не шире `max_width` (с «…»); обрезанная — с полным текстом во
/// всплывающей подсказке. Прямоугольник регистрируется под `id` (по нему проверяют, что
/// текст не заходит под кнопки). Меньше 24 пикселей места — подпись не рисуется.
#[allow(clippy::too_many_arguments)]
pub fn caption_elided(
    ui: &Ui,
    id: egui::Id,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
    align: Align2,
) -> Option<Rect> {
    if max_width < 24.0 || text.is_empty() {
        return None;
    }
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat {
            font_id: font,
            color,
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping {
        max_width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    let rect = align.anchor_size(pos, galley.size());
    ui.painter().galley(rect.min, galley, color);
    let response = ui.interact(rect, id, Sense::hover());
    if elided {
        response.on_hover_text(text.to_string());
    }
    Some(rect)
}

/// Детерминированный оттенок по имени (для сгенерированных миниатюр).
fn hue_of(name: &str) -> f32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    (hash % 360) as f32 / 360.0
}

fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    let rgb = egui::ecolor::Hsva::new(h, s, v, 1.0).to_srgb();
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Буквы для миниатюры: `lantern_keeper` → `LK`, `swarm` → `SW`.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let letters: String = match words.as_slice() {
        [] => "?".to_string(),
        [one] => one.chars().take(2).collect(),
        [first, second, ..] => first.chars().take(1).chain(second.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

/// Сгенерированная миниатюра: приглушённый цвет из имени, инициалы, тонкая сетка.
pub fn paint_tile(painter: &egui::Painter, rect: Rect, name: &str, greyed: bool) {
    let hue = hue_of(name);
    let (base, ink) = if greyed {
        (theme::BG_RAISED, theme::TEXT_DISABLED)
    } else {
        // Приглушённый тёмный тон и светлые инициалы того же оттенка — как значки проектов в IDE.
        (hsv(hue, 0.32, 0.25), hsv(hue, 0.22, 0.86))
    };
    painter.rect_filled(rect, theme::R_S, base);
    // Полоса снизу немного темнее: плитка не выглядит плоской заливкой.
    let band = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rect.height() * 0.22), rect.max);
    painter.rect_filled(
        band,
        egui::Rounding {
            nw: 0.0,
            ne: 0.0,
            sw: theme::R_S,
            se: theme::R_S,
        },
        Color32::from_black_alpha(if greyed { 20 } else { 46 }),
    );
    let size = (rect.height() * 0.30).clamp(10.0, 36.0);
    painter.text(rect.center() - vec2(0.0, rect.height() * 0.06), Align2::CENTER_CENTER, initials(name), theme::strong(size), ink);
    painter.rect_stroke(rect, theme::R_S, Stroke::new(1.0_f32, Color32::from_white_alpha(14)));
}

/// Картинка, вписанная «с обрезкой» в прямоугольник (как `object-fit: cover`).
pub fn paint_cover(ui: &Ui, rect: Rect, texture: &egui::TextureHandle, rounding: f32, tint: Color32) {
    let [w, h] = texture.size();
    let (w, h) = (w as f32, h as f32);
    let want = rect.width() / rect.height();
    let have = w / h.max(1.0);
    let uv = if have > want {
        let fraction = want / have;
        Rect::from_min_max(pos2(0.5 - fraction * 0.5, 0.0), pos2(0.5 + fraction * 0.5, 1.0))
    } else {
        let fraction = have / want;
        Rect::from_min_max(pos2(0.0, 0.5 - fraction * 0.5), pos2(1.0, 0.5 + fraction * 0.5))
    };
    egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
        .uv(uv)
        .rounding(rounding)
        .tint(tint)
        .paint_at(ui, rect);
    ui.painter()
        .rect_stroke(rect, rounding, Stroke::new(1.0_f32, Color32::from_white_alpha(14)));
}

/// Звезда «закреплено». `filled` — закреплено.
pub fn paint_star(painter: &egui::Painter, center: Pos2, radius: f32, filled: bool, color: Color32) {
    let points: Vec<Pos2> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { radius } else { radius * 0.45 };
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            pos2(center.x + angle.cos() * r, center.y + angle.sin() * r)
        })
        .collect();
    if filled {
        // Невыпуклая звезда — пятью треугольниками от центра и выпуклым пятиугольником.
        let inner: Vec<Pos2> = points.iter().skip(1).step_by(2).copied().collect();
        painter.add(Shape::convex_polygon(inner.clone(), color, Stroke::NONE));
        for i in 0..5 {
            let tip = points[i * 2];
            let a = points[(i * 2 + 9) % 10];
            let b = points[i * 2 + 1];
            painter.add(Shape::convex_polygon(vec![a, tip, b], color, Stroke::NONE));
        }
    } else {
        painter.add(Shape::closed_line(points, Stroke::new(1.2_f32, color)));
    }
}

/// Картинка шаблона: векторная схема того, что будет в проекте.
pub fn paint_template_preview(painter: &egui::Painter, rect: Rect, template: Template) {
    let painter = painter.with_clip_rect(rect);
    let s = rect.height() / 72.0;
    let at = |x: f32, y: f32| pos2(rect.left() + x * s, rect.top() + y * s);
    match template {
        Template::Empty2D => {
            painter.rect_filled(rect, theme::R_S, Color32::from_rgb(0x15, 0x17, 0x1c));
            let grid = Color32::from_white_alpha(10);
            let step = 12.0 * s;
            let mut x = rect.left() + step;
            while x < rect.right() {
                painter.vline(x, rect.y_range(), Stroke::new(1.0_f32, grid));
                x += step;
            }
            let mut y = rect.top() + step;
            while y < rect.bottom() {
                painter.hline(rect.x_range(), y, Stroke::new(1.0_f32, grid));
                y += step;
            }
            // Земля и повёрнутый квадрат-спрайт.
            painter.rect_filled(Rect::from_min_max(pos2(rect.left(), at(0.0, 56.0).y), rect.right_bottom()), 0.0, Color32::from_rgb(0x33, 0x37, 0x40));
            let c = pos2(rect.center().x, at(0.0, 34.0).y);
            let half = 11.0 * s;
            let angle = 0.32_f32;
            let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(x, y): &(f32, f32)| {
                    let (sin, cos) = angle.sin_cos();
                    pos2(c.x + (x * cos - y * sin) * half, c.y + (x * sin + y * cos) * half)
                })
                .collect();
            painter.add(Shape::convex_polygon(corners, Color32::from_rgb(0x4d, 0x8c, 0xff), Stroke::NONE));
            // Рамка камеры.
            let frame = rect.shrink(7.0 * s);
            painter.rect_stroke(frame, 1.0, Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
        }
        Template::Empty3D => {
            let sky = Color32::from_rgb(0x1d, 0x26, 0x33);
            painter.rect_filled(rect, theme::R_S, sky);
            let horizon = at(0.0, 40.0).y;
            painter.rect_filled(Rect::from_min_max(pos2(rect.left(), horizon), rect.right_bottom()), 0.0, Color32::from_rgb(0x2a, 0x2d, 0x31));
            // Сетка земли в перспективе.
            let vanish = pos2(rect.center().x, horizon - 30.0 * s);
            let line = Stroke::new(1.0_f32, Color32::from_white_alpha(22));
            for i in -8..=8 {
                let bottom = pos2(rect.center().x + i as f32 * 18.0 * s, rect.bottom());
                let t = (horizon - vanish.y) / (bottom.y - vanish.y);
                let top = pos2(vanish.x + (bottom.x - vanish.x) * t, horizon);
                painter.line_segment([top, bottom], line);
            }
            for k in 1..6 {
                let y = horizon + (rect.bottom() - horizon) * (k as f32 / 6.0).powf(1.6);
                painter.hline(rect.x_range(), y, line);
            }
            // Куб: три грани разной яркости.
            let c = pos2(rect.center().x, at(0.0, 40.0).y);
            let w = 15.0 * s;
            let h = 9.0 * s;
            let top = [pos2(c.x, c.y - 2.0 * h - w * 0.0), pos2(c.x + w, c.y - h - 8.0 * s), pos2(c.x, c.y - 16.0 * s), pos2(c.x - w, c.y - h - 8.0 * s)];
            let left = [top[3], top[2], pos2(c.x, c.y + 4.0 * s), pos2(c.x - w, c.y - 6.0 * s)];
            let right = [top[2], top[1], pos2(c.x + w, c.y - 6.0 * s), pos2(c.x, c.y + 4.0 * s)];
            painter.add(Shape::convex_polygon(top.to_vec(), Color32::from_rgb(0x8f, 0xb4, 0xff), Stroke::NONE));
            painter.add(Shape::convex_polygon(left.to_vec(), Color32::from_rgb(0x4d, 0x8c, 0xff), Stroke::NONE));
            painter.add(Shape::convex_polygon(right.to_vec(), Color32::from_rgb(0x33, 0x63, 0xc4), Stroke::NONE));
            // Солнце.
            painter.circle_filled(at(110.0, 13.0), 4.5 * s, Color32::from_rgb(0xf3, 0xd9, 0x8b));
        }
        Template::Starfall => {
            painter.rect_filled(rect, theme::R_S, Color32::from_rgb(0x12, 0x16, 0x36));
            for (x, y, r) in [(18.0, 12.0, 4.0), (52.0, 26.0, 5.0), (86.0, 8.0, 3.5), (104.0, 34.0, 4.5), (34.0, 40.0, 3.0)] {
                paint_star(&painter, at(x, y), r * s, true, Color32::from_rgb(0xf5, 0xd0, 0x5a));
            }
            for (x, y) in [(10.0, 30.0), (70.0, 16.0), (120.0, 20.0), (96.0, 50.0), (26.0, 22.0)] {
                painter.circle_filled(at(x, y), 0.8 * s, Color32::from_white_alpha(90));
            }
            painter.rect_filled(Rect::from_min_max(pos2(rect.left(), at(0.0, 64.0).y), rect.right_bottom()), 0.0, Color32::from_rgb(0x1f, 0x4d, 0x29));
            // Корзина.
            let basket = vec![at(56.0, 52.0), at(80.0, 52.0), at(76.0, 63.0), at(60.0, 63.0)];
            painter.add(Shape::convex_polygon(basket, Color32::from_rgb(0xb9, 0x7a, 0x45), Stroke::NONE));
        }
    }
    painter.rect_stroke(rect, theme::R_S, Stroke::new(1.0_f32, Color32::from_white_alpha(16)));
}

/// Пункт левой навигации. `count` — число справа.
pub fn nav_item(ui: &mut Ui, id: egui::Id, icon: Icon, label: &str, count: Option<usize>, selected: bool) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::R_S + 1.0, theme::mix(theme::BG_HEADER, Color32::WHITE, 0.07));
        painter.rect_filled(
            Rect::from_min_size(pos2(rect.left(), rect.top() + 7.0), vec2(2.0, rect.height() - 14.0)),
            1.0,
            theme::accent(),
        );
    } else if response.hovered() {
        painter.rect_filled(rect, theme::R_S + 1.0, theme::mix(theme::BG_HEADER, Color32::WHITE, 0.04));
    }
    let color = if selected || response.hovered() { theme::TEXT } else { theme::TEXT_MUTED };
    let icon_color = if selected { theme::accent() } else { color };
    icons::paint(painter, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(15.0, 15.0)), icon, icon_color);
    painter.text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, label, theme::body(), color);
    if let Some(count) = count {
        painter.text(
            pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            count.to_string(),
            theme::caption(),
            theme::TEXT_DISABLED,
        );
    }
    response
}

/// Высота любого контрола хаба (кнопка, поле, список, поиск, сегменты, кнопка-иконка):
/// плотность [`theme::Density::Comfortable`] набора. Одна на весь хаб и установщик.
pub const CONTROL_H: f32 = theme::CONTROL_H_COMFORTABLE;

/// Кнопка хаба — [`controls::button_with_id`] плотности хаба; `primary` — с акцентом.
pub fn button(ui: &mut Ui, id: egui::Id, icon: Option<Icon>, label: &str, primary: bool) -> Response {
    let kind = if primary { Kind::Primary } else { Kind::Normal };
    controls::button_with_id(ui, Some(id), kind, icon, label, 0.0)
}

/// Кнопка строки действий страницы или диалога: та же высота и рамка, не уже
/// [`theme::DIALOG_BUTTON_MIN_W`].
pub fn action_button(ui: &mut Ui, id: egui::Id, icon: Option<Icon>, label: &str, primary: bool) -> Response {
    let kind = if primary { Kind::Primary } else { Kind::Normal };
    controls::button_with_id(ui, Some(id), kind, icon, label, theme::DIALOG_BUTTON_MIN_W)
}

/// Кнопка опасного действия («Удалить»): та же геометрия, другой цвет.
pub fn danger_button(ui: &mut Ui, id: egui::Id, icon: Option<Icon>, label: &str) -> Response {
    controls::button_with_id(ui, Some(id), Kind::Danger, icon, label, theme::DIALOG_BUTTON_MIN_W)
}

/// Обычная кнопка ширины `width` («Browse…» рядом с полем).
pub fn button_sized(ui: &mut Ui, id: egui::Id, label: &str, width: f32) -> Response {
    let width = width.min(ui.available_width());
    controls::button_with_id(ui, Some(id), Kind::Normal, None, label, width)
}

/// Естественная ширина [`button`] — чтобы раскладка (перенос строки кнопок, высота
/// карточки) знала размер заранее.
pub fn button_width(ui: &Ui, icon: Option<Icon>, label: &str) -> f32 {
    controls::button_width(ui, icon, label)
}

/// Ширина [`action_button`].
pub fn action_button_width(ui: &Ui, icon: Option<Icon>, label: &str) -> f32 {
    controls::button_width(ui, icon, label).max(theme::DIALOG_BUTTON_MIN_W)
}

/// Разложить кнопки шириной `widths` по строкам не шире `width` с промежутком `gap`
/// (порядок сохраняется; кнопка шире строки занимает строку одна). Правило для всех строк
/// кнопок в карточках и панелях: не влезает — перенос на следующую строку, а не выход за край.
pub fn wrap_rows(widths: &[f32], width: f32, gap: f32) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        match rows.last_mut() {
            Some(row) if used + gap + w <= width + 0.01 => {
                row.push(i);
                used += gap + w;
            }
            _ => {
                rows.push(vec![i]);
                used = *w;
            }
        }
    }
    rows
}

/// Кнопка-иконка без рамки (подсветка при наведении), сторона [`CONTROL_H`].
pub fn icon_button(ui: &mut Ui, id: egui::Id, icon: Icon, tooltip: &str) -> Response {
    controls::button_with_id(ui, Some(id), Kind::Ghost, Some(icon), "", CONTROL_H).on_hover_text(tooltip)
}

/// Однострочное поле ввода высотой [`CONTROL_H`] — [`controls::text_field_with_id`].
pub fn text_field(ui: &mut Ui, id: egui::Id, text: &mut String, hint: &str, width: f32) -> Response {
    controls::text_field_with_id(ui, Some(id), text, hint, width.max(40.0))
}

/// Поле только для чтения (путь в настройках): рамка поля ввода, текст с «…».
pub fn readonly_field(ui: &mut Ui, text: &str, width: f32) -> Response {
    let pad = theme::Density::Comfortable.pad_x();
    let (rect, response) = ui.allocate_exact_size(vec2(width.min(ui.available_width()), CONTROL_H), Sense::hover());
    controls::inset_frame(ui.painter(), rect);
    paint_elided(ui.painter(), pos2(rect.left() + pad, rect.center().y), text, theme::body(), theme::TEXT, rect.width() - pad * 2.0, Align2::LEFT_CENTER);
    response
}

/// Подпись слева от поля в строке формы: та же высота, что у поля, текст по центру строки.
pub fn field_label(ui: &mut Ui, text: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, CONTROL_H), Sense::hover());
    paint_elided(ui.painter(), pos2(rect.left(), rect.center().y), text, theme::body(), theme::TEXT_MUTED, width, Align2::LEFT_CENTER);
    response
}

/// Полоса хода: `None` — неопределённая (бегущий отрезок).
pub fn progress_bar(ui: &mut Ui, fraction: Option<f32>, failed: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_INSET);
    let color = if failed { theme::ERROR } else { theme::accent() };
    match fraction {
        Some(fraction) => {
            let width = rect.width() * fraction.clamp(0.0, 1.0);
            if width > 0.5 {
                painter.rect_filled(Rect::from_min_size(rect.min, vec2(width.max(6.0), rect.height())), 3.0, color);
            }
        }
        None => {
            let time = ui.input(|i| i.time) as f32;
            let span = rect.width() * 0.22;
            let x = rect.left() + ((time * 0.6).fract() * (rect.width() + span)) - span;
            let bar = Rect::from_min_max(pos2(x.max(rect.left()), rect.top()), pos2((x + span).min(rect.right()), rect.bottom()));
            if bar.width() > 0.0 {
                painter.rect_filled(bar, 3.0, color);
            }
            // Бегущий отрезок — ~30 кадров в секунду, не «сколько сможет» процессор.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

/// Подпись раздела: мелкие прописные.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).font(theme::strong(11.0)).color(theme::TEXT_MUTED));
}

/// Плашка-пояснение с иконкой (внимание или справка).
pub fn note(ui: &mut Ui, icon: Icon, tone: Tone, text: &str) {
    let width = ui.available_width();
    note_width(ui, icon, tone, text, width);
}

/// То же с заданной полной шириной (диалоги: окно не растягивается по содержимому).
pub fn note_width(ui: &mut Ui, icon: Icon, tone: Tone, text: &str, width: f32) {
    let (fill, stroke, color) = tone.colors();
    let ink = if tone == Tone::Neutral { theme::TEXT_MUTED } else { color };
    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, stroke))
        .rounding(theme::R_M)
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            // Полная ширина минус поля (10 + 10) и рамка.
            ui.set_width((width - 22.0).max(40.0));
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(14.0, 14.0)), icon, ink);
                ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT).font(theme::caption())).wrap());
            });
        });
}

/// Горизонтальный разделитель на всю ширину.
pub fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, theme::LINE));
}

/// `01:23` или `1:02:03`.
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_and_clock() {
        assert_eq!(initials("lantern_keeper"), "LK");
        assert_eq!(initials("Swarm"), "SW");
        assert_eq!(initials("Шаровой завод"), "ШЗ");
        assert_eq!(initials(""), "?");
        assert_eq!(clock(83), "01:23");
        assert_eq!(clock(3723), "1:02:03");
        assert_ne!(hue_of("a"), hue_of("b"));
    }
}
