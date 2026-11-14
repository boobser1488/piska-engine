//! Поле-ссылка на объект сцены в инспекторе (`ObjectRef`, `ComponentRef<T>`), как
//! поле `GameObject` в Unity: имя цели с иконкой или «None (GameObject)», перетаскивание
//! строки из Hierarchy, список объектов сцены с поиском (по кнопке-«мишени»), щелчок —
//! подсветить цель в Hierarchy, двойной щелчок — выделить её, Delete / Backspace —
//! очистить. Правка идёт через обычный путь инспектора (`set_field` + `touch`), поэтому
//! работают Undo / Redo и мультивыделение («—» для разных значений).
//!
//! Инспектор держит объект открытым на запись, пока рисует поля, поэтому сведения о
//! других объектах (имена, иконки, компоненты) снимаются заранее — [`publish`] — и
//! лежат в памяти egui на этот кадр. Просьбы виджета к редактору (подсветить, выделить)
//! возвращаются тем же путём и выполняются в [`finish`].

use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine_core::objref::object_requirement;
use engine_core::prelude::*;

use crate::catalog;
use crate::hierarchy::DragObject;
use crate::i18n::{tr_s, trf};
use crate::icons::{self, Icon};
use crate::state::{path_of, Action, Editor};
use crate::theme;

/// Сколько секунд строка Hierarchy подсвечивается после щелчка по полю-ссылке.
const PING_SECONDS: f64 = 1.6;

/// Объект сцены, как его видит поле-ссылка.
#[derive(Clone)]
pub(crate) struct ObjectEntry {
    pub uid: u64,
    pub id: GameObjectId,
    pub name: String,
    pub parent: Option<GameObjectId>,
    pub icon: Icon,
    pub color: Color32,
    pub prefab: bool,
    pub components: Vec<&'static str>,
}

/// Все объекты сцены на этот кадр (в порядке иерархии).
#[derive(Clone, Default)]
pub(crate) struct ObjectDirectory(pub Arc<Vec<ObjectEntry>>);

impl ObjectDirectory {
    fn by_uid(&self, uid: u64) -> Option<&ObjectEntry> {
        if uid == 0 {
            return None;
        }
        self.0.iter().find(|e| e.uid == uid)
    }

    fn by_id(&self, id: GameObjectId) -> Option<&ObjectEntry> {
        self.0.iter().find(|e| e.id == id)
    }

    /// Путь родителей через «/» — подпись в списке выбора.
    fn parents_of(&self, entry: &ObjectEntry) -> String {
        let mut parts = Vec::new();
        let mut cursor = entry.parent;
        while let Some(parent) = cursor.and_then(|p| self.by_id(p)) {
            parts.push(parent.name.as_str());
            cursor = parent.parent;
            if parts.len() > 32 {
                break;
            }
        }
        parts.reverse();
        parts.join("/")
    }
}

/// Что поле-ссылка просит у редактора после кадра.
#[derive(Clone, Default)]
struct Requests {
    ping: Option<GameObjectId>,
    select: Option<GameObjectId>,
}

fn directory_id() -> egui::Id {
    egui::Id::new("inspector_refs_directory")
}

fn requests_id() -> egui::Id {
    egui::Id::new("inspector_refs_requests")
}

fn ping_id() -> egui::Id {
    egui::Id::new("inspector_refs_ping")
}

/// Есть ли у объектов `ids` поля-ссылки (тогда инспектору нужен справочник объектов).
fn needs_directory(scene: &Scene, ids: &[GameObjectId]) -> bool {
    ids.iter().filter_map(|id| scene.get(*id)).any(|object| {
        object.components().any(|component| {
            component.field_names().iter().any(|field| match component.get_field(field) {
                Some(Value::Object(_)) => true,
                Some(Value::List(items)) => items.iter().any(|v| matches!(v, Value::Object(_))),
                _ => false,
            })
        })
    })
}

/// Снять справочник объектов сцены для полей-ссылок выделенных объектов `ids`.
/// Если полей-ссылок у них нет, ничего не делает (большие сцены не обходятся зря).
pub(crate) fn publish(ctx: &egui::Context, scene: &Scene, ids: &[GameObjectId]) {
    if !needs_directory(scene, ids) {
        ctx.data_mut(|d| d.remove::<ObjectDirectory>(directory_id()));
        return;
    }
    let entries: Vec<ObjectEntry> = scene
        .ids()
        .into_iter()
        .filter_map(|id| {
            let object = scene.get(id)?;
            let (icon, color) = catalog::object_icon(object);
            Some(ObjectEntry {
                uid: object.uid(),
                id,
                name: object.name.clone(),
                parent: object.parent(),
                icon,
                color,
                prefab: object.prefab().is_some(),
                components: object.components().map(|c| c.type_name()).collect(),
            })
        })
        .collect();
    ctx.data_mut(|d| d.insert_temp(directory_id(), ObjectDirectory(Arc::new(entries))));
}

fn directory(ctx: &egui::Context) -> ObjectDirectory {
    ctx.data(|d| d.get_temp::<ObjectDirectory>(directory_id())).unwrap_or_default()
}

fn request(ctx: &egui::Context, f: impl FnOnce(&mut Requests)) {
    ctx.data_mut(|d| {
        let mut requests = d.get_temp::<Requests>(requests_id()).unwrap_or_default();
        f(&mut requests);
        d.insert_temp(requests_id(), requests);
    });
}

/// После инспектора: подсветить / выделить цель, о которой просило поле-ссылка.
pub(crate) fn finish(editor: &mut Editor, ctx: &egui::Context) {
    let Some(requests) = ctx.data_mut(|d| d.remove_temp::<Requests>(requests_id())) else {
        return;
    };
    if let Some(target) = requests.ping.filter(|id| editor.app.world.scene.contains(*id)) {
        // Раскрыть предков, чтобы строка была видна.
        let mut cursor = editor.app.world.scene.parent_of(target);
        while let Some(parent) = cursor {
            let path = path_of(&editor.app.world.scene, parent);
            editor.hierarchy.expand(path);
            cursor = editor.app.world.scene.parent_of(parent);
        }
        let now = ctx.input(|i| i.time);
        ctx.data_mut(|d| d.insert_temp(ping_id(), (target, now)));
    }
    if let Some(target) = requests.select.filter(|id| editor.app.world.scene.contains(*id)) {
        editor.actions.push(Action::Select(Some(target)));
    }
}

/// Объект, который сейчас «пингует» поле-ссылка (подсветка строки в Hierarchy).
pub(crate) fn pinged(ctx: &egui::Context) -> Option<GameObjectId> {
    let (id, at) = ctx.data(|d| d.get_temp::<(GameObjectId, f64)>(ping_id()))?;
    (ctx.input(|i| i.time) - at < PING_SECONDS).then_some(id)
}

/// Подсветка строки Hierarchy, если её объект пингуют (рамка цвета акцента, гаснет).
pub(crate) fn paint_ping(ui: &egui::Ui, rect: Rect, id: GameObjectId) {
    let Some((target, at)) = ui.ctx().data(|d| d.get_temp::<(GameObjectId, f64)>(ping_id())) else {
        return;
    };
    if target != id {
        return;
    }
    let age = ui.input(|i| i.time) - at;
    if age >= PING_SECONDS {
        return;
    }
    if age < 0.05 {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
    let fade = (1.0 - age / PING_SECONDS).clamp(0.0, 1.0) as f32;
    let color = theme::accent().gamma_multiply(fade);
    ui.painter().rect_filled(rect.shrink(1.0), theme::R_S, theme::accent_soft().gamma_multiply(fade * 0.6));
    ui.painter().rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.5_f32, color));
    ui.ctx().request_repaint();
}

/// Подходит ли объект полю (для `ComponentRef<T>` — есть ли на нём `T`).
fn fits(entry: &ObjectEntry, requirement: Option<&str>) -> bool {
    requirement.is_none_or(|kind| entry.components.contains(&kind))
}

/// Подпись «None (GameObject)» / «None (Rigidbody)».
fn kind_label(requirement: Option<&str>) -> String {
    match requirement {
        Some(kind) => theme::pretty_type(kind),
        None => tr_s("GameObject").to_string(),
    }
}

/// Поле-ссылка. `uid` — номер объекта в поле (0 — не назначено); `mixed` — у выделенных
/// объектов разные значения (поле показывает «—», любой выбор применяется ко всем).
/// Возвращает `true`, если значение изменили.
pub(crate) fn object_field(
    ui: &mut egui::Ui,
    uid: &mut u64,
    meta: FieldMeta,
    salt: impl std::hash::Hash,
    mixed: bool,
) -> bool {
    let directory = directory(ui.ctx());
    let requirement = object_requirement(&meta);
    let current = directory.by_uid(*uid);
    let kind = kind_label(requirement);
    let (label, label_color) = match current {
        _ if mixed => ("—".to_string(), theme::TEXT),
        Some(entry) => (entry.name.clone(), theme::TEXT),
        None if *uid == 0 => (trf!("None ({kind})", kind = kind), theme::TEXT_MUTED),
        None => (trf!("Missing ({kind})", kind = kind), theme::ERROR),
    };
    let mut changed = false;

    let id = egui::Id::new(("object_field", salt));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::FIELD_H), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    let picker = Rect::from_min_max(pos2(rect.right() - 18.0, rect.top()), rect.max);
    let picker_response = ui.interact(picker, id.with("picker"), Sense::click());
    let popup = id.with("popup");
    let open = ui.memory(|m| m.is_popup_open(popup));
    let focused = response.has_focus();
    let border = if open || focused {
        theme::accent()
    } else if response.hovered() || picker_response.hovered() {
        theme::LINE_HOVER
    } else {
        theme::LINE_STRONG
    };
    crate::controls::paint_frame(ui.painter(), rect, theme::R_S, theme::BG_INSET, border);
    let (icon, icon_color) = match current {
        Some(entry) if !mixed => (entry.icon, if entry.prefab { theme::PREFAB } else { entry.color }),
        _ => (Icon::Object, theme::TEXT_DISABLED),
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 11.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
        icon,
        icon_color,
    );
    let text_rect = Rect::from_min_max(pos2(rect.left() + 21.0, rect.top()), pos2(picker.left() - 2.0, rect.bottom()));
    ui.painter()
        .with_clip_rect(text_rect)
        .text(text_rect.left_center(), Align2::LEFT_CENTER, &label, theme::body(), label_color);
    ui.painter().vline(picker.left(), picker.y_range().shrink(3.0), Stroke::new(1.0_f32, theme::LINE_SOFT));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(picker.center(), vec2(10.0, 10.0)),
        Icon::Target,
        if picker_response.hovered() { theme::TEXT } else { theme::TEXT_MUTED },
    );

    // Щелчок по имени — подсветить цель в Hierarchy, двойной — выделить её.
    if response.clicked() {
        response.request_focus();
        if let Some(entry) = current.filter(|_| !mixed) {
            let target = entry.id;
            request(ui.ctx(), |r| r.ping = Some(target));
        }
    }
    if response.double_clicked() {
        if let Some(entry) = current.filter(|_| !mixed) {
            let target = entry.id;
            request(ui.ctx(), |r| r.select = Some(target));
        }
    }
    let tip = match current {
        Some(entry) if !mixed => {
            let parents = directory.parents_of(entry);
            let place = if parents.is_empty() { entry.name.clone() } else { format!("{parents}/{}", entry.name) };
            trf!("{place}\nClick: show in Hierarchy · Double-click: select · Delete: clear", place = place)
        }
        _ => tr_s("Drag an object here from the Hierarchy, or pick one with the target button").to_string(),
    };
    let response = response.on_hover_text(tip);
    // Поле в фокусе (после щелчка) забирает клавиатуру: Delete очищает поле, а не удаляет
    // выделенный объект (горячие клавиши молчат, пока у виджета фокус).
    if response.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        && (*uid != 0 || mixed)
    {
        *uid = 0;
        changed = true;
    }
    if picker_response.on_hover_text(tr_s("Pick an object from the scene")).clicked() {
        ui.data_mut(|d| d.insert_temp(popup.with("filter"), String::new()));
        ui.memory_mut(|m| m.toggle_popup(popup));
    }

    // Перетаскивание строки из Hierarchy.
    if let Some(dragged) = response.dnd_hover_payload::<DragObject>() {
        let ok = directory.by_id(dragged.0).is_some_and(|e| fits(e, requirement));
        let color = if ok { theme::OK } else { theme::ERROR };
        ui.painter().rect_stroke(rect, theme::R_S, Stroke::new(1.5_f32, color));
    }
    if let Some(dropped) = response.dnd_release_payload::<DragObject>() {
        match directory.by_id(dropped.0) {
            Some(entry) if fits(entry, requirement) => {
                if *uid != entry.uid || mixed {
                    *uid = entry.uid;
                    changed = true;
                }
            }
            Some(entry) => log::warn!(
                "`{}` has no {} component, so it cannot be assigned to this field",
                entry.name,
                requirement.unwrap_or("required")
            ),
            None => {}
        }
    }

    // Список объектов сцены с поиском.
    egui::popup_below_widget(ui, popup, &response, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(rect.width().max(280.0));
        let filter_id = popup.with("filter");
        let mut filter = ui.data(|d| d.get_temp::<String>(filter_id)).unwrap_or_default();
        theme::search_field(ui, &mut filter, &trf!("Search {kind} objects", kind = kind), f32::INFINITY).request_focus();
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        ui.add_space(theme::SP_S);
        let needle = filter.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("object_list", popup))
            .max_height(320.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if picker_row(ui, Icon::Close, theme::TEXT_MUTED, tr_s("None"), "", *uid == 0 && !mixed, popup.with("none")) {
                    *uid = 0;
                    changed = true;
                    ui.memory_mut(|m| m.close_popup());
                }
                let mut shown = 0;
                for entry in directory
                    .0
                    .iter()
                    .filter(|e| fits(e, requirement))
                    .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
                {
                    shown += 1;
                    let color = if entry.prefab { theme::PREFAB } else { entry.color };
                    let selected = !mixed && entry.uid == *uid;
                    let parents = directory.parents_of(entry);
                    if picker_row(ui, entry.icon, color, &entry.name, &parents, selected, popup.with(entry.uid)) {
                        *uid = entry.uid;
                        changed = true;
                        ui.memory_mut(|m| m.close_popup());
                    }
                }
                if shown == 0 {
                    ui.add_space(theme::SP_M);
                    ui.label(egui::RichText::new(trf!("No {kind} objects match", kind = kind)).color(theme::TEXT_MUTED));
                }
            });
    });
    changed
}

/// Строка списка выбора: иконка, имя, путь родителей серым.
fn picker_row(ui: &mut egui::Ui, icon: Icon, color: Color32, name: &str, folder: &str, selected: bool, id: egui::Id) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::ROW), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    let hovered = response.hovered();
    if selected {
        ui.painter().rect_filled(rect, theme::R_S, theme::selection_fill());
    } else if hovered {
        ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(theme::ICON_S + 1.0, theme::ICON_S + 1.0)),
        icon,
        color,
    );
    let name_galley = ui.painter().layout_no_wrap(name.to_string(), theme::body(), Color32::PLACEHOLDER);
    let name_width = name_galley.size().x;
    ui.painter().galley(
        pos2(rect.left() + 26.0, rect.center().y - name_galley.size().y * 0.5),
        name_galley,
        if hovered || selected { theme::TEXT_STRONG } else { theme::TEXT },
    );
    if !folder.is_empty() {
        ui.painter().with_clip_rect(rect).text(
            pos2(rect.left() + 26.0 + name_width + theme::SP_M, rect.center().y),
            Align2::LEFT_CENTER,
            folder,
            theme::caption(),
            theme::TEXT_DISABLED,
        );
    }
    response.clicked()
}

impl Editor {
    /// Идентификатор кнопки-«мишени» поля-ссылки `field` компонента `kind` выделенного
    /// объекта (для тестов): по ней открывается список объектов сцены.
    pub fn object_field_picker_id(kind: &'static str, index: usize, field: &'static str) -> egui::Id {
        egui::Id::new(("object_field", (kind, index, field))).with("picker")
    }

    /// Идентификатор строки списка выбора объекта (`uid` — номер объекта, 0 — «None»).
    pub fn object_field_option_id(kind: &'static str, index: usize, field: &'static str, uid: u64) -> egui::Id {
        let popup = egui::Id::new(("object_field", (kind, index, field))).with("popup");
        if uid == 0 { popup.with("none") } else { popup.with(uid) }
    }

    /// Объект, который сейчас подсвечен в Hierarchy после щелчка по полю-ссылке.
    pub fn pinged_object(&self, ctx: &egui::Context) -> Option<GameObjectId> {
        pinged(ctx)
    }
}
