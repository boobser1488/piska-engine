//! Окно Project — файловый менеджер ассетов: дерево каталогов с избранным слева, содержимое
//! каталога (плитки или список со столбцами) в центре, сведения о выбранном справа.
//! Рисуют `project_tree.rs`, `project_content.rs`, `project_chrome.rs`; операции над
//! файлами — `project_ops.rs` (поверх `project_fs.rs`); навигация и разбор — `project_nav.rs`.
//!
//! Пути показываются относительно проекта (`Assets/prefabs/coin.prefab`): полный
//! путь к каталогу ассетов на диске пользователю не нужен.

use crate::i18n::{tr_s, trf, trn};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine_assets::Guid;

use crate::hierarchy::drag_label;
use crate::icons::{self, Icon};
use crate::project_nav::{History, TypeAhead, CODE_LOC};
use crate::project_ops::{Clipboard, Cmd, ImportJob, Toast};
use crate::state::{file_name, Action, AssetEntry, Editor};
use crate::theme;

/// Что тащат мышью из окна Project. `path` — тот элемент, за который взялись;
/// `paths` — всё перетаскиваемое (выделение целиком, если тащат выделенный элемент).
pub(crate) struct DragAsset {
    pub guid: Guid,
    pub importer: String,
    pub path: String,
    pub paths: Vec<String>,
}

/// Что показано в центре: каталог ассетов или исходники игры.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Assets,
    Code,
}

/// Переименование на месте.
pub(crate) struct Rename {
    pub path: String,
    pub text: String,
    /// Первый кадр: взять фокус и выделить имя без расширения.
    pub fresh: bool,
}

/// Резинка выделения в пустом месте.
pub(crate) struct Band {
    /// Где нажали, в координатах содержимого (не зависят от прокрутки).
    pub origin: egui::Pos2,
    /// Выделение, к которому резинка добавляется (при Ctrl).
    pub base: BTreeSet<String>,
}

/// Подтверждение удаления насовсем (нет корзины).
pub(crate) struct Confirm {
    pub paths: Vec<String>,
}

/// Сводка по выделению из нескольких элементов.
#[derive(Default, Clone)]
pub(crate) struct Summary {
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    pub by_type: Vec<(&'static str, usize)>,
}

pub(crate) struct ProjectState {
    /// Активный (последний выбранный) элемент — путь относительно каталога ассетов.
    pub selected_path: Option<String>,
    /// Всё выделенное (пути каталогов и ассетов).
    pub selection: BTreeSet<String>,
    /// Откуда тянется диапазон при Shift.
    pub anchor: Option<String>,
    /// Что было активным в прошлом кадре: чужие правки `selected_path` подхватываются.
    last_active: Option<String>,
    /// Открытый каталог (относительно каталога ассетов, `""` — корень).
    pub folder: String,
    pub place: Place,
    pub filter: String,
    /// Свёрнутые каталоги дерева.
    pub(crate) collapsed: BTreeSet<String>,
    /// Миниатюры текстур: GUID → текстура egui (`None` — не удалось прочитать).
    pub(crate) thumbnails: HashMap<Guid, Option<egui::TextureHandle>>,
    /// Сколько ассетов было при прошлом кадре: при пересканировании миниатюры сбрасываются.
    generation: usize,
    /// Где нарисованы плитки (строки) ассетов в последнем кадре — для автоматизации и тестов.
    pub(crate) entry_rects: HashMap<String, Rect>,
    /// Где нарисован большой предпросмотр модели справа.
    pub(crate) preview_rect: Option<Rect>,
    /// Размеры и время правки файлов для сортировки и столбцов списка.
    pub(crate) meta: crate::sorting::MetaCache,
    /// Имена элементов в порядке, в каком они нарисованы в последнем кадре (для тестов).
    pub(crate) shown: Vec<String>,
    /// Заголовки групп окна Code («Rust  ·  2») в последнем кадре.
    pub(crate) headings: Vec<String>,

    // --- файловый менеджер ---
    pub(crate) history: History,
    /// Куда указывало окно в прошлом кадре (`""`, путь каталога или [`CODE_LOC`]).
    last_loc: Option<String>,
    pub(crate) clipboard: Option<Clipboard>,
    pub(crate) rename: Option<Rename>,
    /// Новый элемент, который надо выделить и сразу переименовать.
    pub(crate) pending_rename: Option<String>,
    /// Строка пути, пока её правят; `None` — показаны хлебные крошки.
    pub(crate) path_edit: Option<String>,
    pub(crate) path_error: Option<String>,
    pub(crate) typeahead: TypeAhead,
    pub(crate) toasts: Vec<Toast>,
    pub(crate) import: Option<ImportJob>,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) browser: Option<crate::project_chrome::Browser>,
    /// Кэш списка: ключ и сам список. Список пересобирается только при смене ключа.
    pub(crate) listing: Option<Rc<crate::project_content::Listing>>,
    listing_key: String,
    /// Версия списка ассетов: растёт при каждом пересканировании.
    pub(crate) rev: u64,
    /// Последний щелчок по элементу — для «медленного второго щелчка» (переименование).
    pub(crate) last_click: Option<(String, f64)>,
    pub(crate) band: Option<Band>,
    /// Прокрутить к элементу после стрелок.
    pub(crate) scroll_to: Option<String>,
    /// Что перетаскивается сейчас (для подсветки допустимых целей).
    pub(crate) dragging: Vec<String>,
    /// Окно Project под курсором в последнем кадре: ему принадлежат клавиши.
    pub(crate) focus: bool,
    pub(crate) focus_frame: u64,
    pub(crate) panel_rect: Option<Rect>,
    pub(crate) focus_search: bool,
    pub(crate) search_id: Option<egui::Id>,
    pub(crate) summary: Option<(String, Summary)>,
    /// Команды, накопленные за кадр: выполняются в конце `show`.
    pub(crate) cmds: Vec<Cmd>,
    /// Сколько элементов в последнем кадре (строка состояния).
    pub(crate) last_total: usize,
    /// «Favorites» уже решили, свёрнуты ли они (в низком окне при первом показе — свёрнуты).
    pub(crate) favorites_init: bool,
    /// Раскладка последнего кадра: по ней стрелки ищут соседа.
    pub(crate) layout: Option<Rc<crate::project_content::Layout>>,
    /// Сколько строк видно в окне: шаг PageUp / PageDown.
    pub(crate) page_rows: usize,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            selected_path: None,
            selection: BTreeSet::new(),
            anchor: None,
            last_active: None,
            folder: String::new(),
            place: Place::Assets,
            filter: String::new(),
            collapsed: BTreeSet::new(),
            thumbnails: HashMap::new(),
            generation: usize::MAX,
            entry_rects: HashMap::new(),
            preview_rect: None,
            meta: crate::sorting::MetaCache::default(),
            shown: Vec::new(),
            headings: Vec::new(),
            history: History::default(),
            last_loc: None,
            clipboard: None,
            rename: None,
            pending_rename: None,
            path_edit: None,
            path_error: None,
            typeahead: TypeAhead::default(),
            toasts: Vec::new(),
            import: None,
            confirm: None,
            browser: None,
            listing: None,
            listing_key: String::new(),
            rev: 0,
            last_click: None,
            band: None,
            scroll_to: None,
            dragging: Vec::new(),
            focus: false,
            focus_frame: 0,
            panel_rect: None,
            focus_search: false,
            search_id: None,
            summary: None,
            cmds: Vec::new(),
            last_total: 0,
            layout: None,
            page_rows: 8,
            favorites_init: false,
        }
    }
}

impl ProjectState {
    /// Открыть каталог ассета и выделить его.
    pub fn reveal(&mut self, relative: &str) {
        self.place = Place::Assets;
        self.folder = relative.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_string());
        self.selected_path = Some(relative.to_string());
        self.filter.clear();
        let mut path = String::new();
        for part in self.folder.split('/').filter(|p| !p.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            self.collapsed.remove(&path);
        }
    }

    /// Выделить ровно один элемент.
    pub(crate) fn select_only(&mut self, path: &str) {
        self.selection.clear();
        self.selection.insert(path.to_string());
        self.anchor = Some(path.to_string());
        self.selected_path = Some(path.to_string());
        self.last_active = self.selected_path.clone();
    }

    pub(crate) fn select_many(&mut self, paths: impl IntoIterator<Item = String>) {
        self.selection = paths.into_iter().collect();
        self.selected_path = self.selection.iter().next_back().cloned();
        self.anchor = self.selected_path.clone();
        self.last_active = self.selected_path.clone();
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection.clear();
        self.anchor = None;
        self.selected_path = None;
        self.last_active = None;
    }

    /// Где сейчас окно: путь каталога, пусто (корень) или [`CODE_LOC`].
    pub(crate) fn location(&self) -> String {
        if self.place == Place::Code {
            CODE_LOC.to_string()
        } else {
            self.folder.clone()
        }
    }

    /// Перейти в каталог (или в окно Code) без записи в историю — её ведёт `begin_frame`.
    pub(crate) fn go(&mut self, location: &str) {
        if location == CODE_LOC {
            self.place = Place::Code;
        } else {
            self.place = Place::Assets;
            self.folder = location.to_string();
        }
        self.filter.clear();
        let mut path = String::new();
        for part in self.folder.split('/').filter(|p| !p.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            self.collapsed.remove(&path);
        }
    }

    /// Начало кадра: подхватить чужие правки выбора, записать переход в историю.
    pub(crate) fn begin_frame(&mut self) {
        if self.selected_path != self.last_active {
            // Кто-то снаружи (диалог, `select_asset`) задал выбор: он главнее.
            self.selection.clear();
            if let Some(path) = &self.selected_path {
                self.selection.insert(path.clone());
            }
            self.anchor = self.selected_path.clone();
            self.last_active = self.selected_path.clone();
        } else if let Some(path) = self.selected_path.clone() {
            if !self.selection.contains(&path) {
                self.selection.insert(path);
            }
        }
        let location = self.location();
        if self.last_loc.as_deref() != Some(location.as_str()) {
            self.history.visit(&location);
            self.last_loc = Some(location);
            // Выделение остаётся только у элементов нового каталога.
            let folder = self.folder.clone();
            self.selection.retain(|p| crate::project_fs::parent_of(p) == folder);
            if self.selected_path.as_ref().is_some_and(|p| !self.selection.contains(p)) {
                self.selected_path = self.selection.iter().next_back().cloned();
                self.last_active = self.selected_path.clone();
            }
            self.rename = None;
            self.path_edit = None;
            self.path_error = None;
        }
    }

    /// Принадлежат ли клавиши окну Project: курсор над ним и оно рисовалось в последних кадрах.
    pub(crate) fn keys_owned(&self, ctx: &egui::Context) -> bool {
        self.focus && ctx.cumulative_pass_nr() <= self.focus_frame + 3
    }

    /// Открыто окно Project поверх редактора (браузер файлов, вопрос об удалении): клавиши, в
    /// том числе Esc, принадлежат ему.
    pub(crate) fn modal_open(&self) -> bool {
        self.browser.is_some() || self.confirm.is_some()
    }

    pub(crate) fn sync_active(&mut self) {
        self.last_active = self.selected_path.clone();
    }

    pub(crate) fn listing_key_matches(&self, key: &str) -> bool {
        self.listing_key == key
    }

    pub(crate) fn set_listing(&mut self, key: String, listing: Rc<crate::project_content::Listing>) {
        self.listing_key = key;
        self.listing = Some(listing);
    }

    /// Пометить список ассетов изменившимся (после пересканирования).
    pub(crate) fn invalidate(&mut self) {
        self.rev += 1;
        self.thumbnails.clear();
        self.meta.clear();
        self.summary = None;
    }
}

/// Иконка и цвет ассета по импортёру.
pub(crate) fn asset_icon(importer: &str) -> (Icon, Color32) {
    match importer {
        "Texture" => (Icon::Image, theme::OK),
        "Prefab" => (Icon::Prefab, theme::PREFAB),
        "Scene" => (Icon::Scene, theme::accent()),
        "Audio" => (Icon::Audio, Color32::from_rgb(0xe0, 0x8f, 0xd0)),
        "Mesh" => (Icon::Cube, Color32::from_rgb(0xc8, 0xa0, 0x6e)),
        "Material" => (Icon::Sphere, Color32::from_rgb(0xe8, 0x7a, 0x5d)),
        "Font" => (Icon::Text, theme::TEXT),
        "Code" => (Icon::Script, Color32::from_rgb(0x9b, 0x7b, 0xff)),
        "Scenario" => (Icon::Script, Color32::from_rgb(0x4f, 0xd1, 0xc5)),
        "Text" => (Icon::Text, theme::TEXT_MUTED),
        "Shader" => (Icon::Code, Color32::from_rgb(0xf0, 0xb4, 0x4c)),
        _ => (Icon::File, theme::TEXT_MUTED),
    }
}

/// Что сделает двойной щелчок по ассету.
pub(crate) fn default_action(asset: &AssetEntry, root: &std::path::Path) -> Option<(&'static str, Action)> {
    match asset.importer.as_str() {
        "Scene" => Some((tr_s("Open Scene"), Action::OpenScene(asset.path.clone()))),
        "Prefab" => Some((
            tr_s("Add to Scene"),
            Action::Instantiate {
                reference: asset.path.clone(),
                position: None,
            },
        )),
        "Texture" => Some((
            tr_s("Create Sprite"),
            Action::CreateSpriteFromTexture {
                guid: asset.guid,
                position: None,
            },
        )),
        "Code" | "Text" | "Scenario" | "Shader" => Some((tr_s("Open in Code Tab"), Action::OpenCode(root.join(&asset.path)))),
        "Mesh" if asset.path.ends_with(".pmodel") => Some((tr_s("Open in Piska Modeler"), Action::OpenModel(asset.path.clone()))),
        _ => None,
    }
}

pub(crate) fn asset_row_id(guid: Guid) -> egui::Id {
    egui::Id::new(("project_asset", guid.0))
}

/// Строка каталога в дереве слева.
pub(crate) fn tree_row_id(folder: &str) -> egui::Id {
    egui::Id::new(("project_folder", folder))
}

/// Плитка или строка каталога в содержимом справа.
pub(crate) fn folder_entry_id(folder: &str) -> egui::Id {
    egui::Id::new(("project_entry_folder", folder))
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    let Some(root) = editor.assets_root() else {
        theme::empty_state(
            ui,
            Icon::Folder,
            tr_s("No assets directory is mounted"),
            tr_s("Call `engine::mount_assets(&mut app, \"assets\")` before starting the editor"),
        );
        return;
    };
    let ctx = ui.ctx().clone();
    // Миниатюры принадлежат контексту egui: новый контекст (снимок без окна) — новые текстуры.
    let marker = egui::Id::new("project_thumbnails_ctx");
    if ctx.data(|d| d.get_temp::<bool>(marker)).is_none() {
        ctx.data_mut(|d| d.insert_temp(marker, true));
        editor.project.thumbnails.clear();
    }
    if editor.project.generation != editor.assets.len() {
        editor.project.generation = editor.assets.len();
        editor.project.thumbnails.clear();
        editor.project.meta.clear();
    }

    // Кому принадлежат клавиши: окну, над которым курсор (как в Unity).
    let panel = ui.max_rect();
    editor.project.panel_rect = Some(panel);
    editor.project.focus = ctx.pointer_latest_pos().is_some_and(|p| panel.contains(p));
    editor.project.focus_frame = ctx.cumulative_pass_nr();
    editor.project.begin_frame();
    editor.project_poll_import(&ctx);
    editor.project_update_summary();

    crate::project_chrome::top_bar(editor, ui);
    let in_code = editor.project.place == Place::Code;
    crate::project_bar::filter_bar(editor, ui, !in_code);
    crate::project_chrome::status_line(editor, ui);

    let wide = ui.available_width();
    let compact = wide < 460.0;
    if !compact {
        crate::project_tree::tree_panel(editor, ui, wide);
    }
    // Сведения: справа, а в узком окне — снизу.
    let details_open = editor.prefs.project_ui.details;
    let below = wide < 640.0;
    if details_open && !below {
        let details_width = 290.0_f32.min(ui.available_width() * 0.35);
        if ui.available_width() > 360.0 {
            egui::SidePanel::right("project_details")
                .resizable(false)
                .exact_width(details_width)
                .frame(egui::Frame::none().fill(theme::BG_HEADER).inner_margin(egui::Margin::same(theme::SP_L)))
                .show_inside(ui, |ui| details(editor, ui, &root));
        }
    } else if details_open && ui.available_height() > 230.0 {
        egui::TopBottomPanel::bottom("project_details_below")
            .resizable(true)
            .default_height(130.0)
            .height_range(80.0..=(ui.available_height() * 0.6).max(100.0))
            .frame(egui::Frame::none().fill(theme::BG_HEADER).inner_margin(egui::Margin::same(theme::SP_M)))
            .show_inside(ui, |ui| details(editor, ui, &root));
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(theme::BG_INSET).inner_margin(egui::Margin::same(theme::SP_M)))
        .show_inside(ui, |ui| crate::project_content::content(editor, ui, &root));

    crate::project_chrome::overlays(editor, ui);

    if let Some(payload) = egui::DragAndDrop::payload::<DragAsset>(&ctx) {
        if let Some(position) = ctx.pointer_latest_pos() {
            let name = file_name(&payload.path);
            let label = if payload.paths.len() > 1 { format!("{name}  (+{})", payload.paths.len() - 1) } else { name.to_string() };
            let icon = if payload.importer == "Folder" { Icon::Folder } else { asset_icon(&payload.importer).0 };
            drag_label(&ctx, position, icon, &label);
        }
    } else {
        editor.project.dragging.clear();
    }

    // Команды этого кадра — после того, как все панели закончили рисовать.
    let cmds = std::mem::take(&mut editor.project.cmds);
    for cmd in cmds {
        editor.project_run(&ctx, cmd);
    }
}

/// Картинка модели со скруглёнными углами.
pub(crate) fn paint_model(ui: &egui::Ui, texture: egui::TextureId, rect: Rect, rounding: f32) {
    egui::Image::new(egui::load::SizedTexture::new(texture, rect.size()))
        .rounding(rounding)
        .paint_at(ui, rect);
}

/// Миниатюра текстуры: файл → уменьшенная копия → текстура egui.
pub(crate) fn load_thumbnail(ctx: &egui::Context, file: &std::path::Path, name: &str) -> Option<egui::TextureHandle> {
    let image = image::open(file).ok()?.thumbnail(128, 128).to_rgba8();
    // Вписать в квадрат с сохранением пропорций: края прозрачные.
    let side = image.width().max(image.height()) as usize;
    let mut pixels = vec![Color32::TRANSPARENT; side * side];
    let (ox, oy) = ((side - image.width() as usize) / 2, (side - image.height() as usize) / 2);
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        pixels[(y as usize + oy) * side + x as usize + ox] = Color32::from_rgba_unmultiplied(r, g, b, a);
    }
    let color_image = egui::ColorImage { size: [side, side], pixels };
    let options = if side <= 64 { egui::TextureOptions::NEAREST } else { egui::TextureOptions::LINEAR };
    Some(ctx.load_texture(format!("thumb:{name}", name = name), color_image, options))
}

/// Сводка по нескольким выделенным элементам: сколько, какого размера, каких типов.
fn multi_details(editor: &mut Editor, ui: &mut egui::Ui) {
    let count = editor.project.selection.len();
    let summary = editor.project.summary.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        icons::inline(ui, Icon::Copy, theme::TEXT_MUTED);
        ui.label(egui::RichText::new(trn!("{n} item selected", "{n} items selected", count)).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
    });
    ui.add_space(theme::SP_M);
    let row = |ui: &mut egui::Ui, key: &str, value: String| {
        ui.label(egui::RichText::new(key).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
        ui.label(egui::RichText::new(value).color(theme::TEXT));
        ui.add_space(theme::SP_S);
    };
    row(ui, tr_s("Files"), summary.files.to_string());
    if summary.folders > 0 {
        row(ui, tr_s("Folders"), summary.folders.to_string());
    }
    row(ui, tr_s("Total size"), crate::sorting::size_label(summary.bytes));
    ui.label(egui::RichText::new(tr_s("By type")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
    for (kind, n) in &summary.by_type {
        ui.label(egui::RichText::new(format!("{kind}: {n}")).color(theme::TEXT));
    }
    ui.add_space(theme::SP_L);
    ui.label(
        egui::RichText::new(tr_s("Drag onto a folder to move, hold Ctrl to copy. Ctrl+C / Ctrl+X / Delete work on the whole selection."))
            .size(theme::FONT_CAPTION)
            .color(theme::TEXT_MUTED),
    );
}

/// Панель подробностей окна Project. Прокручивается целиком: в низком окне текст не
/// обрезается краем панели.
fn details(editor: &mut Editor, ui: &mut egui::Ui, root: &std::path::Path) {
    egui::ScrollArea::vertical()
        .id_salt("project_details_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            details_body(editor, ui, root);
        });
}

fn details_body(editor: &mut Editor, ui: &mut egui::Ui, root: &std::path::Path) {
    if editor.project.selection.len() > 1 {
        editor.project.preview_rect = None;
        multi_details(editor, ui);
        return;
    }
    let selected = editor
        .project
        .selected_path
        .as_deref()
        .and_then(|path| editor.assets.iter().find(|asset| asset.path == path))
        .cloned();
    let Some(asset) = selected else {
        if let Some(folder) = editor.project.selected_path.clone().filter(|p| editor.folders.contains(p)) {
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Folder, theme::WARN);
                ui.label(egui::RichText::new(folder.rsplit('/').next().unwrap_or(&folder)).font(theme::strong(theme::FONT_TITLE)));
            });
            ui.add_space(theme::SP_S);
            ui.label(egui::RichText::new(format!("Assets/{folder}")).color(theme::TEXT_MUTED));
            let prefix = format!("{folder}/");
            let files = editor.assets.iter().filter(|a| a.path.starts_with(&prefix)).count();
            let folders = editor.folders.iter().filter(|f| f.starts_with(&prefix)).count();
            ui.add_space(theme::SP_S);
            ui.label(egui::RichText::new(trf!("{files}, {folders} inside", files = trn!("{n} file", "{n} files", files), folders = trn!("{n} folder", "{n} folders", folders))).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
            editor.project.preview_rect = None;
            return;
        }
        ui.add_space(theme::SP_M);
        ui.label(egui::RichText::new(tr_s("No asset selected")).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT));
        ui.add_space(theme::SP_S);
        ui.label(
            egui::RichText::new(
                tr_s("Double-click a scene to open it, a prefab or texture to add it to the scene. \
                 Drag assets into the Scene view or onto Inspector fields."),
            )
            .color(theme::TEXT_MUTED),
        );
        return;
    };
    {
        {
            let (icon, color) = asset_icon(&asset.importer);
            let is_model = asset.importer == "Mesh" || asset.importer == "Material";
            if is_model {
                model_preview(editor, ui, &asset);
                if asset.importer == "Material" {
                    ui.add_space(theme::SP_S);
                    crate::material_editor::shape_switch(&mut editor.previews, ui);
                }
                ui.add_space(theme::SP_M);
            } else {
                editor.project.preview_rect = None;
            }
            let preview = editor.project.thumbnails.get(&asset.guid).and_then(|t| t.as_ref().map(|t| t.id()));
            ui.horizontal(|ui| {
                if !is_model {
                    let (square, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                    ui.painter().rect_filled(square, theme::R_M, theme::BG_INSET);
                    match preview {
                        Some(texture) => {
                            ui.painter().image(texture, square.shrink(4.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => icons::paint(ui.painter(), square.shrink(18.0), icon, color),
                    }
                }
                ui.vertical(|ui| {
                    ui.add_space(if is_model { 0.0 } else { theme::SP_M });
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(file_name(&asset.path)).font(theme::strong(theme::FONT_TITLE)).color(theme::TEXT),
                        )
                        .truncate(),
                    );
                    ui.label(egui::RichText::new(&asset.importer).color(theme::TEXT_MUTED));
                });
            });
            ui.add_space(theme::SP_M);
            let detail = |ui: &mut egui::Ui, key: &str, value: egui::RichText| {
                ui.label(egui::RichText::new(key).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
                let response = ui.add(egui::Label::new(value).truncate());
                ui.add_space(theme::SP_S);
                response
            };
            if asset.importer == "Mesh" {
                model_facts(editor, ui, &asset, root);
            }
            detail(ui, tr_s("Path"), egui::RichText::new(format!("Assets/{}", asset.path)));
            detail(ui, tr_s("Type"), egui::RichText::new(crate::sorting::type_label(&asset.importer, &asset.path)));
            if let Ok(modified) = std::fs::metadata(root.join(&asset.path)).and_then(|m| m.modified()) {
                detail(ui, tr_s("Modified"), egui::RichText::new(crate::sorting::date_label(modified)));
            }
            if let (false, Ok(meta)) = (is_model, std::fs::metadata(root.join(&asset.path))) {
                detail(ui, tr_s("Size"), egui::RichText::new(human_size(meta.len())));
            }
            detail(ui, "GUID", egui::RichText::new(asset.guid.to_string()).font(egui::FontId::monospace(12.0)))
                .on_hover_text(tr_s("Scenes reference this asset by its GUID: renaming or moving the file keeps the link"));
            ui.add_space(theme::SP_L);
            ui.horizontal_wrapped(|ui| {
                if let Some((label, action)) = default_action(&asset, root) {
                    if theme::primary_button(ui, label).clicked() {
                        editor.actions.push(action);
                    }
                }
                if matches!(asset.importer.as_str(), "Scene" | "Prefab") && theme::secondary_button(ui, tr_s("Edit as Text")).clicked() {
                    editor.actions.push(Action::OpenCode(root.join(&asset.path)));
                }
            });
        }
    }
}

/// Большой предпросмотр модели: тянуть мышью — вращать, колесо — приблизить, двойной
/// щелчок — вернуть вид. Пока пользователь не коснулся, модель медленно вращается.
fn model_preview(editor: &mut Editor, ui: &mut egui::Ui, asset: &AssetEntry) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let width = ui.available_width();
    // В низком окне Project вид ниже, чтобы цифры под ним не уезжали за край.
    let height = (width * 0.84).min((ui.available_height() - 60.0).max(110.0));
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    editor.project.preview_rect = Some(rect);
    editor.previews.select(Some(asset.guid));
    let detail = &mut editor.previews.detail;
    if response.dragged() {
        let delta = response.drag_delta();
        detail.orbit.yaw -= delta.x * 0.012;
        detail.orbit.pitch = (detail.orbit.pitch + delta.y * 0.012).clamp(-1.45, 1.45);
        detail.auto = false;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            detail.orbit.zoom = (detail.orbit.zoom * (scroll * 0.0015).exp()).clamp(0.3, 6.0);
            detail.auto = false;
            // Колесо над видом приближает его, а не прокручивает панель.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta = vec2(0.0, 0.0));
        }
    }
    if response.double_clicked() {
        detail.orbit = crate::preview::ModelOrbit::default();
        detail.auto = true;
    } else if detail.auto {
        detail.orbit.yaw += ui.input(|i| i.stable_dt).min(0.1) * 0.35;
        // Вращение не требует 60 кадров в секунду.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    let round = |value: f32| ((value * pixels_per_point / 16.0).ceil() * 16.0).clamp(64.0, 1024.0) as u32;
    editor.previews.request_detail(asset.guid, [round(rect.width()), round(rect.height())]);

    let detail = &editor.previews.detail;
    ui.painter().rect_filled(rect, theme::R_M, theme::BG_INSET);
    match (detail.texture, &detail.failed) {
        (Some(texture), None) => {
            paint_model(ui, texture, rect, theme::R_M);
            if response.hovered() {
                ui.painter().text(
                    pos2(rect.center().x, rect.bottom() - 8.0),
                    Align2::CENTER_BOTTOM,
                    tr_s("Drag to rotate · wheel to zoom"),
                    theme::caption(),
                    theme::TEXT_DISABLED,
                );
            }
        }
        (_, Some(reason)) => {
            icons::paint(ui.painter(), Rect::from_center_size(rect.center() - vec2(0.0, 12.0), vec2(36.0, 36.0)), Icon::Cube, theme::ERROR);
            ui.painter().text(
                rect.center() + vec2(0.0, 22.0),
                Align2::CENTER_TOP,
                tr_s("Cannot preview this model"),
                theme::body(),
                theme::TEXT,
            );
            response.clone().on_hover_text(reason.clone());
        }
        (None, None) => {
            icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(36.0, 36.0)), Icon::Cube, theme::TEXT_DISABLED);
        }
    }
}

/// Цифры модели под предпросмотром: вершины, треугольники, части, размеры, материалы.
fn model_facts(editor: &mut Editor, ui: &mut egui::Ui, asset: &AssetEntry, root: &std::path::Path) {
    let Some(database) = editor.app.world.resources.get::<engine_assets::AssetDatabase>() else {
        return;
    };
    let info = editor.previews.info(asset.guid, database).clone();
    let file_size = std::fs::metadata(root.join(&asset.path)).map(|m| human_size(m.len())).ok();
    let fact = |ui: &mut egui::Ui, key: &str, value: String| {
        ui.label(egui::RichText::new(key).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
        ui.label(egui::RichText::new(value).color(theme::TEXT));
        ui.end_row();
    };
    match info {
        Ok(info) => {
            egui::Grid::new("model_facts").num_columns(2).spacing([theme::SP_L, theme::SP_S]).show(ui, |ui| {
                fact(ui, tr_s("Vertices"), thousands(info.vertices));
                fact(ui, tr_s("Triangles"), thousands(info.triangles));
                fact(ui, tr_s("Sub-meshes"), info.submeshes.to_string());
                let size = info.bounds.1 - info.bounds.0;
                fact(ui, tr_s("Bounds"), format!("{:.2} × {:.2} × {:.2}", size.x, size.y, size.z));
                if let Some(size) = file_size {
                    fact(ui, tr_s("File size"), size);
                }
            });
            ui.add_space(theme::SP_S);
            ui.label(egui::RichText::new(tr_s("Materials")).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
            if info.materials.is_empty() {
                ui.label(egui::RichText::new(tr_s("None in the file: default grey")).color(theme::TEXT_DISABLED));
            }
            for material in &info.materials {
                ui.horizontal(|ui| {
                    let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let color = Color32::from_rgb(channel(material.color[0]), channel(material.color[1]), channel(material.color[2]));
                    ui.painter().rect_filled(swatch, 2.0, color);
                    ui.painter().rect_stroke(swatch, 2.0, Stroke::new(1.0_f32, theme::TEXT_DISABLED));
                    ui.add(egui::Label::new(egui::RichText::new(&material.name).color(theme::TEXT)).truncate());
                });
                if let Some(texture) = &material.texture {
                    ui.label(egui::RichText::new(format!("    texture: {texture}", texture = texture)).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
                }
            }
        }
        Err(reason) => {
            ui.label(egui::RichText::new(tr_s("This model cannot be read")).color(theme::ERROR));
            ui.label(egui::RichText::new(reason).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
            if let Some(size) = file_size {
                ui.label(egui::RichText::new(trf!("File size: {size}", size = size)).size(theme::FONT_CAPTION).color(theme::TEXT_MUTED));
            }
        }
    }
    ui.add_space(theme::SP_M);
}

/// `1234567` → `1 234 567`.
fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => trf!("{b} bytes", b = b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_opens_the_folder_of_the_asset() {
        let mut project = ProjectState::default();
        project.reveal("prefabs/coin.prefab");
        assert_eq!(project.folder, "prefabs");
        assert_eq!(project.selected_path.as_deref(), Some("prefabs/coin.prefab"));
        project.reveal("root.png");
        assert_eq!(project.folder, "");
        assert_eq!(human_size(2048), "2.0 KB");
    }
}

