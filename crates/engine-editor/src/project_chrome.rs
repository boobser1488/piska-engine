//! Оправа окна Project: кнопки Назад / Вперёд / Вверх, хлебные крошки со строкой пути,
//! поиск с областью, вид, строка состояния, всплывающие сообщения, подтверждение удаления,
//! подсказка при перетаскивании файлов из ОС и встроенный браузер файлов для импорта.

use crate::i18n::{tr_s, trf, trn};
use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::icons::{self, Icon};
use crate::project::{DragAsset, Place};
use crate::project_fs::file_name;
use crate::project_nav::{click_select, Scope};
use crate::project_ops::{Cmd, ToastKind};
use crate::state::Editor;
use crate::theme::{self, item};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Back,
    Forward,
    Up,
}

/// Квадратная кнопка навигации со стрелкой, нарисованной линиями.
fn nav_button(ui: &mut egui::Ui, kind: Nav, enabled: bool, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(theme::CONTROL_H, theme::CONTROL_H), Sense::click());
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
    }
    let color = if enabled { theme::TEXT } else { theme::TEXT_DISABLED };
    let c = rect.center();
    let points = match kind {
        Nav::Back => vec![c + vec2(2.0, -5.0), c + vec2(-3.0, 0.0), c + vec2(2.0, 5.0)],
        Nav::Forward => vec![c + vec2(-2.0, -5.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 5.0)],
        Nav::Up => vec![c + vec2(-5.0, 2.0), c + vec2(0.0, -3.0), c + vec2(5.0, 2.0)],
    };
    ui.painter().add(egui::Shape::line(points, Stroke::new(1.5_f32, color)));
    response.on_hover_text(tooltip)
}

/// Ширина текста в точках.
fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), font, egui::Color32::WHITE).size().x
}

/// Верхняя полоса: навигация, путь, поиск, вид.
pub(crate) fn top_bar(editor: &mut Editor, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let compact = width < 460.0;
    let can_back = editor.project.history.can_back();
    let can_forward = editor.project.history.can_forward();
    let can_up = editor.project.place == Place::Code || !editor.project.folder.is_empty();
    let ctx = ui.ctx().clone();
    let cx = crate::project_menus::MenuCx::new(editor);
    let create_folder = cx.folder.clone();
    theme::toolbar_strip(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // Создать: каталог, сцену, материал, модель, скрипты — то же, что Create ▸ в меню.
        theme::dropdown_button(ui, tr_s("Create"), tr_s("Create a folder, scene, material, model or script here"), |ui| {
            crate::project_menus::create_items(ui, &mut editor.project.cmds, &cx, &create_folder);
        });
        theme::toolbar_separator(ui);
        if nav_button(ui, Nav::Back, can_back, tr_s("Back (Alt+Left, mouse button 4)")).clicked() && can_back {
            editor.project.cmds.push(Cmd::Back);
        }
        if nav_button(ui, Nav::Forward, can_forward, tr_s("Forward (Alt+Right, mouse button 5)")).clicked() && can_forward {
            editor.project.cmds.push(Cmd::Forward);
        }
        if nav_button(ui, Nav::Up, can_up, tr_s("Up one folder (Alt+Up, Backspace)")).clicked() && can_up {
            editor.project.cmds.push(Cmd::Up);
        }
        ui.add_space(theme::SP_S);
        if compact {
            // Дерево каталогов свёрнуто в выпадающий список.
            let response = crate::controls::menu_anchor(
                ui,
                vec2(theme::CONTROL_H, theme::CONTROL_H),
                true,
                |ui| {
                    theme::menu_body(ui);
                    ui.set_min_size(vec2(220.0, 260.0));
                    ui.set_max_size(vec2(260.0, 300.0));
                    crate::project_tree::tree_contents(editor, ui);
                },
            )
            .response;
            icons::paint(
                ui.painter(),
                Rect::from_center_size(response.rect.center(), vec2(theme::ICON, theme::ICON)),
                Icon::Folder,
                theme::FOLDER,
            );
            response.on_hover_text(tr_s("Folders"));
        }

        // Правая часть занимает, сколько нужно; путь — всё, что осталось.
        let wants_slider = width >= 720.0;
        let wants_scope = width >= 560.0;
        let search_width = if width >= 720.0 { 200.0 } else if width >= 560.0 { 150.0 } else { 120.0 };
        let right = search_width
            + if wants_scope { 98.0 } else { 0.0 }
            + if wants_slider { 96.0 } else { 0.0 }
            + 24.0 * 3.0
            + 90.0;
        let left_used = ui.min_rect().width();
        let path_width = (ui.available_width() - right - theme::SP_M).max(110.0).min(ui.available_width());
        let _ = left_used;
        path_bar(editor, ui, path_width, &ctx);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_S;
            view_controls(editor, ui, wants_slider, wants_scope, search_width);
        });
    });
}

fn view_controls(editor: &mut Editor, ui: &mut egui::Ui, slider: bool, scope: bool, search_width: f32) {
    if icons::button(ui, Icon::Refresh, false, None, tr_s("Rescan the assets directory (Ctrl+R)")).clicked() {
        editor.project.cmds.push(Cmd::Refresh);
    }
    let details = editor.prefs.project_ui.details;
    if icons::button(ui, Icon::Inspector, details, None, tr_s("Show or hide the details pane")).clicked() {
        editor.prefs.project_ui.details = !details;
    }
    let grid = editor.prefs.project_ui.grid;
    if icons::button(ui, Icon::List, !grid, None, tr_s("Show as a list with columns")).clicked() {
        editor.prefs.project_ui.grid = false;
    }
    if icons::button(ui, Icon::Tiles, grid, None, tr_s("Show as tiles with previews")).clicked() {
        editor.prefs.project_ui.grid = true;
    }
    if slider {
        // Левый край бегунка — плотный список, дальше — плитки растущего размера.
        let mut value = if grid { editor.prefs.project_ui.tile } else { 40.0 };
        let changed = ui
            .add(egui::Slider::new(&mut value, 40.0..=160.0).show_value(false))
            .on_hover_text(tr_s("Item size: drag to the left end for a compact list"))
            .changed();
        if changed {
            if value <= 44.0 {
                editor.prefs.project_ui.grid = false;
            } else {
                editor.prefs.project_ui.grid = true;
                editor.prefs.project_ui.tile = value;
            }
        }
    }
    ui.add_space(theme::SP_S);

    // Сохранить поиск (если его ещё нет в «Favorites»).
    let query = editor.project.filter.trim().to_string();
    let known = crate::project_nav::DEFAULT_SEARCHES.iter().any(|(_, q)| *q == query)
        || editor.prefs.project_ui.searches.iter().any(|(_, q)| *q == query);
    if !query.is_empty() && !known && icons::small_button(ui, Icon::Star, tr_s("Save this search to Favorites")).clicked() {
        editor.project.cmds.push(Cmd::SaveSearch);
    }
    if scope {
        let current = editor.prefs.project_ui.scope();
        let mut chosen = None;
        let response = crate::controls::menu_button(
            ui,
            egui::RichText::new(current.label()).size(theme::FONT_CAPTION).color(theme::TEXT),
            92.0,
            |ui| {
                theme::menu_body(ui);
                theme::menu_section(ui, tr_s("Search in"));
                for option in [Scope::ThisFolder, Scope::AllAssets] {
                    if item(option.label()).checked(current == option).show(ui) {
                        chosen = Some(option);
                    }
                }
            },
        )
        .response;
        response.on_hover_text(tr_s("Where the search looks"));
        if let Some(option) = chosen {
            editor.prefs.project_ui.scope = option.id().to_string();
        }
    }
    let ctx = ui.ctx().clone();
    let id = editor.project.search_id;
    if editor.project.focus_search {
        if let Some(id) = id {
            ctx.memory_mut(|m| m.request_focus(id));
            editor.project.focus_search = false;
        }
    }
    let response = theme::search_field(ui, &mut editor.project.filter, tr_s("Search  (t:scene  t:material …)"), search_width);
    editor.project.search_id = Some(response.id);
}

/// Хлебные крошки; щелчок по пустому месту превращает их в поле пути.
fn path_bar(editor: &mut Editor, ui: &mut egui::Ui, width: f32, ctx: &egui::Context) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::CONTROL_H), Sense::hover());
    crate::controls::paint_frame(ui.painter(), rect, theme::R_S, theme::BG_INSET, theme::LINE_STRONG);
    if let Some(text) = editor.project.path_edit.clone() {
        let mut text = text;
        let id = egui::Id::new("project_path_field");
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(theme::SP_S, 0.0))));
        let response = child.add(
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .frame(false)
                .vertical_align(egui::Align::Center)
                .desired_width(rect.width() - theme::SP_M)
                .min_size(vec2(rect.width() - theme::SP_M, theme::CONTROL_H)),
        );
        if editor.project.path_error.is_none() && !response.has_focus() && !response.lost_focus() {
            response.request_focus();
            // Первый кадр: выделить всё.
            let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(text.chars().count()),
            )));
            state.store(ctx, id);
        }
        if let Some(why) = editor.project.path_error.clone() {
            egui::Area::new(id.with("error"))
                .order(egui::Order::Tooltip)
                .fixed_pos(rect.left_bottom() + vec2(0.0, 2.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).fill(theme::BG_RAISED).show(ui, |ui| {
                        ui.label(egui::RichText::new(why).size(theme::FONT_CAPTION).color(theme::ERROR));
                    });
                });
        }
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape {
            editor.project.path_edit = None;
            editor.project.path_error = None;
        } else if response.lost_focus() && enter {
            editor.project.path_edit = Some(text.clone());
            editor.project.cmds.push(Cmd::GoTyped(text));
            // Ошибка вернёт фокус полю в следующем кадре.
            ctx.memory_mut(|m| m.request_focus(id));
        } else if response.lost_focus() {
            editor.project.path_edit = None;
            editor.project.path_error = None;
        } else {
            editor.project.path_edit = Some(text);
        }
        return;
    }

    // Фон ловит щелчок «в пустое место» — крошки заводятся после него и перехватывают свои.
    let background = ui.interact(rect, egui::Id::new("project_path_background"), Sense::click());
    let mut crumbs: Vec<(String, String)> = Vec::new();
    let in_code = editor.project.place == Place::Code;
    crumbs.push((if in_code { tr_s("Code") } else { tr_s("Assets") }.to_string(), if in_code { crate::project_nav::CODE_LOC.to_string() } else { String::new() }));
    if !in_code {
        let mut path = String::new();
        for part in editor.project.folder.split('/').filter(|p| !p.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            crumbs.push((part.to_string(), path.clone()));
        }
    }
    let searching = !editor.project.filter.trim().is_empty() && !in_code;
    // Сколько крошек поместится: берём с конца, лишнее — в меню «…».
    let budget = rect.width() - 2.0 * theme::SP_M - if searching { 110.0 } else { 0.0 };
    let widths: Vec<f32> = crumbs.iter().map(|(label, _)| text_width(ui, label, theme::body()) + theme::SP_M + 14.0).collect();
    let mut first = 0;
    while first + 1 < crumbs.len() && widths[first..].iter().sum::<f32>() + if first > 0 { 30.0 } else { 0.0 } > budget {
        first += 1;
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(theme::SP_S, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    if first > 0 {
        let hidden: Vec<(String, String)> = crumbs[..first].to_vec();
        let mut chosen = None;
        child.menu_button("…", |ui| {
            theme::menu_body(ui);
            for (label, path) in &hidden {
                if item(label).icon(Icon::Folder).show(ui) {
                    chosen = Some(path.clone());
                }
            }
        });
        if let Some(path) = chosen {
            editor.project.cmds.push(Cmd::Go(path));
        }
        icons::inline(&mut child, Icon::ChevronRight, theme::TEXT_DISABLED);
    }
    let count = crumbs.len();
    for (index, (label, path)) in crumbs.iter().enumerate().skip(first) {
        let last = index + 1 == count && !searching;
        let galley = child.painter().layout_no_wrap(label.clone(), theme::body(), theme::TEXT);
        let (crumb, _) = child.allocate_exact_size(vec2(galley.size().x + theme::SP_M, theme::CONTROL_H - 2.0), Sense::hover());
        let response = child.interact(crumb, egui::Id::new(("project_crumb", path.as_str())), Sense::click());
        let dropping = response.dnd_hover_payload::<DragAsset>().is_some() && !in_code;
        if response.hovered() || dropping {
            child.painter().rect_filled(crumb, theme::R_S, if dropping { theme::accent_soft() } else { theme::BG_HOVER });
        }
        let color = if last { theme::TEXT } else { theme::TEXT_MUTED };
        child.painter().galley(crumb.center() - galley.size() * 0.5, galley, color);
        if response.clicked() {
            editor.project.cmds.push(Cmd::Go(path.clone()));
        }
        if let Some(payload) = response.dnd_release_payload::<DragAsset>() {
            if !in_code {
                let copy = ctx.input(|i| i.modifiers.command);
                editor.project.cmds.push(Cmd::Move { paths: payload.paths.clone(), to: path.clone(), copy });
            }
        }
        if index + 1 < count || searching {
            icons::inline(&mut child, Icon::ChevronRight, theme::TEXT_DISABLED);
        }
    }
    if searching {
        child.label(egui::RichText::new(tr_s("Search results")).color(theme::TEXT).size(theme::FONT_BODY));
    }
    if background.clicked() && !in_code {
        let text = if editor.project.folder.is_empty() { tr_s("Assets").to_string() } else { format!("Assets/{}", editor.project.folder) };
        editor.project.path_edit = Some(text);
        editor.project.path_error = None;
    }
    background.on_hover_text(tr_s("Click to type a path"));
}

/// Строка состояния окна: выделение, число элементов.
pub(crate) fn status_line(editor: &mut Editor, ui: &mut egui::Ui) {
    let text = status_text(editor);
    egui::TopBottomPanel::bottom("project_status")
        .exact_height(theme::STATUS_BAR)
        .show_separator_line(false)
        .frame(egui::Frame::none().fill(theme::BG_HEADER))
        .show_inside(ui, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0_f32, theme::LINE));
            let painter = ui.painter().clone();
            painter.with_clip_rect(rect.shrink2(vec2(theme::SP_M, 0.0))).text(
                pos2(rect.left() + theme::SP_M, rect.center().y),
                Align2::LEFT_CENTER,
                &text.0,
                theme::caption(),
                theme::TEXT_MUTED,
            );
            painter.text(
                pos2(rect.right() - theme::SP_M, rect.center().y),
                Align2::RIGHT_CENTER,
                &text.1,
                theme::caption(),
                theme::TEXT_MUTED,
            );
        });
}

/// (слева, справа) для строки состояния.
pub(crate) fn status_text(editor: &Editor) -> (String, String) {
    let project = &editor.project;
    let total = project.last_total;
    let right = if project.filter.trim().is_empty() {
        trn!("{n} item", "{n} items", total)
    } else {
        trn!("{n} result", "{n} results", total)
    };
    let left = match project.selection.len() {
        0 => {
            if project.place == Place::Code {
                tr_s("Code").to_string()
            } else if project.folder.is_empty() {
                tr_s("Assets").to_string()
            } else {
                format!("Assets/{}", project.folder)
            }
        }
        1 => {
            let path = project.selection.iter().next().cloned().unwrap_or_default();
            format!("Assets/{path}")
        }
        n => {
            let summary = project.summary.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
            trn!("{n} item selected · {size}", "{n} items selected · {size}", n, size = crate::sorting::size_label(summary.bytes))
        }
    };
    (left, right)
}

/// Тосты, подтверждение удаления, подсказка при перетаскивании из ОС, браузер файлов.
pub(crate) fn overlays(editor: &mut Editor, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let panel = editor.project.panel_rect.unwrap_or(ui.max_rect());

    // Файлы из файлового менеджера ОС.
    let hovered = ctx.input(|i| !i.raw.hovered_files.is_empty());
    let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
    if hovered && editor.project.place == Place::Assets {
        let dest = if editor.project.folder.is_empty() { tr_s("Assets").to_string() } else { format!("Assets/{}", editor.project.folder) };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("project_drop_hint")));
        painter.rect_filled(panel, 0.0, theme::BG_APP.gamma_multiply(0.6));
        painter.rect_stroke(panel.shrink(4.0), theme::R_M, Stroke::new(1.5_f32, theme::accent()));
        painter.text(panel.center(), Align2::CENTER_CENTER, trf!("Drop files to import them into {dest}", dest = dest), theme::body(), theme::TEXT_STRONG);
    }
    if !dropped.is_empty() {
        if editor.project.place == Place::Assets {
            let dest = editor.project.folder.clone();
            editor.project.cmds.push(Cmd::Import { dest, files: dropped });
        } else {
            editor.project_toast(&ctx, ToastKind::Warn, tr_s("Open an asset folder first: files are imported into the open folder"));
        }
    }

    // Тосты.
    editor.project.toasts.retain(|t| t.progress.is_some() || now - t.born < if t.kind == ToastKind::Info { 4.5 } else { 9.0 });
    if !editor.project.toasts.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        egui::Area::new(egui::Id::new("project_toasts"))
            .order(egui::Order::Foreground)
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(panel.right_bottom() - vec2(theme::SP_M, theme::STATUS_BAR + theme::SP_M))
            .interactable(false)
            .show(&ctx, |ui| {
                ui.set_max_width((panel.width() - 2.0 * theme::SP_M).clamp(200.0, 420.0));
                for toast in editor.project.toasts.iter().rev().take(3) {
                    egui::Frame::popup(ui.style())
                        .fill(theme::BG_RAISED)
                        .stroke(Stroke::new(1.0_f32, theme::LINE_SOFT))
                        .rounding(theme::R_L)
                        .inner_margin(egui::Margin::symmetric(theme::SP_M, theme::SP_S + 2.0))
                        .show(ui, |ui| {
                            let (icon, color) = match toast.kind {
                                ToastKind::Info => (Icon::Info, theme::INFO),
                                ToastKind::Warn => (Icon::Warn, theme::WARN),
                                ToastKind::Error => (Icon::Error, theme::ERROR),
                            };
                            ui.horizontal_top(|ui| {
                                icons::inline(ui, icon, color);
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(&toast.text).size(theme::FONT_BODY).color(theme::TEXT));
                                    if let Some((done, total)) = toast.progress {
                                        let fraction = (total > 0).then(|| done as f32 / total as f32);
                                        theme::progress_bar(ui, fraction, &if total > 0 { trf!("{done} of {total} files", done = done, total = total) } else { tr_s("Counting files…").to_string() });
                                    }
                                });
                            });
                        });
                    ui.add_space(theme::SP_S);
                }
            });
    }

    confirm_dialog(editor, &ctx, panel);
    browser_window(editor, &ctx);
}

/// Удаление насовсем (на системах без корзины): явный вопрос, в нём сказано, что будет.
fn confirm_dialog(editor: &mut Editor, ctx: &egui::Context, panel: Rect) {
    let Some(confirm) = &editor.project.confirm else { return };
    let paths = confirm.paths.clone();
    let mut decision = None;
    egui::Area::new(egui::Id::new("project_confirm"))
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(panel.center())
        .show(ctx, |ui| {
            theme::dialog_frame(ui.style()).show(ui, |ui| {
                ui.set_max_width(380.0);
                theme::dialog_title(ui, Icon::Trash, theme::ERROR, tr_s("Delete permanently?"));
                ui.add_space(theme::SP_S);
                let names: Vec<&str> = paths.iter().take(5).map(|p| file_name(p)).collect();
                let more = if paths.len() > 5 { trf!(" and {} more", paths.len() - 5) } else { String::new() };
                ui.label(
                    egui::RichText::new(trf!(
                        "This system has no Recycle Bin, so {p0} will be deleted PERMANENTLY from disk together with their .meta files. This cannot be undone.\n\n{p1}{more}",
                        p0 = if paths.len() == 1 { "this item".to_string() } else { trf!("these {} items", paths.len()) },
                        p1 = names.join(", ")
                    , more = more))
                    .color(theme::TEXT),
                );
                crate::window_kit::dialog_actions(ui, |ui| {
                    if theme::danger_button(ui, tr_s("Delete Permanently")).clicked() {
                        decision = Some(true);
                    }
                    if theme::secondary_button(ui, tr_s("Cancel")).clicked() {
                        decision = Some(false);
                    }
                });
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        decision = Some(false);
    }
    match decision {
        Some(true) => {
            editor.project.confirm = None;
            editor.project.cmds.push(Cmd::DeletePermanently(paths));
        }
        Some(false) => editor.project.confirm = None,
        None => {}
    }
}

// ----------------------------------------------------------------------
// Браузер файлов для Import New Asset…
// ----------------------------------------------------------------------

/// Встроенный выбор файлов и каталогов снаружи проекта.
pub(crate) struct Browser {
    /// Каталог ассетов, куда импортируем.
    pub dest: String,
    pub dir: PathBuf,
    pub path_text: String,
    pub entries: Vec<(String, bool, PathBuf)>,
    pub selected: BTreeSet<String>,
    anchor: Option<String>,
    pub error: Option<String>,
}

impl Browser {
    pub fn new(dest: String) -> Self {
        let start = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let mut browser = Self {
            dest,
            dir: PathBuf::new(),
            path_text: String::new(),
            entries: Vec::new(),
            selected: BTreeSet::new(),
            anchor: None,
            error: None,
        };
        browser.enter(start);
        browser
    }

    /// Прочитать каталог. Ошибка (нет доступа) остаётся на экране, каталог не меняется.
    pub fn enter(&mut self, dir: PathBuf) {
        match std::fs::read_dir(&dir) {
            Ok(read) => {
                let mut entries: Vec<(String, bool, PathBuf)> = read
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let is_dir = e.file_type().ok()?.is_dir();
                        (!name.to_ascii_lowercase().ends_with(".meta")).then(|| (name, is_dir, e.path()))
                    })
                    .collect();
                entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| crate::sorting::natural_cmp(&a.0, &b.0)));
                self.entries = entries;
                self.path_text = dir.display().to_string();
                self.dir = dir;
                self.selected.clear();
                self.anchor = None;
                self.error = None;
            }
            Err(err) => self.error = Some(trf!("Cannot open `{p0}`: {err}", p0 = dir.display(), err = err)),
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().filter(|(_, _, p)| self.selected.contains(&p.display().to_string())).map(|(_, _, p)| p.clone()).collect()
    }
}

fn browser_window(editor: &mut Editor, ctx: &egui::Context) {
    let Some(mut browser) = editor.project.browser.take() else { return };
    let mut keep = true;
    let mut import: Option<Vec<PathBuf>> = None;
    let title = if browser.dest.is_empty() { tr_s("Assets").to_string() } else { format!("Assets/{}", browser.dest) };
    // Заголовок рисует `dialog_title` внутри (в нём папка назначения).
    let mut open = true;
    crate::window_kit::tool_window(egui::Id::new("project_import_browser"), "")
        .default_size(vec2(520.0, 420.0))
        .show(ctx, &mut open, |ui| {
            theme::dialog_title(ui, Icon::ArrowDown, theme::accent(), &trf!("Import into {title}", title = title));
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                if icons::button(ui, Icon::ArrowUp, false, None, tr_s("Parent folder")).clicked() {
                    if let Some(parent) = browser.dir.parent().map(|p| p.to_path_buf()) {
                        browser.enter(parent);
                    }
                }
                let response = crate::controls::text_field(ui, &mut browser.path_text, "", f32::INFINITY);
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let target = PathBuf::from(browser.path_text.trim());
                    browser.enter(target);
                }
            });
            #[cfg(target_os = "windows")]
            ui.horizontal_wrapped(|ui| {
                for letter in b'A'..=b'Z' {
                    let drive = PathBuf::from(format!("{}:\\", letter as char));
                    if drive.exists() && theme::button(ui, &format!("{}:", letter as char)).clicked() {
                        browser.enter(drive);
                    }
                }
            });
            if let Some(why) = &browser.error {
                ui.label(egui::RichText::new(why).color(theme::ERROR).size(theme::FONT_CAPTION));
            }
            ui.add_space(theme::SP_S);
            let list_height = (ui.available_height() - 40.0).max(120.0);
            let keys: Vec<String> = browser.entries.iter().map(|(_, _, p)| p.display().to_string()).collect();
            let mut clicked: Option<(usize, bool, bool)> = None;
            let mut entered: Option<PathBuf> = None;
            egui::Frame::none().fill(theme::BG_INSET).show(ui, |ui| {
                ui.set_height(list_height);
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical().id_salt("project_import_list").auto_shrink([false, false]).show_rows(
                    ui,
                    theme::ROW,
                    browser.entries.len(),
                    |ui, range| {
                        for index in range {
                            let (name, is_dir, path) = &browser.entries[index];
                            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
                            let response = ui.interact(rect, egui::Id::new(("project_import_row", index, path.display().to_string())), Sense::click());
                            let selected = browser.selected.contains(&keys[index]);
                            theme::list_row(ui.painter(), rect, selected, true, response.hovered());
                            let icon = if *is_dir { Icon::Folder } else { Icon::File };
                            icons::paint(
                                ui.painter(),
                                Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(14.0, 14.0)),
                                icon,
                                if *is_dir { theme::FOLDER } else { theme::TEXT_MUTED },
                            );
                            ui.painter().text(pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, name, theme::body(), theme::TEXT);
                            if response.clicked() {
                                let m = ui.input(|i| i.modifiers);
                                clicked = Some((index, m.command, m.shift));
                            }
                            if response.double_clicked() && *is_dir {
                                entered = Some(path.clone());
                            }
                        }
                    },
                );
            });
            if let Some((index, ctrl, shift)) = clicked {
                let mut anchor = browser.anchor.clone();
                click_select(&mut browser.selected, &mut anchor, &keys, index, ctrl, shift);
                browser.anchor = anchor;
            }
            if let Some(dir) = entered {
                browser.enter(dir);
            }
            crate::window_kit::dialog_actions(ui, |ui| {
                let count = browser.selected.len();
                ui.add_enabled_ui(count > 0, |ui| {
                    let label = if count == 0 { tr_s("Import").to_string() } else { trn!("Import {n} item", "Import {n} items", count) };
                    if theme::primary_button(ui, &label).clicked() {
                        import = Some(browser.paths());
                    }
                });
                if theme::secondary_button(ui, tr_s("Cancel")).clicked() {
                    keep = false;
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(tr_s("Ctrl-click selects several. Double-click opens a folder.")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
                });
            });
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        keep = false;
    }
    if let Some(files) = import {
        let dest = browser.dest.clone();
        editor.project.cmds.push(Cmd::Import { dest, files });
        keep = false;
    }
    if keep {
        editor.project.browser = Some(browser);
    }
}
