//! Окно Hierarchy: дерево объектов сцены.
//!
//! Строки сначала собираются в плоский список (кто, на какой глубине, раскрыт ли),
//! потом рисуются — так большие сцены прокручиваются без затрат на невидимые строки,
//! а контекстное меню может пользоваться всем редактором.

use std::collections::HashSet;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Shape, Stroke};
use engine_core::prelude::*;

use crate::catalog;
use crate::icons::{self, Icon};
use crate::state::{Action, Editor};
use crate::theme::{self, item};

const INDENT: f32 = theme::INDENT;

/// Что тащат мышью из иерархии.
pub(crate) struct DragObject(pub GameObjectId);

#[derive(Default)]
pub(crate) struct HierarchyState {
    pub filter: String,
    /// Свёрнутые узлы — по пути индексов, чтобы переживать Undo и Stop.
    collapsed: HashSet<Vec<usize>>,
    /// Объект, имя которого сейчас редактируется прямо в строке.
    pub rename: Option<Rename>,
}

pub(crate) struct Rename {
    pub id: GameObjectId,
    pub text: String,
    focused: bool,
}

impl HierarchyState {
    pub fn expand(&mut self, path: Option<Vec<usize>>) {
        if let Some(path) = path {
            self.collapsed.remove(&path);
        }
    }

    pub fn begin_rename(&mut self, id: GameObjectId, current: &str) {
        self.rename = Some(Rename {
            id,
            text: current.to_string(),
            focused: false,
        });
    }
}

/// Иконка и цвет строки по содержимому объекта.
pub(crate) fn object_icon(object: &GameObject) -> (Icon, Color32) {
    catalog::object_icon(object)
}

/// Строка списка — всё, что нужно для отрисовки, без ссылок на сцену.
struct Row {
    id: GameObjectId,
    depth: usize,
    name: String,
    icon: Icon,
    icon_color: Color32,
    has_children: bool,
    open: bool,
    /// Флажок самого объекта и активность с учётом предков.
    active_self: bool,
    active: bool,
    prefab: bool,
    parent: Option<GameObjectId>,
    sibling_index: usize,
    /// Последний среди «братьев».
    last: bool,
    path: Vec<usize>,
    /// Где рисовать вертикальные направляющие (по уровням предков).
    guides: Vec<bool>,
    /// В режиме поиска — путь родителей серым.
    context: Option<String>,
}

/// Куда упадёт перетаскиваемый объект относительно строки.
#[derive(Clone, Copy, PartialEq)]
enum Drop {
    Before,
    Into,
    After,
}

fn collect(editor: &Editor) -> Vec<Row> {
    let scene = &editor.app.world.scene;
    let state = &editor.hierarchy;
    let mut rows = Vec::new();
    let filter = state.filter.trim().to_lowercase();

    let row_of = |id: GameObjectId, depth: usize, path: Vec<usize>, guides: Vec<bool>, sibling_index: usize| {
        let object = scene.get(id)?;
        let (icon, icon_color) = object_icon(object);
        let has_children = !object.children().is_empty();
        Some(Row {
            id,
            depth,
            name: object.name.clone(),
            icon,
            icon_color,
            has_children,
            open: has_children && !state.collapsed.contains(&path),
            active_self: object.active,
            active: scene.is_active(id),
            prefab: object.prefab().is_some(),
            parent: object.parent(),
            sibling_index,
            last: true,
            path,
            guides,
            context: None,
        })
    };

    if !filter.is_empty() {
        // Поиск — плоский список совпадений с путём родителей.
        for id in scene.ids() {
            let Some(object) = scene.get(id) else { continue };
            if !object.name.to_lowercase().contains(&filter) {
                continue;
            }
            let path = crate::state::path_of(scene, id).unwrap_or_default();
            let index = path.last().copied().unwrap_or(0);
            if let Some(mut row) = row_of(id, 0, path, Vec::new(), index) {
                row.has_children = false;
                row.open = false;
                let mut parents = Vec::new();
                let mut cursor = object.parent();
                while let Some(parent) = cursor {
                    let Some(p) = scene.get(parent) else { break };
                    parents.push(p.name.clone());
                    cursor = p.parent();
                }
                if !parents.is_empty() {
                    parents.reverse();
                    row.context = Some(parents.join(" / "));
                }
                rows.push(row);
            }
        }
        return rows;
    }

    fn walk(
        rows: &mut Vec<Row>,
        scene: &Scene,
        ids: &[GameObjectId],
        depth: usize,
        path: &mut Vec<usize>,
        guides: &mut Vec<bool>,
        row_of: &dyn Fn(GameObjectId, usize, Vec<usize>, Vec<bool>, usize) -> Option<Row>,
    ) {
        for (index, id) in ids.iter().enumerate() {
            path.push(index);
            let last = index + 1 == ids.len();
            if let Some(mut row) = row_of(*id, depth, path.clone(), guides.clone(), index) {
                row.last = last;
                let open = row.open;
                rows.push(row);
                if open {
                    let children = scene.get(*id).map(|o| o.children().to_vec()).unwrap_or_default();
                    guides.push(!last);
                    walk(rows, scene, &children, depth + 1, path, guides, row_of);
                    guides.pop();
                }
            }
            path.pop();
        }
    }
    walk(&mut rows, scene, scene.roots(), 0, &mut Vec::new(), &mut Vec::new(), &row_of);
    rows
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    // Одна полоса, как в Unity: «+ Create» и поиск (имя окна — на его вкладке в доке).
    theme::toolbar_strip(ui, |ui| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            "Create",
            16.0,
            egui::TextFormat {
                font_id: theme::body(),
                color: theme::TEXT,
                ..Default::default()
            },
        );
        let response = egui::menu::menu_custom_button(
            ui,
            egui::Button::new(job).min_size(vec2(0.0, theme::CONTROL_H)),
            |ui| {
                theme::menu_body(ui);
                editor.gameobject_menu(ui, None);
            },
        )
        .response;
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(response.rect.left() + 14.0, response.rect.center().y), vec2(10.0, 10.0)),
            Icon::Plus,
            theme::TEXT_MUTED,
        );
        response.on_hover_text("Create a new object in the scene");
        let width = ui.available_width();
        theme::search_field(ui, &mut editor.hierarchy.filter, "Search objects", width);
    });

    let rows = collect(editor);
    let dragging = egui::DragAndDrop::payload::<DragObject>(ui.ctx()).map(|p| p.0);
    let searching = !editor.hierarchy.filter.trim().is_empty();

    egui::ScrollArea::vertical()
        .id_salt("hierarchy_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(theme::SP_XS);
            scene_row(editor, ui, dragging);
            for row in &rows {
                show_row(editor, ui, row, dragging, searching);
            }
            if rows.is_empty() && searching {
                ui.add_space(theme::SP_L);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No objects match the search").color(theme::TEXT_MUTED));
                });
            }

            // Пустое место под списком: снять выделение, бросить объект «в корень», меню создания.
            let rest = ui.available_rect_before_wrap();
            let rest = Rect::from_min_size(rest.min, vec2(rest.width(), rest.height().max(48.0)));
            let response = ui.allocate_rect(rest, Sense::click());
            if response.clicked() {
                editor.actions.push(Action::Select(None));
            }
            if response.dnd_hover_payload::<DragObject>().is_some() {
                ui.painter().hline(
                    rest.x_range().shrink(theme::SP_S),
                    rest.top() + 1.0,
                    Stroke::new(2.0_f32, theme::accent()),
                );
            }
            if let Some(payload) = response.dnd_release_payload::<DragObject>() {
                editor.actions.push(Action::Reparent {
                    child: payload.0,
                    parent: None,
                });
            }
            response.context_menu(|ui| {
                theme::menu_body(ui);
                editor.gameobject_menu(ui, None);
            });
        });

    // Подпись у курсора, пока объект тащат.
    if let Some(id) = dragging {
        if let (Some(position), Some(object)) = (ui.ctx().pointer_latest_pos(), editor.app.world.scene.get(id)) {
            drag_label(ui.ctx(), position, object_icon(object).0, &object.name);
        }
    }
}

/// Строка сцены над деревом: имя сцены; бросок на неё — объект в корень.
fn scene_row(editor: &mut Editor, ui: &mut egui::Ui, dragging: Option<GameObjectId>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
    let response = ui.interact(rect, egui::Id::new("hierarchy_scene_row"), Sense::click());
    if dragging.is_some() && response.contains_pointer() {
        ui.painter().rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.5_f32, theme::accent()));
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 15.0, rect.center().y), vec2(theme::ICON_S + 1.0, theme::ICON_S + 1.0)),
        Icon::Scene,
        theme::TEXT_MUTED,
    );
    let name = format!("{}{}", editor.app.world.scene.name, if editor.dirty { "*" } else { "" });
    ui.painter().text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::strong(theme::FONT_BODY),
        theme::TEXT,
    );
    if let Some(payload) = response.dnd_release_payload::<DragObject>() {
        editor.actions.push(Action::Reparent {
            child: payload.0,
            parent: None,
        });
    }
    if response.clicked() {
        editor.actions.push(Action::Select(None));
    }
}

/// Подпись, которая следует за курсором во время перетаскивания.
pub(crate) fn drag_label(ctx: &egui::Context, position: egui::Pos2, icon: Icon, text: &str) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drag_label")));
    let galley = painter.layout_no_wrap(text.to_string(), theme::body(), Color32::WHITE);
    let rect = Rect::from_min_size(position + vec2(16.0, 12.0), galley.size() + vec2(32.0, 6.0));
    painter.add(egui::epaint::Shadow {
        offset: vec2(0.0, 3.0),
        blur: 10.0,
        spread: 0.0,
        color: Color32::from_black_alpha(110),
    }.as_shape(rect, theme::R_S));
    painter.rect(rect, theme::R_S, theme::BG_RAISED, Stroke::new(1.0_f32, theme::LINE_STRONG));
    icons::paint(
        &painter,
        Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
        icon,
        theme::TEXT_MUTED,
    );
    painter.galley(pos2(rect.left() + 24.0, rect.center().y - galley.size().y * 0.5), galley, theme::TEXT);
}

fn show_row(editor: &mut Editor, ui: &mut egui::Ui, row: &Row, dragging: Option<GameObjectId>, searching: bool) {
    let id = row.id;
    let selected = editor.selection == Some(id);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
    let response = ui.interact(rect, Editor::hierarchy_row_id(id), Sense::click_and_drag());
    let painter = ui.painter().clone();

    // Фон: выделение, наведение.
    if selected {
        painter.rect_filled(rect, 0.0, theme::selection_fill());
    } else if response.hovered() && dragging.is_none() {
        painter.rect_filled(rect, 0.0, theme::hover_fill());
    }

    // Направляющие уровней.
    let base = rect.left() + theme::SP_S + 2.0;
    let guide = Stroke::new(1.0_f32, theme::LINE_SOFT);
    for level in 0..row.depth {
        let x = base + level as f32 * INDENT + INDENT * 0.5;
        if level + 1 == row.depth {
            let bottom = if row.last { rect.center().y } else { rect.bottom() };
            painter.vline(x, rect.top()..=bottom, guide);
        } else if row.guides.get(level + 1).copied().unwrap_or(false) {
            painter.vline(x, rect.top()..=rect.bottom(), guide);
        }
    }
    if row.depth > 0 {
        let x = base + (row.depth - 1) as f32 * INDENT + INDENT * 0.5;
        painter.hline(x..=x + INDENT * 0.5 - 1.0, rect.center().y, guide);
    }

    // Стрелка раскрытия.
    let left = base + row.depth as f32 * INDENT;
    let arrow_rect = Rect::from_min_size(pos2(left, rect.top()), vec2(INDENT, theme::ROW));
    if row.has_children {
        let arrow = ui.interact(arrow_rect, Editor::hierarchy_row_id(id).with("arrow"), Sense::click());
        let color = if arrow.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
        icons::paint(
            &painter,
            Rect::from_center_size(arrow_rect.center(), vec2(9.0, 9.0)),
            if row.open { Icon::ChevronDown } else { Icon::ChevronRight },
            color,
        );
        if arrow.clicked() {
            let state = &mut editor.hierarchy;
            if row.open {
                state.collapsed.insert(row.path.clone());
            } else {
                state.collapsed.remove(&row.path);
            }
        }
    }

    // Иконка и имя.
    let dim = |color: Color32| if row.active { color } else { color.gamma_multiply(0.45) };
    let icon_color = if row.prefab {
        theme::PREFAB
    } else if selected {
        theme::TEXT_STRONG
    } else {
        theme::mix(row.icon_color, theme::BG_PANEL, 0.2)
    };
    let icon_rect = Rect::from_center_size(pos2(arrow_rect.right() + 7.0, rect.center().y), vec2(13.0, 13.0));
    icons::paint(&painter, icon_rect, row.icon, dim(icon_color));

    let eye_rect = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(20.0, 20.0));
    let text_rect = Rect::from_min_max(pos2(icon_rect.right() + 5.0, rect.top()), pos2(eye_rect.left() - 2.0, rect.bottom()));
    let renaming = editor.hierarchy.rename.as_ref().is_some_and(|r| r.id == id);
    if renaming {
        let mut finished = None;
        if let Some(rename) = editor.hierarchy.rename.as_mut() {
            let edit = ui.put(
                text_rect.shrink2(vec2(0.0, 2.0)),
                egui::TextEdit::singleline(&mut rename.text).vertical_align(egui::Align::Center),
            );
            if !rename.focused {
                rename.focused = true;
                edit.request_focus();
            } else if edit.lost_focus() {
                let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                finished = Some(cancelled);
            }
        }
        if let Some(cancelled) = finished {
            if let Some(rename) = editor.hierarchy.rename.take() {
                if !cancelled {
                    editor.actions.push(Action::Rename { id, name: rename.text });
                }
            }
        }
    } else {
        let color = if row.prefab {
            theme::PREFAB
        } else if selected {
            theme::TEXT_STRONG
        } else {
            theme::TEXT
        };
        let clipped = painter.with_clip_rect(text_rect);
        let name = painter.layout_no_wrap(row.name.clone(), theme::body(), dim(color));
        let name_width = name.size().x;
        clipped.galley(pos2(text_rect.left(), rect.center().y - name.size().y * 0.5), name, dim(color));
        if let Some(context) = &row.context {
            clipped.text(
                pos2(text_rect.left() + name_width + theme::SP_M, rect.center().y),
                Align2::LEFT_CENTER,
                context,
                theme::caption(),
                theme::TEXT_MUTED,
            );
        }
    }

    // Глаз: включён ли объект. Виден при наведении и у выключенных.
    let eye = ui.interact(eye_rect, Editor::hierarchy_row_id(id).with("active"), Sense::click());
    if response.hovered() || eye.hovered() || !row.active_self {
        if eye.hovered() {
            painter.rect_filled(eye_rect, theme::R_S, theme::BG_HOVER);
        }
        let color = if eye.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
        icons::paint(
            &painter,
            eye_rect.shrink(4.0),
            if row.active_self { Icon::Eye } else { Icon::EyeOff },
            color,
        );
    }
    let eye_tip = if row.active_self { "Deactivate the object" } else { "Activate the object" };
    if eye.on_hover_text(eye_tip).clicked() {
        editor.actions.push(Action::SetActive { id, active: !row.active_self });
    }

    // Перетаскивание: куда упадёт объект.
    let drop = dragging.filter(|dragged| *dragged != id && !searching).and_then(|_| {
        let pointer = ui.ctx().pointer_latest_pos()?;
        if !rect.contains(pointer) {
            return None;
        }
        let t = (pointer.y - rect.top()) / rect.height();
        Some(if t < 0.25 {
            Drop::Before
        } else if t > 0.75 && !row.open {
            Drop::After
        } else {
            Drop::Into
        })
    });
    if let Some(drop) = drop {
        let accent = theme::accent();
        let line_left = if drop == Drop::Into { left } else { left + INDENT * 0.5 };
        match drop {
            Drop::Into => {
                painter.rect_filled(rect.shrink(1.0), theme::R_S, theme::mix(theme::BG_PANEL, accent, 0.2));
                painter.rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.5_f32, accent));
            }
            Drop::Before | Drop::After => {
                let y = if drop == Drop::Before { rect.top() + 1.0 } else { rect.bottom() - 1.0 };
                painter.hline(line_left..=rect.right() - theme::SP_M, y, Stroke::new(2.0_f32, accent));
                painter.add(Shape::circle_stroke(pos2(line_left, y), 3.0, Stroke::new(2.0_f32, accent)));
            }
        }
    }

    // Взаимодействие.
    if response.clicked() || response.secondary_clicked() {
        editor.actions.push(Action::Select(Some(id)));
    }
    if response.double_clicked() {
        editor.hierarchy.begin_rename(id, &row.name);
    }
    if response.drag_started() {
        response.dnd_set_drag_payload(DragObject(id));
    }
    // Материал из окна Project, брошенный на строку, — первому слоту MeshRenderer объекта.
    // Сначала смотрим тип груза: `dnd_release_payload` забирает груз любого типа, и
    // перетаскивание строк иерархии (DragObject) иначе потерялось бы.
    let dragging_material = response
        .dnd_hover_payload::<crate::project::DragAsset>()
        .is_some_and(|asset| asset.importer == "Material");
    if dragging_material {
        if let Some(asset) = response.dnd_release_payload::<crate::project::DragAsset>() {
            editor.actions.push(Action::AssignMaterial { object: id, material: asset.guid, slot: 0 });
        }
    }
    if let Some(payload) = response.dnd_release_payload::<DragObject>() {
        let child = payload.0;
        if child != id {
            match drop.unwrap_or(Drop::Into) {
                Drop::Into => {
                    editor.actions.push(Action::Reparent { child, parent: Some(id) });
                    editor.hierarchy.expand(Some(row.path.clone()));
                }
                place => {
                    let mut index = row.sibling_index + usize::from(place == Drop::After);
                    // Объект уходит со своего места среди тех же «братьев» — индекс сдвигается.
                    let scene = &editor.app.world.scene;
                    let child_parent = scene.get(child).and_then(|o| o.parent());
                    if child_parent == row.parent {
                        let siblings: Vec<GameObjectId> = match row.parent {
                            Some(parent) => scene.get(parent).map(|p| p.children().to_vec()).unwrap_or_default(),
                            None => scene.roots().to_vec(),
                        };
                        if siblings.iter().position(|s| *s == child).is_some_and(|current| current < index) {
                            index -= 1;
                        }
                    }
                    editor.actions.push(Action::ReparentAt { child, parent: row.parent, index });
                }
            }
        }
    }
    response.context_menu(|ui| {
        theme::menu_body(ui);
        context_menu(editor, ui, row);
    });
}

fn context_menu(editor: &mut Editor, ui: &mut egui::Ui, row: &Row) {
    let id = row.id;
    theme::submenu(ui, Icon::Plus, "Create Child", |ui| editor.gameobject_menu(ui, Some(id)));
    theme::menu_separator(ui);
    if item("Rename").icon(Icon::Text).shortcut(&editor.shortcut_text("rename")).show(ui) {
        editor.hierarchy.begin_rename(id, &row.name);
    }
    if item("Duplicate").icon(Icon::Copy).shortcut(&editor.shortcut_text("duplicate")).show(ui) {
        editor.actions.push(Action::Duplicate(id));
    }
    if item("Delete").icon(Icon::Trash).shortcut(&editor.shortcut_text("delete")).show(ui) {
        editor.actions.push(Action::Delete(id));
    }
    theme::menu_separator(ui);
    let (label, icon) = if row.active_self { ("Deactivate", Icon::EyeOff) } else { ("Activate", Icon::Eye) };
    if item(label).icon(icon).show(ui) {
        editor.actions.push(Action::SetActive { id, active: !row.active_self });
    }
    if item("Frame in Scene View").icon(Icon::Search).shortcut(&editor.shortcut_text("frame-selected")).show(ui) {
        editor.actions.push(Action::Select(Some(id)));
        editor.actions.push(Action::FrameSelected);
    }
    theme::menu_separator(ui);
    if item("Move Up").enabled_if(row.sibling_index > 0, "Already the first").show(ui) {
        editor.actions.push(Action::MoveSibling { id, delta: -1 });
    }
    if item("Move Down").show(ui) {
        editor.actions.push(Action::MoveSibling { id, delta: 1 });
    }
    if item("Move to Root").enabled_if(row.parent.is_some(), "Already at the root").show(ui) {
        editor.actions.push(Action::Reparent { child: id, parent: None });
    }
    let children = editor.app.world.scene.get(id).map_or(0, |o| o.children().len());
    if item("Sort Children A-Z")
        .enabled_if(children > 1, "Needs at least two children")
        .show(ui)
    {
        editor.actions.push(Action::SortChildren { parent: Some(id) });
    }
    theme::menu_separator(ui);
    if row.prefab {
        if item("Unpack Prefab").icon(Icon::Prefab).show(ui) {
            editor.actions.push(Action::UnpackPrefab(id));
        }
    } else if item("Save as Prefab")
        .tinted(Icon::Prefab, theme::PREFAB)
        .enabled_if(editor.assets_root().is_some(), "No assets directory is mounted")
        .show(ui)
    {
        editor.actions.push(Action::SavePrefab(id));
    }
}
