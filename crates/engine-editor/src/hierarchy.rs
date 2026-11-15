//! Окно Hierarchy: дерево объектов сцены.
//!
//! Строки сначала собираются в плоский список (кто, на какой глубине, раскрыт ли),
//! потом рисуются — так большие сцены прокручиваются без затрат на невидимые строки,
//! а контекстное меню может пользоваться всем редактором.

use crate::i18n::{tr_s, trn};
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
    /// Замок: объект (с детьми) не выбирается щелчком и рамкой в виде Scene. Только в
    /// редакторе, по пути индексов (переживает Undo и Stop).
    pub locked: HashSet<Vec<usize>>,
    /// Меню окна: по алфавиту вместо порядка сцены (только показ, сцена не меняется).
    pub sort_alpha: bool,
    /// Меню окна: результаты поиска плоским списком, а не в дереве с предками.
    pub flat_search: bool,
    /// Меню окна: без чередования фона строк.
    pub plain_rows: bool,
    /// Выделение, которое уже показали (раскрыли предков и прокрутили к нему).
    revealed: Option<GameObjectId>,
    /// Прокрутить к этой строке в следующем кадре.
    scroll_to: Option<GameObjectId>,
    /// Набор имени с клавиатуры (переход к строке): набранное и время последней буквы.
    type_ahead: (String, f64),
    /// Объект тащат над свёрнутой строкой: с какого времени (раскрыть через полсекунды).
    hover_expand: Option<(Vec<usize>, f64)>,
    /// Сколько объектов нашёл поиск в прошлом кадре.
    pub matches: usize,
    /// Строки прошлого кадра и отпечаток того, из чего они собраны (сцена и состояние
    /// окна): в большой сцене строки не пересобираются каждый кадр.
    rows_cache: Option<(u64, std::sync::Arc<Vec<Row>>)>,
    /// Буфер обхода сцены для отпечатка.
    fingerprint_ids: Vec<GameObjectId>,
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
    /// Сколько прямых детей (число у свёрнутого родителя).
    child_count: usize,
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
    /// Плоский поиск — путь родителей серым.
    context: Option<String>,
    /// Поиск в дереве: предок совпадения, сам не подходит (рисуется приглушённо).
    context_only: bool,
    /// Совпавшие с поиском символы имени (номера символов) — подсвечиваются.
    highlight: Vec<usize>,
    /// Замок на самом объекте / унаследованный от предка.
    locked: bool,
    lock_inherited: bool,
}

/// Куда упадёт перетаскиваемый объект относительно строки.
#[derive(Clone, Copy, PartialEq)]
enum Drop {
    Before,
    Into,
    After,
}

/// Разобранная строка поиска: слова имени (нечётко), `t:Тип`, `tag:Метка`, `layer:Слой`.
#[derive(Default)]
struct Query {
    words: Vec<String>,
    types: Vec<String>,
    tags: Vec<String>,
    layers: Vec<String>,
}

impl Query {
    fn parse(text: &str) -> Self {
        let mut query = Self::default();
        for word in text.split_whitespace() {
            let lower = word.to_lowercase();
            let (list, rest) = if let Some(rest) = lower.strip_prefix("t:") {
                (&mut query.types, rest)
            } else if let Some(rest) = lower.strip_prefix("tag:") {
                (&mut query.tags, rest)
            } else if let Some(rest) = lower.strip_prefix("layer:") {
                (&mut query.layers, rest)
            } else {
                (&mut query.words, lower.as_str())
            };
            if !rest.is_empty() {
                list.push(rest.to_string());
            }
        }
        query
    }

    fn is_empty(&self) -> bool {
        self.words.is_empty() && self.types.is_empty() && self.tags.is_empty() && self.layers.is_empty()
    }

    /// Подходит ли объект; `Some(символы имени для подсветки)`.
    fn matches(&self, object: &GameObject, layer_name: &dyn Fn(u32) -> String) -> Option<Vec<usize>> {
        let has_type = |t: &String| object.components().any(|c| c.type_name().to_lowercase().contains(t.as_str()));
        if !self.types.iter().all(has_type) {
            return None;
        }
        let tag = object.tag.to_lowercase();
        if !self.tags.iter().all(|t| tag.contains(t.as_str())) {
            return None;
        }
        let layer = layer_name(object.layer).to_lowercase();
        if !self.layers.iter().all(|l| layer.contains(l.as_str()) || *l == object.layer.to_string()) {
            return None;
        }
        let mut highlight = Vec::new();
        for word in &self.words {
            highlight.extend(fuzzy_chars(&object.name, word)?);
        }
        highlight.sort_unstable();
        highlight.dedup();
        Some(highlight)
    }
}

/// Символы `name`, совпавшие со словом поиска: подстрока, иначе буквы по порядку
/// («rl3» находит «Rail L 3»). `None` — не подходит.
fn fuzzy_chars(name: &str, word: &str) -> Option<Vec<usize>> {
    let lower: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    let wanted: Vec<char> = word.chars().collect();
    if wanted.is_empty() {
        return Some(Vec::new());
    }
    // Подстрока (у букв с многосимвольным нижним регистром номера могут сдвинуться —
    // тогда подсветка просто чуть неточна).
    if let Some(start) = (0..lower.len()).find(|&i| lower[i..].starts_with(&wanted)) {
        return Some((start..start + wanted.len()).collect());
    }
    let mut found = Vec::with_capacity(wanted.len());
    let mut at = 0;
    for c in wanted {
        let offset = lower[at..].iter().position(|l| *l == c)?;
        found.push(at + offset);
        at += offset + 1;
    }
    Some(found)
}

/// Путь объекта начинается одним из путей набора (сам объект или его предок).
fn under_any(set: &HashSet<Vec<usize>>, path: &[usize]) -> bool {
    !set.is_empty() && (1..=path.len()).any(|n| set.contains(&path[..n]))
}

fn collect(editor: &Editor) -> Vec<Row> {
    let scene = &editor.app.world.scene;
    let state = &editor.hierarchy;
    let mut rows = Vec::new();
    let query = Query::parse(&state.filter);
    // Имена слоёв — из настроек проекта, без них — встроенные (Default, UI, Water…).
    let default_layers = engine_core::settings::TagsAndLayers::default();
    let layers = editor
        .app
        .world
        .resources
        .get::<engine_core::settings::ProjectSettings>()
        .map_or(&default_layers, |s| &s.tags_and_layers);
    let layer_name = |layer: u32| layers.layer_name(layer);

    let row_of = |id: GameObjectId, depth: usize, path: Vec<usize>, guides: Vec<bool>, sibling_index: usize| {
        let object = scene.get(id)?;
        let (icon, icon_color) = object_icon(object);
        let child_count = object.children().len();
        let has_children = child_count > 0;
        let locked = state.locked.contains(&path);
        let lock_inherited = !locked && under_any(&state.locked, &path);
        Some(Row {
            id,
            depth,
            name: object.name.clone(),
            icon,
            icon_color,
            has_children,
            child_count,
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
            context_only: false,
            highlight: Vec::new(),
            locked,
            lock_inherited,
        })
    };

    // Поиск: какие объекты подошли (и подсветка их имён).
    let found: Option<std::collections::HashMap<GameObjectId, Vec<usize>>> = (!query.is_empty()).then(|| {
        scene
            .ids()
            .into_iter()
            .filter_map(|id| {
                let object = scene.get(id)?;
                query.matches(object, &layer_name).map(|highlight| (id, highlight))
            })
            .collect()
    });

    if let (Some(found), true) = (&found, state.flat_search) {
        // Плоский список совпадений (в порядке сцены) с путём родителей.
        for id in scene.ids() {
            let Some(highlight) = found.get(&id) else { continue };
            let Some(object) = scene.get(id) else { continue };
            let path = crate::state::path_of(scene, id).unwrap_or_default();
            let index = path.last().copied().unwrap_or(0);
            if let Some(mut row) = row_of(id, 0, path, Vec::new(), index) {
                row.has_children = false;
                row.open = false;
                row.highlight = highlight.clone();
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

    // Поиск в дереве: совпадения с их предками (раскрытыми), остальное скрыто.
    let shown: Option<HashSet<GameObjectId>> = found.as_ref().map(|found| {
        let mut shown = HashSet::new();
        for id in found.keys() {
            let mut cursor = Some(*id);
            while let Some(current) = cursor {
                if !shown.insert(current) {
                    break;
                }
                cursor = scene.get(current).and_then(|o| o.parent());
            }
        }
        shown
    });

    struct Walk<'a> {
        scene: &'a Scene,
        sort_alpha: bool,
        shown: Option<&'a HashSet<GameObjectId>>,
        found: Option<&'a std::collections::HashMap<GameObjectId, Vec<usize>>>,
    }
    fn walk(
        w: &Walk<'_>,
        rows: &mut Vec<Row>,
        ids: &[GameObjectId],
        depth: usize,
        path: &mut Vec<usize>,
        guides: &mut Vec<bool>,
        row_of: &dyn Fn(GameObjectId, usize, Vec<usize>, Vec<bool>, usize) -> Option<Row>,
    ) {
        // Индексы — место в сцене (пути, Undo); порядок показа — по алфавиту, если включено.
        let mut order: Vec<(usize, GameObjectId)> = ids
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, id)| w.shown.is_none_or(|shown| shown.contains(id)))
            .collect();
        if w.sort_alpha {
            order.sort_by(|a, b| {
                let name = |id: GameObjectId| w.scene.get(id).map_or("", |o| o.name.as_str());
                crate::sorting::natural_cmp(name(a.1), name(b.1))
            });
        }
        let count = order.len();
        for (position, (index, id)) in order.into_iter().enumerate() {
            path.push(index);
            let last = position + 1 == count;
            if let Some(mut row) = row_of(id, depth, path.clone(), guides.clone(), index) {
                row.last = last;
                if let Some(found) = w.found {
                    match found.get(&id) {
                        Some(highlight) => row.highlight = highlight.clone(),
                        None => row.context_only = true,
                    }
                    // Предки совпадений раскрыты, у самих совпадений — как было.
                    let has_shown_children = w
                        .scene
                        .get(id)
                        .is_some_and(|o| o.children().iter().any(|c| w.shown.is_some_and(|s| s.contains(c))));
                    row.has_children = has_shown_children;
                    row.open = has_shown_children;
                }
                let open = row.open;
                rows.push(row);
                if open {
                    let children = w.scene.get(id).map(|o| o.children().to_vec()).unwrap_or_default();
                    guides.push(!last);
                    walk(w, rows, &children, depth + 1, path, guides, row_of);
                    guides.pop();
                }
            }
            path.pop();
        }
    }
    let w = Walk { scene, sort_alpha: state.sort_alpha, shown: shown.as_ref(), found: found.as_ref() };
    walk(&w, &mut rows, scene.roots(), 0, &mut Vec::new(), &mut Vec::new(), &row_of);
    rows
}

/// Отпечаток всего, от чего зависят строки без поиска: порядок и имена объектов, их
/// флажки, родители и дети, префабы, набор компонентов (значок; у `Light` — вид), а также
/// свёрнутые и запертые узлы и сортировка окна. Обход без выделения памяти на объект.
fn fingerprint(editor: &Editor, ids: &mut Vec<GameObjectId>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let state = &editor.hierarchy;
    state.sort_alpha.hash(&mut h);
    state.flat_search.hash(&mut h);
    let set = |set: &HashSet<Vec<usize>>| {
        set.iter().fold(set.len() as u64, |acc, path| {
            let mut one = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut one);
            acc ^ one.finish().wrapping_mul(0x9E37_79B9_7F4A_7C15)
        })
    };
    set(&state.collapsed).hash(&mut h);
    set(&state.locked).hash(&mut h);
    let scene = &editor.app.world.scene;
    scene.ids_into(ids);
    for &id in ids.iter() {
        let Some(object) = scene.get(id) else { continue };
        id.hash(&mut h);
        object.name.hash(&mut h);
        object.active.hash(&mut h);
        object.parent().hash(&mut h);
        object.children().len().hash(&mut h);
        object.prefab().is_some().hash(&mut h);
        for component in object.components() {
            let name = component.type_name();
            name.hash(&mut h);
            if name == "Light" {
                if let Some(Value::Enum(kind)) = component.get_field("kind") {
                    kind.hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// Строки окна: без поиска — из кэша, если отпечаток не изменился.
fn cached_rows(editor: &mut Editor) -> std::sync::Arc<Vec<Row>> {
    if !Query::parse(&editor.hierarchy.filter).is_empty() {
        editor.hierarchy.rows_cache = None;
        return std::sync::Arc::new(collect(editor));
    }
    let mut ids = std::mem::take(&mut editor.hierarchy.fingerprint_ids);
    let key = fingerprint(editor, &mut ids);
    editor.hierarchy.fingerprint_ids = ids;
    if let Some((cached, rows)) = &editor.hierarchy.rows_cache {
        if *cached == key {
            return rows.clone();
        }
    }
    let rows = std::sync::Arc::new(collect(editor));
    editor.hierarchy.rows_cache = Some((key, rows.clone()));
    rows
}

pub(crate) fn show(editor: &mut Editor, ui: &mut egui::Ui) {
    // Одна полоса, как в Unity: «+ Create», поиск и меню окна (имя окна — на вкладке дока).
    theme::toolbar_strip(ui, |ui| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            tr_s("Create"),
            16.0,
            egui::TextFormat {
                font_id: theme::body(),
                color: theme::TEXT,
                ..Default::default()
            },
        );
        let response = crate::controls::menu_button(
            ui,
            job,
            0.0,
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
        response.on_hover_text(tr_s("Create a new object in the scene"));
        let width = ui.available_width() - theme::CONTROL_H - ui.spacing().item_spacing.x;
        theme::search_field(ui, &mut editor.hierarchy.filter, tr_s("Search  (t:  tag:  layer:)"), width)
            .on_hover_text(tr_s(
                "Letters match object names in order (rl3 finds Rail L 3); t:Camera — by component type, tag:Player — by tag, layer:UI — by layer",
            ));
        panel_menu(editor, ui);
    });

    reveal_selection(editor);
    let rows = cached_rows(editor);
    let searching = !editor.hierarchy.filter.trim().is_empty();
    editor.hierarchy.matches = rows.iter().filter(|r| !r.context_only).count();
    if searching {
        search_summary(editor, ui);
    }
    let order: Vec<GameObjectId> = rows.iter().map(|r| r.id).collect();
    keyboard(editor, ui, &rows);
    let dragging = egui::DragAndDrop::payload::<DragObject>(ui.ctx()).map(|p| p.0);

    egui::ScrollArea::vertical()
        .id_salt("hierarchy_scroll")
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let origin = ui.cursor().top();
            ui.add_space(theme::SP_XS);
            scene_row(editor, ui, dragging);
            // Рисуются только строки в видимой части: 5 000 объектов прокручиваются так же
            // быстро, как 50 (остальное — пустое место той же высоты).
            let first = theme::SP_XS + theme::ROW;
            let start = (((viewport.min.y - first) / theme::ROW).floor().max(0.0) as usize).min(rows.len());
            let end = ((((viewport.max.y - first) / theme::ROW).ceil().max(0.0) as usize) + 1).min(rows.len());
            if let Some(target) = editor.hierarchy.scroll_to.take() {
                if let Some(index) = order.iter().position(|id| *id == target) {
                    let top = origin + first + index as f32 * theme::ROW;
                    let rect = Rect::from_min_size(pos2(ui.max_rect().left(), top), vec2(1.0, theme::ROW));
                    ui.scroll_to_rect(rect, None);
                }
            }
            ui.add_space(start as f32 * theme::ROW);
            for (index, row) in rows.iter().enumerate().take(end).skip(start) {
                show_row(editor, ui, row, index, dragging, searching, &order);
            }
            ui.add_space(rows.len().saturating_sub(end) as f32 * theme::ROW);
            if rows.is_empty() && searching {
                ui.add_space(theme::SP_L);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(tr_s("No objects match the search")).color(theme::TEXT_MUTED));
                });
            } else if rows.is_empty() {
                // Пустая сцена: куда нажать дальше.
                ui.add_space(theme::SP_L);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(tr_s("The scene is empty. Add objects with + Create or GameObject."))
                            .color(theme::TEXT_MUTED),
                    );
                    if theme::ghost_button(ui, tr_s("Getting Started…")).clicked() {
                        editor.show_getting_started(true);
                    }
                });
            }

            // Объект тащат к краю списка — список прокручивается сам.
            if dragging.is_some() {
                if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                    let clip = ui.clip_rect();
                    let edge = theme::ROW * 1.5;
                    if clip.x_range().contains(pointer.x) {
                        let speed = if pointer.y < clip.top() + edge && pointer.y > clip.top() - edge {
                            8.0
                        } else if pointer.y > clip.bottom() - edge && pointer.y < clip.bottom() + edge {
                            -8.0
                        } else {
                            0.0
                        };
                        if speed != 0.0 {
                            ui.scroll_with_delta(vec2(0.0, speed));
                            ui.ctx().request_repaint();
                        }
                    }
                }
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
            // `dnd_release_payload` забирает груз любого типа: сначала смотрим, чей он.
            asset_drop(editor, ui, &response, rest);
            if response.dnd_hover_payload::<DragObject>().is_some() {
                if let Some(payload) = response.dnd_release_payload::<DragObject>() {
                    editor.actions.push(Action::Reparent {
                        child: payload.0,
                        parent: None,
                    });
                }
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

/// Меню окна (кнопка «⋮» справа от поиска): раскрыть / свернуть всё, порядок, вид поиска.
fn panel_menu(editor: &mut Editor, ui: &mut egui::Ui) {
    let inner = crate::controls::menu_anchor(
        ui,
        vec2(theme::CONTROL_H, theme::CONTROL_H),
        false,
        |ui| {
            theme::menu_body(ui);
            if item(tr_s("Expand All")).icon(Icon::ChevronDown).show(ui) {
                editor.hierarchy.collapsed.clear();
            }
            if item(tr_s("Collapse All")).icon(Icon::ChevronRight).show(ui) {
                editor.collapse_hierarchy();
            }
            if item(tr_s("Reveal Selection"))
                .icon(Icon::Search)
                .enabled_if(editor.selection.is_some(), tr_s("Select an object first"))
                .show(ui)
            {
                editor.hierarchy.revealed = None;
            }
            theme::menu_separator(ui);
            theme::menu_section(ui, tr_s("Order"));
            if item(tr_s("Scene Order")).checked(!editor.hierarchy.sort_alpha).show(ui) {
                editor.hierarchy.sort_alpha = false;
            }
            if item(tr_s("Alphabetical")).checked(editor.hierarchy.sort_alpha).show(ui) {
                editor.hierarchy.sort_alpha = true;
            }
            theme::menu_separator(ui);
            theme::menu_section(ui, tr_s("Search Results"));
            if item(tr_s("In the Tree")).checked(!editor.hierarchy.flat_search).show(ui) {
                editor.hierarchy.flat_search = false;
            }
            if item(tr_s("Flat List")).checked(editor.hierarchy.flat_search).show(ui) {
                editor.hierarchy.flat_search = true;
            }
            theme::menu_separator(ui);
            if item(tr_s("Alternating Row Shading")).checked(!editor.hierarchy.plain_rows).show(ui) {
                editor.hierarchy.plain_rows = !editor.hierarchy.plain_rows;
            }
            if item(tr_s("Unlock All"))
                .icon(Icon::Unlock)
                .enabled_if(!editor.hierarchy.locked.is_empty(), tr_s("No object is locked"))
                .show(ui)
            {
                editor.hierarchy.locked.clear();
            }
        },
    );
    let rect = inner.response.rect;
    if inner.response.hovered() || inner.inner.is_some() {
        ui.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
    }
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(theme::ICON_S, theme::ICON_S)), Icon::More, theme::TEXT_MUTED);
    inner.response.on_hover_text(tr_s("Hierarchy options: expand or collapse all, order, search results"));
}

/// Строка над списком при поиске: сколько найдено и как показано.
fn search_summary(editor: &mut Editor, ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW), Sense::hover());
    let count = editor.hierarchy.matches;
    ui.painter().text(
        pos2(rect.left() + theme::SP_M, rect.center().y),
        Align2::LEFT_CENTER,
        trn!("{n} object found", "{n} objects found", count),
        theme::caption(),
        theme::TEXT_MUTED,
    );
    let label = if editor.hierarchy.flat_search { tr_s("Show in Tree") } else { tr_s("Show as List") };
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::caption(), theme::TEXT);
    let button = Rect::from_min_max(
        pos2(rect.right() - theme::SP_M - galley.size().x - theme::SP_M, rect.top() + 2.0),
        pos2(rect.right() - theme::SP_S, rect.bottom() - 2.0),
    );
    let response = ui.interact(button, egui::Id::new("hierarchy_search_mode"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(button, theme::R_S, theme::BG_HOVER);
    }
    ui.painter().galley(pos2(button.left() + theme::SP_S, button.center().y - galley.size().y * 0.5), galley, theme::TEXT);
    if response
        .on_hover_text(tr_s("Matches with their parents in the tree, or a flat list with the parent path"))
        .clicked()
    {
        editor.hierarchy.flat_search = !editor.hierarchy.flat_search;
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, theme::LINE));
}

/// Выделение сменилось (щелчок в виде Scene, поиск, Undo): раскрыть предков и прокрутить
/// список к объекту, как «Reveal» в Unity.
fn reveal_selection(editor: &mut Editor) {
    let selection = editor.selection;
    if editor.hierarchy.revealed == selection {
        return;
    }
    editor.hierarchy.revealed = selection;
    let Some(id) = selection else { return };
    let Some(path) = crate::state::path_of(&editor.app.world.scene, id) else { return };
    for depth in 1..path.len() {
        editor.hierarchy.collapsed.remove(&path[..depth]);
    }
    editor.hierarchy.scroll_to = Some(id);
}

/// Пути всех узлов с детьми в поддереве `id` (сам узел — `path`).
fn subtree_parents(scene: &Scene, id: GameObjectId, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    let Some(object) = scene.get(id) else { return };
    if object.children().is_empty() {
        return;
    }
    out.push(path.clone());
    for (index, child) in object.children().iter().enumerate() {
        path.push(index);
        subtree_parents(scene, *child, path, out);
        path.pop();
    }
}

impl Editor {
    /// Свернуть все узлы окна Hierarchy (меню окна → Collapse All).
    pub fn collapse_hierarchy(&mut self) {
        let scene = &self.app.world.scene;
        let mut paths = Vec::new();
        for (index, root) in scene.roots().iter().enumerate() {
            subtree_parents(scene, *root, &mut vec![index], &mut paths);
        }
        self.hierarchy.collapsed.extend(paths);
    }

    /// Раскрыть (`open`) или свернуть объект со всем поддеревом (Alt+щелчок по стрелке).
    pub(crate) fn set_subtree_open(&mut self, id: GameObjectId, open: bool) {
        let scene = &self.app.world.scene;
        let Some(mut path) = crate::state::path_of(scene, id) else { return };
        let mut paths = Vec::new();
        subtree_parents(scene, id, &mut path, &mut paths);
        for path in paths {
            if open {
                self.hierarchy.collapsed.remove(&path);
            } else {
                self.hierarchy.collapsed.insert(path);
            }
        }
    }

    /// В окне Hierarchy набирают имя (последняя буква меньше секунды назад): буквы идут в
    /// набор, а не в сочетания W / E / R / F / H.
    pub(crate) fn hierarchy_typing(&self, now: f64) -> bool {
        let (buffer, last) = &self.hierarchy.type_ahead;
        !buffer.is_empty() && now - last < 1.0 && self.focused_panel() == Some(crate::dock::Panel::Hierarchy)
    }

    /// Объект (или его предок) заперт замком иерархии: в виде Scene его не выбрать.
    pub(crate) fn pick_locked(&self, id: GameObjectId) -> bool {
        !self.hierarchy.locked.is_empty()
            && crate::state::path_of(&self.app.world.scene, id).is_some_and(|path| under_any(&self.hierarchy.locked, &path))
    }

    /// Запереть объект от выбора в виде Scene (`false` — отпереть).
    pub fn set_pick_locked(&mut self, id: GameObjectId, locked: bool) {
        let Some(path) = crate::state::path_of(&self.app.world.scene, id) else { return };
        if locked {
            self.hierarchy.locked.insert(path);
        } else {
            self.hierarchy.locked.remove(&path);
        }
    }

    /// Объекты в окне Hierarchy сверху вниз — то, что видно с учётом свёрнутых узлов,
    /// поиска и порядка (для тестов и инструментов).
    pub fn hierarchy_rows(&self) -> Vec<GameObjectId> {
        collect(self).into_iter().map(|row| row.id).collect()
    }

    /// Свёрнут ли объект в окне Hierarchy.
    pub fn is_hierarchy_collapsed(&self, id: GameObjectId) -> bool {
        crate::state::path_of(&self.app.world.scene, id).is_some_and(|path| self.hierarchy.collapsed.contains(&path))
    }
}

// Строка сцены над деревом: имя сцены; бросок на неё — объект в корень.
fn scene_row(editor: &mut Editor, ui: &mut egui::Ui, dragging: Option<GameObjectId>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::ROW), Sense::hover());
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
    // Имя сцены — выпадающий список сцен проекта (scene_picker.rs).
    let name = format!("{}{}", editor.app.world.scene.name, if editor.dirty { "*" } else { "" });
    crate::scene_picker::scene_switcher(editor, ui, rect, &name);
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

/// Ассет из окна Project, брошенный на пустое место иерархии: сцена открывается (с
/// вопросом о несохранённом), префаб создаётся в корне; остальное не принимается — это
/// видно по подсказке у курсора.
fn asset_drop(editor: &mut Editor, ui: &egui::Ui, response: &egui::Response, rect: Rect) {
    let Some(hovered) = response.dnd_hover_payload::<crate::project::DragAsset>() else { return };
    let accepted = matches!(hovered.importer.as_str(), "Scene" | "Prefab");
    let tip = match hovered.importer.as_str() {
        "Scene" => tr_s("Release to open this scene"),
        "Prefab" => tr_s("Release to add the prefab to the scene"),
        _ => tr_s("Only scenes and prefabs can be dropped into the Hierarchy"),
    };
    let color = if accepted { theme::accent() } else { theme::TEXT_DISABLED };
    ui.painter().rect_stroke(rect.shrink(1.0), theme::R_S, Stroke::new(1.5_f32, color));
    if let Some(position) = ui.ctx().pointer_latest_pos() {
        let icon = if accepted { Icon::Scene } else { Icon::Close };
        drag_label(ui.ctx(), position + vec2(0.0, 24.0), icon, tip);
    }
    if !accepted {
        return;
    }
    if let Some(asset) = response.dnd_release_payload::<crate::project::DragAsset>() {
        match asset.importer.as_str() {
            "Scene" => editor.actions.push(Action::OpenScene(asset.path.clone())),
            _ => editor.actions.push(Action::Instantiate { reference: asset.path.clone(), position: None }),
        }
    }
}

/// Вторая подпись у курсора — что будет, если отпустить (под именем объекта).
pub(crate) fn drag_hint(ctx: &egui::Context, text: &str) {
    if let Some(position) = ctx.pointer_latest_pos() {
        drag_label(ctx, position + vec2(0.0, 24.0), Icon::Prefab, text);
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
    crate::controls::paint_frame(&painter, rect, theme::R_S, theme::BG_RAISED, theme::LINE_STRONG);
    icons::paint(
        &painter,
        Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
        icon,
        theme::TEXT_MUTED,
    );
    painter.galley(pos2(rect.left() + 24.0, rect.center().y - galley.size().y * 0.5), galley, theme::TEXT);
}

fn show_row(
    editor: &mut Editor,
    ui: &mut egui::Ui,
    row: &Row,
    index: usize,
    dragging: Option<GameObjectId>,
    searching: bool,
    order: &[GameObjectId],
) {
    let id = row.id;
    let selected = editor.is_selected(id);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), theme::ROW), Sense::hover());
    let response = ui.interact(rect, Editor::hierarchy_row_id(id), Sense::click_and_drag());
    let painter = ui.painter().clone();

    // Фон: выделение (серое, когда фокус в другом окне), наведение, едва заметная «зебра»
    // (её выключает меню окна) — по ней глаз держит строку в длинных списках.
    if selected {
        let fill = if editor.selection_focused() { theme::selection_fill() } else { theme::selection_unfocused() };
        painter.rect_filled(rect, 0.0, fill);
    } else if response.hovered() && dragging.is_none() {
        painter.rect_filled(rect, 0.0, theme::hover_fill());
    } else if index % 2 == 1 && !editor.hierarchy.plain_rows {
        painter.rect_filled(rect, 0.0, theme::BG_ROW_ALT);
    }
    // Щелчок по полю-ссылке в инспекторе подсвечивает цель (inspector_refs.rs).
    crate::inspector_refs::paint_ping(ui, rect, id);

    // Направляющие уровней.
    let base = rect.left() + theme::SP_S + 2.0;
    let guide = Stroke::new(1.0_f32, theme::LINE_GUIDE);
    for level in 0..row.depth {
        let x = (base + level as f32 * INDENT + INDENT * 0.5).round() + 0.5;
        if level + 1 == row.depth {
            let bottom = if row.last { rect.center().y } else { rect.bottom() };
            painter.vline(x, rect.top()..=bottom, guide);
        } else if row.guides.get(level + 1).copied().unwrap_or(false) {
            painter.vline(x, rect.top()..=rect.bottom(), guide);
        }
    }
    if row.depth > 0 {
        let x = (base + (row.depth - 1) as f32 * INDENT + INDENT * 0.5).round() + 0.5;
        painter.hline(x..=x + INDENT * 0.5 - 1.0, rect.center().y.round() + 0.5, guide);
    }

    // Стрелка раскрытия. Alt+щелчок — раскрыть или свернуть всё поддерево.
    let left = base + row.depth as f32 * INDENT;
    let arrow_rect = Rect::from_min_size(pos2(left, rect.top()), vec2(INDENT, theme::ROW));
    if row.has_children {
        let arrow = ui.interact(arrow_rect, Editor::hierarchy_row_id(id).with("arrow"), Sense::click());
        if arrow.hovered() {
            painter.rect_filled(arrow_rect.shrink2(vec2(0.0, 3.0)), theme::R_S, theme::BG_HOVER);
        }
        let color = if arrow.hovered() { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
        icons::paint(
            &painter,
            Rect::from_center_size(arrow_rect.center(), vec2(9.0, 9.0)),
            if row.open { Icon::ChevronDown } else { Icon::ChevronRight },
            color,
        );
        let arrow = arrow.on_hover_text(tr_s("Expand or collapse  (Alt+click: with all children)"));
        if arrow.clicked() {
            if ui.input(|i| i.modifiers.alt) {
                editor.set_subtree_open(id, !row.open);
            } else if !searching {
                let state = &mut editor.hierarchy;
                if row.open {
                    state.collapsed.insert(row.path.clone());
                } else {
                    state.collapsed.remove(&row.path);
                }
            }
        }
    }

    // Иконка и имя.
    let dim = |color: Color32| if row.active && !row.context_only { color } else { color.gamma_multiply(0.45) };
    let icon_color = if row.prefab {
        theme::PREFAB
    } else if selected {
        theme::TEXT_STRONG
    } else {
        theme::mix(row.icon_color, theme::BG_PANEL, 0.2)
    };
    let icon_rect = Rect::from_center_size(pos2(arrow_rect.right() + 7.0, rect.center().y), vec2(13.0, 13.0));
    icons::paint(&painter, icon_rect, row.icon, dim(icon_color));

    // Справа: замок (не выбирать в виде Scene) и глаз (объект включён). Видны при
    // наведении и остаются, когда включены.
    let eye_rect = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(20.0, 20.0));
    let lock_rect = eye_rect.translate(vec2(-20.0, 0.0));
    // По положению курсора, а не `hovered()`: над замком «наведена» уже не строка.
    let show_lock = response.contains_pointer() || row.locked || row.lock_inherited;
    let mut text_right = if show_lock { lock_rect.left() } else { eye_rect.left() } - 2.0;
    // Число детей у свёрнутого родителя.
    if row.has_children && !row.open {
        let count = row.child_count.to_string();
        let galley = painter.layout_no_wrap(count, theme::caption(), theme::TEXT_DISABLED);
        let x = text_right - galley.size().x - theme::SP_S;
        painter.galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley, theme::TEXT_DISABLED);
        text_right = x - theme::SP_S;
    }
    let text_rect = Rect::from_min_max(pos2(icon_rect.right() + 5.0, rect.top()), pos2(text_right.max(icon_rect.right() + 6.0), rect.bottom()));
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
        } else if row.context_only {
            theme::TEXT_MUTED
        } else {
            theme::TEXT
        };
        // Длинное имя обрезается «…», полное — в подсказке; совпадения поиска подсвечены.
        let name = name_galley(&painter, &row.name, &row.highlight, dim(color), text_rect.width());
        let elided = name.elided;
        let name_width = name.size().x;
        let clipped = painter.with_clip_rect(text_rect);
        clipped.galley(pos2(text_rect.left(), rect.center().y - name.size().y * 0.5), name, dim(color));
        if elided && response.hovered() {
            response.clone().on_hover_text(row.name.clone());
        }
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

    // Замок.
    {
        let lock = ui.interact(lock_rect, Editor::hierarchy_row_id(id).with("lock"), Sense::click());
        if lock.hovered() {
            painter.rect_filled(lock_rect, theme::R_S, theme::BG_HOVER);
        }
        let color = match (lock.hovered(), row.locked) {
            (true, _) => theme::TEXT_STRONG,
            (false, true) => theme::TEXT,
            (false, false) => theme::TEXT_DISABLED,
        };
        let icon = if row.locked || row.lock_inherited { Icon::Lock } else { Icon::Unlock };
        if show_lock || lock.hovered() {
            icons::paint(&painter, lock_rect.shrink(4.0), icon, color);
        }
        let tip = if row.locked {
            tr_s("Locked: cannot be picked in the Scene view. Click to unlock")
        } else if row.lock_inherited {
            tr_s("A parent is locked: this object cannot be picked in the Scene view")
        } else {
            tr_s("Lock: the object and its children cannot be picked in the Scene view")
        };
        if lock.on_hover_text(tip).clicked() {
            editor.set_pick_locked(id, !row.locked);
        }
    }

    // Глаз: включён ли объект. Виден при наведении и у выключенных.
    let eye = ui.interact(eye_rect, Editor::hierarchy_row_id(id).with("active"), Sense::click());
    if response.contains_pointer() || !row.active_self {
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
    let eye_tip = if row.active_self { tr_s("Deactivate the object") } else { tr_s("Activate the object") };
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
    // Тащат над свёрнутым родителем — через полсекунды он раскрывается.
    if drop == Some(Drop::Into) && row.has_children && !row.open {
        let now = ui.input(|i| i.time);
        match &editor.hierarchy.hover_expand {
            Some((path, since)) if *path == row.path => {
                if now - since > 0.5 {
                    editor.hierarchy.collapsed.remove(&row.path);
                    editor.hierarchy.hover_expand = None;
                }
            }
            _ => editor.hierarchy.hover_expand = Some((row.path.clone(), now)),
        }
        ui.ctx().request_repaint();
    }
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

    // Взаимодействие: щелчок — выбрать, Ctrl — добавить / убрать, Shift — диапазон от
    // активного, Ctrl+Shift — добавить диапазон. Двойной щелчок — показать в виде Scene.
    let modifiers = ui.input(|i| i.modifiers);
    if response.clicked() {
        editor.actions.push(click_action(editor, id, modifiers, order));
    }
    if response.secondary_clicked() && !selected {
        editor.actions.push(Action::Select(Some(id)));
    }
    // egui считает двойным и быстрый Ctrl / Shift-щелчок по соседней строке: кадрируем
    // только по двойному щелчку без модификаторов.
    if response.double_clicked() && modifiers.is_none() {
        editor.actions.push(Action::Select(Some(id)));
        editor.actions.push(Action::FrameSelected);
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
        // Тащат выделенный объект — едет всё выделение (верхние уровни, по порядку).
        let moved: Vec<GameObjectId> = if editor.is_selected(payload.0) {
            editor.top_level_selected()
        } else {
            vec![payload.0]
        };
        let place = drop.unwrap_or(Drop::Into);
        let mut index = row.sibling_index + usize::from(place == Drop::After);
        for child in moved.into_iter().filter(|child| *child != id) {
            match place {
                Drop::Into => {
                    editor.actions.push(Action::Reparent { child, parent: Some(id) });
                    editor.hierarchy.expand(Some(row.path.clone()));
                }
                _ => {
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
                    index += 1;
                }
            }
        }
    }
    response.context_menu(|ui| {
        theme::menu_body(ui);
        context_menu(editor, ui, row);
    });
}

/// Действие щелчка по строке с модификаторами (см. `show_row`).
fn click_action(editor: &Editor, id: GameObjectId, modifiers: egui::Modifiers, order: &[GameObjectId]) -> Action {
    let ctrl = modifiers.command || modifiers.ctrl;
    if modifiers.shift {
        let anchor = editor.selection().and_then(|a| order.iter().position(|o| *o == a));
        let Some(here) = order.iter().position(|o| *o == id) else {
            return Action::Select(Some(id));
        };
        let from = anchor.unwrap_or(here);
        let (low, high) = (from.min(here), from.max(here));
        let mut ids: Vec<GameObjectId> = if ctrl { editor.selected() } else { Vec::new() };
        ids.extend_from_slice(&order[low..=high]);
        // Активный остаётся якорем, чтобы следующий Shift+щелчок считался от него.
        return Action::SelectMany { ids, active: editor.selection().or(Some(id)) };
    }
    if ctrl {
        return Action::ToggleSelect(id);
    }
    Action::Select(Some(id))
}

/// Имя строки: обрезка «…» по ширине, совпадения поиска — светлее и на подложке акцента.
fn name_galley(
    painter: &egui::Painter,
    name: &str,
    highlight: &[usize],
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat { font_id: theme::body(), color, ..Default::default() };
    let marked = egui::TextFormat {
        font_id: theme::body(),
        color: theme::TEXT_STRONG,
        background: theme::mix(theme::BG_PANEL, theme::accent(), 0.35),
        ..Default::default()
    };
    if highlight.is_empty() {
        job.append(name, 0.0, plain);
    } else {
        let mut run = String::new();
        let mut run_marked = false;
        for (index, c) in name.chars().enumerate() {
            let is_marked = highlight.binary_search(&index).is_ok();
            if is_marked != run_marked && !run.is_empty() {
                job.append(&run, 0.0, if run_marked { marked.clone() } else { plain.clone() });
                run.clear();
            }
            run_marked = is_marked;
            run.push(c);
        }
        if !run.is_empty() {
            job.append(&run, 0.0, if run_marked { marked } else { plain });
        }
    }
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

/// Клавиатура окна Hierarchy (когда оно в фокусе): вверх / вниз — соседняя строка (с
/// Shift — расширить выделение); вправо — раскрыть, ещё раз — к первому ребёнку; влево —
/// свернуть, ещё раз — к родителю; буквы — переход к строке, имя которой так начинается.
/// Первая буква, занятая сочетанием редактора (W, E, R, F, H, Z), остаётся сочетанием;
/// пока набор идёт (меньше секунды между буквами), все буквы — в набор.
fn keyboard(editor: &mut Editor, ui: &egui::Ui, rows: &[Row]) {
    if editor.focused_panel() != Some(crate::dock::Panel::Hierarchy) || ui.ctx().wants_keyboard_input() {
        return;
    }
    let order: Vec<GameObjectId> = rows.iter().map(|r| r.id).collect();
    let current = editor.selection().and_then(|a| order.iter().position(|o| *o == a));
    let none = egui::Modifiers::NONE;
    let (up, down, shift, left, right, typed, time) = ui.input_mut(|i| {
        let shift = i.modifiers.shift;
        let m = if shift { egui::Modifiers::SHIFT } else { none };
        let up = i.consume_key(m, egui::Key::ArrowUp);
        let down = i.consume_key(m, egui::Key::ArrowDown);
        let left = i.consume_key(none, egui::Key::ArrowLeft);
        let right = i.consume_key(none, egui::Key::ArrowRight);
        // Буква — набор имени, только если её нажатие не забрало сочетание клавиш.
        let pressed: Vec<egui::Key> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.command && !modifiers.alt => Some(*key),
                _ => None,
            })
            .collect();
        let typed: String = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .flat_map(str::chars)
            .filter(|c| {
                !c.is_control()
                    && (c.is_whitespace()
                        || egui::Key::from_name(&c.to_uppercase().to_string()).is_none_or(|key| pressed.contains(&key)))
            })
            .collect();
        (up, down, shift, left, right, typed, i.time)
    });

    if left || right {
        let Some(at) = current else { return };
        let row = &rows[at];
        if right {
            if row.has_children && !row.open {
                editor.hierarchy.collapsed.remove(&row.path);
            } else if row.open {
                if let Some(child) = order.get(at + 1) {
                    editor.actions.push(Action::Select(Some(*child)));
                }
            }
        } else if row.open {
            editor.hierarchy.collapsed.insert(row.path.clone());
        } else if let Some(parent) = row.parent.filter(|p| order.contains(p)) {
            editor.actions.push(Action::Select(Some(parent)));
        }
        return;
    }

    if !typed.is_empty() {
        let (buffer, last) = &mut editor.hierarchy.type_ahead;
        if time - *last > 1.0 {
            buffer.clear();
        }
        *last = time;
        buffer.push_str(&typed.to_lowercase());
        let prefix = buffer.clone();
        // Сначала от текущей строки (повтор той же буквы — следующий объект), потом с начала.
        let from = current.map_or(0, |at| if prefix.chars().count() == 1 { at + 1 } else { at });
        let hit = (from..rows.len())
            .chain(0..from)
            .find(|&i| rows[i].name.to_lowercase().starts_with(&prefix));
        if let Some(i) = hit {
            editor.actions.push(Action::Select(Some(rows[i].id)));
            editor.hierarchy.scroll_to = Some(rows[i].id);
        }
        return;
    }

    if !up && !down {
        return;
    }
    let next = match (current, up) {
        (Some(at), true) => at.saturating_sub(1),
        (Some(at), false) => (at + 1).min(order.len().saturating_sub(1)),
        (None, _) => 0,
    };
    let Some(target) = order.get(next).copied() else { return };
    editor.hierarchy.scroll_to = Some(target);
    if shift {
        let mut ids = editor.selected();
        ids.push(target);
        editor.actions.push(Action::SelectMany { ids, active: Some(target) });
    } else {
        editor.actions.push(Action::Select(Some(target)));
    }
}

/// «Edit in Modeler» / «Edit a Copy in Modeler» для файла модели (меню Hierarchy, Scene и
/// поля Mesh инспектора); без программы Piska Modeler — неактивен с подсказкой.
pub(crate) fn model_menu_item(editor: &mut Editor, ui: &mut egui::Ui, file: &std::path::Path) {
    let Some(entry) = crate::modeler_link::ModelerEntry::for_path(file) else { return };
    let installed = crate::modeler_link::modeler_executable().is_some();
    if item(tr_s(entry.label())).icon(Icon::Cube).enabled_if(installed, tr_s(crate::modeler_link::MODELER_MISSING)).show(ui) {
        editor.actions.push(match entry {
            crate::modeler_link::ModelerEntry::Edit => Action::EditInModeler(file.to_path_buf()),
            crate::modeler_link::ModelerEntry::EditCopy => Action::EditCopyInModeler(file.to_path_buf()),
        });
    }
}

fn context_menu(editor: &mut Editor, ui: &mut egui::Ui, row: &Row) {
    let id = row.id;
    theme::submenu(ui, Icon::Plus, tr_s("Create Child"), |ui| editor.gameobject_menu(ui, Some(id)));
    theme::menu_separator(ui);
    if item(tr_s("Rename")).icon(Icon::Text).shortcut(&editor.shortcut_text("rename")).show(ui) {
        editor.hierarchy.begin_rename(id, &row.name);
    }
    if item(tr_s("Duplicate")).icon(Icon::Copy).shortcut(&editor.shortcut_text("duplicate")).show(ui) {
        editor.actions.push(Action::DuplicateSelection);
    }
    if item(tr_s("Delete")).icon(Icon::Trash).shortcut(&editor.shortcut_text("delete")).show(ui) {
        editor.actions.push(Action::DeleteSelection);
    }
    theme::menu_separator(ui);
    if item(tr_s("Copy")).shortcut(&editor.shortcut_text("copy")).show(ui) {
        editor.actions.push(Action::CopySelection);
    }
    if item(tr_s("Cut")).shortcut(&editor.shortcut_text("cut")).show(ui) {
        editor.actions.push(Action::CutSelection);
    }
    let can_paste = editor.object_clipboard_len() > 0;
    if item(tr_s("Paste")).shortcut(&editor.shortcut_text("paste")).enabled_if(can_paste, tr_s("Copy objects first (Ctrl+C)")).show(ui) {
        editor.actions.push(Action::Paste { as_child: false });
    }
    if item(tr_s("Paste as Child"))
        .shortcut(&editor.shortcut_text("paste-child"))
        .enabled_if(can_paste, tr_s("Copy objects first (Ctrl+C)"))
        .show(ui)
    {
        editor.actions.push(Action::Paste { as_child: true });
    }
    if item(tr_s("Group Selection")).icon(Icon::Hierarchy).shortcut(&editor.shortcut_text("group")).show(ui) {
        editor.actions.push(Action::GroupSelection);
    }
    theme::menu_separator(ui);
    let (label, icon) = if row.active_self { (tr_s("Deactivate"), Icon::EyeOff) } else { (tr_s("Activate"), Icon::Eye) };
    if item(label).icon(icon).show(ui) {
        editor.actions.push(Action::SetActive { id, active: !row.active_self });
    }
    if item(tr_s("Frame in Scene View")).icon(Icon::Search).shortcut(&editor.shortcut_text("frame-selected")).show(ui) {
        editor.actions.push(Action::Select(Some(id)));
        editor.actions.push(Action::FrameSelected);
    }
    let (lock_label, lock_icon) =
        if row.locked { (tr_s("Unlock in Scene View"), Icon::Unlock) } else { (tr_s("Lock in Scene View"), Icon::Lock) };
    if item(lock_label).icon(lock_icon).show(ui) {
        editor.set_pick_locked(id, !row.locked);
    }
    // Модель объекта (MeshRenderer с .pmodel / .obj / .glb) — в Piska Modeler.
    if let Some(file) = editor.object_model_file(id) {
        model_menu_item(editor, ui, &file);
    }
    if row.has_children || row.child_count > 0 {
        if item(tr_s("Expand All Children")).icon(Icon::ChevronDown).show(ui) {
            editor.set_subtree_open(id, true);
        }
        if item(tr_s("Collapse All Children")).icon(Icon::ChevronRight).show(ui) {
            editor.set_subtree_open(id, false);
        }
    }
    theme::menu_separator(ui);
    if item(tr_s("Move Up")).enabled_if(row.sibling_index > 0, tr_s("Already the first")).show(ui) {
        editor.actions.push(Action::MoveSibling { id, delta: -1 });
    }
    if item(tr_s("Move Down")).show(ui) {
        editor.actions.push(Action::MoveSibling { id, delta: 1 });
    }
    if item(tr_s("Move to Root")).enabled_if(row.parent.is_some(), tr_s("Already at the root")).show(ui) {
        editor.actions.push(Action::Reparent { child: id, parent: None });
    }
    let children = editor.app.world.scene.get(id).map_or(0, |o| o.children().len());
    if item(tr_s("Sort Children A-Z"))
        .enabled_if(children > 1, tr_s("Needs at least two children"))
        .show(ui)
    {
        editor.actions.push(Action::SortChildren { parent: Some(id) });
    }
    theme::menu_separator(ui);
    if row.prefab {
        if item(tr_s("Unpack Prefab")).icon(Icon::Prefab).show(ui) {
            editor.actions.push(Action::UnpackPrefab(id));
        }
    } else if item(tr_s("Save as Prefab"))
        .tinted(Icon::Prefab, theme::PREFAB)
        .enabled_if(editor.assets_root().is_some(), tr_s("No assets directory is mounted"))
        .show(ui)
    {
        editor.actions.push(Action::SavePrefab(id));
    }
}
