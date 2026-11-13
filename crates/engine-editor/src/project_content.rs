//! Содержимое каталога в окне Project: список элементов (кэшируется), раскладка плитками
//! или строками, рисование только видимых элементов (тысячи файлов не тормозят), выделение
//! щелчками и резинкой, перетаскивание, переименование на месте и клавиатура.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use egui::{pos2, vec2, Align2, Color32, Modifiers, PointerButton, Rect, Sense, Stroke};
use engine_assets::Guid;

use crate::icons::{self, Icon};
use crate::preview::Thumb;
use crate::project::{asset_icon, asset_row_id, folder_entry_id, load_thumbnail, paint_model, DragAsset, Place};
use crate::project_fs::{file_name, parent_of, validate_name};
use crate::project_menus::{self, MenuCx};
use crate::project_nav::{click_select, neighbour, Dir, Query, Scope};
use crate::project_ops::Cmd;
use crate::sorting::{ItemInfo, Language, SortKey, TypeFilter};
use crate::state::{Action, AssetEntry, Editor};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Folder,
    Asset,
    /// Исходник игры из каталогов кода (вне ассетов): открывается, но не выделяется.
    Source,
}

/// Элемент содержимого каталога.
#[derive(Clone)]
pub(crate) struct Item {
    pub kind: Kind,
    /// Путь относительно каталога ассетов; у исходника — `\u{2}имя`.
    pub key: String,
    pub name: String,
    pub guid: Guid,
    pub importer: String,
    pub type_label: &'static str,
    pub disk: PathBuf,
    /// Результаты поиска: каталог, в котором лежит файл.
    pub dir: String,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

impl Item {
    pub fn selectable(&self) -> bool {
        self.kind != Kind::Source
    }

    fn id(&self) -> egui::Id {
        match self.kind {
            Kind::Folder => folder_entry_id(&self.key),
            Kind::Asset => asset_row_id(self.guid),
            Kind::Source => egui::Id::new(("project_source", &self.key)),
        }
    }
}

pub(crate) struct Group {
    pub heading: Option<String>,
    pub range: Range<usize>,
}

/// Всё, что показано в центре, в порядке показа.
pub(crate) struct Listing {
    pub items: Vec<Item>,
    pub keys: Vec<String>,
    pub groups: Vec<Group>,
    pub searching: bool,
    pub unknown_types: Vec<String>,
}

impl Editor {
    /// Список элементов окна. Пересобирается, только когда меняются каталог, поиск, фильтры,
    /// сортировка или сами ассеты: при тысячах файлов это экономит кадр.
    pub(crate) fn project_listing(&mut self, root: &Path) -> Rc<Listing> {
        let spec = self.project_sort_spec();
        let chips = self.project_type_set();
        let scope = self.prefs.project_ui.scope();
        let wants_meta = matches!(spec.key, SortKey::Modified | SortKey::Size);
        let key = format!(
            "{}|{}|{}|{}|{:?}|{}|{}{}|{}|{}",
            self.project.rev,
            self.assets.len(),
            self.project.folder,
            self.project.filter,
            self.project.place == Place::Code,
            scope.id(),
            spec.key.id(),
            spec.descending,
            crate::sorting::filters_text(&chips),
            self.code.files.len(),
        );
        if self.project.listing_key_matches(&key) {
            if let Some(listing) = &self.project.listing {
                return listing.clone();
            }
        }
        let query = Query::parse(&self.project.filter);
        let searching = !query.is_empty() && self.project.place == Place::Assets;
        let sources_visible = chips.is_empty() || chips.contains(&TypeFilter::Scripts);
        let asset_visible = |asset: &AssetEntry| chips.is_empty() || chips.contains(&TypeFilter::of_importer(&asset.importer));
        let words_match = |text: &str| {
            let lower = text.to_lowercase();
            query.words.iter().all(|w| lower.contains(w.as_str()))
        };
        let asset_item = |asset: &AssetEntry, dir: String| Item {
            kind: Kind::Asset,
            key: asset.path.clone(),
            name: file_name(&asset.path).to_string(),
            guid: asset.guid,
            importer: asset.importer.clone(),
            type_label: crate::sorting::type_label(&asset.importer, &asset.path),
            disk: root.join(&asset.path),
            dir,
            size: None,
            modified: None,
        };
        let folder_item = |path: &str, dir: String| Item {
            kind: Kind::Folder,
            key: path.to_string(),
            name: file_name(path).to_string(),
            guid: Guid(0),
            importer: "Folder".to_string(),
            type_label: "Folder",
            disk: root.join(path),
            dir,
            size: None,
            modified: None,
        };
        let source_item = |name: &str, path: &PathBuf| Item {
            kind: Kind::Source,
            key: format!("\u{2}{name}"),
            name: file_name(name).to_string(),
            guid: Guid(0),
            importer: "Code".to_string(),
            type_label: crate::sorting::type_label("Code", name),
            disk: path.clone(),
            dir: String::new(),
            size: None,
            modified: None,
        };

        let mut items: Vec<Item> = Vec::new();
        if self.project.place == Place::Code {
            for (name, path) in &self.code.files {
                if words_match(name) {
                    items.push(source_item(name, path));
                }
            }
            // Сценарии Scenario и исходники внутри каталога ассетов — тоже код игры.
            for asset in &self.assets {
                let is_code = matches!(asset.importer.as_str(), "Scenario" | "Code");
                let listed = self.code.files.iter().any(|(_, path)| path == &root.join(&asset.path));
                if is_code && !listed && words_match(&asset.path) {
                    items.push(asset_item(asset, String::new()));
                }
            }
        } else if searching {
            let prefix = if scope == Scope::ThisFolder && !self.project.folder.is_empty() {
                format!("{}/", self.project.folder)
            } else {
                String::new()
            };
            for asset in &self.assets {
                if asset.path.starts_with(&prefix) && query.matches(&asset.path, &asset.importer) && asset_visible(asset) {
                    items.push(asset_item(asset, parent_of(&asset.path).to_string()));
                }
            }
            if query.types.is_empty() && query.unknown_types.is_empty() && chips.is_empty() {
                for folder in &self.folders {
                    if folder.starts_with(&prefix) && words_match(folder) {
                        items.push(folder_item(folder, parent_of(folder).to_string()));
                    }
                }
            }
            let scripts_wanted = query.types.is_empty() || query.types.contains(&TypeFilter::Scripts);
            if sources_visible && scripts_wanted && query.unknown_types.is_empty() {
                for (name, path) in &self.code.files {
                    if words_match(name) {
                        items.push(source_item(name, path));
                    }
                }
            }
        } else {
            let folder = &self.project.folder;
            let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
            for sub in &self.folders {
                if let Some(rest) = sub.strip_prefix(&prefix) {
                    if !rest.is_empty() && !rest.contains('/') {
                        items.push(folder_item(sub, String::new()));
                    }
                }
            }
            for asset in &self.assets {
                if let Some(rest) = asset.path.strip_prefix(&prefix) {
                    if !rest.contains('/') && asset_visible(asset) {
                        items.push(asset_item(asset, String::new()));
                    }
                }
            }
        }

        // Размер и время правки нужны сортировке; столбцы списка берут их лениво у видимых.
        let infos: Vec<ItemInfo> = items
            .iter()
            .map(|item| {
                let (size, modified) = if wants_meta { self.project.meta.get(&item.disk) } else { (None, None) };
                ItemInfo {
                    name: item.name.clone(),
                    kind: item.type_label,
                    folder: item.kind == Kind::Folder,
                    modified,
                    size: if item.kind == Kind::Folder { None } else { size },
                }
            })
            .collect();
        let order = crate::sorting::sorted_order(spec, &infos);
        let mut slots: Vec<Option<Item>> = items.into_iter().map(Some).collect();
        let mut sorted: Vec<Item> = order.into_iter().filter_map(|i| slots[i].take()).collect();
        for (item, info) in sorted.iter_mut().zip(order_infos(&infos, spec)) {
            item.size = info.size;
            item.modified = info.modified;
        }

        // Группы: «Code» — по языкам со счётчиками, остальное — одна группа без заголовка.
        let mut groups = Vec::new();
        if self.project.place == Place::Code {
            let mut by_language: std::collections::BTreeMap<Language, Vec<Item>> = Default::default();
            for item in sorted.drain(..) {
                by_language.entry(Language::of_path(&item.name)).or_default().push(item);
            }
            for (language, list) in by_language {
                let start = sorted.len();
                let heading = format!("{}  ·  {}", language.label(), list.len());
                sorted.extend(list);
                groups.push(Group { heading: Some(heading), range: start..sorted.len() });
            }
        } else {
            groups.push(Group { heading: None, range: 0..sorted.len() });
        }
        let keys = sorted.iter().map(|i| i.key.clone()).collect();
        let listing = Rc::new(Listing { items: sorted, keys, groups, searching, unknown_types: query.unknown_types.clone() });
        self.project.set_listing(key, listing.clone());
        listing
    }
}

/// Инфо в порядке сортировки (для переноса размера и времени на элементы).
fn order_infos(infos: &[ItemInfo], spec: crate::sorting::SortSpec) -> Vec<ItemInfo> {
    crate::sorting::sorted_order(spec, infos).into_iter().map(|i| infos[i].clone()).collect()
}

// ----------------------------------------------------------------------
// Раскладка
// ----------------------------------------------------------------------

/// Где лежит каждый элемент (относительно левого верхнего угла содержимого).
pub(crate) struct Layout {
    pub rects: Vec<Rect>,
    pub headings: Vec<(f32, String)>,
    pub total: f32,
}

const HEADING_H: f32 = 26.0;
/// Высота подписи под плиткой.
const LABEL_H: f32 = 20.0;

impl Layout {
    pub fn compute(listing: &Listing, width: f32, grid: bool, tile: f32) -> Layout {
        let gap = theme::SP_S;
        let (cell_w, cell_h) = if grid { (tile, tile + LABEL_H) } else { (width, theme::ROW) };
        let cols = if grid { (((width + gap) / (cell_w + gap)).floor() as usize).max(1) } else { 1 };
        let pitch_y = if grid { cell_h + gap } else { cell_h };
        let mut rects = vec![Rect::NOTHING; listing.items.len()];
        let mut headings = Vec::new();
        let mut y = 0.0;
        for group in &listing.groups {
            if let Some(text) = &group.heading {
                headings.push((y, text.clone()));
                y += HEADING_H;
            }
            for (n, index) in group.range.clone().enumerate() {
                let (col, row) = (n % cols, n / cols);
                let x = col as f32 * (cell_w + if grid { gap } else { 0.0 });
                rects[index] = Rect::from_min_size(pos2(x, y + row as f32 * pitch_y), vec2(cell_w, cell_h));
            }
            let rows = group.range.len().div_ceil(cols);
            y += rows as f32 * pitch_y;
        }
        Layout { rects, headings, total: y + theme::SP_M }
    }
}

// ----------------------------------------------------------------------
// Рисование
// ----------------------------------------------------------------------

struct Env<'a> {
    project: &'a mut crate::project::ProjectState,
    previews: &'a mut crate::preview::Previews,
    listing: &'a Listing,
    ctx: egui::Context,
    root: &'a Path,
    cx: &'a MenuCx,
    budget: usize,
    now: std::time::SystemTime,
    columns: crate::project_bar::Columns,
    list: bool,
}

fn fill_for(selected: bool, focused: bool, hovered: bool) -> Option<Color32> {
    match (selected, focused, hovered) {
        (true, true, _) => Some(theme::selection_fill()),
        (true, false, _) => Some(theme::selection_unfocused()),
        (false, _, true) => Some(theme::hover_fill()),
        _ => None,
    }
}

/// Имя, сжатое посередине, чтобы расширение осталось видно: `very_lo…name.png`.
pub(crate) fn elide_middle(painter: &egui::Painter, text: &str, font: &egui::FontId, max_width: f32) -> String {
    let width = |s: &str| painter.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x;
    if width(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let extension = text.rfind('.').map_or(0, |dot| text[dot..].chars().count());
    let build = |keep: usize| -> String {
        let tail = (extension + 2).min(keep / 2).max(1);
        let head = keep.saturating_sub(tail);
        let mut out: String = chars[..head.min(chars.len())].iter().collect();
        out.push('…');
        out.extend(chars[chars.len().saturating_sub(tail)..].iter());
        out
    };
    let (mut low, mut high) = (2usize, chars.len());
    while low + 1 < high {
        let mid = (low + high) / 2;
        if width(&build(mid)) <= max_width {
            low = mid;
        } else {
            high = mid;
        }
    }
    build(low)
}

/// Миниатюра для элемента: текстуры читаются с диска (не больше четырёх новых за кадр),
/// модели и материалы рисует 3D-предпросмотр.
fn thumb_for(env: &mut Env, item: &Item, size: f32, rect: Rect, visible: bool) -> Thumb {
    if item.kind != Kind::Asset {
        return Thumb::Pending;
    }
    if visible {
        env.project.entry_rects.insert(item.key.clone(), rect);
    }
    let ppp = env.ctx.pixels_per_point();
    if item.importer == "Mesh" || item.importer == "Material" {
        return if visible { env.previews.thumbnail(item.guid, size * ppp) } else { Thumb::Pending };
    }
    if item.importer != "Texture" {
        return Thumb::Pending;
    }
    if let Some(texture) = env.project.thumbnails.get(&item.guid) {
        return texture.as_ref().map_or(Thumb::Pending, |t| Thumb::Ready(t.id()));
    }
    if env.budget == 0 {
        env.ctx.request_repaint();
        return Thumb::Pending;
    }
    env.budget -= 1;
    let texture = load_thumbnail(&env.ctx, &item.disk, &item.key);
    let thumb = texture.as_ref().map_or(Thumb::Pending, |t| Thumb::Ready(t.id()));
    env.project.thumbnails.insert(item.guid, texture);
    thumb
}

fn can_drop(dragging: &[String], folder: &str, copy: bool) -> bool {
    if dragging.is_empty() {
        return false;
    }
    if dragging.iter().any(|p| p == folder || folder.starts_with(&format!("{p}/"))) {
        return false;
    }
    copy || dragging.iter().any(|p| parent_of(p) != folder)
}

/// Поле переименования на месте. `Some(Cmd)` — решение принято.
fn rename_field(ui: &mut egui::Ui, project: &mut crate::project::ProjectState, rect: Rect, center: bool, root: &Path) -> Option<Cmd> {
    let rename = project.rename.as_mut()?;
    let id = egui::Id::new(("project_rename_field", &rename.path));
    let ctx = ui.ctx().clone();
    if rename.fresh {
        rename.fresh = false;
        // Выделено имя без расширения (у каталогов — всё имя).
        let is_dir = project_is_dir(root, &rename.path);
        let stem_chars = if is_dir { rename.text.chars().count() } else { stem_len(&rename.text) };
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(stem_chars),
        )));
        state.store(&ctx, id);
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let mut text = rename.text.clone();
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id(id)
                    .font(theme::body())
                    .desired_width(rect.width())
                    .horizontal_align(if center { egui::Align::Center } else { egui::Align::LEFT })
                    .margin(egui::Margin::symmetric(theme::SP_S, 1.0)),
            )
        })
        .inner;
    rename.text = text.clone();
    let from = rename.path.clone();
    let mut error = validate_name(&text).err();
    if error.is_none() && text != file_name(&from) {
        if let Ok(dir) = crate::project_fs::dir_join(root, parent_of(&from)) {
            let target = dir.join(&text);
            let same = text.eq_ignore_ascii_case(file_name(&from));
            if !same && (target.exists() || crate::project_fs::meta_of(&target).exists()) {
                error = Some(format!("`{text}` already exists here"));
            }
        }
    }
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escape {
        project.rename = None;
        ctx.memory_mut(|m| m.surrender_focus(id));
        return None;
    }
    if let Some(why) = &error {
        // Сообщение под полем.
        egui::Area::new(id.with("error"))
            .order(egui::Order::Tooltip)
            .fixed_pos(rect.left_bottom() + vec2(0.0, 2.0))
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).fill(theme::BG_RAISED).show(ui, |ui| {
                    ui.label(egui::RichText::new(why).size(theme::FONT_CAPTION).color(theme::ERROR));
                });
            });
    }
    if response.lost_focus() {
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        if error.is_some() {
            if enter {
                // Принять неверное имя нельзя: остаёмся в поле.
                ctx.memory_mut(|m| m.request_focus(id));
            } else {
                project.rename = None; // щелчок в сторону при ошибке — отмена
            }
            return None;
        }
        project.rename = None;
        return Some(Cmd::CommitRename { from, name: text });
    }
    None
}

/// Длина имени без расширения в символах (`a.b.png` → 3 символа `a.b`).
fn stem_len(name: &str) -> usize {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].chars().count(),
        _ => name.chars().count(),
    }
}

fn project_is_dir(root: &Path, path: &str) -> bool {
    crate::project_fs::safe_join(root, path).is_ok_and(|p| p.is_dir())
}

/// Нарисовать плитку или строку и обработать ввод по ней.
fn draw_item(ui: &mut egui::Ui, env: &mut Env, index: usize, rect: Rect, viewport_visible: bool) {
    let listing = env.listing;
    let item = &listing.items[index];
    let selected = item.selectable() && env.project.selection.contains(&item.key);
    let renaming = env.project.rename.as_ref().is_some_and(|r| r.path == item.key);
    let cut = env.project.clipboard.as_ref().is_some_and(|c| c.cut && c.paths.contains(&item.key));
    let focused = env.project.focus;
    let size = if env.list { 18.0 } else { rect.width() - 12.0 };
    let thumb = thumb_for(env, item, size, rect, viewport_visible);

    // Ввод: область заводится до рисования, чтобы подсветка учитывала наведение.
    let sense = if item.selectable() { Sense::click_and_drag() } else { Sense::click() };
    let response = ui.interact(rect, item.id(), sense);
    let modifiers = ui.input(|i| i.modifiers);
    let now = ui.input(|i| i.time);
    let dragging_over = item.kind == Kind::Folder
        && response.dnd_hover_payload::<DragAsset>().is_some()
        && can_drop(&env.project.dragging, &item.key, modifiers.command);

    if !renaming && item.selectable() {
        if response.secondary_clicked() && !selected {
            env.project.select_only(&item.key);
        }
        // Второй щелчок двойного egui сообщает как `double_clicked`, а не `clicked`.
        if response.clicked() || response.double_clicked() {
            let only_this = env.project.selection.len() == 1 && selected;
            let slow = env.project.last_click.as_ref().is_some_and(|(k, t)| *k == item.key && now - t >= 0.6 && now - t < 2.0);
            if only_this && slow && !modifiers.command && !modifiers.shift && !response.double_clicked() {
                env.project.cmds.push(Cmd::BeginRename(item.key.clone()));
            } else {
                click_select(
                    &mut env.project.selection,
                    &mut env.project.anchor,
                    &listing.keys,
                    index,
                    modifiers.command,
                    modifiers.shift,
                );
                // Ctrl-щелчок мог снять выделение с элемента: активным остаётся выделенный.
                env.project.selected_path = if env.project.selection.contains(&item.key) {
                    Some(item.key.clone())
                } else {
                    env.project.selection.iter().next_back().cloned()
                };
                env.project.sync_active();
                let material = (item.importer == "Material").then_some(item.guid);
                env.project.cmds.push(Cmd::Act(Action::InspectMaterial(material.filter(|_| env.project.selection.len() == 1))));
            }
            env.project.last_click = Some((item.key.clone(), now));
        }
        if response.drag_started() {
            if !env.project.selection.contains(&item.key) {
                env.project.select_only(&item.key);
            }
            let paths: Vec<String> = env.project.selection.iter().cloned().collect();
            env.project.dragging = paths.clone();
            response.dnd_set_drag_payload(DragAsset {
                guid: item.guid,
                importer: item.importer.clone(),
                path: item.key.clone(),
                paths,
            });
        }
    }
    if item.kind == Kind::Folder {
        if let Some(payload) = response.dnd_release_payload::<DragAsset>() {
            if can_drop(&payload.paths, &item.key, modifiers.command) {
                env.project.cmds.push(Cmd::Move { paths: payload.paths.clone(), to: item.key.clone(), copy: modifiers.command });
            }
        }
    }
    if response.double_clicked() && !renaming {
        match item.kind {
            Kind::Source => env.project.cmds.push(Cmd::SourceOpen(item.disk.clone())),
            _ => env.project.cmds.push(Cmd::Open(vec![item.key.clone()])),
        }
    }
    {
        let keys: Vec<String> = if selected { env.project.selection.iter().cloned().collect() } else { vec![item.key.clone()] };
        let cx = env.cx;
        let cmds = &mut env.project.cmds;
        response.context_menu(|ui| project_menus::item_menu(ui, cmds, cx, item, keys));
    }

    // Рисование.
    let hovered = response.hovered() || dragging_over;
    let (icon, color) = if item.kind == Kind::Folder {
        (Icon::Folder, theme::FOLDER)
    } else {
        asset_icon(&item.importer)
    };
    let dim = |c: Color32| if cut { c.gamma_multiply(0.45) } else { c };
    let name_color = if selected { theme::TEXT_STRONG } else { dim(theme::TEXT) };
    let painter = ui.painter().clone();
    if env.list {
        if let Some(fill) = fill_for(selected, focused, hovered) {
            painter.rect_filled(rect, 0.0, fill);
        }
        if dragging_over {
            painter.rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.0_f32, theme::accent()));
        }
        let icon_rect = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(theme::ICON, theme::ICON));
        paint_thumb(ui, &thumb, item, icon_rect, icon, color, dim, 0.0);
        let name_rect = Rect::from_min_max(pos2(rect.left() + 30.0, rect.top()), pos2(env.columns.name_limit, rect.bottom()));
        if renaming {
            if let Some(cmd) = rename_field(ui, env.project, name_rect.shrink2(vec2(0.0, 1.0)), false, env.root) {
                env.project.cmds.push(cmd);
            }
        } else {
            let font = theme::body();
            let mut shown = elide_middle(&painter, &item.name, &font, name_rect.width());
            if shown != item.name {
                shown = shown.to_string();
            }
            let galley = painter.layout_no_wrap(shown, font, name_color);
            let name_width = galley.size().x;
            painter.with_clip_rect(name_rect).galley(pos2(name_rect.left(), rect.center().y - galley.size().y * 0.5), galley, name_color);
            if !item.dir.is_empty() || (listing.searching && item.kind != Kind::Source) {
                let x = name_rect.left() + name_width + theme::SP_M;
                if x < env.columns.name_limit - 40.0 {
                    painter.with_clip_rect(name_rect).text(
                        pos2(x, rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("Assets/{}", item.dir),
                        theme::caption(),
                        theme::TEXT_DISABLED,
                    );
                }
            }
        }
        let (size_value, modified) = if item.size.is_some() || item.modified.is_some() {
            (item.size, item.modified)
        } else if item.kind == Kind::Folder {
            (None, env.project.meta.get(&item.disk).1)
        } else {
            env.project.meta.get(&item.disk)
        };
        let cell = |right: f32, text: String| {
            painter.with_clip_rect(rect).text(pos2(right - 14.0, rect.center().y), Align2::RIGHT_CENTER, text, theme::caption(), theme::TEXT_MUTED);
        };
        if let Some(right) = env.columns.type_right {
            cell(right, item.type_label.to_string());
        }
        if let Some(right) = env.columns.modified_right {
            cell(right, crate::sorting::modified_label(env.now, modified));
        }
        cell(env.columns.size_right, size_value.map(crate::sorting::size_label).unwrap_or_default());
    } else {
        if let Some(fill) = fill_for(selected, focused, hovered) {
            painter.rect_filled(rect, theme::R_M, fill);
        }
        if dragging_over {
            painter.rect_filled(rect, theme::R_M, theme::accent_soft());
            painter.rect_stroke(rect, theme::R_M, Stroke::new(1.0_f32, theme::accent()));
        }
        let side = rect.width() - 12.0;
        let preview = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(side, side));
        paint_thumb(ui, &thumb, item, preview, icon, color, dim, preview.width() * 0.2);
        let label_rect = Rect::from_min_max(pos2(rect.left() + 2.0, preview.bottom() + 2.0), pos2(rect.right() - 2.0, rect.bottom()));
        if renaming {
            let field = Rect::from_min_max(pos2(rect.left(), preview.bottom() + 1.0), pos2(rect.right().max(rect.left() + 96.0), rect.bottom() + 1.0));
            if let Some(cmd) = rename_field(ui, env.project, field, true, env.root) {
                env.project.cmds.push(cmd);
            }
        } else {
            let font = theme::caption();
            let shown = elide_middle(&painter, &item.name, &font, label_rect.width());
            let galley = painter.layout_no_wrap(shown, font, name_color);
            painter.with_clip_rect(label_rect).galley(
                pos2(label_rect.center().x - galley.size().x * 0.5, label_rect.center().y - galley.size().y * 0.5),
                galley,
                name_color,
            );
        }
    }
    let tip = match &thumb {
        Thumb::Failed(reason) => format!("{}\nCannot preview: {reason}", item.name),
        _ if item.dir.is_empty() => item.name.clone(),
        _ => format!("{}\nAssets/{}", item.name, item.dir),
    };
    response.on_hover_text(tip);
}

#[allow(clippy::too_many_arguments)]
fn paint_thumb(
    ui: &egui::Ui,
    thumb: &Thumb,
    item: &Item,
    rect: Rect,
    icon: Icon,
    color: Color32,
    dim: impl Fn(Color32) -> Color32,
    icon_inset: f32,
) {
    let is_model = item.importer == "Mesh" || item.importer == "Material";
    let painter = ui.painter();
    match thumb {
        Thumb::Ready(texture) if is_model => paint_model(ui, *texture, rect.expand(if rect.width() < 24.0 { 2.0 } else { 0.0 }), theme::R_M),
        Thumb::Ready(texture) => {
            if rect.width() > 24.0 {
                painter.rect_filled(rect, theme::R_S, theme::BG_FIELD);
            }
            let tint = dim(Color32::WHITE);
            painter.image(*texture, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        Thumb::Failed(_) => icons::paint(painter, rect.shrink(icon_inset), icon, theme::ERROR),
        Thumb::Pending => icons::paint(painter, rect.shrink(icon_inset), icon, dim(color)),
    }
}

/// Заголовок группы («Rust · 4») в окне Code.
fn group_header(ui: &egui::Ui, rect: Rect, text: &str) {
    ui.painter().text(
        pos2(rect.left() + theme::SP_S, rect.center().y),
        Align2::LEFT_CENTER,
        text.to_uppercase(),
        theme::strong(theme::FONT_SECTION),
        theme::TEXT_MUTED,
    );
    ui.painter().hline(rect.x_range(), rect.bottom() - 2.0, Stroke::new(1.0_f32, theme::LINE_SOFT));
}

// ----------------------------------------------------------------------
// Клавиатура
// ----------------------------------------------------------------------

#[derive(Default)]
struct KeyInput {
    keys: Vec<(egui::Key, Modifiers)>,
    text: String,
    copy: bool,
    cut: bool,
    paste: Option<String>,
}

fn handled_key(key: egui::Key, m: Modifiers) -> bool {
    use egui::Key::*;
    match key {
        ArrowLeft | ArrowRight | ArrowUp | ArrowDown | Home | End | PageUp | PageDown => !m.command,
        Enter | F2 | Delete | Backspace => !m.command,
        Escape => true,
        A | C | X | V | D | F => m.command && !m.alt,
        _ => false,
    }
}

fn take_input(ctx: &egui::Context) -> KeyInput {
    let mut input = KeyInput::default();
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key, pressed: true, modifiers, .. } if handled_key(*key, *modifiers) => {
                input.keys.push((*key, *modifiers));
                false
            }
            egui::Event::Text(text) if !text.chars().any(char::is_control) => {
                input.text.push_str(text);
                false
            }
            egui::Event::Copy => {
                input.copy = true;
                false
            }
            egui::Event::Cut => {
                input.cut = true;
                false
            }
            egui::Event::Paste(text) => {
                input.paste = Some(text.clone());
                false
            }
            _ => true,
        });
    });
    input
}

/// Клавиши окна Project. Работают, когда курсор над окном и никто не набирает текст.
fn handle_keys(editor: &mut Editor, ctx: &egui::Context, listing: &Listing, layout: Option<&Layout>, page: usize) {
    // Пока идёт набор текста (имя, путь) или открыто окно, клавиши принадлежат им.
    let busy = editor.project.rename.is_some()
        || editor.project.path_edit.is_some()
        || editor.project.browser.is_some()
        || editor.project.confirm.is_some();
    if !editor.project.focus || busy {
        return;
    }
    let ptr_back = ctx.input(|i| i.pointer.button_pressed(PointerButton::Extra1));
    let ptr_forward = ctx.input(|i| i.pointer.button_pressed(PointerButton::Extra2));
    if ptr_back {
        editor.project.cmds.push(Cmd::Back);
    }
    if ptr_forward {
        editor.project.cmds.push(Cmd::Forward);
    }
    if ctx.wants_keyboard_input() {
        return;
    }
    let input = take_input(ctx);
    let now = ctx.input(|i| i.time);
    let selection: Vec<String> = editor.project.selection.iter().cloned().collect();
    let folder = editor.project.folder.clone();
    let in_assets = editor.project.place == Place::Assets;
    let list_view = !editor.prefs.project_ui.grid;
    let active = editor.project.selected_path.as_ref().and_then(|p| listing.keys.iter().position(|k| k == p));
    let usable = layout.filter(|l| l.rects.len() == listing.items.len());

    let select_index = |editor: &mut Editor, index: usize, extend: bool| {
        let Some(item) = listing.items.get(index) else { return };
        if !item.selectable() {
            return;
        }
        if extend {
            click_select(&mut editor.project.selection, &mut editor.project.anchor, &listing.keys, index, false, true);
            editor.project.selected_path = Some(item.key.clone());
            editor.project.sync_active();
        } else {
            editor.project.select_only(&item.key);
        }
        editor.project.scroll_to = Some(item.key.clone());
        let material = (item.importer == "Material").then_some(item.guid);
        editor.project.cmds.push(Cmd::Act(Action::InspectMaterial(material.filter(|_| !extend))));
    };

    for (key, m) in input.keys {
        use egui::Key::*;
        let n = listing.items.len();
        match key {
            ArrowLeft | ArrowRight | ArrowUp | ArrowDown | Home | End | PageUp | PageDown if m.alt => {
                // Alt+← / → / ↑ — навигация по истории и вверх.
                match key {
                    ArrowLeft => editor.project.cmds.push(Cmd::Back),
                    ArrowRight => editor.project.cmds.push(Cmd::Forward),
                    ArrowUp => editor.project.cmds.push(Cmd::Up),
                    _ => {}
                }
            }
            ArrowLeft | ArrowRight | ArrowUp | ArrowDown | Home | End | PageUp | PageDown => {
                if n == 0 {
                    continue;
                }
                let target = match (key, active) {
                    (Home, _) => Some(0),
                    (End, _) => Some(n - 1),
                    (ArrowDown | ArrowRight | PageDown, None) => Some(0),
                    (ArrowUp | ArrowLeft | PageUp, None) => Some(n - 1),
                    (ArrowLeft | ArrowRight, Some(_)) if list_view => None,
                    (_, Some(current)) => usable.map(|l| {
                        let dir = match key {
                            ArrowLeft => Dir::Left,
                            ArrowRight => Dir::Right,
                            ArrowUp | PageUp => Dir::Up,
                            _ => Dir::Down,
                        };
                        let steps = if matches!(key, PageUp | PageDown) { page.max(1) } else { 1 };
                        let mut at = current;
                        for _ in 0..steps {
                            match neighbour(&l.rects, at, dir) {
                                Some(next) => at = next,
                                None => break,
                            }
                        }
                        at
                    }),
                    _ => None,
                };
                if let Some(index) = target {
                    select_index(editor, index, m.shift);
                }
            }
            Enter => {
                if editor.project.rename.is_none() && !selection.is_empty() {
                    editor.project.cmds.push(Cmd::Open(selection.clone()));
                }
            }
            F2 => {
                if selection.len() == 1 {
                    editor.project.cmds.push(Cmd::BeginRename(selection[0].clone()));
                }
            }
            Delete => {
                if !selection.is_empty() {
                    editor.project.cmds.push(Cmd::Delete(selection.clone()));
                }
            }
            Backspace => editor.project.cmds.push(Cmd::Up),
            Escape => editor.project.clear_selection(),
            A if m.command => editor.project.cmds.push(Cmd::SelectAll),
            C if m.command => editor.project.cmds.push(Cmd::Copy(selection.clone())),
            X if m.command => editor.project.cmds.push(Cmd::Cut(selection.clone())),
            V if m.command && in_assets => editor.project.cmds.push(Cmd::Paste(folder.clone())),
            D if m.command => editor.project.cmds.push(Cmd::Duplicate(selection.clone())),
            F if m.command => editor.project.focus_search = true,
            _ => {}
        }
    }
    if input.copy {
        editor.project.cmds.push(Cmd::Copy(selection.clone()));
    }
    if input.cut {
        editor.project.cmds.push(Cmd::Cut(selection.clone()));
    }
    if in_assets && editor.project_paste_allowed(input.paste.as_deref()) && input.paste.is_some() {
        editor.project.cmds.push(Cmd::Paste(folder));
    }
    if !input.text.is_empty() {
        editor.project.typeahead.feed(&input.text, now);
        let names: Vec<String> = listing.items.iter().map(|i| i.name.clone()).collect();
        if let Some(index) = editor.project.typeahead.find(&names, active) {
            select_index(editor, index, false);
        }
    }
}

// ----------------------------------------------------------------------
// Окно
// ----------------------------------------------------------------------

pub(crate) fn content(editor: &mut Editor, ui: &mut egui::Ui, root: &Path) {
    let ctx = ui.ctx().clone();
    let listing = editor.project_listing(root);
    editor.project.shown = listing.items.iter().map(|i| i.name.clone()).collect();
    editor.project.headings = listing.groups.iter().filter_map(|g| g.heading.clone()).collect();
    editor.project.last_total = listing.items.len();

    // Только что созданный элемент: выделить и сразу переименовать.
    if let Some(path) = editor.project.pending_rename.clone() {
        if listing.keys.contains(&path) {
            editor.project.pending_rename = None;
            editor.project.cmds.push(Cmd::BeginRename(path));
        } else if parent_of(&path) == editor.project.folder && editor.project.rev > 0 && !listing.searching {
            editor.project.pending_rename = None;
        }
    }

    let tile = editor.prefs.project_ui.tile;
    let grid = editor.prefs.project_ui.grid;
    let last_layout = editor.project.layout.clone();
    let page = editor.project.page_rows;
    handle_keys(editor, &ctx, &listing, last_layout.as_deref(), page);

    let spec = editor.project_sort_spec();
    let type_filters = editor.project_type_set();
    let total = listing.items.len();
    if total == 0 {
        let (title, hint) = if !listing.unknown_types.is_empty() {
            ("Unknown type in the search", "Use t:script, t:scene, t:prefab, t:model, t:material, t:texture or t:audio")
        } else if !type_filters.is_empty() {
            ("Nothing of the selected types here", "Click `All` above to show every type")
        } else if editor.project.filter.trim().is_empty() {
            ("This folder is empty", "Drop files here from your file manager, or right-click for Create and Import New Asset")
        } else {
            ("Nothing matches", "Try another name, or search in All Assets")
        };
        theme::empty_state(ui, Icon::Folder, title, hint);
    }
    let mut header_spec = spec;
    if !grid && total > 0 {
        let mut columns = editor.prefs.project_ui.columns;
        header_spec = crate::project_bar::list_header(ui, spec, &mut columns);
        editor.prefs.project_ui.columns = columns;
    }

    let cx = MenuCx::new(editor);
    let widths = editor.prefs.project_ui.columns;
    let scroll_to = editor.project.scroll_to.take();
    let page_hint;
    let new_layout;
    {
        let project = &mut editor.project;
        let previews = &mut editor.previews;
        project.entry_rects.clear();
        let mut env = Env {
            project,
            previews,
            listing: &listing,
            ctx: ctx.clone(),
            root,
            cx: &cx,
            budget: 4,
            now: SystemTime::now(),
            columns: crate::project_bar::Columns::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(ui.available_width(), 1.0)), widths),
            list: !grid,
        };
        let out = egui::ScrollArea::vertical()
            .id_salt("project_content")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let width = ui.available_width();
                let layout = Layout::compute(&listing, width, grid, tile);
                let (_, area) = ui.allocate_space(vec2(width, layout.total.max(viewport.height())));
                let origin = area.min;
                env.columns = crate::project_bar::Columns::new(Rect::from_min_size(origin, vec2(width, theme::ROW)), widths);
                // Фон: щелчок снимает выделение, резинка, меню каталога. Заводится ДО
                // элементов: egui отдаёт щелчок виджету, добавленному последним.
                let background = ui.interact(area, egui::Id::new("project_background"), Sense::click_and_drag());
                if let Some(key) = &scroll_to {
                    if let Some(index) = listing.keys.iter().position(|k| k == key) {
                        ui.scroll_to_rect(layout.rects[index].translate(origin.to_vec2()), None);
                    }
                }
                for (y, text) in &layout.headings {
                    let rect = Rect::from_min_size(pos2(origin.x, origin.y + y), vec2(width, HEADING_H));
                    if rect.max.y >= origin.y + viewport.min.y && rect.min.y <= origin.y + viewport.max.y {
                        group_header(ui, rect, text);
                    }
                }
                let pad = 40.0;
                for (index, local) in layout.rects.iter().enumerate() {
                    if local.max.y < viewport.min.y - pad || local.min.y > viewport.max.y + pad {
                        continue;
                    }
                    let visible = local.max.y >= viewport.min.y && local.min.y <= viewport.max.y;
                    draw_item(ui, &mut env, index, local.translate(origin.to_vec2()), visible);
                }

                // Резинка.
                let pointer = ui.input(|i| i.pointer.interact_pos());
                let modifiers = ui.input(|i| i.modifiers);
                if background.drag_started_by(PointerButton::Primary) {
                    if let Some(p) = pointer {
                        env.project.band = Some(crate::project::Band {
                            origin: p - origin.to_vec2(),
                            base: if modifiers.command { env.project.selection.clone() } else { BTreeSet::new() },
                        });
                    }
                }
                if let (Some(band), Some(p)) = (&env.project.band, pointer) {
                    if background.dragged() {
                        let current = p - origin.to_vec2();
                        let rubber = Rect::from_two_pos(band.origin, current);
                        let mut selection = band.base.clone();
                        for (index, local) in layout.rects.iter().enumerate() {
                            if local.intersects(rubber) && listing.items[index].selectable() {
                                selection.insert(listing.keys[index].clone());
                            }
                        }
                        let on_screen = rubber.translate(origin.to_vec2());
                        ui.painter().rect(on_screen, 0.0, theme::accent_soft().gamma_multiply(0.5), Stroke::new(1.0_f32, theme::accent()));
                        env.project.selection = selection;
                        env.project.selected_path = env.project.selection.iter().next_back().cloned();
                        env.project.sync_active();
                        // У края окна резинка прокручивает содержимое.
                        let clip = ui.clip_rect();
                        if p.y < clip.top() + 14.0 {
                            ui.scroll_with_delta(vec2(0.0, 14.0));
                        } else if p.y > clip.bottom() - 14.0 {
                            ui.scroll_with_delta(vec2(0.0, -14.0));
                        }
                        ui.ctx().request_repaint();
                    }
                }
                if background.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
                    env.project.band = None;
                }
                if background.clicked() && !modifiers.command {
                    env.project.clear_selection();
                    env.project.cmds.push(Cmd::Act(Action::InspectMaterial(None)));
                }
                if env.project.place == Place::Assets {
                    if let Some(payload) = background.dnd_release_payload::<DragAsset>() {
                        let to = env.project.folder.clone();
                        let copy = ui.input(|i| i.modifiers.command);
                        if can_drop(&payload.paths, &to, copy) && !listing.searching {
                            env.project.cmds.push(Cmd::Move { paths: payload.paths.clone(), to, copy });
                        }
                    }
                }
                let cmds = &mut env.project.cmds;
                background.context_menu(|ui| project_menus::space_menu(ui, cmds, env.cx));
                layout
            });
        new_layout = out.inner;
        page_hint = (out.inner_rect.height() / (new_layout.rects.first().map_or(theme::ROW, |r| r.height()) + theme::SP_S)) as usize;
    }
    editor.project.page_rows = page_hint.max(1);
    editor.project.layout = Some(Rc::new(new_layout));
    if header_spec != spec {
        editor.set_project_sort_spec(header_spec);
    }
}
