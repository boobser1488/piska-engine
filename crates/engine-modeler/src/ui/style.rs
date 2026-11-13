//! Цвета и мелкие виджеты окна моделей.
//!
//! Цвета берутся из текущего стиля egui: в редакторе и в отдельной программе это общая
//! тема Piska (её ставит хозяин окна), поэтому смена темы меняет и окно моделей.

use egui::{pos2, vec2, Color32, Rounding, FontId, Rect, Response, Sense, Stroke, Ui};

use super::icons::{self, Icon};

/// Палитра интерфейса на этот кадр.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_raised: Color32,
    pub bg_field: Color32,
    pub bg_inset: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub axis: [Color32; 3],
    pub warn: Color32,
    pub error: Color32,
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

impl Palette {
    pub fn from_ctx(ctx: &egui::Context) -> Self {
        let v = ctx.style().visuals.clone();
        let text = v.widgets.noninteractive.fg_stroke.color;
        let panel = v.panel_fill;
        Self {
            bg_app: mix(panel, Color32::BLACK, 0.35),
            bg_panel: panel,
            bg_raised: v.faint_bg_color,
            bg_field: v.widgets.inactive.bg_fill,
            bg_inset: v.extreme_bg_color,
            hover: v.widgets.hovered.bg_fill,
            active: v.widgets.active.bg_fill,
            line: v.widgets.noninteractive.bg_stroke.color,
            line_strong: v.window_stroke.color,
            text,
            text_muted: mix(text, panel, 0.33),
            text_faint: mix(text, panel, 0.58),
            accent: v.hyperlink_color,
            axis: [
                Color32::from_rgb(0xe8, 0x55, 0x55),
                Color32::from_rgb(0x74, 0xc9, 0x52),
                Color32::from_rgb(0x4f, 0x92, 0xec),
            ],
            warn: v.warn_fg_color,
            error: v.error_fg_color,
        }
    }

    pub fn accent_soft(&self) -> Color32 {
        mix(self.bg_panel, self.accent, 0.28)
    }
}

pub fn small() -> FontId {
    FontId::proportional(11.5)
}

pub fn body() -> FontId {
    FontId::proportional(13.0)
}

/// Квадратная кнопка с иконкой. `id_name` — стабильный идентификатор (тесты щёлкают по нему).
pub fn icon_button(ui: &mut Ui, id_name: &str, icon: Icon, selected: bool, enabled: bool, tooltip: &str) -> Response {
    let p = Palette::from_ctx(ui.ctx());
    let size = vec2(28.0, 28.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let id = egui::Id::new(id_name);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, id, sense);
    let bg = if selected {
        p.accent_soft()
    } else if response.is_pointer_button_down_on() && enabled {
        p.active
    } else if response.hovered() && enabled {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, Rounding::same(4.0), bg);
    if selected {
        ui.painter()
            .rect_stroke(rect, Rounding::same(4.0), Stroke::new(1.0_f32, mix(p.accent, p.bg_panel, 0.35)));
    }
    let color = if !enabled {
        p.text_faint
    } else if selected {
        Color32::WHITE
    } else if response.hovered() {
        p.text
    } else {
        p.text_muted
    };
    icons::paint(ui.painter(), rect.shrink(6.0), icon, color);
    response.on_hover_text(tooltip)
}

/// Заголовок сворачиваемого раздела правой колонки. Возвращает, открыт ли раздел.
pub fn section(ui: &mut Ui, title: &str, open: &mut bool, right: impl FnOnce(&mut Ui)) -> bool {
    let p = Palette::from_ctx(ui.ctx());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
    let id = egui::Id::new(("modeler.section", title));
    let header = ui.interact(rect.with_max_x(rect.max.x - 64.0), id, Sense::click());
    if header.clicked() {
        *open = !*open;
    }
    ui.painter().rect_filled(rect, Rounding::ZERO, p.bg_raised);
    ui.painter().line_segment(
        [pos2(rect.min.x, rect.max.y - 0.5), pos2(rect.max.x, rect.max.y - 0.5)],
        Stroke::new(1.0_f32, p.line),
    );
    let chevron = Rect::from_center_size(pos2(rect.min.x + 12.0, rect.center().y), vec2(9.0, 9.0));
    icons::paint(
        ui.painter(),
        chevron,
        if *open { Icon::ChevronDown } else { Icon::ChevronRight },
        p.text_muted,
    );
    ui.painter().text(
        pos2(rect.min.x + 22.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title.to_uppercase(),
        FontId::proportional(11.0),
        if header.hovered() { p.text } else { p.text_muted },
    );
    let mut right_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 2.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right(&mut right_ui);
    *open
}

/// Строка «подпись — содержимое» с подписью фиксированной ширины.
pub fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let p = Palette::from_ctx(ui.ctx());
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(62.0, 22.0), Sense::hover());
        ui.painter()
            .text(pos2(rect.min.x, rect.center().y), egui::Align2::LEFT_CENTER, label, small(), p.text_muted);
        add(ui);
    });
}

/// Три поля X / Y / Z с цветными метками. Возвращает, изменилось ли значение, и было ли
/// это началом / концом перетаскивания (для истории).
pub fn vec3_fields(ui: &mut Ui, id: &str, value: &mut [f32; 3], speed: f64, suffix: &str) -> FieldEdit {
    let p = Palette::from_ctx(ui.ctx());
    let mut edit = FieldEdit::default();
    // Три метки по 12 и пять промежутков между шестью элементами — поля делят остаток.
    let gaps = ui.spacing().item_spacing.x * 5.0;
    let width = ((ui.available_width() - 36.0 - gaps) / 3.0 - 0.5).floor().max(30.0);
    for axis in 0..3 {
        let (tag, _) = ui.allocate_exact_size(vec2(12.0, 20.0), Sense::hover());
        ui.painter().text(
            tag.center(),
            egui::Align2::CENTER_CENTER,
            ["X", "Y", "Z"][axis],
            FontId::proportional(10.5),
            p.axis[axis],
        );
        let response = ui.add_sized(
            [width, 20.0],
            egui::DragValue::new(&mut value[axis])
                .speed(speed)
                .max_decimals(3)
                .suffix(suffix)
                .update_while_editing(false),
        );
        let response = response.on_hover_text(format!("{id} {}: drag or type", ["X", "Y", "Z"][axis]));
        edit.merge(&response);
    }
    edit
}

/// Что произошло с полем ввода в этом кадре.
#[derive(Clone, Copy, Debug, Default)]
pub struct FieldEdit {
    pub changed: bool,
    pub started: bool,
    pub finished: bool,
}

impl FieldEdit {
    pub fn merge(&mut self, r: &Response) {
        self.changed |= r.changed();
        self.started |= r.drag_started() || r.gained_focus();
        self.finished |= r.drag_stopped() || r.lost_focus();
    }
}

/// Цветной образец (палитра). Возвращает ответ для щелчка.
pub fn swatch(ui: &mut Ui, id: egui::Id, color: Color32, size: f32, selected: bool) -> Response {
    let p = Palette::from_ctx(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    checker(ui.painter(), rect, 4.0);
    ui.painter().rect_filled(rect, Rounding::same(2.0), color);
    let stroke = if selected {
        Stroke::new(2.0_f32, Color32::WHITE)
    } else if response.hovered() {
        Stroke::new(1.0_f32, p.text)
    } else {
        Stroke::new(1.0_f32, mix(p.line_strong, Color32::BLACK, 0.3))
    };
    ui.painter().rect_stroke(rect, Rounding::same(2.0), stroke);
    response
}

/// Шахматка под прозрачным цветом.
pub fn checker(painter: &egui::Painter, rect: Rect, cell: f32) {
    painter.rect_filled(rect, Rounding::ZERO, Color32::from_gray(0x9a));
    let mut y = rect.min.y;
    let mut row = 0;
    while y < rect.max.y {
        let mut x = rect.min.x + if row % 2 == 0 { 0.0 } else { cell };
        while x < rect.max.x {
            let r = Rect::from_min_max(pos2(x, y), pos2((x + cell).min(rect.max.x), (y + cell).min(rect.max.y)));
            painter.rect_filled(r, Rounding::ZERO, Color32::from_gray(0x6e));
            x += cell * 2.0;
        }
        y += cell;
        row += 1;
    }
}

pub fn to_color32(c: crate::image::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub fn from_color32(c: Color32) -> crate::image::Rgba {
    c.to_srgba_unmultiplied()
}

/// Подпись-подсказка горячей клавиши справа в тексте подсказки: `"Move  W"`.
pub fn tip(name: &str, key: &str, what: &str) -> String {
    if key.is_empty() {
        format!("{name}\n{what}")
    } else {
        format!("{name}  ·  {key}\n{what}")
    }
}

/// Тонкий вертикальный разделитель в горизонтальной панели.
pub fn separator(ui: &mut Ui) {
    let p = Palette::from_ctx(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().line_segment(
        [pos2(rect.center().x, rect.min.y + 3.0), pos2(rect.center().x, rect.max.y - 3.0)],
        Stroke::new(1.0_f32, p.line_strong),
    );
}
