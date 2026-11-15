//! Окна редактора как в Unity: каждая панель — вкладка в области дока.
//!
//! Вкладку можно перетащить в другую полосу вкладок, к краю области (разделить её),
//! к краю окна редактора (разделить всё) или бросить подальше — получится плавающее
//! окно. Разделители тянутся, двойной щелчок по вкладке или Shift+Space над панелью
//! разворачивают её на всю область. Раскладка (вместе с плавающими окнами)
//! сохраняется в настройках редактора, именованные раскладки — тоже.
//!
//! Дерево дока и его отрисовку даёт `egui_dock`; здесь — модель панелей, готовые
//! раскладки Unity, сохранение в текст, разворот, меню вкладок и стыковка к краю окна.
//! Плавающие окна живут внутри главного окна (окна egui), отдельных окон ОС нет.

use crate::i18n::{tr_s, trf};
use std::collections::HashSet;

use egui::{pos2, vec2, Color32, Id, Margin, Pos2, Rect, Rounding, Stroke};
use egui_dock::{
    DockArea, DockState, Node, NodeIndex, OverlayType, Split, Style, Surface, SurfaceIndex, TabIndex,
    TabInteractionStyle, TabViewer, Tree,
};

use crate::icons::{self, Icon};
use crate::state::{BottomTab, Editor, Mode, ViewTab};
use crate::theme::{self, item};
use crate::EditorError;

/// Окно (панель) редактора. Каждое — вкладка в доке.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    Hierarchy,
    Inspector,
    /// Вид Scene: камера редактора, сетка, гизмо.
    Scene,
    /// Вид Game: то, что видит камера игры.
    Game,
    /// Редактор кода.
    Code,
    Project,
    Console,
    /// Ошибки и предупреждения кода (проверка при наборе и компилятор).
    ErrorList,
    /// Find in Files и Find All References.
    FindResults,
    Profiler,
}

impl Panel {
    /// Все окна в порядке меню Window.
    pub const ALL: [Panel; 10] = [
        Panel::Scene,
        Panel::Game,
        Panel::Code,
        Panel::Hierarchy,
        Panel::Inspector,
        Panel::Project,
        Panel::Console,
        Panel::ErrorList,
        Panel::FindResults,
        Panel::Profiler,
    ];

    /// Заголовок вкладки.
    pub fn title(self) -> &'static str {
        match self {
            Panel::Hierarchy => tr_s("Hierarchy"),
            Panel::Inspector => tr_s("Inspector"),
            Panel::Scene => tr_s("Scene"),
            Panel::Game => tr_s("Game"),
            Panel::Code => tr_s("Code"),
            Panel::Project => tr_s("Project"),
            Panel::Console => tr_s("Console"),
            Panel::ErrorList => tr_s("Error List"),
            Panel::FindResults => tr_s("Find Results"),
            Panel::Profiler => tr_s("Profiler"),
        }
    }

    pub(crate) fn icon(self) -> Icon {
        match self {
            Panel::Hierarchy => Icon::Hierarchy,
            Panel::Inspector => Icon::Inspector,
            Panel::Scene => Icon::Scene,
            Panel::Game => Icon::Play,
            Panel::Code => Icon::Code,
            Panel::Project => Icon::Folder,
            Panel::Console => Icon::Console,
            Panel::ErrorList => Icon::Error,
            Panel::FindResults => Icon::Search,
            Panel::Profiler => Icon::Chart,
        }
    }

    /// Имя в сохранённой раскладке.
    fn key(self) -> &'static str {
        match self {
            Panel::Hierarchy => "hierarchy",
            Panel::Inspector => "inspector",
            Panel::Scene => "scene",
            Panel::Game => "game",
            Panel::Code => "code",
            Panel::Project => "project",
            Panel::Console => "console",
            Panel::ErrorList => "error-list",
            Panel::FindResults => "find-results",
            Panel::Profiler => "profiler",
        }
    }

    fn from_key(key: &str) -> Option<Panel> {
        Panel::ALL.into_iter().find(|panel| panel.key() == key)
    }

    /// Окна, которые обычно живут в одной полосе вкладок.
    fn group(self) -> &'static [Panel] {
        match self {
            Panel::Scene | Panel::Game | Panel::Code => &[Panel::Scene, Panel::Game, Panel::Code],
            Panel::Project | Panel::Console | Panel::ErrorList | Panel::FindResults | Panel::Profiler => {
                &[Panel::Project, Panel::Console, Panel::ErrorList, Panel::FindResults, Panel::Profiler]
            }
            Panel::Hierarchy => &[Panel::Hierarchy],
            Panel::Inspector => &[Panel::Inspector],
        }
    }
}

/// Готовые раскладки из меню Window → Layouts (как в Unity).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutPreset {
    /// Hierarchy слева и Inspector справа во всю высоту, Scene / Game / Code в центре,
    /// Project / Console / Profiler под ними.
    Default,
    /// Scene над Game слева, дальше колонки Hierarchy, Project, Inspector.
    TwoByThree,
    /// Высокий Scene / Game слева, Hierarchy над Project, Inspector справа.
    Tall,
    /// Широкий Scene / Game сверху, внизу Hierarchy, Project, Console; Inspector справа.
    Wide,
}

impl LayoutPreset {
    pub const ALL: [LayoutPreset; 4] = [
        LayoutPreset::Default,
        LayoutPreset::TwoByThree,
        LayoutPreset::Tall,
        LayoutPreset::Wide,
    ];

    /// Название в меню.
    pub fn name(self) -> &'static str {
        match self {
            LayoutPreset::Default => tr_s("Default"),
            LayoutPreset::TwoByThree => "2 by 3",
            LayoutPreset::Tall => tr_s("Tall"),
            LayoutPreset::Wide => tr_s("Wide"),
        }
    }

    fn desc(self) -> LayoutDesc {
        use Panel::*;
        let views = || leaf(&[Scene, Game, Code]);
        let bottom = || leaf(&[Project, Console, ErrorList, FindResults, Profiler]);
        let main = match self {
            LayoutPreset::Default => split_h(
                0.19,
                leaf(&[Hierarchy]),
                split_h(0.7, split_v(0.7, views(), bottom()), leaf(&[Inspector])),
            ),
            LayoutPreset::TwoByThree => split_h(
                0.42,
                split_v(0.5, leaf(&[Scene, Code]), leaf(&[Game, Profiler])),
                split_h(0.27, leaf(&[Hierarchy]), split_h(0.52, leaf(&[Project, Console, ErrorList, FindResults]), leaf(&[Inspector]))),
            ),
            LayoutPreset::Tall => split_h(
                0.5,
                views(),
                split_h(0.56, split_v(0.45, leaf(&[Hierarchy]), bottom()), leaf(&[Inspector])),
            ),
            LayoutPreset::Wide => split_h(
                0.76,
                split_v(
                    0.62,
                    views(),
                    split_h(0.3, leaf(&[Hierarchy]), split_h(0.55, leaf(&[Project, Profiler]), leaf(&[Console, ErrorList, FindResults]))),
                ),
                leaf(&[Inspector]),
            ),
        };
        LayoutDesc {
            main: Some(main),
            windows: Vec::new(),
            maximize_on_play: false,
        }
    }
}

/// Куда пристыковать окно относительно другого (или края окна редактора).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockSide {
    Left,
    Right,
    Top,
    Bottom,
    /// Вкладкой в ту же полосу.
    Center,
}

/// Действия с окнами — то, что делают меню Window, меню вкладок и мышь.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutAction {
    /// Открыть окно (или показать, если уже открыто).
    Open(Panel),
    /// Закрыть вкладку. Последняя вкладка области убирает и область.
    Close(Panel),
    /// Показать окно и сделать его активным.
    Focus(Panel),
    /// Развернуть на всю область дока или вернуть обратно.
    ToggleMaximize(Panel),
    /// Вынести в плавающее окно.
    Float(Panel),
    /// Пристыковать окно к другому: сбоку (разделить область) или вкладкой.
    Dock { panel: Panel, target: Panel, side: DockSide },
    /// Пристыковать окно к краю окна редактора.
    DockToEdge { panel: Panel, side: DockSide },
    /// Готовая раскладка.
    Preset(LayoutPreset),
    /// Сохранить текущую раскладку под именем (в настройках редактора).
    Save(String),
    /// Загрузить сохранённую раскладку.
    Load(String),
    /// Удалить сохранённую раскладку.
    Delete(String),
    /// Вернуть раскладку Default.
    Reset,
    /// Разворачивать Game при входе в Play.
    MaximizeOnPlay(bool),
}

// ---------------------------------------------------------------------------
// Описание раскладки: дерево, текст, сборка DockState
// ---------------------------------------------------------------------------

/// Узел раскладки без состояния отрисовки.
#[derive(Clone, Debug, PartialEq)]
enum Desc {
    Leaf { tabs: Vec<Panel>, active: usize },
    /// `vertical` — первый узел сверху, иначе слева; `fraction` — его доля.
    Split { vertical: bool, fraction: f32, first: Box<Desc>, second: Box<Desc> },
}

fn leaf(tabs: &[Panel]) -> Desc {
    Desc::Leaf { tabs: tabs.to_vec(), active: 0 }
}

fn split_h(fraction: f32, first: Desc, second: Desc) -> Desc {
    Desc::Split { vertical: false, fraction, first: Box::new(first), second: Box::new(second) }
}

fn split_v(fraction: f32, first: Desc, second: Desc) -> Desc {
    Desc::Split { vertical: true, fraction, first: Box::new(first), second: Box::new(second) }
}

impl Desc {
    /// Вкладки самого левого (верхнего) листа: с них начинается сборка дерева.
    fn first_tabs(&self) -> Vec<Panel> {
        match self {
            Desc::Leaf { tabs, .. } => tabs.clone(),
            Desc::Split { first, .. } => first.first_tabs(),
        }
    }

    fn panels(&self, out: &mut Vec<Panel>) {
        match self {
            Desc::Leaf { tabs, .. } => out.extend(tabs.iter().copied()),
            Desc::Split { first, second, .. } => {
                first.panels(out);
                second.panels(out);
            }
        }
    }

    fn write(&self, out: &mut String) {
        match self {
            Desc::Leaf { tabs, active } => {
                out.push_str(&format!("(L {active}"));
                for tab in tabs {
                    out.push(' ');
                    out.push_str(tab.key());
                }
                out.push(')');
            }
            Desc::Split { vertical, fraction, first, second } => {
                out.push_str(&format!("({} {fraction:.4} ", if *vertical { "V" } else { "H" }));
                first.write(out);
                out.push(' ');
                second.write(out);
                out.push(')');
            }
        }
    }

    fn parse(tokens: &mut std::iter::Peekable<std::vec::IntoIter<String>>, depth: usize) -> Result<Desc, String> {
        if depth > 24 {
            return Err("the layout tree is too deep".to_string());
        }
        fn next(tokens: &mut std::iter::Peekable<std::vec::IntoIter<String>>) -> Result<String, String> {
            tokens.next().ok_or_else(|| "the layout ends too early".to_string())
        }
        if next(tokens)? != "(" {
            return Err("expected `(`".to_string());
        }
        let kind = next(tokens)?;
        let desc = match kind.as_str() {
            "L" => {
                let active: usize = next(tokens)?.parse().map_err(|_| "a tab index is not a number".to_string())?;
                let mut tabs = Vec::new();
                while tokens.peek().is_some_and(|t| t != ")") {
                    let key = tokens.next().unwrap_or_default();
                    tabs.push(Panel::from_key(&key).ok_or_else(|| trf!("unknown window `{key}`", key = key))?);
                }
                if tabs.is_empty() {
                    return Err("an area without tabs".to_string());
                }
                Desc::Leaf { active: active.min(tabs.len() - 1), tabs }
            }
            "H" | "V" => {
                let fraction: f32 = next(tokens)?.parse().map_err(|_| "a split fraction is not a number".to_string())?;
                if !fraction.is_finite() {
                    return Err("a split fraction is not finite".to_string());
                }
                let first = Desc::parse(tokens, depth + 1)?;
                let second = Desc::parse(tokens, depth + 1)?;
                Desc::Split {
                    vertical: kind == "V",
                    fraction: fraction.clamp(0.05, 0.95),
                    first: Box::new(first),
                    second: Box::new(second),
                }
            }
            other => return Err(trf!("unknown layout node `{other}`", other = other)),
        };
        if tokens.next().as_deref() != Some(")") {
            return Err("expected `)`".to_string());
        }
        Ok(desc)
    }

    /// Описание узла `index` дерева дока.
    fn of_tree(tree: &Tree<Panel>, index: NodeIndex) -> Option<Desc> {
        if index.0 >= tree.len() {
            return None;
        }
        match &tree[index] {
            Node::Empty => None,
            Node::Leaf { tabs, active, .. } => (!tabs.is_empty()).then(|| Desc::Leaf {
                tabs: tabs.clone(),
                active: active.0.min(tabs.len().saturating_sub(1)),
            }),
            Node::Horizontal { fraction, .. } | Node::Vertical { fraction, .. } => {
                let vertical = tree[index].is_vertical();
                let first = Desc::of_tree(tree, index.left());
                let second = Desc::of_tree(tree, index.right());
                match (first, second) {
                    (Some(first), Some(second)) => Some(Desc::Split {
                        vertical,
                        fraction: *fraction,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                    (one, other) => one.or(other),
                }
            }
        }
    }

    /// Достроить дерево: в узле `at` уже лежит лист с `self.first_tabs()`.
    fn build(&self, tree: &mut Tree<Panel>, at: NodeIndex) {
        match self {
            Desc::Leaf { active, .. } => tree.set_active_tab(at, TabIndex(*active)),
            Desc::Split { vertical, fraction, first, second } => {
                let split = if *vertical { Split::Below } else { Split::Right };
                let [old, new] = tree.split(at, split, *fraction, Node::leaf_with(second.first_tabs()));
                first.build(tree, old);
                second.build(tree, new);
            }
        }
    }
}

/// Раскладка целиком: главная область, плавающие окна, флажок Maximize On Play.
#[derive(Clone, Debug, PartialEq)]
struct LayoutDesc {
    main: Option<Desc>,
    windows: Vec<(Rect, Desc)>,
    maximize_on_play: bool,
}

/// Первая строка сохранённой раскладки: формат и его версия.
const LAYOUT_HEADER: &str = "piska-layout 1";

impl LayoutDesc {
    /// Текст в одну строку без `,` и `:` — так он ложится в файл настроек.
    fn to_text(&self) -> String {
        let mut out = String::from(LAYOUT_HEADER);
        out.push_str("; main ");
        match &self.main {
            Some(desc) => desc.write(&mut out),
            None => out.push('-'),
        }
        for (rect, desc) in &self.windows {
            out.push_str(&format!(
                "; window {} {} {} {} ",
                format!("{:.0}", rect.min.x),
                format!("{:.0}", rect.min.y),
                format!("{:.0}", rect.width()),
                format!("{:.0}", rect.height())
            ));
            desc.write(&mut out);
        }
        out.push_str(&format!("; maxplay {}", u8::from(self.maximize_on_play)));
        out
    }

    fn parse(text: &str) -> Result<LayoutDesc, String> {
        let mut sections = text.split(';').map(str::trim);
        if sections.next() != Some(LAYOUT_HEADER) {
            return Err("the layout was saved by another version of the editor".to_string());
        }
        let tokens = |text: &str| -> std::iter::Peekable<std::vec::IntoIter<String>> {
            text.replace('(', " ( ")
                .replace(')', " ) ")
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
                .peekable()
        };
        let mut layout = LayoutDesc { main: None, windows: Vec::new(), maximize_on_play: false };
        let mut seen_main = false;
        for section in sections.filter(|s| !s.is_empty()) {
            let (key, rest) = section.split_once(' ').unwrap_or((section, ""));
            match key {
                "main" => {
                    seen_main = true;
                    if rest.trim() != "-" {
                        let mut tokens = tokens(rest);
                        layout.main = Some(Desc::parse(&mut tokens, 0)?);
                        if tokens.next().is_some() {
                            return Err("unexpected text after the main area".to_string());
                        }
                    }
                }
                "window" => {
                    let mut tokens = tokens(rest);
                    let mut number = || -> Result<f32, String> {
                        tokens
                            .next()
                            .and_then(|t| t.parse::<f32>().ok())
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| "a window position is not a number".to_string())
                    };
                    let (x, y, w, h) = (number()?, number()?, number()?, number()?);
                    let rect = Rect::from_min_size(pos2(x, y), vec2(w.max(160.0), h.max(120.0)));
                    layout.windows.push((rect, Desc::parse(&mut tokens, 0)?));
                }
                "maxplay" => layout.maximize_on_play = rest.trim() == "1",
                other => return Err(trf!("unknown layout section `{other}`", other = other)),
            }
        }
        if !seen_main {
            return Err("the layout has no main area".to_string());
        }
        let mut panels = Vec::new();
        if let Some(main) = &layout.main {
            main.panels(&mut panels);
        }
        for (_, desc) in &layout.windows {
            desc.panels(&mut panels);
        }
        let unique: HashSet<Panel> = panels.iter().copied().collect();
        if unique.len() != panels.len() {
            return Err("a window appears twice".to_string());
        }
        Ok(layout)
    }

    /// `requested` — где окна должны появиться, пока их ещё ни разу не нарисовали.
    fn of_state(state: &DockState<Panel>, maximize_on_play: bool, requested: &[(SurfaceIndex, Rect)]) -> LayoutDesc {
        let mut layout = LayoutDesc { main: None, windows: Vec::new(), maximize_on_play };
        for (index, surface) in state.iter_surfaces().enumerate() {
            match surface {
                Surface::Main(tree) => layout.main = Desc::of_tree(tree, NodeIndex::root()),
                Surface::Window(tree, window) => {
                    if let Some(desc) = Desc::of_tree(tree, NodeIndex::root()) {
                        let shown = window.rect();
                        let rect = if shown.is_finite() && shown.width() > 1.0 {
                            shown
                        } else {
                            requested
                                .iter()
                                .find(|(surface, _)| surface.0 == index)
                                .map(|(_, rect)| *rect)
                                .unwrap_or(Rect::from_min_size(pos2(240.0, 160.0), vec2(420.0, 340.0)))
                        };
                        layout.windows.push((rect, desc));
                    }
                }
                Surface::Empty => {}
            }
        }
        layout
    }

    fn to_state(&self) -> DockState<Panel> {
        let mut state = DockState::new(Vec::new());
        match &self.main {
            Some(desc) => {
                *state.main_surface_mut() = Tree::new(desc.first_tabs());
                desc.build(state.main_surface_mut(), NodeIndex::root());
            }
            None => *state.main_surface_mut() = Tree::default(),
        }
        for (rect, desc) in &self.windows {
            let surface = state.add_window(desc.first_tabs());
            desc.build(&mut state[surface], NodeIndex::root());
            if let Some(window) = state.get_window_state_mut(surface) {
                window.set_position(rect.min).set_size(rect.size());
            }
        }
        state
    }
}

// ---------------------------------------------------------------------------
// Состояние дока в редакторе
// ---------------------------------------------------------------------------

/// Вид Game: своя текстура и свой размер (Scene и Game могут быть видны вместе).
pub(crate) struct GameView {
    pub texture: Option<egui::TextureId>,
    pub rect: Rect,
    pub pixels: [u32; 2],
    /// Слой egui, в котором нарисован вид Game (плавающее окно — свой слой): меню и
    /// окна поверх вида забирают щелчки себе, игре они не достаются.
    pub layer: egui::LayerId,
    /// Ввод у игры (в прошлом кадре): см. [`crate::Editor::game_has_input_focus`].
    pub input_focus: bool,
    /// Shift+Esc отдал ввод редактору; щелчок по виду Game возвращает его игре.
    pub released: bool,
    /// Окно редактора в фокусе системы.
    pub window_focused: bool,
    /// Окно дока в фокусе в прошлом кадре: фокус пришёл на Game (вкладкой, Ctrl+2) —
    /// снять Shift+Esc.
    pub last_panel: Option<Panel>,
    /// Курсор системы: запереть (`true`) или вернуть — забирает окно редактора.
    pub cursor_request: Option<bool>,
}

/// Где на экране окно было в прошлом кадре.
#[derive(Clone, Copy, Debug)]
struct Placed {
    panel: Panel,
    /// Содержимое (без полосы вкладок).
    body: Rect,
    /// Плавающее окно (лежит поверх главной области).
    floating: bool,
}

/// Перетаскивание вкладки: какое окно и откуда начали.
#[derive(Clone, Copy, Debug)]
struct TabDrag {
    panel: Panel,
    origin: Pos2,
}

pub(crate) struct DockLayout {
    state: DockState<Panel>,
    maximized: Option<Panel>,
    pub(crate) maximize_on_play: bool,
    /// Game развернули при входе в Play — при выходе вернуть как было.
    maximized_by_play: bool,
    was_playing: bool,
    pub(crate) game: GameView,
    /// Окна, нарисованные в прошлом кадре.
    placed: Vec<Placed>,
    tab_rects: Vec<(Panel, Rect)>,
    /// Последние `view_tab` / `bottom_tab`, которые док уже показал.
    seen_view: ViewTab,
    seen_bottom: BottomTab,
    drag: Option<TabDrag>,
    /// Прямоугольник всей области дока (с полем для стыковки к краю окна).
    area: Rect,
    /// Куда поставить новые плавающие окна (пока их не нарисовали).
    requested: Vec<(SurfaceIndex, Rect)>,
    /// Открыт диалог Save Layout… (введённое имя).
    pub(crate) save_dialog: Option<String>,
    /// Пока док рисует окна, его состояние вынуто из `state`: окно в фокусе на это время
    /// хранится здесь (иначе окна внутри дока видели бы «фокуса нет»).
    rendering_focus: Option<Option<Panel>>,
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            state: LayoutPreset::Default.desc().to_state(),
            maximized: None,
            maximize_on_play: false,
            maximized_by_play: false,
            was_playing: false,
            game: GameView {
                texture: None,
                rect: Rect::from_min_size(Pos2::ZERO, vec2(960.0, 540.0)),
                pixels: [960, 540],
                layer: egui::LayerId::background(),
                input_focus: false,
                released: false,
                window_focused: true,
                last_panel: None,
                cursor_request: None,
            },
            placed: Vec::new(),
            tab_rects: Vec::new(),
            seen_view: ViewTab::Scene,
            seen_bottom: BottomTab::Project,
            drag: None,
            area: Rect::NOTHING,
            requested: Vec::new(),
            save_dialog: None,
            rendering_focus: None,
        }
    }
}

/// Поле вокруг дока: бросить вкладку сюда — пристыковать её к краю окна редактора.
const EDGE: f32 = 6.0;
/// Высота полосы вкладок (в точках при масштабе 100 %).
const TAB_BAR: f32 = theme::TAB_BAR;
/// Минимальная ширина и высота области: разделитель дальше не сдвигается.
const MIN_AREA: f32 = 120.0;

impl DockLayout {
    fn find(&self, panel: Panel) -> Option<(SurfaceIndex, NodeIndex, TabIndex)> {
        self.state.find_tab(&panel)
    }

    fn is_open(&self, panel: Panel) -> bool {
        self.find(panel).is_some()
    }

    fn is_floating(&self, panel: Panel) -> bool {
        self.find(panel).is_some_and(|(surface, _, _)| !surface.is_main())
    }

    /// Окно видно: развёрнуто, или это активная вкладка своей области.
    fn is_visible(&self, panel: Panel) -> bool {
        if let Some(maximized) = self.maximized {
            return maximized == panel;
        }
        self.find(panel).is_some_and(|(surface, node, tab)| match &self.state[surface][node] {
            Node::Leaf { active, collapsed, .. } => *active == tab && !collapsed,
            _ => false,
        })
    }

    fn panels(&self) -> Vec<Panel> {
        self.state.iter_all_tabs().map(|(_, panel)| *panel).collect()
    }

    fn focus(&mut self, panel: Panel) {
        if self.maximized.is_some_and(|maximized| maximized != panel) {
            self.maximized = None;
            self.maximized_by_play = false;
        }
        match self.find(panel) {
            Some((surface, node, tab)) => {
                self.state.set_active_tab((surface, node, tab));
                self.state.set_focused_node_and_surface((surface, node));
            }
            None => self.open(panel),
        }
    }

    /// Открыть закрытое окно туда, где ему место: к окнам своей группы, а если их
    /// нет — у края главной области, как в раскладке Default.
    fn open(&mut self, panel: Panel) {
        if self.is_open(panel) {
            self.focus(panel);
            return;
        }
        let sibling = panel.group().iter().find_map(|other| self.find(*other));
        if let Some((surface, node, _)) = sibling {
            self.state[surface][node].append_tab(panel);
        } else {
            let main = self.state.main_surface_mut();
            if main.is_empty() || main.num_tabs() == 0 {
                *main = Tree::new(vec![panel]);
            } else {
                let (split, fraction) = match panel {
                    Panel::Hierarchy => (Split::Left, 0.2),
                    Panel::Inspector => (Split::Right, 0.76),
                    Panel::Project | Panel::Console | Panel::ErrorList | Panel::FindResults | Panel::Profiler => (Split::Below, 0.7),
                    Panel::Scene | Panel::Game | Panel::Code => (Split::Right, 0.3),
                };
                main.split(NodeIndex::root(), split, fraction, Node::leaf(panel));
            }
        }
        self.focus(panel);
    }

    fn close(&mut self, panel: Panel) {
        if let Some(found) = self.find(panel) {
            self.state.remove_tab(found);
        }
        if self.maximized == Some(panel) {
            self.maximized = None;
            self.maximized_by_play = false;
        }
    }

    /// Пристыковать `panel` к `target`: сбоку (поделить область пополам) или вкладкой.
    fn dock(&mut self, panel: Panel, target: Panel, side: DockSide) -> Result<(), String> {
        if panel == target {
            return Err(trf!("{} cannot be docked to itself", panel.title()));
        }
        if !self.is_open(target) {
            return Err(trf!("{} is not open: open it from the Window menu first", target.title()));
        }
        self.close(panel);
        let (surface, node, _) = self.find(target).expect("the target is open");
        match split_of(side) {
            None => self.state[surface][node].append_tab(panel),
            Some(split) => {
                self.state.split((surface, node), split, 0.5, Node::leaf(panel));
            }
        }
        self.focus(panel);
        Ok(())
    }

    /// Пристыковать окно к краю главной области (всего окна редактора).
    fn dock_to_edge(&mut self, panel: Panel, side: DockSide) {
        self.close(panel);
        let main = self.state.main_surface_mut();
        match split_of(side) {
            Some(split) if !main.is_empty() && main.num_tabs() > 0 => {
                // Доля хранится для левого (верхнего) узла.
                let fraction = match split {
                    Split::Left | Split::Above => 0.25,
                    Split::Right | Split::Below => 0.75,
                };
                main.split(NodeIndex::root(), split, fraction, Node::leaf(panel));
            }
            _ => {
                if main.is_empty() || main.num_tabs() == 0 {
                    *main = Tree::new(vec![panel]);
                } else {
                    main.push_to_first_leaf(panel);
                }
            }
        }
        self.focus(panel);
    }

    /// Вынести окно в плавающее.
    fn float(&mut self, panel: Panel, rect: Option<Rect>) {
        let size = self
            .placed
            .iter()
            .find(|p| p.panel == panel)
            .map_or(vec2(420.0, 340.0), |p| (p.body.size() + vec2(0.0, TAB_BAR)).max(vec2(300.0, 220.0)));
        let rect = rect.unwrap_or_else(|| {
            let center = if self.area.is_finite() { self.area.center() } else { pos2(640.0, 400.0) };
            Rect::from_center_size(center, size.min(vec2(720.0, 520.0)))
        });
        self.close(panel);
        let surface = self.state.add_window(vec![panel]);
        if let Some(window) = self.state.get_window_state_mut(surface) {
            window.set_position(rect.min).set_size(rect.size());
        }
        self.requested.retain(|(s, _)| *s != surface);
        self.requested.push((surface, rect));
        self.focus(panel);
    }

    fn toggle_maximize(&mut self, panel: Panel) {
        self.maximized_by_play = false;
        if self.maximized == Some(panel) {
            self.maximized = None;
        } else if self.is_open(panel) {
            self.maximized = Some(panel);
        }
    }

    fn describe(&self) -> LayoutDesc {
        LayoutDesc::of_state(&self.state, self.maximize_on_play, &self.requested)
    }

    fn load(&mut self, desc: &LayoutDesc) {
        self.state = desc.to_state();
        // Окна добавлялись по порядку с первого свободного места (0 — главная область).
        self.requested = desc
            .windows
            .iter()
            .enumerate()
            .map(|(index, (rect, _))| (SurfaceIndex(index + 1), *rect))
            .collect();
        self.maximize_on_play = desc.maximize_on_play;
        self.maximized = None;
        self.maximized_by_play = false;
        self.placed.clear();
        self.tab_rects.clear();
        // Активной (с фокусом) делаем область вида Scene, как в Unity после загрузки.
        if let Some((surface, node, _)) = self.find(Panel::Scene) {
            self.state.set_focused_node_and_surface((surface, node));
        }
    }

    /// Окно под курсором (плавающие — поверх главной области).
    fn panel_at(&self, pointer: Pos2) -> Option<Panel> {
        let hit = |floating: bool| {
            self.placed
                .iter()
                .rev()
                .find(|p| p.floating == floating && p.body.expand2(vec2(0.0, TAB_BAR)).contains(pointer))
                .map(|p| p.panel)
        };
        hit(true).or_else(|| hit(false))
    }
}

fn split_of(side: DockSide) -> Option<Split> {
    match side {
        DockSide::Left => Some(Split::Left),
        DockSide::Right => Some(Split::Right),
        DockSide::Top => Some(Split::Above),
        DockSide::Bottom => Some(Split::Below),
        DockSide::Center => None,
    }
}

/// Отображение `ViewTab` / `BottomTab` (старый API) на окна.
fn view_panel(tab: ViewTab) -> Panel {
    match tab {
        ViewTab::Scene => Panel::Scene,
        ViewTab::Game => Panel::Game,
        ViewTab::Code => Panel::Code,
    }
}

fn bottom_panel(tab: BottomTab) -> Panel {
    match tab {
        BottomTab::Project => Panel::Project,
        BottomTab::Console => Panel::Console,
        BottomTab::Profiler => Panel::Profiler,
    }
}

// ---------------------------------------------------------------------------
// Внешний вид
// ---------------------------------------------------------------------------

fn dock_style(egui_style: &egui::Style) -> Style {
    let mut style = Style::from_egui(egui_style);
    // Панели встык с краями окна; поле EDGE для пристыковки к краю остаётся.
    style.dock_area_padding = Some(Margin::ZERO);
    style.main_surface_border_stroke = Stroke::NONE;
    style.main_surface_border_rounding = Rounding::ZERO;

    // Панели встык: между ними волосяная линия, за которую можно тянуть (зона шире линии).
    style.separator.width = 1.0;
    style.separator.extra_interact_width = 6.0;
    style.separator.extra = MIN_AREA;
    style.separator.color_idle = theme::LINE;
    style.separator.color_hovered = theme::accent();
    style.separator.color_dragged = theme::accent();

    style.tab_bar.bg_fill = theme::BG_HEADER;
    style.tab_bar.height = TAB_BAR;
    style.tab_bar.rounding = Rounding::ZERO;
    style.tab_bar.hline_color = theme::LINE;
    style.tab_bar.fill_tab_bar = false;
    style.tab_bar.show_scroll_bar_on_overflow = false;

    let top = Rounding { nw: theme::R_S, ne: theme::R_S, sw: 0.0, se: 0.0 };
    let tab = |bg_fill: Color32, text_color: Color32| TabInteractionStyle {
        outline_color: Color32::TRANSPARENT,
        rounding: top,
        bg_fill,
        text_color,
    };
    style.tab.active = tab(theme::BG_PANEL, theme::TEXT);
    style.tab.focused = tab(theme::BG_PANEL, theme::TEXT_STRONG);
    style.tab.inactive = tab(theme::BG_HEADER, theme::TEXT_MUTED);
    style.tab.hovered = tab(theme::mix(theme::BG_HEADER, Color32::WHITE, 0.04), theme::TEXT);
    style.tab.active_with_kb_focus = style.tab.active.clone();
    style.tab.focused_with_kb_focus = style.tab.focused.clone();
    style.tab.inactive_with_kb_focus = style.tab.hovered.clone();
    style.tab.tab_body.inner_margin = Margin::ZERO;
    style.tab.tab_body.stroke = Stroke::NONE;
    style.tab.tab_body.rounding = Rounding::ZERO;
    style.tab.tab_body.bg_fill = theme::BG_PANEL;
    style.tab.hline_below_active_tab_name = false;
    style.tab.minimum_width = Some(72.0);

    let accent = theme::accent();
    style.overlay.overlay_type = OverlayType::HighlightedAreas;
    style.overlay.selection_color = accent.gamma_multiply(0.28);
    style.overlay.selection_stroke_width = 2.0;
    style.overlay.hovered_leaf_highlight.color = Color32::TRANSPARENT;
    style.overlay.hovered_leaf_highlight.stroke = Stroke::new(1.5_f32, accent.gamma_multiply(0.8));
    style.overlay.hovered_leaf_highlight.rounding = Rounding::same(theme::R_S);
    style.overlay.hovered_leaf_highlight.expansion = 0.0;
    // Бросить в середину области — плавающее окно; у краёв — разделить; на полосу
    // вкладок — вкладкой (как в Unity).
    style.overlay.feel.center_drop_coverage = 0.0;
    style.overlay.feel.window_drop_coverage = 0.42;
    style.overlay.surface_fade_opacity = 0.35;
    style
}

/// Заголовок вкладки: место под иконку, имя, у Console — число ошибок или предупреждений.
fn tab_title(panel: Panel, badge: Option<(usize, Color32)>) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        panel.title(),
        18.0,
        egui::TextFormat {
            font_id: theme::body(),
            color: Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    if let Some((count, color)) = badge {
        job.append(
            &count.to_string(),
            7.0,
            egui::TextFormat {
                font_id: theme::strong(theme::FONT_CAPTION),
                color,
                ..Default::default()
            },
        );
    }
    job
}

// ---------------------------------------------------------------------------
// Отрисовка
// ---------------------------------------------------------------------------

/// Что вкладки и их меню попросили за кадр.
#[derive(Default)]
struct Requests {
    actions: Vec<LayoutAction>,
    /// Перетаскиваемая вкладка в этом кадре.
    dragging: Option<TabDrag>,
}

/// Рисует содержимое вкладок: те же функции окон, что и раньше.
struct Viewer<'a> {
    editor: &'a mut Editor,
    dock_id: Id,
    /// Где каждая вкладка лежала в начале кадра — для её `Id` в egui_dock.
    places: Vec<(Panel, SurfaceIndex, NodeIndex, usize)>,
    active: HashSet<Panel>,
    focused: Option<Panel>,
    badge: Option<(usize, Color32)>,
    /// Число ошибок (или предупреждений) кода на вкладке Error List.
    error_badge: Option<(usize, Color32)>,
    maximized: bool,
    requests: Requests,
    placed: Vec<Placed>,
    tab_rects: Vec<(Panel, Rect)>,
}

impl Viewer<'_> {
    fn tab_id(&self, panel: Panel) -> Option<Id> {
        self.places.iter().find(|p| p.0 == panel).map(|(_, surface, node, tab)| {
            self.dock_id
                .with((*surface, "surface"))
                .with((*node, "node"))
                .with((*tab, "tab"))
        })
    }

    fn floating(&self, panel: Panel) -> bool {
        self.places.iter().any(|p| p.0 == panel && !p.1.is_main())
    }

    /// Пункты меню окна: из «⋮» области и из меню вкладки по правой кнопке.
    fn window_menu(&mut self, ui: &mut egui::Ui, panel: Panel) {
        theme::menu_body(ui);
        let maximize = if self.maximized { tr_s("Restore") } else { tr_s("Maximize") };
        if item(maximize).icon(Icon::Collapse).shortcut(&crate::shortcuts::label(ui.ctx(), "maximize-panel")).checked(self.maximized).show(ui) {
            self.requests.actions.push(LayoutAction::ToggleMaximize(panel));
        }
        if item(tr_s("Close Tab")).icon(Icon::Close).show(ui) {
            self.requests.actions.push(LayoutAction::Close(panel));
        }
        if !self.maximized && !self.floating(panel) && item(tr_s("Float Window")).icon(Icon::External).show(ui) {
            self.requests.actions.push(LayoutAction::Float(panel));
        }
        theme::menu_separator(ui);
        let open: HashSet<Panel> = self.places.iter().map(|p| p.0).collect();
        theme::submenu(ui, Icon::Plus, tr_s("Add Tab"), |ui| {
            for other in Panel::ALL {
                if other == panel {
                    continue;
                }
                // Уже открытое окно переезжает сюда вкладкой.
                if item(other.title()).icon(other.icon()).checked(open.contains(&other)).show(ui) {
                    self.requests.actions.push(LayoutAction::Dock { panel: other, target: panel, side: DockSide::Center });
                }
            }
        });
    }

    /// Кнопка «⋮» в правом конце полосы вкладок области.
    fn area_button(&mut self, ui: &egui::Ui, panel: Panel, body: Rect) {
        let bar = Rect::from_min_max(pos2(body.left(), body.top() - TAB_BAR), pos2(body.right(), body.top()));
        if bar.width() < 60.0 {
            return;
        }
        let rect = Rect::from_center_size(pos2(bar.right() - 13.0, bar.center().y), vec2(20.0, TAB_BAR - 4.0));
        let mut child = egui::Ui::new(
            ui.ctx().clone(),
            Id::new(("dock_area_menu", panel)),
            egui::UiBuilder::new().layer_id(ui.layer_id()).max_rect(rect),
        );
        child.set_clip_rect(rect);
        let hovered = ui.ctx().pointer_hover_pos().is_some_and(|p| rect.contains(p));
        if hovered {
            child.painter().rect_filled(rect, theme::R_S, theme::BG_HOVER);
        }
        let response = crate::controls::menu_anchor(
            &mut child,
            rect.size(),
            false,
            |ui| self.window_menu(ui, panel),
        )
        .response;
        icons::paint(
            child.painter(),
            Rect::from_center_size(rect.center(), vec2(theme::ICON_S, theme::ICON_S)),
            Icon::More,
            if hovered { theme::TEXT } else { theme::TEXT_MUTED },
        );
        response.on_hover_text(tr_s("Window options: maximize, close, add a tab"));
    }
}

impl TabViewer for Viewer<'_> {
    type Tab = Panel;

    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        let badge = match *tab {
            Panel::Console => self.badge,
            Panel::ErrorList => self.error_badge,
            _ => None,
        };
        tab_title(*tab, badge).into()
    }

    fn id(&mut self, tab: &mut Panel) -> Id {
        Id::new(("dock_tab", *tab))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        let panel = *tab;
        let body = ui.max_rect();
        let floating = self.floating(panel);
        self.placed.push(Placed { panel, body, floating });
        let editor = &mut *self.editor;
        match panel {
            Panel::Hierarchy => crate::hierarchy::show(editor, ui),
            Panel::Inspector => crate::inspector::show(editor, ui),
            Panel::Scene => crate::viewport::show_scene(editor, ui),
            Panel::Game => crate::viewport::show_game(editor, ui),
            Panel::Code => crate::code::show(editor, ui),
            Panel::Project => crate::project::show(editor, ui),
            Panel::Console => crate::console::show(editor, ui),
            Panel::ErrorList => crate::error_list::show(editor, ui),
            Panel::FindResults => crate::find_in_files::show(editor, ui),
            Panel::Profiler => crate::profiler::show(editor, ui),
        }
        self.area_button(ui, panel, body);
    }

    fn on_tab_button(&mut self, tab: &mut Panel, response: &egui::Response) {
        let panel = *tab;
        let ctx = response.ctx.clone();
        let tab_id = self.tab_id(panel);
        let dragged = tab_id.is_some_and(|id| ctx.is_being_dragged(id));
        let rect = response.rect;
        self.tab_rects.push((panel, rect));

        // Иконка — в начале подписи (место под неё оставлено отступом).
        let job = tab_title(
            panel,
            match panel {
                Panel::Console => self.badge,
                Panel::ErrorList => self.error_badge,
                _ => None,
            },
        );
        let width = ctx.fonts(|fonts| fonts.layout_job(job)).size().x;
        let active = self.active.contains(&panel);
        let focused = self.focused == Some(panel);
        if dragged {
            // Заголовок летит за курсором отдельным слоем egui_dock; иконку там не рисуем.
            if let Some(origin) = ctx.input(|i| i.pointer.press_origin()) {
                self.requests.dragging = Some(TabDrag { panel, origin });
            }
        } else {
            let painter = ctx.layer_painter(response.layer_id).with_clip_rect(rect.intersect(response.interact_rect));
            let text_left = rect.center().x - width * 0.5;
            let color = if focused {
                theme::TEXT
            } else if active {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_DISABLED
            };
            icons::paint(
                &painter,
                Rect::from_center_size(pos2(text_left + 6.0, rect.center().y), vec2(theme::ICON_S, theme::ICON_S)),
                panel.icon(),
                color,
            );
            // Активная вкладка — полоса сверху: яркая у окна с фокусом.
            if active {
                let line = if focused { theme::accent() } else { theme::BG_ACTIVE };
                painter.rect_filled(
                    Rect::from_min_size(rect.min, vec2(rect.width(), if focused { 2.0 } else { 1.0 })),
                    Rounding { nw: theme::R_S, ne: theme::R_S, sw: 0.0, se: 0.0 },
                    line,
                );
            }
        }

        if response.double_clicked() {
            self.requests.actions.push(LayoutAction::ToggleMaximize(panel));
        }
        if !dragged {
            response.context_menu(|ui| self.window_menu(ui, panel));
        }
    }

    fn closeable(&mut self, _tab: &mut Panel) -> bool {
        true
    }

    fn scroll_bars(&self, _tab: &Panel) -> [bool; 2] {
        // Окна сами решают, где прокручиваться.
        [false, false]
    }
}

impl Editor {
    /// Док со всеми окнами: вызывается из [`Editor::ui`] внутри центральной панели.
    pub(crate) fn dock_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let area = ui.available_rect_before_wrap();
        self.dock.area = area;
        self.sync_from_fields();
        self.play_maximize();

        // Shift+Space над окном — развернуть или вернуть (как в Unity).
        if !ctx.wants_keyboard_input() && self.dialog.is_none() {
            let toggle = self.consume_command(&ctx, "maximize-panel");
            if toggle {
                let hovered = ctx.pointer_hover_pos().and_then(|p| self.dock.panel_at(p));
                if let Some(panel) = hovered.or(self.dock.maximized) {
                    self.dock.toggle_maximize(panel);
                }
            }
        }

        // Вкладку тянут к краю окна редактора (полю вокруг дока) или за его пределы.
        let released = ctx.input(|i| i.pointer.primary_released());
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let edge_drop = self.dock.drag.and_then(|drag| {
            let pointer = pointer?;
            let moved = pointer - drag.origin;
            if moved.x.abs() <= 30.0 && moved.y.abs() <= 6.0 {
                return None;
            }
            Some((drag.panel, edge_side(area, pointer)))
        });
        if let Some((_, Some(side))) = edge_drop {
            paint_edge_preview(&ctx, area, side);
        }

        let errors = self.console.count(engine_core::logging::Level::Error);
        let warnings = self.console.count(engine_core::logging::Level::Warn);
        let badge = if errors > 0 {
            Some((errors, theme::ERROR))
        } else if warnings > 0 {
            Some((warnings, theme::WARN))
        } else {
            None
        };

        let (code_errors, code_warnings, _) = self.code_problem_counts();
        let error_badge = if code_errors > 0 {
            Some((code_errors, theme::ERROR))
        } else if code_warnings > 0 {
            Some((code_warnings, theme::WARN))
        } else {
            None
        };

        let maximized = self.dock.maximized.filter(|panel| self.dock.is_open(*panel));
        self.dock.maximized = maximized;
        let mut state = match maximized {
            Some(panel) => DockState::new(vec![panel]),
            None => std::mem::replace(&mut self.dock.state, DockState::new(Vec::new())),
        };
        let dock_id = Id::new(if maximized.is_some() { "editor_dock_maximized" } else { "editor_dock" });
        let places = state
            .iter_all_tabs()
            .map(|((surface, node), panel)| {
                let index = state[surface][node].tabs().map_or(0, |tabs| {
                    tabs.iter().position(|tab| tab == panel).unwrap_or(0)
                });
                (*panel, surface, node, index)
            })
            .collect();
        let active: HashSet<Panel> = state
            .iter_all_nodes()
            .filter_map(|(_, node)| match node {
                Node::Leaf { tabs, active, .. } => tabs.get(active.0).copied(),
                _ => None,
            })
            .collect();
        let focused = state.focused_leaf().and_then(|(surface, node)| match &state[surface][node] {
            Node::Leaf { tabs, active, .. } => tabs.get(active.0).copied(),
            _ => None,
        });

        let style = dock_style(&ctx.style());
        if maximized.is_none() {
            self.dock.rendering_focus = Some(focused);
        }
        let mut viewer = Viewer {
            editor: self,
            dock_id,
            places,
            active,
            focused,
            badge,
            error_badge,
            maximized: maximized.is_some(),
            requests: Requests::default(),
            placed: Vec::new(),
            tab_rects: Vec::new(),
        };
        DockArea::new(&mut state)
            .id(dock_id)
            .style(style)
            .show_close_buttons(false)
            .show_add_buttons(false)
            .tab_context_menus(false)
            .draggable_tabs(maximized.is_none())
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .window_bounds(area)
            .show_inside(ui, &mut viewer);
        let Viewer { requests, placed, tab_rects, .. } = viewer;
        self.dock.rendering_focus = None;

        if maximized.is_none() {
            self.dock.state = state;
            enforce_min_sizes(&mut self.dock.state);
        }
        self.dock.placed = placed;
        self.dock.tab_rects = tab_rects;

        // Вкладку отпустили у края окна или за пределами дока.
        if released {
            if let (Some((panel, side)), Some(pointer)) = (edge_drop, pointer) {
                match side {
                    Some(side) => self.dock.dock_to_edge(panel, side),
                    None if !area.contains(pointer) => self.dock.float(
                        panel,
                        Some(Rect::from_min_size(
                            pointer - vec2(40.0, 12.0),
                            vec2(420.0, 340.0),
                        )),
                    ),
                    None => {}
                }
            }
            self.dock.drag = None;
        } else if requests.dragging.is_some() {
            self.dock.drag = requests.dragging;
        } else if !ctx.input(|i| i.pointer.any_down()) {
            self.dock.drag = None;
        }

        for action in requests.actions {
            if let Err(err) = self.apply_layout_action(action) {
                log::error!("{err}");
            }
        }
        self.sync_to_fields();
        self.persist_layout(&ctx);
    }

    /// Диалог Window → Layouts → Save Layout…: имя раскладки.
    pub(crate) fn save_layout_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.dock.save_dialog.as_mut() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let response = crate::window_kit::dialog(Id::new("save_layout_dialog"), "").width(0.0).show(ctx, |ui| {
            ui.set_width(380.0);
            theme::dialog_title(ui, Icon::Tiles, theme::TEXT_MUTED, tr_s("Save Layout"));
            ui.label(
                egui::RichText::new(tr_s("The current windows, their sizes and floating windows are saved under this name."))
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(theme::SP_L);
            let edit = crate::controls::text_field(ui, name, tr_s("My Layout"), f32::INFINITY);
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                save = true;
            }
            crate::window_kit::dialog_actions(ui, |ui| {
                let valid = layout_name(name).is_some();
                if ui.add_enabled_ui(valid, |ui| crate::window_kit::primary_action(ui, tr_s("Save"))).inner.clicked() {
                    save = true;
                }
                if crate::window_kit::action_button(ui, tr_s("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if response.should_close() {
            cancel = true;
        }
        if save {
            let name = self.dock.save_dialog.take().unwrap_or_default();
            if let Err(err) = self.apply_layout_action(LayoutAction::Save(name)) {
                log::error!("{err}");
            }
        } else if cancel {
            self.dock.save_dialog = None;
        }
    }

    /// Поля `view_tab` / `bottom_tab` поменяли в обход дока (Play, Stop, открыть файл,
    /// ошибка сборки) — показать соответствующее окно.
    fn sync_from_fields(&mut self) {
        if self.view_tab != self.dock.seen_view {
            self.dock.seen_view = self.view_tab;
            self.dock.focus(view_panel(self.view_tab));
        }
        if self.bottom_tab != self.dock.seen_bottom {
            self.dock.seen_bottom = self.bottom_tab;
            self.dock.focus(bottom_panel(self.bottom_tab));
        }
    }

    /// Окно с фокусом — последний выбранный вид (`view_tab`) или нижнее окно.
    fn sync_to_fields(&mut self) {
        let focused = self.dock.state.focused_leaf().and_then(|(surface, node)| match &self.dock.state[surface][node] {
            Node::Leaf { tabs, active, .. } => tabs.get(active.0).copied(),
            _ => None,
        });
        let focused = self.dock.maximized.or(focused);
        match focused {
            Some(Panel::Scene) => self.view_tab = ViewTab::Scene,
            Some(Panel::Game) => self.view_tab = ViewTab::Game,
            Some(Panel::Code) => self.view_tab = ViewTab::Code,
            Some(Panel::Project) => self.bottom_tab = BottomTab::Project,
            Some(Panel::Console) => self.bottom_tab = BottomTab::Console,
            Some(Panel::Profiler) => self.bottom_tab = BottomTab::Profiler,
            _ => {}
        }
        self.dock.seen_view = self.view_tab;
        self.dock.seen_bottom = self.bottom_tab;
    }

    /// Вход в Play: показать Game (развернуть, если включено Maximize On Play);
    /// выход — вернуть раскладку.
    fn play_maximize(&mut self) {
        let playing = self.mode != Mode::Edit;
        if playing && !self.dock.was_playing {
            if self.dock.maximize_on_play && self.dock.maximized.is_none() {
                if !self.dock.is_open(Panel::Game) {
                    self.dock.open(Panel::Game);
                }
                self.dock.maximized = Some(Panel::Game);
                self.dock.maximized_by_play = true;
            } else if !self.dock.is_visible(Panel::Game) {
                match self.dock.find(Panel::Game) {
                    // Focus Game View On Play выключен: показать вкладку, фокус не трогать.
                    Some(place) if !self.prefs.focus_game_on_play => self.dock.state.set_active_tab(place),
                    _ => self.dock.focus(Panel::Game),
                }
            }
        }
        if !playing && self.dock.was_playing && self.dock.maximized_by_play {
            self.dock.maximized = None;
            self.dock.maximized_by_play = false;
        }
        self.dock.was_playing = playing;
    }

    /// Текущая раскладка — в настройки (их запишет на диск окно редактора), когда
    /// мышь отпущена: не на каждом шаге перетаскивания.
    fn persist_layout(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let text = self.dock.describe().to_text();
        if self.prefs.layout.as_deref() != Some(text.as_str()) {
            self.prefs.layout = Some(text);
        }
    }

    /// Восстановить раскладку из настроек (при старте). Испорченная или устаревшая
    /// раскладка — Default и одно предупреждение в лог.
    pub(crate) fn restore_layout_from_prefs(&mut self) {
        let Some(text) = self.prefs.layout.clone() else {
            return;
        };
        if text == self.dock.describe().to_text() {
            return;
        }
        match LayoutDesc::parse(&text) {
            Ok(desc) => self.dock.load(&desc),
            Err(reason) => {
                log::warn!("the saved window layout cannot be restored ({reason}); using the Default layout");
                self.dock.load(&LayoutPreset::Default.desc());
                self.prefs.layout = Some(self.dock.describe().to_text());
            }
        }
    }

    /// Выполнить действие с окнами.
    pub(crate) fn apply_layout_action(&mut self, action: LayoutAction) -> Result<(), EditorError> {
        match action {
            LayoutAction::Open(panel) => self.dock.open(panel),
            LayoutAction::Close(panel) => self.dock.close(panel),
            LayoutAction::Focus(panel) => self.dock.focus(panel),
            LayoutAction::ToggleMaximize(panel) => self.dock.toggle_maximize(panel),
            LayoutAction::Float(panel) => self.dock.float(panel, None),
            LayoutAction::Dock { panel, target, side } => {
                self.dock.dock(panel, target, side).map_err(EditorError::Other)?;
            }
            LayoutAction::DockToEdge { panel, side } => self.dock.dock_to_edge(panel, side),
            LayoutAction::Preset(preset) => {
                let mut desc = preset.desc();
                desc.maximize_on_play = self.dock.maximize_on_play;
                self.dock.load(&desc);
            }
            LayoutAction::Reset => {
                self.dock.load(&LayoutPreset::Default.desc());
            }
            LayoutAction::Save(name) => {
                let name = layout_name(&name).ok_or_else(|| {
                    EditorError::Other(tr_s("a layout needs a name: type one in the Save Layout dialog").to_string())
                })?;
                let text = self.dock.describe().to_text();
                match self.prefs.saved_layouts.iter_mut().find(|(saved, _)| *saved == name) {
                    Some((_, layout)) => *layout = text,
                    None => self.prefs.saved_layouts.push((name.clone(), text)),
                }
                log::info!("window layout `{name}` saved");
            }
            LayoutAction::Load(name) => {
                let text = self
                    .prefs
                    .saved_layouts
                    .iter()
                    .find(|(saved, _)| *saved == name)
                    .map(|(_, text)| text.clone())
                    .ok_or_else(|| EditorError::Other(trf!("there is no saved layout named `{name}`", name = name)))?;
                let desc = LayoutDesc::parse(&text).map_err(|reason| {
                    EditorError::Other(trf!(
                        "the saved layout `{name}` cannot be loaded ({reason}); delete it from Window → Layouts → Delete Layout"
                    , name = name, reason = reason))
                })?;
                self.dock.load(&desc);
            }
            LayoutAction::Delete(name) => {
                let before = self.prefs.saved_layouts.len();
                self.prefs.saved_layouts.retain(|(saved, _)| *saved != name);
                if self.prefs.saved_layouts.len() == before {
                    return Err(EditorError::Other(trf!("there is no saved layout named `{name}`", name = name)));
                }
            }
            LayoutAction::MaximizeOnPlay(on) => self.dock.maximize_on_play = on,
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Публичный API окон (для тестов и инструментов)
    // ------------------------------------------------------------------

    /// Открытые окна (в доке и плавающие).
    pub fn open_panels(&self) -> Vec<Panel> {
        self.dock.panels()
    }

    /// Окно открыто (вкладкой где-нибудь или плавающим).
    pub fn is_panel_open(&self, panel: Panel) -> bool {
        self.dock.is_open(panel)
    }

    /// Окно видно: это активная вкладка своей области (или оно развёрнуто).
    pub fn is_panel_visible(&self, panel: Panel) -> bool {
        self.dock.is_visible(panel)
    }

    /// Окно плавающее (не в главной области).
    pub fn is_panel_floating(&self, panel: Panel) -> bool {
        self.dock.is_floating(panel)
    }

    /// Открыть окно (закрытое появляется рядом с окнами своей группы).
    pub fn open_panel(&mut self, panel: Panel) {
        self.dock.open(panel);
    }

    /// Закрыть вкладку окна.
    pub fn close_panel(&mut self, panel: Panel) {
        self.dock.close(panel);
    }

    /// Показать окно и дать ему фокус (закрытое — открыть).
    pub fn focus_panel(&mut self, panel: Panel) {
        self.dock.focus(panel);
    }

    /// Пристыковать окно к другому: `DockSide::Center` — вкладкой, иначе сбоку.
    pub fn dock_panel(&mut self, panel: Panel, target: Panel, side: DockSide) -> Result<(), EditorError> {
        self.dock.dock(panel, target, side).map_err(EditorError::Other)
    }

    /// Пристыковать окно к краю окна редактора.
    pub fn dock_panel_to_edge(&mut self, panel: Panel, side: DockSide) {
        self.dock.dock_to_edge(panel, side);
    }

    /// Вынести окно в плавающее (`rect` — где, в точках интерфейса).
    pub fn float_panel(&mut self, panel: Panel, rect: Option<egui::Rect>) {
        self.dock.float(panel, rect);
    }

    /// Развернуть окно на всю область дока (`None` — вернуть раскладку).
    pub fn maximize_panel(&mut self, panel: Option<Panel>) {
        match panel {
            Some(panel) if self.dock.is_open(panel) => {
                self.dock.maximized = Some(panel);
                self.dock.maximized_by_play = false;
            }
            Some(_) => {}
            None => {
                self.dock.maximized = None;
                self.dock.maximized_by_play = false;
            }
        }
    }

    /// Развёрнутое окно.
    pub fn maximized_panel(&self) -> Option<Panel> {
        self.dock.maximized
    }

    /// Окно с фокусом клавиатуры: развёрнутое или то, по которому щёлкнули последним.
    pub fn focused_panel(&self) -> Option<Panel> {
        // Окна спрашивают фокус и во время отрисовки дока (стрелки в Hierarchy, полёт в Scene).
        if let Some(focused) = self.dock.rendering_focus {
            return self.dock.maximized.or(focused);
        }
        let state = &self.dock.state;
        let focused = state.focused_leaf().and_then(|(surface, node)| match &state[surface][node] {
            Node::Leaf { tabs, active, .. } => tabs.get(active.0).copied(),
            _ => None,
        });
        self.dock.maximized.or(focused)
    }

    /// Выделение «в фокусе» (синее): фокус в окне, которое с ним работает. Иначе — серое.
    pub(crate) fn selection_focused(&self) -> bool {
        matches!(
            self.focused_panel(),
            None | Some(Panel::Hierarchy | Panel::Scene | Panel::Game | Panel::Inspector)
        )
    }

    /// Готовая раскладка: Default, 2 by 3, Tall, Wide.
    pub fn set_layout(&mut self, preset: LayoutPreset) {
        let _ = self.apply_layout_action(LayoutAction::Preset(preset));
    }

    /// Текущая раскладка текстом (так она хранится в настройках).
    pub fn layout_text(&self) -> String {
        self.dock.describe().to_text()
    }

    /// Применить раскладку из текста [`Editor::layout_text`].
    pub fn set_layout_text(&mut self, text: &str) -> Result<(), EditorError> {
        let desc = LayoutDesc::parse(text)
            .map_err(|reason| EditorError::Other(trf!("the window layout cannot be read: {reason}", reason = reason)))?;
        self.dock.load(&desc);
        Ok(())
    }

    /// Сохранить текущую раскладку под именем (Window → Layouts → Save Layout…).
    pub fn save_layout(&mut self, name: &str) -> Result<(), EditorError> {
        self.apply_layout_action(LayoutAction::Save(name.to_string()))
    }

    /// Загрузить сохранённую раскладку.
    pub fn load_layout(&mut self, name: &str) -> Result<(), EditorError> {
        self.apply_layout_action(LayoutAction::Load(name.to_string()))
    }

    /// Удалить сохранённую раскладку.
    pub fn delete_layout(&mut self, name: &str) -> Result<(), EditorError> {
        self.apply_layout_action(LayoutAction::Delete(name.to_string()))
    }

    /// Имена сохранённых раскладок.
    pub fn saved_layouts(&self) -> Vec<String> {
        self.prefs.saved_layouts.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Где в прошлом кадре было содержимое окна (без полосы вкладок), если оно было видно.
    pub fn panel_rect(&self, panel: Panel) -> Option<egui::Rect> {
        self.dock.placed.iter().find(|p| p.panel == panel).map(|p| p.body)
    }

    /// Где в прошлом кадре была вкладка окна (её заголовок).
    pub fn panel_tab_rect(&self, panel: Panel) -> Option<egui::Rect> {
        self.dock.tab_rects.iter().find(|(p, _)| *p == panel).map(|(_, rect)| *rect)
    }

    /// Разворачивать Game при входе в Play (кнопка Maximize On Play в окне Game).
    pub fn maximize_on_play(&self) -> bool {
        self.dock.maximize_on_play
    }

    pub fn set_maximize_on_play(&mut self, on: bool) {
        self.dock.maximize_on_play = on;
    }

    /// Прямоугольник вида Game в окне (в точках egui).
    pub fn game_view_rect(&self) -> egui::Rect {
        self.dock.game.rect
    }

    /// Идентификатор области вида Game.
    pub fn game_view_id() -> egui::Id {
        egui::Id::new("editor_game_view")
    }

    /// Какие виды рисовать в этом кадре: [Scene, Game].
    pub(crate) fn visible_views(&self) -> [bool; 2] {
        [self.dock.is_visible(Panel::Scene), self.dock.is_visible(Panel::Game)]
    }

    pub(crate) fn game_view_pixels(&self) -> [u32; 2] {
        self.dock.game.pixels
    }

    pub(crate) fn set_game_texture(&mut self, texture: egui::TextureId) {
        self.dock.game.texture = Some(texture);
    }

    /// Вид камеры для конкретного окна (Scene или Game), независимо от `view_tab`.
    pub(crate) fn camera_view_for(
        &mut self,
        renderer: &mut engine_render::Renderer,
        size: [u32; 2],
        tab: ViewTab,
    ) -> engine_render::CameraView {
        let saved = std::mem::replace(&mut self.view_tab, tab);
        let view = self.camera_view(renderer, size);
        self.view_tab = saved;
        view
    }
}

/// Имя раскладки без символов, которые ломают файл настроек.
fn layout_name(name: &str) -> Option<String> {
    let clean: String = name
        .chars()
        .map(|c| if matches!(c, ',' | ':' | ';' | '\n' | '\r' | '"') { ' ' } else { c })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    (!clean.is_empty()).then_some(clean)
}

/// В поле у края дока — к какому краю окна пристыковать.
fn edge_side(area: Rect, pointer: Pos2) -> Option<DockSide> {
    if !area.contains(pointer) || area.shrink(EDGE).contains(pointer) {
        return None;
    }
    let distances = [
        (pointer.x - area.left(), DockSide::Left),
        (area.right() - pointer.x, DockSide::Right),
        (pointer.y - area.top(), DockSide::Top),
        (area.bottom() - pointer.y, DockSide::Bottom),
    ];
    distances
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, side)| side)
}

/// Подсветка будущего места окна у края редактора.
fn paint_edge_preview(ctx: &egui::Context, area: Rect, side: DockSide) {
    let inner = area.shrink(EDGE);
    let rect = match side {
        DockSide::Left => Rect::from_min_size(inner.min, vec2(inner.width() * 0.25, inner.height())),
        DockSide::Right => Rect::from_min_max(pos2(inner.right() - inner.width() * 0.25, inner.top()), inner.max),
        DockSide::Top => Rect::from_min_size(inner.min, vec2(inner.width(), inner.height() * 0.25)),
        DockSide::Bottom => Rect::from_min_max(pos2(inner.left(), inner.bottom() - inner.height() * 0.25), inner.max),
        DockSide::Center => inner,
    };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("dock_edge_preview")));
    painter.rect(rect, theme::R_S, theme::accent().gamma_multiply(0.28), Stroke::new(2.0_f32, theme::accent()));
}

/// Разделители не сжимают область меньше `MIN_AREA` (если места хватает) — даже
/// после того, как окно редактора уменьшили.
fn enforce_min_sizes(state: &mut DockState<Panel>) {
    for surface in state.iter_surfaces_mut() {
        let Some(tree) = surface.node_tree_mut() else { continue };
        for node in tree.iter_mut() {
            let (fraction, size) = match node {
                Node::Horizontal { fraction, rect, .. } => (fraction, rect.width()),
                Node::Vertical { fraction, rect, .. } => (fraction, rect.height()),
                _ => continue,
            };
            if !size.is_finite() || size <= 2.0 * MIN_AREA {
                continue;
            }
            let min = MIN_AREA / size;
            *fraction = fraction.clamp(min, 1.0 - min);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels_of(desc: &LayoutDesc) -> Vec<Panel> {
        let mut panels = Vec::new();
        if let Some(main) = &desc.main {
            main.panels(&mut panels);
        }
        for (_, window) in &desc.windows {
            window.panels(&mut panels);
        }
        panels
    }

    #[test]
    fn every_preset_holds_every_window_exactly_once() {
        for preset in LayoutPreset::ALL {
            let desc = preset.desc();
            let mut panels = panels_of(&desc);
            panels.sort_by_key(|p| p.key());
            let mut all = Panel::ALL.to_vec();
            all.sort_by_key(|p| p.key());
            assert_eq!(panels, all, "{}", preset.name());
            // Через DockState и обратно — то же дерево.
            let state = desc.to_state();
            assert_eq!(LayoutDesc::of_state(&state, false, &[]), desc, "{}", preset.name());
        }
    }

    #[test]
    fn layout_text_round_trips() {
        let mut layout = DockLayout::default();
        layout.float(Panel::Inspector, Some(Rect::from_min_size(pos2(300.0, 200.0), vec2(380.0, 420.0))));
        layout.dock(Panel::Game, Panel::Scene, DockSide::Right).unwrap();
        layout.maximize_on_play = true;
        let text = layout.describe().to_text();
        assert!(!text.contains(',') && !text.contains(':') && !text.contains('\n'), "{text}");
        let parsed = LayoutDesc::parse(&text).unwrap();
        assert_eq!(parsed, layout.describe());
        assert_eq!(parsed.windows.len(), 1);
        assert_eq!(parsed.windows[0].0, Rect::from_min_size(pos2(300.0, 200.0), vec2(380.0, 420.0)));
        assert!(parsed.maximize_on_play);
        let mut restored = DockLayout::default();
        restored.load(&parsed);
        assert!(restored.is_floating(Panel::Inspector));
        assert!(restored.is_visible(Panel::Scene) && restored.is_visible(Panel::Game));
    }

    #[test]
    fn broken_layout_text_is_rejected_with_a_reason() {
        for broken in [
            "",
            "garbage",
            "piska-layout 0; main (L 0 scene)",
            "piska-layout 1; main (L 0 scene",
            "piska-layout 1; main (L 0 nonsense)",
            "piska-layout 1; main (H 0.5 (L 0 scene) (L 0 scene))",
            "piska-layout 1; main (H NaN (L 0 scene) (L 0 game))",
            "piska-layout 1; window 1 2 3",
            "piska-layout 1; main (L 0)",
        ] {
            assert!(LayoutDesc::parse(broken).is_err(), "{broken:?} must not parse");
        }
        assert!(LayoutDesc::parse("piska-layout 1; main -").unwrap().main.is_none());
    }

    #[test]
    fn closing_and_reopening_puts_a_window_back_with_its_group() {
        let mut layout = DockLayout::default();
        layout.close(Panel::Console);
        assert!(!layout.is_open(Panel::Console));
        layout.open(Panel::Console);
        let (surface, node, _) = layout.find(Panel::Console).unwrap();
        assert_eq!(Some((surface, node)), layout.find(Panel::Project).map(|(s, n, _)| (s, n)));
        assert!(layout.is_visible(Panel::Console), "a reopened window is shown");

        // Последняя вкладка области уносит область с собой; открытое снова окно встаёт сбоку.
        layout.close(Panel::Inspector);
        assert!(!layout.is_open(Panel::Inspector));
        layout.open(Panel::Inspector);
        assert!(layout.is_visible(Panel::Inspector));
        assert_eq!(layout.panels().len(), Panel::ALL.len());
    }

    #[test]
    fn docking_splits_and_maximize_toggles() {
        let mut layout = DockLayout::default();
        assert!(!layout.is_visible(Panel::Game), "Game is a hidden tab next to Scene");
        layout.dock(Panel::Game, Panel::Scene, DockSide::Bottom).unwrap();
        assert!(layout.is_visible(Panel::Game) && layout.is_visible(Panel::Scene));
        assert!(layout.dock(Panel::Game, Panel::Game, DockSide::Left).is_err());

        layout.toggle_maximize(Panel::Game);
        assert!(layout.is_visible(Panel::Game) && !layout.is_visible(Panel::Scene));
        layout.toggle_maximize(Panel::Game);
        assert!(layout.is_visible(Panel::Scene));

        layout.dock_to_edge(Panel::Console, DockSide::Left);
        let root = &layout.state.main_surface()[NodeIndex::root()];
        assert!(root.is_horizontal());
        assert_eq!(layout.state.main_surface()[NodeIndex::root().left()].tabs(), Some(&[Panel::Console][..]));
    }

    #[test]
    fn layout_names_are_cleaned_for_the_preferences_file() {
        assert_eq!(layout_name("  My: layout, v2 "), Some("My layout v2".to_string()));
        assert_eq!(layout_name(" ,: "), None);
    }
}
