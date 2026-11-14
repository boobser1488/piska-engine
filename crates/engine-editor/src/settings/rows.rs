//! Строки страницы настроек: подпись, контрол, описание, стрелка «вернуть по умолчанию».
//!
//! Страница — обычная функция `fn(&mut SettingsUi, &mut Editor)`, которая описывает свои
//! строки через методы [`SettingsUi`]. Одна и та же функция вызывается в трёх проходах:
//!
//! * [`Pass::Draw`] — рисует строки (с учётом поиска: несовпавшие скрыты, совпадения
//!   подсвечены);
//! * [`Pass::Probe`] — ничего не рисует и ничего не меняет, только считает строки,
//!   подходящие под поиск (так поиск видит все страницы сразу);
//! * [`Pass::Reset`] — возвращает каждое значение страницы к умолчанию (Reset page /
//!   Reset all): готовые помощники сами пишут `default`, а [`SettingsUi::row`] и
//!   [`SettingsUi::block`] возвращают `reset: true`.
//!
//! Поэтому всё, что страница меняет, она меняет только через результат этих методов.

use crate::i18n::{tr_s, trf};
use std::ops::RangeInclusive;
use std::path::Path;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, Ui};

use super::SettingsScope;
use crate::icons::{self, Icon};
use crate::theme;

/// Проход страницы (см. модуль).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    Draw,
    Probe,
    Reset,
}

/// Результат строки: нажата стрелка сброса (или идёт проход Reset) и то, что вернул
/// контрол (`None` — строка не рисовалась: другой проход или скрыта поиском).
pub(crate) struct Row<R> {
    pub reset: bool,
    pub inner: Option<R>,
}

/// Высота первой строки (подпись и контрол).
const LINE: f32 = theme::ROW + 4.0;
/// Высота строки описания.
const DESC: f32 = 15.0;
/// Отступ слева под полоску «изменено».
const GUTTER: f32 = theme::SP_M + 2.0;
/// Ширина числового поля.
pub(crate) const NUMBER_W: f32 = 96.0;
/// Ширина выпадающего списка и текстового поля.
pub(crate) const WIDE_W: f32 = 240.0;

/// Строитель страницы настроек — см. модуль.
pub(crate) struct SettingsUi<'a> {
    ui: &'a mut Ui,
    pass: Pass,
    scope: SettingsScope,
    /// Поиск в нижнем регистре; пусто — показывать всё.
    query: String,
    /// Поиск совпал с заголовком страницы — видны все её строки.
    page_hit: bool,
    group: String,
    group_hit: bool,
    group_shown: bool,
    any_shown: bool,
    /// Сколько строк совпало с поиском (или всего строк без поиска).
    pub(crate) hits: usize,
    /// Нарисованные строки: подпись → область (для тестов и автоматизации).
    pub(crate) rows: Vec<(String, Rect)>,
    label_w: f32,
}

impl<'a> SettingsUi<'a> {
    pub(crate) fn new(ui: &'a mut Ui, pass: Pass, scope: SettingsScope, query: &str, page_title: &str) -> Self {
        let query = query.trim().to_lowercase();
        let page_hit = !query.is_empty() && page_title.to_lowercase().contains(&query);
        let label_w = (ui.available_width() * 0.36).clamp(150.0, 210.0);
        Self {
            ui,
            pass,
            scope,
            query,
            page_hit,
            group: String::new(),
            group_hit: false,
            group_shown: false,
            any_shown: false,
            hits: 0,
            rows: Vec::new(),
            label_w,
        }
    }

    /// Строки рисуются (проход Draw) — для контролов, которых нет в помощниках.
    pub(crate) fn drawing(&self) -> bool {
        self.pass == Pass::Draw
    }

    /// Правку можно сохранять: идёт проход Reset или (в проходе Draw) мышь отпущена и
    /// никто не печатает. Для страниц, которые пишут свои файлы сами (Build).
    pub(crate) fn quiet(&self) -> bool {
        match self.pass {
            Pass::Reset => true,
            Pass::Probe => false,
            Pass::Draw => !self.ui.ctx().input(|i| i.pointer.any_down()) && !self.ui.ctx().wants_keyboard_input(),
        }
    }

    /// Начать группу строк. Заголовок группы появляется только перед первой видимой строкой
    /// (если поиск скрыл все строки группы, заголовка нет).
    pub(crate) fn group(&mut self, title: &str) {
        self.group = title.to_string();
        self.group_hit = !self.query.is_empty() && title.to_lowercase().contains(&self.query);
        self.group_shown = false;
    }

    fn hit(&self, label: &str, description: &str) -> bool {
        self.query.is_empty()
            || self.page_hit
            || self.group_hit
            || label.to_lowercase().contains(&self.query)
            || description.to_lowercase().contains(&self.query)
    }

    /// Учесть строку в поиске; `true` — её надо нарисовать сейчас.
    fn begin(&mut self, label: &str, description: &str) -> bool {
        let hit = self.hit(label, description);
        if hit {
            self.hits += 1;
        }
        if self.pass != Pass::Draw || !hit {
            return false;
        }
        self.emit_group();
        true
    }

    /// Заголовок текущей группы, если его ещё не рисовали.
    fn emit_group(&mut self) {
        if !self.group_shown && !self.group.is_empty() {
            if self.any_shown {
                self.ui.add_space(theme::SP_L);
            }
            theme::section_header(self.ui, &self.group);
            self.group_shown = true;
        }
        self.any_shown = true;
    }

    /// Текст с подсветкой совпадения с поиском, обрезанный по ширине.
    fn job(&self, text: &str, font: egui::FontId, color: Color32, width: f32) -> LayoutJob {
        let mut job = LayoutJob::default();
        let plain = TextFormat { font_id: font.clone(), color, ..Default::default() };
        let found = if self.query.is_empty() { None } else { text.to_lowercase().find(&self.query) };
        // Нижний регистр может поменять длину (редкие буквы): тогда без подсветки.
        match found.filter(|_| text.to_lowercase().len() == text.len()) {
            Some(start) if text.is_char_boundary(start) && text.is_char_boundary(start + self.query.len()) => {
                let end = start + self.query.len();
                job.append(&text[..start], 0.0, plain.clone());
                job.append(
                    &text[start..end],
                    0.0,
                    TextFormat { font_id: font, color: theme::TEXT_STRONG, background: theme::accent_soft(), ..Default::default() },
                );
                job.append(&text[end..], 0.0, plain);
            }
            _ => job.append(text, 0.0, plain),
        }
        job.wrap = TextWrapping::truncate_at_width(width.max(10.0));
        job
    }

    /// Первая строка (подпись, место под контрол, стрелка сброса) и описание под ней.
    /// Возвращает (область строки, область контрола, нажат сброс).
    fn frame_row(&mut self, label: &str, description: &str, modified: bool, full_label: bool) -> (Rect, Rect, bool) {
        let width = self.ui.available_width();
        let height = LINE + if description.is_empty() { 0.0 } else { DESC } + theme::SP_S;
        let (rect, response) = self.ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let painter = self.ui.painter().clone();
        if response.hovered() {
            painter.rect_filled(rect, theme::R_S, theme::hover_fill());
        }
        if modified {
            painter.rect_filled(
                Rect::from_min_size(rect.min + vec2(0.0, 4.0), vec2(2.0, LINE - 8.0)),
                0.0,
                theme::accent(),
            );
        }
        // Стрелка сброса — только у изменённой строки.
        let reset_rect = Rect::from_center_size(pos2(rect.right() - 13.0, rect.top() + LINE * 0.5), vec2(20.0, 20.0));
        let mut reset = false;
        if modified {
            let r = self.ui.interact(reset_rect, reset_id(self.scope, label), Sense::click());
            if r.hovered() {
                painter.rect_filled(reset_rect, theme::R_S, theme::BG_HOVER);
            }
            let color = if r.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
            icons::paint(&painter, Rect::from_center_size(reset_rect.center(), vec2(theme::ICON_S, theme::ICON_S)), Icon::Reset, color);
            reset = r.on_hover_text(tr_s("Reset to default")).clicked();
        }
        let label_w = if full_label { reset_rect.left() - rect.left() - GUTTER - theme::SP_S } else { self.label_w };
        let label_rect = Rect::from_min_size(pos2(rect.left() + GUTTER, rect.top()), vec2(label_w, LINE));
        let label_font = if full_label { theme::strong(theme::FONT_TITLE) } else { theme::body() };
        let galley = painter.layout_job(self.job(label, label_font, theme::TEXT, label_w - theme::SP_S));
        painter.galley(pos2(label_rect.left(), label_rect.center().y - galley.size().y * 0.5), galley, theme::TEXT);
        let tip = self.ui.interact(label_rect, row_id(self.scope, label).with("label"), Sense::hover());
        if !description.is_empty() {
            tip.on_hover_text(description);
            let desc_w = reset_rect.left() - rect.left() - GUTTER - theme::SP_S;
            let galley = painter.layout_job(self.job(description, theme::caption(), theme::TEXT_MUTED, desc_w));
            painter.galley(pos2(rect.left() + GUTTER, rect.top() + LINE - 3.0), galley, theme::TEXT_MUTED);
        }
        let control = Rect::from_min_max(
            pos2(label_rect.right() + theme::SP_M, rect.top()),
            pos2(reset_rect.left() - theme::SP_S, rect.top() + LINE),
        );
        self.rows.push((label.to_string(), rect));
        (rect, control, reset)
    }

    /// Общая строка: подпись `label`, описание в одну строку (и подсказка), контрол справа.
    /// `modified` — значение отличается от умолчания: тогда видна стрелка сброса, а её
    /// нажатие (и проход Reset) даёт `reset: true` — значение возвращает вызывающий.
    pub(crate) fn row<R>(&mut self, label: &str, description: &str, modified: bool, control: impl FnOnce(&mut Ui) -> R) -> Row<R> {
        if self.pass == Pass::Reset {
            return Row { reset: true, inner: None };
        }
        if !self.begin(label, description) {
            return Row { reset: false, inner: None };
        }
        let (_, area, reset) = self.frame_row(label, description, modified, false);
        let mut child = self.ui.new_child(
            egui::UiBuilder::new()
                .id_salt(row_id(self.scope, label))
                .max_rect(area)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = theme::SP_S;
        let inner = control(&mut child);
        Row { reset, inner: Some(inner) }
    }

    /// Строка-блок: заголовок и описание во всю ширину, под ними — произвольное
    /// содержимое `add` (списки, матрица слоёв).
    pub(crate) fn block<R>(&mut self, label: &str, description: &str, modified: bool, add: impl FnOnce(&mut Ui) -> R) -> Row<R> {
        if self.pass == Pass::Reset {
            return Row { reset: true, inner: None };
        }
        if !self.begin(label, description) {
            return Row { reset: false, inner: None };
        }
        let (_, _, reset) = self.frame_row(label, description, modified, true);
        let inner = egui::Frame::none()
            .inner_margin(egui::Margin { left: GUTTER, right: theme::SP_S, top: theme::SP_XS, bottom: theme::SP_M })
            .show(self.ui, |ui| {
                ui.set_width(ui.available_width());
                add(ui)
            })
            .inner;
        Row { reset, inner: Some(inner) }
    }

    // ------------------------------------------------------------------
    // Готовые строки: сравнивают с умолчанием и сбрасывают сами.
    // Возвращают `true`, если значение изменилось.
    // ------------------------------------------------------------------

    /// Флажок.
    pub(crate) fn toggle(&mut self, label: &str, description: &str, value: &mut bool, default: bool) -> bool {
        let before = *value;
        let row = self.row(label, description, *value != default, |ui| {
            ui.checkbox(value, "");
        });
        if row.reset {
            *value = default;
        }
        *value != before
    }

    /// Число (перетаскиванием или вводом), `suffix` — единицы («s», « px»).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn number<N: egui::emath::Numeric>(
        &mut self,
        label: &str,
        description: &str,
        value: &mut N,
        default: N,
        range: RangeInclusive<N>,
        speed: f64,
        suffix: &str,
    ) -> bool {
        let before = *value;
        let differs = |a: N, b: N| (a.to_f64() - b.to_f64()).abs() > 1e-6;
        let row = self.row(label, description, differs(*value, default), |ui| {
            crate::controls::number_field_range(ui, value, range, speed, suffix, NUMBER_W);
        });
        if row.reset {
            *value = default;
        }
        differs(*value, before)
    }

    /// Доля 0…1 ползунком, показывается в процентах.
    pub(crate) fn percent(&mut self, label: &str, description: &str, value: &mut f32, default: f32) -> bool {
        let before = *value;
        let row = self.row(label, description, (*value - default).abs() > 1e-4, |ui| {
            ui.spacing_mut().slider_width = WIDE_W - 56.0;
            ui.add(egui::Slider::new(value, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)).custom_parser(|s| {
                s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)
            }));
        });
        if row.reset {
            *value = default;
        }
        (*value - before).abs() > 1e-6
    }

    /// Вектор из `N` чисел с буквами осей (гравитация).
    pub(crate) fn vector<const N: usize>(
        &mut self,
        label: &str,
        description: &str,
        value: &mut [f32; N],
        default: [f32; N],
        speed: f64,
    ) -> bool {
        let before = *value;
        let row = self.row(label, description, *value != default, |ui| {
            // Те же ячейки «метка оси + поле», что в инспекторе: промежутки — из шкалы.
            let width = (N as f32 * 80.0).min(ui.available_width());
            ui.allocate_ui(egui::vec2(width, theme::CONTROL_H), |ui| crate::inspector::axes(ui, value, speed));
        });
        if row.reset {
            *value = default;
        }
        *value != before
    }

    /// Выбор из списка вариантов.
    pub(crate) fn combo<T: PartialEq + Copy>(
        &mut self,
        label: &str,
        description: &str,
        value: &mut T,
        default: T,
        options: &[(T, &str)],
    ) -> bool {
        let before = *value;
        let scope = self.scope;
        let row = self.row(label, description, *value != default, |ui| {
            let selected = options.iter().find(|(v, _)| *v == *value).map_or("", |(_, name)| *name);
            crate::controls::combo(ui, ("settings_combo", scope, label), selected, WIDE_W, |ui| {
                for (option, name) in options {
                    ui.selectable_value(value, *option, *name);
                }
            });
        });
        if row.reset {
            *value = default;
        }
        *value != before
    }

    /// Выбор строки из списка (`options`, например пути ассетов); пустая строка — `none`.
    /// Значение, которого нет в списке, показывается с пометкой «missing».
    pub(crate) fn choice(
        &mut self,
        label: &str,
        description: &str,
        value: &mut String,
        default: &str,
        options: &[String],
        none: &str,
    ) -> bool {
        let before = value.clone();
        let scope = self.scope;
        let row = self.row(label, description, value.as_str() != default, |ui| {
            let missing = !value.is_empty() && !options.iter().any(|o| o == value);
            let shown = if value.is_empty() {
                none.to_string()
            } else if missing {
                trf!("{value} (missing)", value = value)
            } else {
                value.clone()
            };
            let shown = egui::RichText::new(shown).color(if missing { theme::WARN } else { theme::TEXT });
            crate::controls::combo_ex(ui, ("settings_choice", scope, label), shown, WIDE_W, |ui| {
                    ui.selectable_value(value, String::new(), none);
                    for option in options {
                        ui.selectable_value(value, option.clone(), option.as_str());
                    }
                });
        });
        if row.reset {
            *value = default.to_string();
        }
        *value != before
    }

    /// Строка текста.
    pub(crate) fn text(&mut self, label: &str, description: &str, value: &mut String, default: &str, hint: &str) -> bool {
        let before = value.clone();
        let row = self.row(label, description, value.as_str() != default, |ui| {
            crate::controls::text_field(ui, value, hint, WIDE_W);
        });
        if row.reset {
            *value = default.to_string();
        }
        *value != before
    }

    // `path` и `color` — для страниц Preferences (Build & Tools, Scene View), которые
    // добавляются следующими; страницы этого набора их пока не используют.
    #[allow(dead_code)]
    /// Путь к файлу или каталогу: поле моноширинным шрифтом и значок «есть / нет на
    /// диске». Относительный путь считается от `base` (если задан).
    pub(crate) fn path(&mut self, label: &str, description: &str, value: &mut String, default: &str, base: Option<&Path>) -> bool {
        let before = value.clone();
        let row = self.row(label, description, value.as_str() != default, |ui| {
            let width = (ui.available_width() - theme::ICON - theme::SP_S * 2.0).max(80.0);
            crate::controls::text_field_font(ui, None, value, "", width, theme::mono());
            if !value.trim().is_empty() {
                let path = Path::new(value.trim());
                let full = match base {
                    Some(base) if path.is_relative() => base.join(path),
                    _ => path.to_path_buf(),
                };
                let (rect, response) = ui.allocate_exact_size(vec2(theme::ICON, theme::ICON), Sense::hover());
                let (icon, color, tip) = if full.exists() {
                    (Icon::Check, theme::OK, tr_s("Found on disk"))
                } else {
                    (Icon::Warn, theme::WARN, tr_s("Not found on disk"))
                };
                icons::paint(ui.painter(), rect.shrink(1.0), icon, color);
                response.on_hover_text(format!("{tip}: {}", full.display()));
            }
        });
        if row.reset {
            *value = default.to_string();
        }
        *value != before
    }

    /// Цвет (кнопка с образцом и палитрой).
    #[allow(dead_code)]
    pub(crate) fn color(&mut self, label: &str, description: &str, value: &mut Color32, default: Color32) -> bool {
        let before = *value;
        let row = self.row(label, description, *value != default, |ui| {
            crate::controls::color_button(ui, value);
        });
        if row.reset {
            *value = default;
        }
        *value != before
    }

    /// Строка-пояснение мелким текстом (видна, когда поиск пуст).
    pub(crate) fn note(&mut self, text: &str) {
        if self.pass != Pass::Draw || !self.query.is_empty() {
            return;
        }
        self.emit_group();
        let ui = &mut *self.ui;
        egui::Frame::none().inner_margin(egui::Margin { left: GUTTER, right: theme::SP_S, top: 0.0, bottom: theme::SP_S }).show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).font(theme::caption()).color(theme::TEXT_MUTED)).wrap());
        });
    }
}

/// Идентификатор строки настройки `label`.
pub(crate) fn row_id(scope: SettingsScope, label: &str) -> egui::Id {
    egui::Id::new(("settings_row", scope, label.to_string()))
}

/// Идентификатор стрелки сброса строки `label`.
pub(crate) fn reset_id(scope: SettingsScope, label: &str) -> egui::Id {
    egui::Id::new(("settings_reset", scope, label.to_string()))
}

/// Линия-разделитель на всю ширину (между частями собственных блоков).
pub(crate) fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, theme::LINE_SOFT));
}
