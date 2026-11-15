//! Команды окна Project: всё, что меняет файлы, выделение и навигацию. Панели только
//! кладут [`Cmd`] в очередь; выполняет их [`Editor::project_run`] в конце кадра, и после
//! любой правки файлов база ассетов пересканируется один раз, а не по разу на файл.

use crate::i18n::{tr_s, trf, trn};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use engine_core::reflect::Value;

use crate::project::{default_action, Confirm, Place};
use crate::project_fs::{self, file_name, join, parent_of, Outcome};
use crate::project_nav::{PathTarget, CODE_LOC};
use crate::state::{Action, Editor};

/// Что создать из меню Create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Create {
    Folder,
    Scene,
    Material,
    PrefabFromSelection,
    RustScript,
    CppScript,
    ScenarioScript,
    Model,
    /// Свой шейдер из шаблона (docs/api/shaders.md).
    Shader(engine_assets::ShaderKind),
}

/// Команда окну Project.
#[derive(Clone, Debug)]
pub(crate) enum Cmd {
    /// Перейти в каталог (`""` — корень) или в окно Code.
    Go(String),
    Back,
    Forward,
    Up,
    /// Набранный в строке пути текст.
    GoTyped(String),
    /// Открыть элементы (двойной щелчок, Enter).
    Open(Vec<String>),
    BeginRename(String),
    CommitRename { from: String, name: String },
    Cut(Vec<String>),
    Copy(Vec<String>),
    /// Вставить в каталог.
    Paste(String),
    Duplicate(Vec<String>),
    Delete(Vec<String>),
    DeletePermanently(Vec<String>),
    Move { paths: Vec<String>, to: String, copy: bool },
    Create { kind: Create, folder: String },
    CopyPath(String),
    CopyGuid(String),
    Reveal(String),
    Reimport(Vec<String>),
    FindReferences(String),
    Properties,
    Pin(String),
    Unpin(String),
    SaveSearch,
    DeleteSearch(String),
    RunSearch(String),
    /// Файловый браузер «Import New Asset…» в каталог.
    ImportDialog(String),
    /// Копировать файлы и каталоги снаружи в каталог `dest`.
    Import { dest: String, files: Vec<PathBuf> },
    SelectAll,
    Refresh,
    /// Обычное действие редактора (например, показать материал в инспекторе).
    Act(Action),
    /// Исходник игры вне ассетов: открыть, переименовать, удалить.
    SourceOpen(PathBuf),
    SourceRename(PathBuf),
    SourceDelete(PathBuf),
}

/// Внутренний буфер обмена окна Project (Ctrl+X / Ctrl+C).
#[derive(Clone, Debug)]
pub(crate) struct Clipboard {
    pub paths: Vec<String>,
    pub cut: bool,
    /// Тот же список текстом: так видно, что в системном буфере лежит наше.
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Info,
    Warn,
    Error,
}

/// Всплывающее сообщение внизу окна Project.
pub(crate) struct Toast {
    pub id: u64,
    pub text: String,
    pub kind: ToastKind,
    pub born: f64,
    /// Идущая операция: (сделано, всего).
    pub progress: Option<(usize, usize)>,
}

pub(crate) enum ImportMsg {
    Progress(usize, usize),
    Done(Outcome),
}

/// Копирование снаружи идёт в отдельном потоке: окно не замирает на больших каталогах.
pub(crate) struct ImportJob {
    pub rx: Receiver<ImportMsg>,
    pub dest: String,
    pub toast: u64,
}

/// Пустая сцена для Create → Scene.
const EMPTY_SCENE: &str = "// Новая сцена. Формат: RON, дерево объектов.\n(\n    name: \"New Scene\",\n    objects: [],\n)\n";

/// Подставить новый путь переименованного или перенесённого элемента.
pub(crate) fn remap(path: &str, moves: &[(String, String)]) -> Option<String> {
    for (from, to) in moves {
        if path == from {
            return Some(to.clone());
        }
        if let Some(rest) = path.strip_prefix(&format!("{from}/")) {
            return Some(format!("{to}/{rest}"));
        }
    }
    None
}

fn value_refers(value: &Value, guid: &str) -> bool {
    match value {
        Value::Asset(text) => text.eq_ignore_ascii_case(guid),
        Value::List(items) => items.iter().any(|v| value_refers(v, guid)),
        _ => false,
    }
}

impl Editor {
    /// Показать сообщение в окне Project и в консоли.
    pub(crate) fn project_toast(&mut self, ctx: &egui::Context, kind: ToastKind, text: impl Into<String>) -> u64 {
        let text = text.into();
        match kind {
            ToastKind::Info => log::info!("{text}"),
            ToastKind::Warn => log::warn!("{text}"),
            ToastKind::Error => log::error!("{text}"),
        }
        let id = self.project.toasts.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let born = ctx.input(|i| i.time);
        self.project.toasts.push(Toast { id, text, kind, born, progress: None });
        ctx.request_repaint();
        id
    }

    /// После переименования / переноса: открытая сцена, каталог окна, избранное и
    /// выделение едут вместе с файлами; база ассетов читается заново один раз.
    fn project_after_moves(&mut self, moves: &[(String, String)]) {
        if let Some(scene) = self.scene_path.clone() {
            if let Some(new) = remap(&scene, moves) {
                self.scene_path = Some(new);
            }
        }
        if let Some(folder) = remap(&self.project.folder, moves) {
            self.project.folder = folder;
        }
        let collapsed: Vec<String> = self.project.collapsed.iter().cloned().collect();
        self.project.collapsed = collapsed.into_iter().map(|p| remap(&p, moves).unwrap_or(p)).collect();
        for favorite in &mut self.prefs.project_ui.favorites {
            if let Some(new) = remap(favorite, moves) {
                *favorite = new;
            }
        }
        if let Some(clipboard) = &mut self.project.clipboard {
            for path in &mut clipboard.paths {
                if let Some(new) = remap(path, moves) {
                    *path = new;
                }
            }
        }
        self.project_refresh();
    }

    fn project_refresh(&mut self) {
        if let Err(err) = self.refresh_assets() {
            log::error!("cannot rescan the assets directory: {err}");
        }
    }

    fn project_is_folder(&self, path: &str) -> bool {
        self.folders.iter().any(|f| f == path)
    }

    pub(crate) fn project_run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        let Some(root) = self.assets_root() else { return };
        match cmd {
            Cmd::Go(location) => {
                self.project.go(&location);
            }
            Cmd::Back | Cmd::Forward => {
                let folders = &self.folders;
                let exists = |loc: &str| loc.is_empty() || loc == CODE_LOC || folders.iter().any(|f| f == loc);
                let target = if matches!(cmd, Cmd::Back) {
                    self.project.history.back(&exists)
                } else {
                    self.project.history.forward(&exists)
                };
                if let Some(location) = target {
                    self.project.go(&location);
                }
            }
            Cmd::Up => {
                let up = if self.project.place == Place::Code { String::new() } else { parent_of(&self.project.folder).to_string() };
                if self.project.place == Place::Code || !self.project.folder.is_empty() {
                    self.project.go(&up);
                }
            }
            Cmd::GoTyped(text) => {
                let files: Vec<String> = self.assets.iter().map(|a| a.path.clone()).collect();
                match crate::project_nav::resolve_path(&text, &self.folders, &files) {
                    Ok(PathTarget::Folder(folder)) => {
                        self.project.go(&folder);
                        self.project.path_edit = None;
                        self.project.path_error = None;
                    }
                    Ok(PathTarget::Asset(asset)) => {
                        self.project.reveal(&asset);
                        self.project.path_edit = None;
                        self.project.path_error = None;
                    }
                    Err(why) => self.project.path_error = Some(why),
                }
            }
            Cmd::Open(keys) => self.project_open(&root, keys),
            Cmd::BeginRename(key) => {
                if self.project_is_folder(&key) || self.assets.iter().any(|a| a.path == key) {
                    // Элемента нет на экране (строка дерева, результаты другого каталога):
                    // открыть его каталог, поле появится, когда элемент будет в списке.
                    let listed = self.project.listing.as_ref().is_some_and(|l| l.keys.contains(&key));
                    if !listed {
                        self.project.go(parent_of(&key));
                        self.project.pending_rename = Some(key);
                        return;
                    }
                    let name = file_name(&key).to_string();
                    self.project.select_only(&key);
                    self.project.rename = Some(crate::project::Rename { path: key.clone(), text: name, fresh: true });
                    self.project.scroll_to = Some(key);
                }
            }
            Cmd::CommitRename { from, name } => {
                self.project.rename = None;
                if name == file_name(&from) {
                    return;
                }
                match project_fs::rename_item(&root, &from, &name) {
                    Ok(to) => {
                        self.project_after_moves(&[(from.clone(), to.clone())]);
                        self.project.select_only(&to);
                        log::info!("renamed `{from}` to `{to}`");
                    }
                    Err(why) => {
                        self.project_toast(ctx, ToastKind::Error, trf!("Cannot rename `{p0}`: {why}", p0 = file_name(&from), why = why));
                    }
                }
            }
            Cmd::Cut(keys) | Cmd::Copy(keys) if keys.is_empty() => {}
            Cmd::Cut(keys) => self.project_set_clipboard(ctx, keys, true),
            Cmd::Copy(keys) => self.project_set_clipboard(ctx, keys, false),
            Cmd::Paste(into) => self.project_paste(ctx, &root, &into),
            Cmd::Duplicate(keys) => {
                let mut outcome = Outcome::default();
                for key in &keys {
                    match project_fs::copy_into(&root, key, parent_of(key)) {
                        Ok(new) => outcome.done.push((key.clone(), new)),
                        Err(why) => outcome.failed.push((key.clone(), why)),
                    }
                }
                self.project_finish_copy(ctx, &outcome, tr_s("Duplicated"), keys.len());
            }
            Cmd::Delete(keys) => {
                if keys.is_empty() {
                    return;
                }
                if project_fs::recycle_available() {
                    self.project_trash(ctx, &root, keys);
                } else {
                    self.project.confirm = Some(Confirm { paths: keys });
                }
            }
            Cmd::DeletePermanently(keys) => {
                let outcome = project_fs::delete_permanently(&root, &keys);
                let gone: Vec<String> = outcome.done.iter().map(|(p, _)| p.clone()).collect();
                self.project_after_delete(ctx, &gone, &outcome, keys.len(), tr_s("Deleted"));
            }
            Cmd::Move { paths, to, copy } => self.project_move(ctx, &root, paths, &to, copy),
            Cmd::Create { kind, folder } => self.project_create(ctx, &root, kind, &folder),
            Cmd::CopyPath(key) => {
                ctx.copy_text(format!("Assets/{key}"));
                self.project_toast(ctx, ToastKind::Info, trf!("Copied the path `Assets/{key}`", key = key));
            }
            Cmd::CopyGuid(key) => {
                if let Some(asset) = self.assets.iter().find(|a| a.path == key) {
                    ctx.copy_text(asset.guid.to_string());
                    self.project_toast(ctx, ToastKind::Info, trf!("Copied the GUID of `{}`", file_name(&key)));
                }
            }
            Cmd::Reveal(key) => self.apply(Action::RevealAsset(key)),
            Cmd::Reimport(keys) => self.project_reimport(ctx, keys),
            Cmd::FindReferences(key) => self.project_find_references(ctx, &key),
            Cmd::Properties => self.prefs.project_ui.details = true,
            Cmd::Pin(folder) => {
                if !self.prefs.project_ui.favorites.contains(&folder) {
                    self.prefs.project_ui.favorites.push(folder);
                }
            }
            Cmd::Unpin(folder) => self.prefs.project_ui.favorites.retain(|f| *f != folder),
            Cmd::SaveSearch => {
                let query = self.project.filter.trim().to_string();
                let known = crate::project_nav::DEFAULT_SEARCHES.iter().any(|(_, q)| *q == query)
                    || self.prefs.project_ui.searches.iter().any(|(_, q)| *q == query);
                if !query.is_empty() && !known {
                    self.prefs.project_ui.searches.push((query.clone(), query));
                }
            }
            Cmd::DeleteSearch(query) => self.prefs.project_ui.searches.retain(|(_, q)| *q != query),
            Cmd::RunSearch(query) => {
                self.project.place = Place::Assets;
                self.project.filter = query;
                self.prefs.project_ui.scope = crate::project_nav::Scope::AllAssets.id().to_string();
            }
            Cmd::ImportDialog(dest) => self.project.browser = Some(crate::project_chrome::Browser::new(dest)),
            Cmd::Import { dest, files } => self.project_import(ctx, &root, dest, files),
            Cmd::SelectAll => {
                if let Some(listing) = self.project.listing.clone() {
                    let keys: Vec<String> = listing.items.iter().filter(|i| i.selectable()).map(|i| i.key.clone()).collect();
                    self.project.select_many(keys);
                }
            }
            Cmd::Refresh => self.apply(Action::RefreshAssets),
            Cmd::Act(action) => self.actions.push(action),
            Cmd::SourceOpen(path) => self.actions.push(Action::OpenCode(path)),
            Cmd::SourceRename(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.rust.file_dialog = Some(crate::rust_scripts::FileDialog::Rename { path, name });
            }
            Cmd::SourceDelete(path) => self.rust.file_dialog = Some(crate::rust_scripts::FileDialog::Delete { path }),
        }
    }

    fn project_open(&mut self, root: &std::path::Path, keys: Vec<String>) {
        if keys.len() == 1 && self.project_is_folder(&keys[0]) {
            self.project.go(&keys[0]);
            return;
        }
        let mut scene_opened = false;
        for key in &keys {
            let Some(asset) = self.assets.iter().find(|a| a.path == *key).cloned() else { continue };
            // Несколько сцен сразу не открыть: берём первую.
            if asset.importer == "Scene" {
                if scene_opened {
                    continue;
                }
                scene_opened = true;
            }
            if let Some((_, action)) = default_action(&asset, root) {
                self.actions.push(action);
            }
        }
    }

    fn project_set_clipboard(&mut self, ctx: &egui::Context, keys: Vec<String>, cut: bool) {
        let text = keys.iter().map(|k| format!("Assets/{k}")).collect::<Vec<_>>().join("\n");
        // Копия в системный буфер: без непустого текста там winit не пришлёт Ctrl+V.
        ctx.copy_text(text.clone());
        self.project.clipboard = Some(Clipboard { paths: keys, cut, text });
    }

    /// Что вставит Ctrl+V: внутренний буфер, если системный остался нашим.
    pub(crate) fn project_paste_allowed(&self, pasted_text: Option<&str>) -> bool {
        match (&self.project.clipboard, pasted_text) {
            (Some(_), None) => true,
            (Some(clipboard), Some(text)) => text.replace("\r\n", "\n") == clipboard.text,
            _ => false,
        }
    }

    fn project_paste(&mut self, ctx: &egui::Context, root: &std::path::Path, into: &str) {
        let Some(clipboard) = self.project.clipboard.clone() else { return };
        let mut outcome = Outcome::default();
        for path in &clipboard.paths {
            let result = if clipboard.cut { project_fs::move_into(root, path, into) } else { project_fs::copy_into(root, path, into) };
            match result {
                Ok(new) => outcome.done.push((path.clone(), new)),
                Err(why) => outcome.failed.push((path.clone(), why)),
            }
        }
        if clipboard.cut {
            // Перенос на то же место ничего не менял: не считаем его.
            let moves: Vec<(String, String)> = outcome.done.iter().filter(|(a, b)| a != b).cloned().collect();
            if outcome.is_clean() {
                self.project.clipboard = None;
            }
            self.project_after_moves(&moves);
            let text = outcome.summary(tr_s("Moved"), clipboard.paths.len());
            let kind = if outcome.is_clean() { ToastKind::Info } else { ToastKind::Warn };
            self.project_toast(ctx, kind, text);
            let new: Vec<String> = outcome.done.into_iter().map(|(_, to)| to).collect();
            self.project_select_new(new);
        } else {
            self.project_finish_copy(ctx, &outcome, tr_s("Pasted"), clipboard.paths.len());
        }
    }

    /// Копирование закончено: один пересканирование, выделить новые, сказать итог.
    fn project_finish_copy(&mut self, ctx: &egui::Context, outcome: &Outcome, verb: &str, total: usize) {
        self.project_refresh();
        let kind = if outcome.is_clean() { ToastKind::Info } else { ToastKind::Warn };
        self.project_toast(ctx, kind, outcome.summary(verb, total));
        self.project_select_new(outcome.done.iter().map(|(_, to)| to.clone()).collect());
    }

    fn project_select_new(&mut self, new: Vec<String>) {
        let current = self.project.folder.clone();
        let here: Vec<String> = new.into_iter().filter(|p| parent_of(p) == current).collect();
        if let Some(first) = here.first().cloned() {
            self.project.select_many(here);
            self.project.scroll_to = Some(first);
        }
    }

    fn project_move(&mut self, ctx: &egui::Context, root: &std::path::Path, paths: Vec<String>, to: &str, copy: bool) {
        let mut outcome = Outcome::default();
        for path in &paths {
            let result = if copy { project_fs::copy_into(root, path, to) } else { project_fs::move_into(root, path, to) };
            match result {
                Ok(new) => outcome.done.push((path.clone(), new)),
                Err(why) => outcome.failed.push((path.clone(), why)),
            }
        }
        if copy {
            self.project_finish_copy(ctx, &outcome, tr_s("Copied"), paths.len());
            return;
        }
        let moves: Vec<(String, String)> = outcome.done.iter().filter(|(a, b)| a != b).cloned().collect();
        if moves.is_empty() && outcome.is_clean() {
            return; // всё уже лежало там
        }
        self.project_after_moves(&moves);
        let kind = if outcome.is_clean() { ToastKind::Info } else { ToastKind::Warn };
        self.project_toast(ctx, kind, outcome.summary(tr_s("Moved"), paths.len()));
        // Выделение следует за перенесённым, пока оно в открытом каталоге; иначе сбрасывается.
        let moved: Vec<String> = self.project.selection.iter().filter_map(|p| remap(p, &moves)).collect();
        self.project.selection.clear();
        self.project.selected_path = None;
        self.project_select_new(moved);
    }

    /// Удаление в корзину ОС: один вызов на всё выделение.
    fn project_trash(&mut self, ctx: &egui::Context, root: &std::path::Path, keys: Vec<String>) {
        let mut targets = Vec::new();
        let mut valid = Vec::new();
        let mut outcome = Outcome::default();
        for key in &keys {
            match project_fs::safe_join(root, key) {
                Ok(path) if path.exists() => {
                    if path.is_file() && project_fs::meta_of(&path).is_file() {
                        targets.push(project_fs::meta_of(&path));
                    }
                    targets.push(path);
                    valid.push(key.clone());
                }
                Ok(_) => outcome.failed.push((key.clone(), "it no longer exists".to_string())),
                Err(why) => outcome.failed.push((key.clone(), why)),
            }
        }
        let result = project_fs::trash(&targets);
        // Что осталось на месте — не удалено, что бы ни сказал вызов.
        for key in valid {
            let still = project_fs::safe_join(root, &key).is_ok_and(|p| p.exists());
            if still {
                let why = result.clone().err().unwrap_or_else(|| tr_s("the Recycle Bin did not take it").to_string());
                outcome.failed.push((key, why));
            } else {
                outcome.done.push((key, String::new()));
            }
        }
        let gone: Vec<String> = outcome.done.iter().map(|(p, _)| p.clone()).collect();
        self.project_after_delete(ctx, &gone, &outcome, keys.len(), tr_s("Moved to the Recycle Bin:"));
    }

    fn project_after_delete(&mut self, ctx: &egui::Context, gone: &[String], outcome: &Outcome, total: usize, verb: &str) {
        // Каталог, в котором стоим, мог исчезнуть: поднимаемся до существующего.
        let mut folder = self.project.folder.clone();
        while !folder.is_empty() && gone.iter().any(|g| folder == *g || folder.starts_with(&format!("{g}/"))) {
            folder = parent_of(&folder).to_string();
        }
        self.project.folder = folder;
        self.project.clear_selection();
        self.project.rename = None;
        self.project.confirm = None;
        for g in gone {
            self.prefs.project_ui.favorites.retain(|f| f != g && !f.starts_with(&format!("{g}/")));
        }
        if let Some(clipboard) = &mut self.project.clipboard {
            clipboard.paths.retain(|p| !gone.iter().any(|g| p == g || p.starts_with(&format!("{g}/"))));
        }
        self.project_refresh();
        let text = if outcome.is_clean() {
            outcome.done_text(verb)
        } else {
            outcome.summary(verb, total)
        };
        let kind = if outcome.is_clean() { ToastKind::Info } else { ToastKind::Error };
        self.project_toast(ctx, kind, text);
    }

    fn project_create(&mut self, ctx: &egui::Context, root: &std::path::Path, kind: Create, folder: &str) {
        match kind {
            Create::Folder => {
                let Ok(dir) = project_fs::dir_join(root, folder) else { return };
                let name = project_fs::unique_name(&dir, "New Folder", true);
                match project_fs::create_folder(root, folder, &name) {
                    Ok(path) => {
                        self.project_refresh();
                        self.project.go(folder);
                        self.project.pending_rename = Some(path);
                    }
                    Err(why) => {
                        self.project_toast(ctx, ToastKind::Error, trf!("Cannot create a folder: {why}", why = why));
                    }
                }
            }
            Create::Scene => {
                let Ok(dir) = project_fs::dir_join(root, folder) else { return };
                let name = project_fs::unique_name(&dir, "New Scene.scene", false);
                match std::fs::write(dir.join(&name), EMPTY_SCENE) {
                    Ok(()) => {
                        self.project_refresh();
                        self.project.go(folder);
                        self.project.pending_rename = Some(join(folder, &name));
                    }
                    Err(err) => {
                        self.project_toast(ctx, ToastKind::Error, trf!("Cannot create a scene in `{folder}`: {err}", folder = folder, err = err));
                    }
                }
            }
            Create::Material => self.apply(Action::CreateMaterial { folder: folder.to_string() }),
            Create::Shader(kind) => self.apply(Action::CreateShader { folder: folder.to_string(), kind }),
            Create::Model => self.apply(Action::CreateModel { folder: folder.to_string() }),
            Create::PrefabFromSelection => {
                if let Some(id) = self.selection {
                    self.apply(Action::SavePrefab(id));
                }
            }
            Create::RustScript => self.open_script_dialog(true, String::new(), self.selection),
            Create::CppScript => self.open_script_dialog(false, String::new(), self.selection),
            Create::ScenarioScript => self.open_scenario_dialog(folder, self.selection),
        }
    }

    fn project_reimport(&mut self, ctx: &egui::Context, keys: Vec<String>) {
        let mut count = 0;
        for asset in &self.assets {
            let under = keys.iter().any(|k| asset.path == *k || asset.path.starts_with(&format!("{k}/")));
            if under {
                self.project.thumbnails.remove(&asset.guid);
                count += 1;
            }
        }
        // Пересканирование перечитывает `.meta` и сбрасывает предпросмотры и кэши рендера.
        self.project_refresh();
        self.project_toast(ctx, ToastKind::Info, trf!("Reimported {count} {p0}", p0 = if count == 1 { "asset" } else { "assets" }, count = count));
    }

    /// «Find References In Scene»: выделить объект открытой сцены, который использует ассет.
    fn project_find_references(&mut self, ctx: &egui::Context, key: &str) {
        let Some(asset) = self.assets.iter().find(|a| a.path == key).cloned() else { return };
        let needle = asset.guid.to_string();
        let scene = &self.app.world.scene;
        let mut hits = Vec::new();
        for id in scene.ids() {
            let Some(object) = scene.get(id) else { continue };
            let by_prefab = asset.importer == "Prefab" && object.prefab() == Some(asset.path.as_str());
            let by_field = object.components().any(|component| {
                component.field_names().iter().any(|name| component.get_field(name).is_some_and(|v| value_refers(&v, &needle)))
            });
            if by_prefab || by_field {
                hits.push(id);
            }
        }
        let name = file_name(key).to_string();
        match hits.first().copied() {
            Some(first) => {
                self.selection = Some(first);
                let text = if hits.len() == 1 {
                    trf!("`{name}` is used by 1 object in the open scene; it is selected", name = name)
                } else {
                    trn!("`{name}` is used by {n} object in the open scene; the first one is selected", "`{name}` is used by {n} objects in the open scene; the first one is selected", hits.len(), name = name)
                };
                self.project_toast(ctx, ToastKind::Info, text);
            }
            None => {
                self.project_toast(ctx, ToastKind::Info, trf!("No object in the open scene uses `{name}`", name = name));
            }
        }
    }

    fn project_import(&mut self, ctx: &egui::Context, root: &std::path::Path, dest: String, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        if self.project.import.is_some() {
            self.project_toast(ctx, ToastKind::Warn, tr_s("Another import is still running; drop the files again when it finishes"));
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (root, to) = (root.to_path_buf(), dest.clone());
        let progress_tx = tx.clone();
        std::thread::spawn(move || {
            let outcome = project_fs::import_external(&root, &to, &files, &mut |done, total| {
                let _ = progress_tx.send(ImportMsg::Progress(done, total));
            });
            let _ = tx.send(ImportMsg::Done(outcome));
        });
        let toast = self.project_toast(ctx, ToastKind::Info, trf!("Importing into Assets/{dest}…", dest = dest));
        if let Some(t) = self.project.toasts.iter_mut().find(|t| t.id == toast) {
            t.progress = Some((0, 0));
        }
        self.project.import = Some(ImportJob { rx, dest, toast });
    }

    /// Читать сообщения потока импорта; по окончании пересканировать один раз.
    pub(crate) fn project_poll_import(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.project.import else { return };
        let mut finished = None;
        let mut progress = None;
        while let Ok(message) = job.rx.try_recv() {
            match message {
                ImportMsg::Progress(done, total) => progress = Some((done, total)),
                ImportMsg::Done(outcome) => finished = Some(outcome),
            }
        }
        let (toast, dest) = (job.toast, job.dest.clone());
        if let Some(progress) = progress {
            if let Some(t) = self.project.toasts.iter_mut().find(|t| t.id == toast) {
                t.progress = Some(progress);
            }
        }
        ctx.request_repaint();
        let Some(outcome) = finished else { return };
        self.project.import = None;
        self.project.toasts.retain(|t| t.id != toast);
        self.project_refresh();
        let total = outcome.done.len() + outcome.failed.len();
        let kind = if outcome.is_clean() { ToastKind::Info } else { ToastKind::Warn };
        self.project_toast(ctx, kind, outcome.summary(tr_s("Imported"), total));
        let new: Vec<String> = outcome.done.into_iter().map(|(_, to)| to).collect();
        if self.project.folder != dest {
            self.project.go(&dest);
        }
        self.project_select_new(new);
    }

    /// Сводка по выделению из нескольких элементов; считается один раз на выделение.
    pub(crate) fn project_update_summary(&mut self) {
        if self.project.selection.len() < 2 {
            self.project.summary = None;
            return;
        }
        let key = format!("{}|{}", self.project.rev, self.project.selection.iter().cloned().collect::<Vec<_>>().join("\u{1}"));
        if self.project.summary.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let Some(root) = self.assets_root() else { return };
        let mut summary = crate::project::Summary::default();
        let mut by_type: std::collections::BTreeMap<&'static str, usize> = Default::default();
        for path in &self.project.selection {
            if self.project_is_folder(path) {
                summary.folders += 1;
                let prefix = format!("{path}/");
                for asset in self.assets.iter().filter(|a| a.path.starts_with(&prefix)) {
                    summary.files += 1;
                    summary.bytes += std::fs::metadata(root.join(&asset.path)).map_or(0, |m| m.len());
                }
            } else if let Some(asset) = self.assets.iter().find(|a| a.path == *path) {
                summary.files += 1;
                summary.bytes += std::fs::metadata(root.join(&asset.path)).map_or(0, |m| m.len());
                *by_type.entry(crate::sorting::type_label(&asset.importer, &asset.path)).or_default() += 1;
            }
        }
        if summary.folders > 0 {
            by_type.insert("Folder", summary.folders);
        }
        summary.by_type = by_type.into_iter().collect();
        self.project.summary = Some((key, summary));
    }
}
