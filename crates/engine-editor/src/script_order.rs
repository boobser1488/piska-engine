//! Страница Project Settings → Script Execution Order: порядок выполнения скриптов.
//!
//! Список типов с числами (меньше — раньше), линия «Default Time» на нуле, «+» добавляет
//! тип из реестра, строки перетаскиваются (число выбирается между соседями), Apply пишет
//! `project.settings` в каталог ассетов и сразу применяет порядок к сцене — в том числе в
//! режиме Play. Логика порядка строк — чистые функции внизу файла (под тестами).
//!
//! В отличие от остальных страниц настроек, здесь правки ждут Apply: перетаскивание даёт
//! промежуточные числа, а каждое применение пересортировывает вызовы уже работающих
//! компонентов (в том числе в Play). Неприменённые правки живут в `draft` этой страницы;
//! остальные страницы при записи файла берут действующий порядок.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine_core::prelude::*;

use crate::add_menu;
use crate::icons::{self, Icon};
use crate::sorting::natural_cmp;
use crate::state::Editor;
use crate::theme;
use crate::EditorError;

/// Имя строки-линии «Default Time» в списке строк.
pub(crate) const MARKER: &str = "\u{1}default";

/// Состояние окна.
#[derive(Default)]
pub(crate) struct ScriptOrderState {
    /// Открыто окно Project Settings (любая его страница).
    pub open: bool,
    /// Правится на странице (используется только `script_order`); Apply делает это
    /// действующим — переносит в черновик настроек проекта и записывает файл.
    pub draft: ProjectSettings,
    /// Черновик взят из настроек проекта (при первом открытии).
    loaded: bool,
    search: String,
    error: Option<String>,
    /// Прямоугольники строк последнего кадра (имя → область) — для тестов и автоматизации.
    pub(crate) rows: Vec<(String, Rect)>,
}

/// Что тащат: имя типа.
struct DragRow(String);

pub(crate) fn row_id(name: &str) -> egui::Id {
    egui::Id::new(("script_order_row", name.to_string()))
}

pub(crate) fn handle_id(name: &str) -> egui::Id {
    egui::Id::new(("script_order_handle", name.to_string()))
}

impl Editor {
    /// Открыть или закрыть окно Project Settings.
    pub fn show_project_settings(&mut self, open: bool) {
        self.script_order.open = open;
    }

    pub fn is_project_settings_open(&self) -> bool {
        self.script_order.open
    }

    /// Строки порядка выполнения, как их показывает окно, без линии «Default Time»:
    /// (тип, число) по возрастанию числа.
    pub fn script_order_draft(&self) -> Vec<(String, i32)> {
        let mut rows: Vec<(String, i32)> = self.script_order.draft.script_order.clone().into_iter().collect();
        rows.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| natural_cmp(&a.0, &b.0)));
        rows
    }

    /// Задать число типу в черновике (как ввод в поле строки); Apply его применит.
    pub fn set_script_order(&mut self, type_name: &str, order: i32) {
        self.ensure_script_order_loaded();
        self.script_order.draft.set_order(type_name, order);
    }

    /// Убрать тип из черновика: он вернётся к порядку из кода.
    pub fn remove_script_order(&mut self, type_name: &str) {
        self.ensure_script_order_loaded();
        self.script_order.draft.clear_order(type_name);
    }

    /// Отличается ли черновик от действующих настроек (кнопка Apply доступна).
    pub fn script_order_is_dirty(&self) -> bool {
        self.script_order.loaded && self.script_order.draft.script_order != self.project_settings_draft().script_order
    }

    /// Записать черновик в `project.settings` и применить к сцене (кнопка Apply).
    pub fn apply_script_order(&mut self) -> Result<(), EditorError> {
        self.ensure_script_order_loaded();
        let order = self.script_order.draft.script_order.clone();
        self.assets_root().ok_or_else(|| {
            EditorError::Other(
                "cannot save the script execution order: no assets directory is mounted. \
                 Call `engine::mount_assets(&mut app, \"assets\")` before starting the editor"
                    .to_string(),
            )
        })?;
        self.project_draft_mut().script_order = order;
        self.commit_project_settings(true)?;
        self.script_order.error = None;
        Ok(())
    }

    /// Вернуть черновик к действующим настройкам (кнопка Revert).
    pub fn revert_script_order(&mut self) {
        self.ensure_script_order_loaded();
        self.script_order.draft.script_order = self.project_settings_draft().script_order;
        self.script_order.error = None;
    }

    /// Идентификатор строки типа в окне Project Settings.
    pub fn script_order_row_id(type_name: &str) -> egui::Id {
        row_id(type_name)
    }

    /// Идентификатор «ручки» строки (за неё строку тащат).
    pub fn script_order_handle_id(type_name: &str) -> egui::Id {
        handle_id(type_name)
    }

    /// Идентификатор кнопки Apply.
    pub fn script_order_apply_id() -> egui::Id {
        egui::Id::new("script_order_apply")
    }

    /// Идентификатор кнопки Revert.
    pub fn script_order_revert_id() -> egui::Id {
        egui::Id::new("script_order_revert")
    }

    /// Где нарисована строка типа в последнем кадре.
    pub fn script_order_row_rect(&self, type_name: &str) -> Option<egui::Rect> {
        self.script_order.rows.iter().find(|(name, _)| name == type_name).map(|(_, rect)| *rect)
    }

    pub(crate) fn ensure_script_order_loaded(&mut self) {
        if !self.script_order.loaded {
            self.script_order.loaded = true;
            self.ensure_project_draft();
            self.script_order.draft = self.project_settings_draft();
        }
    }

    fn script_order_ui(&mut self, ui: &mut egui::Ui) {
        self.ensure_script_order_loaded();

        // Тип → (язык, порядок в коде): из реестра.
        let registry_info: std::collections::BTreeMap<&'static str, (Language, i32)> = self
            .app
            .world
            .registry
            .names()
            .map(|name| {
                let category = self.app.world.registry.category_of(name);
                (name, (language_of(category), self.app.world.registry.default_order_of(name).unwrap_or(0)))
            })
            .collect();

        // «+» с поиском по зарегистрированным типам.
        let mut add: Option<String> = None;
        ui.horizontal(|ui| {
            let popup = ui.make_persistent_id("script_order_add_popup");
            let plus = theme::icon_text_button(ui, Icon::Plus, "Add script");
            if plus.clicked() {
                self.script_order.search.clear();
                ui.memory_mut(|m| m.toggle_popup(popup));
            }
            egui::popup_below_widget(ui, popup, &plus, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                ui.set_min_width(340.0);
                let search = theme::search_field(ui, &mut self.script_order.search, "Search components", f32::INFINITY);
                search.request_focus();
                ui.add_space(theme::SP_S);
                let listed = &self.script_order.draft.script_order;
                let entries: Vec<add_menu::AddEntry> = add_menu::entries(&self.app.world.registry)
                    .into_iter()
                    .filter(|e| !listed.contains_key(e.kind))
                    .collect();
                let found = add_menu::search(&entries, &self.script_order.search);
                egui::ScrollArea::vertical().id_salt("script_order_add_list").max_height(280.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for entry in &found {
                        let language = registry_info.get(entry.kind).map_or(Language::Engine, |(l, _)| *l);
                        if theme::item(entry.kind)
                            .icon(crate::catalog::component_icon(entry.kind))
                            .shortcut(language.label())
                            .show(ui)
                        {
                            add = Some(entry.kind.to_string());
                            ui.memory_mut(|m| m.close_popup());
                        }
                    }
                    if found.is_empty() {
                        ui.add_space(theme::SP_M);
                        ui.label(egui::RichText::new("No component to add").color(theme::TEXT_MUTED));
                    }
                });
            });
            ui.label(
                egui::RichText::new("Rust, C++ and engine components that are registered in this editor")
                    .size(theme::FONT_CAPTION)
                    .color(theme::TEXT_DISABLED),
            );
        });
        if let Some(name) = add {
            let default = registry_info.get(name.as_str()).map_or(0, |(_, order)| *order);
            self.script_order.draft.set_order(&name, default);
        }
        ui.add_space(theme::SP_S);

        // Список строк с линией Default Time.
        let display = display_rows(&self.script_order.draft);
        let mut edit: Option<(String, i32)> = None;
        let mut remove: Option<String> = None;
        let mut moved: Option<(String, usize)> = None;
        let mut rects: Vec<(String, Rect)> = Vec::new();
        let dragging = egui::DragAndDrop::payload::<DragRow>(ui.ctx()).map(|p| p.0.clone());
        let list_height = (ui.available_height() - 56.0).clamp(160.0, 420.0);
        egui::Frame::none()
            .fill(theme::BG_INSET)
            .stroke(Stroke::new(1.0_f32, theme::LINE))
            .rounding(theme::R_M)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().id_salt("script_order_rows").max_height(list_height).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let top = ui.cursor().min;
                    for (name, order) in &display {
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW + 6.0), Sense::hover());
                        rects.push((name.clone(), rect));
                        if name == MARKER {
                            paint_marker(ui, rect, display.len() == 1);
                            continue;
                        }
                        let info = registry_info.get(name.as_str()).copied();
                        let outcome = paint_row(ui, rect, name, *order, info, dragging.as_deref() == Some(name.as_str()));
                        if let Some(new) = outcome.new_order {
                            edit = Some((name.clone(), new));
                        }
                        if outcome.remove {
                            remove = Some(name.clone());
                        }
                    }
                    let bottom = ui.cursor().min;
                    let list = Rect::from_min_max(top, pos2(top.x + ui.available_width(), bottom.y.max(top.y + 1.0)));
                    // Куда бросят: щель между строками ближе всего к указателю.
                    let drop_zone = ui.interact(list.expand2(vec2(0.0, 12.0)), egui::Id::new("script_order_drop_zone"), Sense::hover());
                    if let (Some(name), Some(pointer)) = (&dragging, ui.ctx().pointer_latest_pos()) {
                        if drop_zone.contains_pointer() {
                            let gap = rects.iter().filter(|(_, r)| r.center().y < pointer.y).count();
                            let y = if gap == 0 { rects[0].1.top() } else { rects[gap - 1].1.bottom() };
                            ui.painter().hline(
                                list.x_range().shrink(6.0),
                                y,
                                Stroke::new(2.0_f32, theme::accent()),
                            );
                            if let Some(payload) = drop_zone.dnd_release_payload::<DragRow>() {
                                moved = Some((payload.0.clone(), gap));
                            }
                        }
                        let _ = name;
                    }
                });
            });
        self.script_order.rows = rects.into_iter().filter(|(n, _)| n != MARKER).collect();
        if let Some((name, order)) = edit {
            self.script_order.draft.set_order(&name, order);
        }
        if let Some(name) = remove {
            self.script_order.draft.clear_order(&name);
        }
        if let Some((name, gap)) = moved {
            for (changed, order) in drop_row(&display, &name, gap) {
                self.script_order.draft.set_order(&changed, order);
            }
        }

        if let Some(error) = &self.script_order.error {
            ui.add_space(theme::SP_S);
            ui.label(egui::RichText::new(error).color(theme::ERROR));
        }
        ui.add_space(theme::SP_M);
        let dirty = self.script_order_is_dirty();
        ui.horizontal(|ui| {
            let apply = ui.add_enabled_ui(dirty, |ui| theme::primary_button(ui, "Apply")).inner;
            let apply = ui.interact(apply.rect, Editor::script_order_apply_id(), Sense::click()).on_hover_text("Save project.settings and use this order right away, also in Play");
            if dirty && apply.clicked() {
                if let Err(error) = self.apply_script_order() {
                    self.script_order.error = Some(error.to_string());
                }
            }
            let revert = ui.add_enabled_ui(dirty, |ui| theme::secondary_button(ui, "Revert")).inner;
            let revert = ui.interact(revert.rect, Editor::script_order_revert_id(), Sense::click());
            if dirty && revert.clicked() {
                self.revert_script_order();
            }
            ui.label(
                egui::RichText::new(if dirty { "Unsaved changes" } else { "Saved in project.settings" })
                    .size(theme::FONT_CAPTION)
                    .color(if dirty { theme::WARN } else { theme::TEXT_MUTED }),
            );
        });
    }
}

/// Страница Script Execution Order окна Project Settings.
pub(crate) fn page(s: &mut crate::settings::SettingsUi<'_>, editor: &mut Editor) {
    editor.ensure_script_order_loaded();
    let modified = !editor.project_settings_draft().script_order.is_empty() || editor.script_order_is_dirty();
    let row = s.block(
        "Execution order",
        "Lowest number runs first in Start, FixedUpdate, Update and LateUpdate. Unlisted types run at Default Time (0).",
        modified,
        |ui| editor.script_order_ui(ui),
    );
    if row.reset {
        editor.script_order.draft.script_order.clear();
        editor.project_draft_mut().script_order.clear();
    }
}

/// Откуда тип: по его категории в реестре.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    Rust,
    Cpp,
    Scenario,
    Engine,
    /// Типа нет в реестре этого редактора (плагин не загружен).
    Missing,
}

impl Language {
    pub fn label(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Cpp => "C++",
            Language::Scenario => "Scenario",
            Language::Engine => "Engine",
            Language::Missing => "Not registered",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Language::Rust => Color32::from_rgb(0xe0, 0x8a, 0x5a),
            Language::Cpp => Color32::from_rgb(0x6c, 0x9c, 0xe8),
            Language::Scenario => Color32::from_rgb(0x4f, 0xd1, 0xc5),
            Language::Engine => theme::TEXT_MUTED,
            Language::Missing => theme::ERROR,
        }
    }
}

/// Язык по категории типа в реестре: без категории и «Scripts» — Rust, «Scripts (C++)» —
/// C++, «Scripts (Scenario)» — Scenario, остальные категории — компоненты движка.
pub(crate) fn language_of(category: Option<&str>) -> Language {
    match category {
        None | Some("Scripts") => Language::Rust,
        Some("Scripts (C++)") => Language::Cpp,
        Some("Scripts (Scenario)") => Language::Scenario,
        Some(_) => Language::Engine,
    }
}

struct RowOutcome {
    new_order: Option<i32>,
    remove: bool,
}

fn paint_row(ui: &mut egui::Ui, rect: Rect, name: &str, order: i32, info: Option<(Language, i32)>, being_dragged: bool) -> RowOutcome {
    let mut outcome = RowOutcome { new_order: None, remove: false };
    let hover = ui.interact(rect, row_id(name), Sense::hover());
    if being_dragged {
        ui.painter().rect_filled(rect.shrink2(vec2(2.0, 1.0)), theme::R_S, theme::selection_fill());
    } else if hover.hovered() {
        ui.painter().rect_filled(rect.shrink2(vec2(2.0, 1.0)), theme::R_S, theme::BG_HOVER);
    }
    // Ручка: шесть точек; за неё и за имя строку тащат.
    let handle = Rect::from_min_max(rect.min, pos2(rect.left() + 230.0, rect.bottom()));
    let drag = ui.interact(handle, handle_id(name), Sense::drag());
    if drag.drag_started() {
        drag.dnd_set_drag_payload(DragRow(name.to_string()));
    }
    if drag.hovered() || drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let dots = if drag.hovered() { theme::TEXT } else { theme::TEXT_DISABLED };
    for column in 0..2 {
        for row in 0..3 {
            ui.painter().circle_filled(pos2(rect.left() + 12.0 + column as f32 * 5.0, rect.center().y - 5.0 + row as f32 * 5.0), 1.2, dots);
        }
    }
    let language = info.map_or(Language::Missing, |(language, _)| language);
    ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(rect.left() + 232.0, rect.bottom()))).text(
        pos2(rect.left() + 28.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::body(),
        if language == Language::Missing { theme::TEXT_MUTED } else { theme::TEXT },
    );
    // Бейдж языка.
    let badge = ui.painter().layout_no_wrap(language.label().to_string(), theme::strong(11.0), language.color());
    let badge_rect = Rect::from_min_size(pos2(rect.left() + 238.0, rect.center().y - 9.0), badge.size() + vec2(12.0, 4.0));
    ui.painter().rect_stroke(badge_rect, theme::R_S, Stroke::new(1.0_f32, language.color().gamma_multiply(0.7)));
    ui.painter().galley(badge_rect.min + vec2(6.0, 2.0), badge, language.color());
    // Число.
    let field = Rect::from_center_size(pos2(rect.right() - 74.0, rect.center().y), vec2(78.0, 24.0));
    let mut value = order;
    let response = ui.put(field, egui::DragValue::new(&mut value).speed(1.0).range(-32_000..=32_000));
    let hint = match info {
        Some((_, code)) if code != order => format!("Order in code: {code}. Remove the row to use it."),
        Some(_) => "The same as in code".to_string(),
        None => "This component is not registered in this editor: its order still applies in games that register it".to_string(),
    };
    response.on_hover_text(hint);
    if value != order {
        outcome.new_order = Some(value);
    }
    let remove = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(22.0, 22.0));
    let remove_response = ui.interact(remove, egui::Id::new(("script_order_remove", name.to_string())), Sense::click());
    if remove_response.hovered() {
        ui.painter().rect_filled(remove, theme::R_S, theme::BG_ACTIVE);
    }
    icons::paint(ui.painter(), remove.shrink(6.0), Icon::Close, if remove_response.hovered() { Color32::WHITE } else { theme::TEXT_MUTED });
    if remove_response.on_hover_text("Remove from the list: the component goes back to the order from its code").clicked() {
        outcome.remove = true;
    }
    outcome
}

fn paint_marker(ui: &mut egui::Ui, rect: Rect, alone: bool) {
    let y = rect.center().y;
    ui.painter().hline(rect.x_range().shrink(8.0), y, Stroke::new(1.0_f32, theme::accent()));
    let text = if alone { "Default Time  \u{b7}  0  \u{b7}  add a script to run before or after everything else" } else { "Default Time  \u{b7}  0" };
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::strong(11.5), theme::accent());
    let pill = Rect::from_center_size(rect.center(), galley.size() + vec2(20.0, 6.0));
    ui.painter().rect_filled(pill, theme::R_S, theme::BG_INSET);
    ui.painter().rect_stroke(pill, theme::R_S, Stroke::new(1.0_f32, theme::accent()));
    ui.painter().galley(pill.min + vec2(10.0, 3.0), galley, theme::accent());
}

// ---------------------------------------------------------------------------
// Порядок строк: чистые функции
// ---------------------------------------------------------------------------

/// Строки в порядке выполнения с линией «Default Time» между отрицательными и остальными.
pub(crate) fn display_rows(settings: &ProjectSettings) -> Vec<(String, i32)> {
    let mut rows: Vec<(String, i32)> = settings.script_order.iter().map(|(k, v)| (k.clone(), *v)).collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| natural_cmp(&a.0, &b.0)));
    let marker = rows.iter().position(|(_, order)| *order >= 0).unwrap_or(rows.len());
    rows.insert(marker, (MARKER.to_string(), 0));
    rows
}

/// Перенос строки `moved` в щель `gap` (0 — перед первой строкой списка `display`, где
/// есть и линия Default Time; `display.len()` — после последней). Число выбирается между
/// соседями; если места нет (разница меньше 2 или равные числа), строки этой стороны от
/// линии получают числа заново с шагом 10. Возвращает изменившиеся (тип, число).
pub(crate) fn drop_row(display: &[(String, i32)], moved: &str, gap: usize) -> Vec<(String, i32)> {
    let Some(from) = display.iter().position(|(name, _)| name == moved) else {
        return Vec::new();
    };
    if moved == MARKER {
        return Vec::new();
    }
    let mut list: Vec<(String, i32)> = display.to_vec();
    list.remove(from);
    let gap = if from < gap { gap - 1 } else { gap }.min(list.len());
    let marker = list.iter().position(|(name, _)| name == MARKER).unwrap_or(0);
    let before = gap <= marker;
    let prev = gap.checked_sub(1).map(|i| list[i].1);
    let next = list.get(gap).map(|(_, order)| *order);
    let direct = match (prev, next) {
        (Some(a), Some(b)) if b - a >= 2 => Some(a + (b - a) / 2),
        (Some(a), None) => Some(a + 100),
        (None, Some(b)) => Some(b - 100),
        _ => None,
    };
    // Строка не должна перескочить линию Default Time.
    let direct = direct.filter(|value| if before { *value < 0 } else { *value >= 0 });
    let mut changes = Vec::new();
    match direct {
        Some(value) => changes.push((moved.to_string(), value)),
        None => {
            // Нет места: перенумеровать сторону целиком, шаг 10 от линии.
            list.insert(gap, (moved.to_string(), 0));
            let marker = list.iter().position(|(name, _)| name == MARKER).unwrap_or(0);
            let range = if before { 0..marker } else { marker + 1..list.len() };
            let count = range.len() as i32;
            for (offset, index) in range.enumerate() {
                let value = if before { -10 * (count - offset as i32) } else { 10 * (offset as i32 + 1) };
                if list[index].1 != value || list[index].0 == moved {
                    changes.push((list[index].0.clone(), value));
                }
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(rows: &[(&str, i32)]) -> ProjectSettings {
        let mut settings = ProjectSettings::default();
        for (name, order) in rows {
            settings.set_order(name, *order);
        }
        settings
    }

    /// Применить изменения и вернуть имена по порядку (линия — «|»).
    fn after(rows: &[(&str, i32)], moved: &str, gap: usize) -> Vec<String> {
        let mut s = settings(rows);
        let display = display_rows(&s);
        for (name, order) in drop_row(&display, moved, gap) {
            s.set_order(&name, order);
        }
        display_rows(&s).into_iter().map(|(n, _)| if n == MARKER { "|".to_string() } else { n }).collect()
    }

    #[test]
    fn rows_are_sorted_with_the_default_line_after_the_negatives() {
        let display = display_rows(&settings(&[("C", 50), ("A", -100), ("B", -5), ("D", 0)]));
        let names: Vec<&str> = display.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["A", "B", MARKER, "D", "C"]);
        assert_eq!(display_rows(&ProjectSettings::default()), [(MARKER.to_string(), 0)]);
    }

    #[test]
    fn dropping_between_neighbours_takes_a_number_between_them() {
        // A(-100) B(-10) | C(100): тащим C между A и B → между -100 и -10 ... но C был после линии.
        let result = after(&[("A", -100), ("B", -10), ("C", 100)], "C", 1);
        assert_eq!(result, ["A", "C", "B", "|"]);
        // Число между: -100 и -10 → -55.
        let mut s = settings(&[("A", -100), ("B", -10), ("C", 100)]);
        let d = display_rows(&s);
        let changes = drop_row(&d, "C", 1);
        assert_eq!(changes, [("C".to_string(), -55)]);
        s.set_order("C", -55);
        assert_eq!(display_rows(&s)[1].0, "C");
    }

    #[test]
    fn dropping_at_the_ends_extends_by_a_hundred() {
        assert_eq!(drop_row(&display_rows(&settings(&[("A", -100), ("B", 100)])), "B", 0), [("B".to_string(), -200)]);
        assert_eq!(drop_row(&display_rows(&settings(&[("A", -100), ("B", 100)])), "A", 4), [("A".to_string(), 200)]);
        // Сразу над линией, выше только A(-100): между -100 и линией 0.
        assert_eq!(drop_row(&display_rows(&settings(&[("A", -100), ("B", 100)])), "B", 1), [("B".to_string(), -50)]);
    }

    #[test]
    fn a_row_never_jumps_over_the_default_line() {
        // Одна строка и линия: над линией — отрицательное, под — положительное.
        let rows = [("A", 40)];
        assert_eq!(drop_row(&display_rows(&settings(&rows)), "A", 0), [("A".to_string(), -100)]);
        assert_eq!(drop_row(&display_rows(&settings(&[("A", -40)])), "A", 2), [("A".to_string(), 100)]);
    }

    #[test]
    fn no_room_renumbers_that_side() {
        // A(-2) B(-1) | : между A и B нет целого числа — сторона получает числа заново.
        let result = after(&[("A", -2), ("B", -1), ("C", 5)], "C", 1);
        assert_eq!(result, ["A", "C", "B", "|"]);
        let mut s = settings(&[("A", -2), ("B", -1), ("C", 5)]);
        for (name, order) in drop_row(&display_rows(&s), "C", 1) {
            s.set_order(&name, order);
        }
        assert_eq!(s.order_of("A"), Some(-30));
        assert_eq!(s.order_of("C"), Some(-20));
        assert_eq!(s.order_of("B"), Some(-10));
        // Равные числа тоже «без места».
        let result = after(&[("A", 7), ("B", 7), ("C", 9)], "C", 2);
        assert_eq!(result, ["|", "A", "C", "B"]);
    }

    #[test]
    fn moving_within_the_same_spot_changes_nothing_important() {
        // Бросили на соседнюю щель — порядок тот же.
        let result = after(&[("A", -100), ("B", -10)], "A", 0);
        assert_eq!(result, ["A", "B", "|"]);
        assert!(drop_row(&display_rows(&settings(&[("A", 1)])), "Nope", 0).is_empty());
        assert!(drop_row(&display_rows(&settings(&[("A", 1)])), MARKER, 0).is_empty());
    }

    #[test]
    fn language_comes_from_the_registry_category() {
        assert_eq!(language_of(None), Language::Rust);
        assert_eq!(language_of(Some("Scripts")), Language::Rust);
        assert_eq!(language_of(Some("Scripts (C++)")), Language::Cpp);
        assert_eq!(language_of(Some("Scripts (Scenario)")), Language::Scenario);
        assert_eq!(language_of(Some("Physics 3D")), Language::Engine);
    }
}
