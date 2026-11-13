//! Визуальная система редактора: палитра, шкалы отступов, шрифтов и скруглений,
//! и небольшие виджеты, из которых собраны все панели (вкладки, заголовки, пункты
//! меню, кнопки диалогов, поля поиска).
//!
//! Правило одно: цвета, размеры и отступы берутся отсюда, а не пишутся числами в панелях.
//! Палитра — сдержанная тёмная с одним акцентом; акцент выбирается в Preferences.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use egui::text::{LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Rect, Response, Rounding, Sense, Stroke, TextStyle, Ui};

use crate::icons::{self, Icon};

// ---------------------------------------------------------------------------
// Палитра
// ---------------------------------------------------------------------------

// Нейтральные серые без синевы; уровни близко друг к другу (docs/design/ui-guidelines.md).

/// Фон окна под панелями, строка состояния; им же видна линия между панелями.
pub const BG_APP: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
/// Полосы вкладок, меню, главная панель инструментов.
pub const BG_HEADER: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
/// Содержимое панелей.
pub const BG_PANEL: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
/// Поднятое над панелью: заголовки компонентов, меню, окна, подсказки.
pub const BG_RAISED: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2c);
/// Кнопки и выпадающие списки.
pub const BG_FIELD: Color32 = Color32::from_rgb(0x35, 0x35, 0x35);
/// Утопленные поля ввода (текст, числа, поиск).
pub const BG_INSET: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
pub const BG_HOVER: Color32 = Color32::from_rgb(0x3e, 0x3e, 0x3e);
pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x48, 0x48, 0x48);
/// Волосяные линии: между панелями, под полосами, между строками таблиц.
pub const LINE: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
/// Рамки полей и кнопок.
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x0f, 0x0f, 0x0f);
/// Светлая линия на поднятых поверхностях (разделители меню, края заголовков).
pub const LINE_SOFT: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
/// Рамка поля под курсором.
pub const LINE_HOVER: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);

pub const TEXT: Color32 = Color32::from_rgb(0xdc, 0xdc, 0xdc);
/// Текст на акценте, выбранная вкладка, имя под курсором.
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
/// Вторичный текст: подписи полей, пути, счётчики. Читается, а не «угадывается».
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
/// Недоступное.
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x6a, 0x6a, 0x6a);

pub const OK: Color32 = Color32::from_rgb(0x6a, 0xa8, 0x74);
pub const WARN: Color32 = Color32::from_rgb(0xd2, 0xa2, 0x4c);
pub const ERROR: Color32 = Color32::from_rgb(0xd6, 0x5f, 0x5a);
/// Информационные сообщения.
pub const INFO: Color32 = Color32::from_rgb(0x6f, 0x9f, 0xd2);
/// Экземпляры префабов — как в Unity, голубым.
pub const PREFAB: Color32 = Color32::from_rgb(0x6a, 0xb0, 0xe0);
/// Каталоги в окне Project — приглушённый жёлтый.
pub const FOLDER: Color32 = Color32::from_rgb(0xc8, 0xa8, 0x5a);
pub const AXIS_X: Color32 = Color32::from_rgb(0xe0, 0x52, 0x4a);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x7c, 0xc0, 0x4e);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x4a, 0x8c, 0xe6);

/// Фон вида Scene (линейное пространство — его получает рендерер).
pub const SCENE_CLEAR: [f32; 3] = [0.016, 0.016, 0.016];

/// Готовые акцентные цвета (Preferences → Accent). Все приглушённые: акцент отмечает
/// выделение и фокус, а не украшает.
pub const ACCENTS: [(&str, Color32); 5] = [
    ("Blue", Color32::from_rgb(0x47, 0x72, 0xb3)),
    ("Orange", Color32::from_rgb(0xc4, 0x75, 0x30)),
    ("Green", Color32::from_rgb(0x46, 0x8f, 0x5e)),
    ("Purple", Color32::from_rgb(0x7d, 0x66, 0xb8)),
    ("Teal", Color32::from_rgb(0x34, 0x8e, 0x96)),
];

static ACCENT: AtomicU32 = AtomicU32::new(0x4772b3);

/// Акцент: выделение, активная вкладка, главная кнопка, режим Play.
pub fn accent() -> Color32 {
    let rgb = ACCENT.load(Ordering::Relaxed);
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub fn set_accent(color: Color32) {
    let rgb = (u32::from(color.r()) << 16) | (u32::from(color.g()) << 8) | u32::from(color.b());
    ACCENT.store(rgb, Ordering::Relaxed);
}

/// Смешать два цвета: `t = 0` — `a`, `t = 1` — `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

/// Фон выделенной строки (непрозрачный: под ним бывают линии-направляющие).
pub fn selection_fill() -> Color32 {
    mix(BG_PANEL, accent(), 0.62)
}

/// Фон выделенной строки, когда окно не в фокусе (как в Unity — серый).
pub fn selection_unfocused() -> Color32 {
    mix(BG_PANEL, Color32::WHITE, 0.13)
}

/// Фон строки под курсором.
pub fn hover_fill() -> Color32 {
    mix(BG_PANEL, Color32::WHITE, 0.045)
}

/// Приглушённый акцент: включённые переключатели, подсветка цели перетаскивания.
pub fn accent_soft() -> Color32 {
    mix(BG_FIELD, accent(), 0.5)
}

/// Тонировка главной панели инструментов в режиме Play (как в Unity; сила — 0…1).
pub fn play_tint(strength: f32) -> Color32 {
    mix(BG_HEADER, accent(), 0.3 * strength.clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// Шкалы
// ---------------------------------------------------------------------------

/// Отступы: 2 · 4 · 8 · 12 · 16 · 24.
pub const SP_XS: f32 = 2.0;
pub const SP_S: f32 = 4.0;
pub const SP_M: f32 = 8.0;
pub const SP_L: f32 = 12.0;
pub const SP_XL: f32 = 16.0;
pub const SP_XXL: f32 = 24.0;

/// Скругления: контролы · плашки и миниатюры · меню и окна. У панелей — 0.
pub const R_S: f32 = 2.0;
pub const R_M: f32 = 3.0;
pub const R_L: f32 = 4.0;

/// Шрифты: подпись · основной · заголовок раздела · заголовок диалога · моно.
pub const FONT_CAPTION: f32 = 11.5;
pub const FONT_BODY: f32 = 12.5;
pub const FONT_TITLE: f32 = 12.5;
pub const FONT_HEADING: f32 = 14.0;
pub const FONT_MONO: f32 = 12.0;
/// Подписи групп прописными (`section_label`).
pub const FONT_SECTION: f32 = 10.5;

/// Высота строки списков (иерархия, проект, консоль, таблицы).
pub const ROW: f32 = 22.0;
/// Высота полосы вкладок дока.
pub const TAB_BAR: f32 = 24.0;
/// Полоса инструментов под вкладкой (`toolbar_strip`, `panel_header`).
pub const STRIP: f32 = 28.0;
/// Поле или кнопка в инспекторе и диалогах.
pub const FIELD_H: f32 = 20.0;
/// Кнопка, поиск, переключатель на полосе инструментов.
pub const CONTROL_H: f32 = 22.0;
/// Главное меню, главная панель инструментов, строка состояния.
pub const MENU_BAR: f32 = 22.0;
pub const MAIN_TOOLBAR: f32 = 32.0;
pub const STATUS_BAR: f32 = 22.0;
/// Иконка: обычная и маленькая (в полях, строке состояния).
pub const ICON: f32 = 16.0;
pub const ICON_S: f32 = 12.0;
/// Шаг вложенности дерева.
pub const INDENT: f32 = 14.0;
/// Высота пункта меню.
pub const MENU_ROW: f32 = 22.0;
/// Минимальная ширина выпадающего меню.
pub const MENU_WIDTH: f32 = 220.0;
/// Колонка иконки в меню.
const MENU_GUTTER: f32 = 26.0;

/// Ширина колонки подписей в инспекторе для панели шириной `width`.
pub fn label_width(width: f32) -> f32 {
    (width * 0.4).clamp(110.0, 180.0)
}

pub fn body() -> FontId {
    FontId::proportional(FONT_BODY)
}

pub fn caption() -> FontId {
    FontId::proportional(FONT_CAPTION)
}

pub fn mono() -> FontId {
    FontId::monospace(FONT_MONO)
}

/// Полужирный — для заголовков (если в системе нет жирного шрифта, обычный).
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG.into()))
}

const STRONG: &str = "strong";

// ---------------------------------------------------------------------------
// Шрифты
// ---------------------------------------------------------------------------

struct SystemFonts {
    regular: Option<Vec<u8>>,
    bold: Option<Vec<u8>>,
    mono: Option<Vec<u8>>,
}

/// Системные шрифты интерфейса (Segoe UI на Windows, DejaVu / Noto на Linux):
/// встроенный в egui шрифт тонкий и мелкий. Чего нет — заменяется встроенным.
fn system_fonts() -> &'static SystemFonts {
    static FONTS: OnceLock<SystemFonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let windows = std::env::var("WINDIR").map(|dir| format!("{dir}/Fonts/")).unwrap_or_default();
        let first = |candidates: &[String]| candidates.iter().find_map(|path| std::fs::read(path).ok());
        let win = |name: &str| format!("{windows}{name}");
        SystemFonts {
            regular: first(&[
                win("segoeui.ttf"),
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf".to_string(),
                "/usr/share/fonts/noto/NotoSans-Regular.ttf".to_string(),
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
                "/usr/share/fonts/TTF/DejaVuSans.ttf".to_string(),
            ]),
            bold: first(&[
                win("segoeuisb.ttf"),
                win("segoeuib.ttf"),
                "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf".to_string(),
                "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf".to_string(),
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".to_string(),
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf".to_string(),
            ]),
            mono: first(&[
                win("CascadiaMono.ttf"),
                win("consola.ttf"),
                "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf".to_string(),
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf".to_string(),
                "/usr/share/fonts/TTF/DejaVuSansMono.ttf".to_string(),
            ]),
        }
    })
}

fn install_fonts(ctx: &egui::Context) {
    let system = system_fonts();
    let mut fonts = egui::FontDefinitions::default();
    let defaults_proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let defaults_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut add = |name: &str, bytes: &Option<Vec<u8>>| -> Option<String> {
        let bytes = bytes.as_ref()?;
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(egui::FontData::from_owned(bytes.clone())));
        Some(name.to_string())
    };
    let regular = add("ui-regular", &system.regular);
    let bold = add("ui-bold", &system.bold);
    let mono = add("ui-mono", &system.mono);

    let mut proportional: Vec<String> = regular.iter().cloned().collect();
    proportional.extend(defaults_proportional.iter().cloned());
    let mut strong_family: Vec<String> = bold.or(regular).into_iter().collect();
    strong_family.extend(defaults_proportional);
    let mut monospace: Vec<String> = mono.into_iter().collect();
    monospace.extend(defaults_mono);

    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts.families.insert(FontFamily::Name(STRONG.into()), strong_family);
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Стиль egui
// ---------------------------------------------------------------------------

/// Применить тему к контексту (шрифты ставятся один раз; цвета — при каждом вызове,
/// чтобы смена акцента в Preferences сразу была видна).
pub fn apply(ctx: &egui::Context, with_fonts: bool) {
    if with_fonts {
        install_fonts(ctx);
    }
    let accent = accent();
    let mut style = (*ctx.style()).clone();

    style.text_styles = [
        (TextStyle::Small, FontId::proportional(FONT_CAPTION)),
        (TextStyle::Body, FontId::proportional(FONT_BODY)),
        (TextStyle::Button, FontId::proportional(FONT_BODY)),
        (TextStyle::Heading, strong(FONT_HEADING)),
        (TextStyle::Monospace, FontId::monospace(FONT_MONO)),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(SP_S + 2.0, SP_S);
    spacing.button_padding = vec2(SP_M, 2.0);
    spacing.interact_size = vec2(36.0, FIELD_H);
    spacing.indent = INDENT;
    spacing.menu_margin = egui::Margin::same(SP_S);
    spacing.window_margin = egui::Margin::same(SP_L);
    spacing.combo_height = 320.0;
    spacing.menu_width = MENU_WIDTH + 8.0;
    spacing.icon_width = 13.0;
    spacing.icon_width_inner = 7.0;
    spacing.icon_spacing = SP_S + 1.0;
    spacing.tooltip_width = 340.0;
    spacing.slider_rail_height = 4.0;
    spacing.scroll = egui::style::ScrollStyle {
        bar_width: 6.0,
        floating: true,
        floating_width: 2.0,
        floating_allocated_width: 0.0,
        bar_inner_margin: 2.0,
        bar_outer_margin: 1.0,
        handle_min_length: 20.0,
        ..egui::style::ScrollStyle::floating()
    };
    style.interaction.tooltip_delay = 0.4;
    // Числа в полях — моноширинным, как в Unity и Blender.
    style.drag_value_text_style = TextStyle::Monospace;
    style.interaction.show_tooltips_only_when_still = false;

    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::dark();
    visuals.override_text_color = None;
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_RAISED;
    visuals.extreme_bg_color = BG_INSET;
    visuals.code_bg_color = BG_INSET;
    visuals.faint_bg_color = BG_RAISED;
    visuals.window_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    visuals.window_rounding = Rounding::same(R_L);
    visuals.menu_rounding = Rounding::same(R_L);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 6.0),
        blur: 20.0,
        spread: 0.0,
        color: Color32::from_black_alpha(130),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 3.0),
        blur: 10.0,
        spread: 0.0,
        color: Color32::from_black_alpha(110),
    };
    visuals.hyperlink_color = mix(accent, Color32::WHITE, 0.35);
    visuals.selection.bg_fill = mix(BG_INSET, accent, 0.6);
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT_STRONG);
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };
    visuals.striped = false;
    visuals.indent_has_left_vline = false;
    visuals.collapsing_header_frame = false;
    visuals.text_cursor.stroke = Stroke::new(1.5_f32, TEXT_STRONG);
    visuals.interact_cursor = None;
    visuals.button_frame = true;

    let rounding = Rounding::same(R_S);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = BG_PANEL;
    widgets.noninteractive.weak_bg_fill = BG_PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.noninteractive.rounding = rounding;

    widgets.inactive.bg_fill = BG_FIELD;
    widgets.inactive.weak_bg_fill = BG_FIELD;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.inactive.rounding = rounding;
    widgets.inactive.expansion = 0.0;

    widgets.hovered.bg_fill = BG_HOVER;
    widgets.hovered.weak_bg_fill = BG_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_STRONG);
    widgets.hovered.rounding = rounding;
    widgets.hovered.expansion = 0.0;

    widgets.active.bg_fill = BG_ACTIVE;
    widgets.active.weak_bg_fill = BG_ACTIVE;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
    widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT_STRONG);
    widgets.active.rounding = rounding;
    widgets.active.expansion = 0.0;

    widgets.open.bg_fill = BG_HOVER;
    widgets.open.weak_bg_fill = BG_HOVER;
    widgets.open.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT_STRONG);
    widgets.open.rounding = rounding;

    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// Рамки панелей
// ---------------------------------------------------------------------------

/// Верх окна. Имя и иконку окна показывает его вкладка в доке, поэтому здесь —
/// только полоса с кнопками `right` (и только если они есть).
pub fn panel_header(ui: &mut Ui, _icon: Icon, _title: &str, right: impl FnOnce(&mut Ui)) {
    let bar = Rect::from_min_size(ui.available_rect_before_wrap().min, vec2(ui.available_width(), STRIP));
    let background = ui.painter().add(egui::Shape::Noop);
    let mut right_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar.shrink2(vec2(SP_M, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right_ui.spacing_mut().item_spacing.x = SP_S;
    right(&mut right_ui);
    if right_ui.min_rect().width() > 1.0 {
        ui.painter().set(background, egui::Shape::rect_filled(bar, 0.0, BG_PANEL));
        ui.painter()
            .hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, LINE));
        ui.allocate_rect(bar, Sense::hover());
    }
}

/// Полоса инструментов под заголовком окна (поиск, фильтры).
pub fn toolbar_strip(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, LINE));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(SP_S + 2.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = SP_S;
    add(&mut child);
}

/// Вертикальный разделитель 1 px между группами кнопок на полосе инструментов.
pub fn toolbar_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(SP_M + 1.0, CONTROL_H), Sense::hover());
    ui.painter().vline(
        rect.center().x.round() + 0.5,
        rect.y_range().shrink(3.0),
        Stroke::new(1.0_f32, LINE_SOFT),
    );
}

/// Подпись группы прописными («MATERIALS», «SCENE VIEW»).
pub fn section_label(ui: &mut Ui, title: &str) -> Response {
    let text = egui::RichText::new(title.to_uppercase())
        .font(strong(FONT_SECTION))
        .color(TEXT_MUTED)
        .extra_letter_spacing(0.6);
    ui.add(egui::Label::new(text).selectable(false))
}

/// Заголовок раздела в окне (настройки, диалоги): полужирный текст и линия под ним.
pub fn section_header(ui: &mut Ui, title: &str) {
    ui.add_space(SP_S);
    ui.label(egui::RichText::new(title).font(strong(FONT_TITLE)).color(TEXT));
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .hline(rect.x_range(), rect.top() + 2.5, Stroke::new(1.0_f32, LINE_SOFT));
    ui.add_space(SP_M);
}

/// Подпись поля в колонке подписей: длинная обрезается многоточием, полный текст и
/// `tooltip` — в подсказке.
pub fn field_label(ui: &mut Ui, label: &str, tooltip: &str) -> Response {
    let width = ui.available_width();
    let response = ui.add_sized(
        vec2(width, FIELD_H),
        egui::Label::new(egui::RichText::new(label).color(TEXT_MUTED))
            .truncate()
            .selectable(false),
    );
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(format!("{label}\n{tooltip}"))
    }
}

/// Фон строки списка по состояниям: выбрана (в фокусе / без фокуса), под курсором.
pub fn list_row(painter: &egui::Painter, rect: Rect, selected: bool, focused: bool, hovered: bool) {
    let fill = match (selected, focused, hovered) {
        (true, true, _) => selection_fill(),
        (true, false, _) => selection_unfocused(),
        (false, _, true) => hover_fill(),
        _ => return,
    };
    painter.rect_filled(rect, 0.0, fill);
}

/// Крошечный счётчик — единственная допустимая «таблетка».
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(10.5), TEXT_STRONG);
    let size = vec2((galley.size().x + 8.0).max(16.0), 14.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 7.0, color);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, TEXT_STRONG);
    response
}

/// Тонкая полоса прогресса (4 px) с подписью над ней; `fraction = None` — «бегущая».
pub fn progress_bar(ui: &mut Ui, fraction: Option<f32>, text: &str) -> Response {
    let width = ui.available_width().min(260.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 4.0), rect.max);
    ui.painter().rect_filled(bar, 2.0, BG_INSET);
    let time = ui.input(|i| i.time) as f32;
    let filled = match fraction {
        Some(f) => Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.0, 1.0), bar.height())),
        None => {
            ui.ctx().request_repaint();
            let t = (time * 0.8).fract();
            let x = bar.left() + (bar.width() * 1.3) * t - bar.width() * 0.3;
            Rect::from_min_max(pos2(x.max(bar.left()), bar.top()), pos2((x + bar.width() * 0.3).min(bar.right()), bar.bottom()))
        }
    };
    ui.painter().rect_filled(filled, 2.0, accent());
    ui.painter().text(
        pos2(rect.left(), bar.top() - 2.0),
        Align2::LEFT_BOTTOM,
        text,
        caption(),
        TEXT_MUTED,
    );
    response
}

/// Рамка модального диалога.
pub fn dialog_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style)
        .fill(BG_RAISED)
        .stroke(Stroke::new(1.0_f32, LINE_STRONG))
        .inner_margin(egui::Margin::same(SP_XL))
        .rounding(R_L)
}

/// Рамка плавающего окна (Preferences, Project Settings, Build).
pub fn window_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .fill(BG_PANEL)
        .stroke(Stroke::new(1.0_f32, LINE_STRONG))
        .inner_margin(egui::Margin::same(SP_L))
        .rounding(R_L)
}

/// Заголовок диалога: иконка 16 px и текст слева.
pub fn dialog_title(ui: &mut Ui, icon: Icon, color: Color32, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::hover());
        icons::paint(ui.painter(), rect.shrink(0.5), icon, color);
        ui.add_space(SP_XS);
        ui.label(egui::RichText::new(title).font(strong(FONT_HEADING)).color(TEXT_STRONG));
    });
    ui.add_space(SP_M);
}

// ---------------------------------------------------------------------------
// Кнопки
// ---------------------------------------------------------------------------

/// Главная кнопка диалога (акцентная заливка). Одна на окно.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let enabled = ui.is_enabled();
    let text = egui::RichText::new(label).color(if enabled { TEXT_STRONG } else { TEXT_DISABLED });
    let fill = if enabled { accent() } else { BG_FIELD };
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, LINE_STRONG))
            .min_size(vec2(80.0, FIELD_H + 2.0)),
    )
}

/// Широкая кнопка с иконкой (Add Component): по центру, как в Unity, не на всю ширину.
pub fn primary_button_wide(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), body(), TEXT);
    let width = (galley.size().x + ICON_S + 40.0).max(ui.available_width() * 0.55).min(ui.available_width());
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), FIELD_H + 4.0), Sense::hover());
    let rect = Rect::from_center_size(row.center(), vec2(width, FIELD_H + 2.0));
    let response = ui.interact(rect, ui.id().with(("wide_button", label)), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        BG_ACTIVE
    } else if response.hovered() {
        BG_HOVER
    } else {
        BG_FIELD
    };
    ui.painter().rect(rect, R_S, fill, Stroke::new(1.0_f32, LINE_STRONG));
    let total = ICON_S + SP_S + 2.0 + galley.size().x;
    let start = rect.center().x - total * 0.5;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(start + ICON_S * 0.5, rect.center().y), vec2(ICON_S, ICON_S)),
        icon,
        TEXT_MUTED,
    );
    ui.painter().galley(
        pos2(start + ICON_S + SP_S + 2.0, rect.center().y - galley.size().y * 0.5),
        galley,
        TEXT,
    );
    response
}

/// Обычная кнопка (в инспекторе, на полосах): высота поля, без минимальной ширины.
pub fn button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(label).min_size(vec2(0.0, FIELD_H)))
}

/// Обычная кнопка диалога.
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(label).min_size(vec2(80.0, FIELD_H + 2.0)))
}

/// Текстовая кнопка без фона (действие-ссылка в строке).
pub fn ghost_button(ui: &mut Ui, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), body(), TEXT);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(SP_M, 4.0), Sense::click());
    let enabled = ui.is_enabled();
    if response.hovered() && enabled {
        ui.painter().rect_filled(rect, R_S, BG_HOVER);
    }
    let color = if !enabled {
        TEXT_DISABLED
    } else if response.hovered() {
        TEXT_STRONG
    } else {
        mix(accent(), Color32::WHITE, 0.45)
    };
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
    response
}

/// Опасное действие (удалить безвозвратно).
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    let text = egui::RichText::new(label).color(TEXT_STRONG);
    ui.add(
        egui::Button::new(text)
            .fill(mix(BG_FIELD, ERROR, 0.7))
            .stroke(Stroke::new(1.0_f32, LINE_STRONG))
            .min_size(vec2(80.0, FIELD_H + 2.0)),
    )
}

/// Кнопка с иконкой и подписью — обычного вида (полосы инструментов).
pub fn icon_text_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), body(), Color32::PLACEHOLDER);
    let size = vec2(SP_S + 2.0 + ICON_S + SP_S + 1.0 + galley.size().x + SP_M, CONTROL_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let visuals = ui.style().interact(&response);
    let fill = if enabled { visuals.weak_bg_fill } else { BG_FIELD };
    ui.painter().rect(rect, R_S, fill, Stroke::new(1.0_f32, LINE_STRONG));
    let color = if enabled { TEXT } else { TEXT_DISABLED };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + SP_S + 2.0 + ICON_S * 0.5, rect.center().y), vec2(ICON_S, ICON_S)),
        icon,
        if enabled { TEXT_MUTED } else { TEXT_DISABLED },
    );
    ui.painter().galley(
        pos2(rect.left() + SP_S + 2.0 + ICON_S + SP_S + 1.0, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
    response
}

/// Переключатель с иконкой и подписью на полосе инструментов (Collapse, Error Pause…).
pub fn toggle_button(ui: &mut Ui, on: &mut bool, icon: Icon, label: &str, tooltip: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), body(), Color32::PLACEHOLDER);
    let size = vec2(SP_S + 2.0 + ICON_S + SP_S + 1.0 + galley.size().x + SP_M, CONTROL_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if *on {
        accent_soft()
    } else if response.hovered() {
        BG_HOVER
    } else {
        BG_FIELD
    };
    ui.painter().rect(rect, R_S, fill, Stroke::new(1.0_f32, LINE_STRONG));
    let color = if *on { TEXT_STRONG } else { TEXT };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + SP_S + 2.0 + ICON_S * 0.5, rect.center().y), vec2(ICON_S, ICON_S)),
        icon,
        if *on { TEXT_STRONG } else { TEXT_MUTED },
    );
    ui.painter().galley(
        pos2(rect.left() + SP_S + 2.0 + ICON_S + SP_S + 1.0, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
    if response.clicked() {
        *on = !*on;
    }
    response.on_hover_text(tooltip)
}

/// Сегментированный переключатель: варианты встык, выбранный — с акцентом.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str, &str)]) -> bool {
    let mut changed = false;
    let height = CONTROL_H;
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label, _)| ui.painter().layout_no_wrap(label.to_string(), body(), Color32::PLACEHOLDER))
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| (g.size().x + 16.0).max(28.0).round()).collect();
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    ui.painter().rect(rect, R_S, BG_FIELD, Stroke::new(1.0_f32, LINE_STRONG));
    let mut x = rect.left();
    let count = options.len();
    for (index, (((option, _, tooltip), galley), width)) in options.iter().zip(galleys).zip(widths).enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(width, height));
        x += width;
        let response = ui
            .interact(cell, ui.id().with(("segmented", tooltip)), Sense::click())
            .on_hover_text(*tooltip);
        let selected = *value == *option;
        let inner = cell.shrink(1.0);
        if selected {
            ui.painter().rect_filled(inner, R_S, accent_soft());
        } else if response.hovered() {
            ui.painter().rect_filled(inner, R_S, BG_HOVER);
        }
        if index + 1 < count && !selected {
            ui.painter()
                .vline(cell.right() - 0.5, cell.y_range().shrink(4.0), Stroke::new(1.0_f32, LINE_STRONG));
        }
        let color = if selected { TEXT_STRONG } else { TEXT_MUTED };
        ui.painter().galley(cell.center() - galley.size() * 0.5, galley, color);
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
    }
    changed
}

/// Компактное выпадающее меню полосы инструментов («Gizmos ▾», «Camera ▾»): кнопка без
/// рамки, по щелчку — меню из `item(..)`. Возвращает результат `add`, если меню открыто.
pub fn dropdown_button<R>(ui: &mut Ui, label: &str, tooltip: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let mut job = LayoutJob::default();
    let format = TextFormat {
        font_id: body(),
        color: TEXT,
        ..TextFormat::default()
    };
    job.append(label, 0.0, format.clone());
    // Место под стрелку справа.
    job.append("    ", 0.0, format);
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = egui::menu::menu_custom_button(
        ui,
        egui::Button::new(job).frame(false).min_size(vec2(0.0, CONTROL_H)),
        |ui| {
            menu_body(ui);
            add(ui)
        },
    );
    let rect = inner.response.rect;
    if inner.response.hovered() || inner.inner.is_some() {
        ui.painter().set(background, egui::Shape::rect_filled(rect, R_S, BG_HOVER));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y + 1.0), vec2(8.0, 8.0)),
        Icon::ChevronDown,
        TEXT_MUTED,
    );
    inner.response.on_hover_text(tooltip);
    inner.inner
}

/// Поле поиска с лупой и кнопкой очистки. Возвращает ответ текстового поля.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let height = CONTROL_H;
    let width = width.min(ui.available_width()).max(80.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let edit_rect = Rect::from_min_max(pos2(rect.left() + 22.0, rect.top()), pos2(rect.right() - 20.0, rect.bottom()));
    // Место под рамку — до текста, чтобы фон лёг под него.
    let background = ui.painter().add(egui::Shape::Noop);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect));
    let response = child.add(
        egui::TextEdit::singleline(text)
            .hint_text(egui::RichText::new(hint).color(TEXT_DISABLED))
            .frame(false)
            .vertical_align(egui::Align::Center)
            .min_size(vec2(edit_rect.width(), height))
            .desired_width(edit_rect.width()),
    );
    let stroke = if response.has_focus() {
        Stroke::new(1.0_f32, accent())
    } else if response.hovered() {
        Stroke::new(1.0_f32, LINE_HOVER)
    } else {
        Stroke::new(1.0_f32, LINE_STRONG)
    };
    ui.painter().set(
        background,
        egui::epaint::RectShape::new(rect, R_S, BG_INSET, stroke),
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 11.5, rect.center().y), vec2(ICON_S, ICON_S)),
        Icon::Search,
        TEXT_DISABLED,
    );
    if !text.is_empty() {
        let clear = Rect::from_center_size(pos2(rect.right() - 11.0, rect.center().y), vec2(16.0, 16.0));
        let clear_response = ui.interact(clear, response.id.with("clear"), Sense::click());
        if clear_response.hovered() {
            ui.painter().rect_filled(clear, R_S, BG_HOVER);
        }
        icons::paint(ui.painter(), clear.shrink(4.5), Icon::Close, TEXT_MUTED);
        if clear_response.clicked() {
            text.clear();
        }
    }
    response
}

/// Фильтр-переключатель с иконкой и числом (уровни консоли). Прямоугольный, без заливки
/// акцентом: включённый — светлее и с цветной иконкой, выключенный — серый.
pub fn pill_toggle(ui: &mut Ui, on: &mut bool, icon: Icon, color: Color32, text: &str, tooltip: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), mono(), Color32::PLACEHOLDER);
    let size = vec2(SP_S + 2.0 + ICON_S + SP_S + galley.size().x + SP_S + 2.0, CONTROL_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if *on {
        BG_ACTIVE
    } else if response.hovered() {
        BG_HOVER
    } else {
        BG_FIELD
    };
    ui.painter().rect(rect, R_S, fill, Stroke::new(1.0_f32, LINE_STRONG));
    let tint = if *on { color } else { TEXT_DISABLED };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + SP_S + 2.0 + ICON_S * 0.5, rect.center().y), vec2(ICON_S, ICON_S)),
        icon,
        tint,
    );
    let text_color = if *on { TEXT_STRONG } else { TEXT_DISABLED };
    ui.painter().galley(
        pos2(rect.left() + SP_S + 2.0 + ICON_S + SP_S, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );
    if response.clicked() {
        *on = !*on;
    }
    response.on_hover_text(tooltip)
}

// ---------------------------------------------------------------------------
// Меню
// ---------------------------------------------------------------------------

/// Пункт меню: иконка в колонке слева, подпись, сочетание клавиш справа.
pub struct MenuItem<'a> {
    label: &'a str,
    icon: Option<(Icon, Option<Color32>)>,
    shortcut: Option<&'a str>,
    disabled_reason: Option<&'a str>,
    checked: Option<bool>,
}

pub fn item(label: &str) -> MenuItem<'_> {
    MenuItem {
        label,
        icon: None,
        shortcut: None,
        disabled_reason: None,
        checked: None,
    }
}

impl<'a> MenuItem<'a> {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some((icon, None));
        self
    }

    pub fn tinted(mut self, icon: Icon, color: Color32) -> Self {
        self.icon = Some((icon, Some(color)));
        self
    }

    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Недоступен, если `enabled == false`; `reason` показывается в подсказке.
    pub fn enabled_if(mut self, enabled: bool, reason: &'a str) -> Self {
        if !enabled {
            self.disabled_reason = Some(reason);
        }
        self
    }

    /// Пункт-флажок: галочка в колонке иконки.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Нарисовать. `true` — пункт выбрали (меню закрывается само).
    pub fn show(self, ui: &mut Ui) -> bool {
        let enabled = self.disabled_reason.is_none();
        let label = ui.painter().layout_no_wrap(self.label.to_string(), body(), Color32::PLACEHOLDER);
        let shortcut = self
            .shortcut
            .map(|s| ui.painter().layout_no_wrap(s.to_string(), caption(), Color32::PLACEHOLDER));
        let shortcut_width = shortcut.as_ref().map_or(0.0, |g| g.size().x + SP_XXL);
        let desired = vec2(
            (MENU_GUTTER + label.size().x + shortcut_width + SP_L).max(MENU_WIDTH),
            MENU_ROW,
        );
        let (rect, response) = ui.allocate_at_least(desired, Sense::click());
        let hovered = response.hovered() && enabled;
        if hovered {
            ui.painter().rect_filled(rect, R_S, mix(BG_RAISED, accent(), 0.55));
        }
        let (text_color, muted) = if !enabled {
            (TEXT_DISABLED, TEXT_DISABLED)
        } else if hovered {
            (TEXT_STRONG, mix(TEXT_MUTED, Color32::WHITE, 0.3))
        } else {
            (TEXT, TEXT_MUTED)
        };
        let gutter = Rect::from_min_size(rect.min, vec2(MENU_GUTTER, rect.height()));
        if self.checked == Some(true) {
            icons::paint(ui.painter(), Rect::from_center_size(gutter.center(), vec2(ICON_S, ICON_S)), Icon::Check, text_color);
        } else if let Some((icon, tint)) = self.icon {
            let color = match tint {
                Some(tint) if enabled && !hovered => tint,
                _ => muted,
            };
            icons::paint(ui.painter(), Rect::from_center_size(gutter.center(), vec2(14.0, 14.0)), icon, color);
        }
        ui.painter().galley(
            pos2(gutter.right(), rect.center().y - label.size().y * 0.5),
            label,
            text_color,
        );
        if let Some(galley) = shortcut {
            ui.painter().galley(
                pos2(rect.right() - SP_M - galley.size().x, rect.center().y - galley.size().y * 0.5),
                galley,
                muted,
            );
        }
        let clicked = enabled && response.clicked();
        if let Some(reason) = self.disabled_reason {
            response.on_hover_text(reason);
        }
        if clicked {
            ui.close_menu();
        }
        clicked
    }
}

/// Подпись раздела внутри меню.
pub fn menu_section(ui: &mut Ui, title: &str) {
    ui.add_space(SP_XS);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(MENU_WIDTH), 18.0), Sense::hover());
    let galley = ui.painter().layout_no_wrap(title.to_uppercase(), strong(FONT_SECTION), TEXT_DISABLED);
    ui.painter().galley(
        pos2(rect.left() + SP_M, rect.center().y - galley.size().y * 0.5),
        galley,
        TEXT_DISABLED,
    );
}

/// Разделитель в меню.
pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
    ui.painter().hline(
        rect.x_range().shrink(SP_XS),
        rect.center().y.round() + 0.5,
        Stroke::new(1.0_f32, LINE_SOFT),
    );
}

/// Начало содержимого меню: плотные строки.
pub fn menu_body(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(MENU_WIDTH);
}

/// Подменю, выровненное по колонке иконок остальных пунктов.
pub fn submenu<R>(ui: &mut Ui, icon: Icon, title: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let mut job = LayoutJob::default();
    job.append(
        title,
        MENU_GUTTER - 2.0,
        TextFormat {
            font_id: body(),
            color: TEXT,
            ..TextFormat::default()
        },
    );
    let previous = ui.spacing().interact_size.y;
    ui.spacing_mut().interact_size.y = MENU_ROW;
    let response = ui.menu_button(job, |ui| {
        menu_body(ui);
        add(ui)
    });
    ui.spacing_mut().interact_size.y = previous;
    let rect = response.response.rect;
    let gutter = Rect::from_min_size(rect.min, vec2(MENU_GUTTER, rect.height()));
    icons::paint(ui.painter(), Rect::from_center_size(gutter.center(), vec2(14.0, 14.0)), icon, TEXT_MUTED);
    response.inner
}

// ---------------------------------------------------------------------------
// Прочее
// ---------------------------------------------------------------------------

/// Пустое состояние окна: иконка, заголовок, пояснение.
pub fn empty_state(ui: &mut Ui, icon: Icon, title: &str, hint: &str) {
    ui.add_space(SP_XL);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
        icons::paint(ui.painter(), rect, icon, TEXT_DISABLED);
        ui.add_space(SP_S + 2.0);
        ui.label(egui::RichText::new(title).color(TEXT_MUTED));
        ui.add_space(SP_XS);
        ui.label(egui::RichText::new(hint).size(FONT_CAPTION).color(TEXT_DISABLED));
    });
}

/// Подпись на полупрозрачной плашке поверх вида.
pub fn chip(painter: &egui::Painter, position: egui::Pos2, text: &str, color: Color32) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), caption(), color);
    let rect = Rect::from_min_size(position, galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(rect, R_S, Color32::from_rgba_unmultiplied(16, 16, 16, 190));
    painter.galley(rect.min + vec2(6.0, 3.0), galley, color);
    rect
}

/// Строка «подпись: значение» для сводок (профайлер, детали ассета).
pub fn key_value(ui: &mut Ui, key: &str, value: impl Into<egui::WidgetText>) {
    ui.label(egui::RichText::new(key).color(TEXT_MUTED));
    ui.label(value);
    ui.end_row();
}

/// `jump_speed` → `Jump Speed` — подписи полей в инспекторе.
pub fn pretty_name(field: &str) -> String {
    field
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PlayerController` → `Player Controller`, `Rigidbody2D` → `Rigidbody 2D`.
pub fn pretty_type(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let boundary = (c.is_uppercase() && (prev.is_lowercase() || (prev.is_uppercase() && next_lower)))
                || (c.is_ascii_digit() && prev.is_alphabetic());
            if boundary {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_humanized() {
        assert_eq!(pretty_name("jump_speed"), "Jump Speed");
        assert_eq!(pretty_name("size"), "Size");
        assert_eq!(pretty_type("PlayerController"), "Player Controller");
        assert_eq!(pretty_type("Rigidbody2D"), "Rigidbody 2D");
        assert_eq!(pretty_type("Camera"), "Camera");
    }

    #[test]
    fn accent_can_be_changed_and_mixed() {
        let before = accent();
        set_accent(Color32::from_rgb(1, 2, 3));
        assert_eq!(accent(), Color32::from_rgb(1, 2, 3));
        set_accent(before);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.5), Color32::from_rgb(128, 128, 128));
    }
}
