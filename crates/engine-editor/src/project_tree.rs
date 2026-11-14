//! Левая панель окна Project: «Favorites» (сохранённые поиски и закреплённые каталоги),
//! дерево каталогов и «Code». Строки каталогов принимают перетаскиваемые элементы.

use crate::i18n::{tr_s, trf};
use std::collections::HashSet;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::icons::{self, Icon};
use crate::project::{tree_row_id, DragAsset, Place};
use crate::project_content::elide_middle;
use crate::project_fs::{file_name, parent_of};
use crate::project_menus::{self, MenuCx};
use crate::project_nav::{CODE_LOC, DEFAULT_SEARCHES};
use crate::project_ops::Cmd;
use crate::state::Editor;
use crate::theme;

/// Ключи служебных строк в наборе свёрнутых.
const FAVORITES_KEY: &str = "\u{3}favorites";

enum Row {
    Header { key: &'static str, title: &'static str, open: bool },
    Search { query: String, name: String, user: bool },
    Pinned(String),
    Folder { path: String, depth: usize, has_children: bool, open: bool },
    Code,
}

fn rows(editor: &Editor) -> Vec<Row> {
    let mut rows = Vec::new();
    let collapsed = &editor.project.collapsed;
    let favorites_open = !collapsed.contains(FAVORITES_KEY);
    rows.push(Row::Header { key: FAVORITES_KEY, title: tr_s("Favorites"), open: favorites_open });
    if favorites_open {
        for (name, query) in DEFAULT_SEARCHES {
            rows.push(Row::Search { query: query.to_string(), name: tr_s(name).to_string(), user: false });
        }
        for (name, query) in &editor.prefs.project_ui.searches {
            rows.push(Row::Search { query: query.clone(), name: name.clone(), user: true });
        }
        for folder in &editor.prefs.project_ui.favorites {
            rows.push(Row::Pinned(folder.clone()));
        }
    }
    let assets_open = !collapsed.contains("");
    rows.push(Row::Folder { path: String::new(), depth: 0, has_children: !editor.folders.is_empty(), open: assets_open });
    if assets_open {
        let parents: HashSet<&str> = editor.folders.iter().map(|f| parent_of(f)).collect();
        for folder in &editor.folders {
            // Виден, только если раскрыты все предки.
            let mut hidden = false;
            let mut path = String::new();
            let parts: Vec<&str> = folder.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(part);
                hidden |= collapsed.contains(&path);
            }
            if hidden {
                continue;
            }
            rows.push(Row::Folder {
                path: folder.clone(),
                depth: parts.len(),
                has_children: parents.contains(folder.as_str()),
                open: !collapsed.contains(folder),
            });
        }
    }
    let has_code = !editor.code.files.is_empty()
        || editor.assets.iter().any(|asset| matches!(asset.importer.as_str(), "Scenario" | "Code"));
    if has_code {
        rows.push(Row::Code);
    }
    rows
}

/// Панель дерева слева с разделителем; его положение запоминается в настройках.
pub(crate) fn tree_panel(editor: &mut Editor, ui: &mut egui::Ui, wide: f32) {
    // В низком окне Favorites не должны вытеснять дерево за край: при первом показе свёрнуты.
    if !editor.project.favorites_init {
        editor.project.favorites_init = true;
        if ui.available_height() < 320.0 {
            editor.project.collapsed.insert(FAVORITES_KEY.to_string());
        }
    }
    let saved = editor.prefs.project_ui.split;
    let default = if saved >= 60.0 { saved } else { (wide * 0.24).clamp(140.0, 230.0) };
    let response = egui::SidePanel::left("project_tree")
        .resizable(true)
        .default_width(default)
        .width_range(100.0..=(wide * 0.6).max(140.0))
        .frame(egui::Frame::none().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(0.0, theme::SP_S)))
        .show_inside(ui, |ui| tree_contents(editor, ui));
    let width = response.response.rect.width();
    // Положение разделителя запоминается, когда его отпустили.
    if (width - saved).abs() > 0.5 && !ui.input(|i| i.pointer.any_down()) {
        editor.prefs.project_ui.split = width.round();
    }
}

/// Строки дерева. Используется и панелью, и выпадающим деревом узкого режима.
pub(crate) fn tree_contents(editor: &mut Editor, ui: &mut egui::Ui) {
    let rows = rows(editor);
    let cx = MenuCx::new(editor);
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .id_salt("project_tree_scroll")
        .auto_shrink([false, false])
        .show_rows(ui, theme::ROW, rows.len(), |ui, range| {
            for row in &rows[range] {
                draw_row(editor, ui, row, &cx);
            }
        });
}

fn dropped_into(editor: &mut Editor, ui: &egui::Ui, response: &egui::Response, folder: &str) {
    let copy = ui.input(|i| i.modifiers.command);
    if let Some(payload) = response.dnd_release_payload::<DragAsset>() {
        if drop_allowed(&payload.paths, folder, copy) {
            editor.project.cmds.push(Cmd::Move { paths: payload.paths.clone(), to: folder.to_string(), copy });
        }
    }
}

fn drop_allowed(paths: &[String], folder: &str, copy: bool) -> bool {
    !paths.is_empty()
        && !paths.iter().any(|p| p == folder || folder.starts_with(&format!("{p}/")))
        && (copy || paths.iter().any(|p| parent_of(p) != folder))
}

fn draw_row(editor: &mut Editor, ui: &mut egui::Ui, row: &Row, cx: &MenuCx) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
    let focused = editor.project.focus;
    let painter = ui.painter().clone();
    let left = rect.left() + theme::SP_M;
    match row {
        Row::Header { key, title, open } => {
            let response = ui.interact(rect, egui::Id::new(("project_tree_header", *key)), Sense::click());
            if response.hovered() {
                painter.rect_filled(rect, 0.0, theme::hover_fill());
            }
            icons::paint(
                &painter,
                Rect::from_center_size(pos2(left + 5.0, rect.center().y), vec2(9.0, 9.0)),
                if *open { Icon::ChevronDown } else { Icon::ChevronRight },
                theme::TEXT_MUTED,
            );
            painter.text(
                pos2(left + 18.0, rect.center().y),
                Align2::LEFT_CENTER,
                title.to_uppercase(),
                theme::strong(theme::FONT_SECTION),
                theme::TEXT_MUTED,
            );
            if response.clicked() {
                if *open {
                    editor.project.collapsed.insert((*key).to_string());
                } else {
                    editor.project.collapsed.remove(*key);
                }
            }
            // Каталог, брошенный на «Favorites», закрепляется.
            let pin_target = response.dnd_hover_payload::<DragAsset>().is_some();
            if pin_target {
                painter.rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.0_f32, theme::accent()));
            }
            if let Some(payload) = response.dnd_release_payload::<DragAsset>() {
                pin_dropped(editor, &payload.paths);
            }
        }
        Row::Search { query, name, user } => {
            let response = ui.interact(rect, egui::Id::new(("project_search", query.as_str())), Sense::click());
            let selected = editor.project.filter.trim() == query.as_str();
            theme::list_row(&painter, rect, selected, focused, response.hovered());
            icons::paint(
                &painter,
                Rect::from_center_size(pos2(left + 26.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
                Icon::Search,
                theme::TEXT_MUTED,
            );
            let text = elide_middle(&painter, name, &theme::body(), rect.right() - left - 44.0);
            painter.text(pos2(left + 38.0, rect.center().y), Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
            if response.clicked() {
                editor.project.cmds.push(Cmd::RunSearch(query.clone()));
            }
            let user = *user;
            let query = query.clone();
            let cmds = &mut editor.project.cmds;
            response.on_hover_text(trf!("Search: {query}", query = query)).context_menu(|ui| {
                theme::menu_body(ui);
                if theme::item(tr_s("Show Results")).icon(Icon::Search).show(ui) {
                    cmds.push(Cmd::RunSearch(query.clone()));
                }
                if user && theme::item(tr_s("Delete Saved Search")).icon(Icon::Trash).show(ui) {
                    cmds.push(Cmd::DeleteSearch(query.clone()));
                }
            });
        }
        Row::Pinned(path) => {
            let response = ui.interact(rect, egui::Id::new(("project_fav", path.as_str())), Sense::click());
            let selected = editor.project.place == Place::Assets && editor.project.folder == *path && editor.project.filter.trim().is_empty();
            let dropping = response.dnd_hover_payload::<DragAsset>().is_some() && drop_allowed(&editor.project.dragging, path, false);
            theme::list_row(&painter, rect, selected, focused, response.hovered() || dropping);
            if dropping {
                painter.rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.0_f32, theme::accent()));
            }
            icons::paint(
                &painter,
                Rect::from_center_size(pos2(left + 26.0, rect.center().y), vec2(14.0, 14.0)),
                Icon::Folder,
                theme::FOLDER,
            );
            let name = if path.is_empty() { tr_s("Assets") } else { file_name(path) };
            let text = elide_middle(&painter, name, &theme::body(), rect.right() - left - 44.0);
            painter.text(pos2(left + 38.0, rect.center().y), Align2::LEFT_CENTER, text, theme::body(), theme::TEXT);
            if response.clicked() {
                editor.project.cmds.push(Cmd::Go(path.clone()));
            }
            dropped_into(editor, ui, &response, path);
            let path = path.clone();
            let cmds = &mut editor.project.cmds;
            response.on_hover_text(format!("Assets/{path}")).context_menu(|ui| {
                theme::menu_body(ui);
                if theme::item(tr_s("Open")).icon(Icon::FolderOpen).show(ui) {
                    cmds.push(Cmd::Go(path.clone()));
                }
                if theme::item(tr_s("Remove from Favorites")).icon(Icon::Check).show(ui) {
                    cmds.push(Cmd::Unpin(path.clone()));
                }
            });
        }
        Row::Folder { path, depth, has_children, open } => {
            let response = ui.interact(rect, tree_row_id(path), Sense::click());
            let selected = editor.project.place == Place::Assets && editor.project.folder == *path && editor.project.filter.trim().is_empty();
            let copy = ui.input(|i| i.modifiers.command);
            let dropping = response.dnd_hover_payload::<DragAsset>().is_some() && drop_allowed(&editor.project.dragging, path, copy);
            theme::list_row(&painter, rect, selected, focused, response.hovered() || dropping);
            if dropping {
                painter.rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.0_f32, theme::accent()));
            }
            let left = left + *depth as f32 * theme::INDENT;
            if *has_children {
                let arrow_rect = Rect::from_min_size(pos2(left, rect.top()), vec2(14.0, rect.height()));
                let arrow = ui.interact(arrow_rect, egui::Id::new(("project_folder_arrow", path.as_str())), Sense::click());
                icons::paint(
                    &painter,
                    Rect::from_center_size(arrow_rect.center(), vec2(9.0, 9.0)),
                    if *open { Icon::ChevronDown } else { Icon::ChevronRight },
                    if arrow.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED },
                );
                if arrow.clicked() {
                    if *open {
                        editor.project.collapsed.insert(path.clone());
                    } else {
                        editor.project.collapsed.remove(path);
                    }
                }
            }
            icons::paint(
                &painter,
                Rect::from_center_size(pos2(left + 22.0, rect.center().y), vec2(14.0, 14.0)),
                if selected { Icon::FolderOpen } else { Icon::Folder },
                theme::FOLDER,
            );
            let name = if path.is_empty() { tr_s("Assets") } else { file_name(path) };
            let color = if selected { theme::TEXT_STRONG } else { theme::TEXT };
            let text = elide_middle(&painter, name, &theme::body(), (rect.right() - left - 40.0).max(30.0));
            painter.text(pos2(left + 34.0, rect.center().y), Align2::LEFT_CENTER, text, theme::body(), color);
            if response.clicked() {
                editor.project.cmds.push(Cmd::Go(path.clone()));
            }
            dropped_into(editor, ui, &response, path);
            let cmds = &mut editor.project.cmds;
            let folder = path.clone();
            response.context_menu(|ui| project_menus::tree_menu(ui, cmds, cx, &folder));
        }
        Row::Code => {
            let response = ui.interact(rect, tree_row_id(CODE_LOC), Sense::click());
            let selected = editor.project.place == Place::Code;
            theme::list_row(&painter, rect, selected, focused, response.hovered());
            icons::paint(
                &painter,
                Rect::from_center_size(pos2(left + 22.0, rect.center().y), vec2(14.0, 14.0)),
                Icon::Code,
                if selected { theme::TEXT_STRONG } else { Color32::from_rgb(0x9b, 0x7b, 0xff) },
            );
            painter.text(
                pos2(left + 34.0, rect.center().y),
                Align2::LEFT_CENTER,
                tr_s("Code"),
                theme::body(),
                if selected { theme::TEXT_STRONG } else { theme::TEXT },
            );
            if response.clicked() {
                editor.project.cmds.push(Cmd::Go(CODE_LOC.to_string()));
            }
        }
    }
}

/// Закрепить брошенные каталоги.
fn pin_dropped(editor: &mut Editor, paths: &[String]) {
    for path in paths {
        if editor.folders.iter().any(|f| f == path) {
            editor.project.cmds.push(Cmd::Pin(path.clone()));
        }
    }
}
