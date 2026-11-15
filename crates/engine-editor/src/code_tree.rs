//! Дерево файлов вкладки Code (как Solution Explorer / проводник VS Code): каталоги (и
//! пустые) и файлы кода, выделение (щелчок, Ctrl+щелчок, Shift+щелчок, стрелки, Enter,
//! набор имени), свёртка каталогов, правый щелчок — меню файла / каталога / пустого места,
//! переименование на месте (F2), удаление с подтверждением (Del), копирование и перенос
//! (Ctrl+C / Ctrl+X / Ctrl+V, перетаскивание на каталог), новые файлы из шаблонов.
//!
//! Файловые операции — те же, что у окна Project (`project_fs.rs`: `.meta` едет вместе с
//! ассетом), в пределах каталогов кода и ассетов. После операции: список перечитывается,
//! вкладки следуют за переименованными файлами (с историей Undo), вкладки удалённых
//! закрываются, `mod.rs` скриптов на Rust пишется заново, база ассетов обновляется.

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::i18n::{tr_s, trf};
use crate::icons::{self, Icon};
use crate::project_fs::{self, Outcome};
use crate::state::{Action, Editor};
use crate::theme::{self, item as menu_item};

/// Состояние дерева файлов.
#[derive(Default)]
pub(crate) struct Tree {
    /// Выделенные файлы и каталоги (по порядку выделения).
    pub selected: Vec<PathBuf>,
    /// Начало выделения диапазоном (Shift).
    anchor: Option<PathBuf>,
    /// Переименование на месте: путь и набранное имя.
    pub rename: Option<InlineRename>,
    /// Вырезанные / скопированные файлы: (пути, вырезаны ли).
    pub clipboard: Option<(Vec<PathBuf>, bool)>,
    /// Подтверждение удаления.
    pub confirm_delete: Option<Vec<PathBuf>>,
    /// Окно «New File»: каталог и имя.
    pub new_file: Option<NewFile>,
    /// Набор имени для перехода (type-ahead) и когда нажали последнюю букву.
    typed: String,
    typed_at: f64,
    /// Перетаскивают эти файлы; над каким каталогом.
    drag: Option<Vec<PathBuf>>,
    drop_target: Option<PathBuf>,
    /// Файл активной вкладки, который уже показан в дереве.
    revealed: Option<PathBuf>,
    /// Прокрутить к этой строке.
    scroll_to: Option<PathBuf>,
    /// Клавиатура у дерева (для F2 / Del — см. `shortcuts.rs`).
    pub focused: bool,
    /// Тесты: что «показали в проводнике» и открыли во внешнем редакторе (без запуска программ).
    #[cfg(test)]
    pub launched: Vec<PathBuf>,
    /// Тесты и снимки: где строки дерева на экране.
    #[cfg(test)]
    pub rows_at: Vec<(PathBuf, Rect)>,
}

pub(crate) struct InlineRename {
    pub path: PathBuf,
    pub text: String,
    focus: bool,
}

pub(crate) struct NewFile {
    pub folder: PathBuf,
    pub name: String,
    pub error: Option<String>,
    focus: bool,
}

/// Что создать в каталоге.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NewKind {
    RustScript,
    ScenarioScript,
    CppScript,
    Shader(engine_assets::ShaderKind),
    TextFile,
    Folder,
}

/// Команда дереву (меню, клавиатура, перетаскивание) — выполняется после кадра.
#[derive(Clone, Debug)]
pub(crate) enum TreeCmd {
    Open(Vec<PathBuf>),
    BeginRename(PathBuf),
    CommitRename(PathBuf, String),
    Delete(Vec<PathBuf>),
    ConfirmDelete,
    Duplicate(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Copy(Vec<PathBuf>),
    Paste(PathBuf),
    Move(Vec<PathBuf>, PathBuf),
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    Reveal(PathBuf),
    External(PathBuf),
    New(NewKind, PathBuf),
    CreateFile,
    Refresh,
}

/// Строка дерева.
#[derive(Clone, Debug)]
struct Row {
    path: PathBuf,
    /// Имя для показа с корнем (`src/scripts/a.rs`) — ключ свёрнутых каталогов.
    shown: String,
    name: String,
    depth: usize,
    folder: bool,
    open: bool,
}

/// Строки дерева по порядку: каталоги перед файлами, имена без учёта регистра.
fn rows(editor: &Editor) -> Vec<Row> {
    let code = &editor.code;
    let mut children: std::collections::BTreeMap<String, Vec<(String, PathBuf, bool)>> = Default::default();
    let mut roots = Vec::new();
    for (shown, path) in &code.folders {
        match shown.rsplit_once('/') {
            Some((parent, _)) => children.entry(parent.to_string()).or_default().push((shown.clone(), path.clone(), true)),
            None => roots.push((shown.clone(), path.clone())),
        }
    }
    for (shown, path) in &code.files {
        if let Some((parent, _)) = shown.rsplit_once('/') {
            children.entry(parent.to_string()).or_default().push((shown.clone(), path.clone(), false));
        }
    }
    for list in children.values_mut() {
        list.sort_by_key(|(shown, _, folder)| (!*folder, shown.rsplit('/').next().unwrap_or(shown).to_lowercase()));
        list.dedup_by(|a, b| a.1 == b.1);
    }
    let mut out = Vec::new();
    fn visit(code: &crate::code::CodeState, children: &std::collections::BTreeMap<String, Vec<(String, PathBuf, bool)>>, shown: &str, path: &Path, depth: usize, out: &mut Vec<Row>) {
        let open = !code.collapsed.contains(shown);
        out.push(Row { path: path.to_path_buf(), shown: shown.to_string(), name: shown.rsplit('/').next().unwrap_or(shown).to_string(), depth, folder: true, open });
        if !open {
            return;
        }
        for (child, child_path, folder) in children.get(shown).into_iter().flatten() {
            if *folder {
                visit(code, children, child, child_path, depth + 1, out);
            } else {
                out.push(Row { path: child_path.clone(), shown: child.clone(), name: child.rsplit('/').next().unwrap_or(child).to_string(), depth: depth + 1, folder: false, open: false });
            }
        }
    }
    for (shown, path) in roots {
        visit(code, &children, &shown, &path, 0, &mut out);
    }
    out
}

/// Каталоги, в которых разрешены операции: каталоги кода и ассеты.
fn roots(editor: &Editor) -> Vec<PathBuf> {
    let mut out = editor.config.code_dirs.clone();
    if let Some(assets) = editor.assets_root() {
        out.push(assets);
    }
    out
}

/// Корень (самый глубокий из подходящих) и путь относительно него (`a/b.rs`). Отказ —
/// вне каталогов проекта, в `target/` и `.git/`.
fn locate(roots: &[PathBuf], path: &Path) -> Result<(PathBuf, String), String> {
    let found = roots.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.components().count());
    let Some(root) = found else {
        return Err(trf!("`{p0}` is outside the project's code folders and assets", p0 = path.display()));
    };
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    if relative.split('/').any(|part| matches!(part, "target" | ".git")) {
        return Err(trf!("`{p0}` is inside a build or repository folder; the editor does not touch it", p0 = path.display()));
    }
    Ok((root.clone(), relative))
}

fn folder_of(path: &Path, is_folder: bool) -> PathBuf {
    if is_folder {
        path.to_path_buf()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// Панель FILES: заголовок с кнопками, дерево, меню, окна.
pub(crate) fn panel(editor: &mut Editor, ui: &mut egui::Ui) {
    let mut cmds: Vec<TreeCmd> = std::mem::take(&mut editor.code.tree_commands);
    let caps = NewCaps::of(editor);
    let selected_folder = selected_folder(editor);
    // --- Заголовок: FILES, «новый файл ▾», «новый каталог» ---
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::TAB_BAR), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, theme::BG_HEADER);
    ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
    ui.painter().text(pos2(bar.left() + theme::SP_L, bar.center().y), Align2::LEFT_CENTER, tr_s("FILES"), theme::strong(theme::FONT_SECTION), theme::TEXT_MUTED);
    let mut header = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(theme::SP_M, 0.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
    header.spacing_mut().item_spacing.x = theme::SP_XS;
    if let Some(folder) = selected_folder.clone() {
        if crate::controls::icon_button(&mut header, Icon::Folder, tr_s("New Folder")).clicked() {
            cmds.push(TreeCmd::New(NewKind::Folder, folder.clone()));
        }
        let size = vec2(theme::control_h(&header), theme::control_h(&header));
        let response = crate::controls::menu_anchor(&mut header, size, false, |ui| {
            theme::menu_body(ui);
            new_file_items(ui, caps, &folder, &mut cmds);
        });
        icons::paint(header.painter(), Rect::from_center_size(response.response.rect.center(), vec2(theme::ICON_S, theme::ICON_S)), Icon::Plus, theme::TEXT_MUTED);
        response.response.on_hover_text(tr_s("New File"));
    }
    if editor.code.files.is_empty() && editor.code.folders.is_empty() {
        ui.add_space(theme::SP_L);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(theme::SP_L);
            ui.label(egui::RichText::new(tr_s("No code folders. Start the editor with `EditorConfig::code_dir(\"src\")`.")).color(theme::TEXT_MUTED));
        });
        return;
    }
    reveal_active(editor);
    let list = rows(editor);
    let now = ui.input(|i| i.time);
    let tree_id = egui::Id::new("code_file_tree");
    let focused = ui.memory(|m| m.has_focus(tree_id));
    editor.code.tree.focused = focused;
    let active = editor.code.documents.get(editor.code.active).map(|d| d.path.clone());
    let can_paste = editor.code.tree.clipboard.as_ref().is_some_and(|(p, _)| !p.is_empty());
    let external = !editor.prefs.external_editor.trim().is_empty();
    let shortcuts = MenuKeys { rename: editor.prefs.shortcuts.text("rename-symbol") }; // i18n-skip: id команды

    let mut toggle: Option<String> = None;
    let mut click: Option<(usize, egui::Modifiers)> = None;
    let mut list_rect = Rect::NOTHING;
    egui::ScrollArea::vertical().id_salt("code_files_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(theme::SP_S);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let tree = &mut editor.code.tree;
        #[cfg(test)]
        tree.rows_at.clear();
        for (k, row) in list.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::click_and_drag());
            #[cfg(test)]
            tree.rows_at.push((row.path.clone(), rect));
            if tree.scroll_to.as_ref() == Some(&row.path) {
                ui.scroll_to_rect(rect, None);
                tree.scroll_to = None;
            }
            let selected = tree.selected.contains(&row.path);
            let dropping = tree.drag.is_some() && row.folder && pointer.is_some_and(|p| rect.contains(p));
            if dropping {
                tree.drop_target = Some(row.path.clone());
                ui.painter().rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.0_f32, theme::accent()));
            } else if selected {
                ui.painter().rect_filled(rect, 0.0, if focused { theme::selection_fill() } else { theme::selection_unfocused() });
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 0.0, theme::hover_fill());
            }
            let left = rect.left() + theme::SP_M + row.depth as f32 * theme::INDENT;
            if row.folder {
                icons::paint(ui.painter(), Rect::from_center_size(pos2(left + 6.0, rect.center().y), vec2(9.0, 9.0)), if row.open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_MUTED);
            }
            let (icon, color) = row_icon(row);
            icons::paint(ui.painter(), Rect::from_center_size(pos2(left + 22.0, rect.center().y), vec2(14.0, 14.0)), icon, color);
            let label_left = left + 34.0;
            let renaming = tree.rename.as_ref().is_some_and(|r| r.path == row.path);
            if renaming {
                let rename = tree.rename.as_mut().expect("renaming");
                let field_rect = Rect::from_min_max(pos2(label_left - theme::SP_XS, rect.top() + 1.0), pos2(rect.right() - theme::SP_S, rect.bottom() - 1.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field_rect));
                let field = crate::controls::text_field_with_id(&mut child, Some(tree_id.with("rename")), &mut rename.text, "", field_rect.width());
                if std::mem::take(&mut rename.focus) {
                    field.request_focus();
                    // Имя без расширения выделено, как в проводнике.
                    let stem = rename.text.rfind('.').filter(|&i| i > 0 && !row.folder).unwrap_or(rename.text.len());
                    let chars = rename.text[..stem].chars().count();
                    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), field.id).unwrap_or_default();
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(chars))));
                    state.store(ui.ctx(), field.id);
                }
                let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if escape {
                    tree.rename = None;
                    ui.memory_mut(|m| m.request_focus(tree_id));
                } else if field.lost_focus() || (enter && field.has_focus()) {
                    cmds.push(TreeCmd::CommitRename(row.path.clone(), rename.text.clone()));
                    ui.memory_mut(|m| m.request_focus(tree_id));
                }
            } else {
                let cut = tree.clipboard.as_ref().is_some_and(|(p, cut)| *cut && p.contains(&row.path));
                let text_color = if cut { theme::TEXT_DISABLED } else if selected || active.as_deref() == Some(row.path.as_path()) { theme::TEXT_STRONG } else { theme::TEXT };
                ui.painter().with_clip_rect(rect).text(pos2(label_left, rect.center().y), Align2::LEFT_CENTER, &row.name, theme::body(), text_color);
            }
            // Щелчок: выделить (файл — и открыть); по стрелке каталога — свернуть.
            if response.clicked() || response.secondary_clicked() {
                // Модификаторы — из самого события кнопки (как их видела система в тот миг).
                let modifiers = ui.input(|i| {
                    i.events.iter().rev().find_map(|e| match e {
                        egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                        _ => None,
                    })
                    .unwrap_or(i.modifiers)
                });
                let on_chevron = row.folder && pointer.is_some_and(|p| p.x < left + 14.0);
                if response.clicked() && on_chevron {
                    toggle = Some(row.shown.clone());
                } else if response.secondary_clicked() {
                    // Правый щелчок только выделяет (каталог не сворачивается, файл не открывается).
                    if !tree.selected.contains(&row.path) {
                        tree.selected = vec![row.path.clone()];
                        tree.anchor = Some(row.path.clone());
                    }
                } else {
                    click = Some((k, modifiers));
                }
                ui.memory_mut(|m| m.request_focus(tree_id));
            }
            if response.double_clicked() && row.folder {
                toggle = Some(row.shown.clone());
            }
            if response.drag_started() {
                let paths = if tree.selected.contains(&row.path) { tree.selected.clone() } else { vec![row.path.clone()] };
                tree.drag = Some(paths);
            }
            let keys = if tree.selected.contains(&row.path) { tree.selected.clone() } else { vec![row.path.clone()] };
            let path = row.path.clone();
            let folder = row.folder;
            response.context_menu(|ui| {
                theme::menu_body(ui);
                if folder {
                    folder_menu(ui, editor_state(&shortcuts, can_paste, external), &path, &keys, &mut cmds, |ui, cmds| new_file_submenu(ui, caps, &path, cmds));
                } else {
                    file_menu(ui, editor_state(&shortcuts, can_paste, external), &path, &keys, &mut cmds);
                }
            });
            list_rect = list_rect.union(rect);
        }
        // Пустое место под списком: меню каталога-корня (или выделенного каталога).
        let rest = ui.available_size();
        let (empty, empty_response) = ui.allocate_exact_size(vec2(rest.x, rest.y.max(theme::ROW * 2.0)), Sense::click());
        if empty_response.clicked() {
            tree.selected.clear();
            ui.memory_mut(|m| m.request_focus(tree_id));
        }
        let _ = empty;
        if let Some(folder) = selected_folder.clone() {
            empty_response.context_menu(|ui| {
                theme::menu_body(ui);
                empty_menu(ui, editor_state(&shortcuts, can_paste, external), &folder, &mut cmds, |ui, cmds| new_file_submenu(ui, caps, &folder, cmds));
            });
        }
    });
    // New File в подменю пустого места и каталога — пункты, которым нужен редактор
    // (C++ / шейдер), отбираются при выполнении.
    // Клавиатура у дерева: стрелки и Tab не уводят фокус к другим полям.
    ui.interact(list_rect, tree_id, Sense::focusable_noninteractive());
    if focused {
        ui.memory_mut(|m| m.set_focus_lock_filter(tree_id, egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false }));
    }

    // Перетаскивание: отпустили над каталогом — перенести.
    if editor.code.tree.drag.is_some() && ui.input(|i| i.pointer.any_released()) {
        let paths = editor.code.tree.drag.take().unwrap_or_default();
        if let Some(target) = editor.code.tree.drop_target.take() {
            if !paths.contains(&target) {
                cmds.push(TreeCmd::Move(paths, target));
            }
        }
    }
    if editor.code.tree.drag.is_none() {
        editor.code.tree.drop_target = None;
    } else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    if let Some(shown) = toggle {
        if !editor.code.collapsed.remove(&shown) {
            editor.code.collapsed.insert(shown);
        }
    }
    if let Some((k, modifiers)) = click {
        select_click(editor, &list, k, modifiers, &mut cmds);
    }
    if focused {
        keyboard(editor, ui, &list, now, &mut cmds);
    }
    dialogs(editor, ui.ctx(), &mut cmds);
    for cmd in cmds {
        run(editor, ui.ctx(), cmd);
    }
}

/// Сочетания, которые показывают меню.
struct MenuKeys {
    rename: String,
}

#[derive(Clone, Copy)]
struct MenuState<'a> {
    keys: &'a MenuKeys,
    can_paste: bool,
    external: bool,
}

fn editor_state(keys: &MenuKeys, can_paste: bool, external: bool) -> MenuState<'_> {
    MenuState { keys, can_paste, external }
}

fn row_icon(row: &Row) -> (Icon, egui::Color32) {
    if row.folder {
        return (if row.open { Icon::FolderOpen } else { Icon::Folder }, theme::FOLDER);
    }
    let language = crate::code::Language::of(Path::new(&row.name));
    let color = match language {
        crate::code::Language::Rust => egui::Color32::from_rgb(0xc8, 0x8a, 0x64),
        crate::code::Language::Cpp => egui::Color32::from_rgb(0x7f, 0xa8, 0xd8),
        crate::code::Language::Ron => theme::PREFAB,
        crate::code::Language::Wgsl => egui::Color32::from_rgb(0xa0, 0x8c, 0xcc),
        crate::code::Language::Scenario => egui::Color32::from_rgb(0x6c, 0xb0, 0xa8),
        crate::code::Language::Plain => theme::TEXT_MUTED,
    };
    (if language == crate::code::Language::Plain { Icon::Text } else { Icon::Script }, color)
}

/// Каталог для «нового файла»: выделенный каталог, каталог выделенного файла, иначе
/// первый корень.
pub(crate) fn selected_folder(editor: &Editor) -> Option<PathBuf> {
    let tree = &editor.code.tree;
    if let Some(last) = tree.selected.last() {
        let is_folder = editor.code.folders.iter().any(|(_, p)| p == last);
        return Some(folder_of(last, is_folder));
    }
    editor.code.folders.iter().find(|(shown, _)| !shown.contains('/')).map(|(_, p)| p.clone())
}

/// Что можно создать в проекте (собирается раз в кадр).
#[derive(Clone, Copy)]
struct NewCaps {
    rust: bool,
    assets: bool,
    cpp: bool,
}

impl NewCaps {
    fn of(editor: &Editor) -> Self {
        Self { rust: editor.config.rust_script_dir.is_some(), assets: editor.assets_root().is_some(), cpp: editor.config.script_dir.is_some() }
    }
}

/// Пункты «New File»: настоящие отправные точки (недоступное — выключено с причиной).
fn new_file_items(ui: &mut egui::Ui, caps: NewCaps, folder: &Path, cmds: &mut Vec<TreeCmd>) {
    if menu_item(tr_s("Rust Script…")).icon(Icon::Script).enabled_if(caps.rust, tr_s("The editor does not know where Rust scripts live (`EditorConfig::rust_scripts`)")).show(ui) {
        cmds.push(TreeCmd::New(NewKind::RustScript, folder.to_path_buf()));
    }
    if menu_item(tr_s("Scenario Script…")).icon(Icon::Script).enabled_if(caps.assets, tr_s("Scenario scripts live in the assets; mount an assets directory first")).show(ui) {
        cmds.push(TreeCmd::New(NewKind::ScenarioScript, folder.to_path_buf()));
    }
    if caps.cpp && menu_item(tr_s("C++ Script…")).icon(Icon::Script).show(ui) {
        cmds.push(TreeCmd::New(NewKind::CppScript, folder.to_path_buf()));
    }
    if caps.assets {
        theme::submenu(ui, Icon::Code, tr_s("Shader"), |ui| {
            for (kind, label) in [
                (engine_assets::ShaderKind::Surface, tr_s("Surface Shader")),
                (engine_assets::ShaderKind::Unlit, tr_s("Unlit Shader")),
                (engine_assets::ShaderKind::Sprite, tr_s("Sprite Shader")),
                (engine_assets::ShaderKind::Post, tr_s("Post Effect")),
            ] {
                if menu_item(label).icon(Icon::Code).show(ui) {
                    cmds.push(TreeCmd::New(NewKind::Shader(kind), folder.to_path_buf()));
                }
            }
        });
    }
    if menu_item(tr_s("Text File…")).icon(Icon::Text).shortcut("Ctrl+N").show(ui) {
        cmds.push(TreeCmd::New(NewKind::TextFile, folder.to_path_buf()));
    }
}

/// Подменю New File ▸ в меню каталога и пустого места.
fn new_file_submenu(ui: &mut egui::Ui, caps: NewCaps, folder: &Path, cmds: &mut Vec<TreeCmd>) {
    theme::submenu(ui, Icon::Plus, tr_s("New File"), |ui| new_file_items(ui, caps, folder, cmds));
}

fn common_items(ui: &mut egui::Ui, st: MenuState<'_>, path: &Path, cmds: &mut Vec<TreeCmd>) {
    theme::menu_separator(ui);
    if menu_item(tr_s("Copy Path")).icon(Icon::Copy).show(ui) {
        cmds.push(TreeCmd::CopyPath(path.to_path_buf()));
    }
    if menu_item(tr_s("Copy Relative Path")).icon(Icon::Copy).show(ui) {
        cmds.push(TreeCmd::CopyRelativePath(path.to_path_buf()));
    }
    if menu_item(tr_s("Show in File Manager")).icon(Icon::FolderOpen).show(ui) {
        cmds.push(TreeCmd::Reveal(path.to_path_buf()));
    }
    let _ = st;
}

fn file_menu(ui: &mut egui::Ui, st: MenuState<'_>, path: &Path, keys: &[PathBuf], cmds: &mut Vec<TreeCmd>) {
    if menu_item(tr_s("Open")).icon(Icon::Code).shortcut("Enter").show(ui) {
        cmds.push(TreeCmd::Open(keys.to_vec()));
    }
    if menu_item(tr_s("Open in External Editor")).icon(Icon::External).enabled_if(st.external, tr_s("Set the external code editor in Preferences → General")).show(ui) {
        cmds.push(TreeCmd::External(path.to_path_buf()));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Cut")).icon(Icon::Copy).shortcut("Ctrl+X").show(ui) {
        cmds.push(TreeCmd::Cut(keys.to_vec()));
    }
    if menu_item(tr_s("Copy")).icon(Icon::Copy).shortcut("Ctrl+C").show(ui) {
        cmds.push(TreeCmd::Copy(keys.to_vec()));
    }
    if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(st.can_paste, tr_s("Copy or cut files first")).show(ui) {
        cmds.push(TreeCmd::Paste(folder_of(path, false)));
    }
    if menu_item(tr_s("Duplicate")).icon(Icon::Copy).show(ui) {
        cmds.push(TreeCmd::Duplicate(keys.to_vec()));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Rename…")).icon(Icon::Text).shortcut(&st.keys.rename).enabled_if(keys.len() == 1, tr_s("Select one file to rename")).show(ui) {
        cmds.push(TreeCmd::BeginRename(path.to_path_buf()));
    }
    if menu_item(tr_s("Delete…")).icon(Icon::Trash).shortcut("Del").show(ui) {
        cmds.push(TreeCmd::Delete(keys.to_vec()));
    }
    common_items(ui, st, path, cmds);
}

fn folder_menu(ui: &mut egui::Ui, st: MenuState<'_>, path: &Path, keys: &[PathBuf], cmds: &mut Vec<TreeCmd>, new_file: impl FnOnce(&mut egui::Ui, &mut Vec<TreeCmd>)) {
    new_file(ui, cmds);
    if menu_item(tr_s("New Folder")).icon(Icon::Folder).show(ui) {
        cmds.push(TreeCmd::New(NewKind::Folder, path.to_path_buf()));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Cut")).icon(Icon::Copy).shortcut("Ctrl+X").show(ui) {
        cmds.push(TreeCmd::Cut(keys.to_vec()));
    }
    if menu_item(tr_s("Copy")).icon(Icon::Copy).shortcut("Ctrl+C").show(ui) {
        cmds.push(TreeCmd::Copy(keys.to_vec()));
    }
    if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(st.can_paste, tr_s("Copy or cut files first")).show(ui) {
        cmds.push(TreeCmd::Paste(path.to_path_buf()));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Rename…")).icon(Icon::Text).shortcut(&st.keys.rename).enabled_if(keys.len() == 1, tr_s("Select one folder to rename")).show(ui) {
        cmds.push(TreeCmd::BeginRename(path.to_path_buf()));
    }
    if menu_item(tr_s("Delete…")).icon(Icon::Trash).shortcut("Del").show(ui) {
        cmds.push(TreeCmd::Delete(keys.to_vec()));
    }
    common_items(ui, st, path, cmds);
    theme::menu_separator(ui);
    if menu_item(tr_s("Refresh")).icon(Icon::Refresh).show(ui) {
        cmds.push(TreeCmd::Refresh);
    }
}

fn empty_menu(ui: &mut egui::Ui, st: MenuState<'_>, folder: &Path, cmds: &mut Vec<TreeCmd>, new_file: impl FnOnce(&mut egui::Ui, &mut Vec<TreeCmd>)) {
    new_file(ui, cmds);
    if menu_item(tr_s("New Folder")).icon(Icon::Folder).show(ui) {
        cmds.push(TreeCmd::New(NewKind::Folder, folder.to_path_buf()));
    }
    if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(st.can_paste, tr_s("Copy or cut files first")).show(ui) {
        cmds.push(TreeCmd::Paste(folder.to_path_buf()));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Show in File Manager")).icon(Icon::FolderOpen).show(ui) {
        cmds.push(TreeCmd::Reveal(folder.to_path_buf()));
    }
    if menu_item(tr_s("Refresh")).icon(Icon::Refresh).show(ui) {
        cmds.push(TreeCmd::Refresh);
    }
}

/// Файл активной вкладки — раскрыть его каталоги и выделить (один раз на смену вкладки).
fn reveal_active(editor: &mut Editor) {
    let Some(active) = editor.code.documents.get(editor.code.active).map(|d| d.path.clone()) else { return };
    if editor.code.tree.revealed.as_ref() == Some(&active) {
        return;
    }
    editor.code.tree.revealed = Some(active.clone());
    let Some((shown, _)) = editor.code.files.iter().find(|(_, p)| *p == active).cloned() else { return };
    let mut dir = shown.as_str();
    while let Some((parent, _)) = dir.rsplit_once('/') {
        editor.code.collapsed.remove(parent);
        dir = parent;
    }
    editor.code.tree.selected = vec![active.clone()];
    editor.code.tree.anchor = Some(active.clone());
    editor.code.tree.scroll_to = Some(active);
}

/// Щелчок по строке: выделить (Ctrl — добавить / убрать, Shift — диапазон); простой
/// щелчок по файлу его и открывает.
fn select_click(editor: &mut Editor, list: &[Row], k: usize, modifiers: egui::Modifiers, cmds: &mut Vec<TreeCmd>) {
    let row = &list[k];
    let tree = &mut editor.code.tree;
    if modifiers.shift {
        let from = tree.anchor.as_ref().and_then(|a| list.iter().position(|r| r.path == *a)).unwrap_or(k);
        let (a, b) = (from.min(k), from.max(k));
        tree.selected = list[a..=b].iter().map(|r| r.path.clone()).collect();
    } else if modifiers.command || modifiers.ctrl {
        if let Some(at) = tree.selected.iter().position(|p| *p == row.path) {
            tree.selected.remove(at);
        } else {
            tree.selected.push(row.path.clone());
        }
        tree.anchor = Some(row.path.clone());
    } else {
        tree.selected = vec![row.path.clone()];
        tree.anchor = Some(row.path.clone());
        if row.folder {
            if !editor.code.collapsed.remove(&row.shown) {
                editor.code.collapsed.insert(row.shown.clone());
            }
        } else {
            cmds.push(TreeCmd::Open(vec![row.path.clone()]));
        }
    }
}

/// Клавиатура дерева: стрелки, Home / End, ← / → (свернуть / развернуть), Enter, Del,
/// Ctrl+C / X / V, набор имени.
fn keyboard(editor: &mut Editor, ui: &mut egui::Ui, list: &[Row], now: f64, cmds: &mut Vec<TreeCmd>) {
    if editor.code.tree.rename.is_some() || list.is_empty() {
        return;
    }
    let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
    let current = editor.code.tree.selected.last().and_then(|p| list.iter().position(|r| r.path == *p));
    for event in events {
        match event {
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let target = match key {
                    egui::Key::ArrowDown => Some(current.map_or(0, |c| (c + 1).min(list.len() - 1))),
                    egui::Key::ArrowUp => Some(current.map_or(0, |c| c.saturating_sub(1))),
                    egui::Key::Home => Some(0),
                    egui::Key::End => Some(list.len() - 1),
                    _ => None,
                };
                if let Some(t) = target {
                    let tree = &mut editor.code.tree;
                    if modifiers.shift {
                        let from = tree.anchor.as_ref().and_then(|a| list.iter().position(|r| r.path == *a)).unwrap_or(t);
                        let (a, b) = (from.min(t), from.max(t));
                        tree.selected = list[a..=b].iter().map(|r| r.path.clone()).collect();
                        // Последний — у курсора.
                        if let Some(at) = tree.selected.iter().position(|p| *p == list[t].path) {
                            let p = tree.selected.remove(at);
                            tree.selected.push(p);
                        }
                    } else {
                        tree.selected = vec![list[t].path.clone()];
                        tree.anchor = Some(list[t].path.clone());
                    }
                    tree.scroll_to = Some(list[t].path.clone());
                    continue;
                }
                let Some(c) = current else { continue };
                let row = &list[c];
                match key {
                    egui::Key::ArrowLeft if row.folder && row.open => {
                        editor.code.collapsed.insert(row.shown.clone());
                    }
                    egui::Key::ArrowRight if row.folder && !row.open => {
                        editor.code.collapsed.remove(&row.shown);
                    }
                    egui::Key::Enter => {
                        if row.folder {
                            if !editor.code.collapsed.remove(&row.shown) {
                                editor.code.collapsed.insert(row.shown.clone());
                            }
                        } else {
                            let files: Vec<PathBuf> = editor.code.tree.selected.iter().filter(|p| !editor.code.folders.iter().any(|(_, f)| f == *p)).cloned().collect();
                            cmds.push(TreeCmd::Open(files));
                        }
                    }
                    egui::Key::Delete if modifiers.is_none() => cmds.push(TreeCmd::Delete(editor.code.tree.selected.clone())),
                    egui::Key::F2 if modifiers.is_none() => cmds.push(TreeCmd::BeginRename(row.path.clone())),
                    _ => {}
                }
            }
            egui::Event::Copy => cmds.push(TreeCmd::Copy(editor.code.tree.selected.clone())),
            egui::Event::Cut => cmds.push(TreeCmd::Cut(editor.code.tree.selected.clone())),
            egui::Event::Paste(_) => {
                if let Some(folder) = selected_folder(editor) {
                    cmds.push(TreeCmd::Paste(folder));
                }
            }
            egui::Event::Text(t) => {
                // Набор имени: к первой строке, чьё имя начинается с набранного.
                let tree = &mut editor.code.tree;
                if now - tree.typed_at > 1.0 {
                    tree.typed.clear();
                }
                tree.typed.push_str(&t.to_lowercase());
                tree.typed_at = now;
                let start = current.unwrap_or(0);
                let found = (0..list.len()).map(|i| (start + i) % list.len()).find(|&i| list[i].name.to_lowercase().starts_with(&tree.typed));
                if let Some(i) = found {
                    tree.selected = vec![list[i].path.clone()];
                    tree.anchor = Some(list[i].path.clone());
                    tree.scroll_to = Some(list[i].path.clone());
                }
            }
            _ => {}
        }
    }
}

/// Окна: подтверждение удаления, имя нового файла.
fn dialogs(editor: &mut Editor, ctx: &egui::Context, cmds: &mut Vec<TreeCmd>) {
    if let Some(paths) = editor.code.tree.confirm_delete.clone() {
        let unsaved: Vec<String> = editor
            .code
            .documents
            .iter()
            .filter(|d| d.dirty() && paths.iter().any(|p| d.path.starts_with(p)))
            .map(|d| d.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            .collect();
        let names: Vec<String> = paths.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
        let recycle = recycle();
        let mut confirm = false;
        let mut cancel = false;
        let response = crate::window_kit::dialog(egui::Id::new("code_tree_delete"), "").width(0.0).show(ctx, |ui| {
            ui.set_width(420.0);
            let title = if names.len() == 1 { trf!("Delete “{name}”?", name = names[0]) } else { trf!("Delete {n} items?", n = names.len()) };
            theme::dialog_title(ui, Icon::Trash, theme::ERROR, &title);
            ui.add_space(theme::SP_M);
            let shown = if names.len() > 8 { format!("{}, …", names[..8].join(", ")) } else { names.join(", ") }; // i18n-skip: список имён
            ui.label(egui::RichText::new(shown).color(theme::TEXT_STRONG));
            ui.add_space(theme::SP_S);
            let how = if recycle { tr_s("They go to the Recycle Bin.") } else { tr_s("They are deleted permanently (there is no undo).") };
            ui.label(egui::RichText::new(how).color(theme::TEXT_MUTED));
            if !unsaved.is_empty() {
                ui.add_space(theme::SP_S);
                ui.label(egui::RichText::new(trf!("Unsaved changes in {names} will be lost.", names = unsaved.join(", "))).color(theme::WARN));
            }
            crate::window_kit::dialog_actions(ui, |ui| {
                confirm |= theme::danger_button(ui, tr_s("Delete")).clicked();
                cancel |= theme::secondary_button(ui, tr_s("Cancel")).clicked();
            });
        });
        if response.should_close() && !confirm {
            cancel = true;
        }
        if confirm {
            cmds.push(TreeCmd::ConfirmDelete);
        } else if cancel {
            editor.code.tree.confirm_delete = None;
        }
    }
    if editor.code.tree.new_file.is_some() {
        let mut create = false;
        let mut cancel = false;
        let folder_label = editor.code.tree.new_file.as_ref().map(|n| n.folder.display().to_string()).unwrap_or_default();
        let response = crate::window_kit::dialog(egui::Id::new("code_tree_new_file"), "").width(0.0).show(ctx, |ui| {
            ui.set_width(420.0);
            let Some(new) = editor.code.tree.new_file.as_mut() else { return };
            theme::dialog_title(ui, Icon::Text, theme::TEXT_MUTED, tr_s("New File"));
            ui.add_space(theme::SP_M);
            ui.label(egui::RichText::new(trf!("In `{folder}`. Any name and extension, for example `notes.md` or `helpers.rs`.", folder = folder_label)).color(theme::TEXT_MUTED));
            ui.add_space(theme::SP_L);
            let field = crate::controls::text_field_with_id(ui, Some(egui::Id::new("code_tree_new_file_name")), &mut new.name, "", f32::INFINITY);
            if std::mem::take(&mut new.focus) {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            if let Some(error) = &new.error {
                ui.label(egui::RichText::new(error).color(theme::ERROR));
            }
            crate::window_kit::dialog_actions(ui, |ui| {
                create |= theme::primary_button(ui, tr_s("Create")).clicked();
                cancel |= theme::secondary_button(ui, tr_s("Cancel")).clicked();
            });
        });
        if response.should_close() && !create {
            cancel = true;
        }
        if create {
            cmds.push(TreeCmd::CreateFile);
        } else if cancel {
            editor.code.tree.new_file = None;
        }
    }
}

/// Удалять в корзину (в тестах — никогда: корзина пользователя не для них).
fn recycle() -> bool {
    !cfg!(test) && project_fs::recycle_available()
}

/// Выполнить команду дерева.
pub(crate) fn run(editor: &mut Editor, ctx: &egui::Context, cmd: TreeCmd) {
    let roots = roots(editor);
    match cmd {
        TreeCmd::Open(paths) => {
            for path in paths.into_iter().filter(|p| p.is_file()) {
                editor.actions.push(Action::OpenCode(path));
            }
        }
        TreeCmd::BeginRename(path) => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            editor.code.tree.rename = Some(InlineRename { path, text: name, focus: true });
        }
        TreeCmd::CommitRename(path, name) => {
            editor.code.tree.rename = None;
            let old = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name == old {
                return;
            }
            match locate(&roots, &path).and_then(|(root, rel)| project_fs::rename_item(&root, &rel, &name).map(|to| (root, to))) {
                Ok((root, to)) => {
                    let to = root.join(to);
                    log::info!("renamed `{}` to `{}`", path.display(), to.display());
                    moved(editor, ctx, &[(path, to.clone())]);
                    editor.code.tree.selected = vec![to];
                }
                Err(why) => log::error!("{}", trf!("Cannot rename `{old}`: {why}", old = old, why = why)),
            }
        }
        TreeCmd::Delete(paths) => {
            if !paths.is_empty() {
                editor.code.tree.confirm_delete = Some(paths);
            }
        }
        TreeCmd::ConfirmDelete => {
            let Some(paths) = editor.code.tree.confirm_delete.take() else { return };
            let mut outcome = Outcome::default();
            let mut gone = Vec::new();
            for path in &paths {
                let result = locate(&roots, path).and_then(|(root, rel)| {
                    if recycle() {
                        let mut targets = vec![path.clone()];
                        if path.is_file() && project_fs::meta_of(path).is_file() {
                            targets.push(project_fs::meta_of(path));
                        }
                        project_fs::trash(&targets)
                    } else {
                        let o = project_fs::delete_permanently(&root, &[rel]);
                        o.failed.first().map_or(Ok(()), |(_, why)| Err(why.clone()))
                    }
                });
                match result {
                    Ok(()) => {
                        gone.push(path.clone());
                        outcome.done.push((path.display().to_string(), String::new()));
                    }
                    Err(why) => outcome.failed.push((path.display().to_string(), why)),
                }
            }
            // Вкладки удалённых — закрыть (о несохранённом спросили в окне).
            for path in &gone {
                while let Some(index) = editor.code.documents.iter().position(|d| d.path.starts_with(path)) {
                    editor.code.close(index);
                }
            }
            editor.code.tree.selected.clear();
            for (path, why) in &outcome.failed {
                log::error!("{}", trf!("Cannot delete `{path}`: {why}", path = path, why = why));
            }
            changed(editor, &gone);
        }
        TreeCmd::Duplicate(paths) => {
            let mut new = Vec::new();
            for path in &paths {
                match locate(&roots, path).and_then(|(root, rel)| project_fs::copy_into(&root, &rel, project_fs::parent_of(&rel)).map(|to| root.join(to))) {
                    Ok(to) => new.push(to),
                    Err(why) => log::error!("{}", trf!("Cannot duplicate `{p0}`: {why}", p0 = path.display(), why = why)),
                }
            }
            changed(editor, &new);
            editor.code.tree.selected = new;
        }
        TreeCmd::Cut(paths) | TreeCmd::Copy(paths) if paths.is_empty() => {}
        TreeCmd::Cut(paths) => editor.code.tree.clipboard = Some((paths, true)),
        TreeCmd::Copy(paths) => editor.code.tree.clipboard = Some((paths, false)),
        TreeCmd::Paste(folder) => {
            let Some((paths, cut)) = editor.code.tree.clipboard.clone() else { return };
            let done = transfer(editor, ctx, &roots, &paths, &folder, !cut);
            if cut && !done.is_empty() {
                editor.code.tree.clipboard = None;
            }
        }
        TreeCmd::Move(paths, folder) => {
            transfer(editor, ctx, &roots, &paths, &folder, false);
        }
        TreeCmd::CopyPath(path) => ctx.copy_text(path.display().to_string()),
        TreeCmd::CopyRelativePath(path) => {
            let text = match locate(&roots, &path) {
                Ok((root, rel)) => {
                    let base = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    format!("{base}/{rel}")
                }
                Err(_) => path.display().to_string(),
            };
            ctx.copy_text(text);
        }
        TreeCmd::Reveal(path) => {
            #[cfg(test)]
            editor.code.tree.launched.push(path.clone());
            #[cfg(not(test))]
            crate::state::reveal(&path);
        }
        TreeCmd::External(path) => {
            #[cfg(test)]
            editor.code.tree.launched.push(path.clone());
            #[cfg(not(test))]
            if let Err(error) = editor.open_in_external_editor(&path, 1) {
                log::error!("{error}");
            }
        }
        TreeCmd::New(kind, folder) => new_item(editor, &roots, kind, folder),
        TreeCmd::CreateFile => {
            let Some(new) = editor.code.tree.new_file.as_mut() else { return };
            let name = new.name.trim().to_string();
            if let Err(why) = project_fs::validate_name(&name) {
                new.error = Some(why);
                return;
            }
            let path = new.folder.join(&name);
            if path.exists() {
                new.error = Some(trf!("`{name}` already exists here", name = name));
                return;
            }
            if let Err(why) = locate(&roots, &new.folder) {
                new.error = Some(why);
                return;
            }
            if let Err(err) = std::fs::write(&path, "") {
                new.error = Some(err.to_string());
                return;
            }
            editor.code.tree.new_file = None;
            log::info!("created `{}`", path.display());
            changed(editor, std::slice::from_ref(&path));
            editor.code.tree.selected = vec![path.clone()];
            editor.actions.push(Action::OpenCode(path));
        }
        TreeCmd::Refresh => changed(editor, &[]),
    }
}

/// Новый файл / каталог в `folder`.
fn new_item(editor: &mut Editor, roots: &[PathBuf], kind: NewKind, folder: PathBuf) {
    let assets = editor.assets_root();
    let assets_relative = |folder: &Path| -> String {
        match &assets {
            Some(a) if folder.starts_with(a) => folder.strip_prefix(a).unwrap_or(folder).to_string_lossy().replace('\\', "/"),
            _ => String::new(),
        }
    };
    match kind {
        NewKind::RustScript => editor.open_script_dialog(true, String::new(), None),
        NewKind::CppScript => editor.open_script_dialog(false, String::new(), None),
        NewKind::ScenarioScript => {
            let rel = assets_relative(&folder);
            editor.open_scenario_dialog(&rel, None);
        }
        NewKind::Shader(kind) => {
            let rel = assets_relative(&folder);
            editor.apply(Action::CreateShader { folder: rel, kind });
            changed(editor, &[]);
        }
        NewKind::TextFile => {
            editor.code.tree.new_file = Some(NewFile { folder, name: "NewFile.txt".to_string(), error: None, focus: true });
        }
        NewKind::Folder => match locate(roots, &folder) {
            Ok((root, rel)) => {
                let dir = root.join(&rel);
                let name = project_fs::unique_name(&dir, "New Folder", true);
                match std::fs::create_dir(dir.join(&name)) {
                    Ok(()) => {
                        let path = dir.join(&name);
                        changed(editor, std::slice::from_ref(&path));
                        editor.code.tree.selected = vec![path.clone()];
                        editor.code.tree.rename = Some(InlineRename { path, text: name, focus: true });
                    }
                    Err(err) => log::error!("{}", trf!("Cannot create a folder: {why}", why = err)),
                }
            }
            Err(why) => log::error!("{why}"),
        },
    }
}

/// Перенести (или скопировать) файлы в каталог. Возвращает новые пути.
fn transfer(editor: &mut Editor, ctx: &egui::Context, roots: &[PathBuf], paths: &[PathBuf], folder: &Path, copy: bool) -> Vec<PathBuf> {
    let mut moves = Vec::new();
    let mut new = Vec::new();
    for path in paths {
        let result = locate(roots, path).and_then(|(root, rel)| {
            let (dest_root, dest_rel) = locate(roots, folder)?;
            if dest_root != root {
                return Err(tr_s("moving files between the code folders and the assets is not supported; use the system file manager").to_string());
            }
            if copy { project_fs::copy_into(&root, &rel, &dest_rel) } else { project_fs::move_into(&root, &rel, &dest_rel) }.map(|to| root.join(to))
        });
        match result {
            Ok(to) => {
                if !copy && to != *path {
                    moves.push((path.clone(), to.clone()));
                }
                new.push(to);
            }
            Err(why) => log::error!("{}", trf!("Cannot move `{p0}`: {why}", p0 = path.display(), why = why)),
        }
    }
    if copy {
        changed(editor, &new);
    } else {
        moved(editor, ctx, &moves);
    }
    editor.code.tree.selected = new.clone();
    new
}

/// Файлы переехали: вкладки следуют за ними (с историей Undo), затем — общее «изменилось».
pub(crate) fn moved(editor: &mut Editor, ctx: &egui::Context, moves: &[(PathBuf, PathBuf)]) {
    for (from, to) in moves {
        let renamed: Vec<(PathBuf, PathBuf)> = editor
            .code
            .documents
            .iter()
            .filter(|d| d.path.starts_with(from))
            .map(|d| {
                let rest = d.path.strip_prefix(from).unwrap_or(Path::new(""));
                let new = if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) };
                (d.path.clone(), new)
            })
            .collect();
        for (old, new) in renamed {
            let old_id = editor.code.documents.iter().find(|d| d.path == old).map(|d| d.text_id());
            editor.code.rename_document(&old, &new);
            let new_id = editor.code.documents.iter().find(|d| d.path == new).map(|d| d.text_id());
            // История Undo и курсоры вкладки — под новым путём.
            if let (Some(old_id), Some(new_id)) = (old_id, new_id) {
                let state = crate::code_widget::take(ctx, old_id);
                crate::code_widget::put(ctx, new_id, state);
            }
            for p in editor.code.nav.mru.iter_mut().chain(editor.code.nav.closed.iter_mut()) {
                if *p == old {
                    p.clone_from(&new);
                }
            }
            if editor.code.tree.revealed.as_ref() == Some(&old) {
                editor.code.tree.revealed = Some(new.clone());
            }
        }
    }
    let mut touched: Vec<PathBuf> = moves.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect();
    touched.dedup();
    changed(editor, &touched);
}

/// После файловой операции: перечитать список, `mod.rs` скриптов на Rust, базу ассетов.
pub(crate) fn changed(editor: &mut Editor, touched: &[PathBuf]) {
    let assets = editor.assets_root();
    editor.code.scan(&editor.config.code_dirs, assets.as_deref());
    if let Some(dir) = editor.config.rust_script_dir.clone() {
        let rust = touched.iter().any(|p| p.parent().is_some_and(|parent| same_dir(parent, &dir)) || same_dir(p, &dir));
        if rust {
            if let Err(err) = crate::rust_scripts::regenerate_mod(&dir) {
                log::error!("{err}");
            }
            editor.rust.build_needed = true;
        }
    }
    if let Some(assets) = assets {
        if touched.is_empty() || touched.iter().any(|p| p.starts_with(&assets)) {
            editor.actions.push(Action::RefreshAssets);
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
