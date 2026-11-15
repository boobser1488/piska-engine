//! Всплывающие окна и диалоги — одна реализация для редактора, Piska Hub и (копией через
//! `modeler/scripts/sync_from_engine.py`) Piska Modeler.
//!
//! * [`dialog`] — модальный диалог (затемнение, ввод только в нём) и [`tool_window`] —
//!   плавающее окно (Build, Preferences, Undo History…). У обоих полоса заголовка по
//!   `ui-guidelines.md`: значок и заголовок слева, крестик справа. **Окно тянут за
//!   заголовок**; двойной щелчок по нему возвращает окно в центр. Положение запоминается на
//!   время работы программы по id окна ([`window_offset`]; большие окна редактор хранит и
//!   в настройках) и не даёт заголовку уйти за край окна программы (и после изменения его
//!   размера). Esc закрывает верхнее окно.
//! * [`dialog_actions`] — строка кнопок внизу диалога: отступ [`DIALOG_ACTIONS_GAP`] от
//!   последнего поля, кнопки справа, главная — правее всех (её добавляют первой), одна
//!   высота и минимальная ширина.
//! * [`elided_path`] — длинный путь в подсказке диалога: середина «…», полностью — во
//!   всплывающей подсказке.
//!
//! Для проверок выравнивания каждое окно записывает свою раскладку ([`last_layout`]).

use egui::{pos2, vec2, Align2, Color32, Id, Rect, Sense, Stroke, Vec2};

use crate::theme::{self, BG_HOVER, BG_PANEL, BG_RAISED, LINE_STRONG, R_L, R_S, SP_L, SP_M, SP_S, SP_XL, TEXT, TEXT_MUTED, TEXT_STRONG};

/// Отступ строки кнопок диалога от последнего поля (`ui-guidelines.md`: «12–14 px»).
pub const DIALOG_ACTIONS_GAP: f32 = theme::DIALOG_ACTIONS_GAP;
/// Высота полосы заголовка окна.
pub const TITLE_BAR: f32 = 26.0;
/// Сколько заголовка обязано остаться в окне программы, когда окно тянут к краю.
const KEEP_VISIBLE: f32 = 80.0;
/// Минимальная ширина кнопки диалога.
pub const DIALOG_BUTTON_MIN_W: f32 = theme::DIALOG_BUTTON_MIN_W;

/// Как окно разложено в последнем кадре — для тестов выравнивания.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowLayout {
    /// Всё окно (рамка).
    pub frame: Rect,
    /// Полоса заголовка (её тянут).
    pub title: Rect,
    /// Содержимое без отступов рамки.
    pub body: Rect,
    /// Строка кнопок ([`dialog_actions`]), если есть.
    pub actions: Option<Rect>,
    /// Низ содержимого над строкой кнопок.
    pub content_bottom: f32,
}

impl Default for WindowLayout {
    fn default() -> Self {
        Self { frame: Rect::NOTHING, title: Rect::NOTHING, body: Rect::NOTHING, actions: None, content_bottom: 0.0 }
    }
}

/// Взять и убрать временное значение (`remove_temp` egui требует `Default`, у `Rect` его нет).
fn take_temp<T: 'static + Clone + Send + Sync>(d: &mut egui::util::IdTypeMap, id: Id) -> Option<T> {
    let value = d.get_temp::<T>(id);
    d.remove::<T>(id);
    value
}

/// Смещение окна от центра экрана (то, что пользователь натянул мышью); ноль — по центру.
pub fn window_offset(ctx: &egui::Context, id: Id) -> Vec2 {
    ctx.data(|d| d.get_temp::<Vec2>(id.with("window_offset"))).unwrap_or(Vec2::ZERO)
}

/// Поставить окно (смещение от центра экрана) — так редактор возвращает сохранённое место.
pub fn set_window_offset(ctx: &egui::Context, id: Id, offset: Vec2) {
    ctx.data_mut(|d| d.insert_temp(id.with("window_offset"), offset));
}

/// Раскладка окна `id` в последнем кадре.
pub fn last_layout(ctx: &egui::Context, id: Id) -> Option<WindowLayout> {
    ctx.data(|d| d.get_temp::<WindowLayout>(id.with("window_layout")))
}

/// Полоса заголовка окна `id`: где её тянуть в тестах.
pub fn title_bar_id(id: Id) -> Id {
    id.with("title_bar")
}

/// Крестик окна `id`.
pub fn close_button_id(id: Id) -> Id {
    id.with("close")
}

/// Значок в заголовке: рисует в квадрате 16 px.
pub type TitleIcon<'a> = &'a dyn Fn(&egui::Painter, Rect);

/// Полоса заголовка внутри рамки окна: значок и заголовок слева, крестик справа; тянуть —
/// двигать окно, двойной щелчок — в центр. Возвращает «закрыть».
fn title_bar(ui: &mut egui::Ui, id: Id, title: &str, icon: Option<TitleIcon<'_>>, closable: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TITLE_BAR), Sense::hover());
    let close_rect = Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(20.0, 20.0));
    let drag_rect = if closable { Rect::from_min_max(rect.min, pos2(close_rect.left() - SP_S, rect.max.y)) } else { rect };
    let drag = ui.interact(drag_rect, title_bar_id(id), Sense::click_and_drag());
    if drag.dragged() {
        let delta = drag.drag_delta();
        set_window_offset(ui.ctx(), id, window_offset(ui.ctx(), id) + delta);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if drag.double_clicked() {
        set_window_offset(ui.ctx(), id, Vec2::ZERO);
    }
    let mut x = rect.left();
    if let Some(icon) = icon {
        let square = Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0));
        icon(ui.painter(), square);
        x += 16.0 + SP_S + 2.0;
    }
    let text_right = if closable { close_rect.left() - SP_S } else { rect.right() };
    let galley = ui.painter().layout(
        title.to_string(),
        theme::strong(theme::FONT_HEADING),
        TEXT_STRONG,
        (text_right - x).max(10.0),
    );
    ui.painter()
        .with_clip_rect(Rect::from_min_max(pos2(x, rect.top()), pos2(text_right, rect.bottom())))
        .galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley, TEXT_STRONG);
    let mut close = false;
    if closable {
        let button = ui.interact(close_rect, close_button_id(id), Sense::click());
        if button.hovered() {
            ui.painter().rect_filled(close_rect, R_S, BG_HOVER);
        }
        // Крестик линиями (у Modeler свой набор иконок, поэтому без `icons`).
        let c = close_rect.center();
        let color = if button.hovered() { TEXT_STRONG } else { TEXT_MUTED };
        let stroke = Stroke::new(1.4_f32, color);
        ui.painter().line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], stroke);
        ui.painter().line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], stroke);
        close = button.on_hover_text(crate::i18n::tr_s("Close (Esc)")).clicked();
    }
    ui.add_space(SP_M);
    close
}

/// Отметить, что масштаб интерфейса только что запросили сменить на `target`
/// (`set_zoom_factor`; egui применит его со следующего кадра). Тот же масштаб — не смена.
pub fn note_zoom_change(ctx: &egui::Context, target: f32) {
    if (ctx.zoom_factor() - target).abs() < 1e-3 {
        return;
    }
    let now = ctx.input(|i| i.time);
    // (время кадра смены, время последнего виденного кадра, сколько кадров прошло)
    ctx.data_mut(|d| d.insert_temp(Id::new("kit_zoom_change"), (now, now, 0_u32)));
}

/// Идёт кадр смены масштаба или следующий за ним (все их проходы): размер экрана в точках
/// ещё не устоялся, окно, нарисованное сейчас, на следующем кадре перескочило бы.
pub fn zoom_settling(ctx: &egui::Context) -> bool {
    let key = Id::new("kit_zoom_change");
    let Some((changed, last, frames)) = ctx.data(|d| d.get_temp::<(f64, f64, u32)>(key)) else { return false };
    let now = ctx.input(|i| i.time);
    let frames = if now != last { frames + 1 } else { frames };
    ctx.data_mut(|d| d.insert_temp(key, (changed, now, frames)));
    frames <= 1
}

/// Смещение левого верхнего угла окна размера `size` (с рамкой), стоящего по центру экрана
/// со сдвигом [`window_offset`]. Окна ставятся углом, а не центром: egui добавляет поля рамки
/// к размеру только с третьего кадра, и окно с якорем по центру сдвигалось бы на половину
/// полей (строка, прочитанная в первом кадре, уезжала из-под указателя).
pub fn centered_corner(ctx: &egui::Context, id: Id, size: Vec2) -> Vec2 {
    let screen = ctx.screen_rect();
    (screen.center() - size * 0.5 - screen.min).round() + window_offset(ctx, id)
}

/// Не дать заголовку уйти за край окна программы: поправить смещение по прошлому кадру.
fn clamp_offset(ctx: &egui::Context, id: Id) {
    let Some(layout) = last_layout(ctx, id) else { return };
    let screen = ctx.screen_rect();
    let offset = window_offset(ctx, id);
    let title = layout.title;
    let mut fix = Vec2::ZERO;
    if title.right() < screen.left() + KEEP_VISIBLE {
        fix.x = screen.left() + KEEP_VISIBLE - title.right();
    }
    if title.left() > screen.right() - KEEP_VISIBLE {
        fix.x = screen.right() - KEEP_VISIBLE - title.left();
    }
    if title.top() < screen.top() {
        fix.y = screen.top() - title.top();
    }
    if title.bottom() > screen.bottom() {
        fix.y = screen.bottom() - title.bottom();
    }
    if fix != Vec2::ZERO {
        set_window_offset(ctx, id, offset + fix);
    }
}

/// Записать раскладку окна (рамка — после показа).
fn store_layout(ctx: &egui::Context, id: Id, layout: WindowLayout) {
    ctx.data_mut(|d| d.insert_temp(id.with("window_layout"), layout));
}

/// Начало содержимого: заголовок + место для раскладки.
fn begin_layout(ui: &egui::Ui) -> WindowLayout {
    WindowLayout { body: ui.max_rect(), content_bottom: ui.min_rect().bottom(), ..Default::default() }
}

/// Модальный диалог.
pub struct Dialog<'a> {
    id: Id,
    title: &'a str,
    width: f32,
    icon: Option<TitleIcon<'a>>,
    closable: bool,
}

/// Модальный диалог с заголовком `title` (см. описание модуля). По центру, пока его не
/// сдвинули.
pub fn dialog<'a>(id: impl std::hash::Hash, title: &'a str) -> Dialog<'a> {
    Dialog { id: Id::new(id), title, width: 400.0, icon: None, closable: true }
}

/// Окно, содержимое которого рисуется сейчас: его заголовок может нарисовать
/// `theme::dialog_title` ([`inline_title`]).
fn set_current(ctx: &egui::Context, current: Option<(Id, bool)>) {
    ctx.data_mut(|d| match current {
        Some(current) => d.insert_temp(Id::new("kit_current_window"), current),
        None => {
            d.remove::<(Id, bool)>(Id::new("kit_current_window"));
        }
    });
}

fn take_inline_close(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(Id::new("kit_inline_close"))).unwrap_or(false)
}

/// Заголовок окна изнутри его содержимого (так его рисуют диалоги с заголовком, который
/// зависит от состояния): тянуть, двойной щелчок, крестик — как у [`dialog`]. `false` —
/// сейчас не рисуется окно из этого модуля (тогда нарисуйте заголовок как обычно).
pub fn inline_title(ui: &mut egui::Ui, title: &str, icon: TitleIcon<'_>) -> bool {
    let Some((id, closable)) = ui.ctx().data(|d| d.get_temp::<(Id, bool)>(Id::new("kit_current_window"))) else {
        return false;
    };
    let top = ui.min_rect().bottom();
    if title_bar(ui, id, title, Some(icon), closable) {
        ui.ctx().data_mut(|d| d.insert_temp(Id::new("kit_inline_close"), true));
    }
    // Заголовок — то, что тянут: место для проверок раскладки.
    let rect = Rect::from_min_size(pos2(ui.max_rect().left(), top), vec2(ui.max_rect().width(), TITLE_BAR));
    ui.ctx().data_mut(|d| d.insert_temp(id.with("inline_title"), rect));
    true
}

/// Итог кадра диалога.
pub struct DialogResponse<R> {
    pub inner: R,
    /// Закрыть как «Отмена»: крестик, Esc или щелчок мимо окна.
    pub cancel: bool,
}

impl<R> DialogResponse<R> {
    /// То же, что `egui::ModalResponse::should_close` (для прежнего кода диалогов).
    pub fn should_close(&self) -> bool {
        self.cancel
    }
}

impl<'a> Dialog<'a> {
    /// Ширина содержимого.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn icon(mut self, icon: TitleIcon<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Без крестика (диалог, который нельзя просто закрыть).
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub fn show<R>(self, ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui) -> R) -> DialogResponse<R> {
        let Self { id, title, width, icon, closable } = self;
        clamp_offset(ctx, id);
        let area = egui::Modal::default_area(id).anchor(Align2::CENTER_CENTER, window_offset(ctx, id));
        let mut close = false;
        let mut layout = WindowLayout::default();
        let response = egui::Modal::new(id).area(area).frame(dialog_frame(&ctx.style())).show(ctx, |ui| {
            if width > 0.0 {
                ui.set_width(width);
            }
            let top = ui.min_rect().top();
            if !title.is_empty() {
                close = title_bar(ui, id, title, icon, closable);
            }
            layout.title = Rect::from_min_size(pos2(ui.max_rect().left(), top), vec2(ui.max_rect().width(), TITLE_BAR));
            let body = begin_layout(ui);
            set_current(ui.ctx(), Some((id, closable)));
            let inner = add(ui);
            close |= take_inline_close(ui.ctx());
            set_current(ui.ctx(), None);
            layout.body = Rect::from_min_max(body.body.min, pos2(body.body.right(), ui.min_rect().bottom()));
            layout.actions = ui.ctx().data_mut(|d| take_temp::<Rect>(d, Id::new("kit_dialog_actions")));
            layout.content_bottom =
                ui.ctx().data_mut(|d| d.remove_temp::<f32>(Id::new("kit_dialog_content_bottom"))).unwrap_or(ui.min_rect().bottom());
            inner
        });
        layout.frame = response.response.rect;
        if title.is_empty() {
            if let Some(rect) = ctx.data_mut(|d| take_temp::<Rect>(d, id.with("inline_title"))) {
                layout.title = rect;
            }
        }
        store_layout(ctx, id, layout);
        let cancel = close || response.should_close();
        DialogResponse { inner: response.inner, cancel }
    }
}

/// Плавающее (не модальное) окно.
pub struct ToolWindow<'a> {
    id: Id,
    title: &'a str,
    default_size: Vec2,
    min_size: Vec2,
    resizable: bool,
    icon: Option<TitleIcon<'a>>,
}

/// Плавающее окно с заголовком `title` (см. описание модуля).
pub fn tool_window<'a>(id: impl std::hash::Hash, title: &'a str) -> ToolWindow<'a> {
    ToolWindow { id: Id::new(id), title, default_size: vec2(420.0, 320.0), min_size: vec2(240.0, 140.0), resizable: true, icon: None }
}

impl<'a> ToolWindow<'a> {
    pub fn default_size(mut self, size: Vec2) -> Self {
        self.default_size = size;
        self
    }

    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = size;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn icon(mut self, icon: TitleIcon<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Показать, пока `*open`; крестик и Esc (если окно верхнее) ставят `false`.
    pub fn show<R>(self, ctx: &egui::Context, open: &mut bool, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
        if !*open {
            return None;
        }
        let Self { id, title, default_size, min_size, resizable, icon } = self;
        clamp_offset(ctx, id);
        let mut close = false;
        let mut layout = WindowLayout::default();
        let shown = egui::Window::new(title)
            .id(id)
            .title_bar(false)
            .movable(false)
            .collapsible(false)
            .resizable(resizable)
            .default_size(default_size)
            .min_size(min_size)
            .anchor(Align2::LEFT_TOP, centered_corner(ctx, id, default_size + Vec2::splat(SP_L * 2.0)))
            .frame(window_frame(&ctx.style()))
            .show(ctx, |ui| {
                let top = ui.min_rect().top();
                if !title.is_empty() {
                    close = title_bar(ui, id, title, icon, true);
                }
                layout.title = Rect::from_min_size(pos2(ui.max_rect().left(), top), vec2(ui.max_rect().width(), TITLE_BAR));
                let body = begin_layout(ui);
                set_current(ui.ctx(), Some((id, true)));
                let inner = add(ui);
                close |= take_inline_close(ui.ctx());
                set_current(ui.ctx(), None);
                layout.body = Rect::from_min_max(body.body.min, pos2(body.body.right(), ui.min_rect().bottom()));
                layout.actions = ui.ctx().data_mut(|d| take_temp::<Rect>(d, Id::new("kit_dialog_actions")));
                inner
            });
        let shown = shown?;
        layout.frame = shown.response.rect;
        if title.is_empty() {
            if let Some(rect) = ctx.data_mut(|d| take_temp::<Rect>(d, id.with("inline_title"))) {
                layout.title = rect;
            }
        }
        store_layout(ctx, id, layout);
        // Номер прохода — до `data_mut`: вызов контекста внутри его замка — взаимоблокировка.
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(Id::new("kit_tool_window_pass"), pass));
        // Esc — верхнему окну (не модальному диалогу над ним и не полю ввода).
        let top = ctx.memory(|m| m.areas().top_layer_id(egui::Order::Middle)) == Some(shown.response.layer_id);
        let no_modal = ctx.memory(|m| m.top_modal_layer()).is_none();
        if top && no_modal && !ctx.wants_keyboard_input() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        if close {
            *open = false;
        }
        shown.inner
    }
}

/// Рамка модального диалога: поднятая поверхность, равные отступы со всех сторон.
pub fn dialog_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style)
        .fill(BG_RAISED)
        .stroke(Stroke::new(1.0_f32, LINE_STRONG))
        .inner_margin(egui::Margin::same(SP_XL))
        .rounding(R_L)
}

/// Рамка плавающего окна.
pub fn window_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .fill(BG_PANEL)
        .stroke(Stroke::new(1.0_f32, LINE_STRONG))
        .inner_margin(egui::Margin::same(SP_L))
        .rounding(R_L)
}

/// Строка кнопок внизу диалога: отступ [`DIALOG_ACTIONS_GAP`], кнопки справа налево —
/// добавляйте главную первой (она встанет правее всех), затем «Cancel». Кнопки — через
/// [`action_button`] / [`primary_action`] (одна высота и минимальная ширина).
pub fn dialog_actions<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let content_bottom = ui.min_rect().bottom();
    ui.add_space(DIALOG_ACTIONS_GAP);
    let width = ui.available_width();
    let inner = ui.allocate_ui_with_layout(vec2(width, theme::control_h(ui)), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = SP_M;
        add(ui)
    });
    let rect = inner.response.rect;
    // Раскладку забирает окно, которое рисуется сейчас (окна рисуются по очереди).
    ui.ctx().data_mut(|d| {
        d.insert_temp(Id::new("kit_dialog_actions"), rect);
        d.insert_temp(Id::new("kit_dialog_content_bottom"), content_bottom);
    });
    inner.inner
}

/// Обычная кнопка строки [`dialog_actions`].
pub fn action_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    crate::controls::button_ex(ui, crate::controls::Kind::Normal, None, label, DIALOG_BUTTON_MIN_W)
}

/// Главная кнопка строки [`dialog_actions`] (заливка акцентом; одна на окно).
pub fn primary_action(ui: &mut egui::Ui, label: &str) -> egui::Response {
    crate::controls::button_ex(ui, crate::controls::Kind::Primary, None, label, DIALOG_BUTTON_MIN_W)
}

/// Подсказка-путь в диалоге: одна строка, середина длинного пути — «…», полный путь — во
/// всплывающей подсказке.
pub fn elided_path(ui: &mut egui::Ui, prefix: &str, path: &str) -> egui::Response {
    let font = theme::caption();
    let width = ui.available_width().max(40.0);
    let full = format!("{prefix}{path}");
    let fits = |text: &str| ui.fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), TEXT_MUTED).size().x) <= width;
    let mut shown = full.clone();
    if !fits(&shown) {
        let chars: Vec<char> = path.chars().collect();
        let mut keep = chars.len();
        while keep > 4 {
            keep -= 1;
            let head = keep / 3;
            let tail = keep - head;
            let candidate = format!(
                "{prefix}{}…{}",
                chars[..head].iter().collect::<String>(),
                chars[chars.len() - tail..].iter().collect::<String>()
            );
            if fits(&candidate) {
                shown = candidate;
                break;
            }
        }
    }
    let response = ui.add(egui::Label::new(egui::RichText::new(&shown).font(font).color(TEXT_MUTED)).wrap_mode(egui::TextWrapMode::Extend));
    if shown != full {
        response.on_hover_text(full)
    } else {
        response
    }
}

/// Полоса прогресса «работа идёт»: подпись и бегущий отрезок (одна для заставки и для
/// долгих операций внутри программ). `fraction` — доля, если она известна.
pub fn busy_indicator(ui: &mut egui::Ui, label: &str, fraction: Option<f32>) -> egui::Response {
    let width = ui.available_width().clamp(120.0, 360.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let painter = ui.painter();
    painter.text(pos2(rect.left(), rect.top() + 6.0), Align2::LEFT_CENTER, label, theme::caption(), TEXT);
    let bar = Rect::from_min_size(pos2(rect.left(), rect.bottom() - 4.0), vec2(width, 2.0));
    paint_progress(painter, bar, fraction, ui.input(|i| i.time), 1.0);
    ui.ctx().request_repaint();
    response
}

pub use crate::boot::{paint_progress, LoadingScreen};

/// Своя полоса заголовка окна (Preferences, Project Settings рисуют её сами): область
/// `rect` тянут — окно `id` едет, двойной щелчок — в центр. Окно должно стоять по
/// [`window_offset`] (`anchor(CENTER_CENTER, window_offset(ctx, id))`) и звать
/// [`keep_on_screen`] до показа и [`record_window`] после.
pub fn drag_handle(ui: &mut egui::Ui, id: Id, rect: Rect) -> egui::Response {
    let drag = ui.interact(rect, title_bar_id(id), Sense::click_and_drag());
    if drag.dragged() {
        set_window_offset(ui.ctx(), id, window_offset(ui.ctx(), id) + drag.drag_delta());
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if drag.double_clicked() {
        set_window_offset(ui.ctx(), id, Vec2::ZERO);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id.with("inline_title"), rect));
    drag
}

/// Поправить смещение окна `id`, чтобы его заголовок не ушёл за край (до показа окна).
pub fn keep_on_screen(ctx: &egui::Context, id: Id) {
    clamp_offset(ctx, id);
}

/// Записать раскладку окна со своей полосой заголовка ([`drag_handle`]) после показа.
pub fn record_window(ctx: &egui::Context, id: Id, frame: Rect) {
    let title = ctx.data_mut(|d| take_temp::<Rect>(d, id.with("inline_title"))).unwrap_or(Rect::from_min_size(frame.min, vec2(frame.width(), TITLE_BAR)));
    store_layout(ctx, id, WindowLayout { frame, title, body: frame, actions: None, content_bottom: frame.bottom() });
}

/// Знак движка (`assets/branding/icon.svg`): белая «P» и янтарная искра на синей плитке,
/// с прозрачностью `alpha` — для заставок (редактор, Piska Hub).
pub fn paint_engine_mark(painter: &egui::Painter, rect: Rect, alpha: f32) {
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let tile = fade(Color32::from_rgb(47, 84, 235));
    let at = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
    let block = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let r = rect.width();
    painter.rect_filled(rect, 0.21 * r, tile);
    let white = fade(Color32::WHITE);
    painter.rect_filled(block(0.46, 0.19, 0.52, 0.57), 0.0, white);
    painter.rect_filled(block(0.29, 0.19, 0.46, 0.81), 0.0, white);
    painter.circle_filled(at(0.52, 0.38), 0.19 * r, white);
    painter.rect_filled(block(0.46, 0.31, 0.53, 0.45), 0.0, tile);
    painter.circle_filled(at(0.53, 0.38), 0.07 * r, tile);
    painter.rect_filled(block(0.58, 0.64, 0.72, 0.78), 0.0, fade(Color32::from_rgb(255, 176, 32)));
}

/// Окно-владелец системных окон «Открыть» / «Сохранить» / «Папка» (HWND на Windows):
/// они встают над окном программы и блокируют его, пока открыты.
pub fn set_file_dialog_owner(window: &winit::window::Window) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok().and_then(|h| match h.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    });
    engine_build::file_dialog::set_owner_window(handle);
}

/// Было ли на прошлом кадре открыто окно [`tool_window`]: тогда Esc — ему, а не горячей
/// клавише программы («Select None»).
pub fn tool_window_was_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(Id::new("kit_tool_window_pass"))).is_some_and(|drawn| drawn + 2 >= pass)
}

/// Окна редактора, чьё место хранится в Preferences между запусками: (имя, id окна).
fn persisted_windows() -> [(&'static str, Id); 4] {
    [
        ("build", Id::new(Id::new("editor_export"))),
        ("preferences", Id::new("editor_preferences")),
        ("project_settings", Id::new("editor_project_settings")),
        ("undo_history", Id::new(Id::new("undo_history_window"))),
    ]
}

impl crate::state::Editor {
    /// Начало кадра: окна Build, Preferences, Project Settings и Undo History встают туда,
    /// где их оставили в прошлый раз (в новом контексте egui места ещё нет).
    pub(crate) fn restore_window_places(&self, ctx: &egui::Context) {
        let key = Id::new("kit_window_places_restored");
        if ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(key, true));
        for (name, id) in persisted_windows() {
            if let Some((_, [x, y])) = self.prefs.window_offsets.iter().find(|(n, _)| n == name) {
                set_window_offset(ctx, id, vec2(*x, *y));
            }
        }
    }

    /// Конец кадра: новое место этих окон — в Preferences (их записывает программа, когда
    /// настройки меняются); пока окно тянут, ничего не пишется.
    pub(crate) fn remember_window_places(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        for (name, id) in persisted_windows() {
            let offset = window_offset(ctx, id);
            let value = [offset.x.round(), offset.y.round()];
            match self.prefs.window_offsets.iter().position(|(n, _)| n == name) {
                Some(at) if self.prefs.window_offsets[at].1 != value => self.prefs.window_offsets[at].1 = value,
                None if offset != Vec2::ZERO => self.prefs.window_offsets.push((name.to_string(), value)),
                _ => {}
            }
        }
    }
}
