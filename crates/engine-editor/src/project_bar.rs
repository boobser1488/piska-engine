//! Полоса фильтров окна Project (чипы по типу и выбор сортировки) и заголовки столбцов
//! списка. Логика сортировки — в `sorting.rs`, выбор хранится в настройках редактора.

use crate::i18n::{tr_s, trf};
use std::collections::BTreeSet;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::icons::{self, Icon};
use crate::sorting::{filters_text, parse_filters, SortKey, SortSpec, TypeFilter};
use crate::state::Editor;
use crate::theme::{self, item};

impl Editor {
    /// Сортировка окна Project из настроек.
    pub(crate) fn project_sort_spec(&self) -> SortSpec {
        SortSpec {
            key: SortKey::parse(&self.prefs.project_sort).unwrap_or(SortKey::Name),
            descending: self.prefs.project_sort_desc,
        }
    }

    pub(crate) fn set_project_sort_spec(&mut self, spec: SortSpec) {
        self.prefs.project_sort = spec.key.id().to_string();
        self.prefs.project_sort_desc = spec.descending;
    }

    pub(crate) fn project_type_set(&self) -> BTreeSet<TypeFilter> {
        parse_filters(&self.prefs.project_types)
    }

    /// Сортировка окна Project: ключ (`name`, `type`, `modified`, `size`) и «по убыванию».
    pub fn project_sort(&self) -> (&'static str, bool) {
        let spec = self.project_sort_spec();
        (spec.key.id(), spec.descending)
    }

    /// Выбрать сортировку окна Project. Неизвестный ключ не меняет ничего.
    pub fn set_project_sort(&mut self, key: &str, descending: bool) {
        if let Some(key) = SortKey::parse(key) {
            self.set_project_sort_spec(SortSpec { key, descending });
        }
    }

    /// Включённые фильтры по типу (`scripts`, `scenes`, `prefabs`, `models`, `materials`,
    /// `textures`, `audio`, `other`); пусто — показано всё.
    pub fn project_type_filters(&self) -> Vec<&'static str> {
        self.project_type_set().iter().map(|f| f.id()).collect()
    }

    /// Задать фильтры по типу; неизвестные имена пропускаются.
    pub fn set_project_type_filters(&mut self, filters: &[&str]) {
        let set: BTreeSet<TypeFilter> = filters.iter().filter_map(|name| TypeFilter::parse(name)).collect();
        self.prefs.project_types = filters_text(&set);
    }

    /// Идентификатор чипа фильтра окна Project: имя типа или `"all"`.
    pub fn project_filter_chip_id(name: &str) -> egui::Id {
        chip_id(name)
    }

    /// Идентификатор заголовка столбца списка окна Project (`name`, `type`, `modified`, `size`).
    pub fn project_sort_header_id(key: &str) -> egui::Id {
        header_id(key)
    }

    /// Имена элементов окна Project в том порядке, в каком они нарисованы в последнем кадре.
    pub fn project_shown(&self) -> &[String] {
        &self.project.shown
    }

    /// Заголовки групп окна Code в последнем кадре: «Rust  ·  2», «C++  ·  1»…
    pub fn project_group_headings(&self) -> &[String] {
        &self.project.headings
    }

    /// Текст в поле поиска окна Project (пусто — поиска нет).
    pub fn set_project_search(&mut self, text: &str) {
        self.project.filter = text.to_string();
    }

    /// Идентификатор кнопки выбора сортировки.
    pub fn project_sort_button_id() -> egui::Id {
        egui::Id::new("project_sort_button")
    }
}

pub(crate) fn chip_id(name: &str) -> egui::Id {
    egui::Id::new(("project_type_chip", name.to_string()))
}

pub(crate) fn header_id(key: &str) -> egui::Id {
    egui::Id::new(("project_sort_header", key.to_string()))
}

/// Геометрия столбцов списка: справа налево Size, Modified, Type; остальное — имя.
/// Ширины столбцов задаёт пользователь (тянет границы в заголовке), они запоминаются.
#[derive(Clone, Copy)]
pub(crate) struct Columns {
    pub size_right: f32,
    pub modified_right: Option<f32>,
    pub type_right: Option<f32>,
    /// Левая граница столбцов: имя обрезается тут.
    pub name_limit: f32,
}

impl Columns {
    /// Столбцы убираются (сначала Type, потом Modified), если имени остаётся меньше этого.
    const MIN_NAME: f32 = 100.0;
    pub const MIN_WIDTH: f32 = 48.0;
    pub const MAX_WIDTH: f32 = 400.0;

    pub fn new(rect: Rect, widths: [f32; 3]) -> Self {
        let [kind, modified, size] = widths;
        let width = rect.width();
        let mut right = rect.right() - theme::SP_M;
        let size_right = right;
        right -= size;
        let modified_right = (width - size - modified - 2.0 * theme::SP_M >= Self::MIN_NAME).then(|| {
            let at = right;
            right -= modified;
            at
        });
        let type_right = (modified_right.is_some() && width - size - modified - kind - 2.0 * theme::SP_M >= Self::MIN_NAME).then(|| {
            let at = right;
            right -= kind;
            at
        });
        Columns { size_right, modified_right, type_right, name_limit: right }
    }

    pub fn right_of(&self, key: SortKey) -> Option<f32> {
        match key {
            SortKey::Name => None,
            SortKey::Type => self.type_right,
            SortKey::Modified => self.modified_right,
            SortKey::Size => Some(self.size_right),
        }
    }

    /// Индекс ширины в `widths`.
    fn slot(key: SortKey) -> Option<usize> {
        match key {
            SortKey::Name => None,
            SortKey::Type => Some(0),
            SortKey::Modified => Some(1),
            SortKey::Size => Some(2),
        }
    }
}

/// Заголовок списка: Name, Type, Date modified, Size; щелчок сортирует, стрелка показывает
/// направление, границы столбцов тянутся мышью.
pub(crate) fn list_header(ui: &mut egui::Ui, spec: SortSpec, widths: &mut [f32; 3]) -> SortSpec {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::BG_HEADER);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
    let columns = Columns::new(rect, *widths);
    let mut next = spec;
    for key in SortKey::ALL {
        let cell = match key {
            SortKey::Name => Rect::from_min_max(rect.min + vec2(theme::SP_M, 0.0), pos2(columns.name_limit, rect.bottom())),
            other => {
                let (Some(right), Some(slot)) = (columns.right_of(other), Columns::slot(other)) else { continue };
                Rect::from_min_max(pos2(right - widths[slot], rect.top()), pos2(right, rect.bottom()))
            }
        };
        let response = ui
            .interact(cell, header_id(key.id()), Sense::click())
            .on_hover_text(trf!("Sort by {}", key.label().to_lowercase()));
        let active = spec.key == key;
        if response.hovered() {
            ui.painter().rect_filled(cell, 0.0, theme::BG_HOVER);
        }
        let color = if active { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
        let label = if key == SortKey::Modified { tr_s("Modified") } else { key.label() };
        let align = if key == SortKey::Name { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER };
        let anchor = if key == SortKey::Name { pos2(cell.left(), cell.center().y) } else { pos2(cell.right() - 14.0, cell.center().y) };
        ui.painter().with_clip_rect(cell).text(anchor, align, label, theme::caption(), color);
        if active {
            let arrow = Rect::from_center_size(
                pos2(if key == SortKey::Name { cell.left() + ui.painter().layout_no_wrap(label.to_string(), theme::caption(), color).size().x + 12.0 } else { cell.right() - 7.0 }, cell.center().y),
                vec2(9.0, 9.0),
            );
            icons::paint(ui.painter(), arrow, if spec.descending { Icon::ChevronDown } else { Icon::ChevronUp }, theme::TEXT_MUTED);
        }
        if response.clicked() {
            next.click(key);
        }
        // Граница столбца: тянуть влево — шире, вправо — уже.
        if let Some(slot) = Columns::slot(key) {
            let handle = Rect::from_center_size(pos2(cell.left(), cell.center().y), vec2(7.0, cell.height()));
            let grip = ui.interact(handle, egui::Id::new(("project_column_grip", key.id())), Sense::drag());
            if grip.hovered() || grip.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if grip.dragged() {
                widths[slot] = (widths[slot] - grip.drag_delta().x).clamp(Columns::MIN_WIDTH, Columns::MAX_WIDTH);
            }
            ui.painter().vline(
                cell.left(),
                cell.y_range().shrink(5.0),
                Stroke::new(1.0_f32, if grip.hovered() || grip.dragged() { theme::LINE_HOVER } else { theme::LINE_SOFT }),
            );
        }
    }
    next
}

/// Полоса под панелью инструментов: чипы типов слева, сортировка справа.
pub(crate) fn filter_bar(editor: &mut Editor, ui: &mut egui::Ui, show_chips: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::STRIP), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(theme::SP_M, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar.spacing_mut().item_spacing.x = theme::SP_S;

    // Сортировка — справа, чипы занимают остальное.
    let spec = editor.project_sort_spec();
    let sort_label = trf!("Sort: {} {}", spec.key.label(), if spec.descending { "\u{2193}" } else { "\u{2191}" });
    let button_width = 150.0_f32;
    let chips_rect = Rect::from_min_max(bar.max_rect().min, pos2(bar.max_rect().right() - button_width - theme::SP_M, bar.max_rect().bottom()));
    let mut chips_ui = bar.new_child(
        egui::UiBuilder::new().max_rect(chips_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    chips_ui.spacing_mut().item_spacing.x = theme::SP_S;
    // Узкое окно (или крупный масштаб): чипы, не влезшие до кнопки сортировки, обрезаются,
    // а не наезжают на неё.
    chips_ui.set_clip_rect(chips_rect.intersect(ui.clip_rect()));
    if show_chips {
        let mut filters = editor.project_type_set();
        let before = filters.clone();
        if chip(&mut chips_ui, "all", tr_s("All"), filters.is_empty()).clicked() {
            filters.clear();
        }
        for filter in TypeFilter::ALL {
            let clicked = chip(&mut chips_ui, filter.id(), filter.label(), filters.contains(&filter)).clicked();
            if clicked && !filters.remove(&filter) {
                filters.insert(filter);
            }
        }
        if filters != before {
            editor.prefs.project_types = filters_text(&filters);
        }
    } else {
        chips_ui.label(egui::RichText::new(tr_s("Scripts grouped by language")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
    }

    let sort_rect = Rect::from_min_max(pos2(bar.max_rect().right() - button_width, bar.max_rect().top() + 4.0), pos2(bar.max_rect().right(), bar.max_rect().bottom() - 4.0));
    let mut sort_ui = bar.new_child(egui::UiBuilder::new().max_rect(sort_rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
    let mut chosen = None;
    let response = crate::controls::menu_button(
        &mut sort_ui,
        egui::RichText::new(sort_label).size(theme::FONT_CAPTION).color(theme::TEXT),
        button_width,
        |ui| {
            theme::menu_body(ui);
            theme::menu_section(ui, tr_s("Sort by"));
            for key in SortKey::ALL {
                if item(key.label()).checked(spec.key == key).show(ui) {
                    chosen = Some(SortSpec { key, descending: spec.descending });
                }
            }
            theme::menu_separator(ui);
            if item(tr_s("Ascending")).checked(!spec.descending).show(ui) {
                chosen = Some(SortSpec { descending: false, ..spec });
            }
            if item(tr_s("Descending")).checked(spec.descending).show(ui) {
                chosen = Some(SortSpec { descending: true, ..spec });
            }
        },
    )
    .response;
    response.on_hover_text(tr_s("Folders are always listed first"));
    if let Some(spec) = chosen {
        editor.set_project_sort_spec(spec);
    }
}

/// Чип-переключатель: прямоугольная кнопка с подписью.
fn chip(ui: &mut egui::Ui, id: &str, label: &str, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::caption(), Color32::PLACEHOLDER);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 18.0, theme::CONTROL_H), Sense::hover());
    let response = ui.interact(rect, chip_id(id), Sense::click());
    let fill = if on {
        theme::accent_soft()
    } else if response.hovered() {
        theme::BG_HOVER
    } else {
        theme::BG_FIELD
    };
    crate::controls::paint_frame(ui.painter(), rect, theme::R_S, fill, theme::LINE_STRONG);
    let color = if on { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
    response.on_hover_text(if id == "all" { tr_s("Show every type") } else { tr_s("Show only this type (click several to combine)") })
}
