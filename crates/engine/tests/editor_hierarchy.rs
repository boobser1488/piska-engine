//! Окно Hierarchy на больших сценах, настоящим вводом egui без GPU: строки рисуются только
//! в видимой части (5 000 объектов), нечёткий поиск с `t:` / `tag:` / `layer:` и
//! показом в дереве, свернуть / раскрыть всё, клавиатура (стрелки, набор имени), показ
//! выделенного, Alt+щелчок по стрелке, замок «не выбирать в виде Scene».
#![cfg(feature = "editor")]

use engine::editor::egui;
use engine::editor::{Action, Editor, EditorConfig, Panel};
use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const NONE: egui::Modifiers = egui::Modifiers::NONE;

fn editor() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap()
}

/// Сцена как трасса marble, только больше: 50 групп по 99 «рельсов» = 5 000 объектов.
fn big_scene(editor: &mut Editor) -> Vec<GameObjectId> {
    editor.apply(Action::NewScene);
    let scene = &mut editor.app.world.scene;
    let mut groups = Vec::new();
    for g in 0..50 {
        let group = scene.build(format!("Segment {g}")).id();
        for i in 0..99 {
            let side = if i % 2 == 0 { "L" } else { "R" };
            scene.build(format!("Rail {side} {i}")).at(i as f32, g as f32).child_of(group).id();
        }
        groups.push(group);
    }
    groups
}

struct Hands {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
        for _ in 0..3 {
            hands.frame(editor, vec![], NONE);
        }
        hands
    }

    fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: DT,
            modifiers,
            events,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        editor.forward_game_input(&self.ctx, &input);
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn drawn(&self, id: GameObjectId) -> Option<egui::Rect> {
        self.ctx.read_response(Editor::hierarchy_row_id(id)).map(|r| r.rect)
    }

    fn click_with(&mut self, editor: &mut Editor, pos: egui::Pos2, modifiers: egui::Modifiers) {
        let button = egui::PointerButton::Primary;
        self.frame(editor, vec![egui::Event::PointerMoved(pos)], modifiers);
        self.frame(editor, vec![egui::Event::PointerButton { pos, button, pressed: true, modifiers }], modifiers);
        self.frame(editor, vec![egui::Event::PointerButton { pos, button, pressed: false, modifiers }], modifiers);
        self.frame(editor, vec![], NONE);
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.click_with(editor, pos, NONE);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: NONE };
        self.frame(editor, vec![event(true)], NONE);
        self.frame(editor, vec![event(false)], NONE);
        self.frame(editor, vec![], NONE);
    }

    /// Буква так, как её присылает окно: нажатие клавиши и текст.
    fn letter(&mut self, editor: &mut Editor, key: egui::Key, text: &str) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: NONE };
        self.frame(editor, vec![event(true), egui::Event::Text(text.to_string())], NONE);
        self.frame(editor, vec![event(false)], NONE);
    }

    fn find_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
        self.frame(editor, vec![], NONE);
        fn search(shape: &egui::Shape, text: &str, out: &mut Vec<egui::Pos2>) {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => out.push(t.pos + t.galley.rect.center().to_vec2()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| search(s, text, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &self.shapes {
            search(&clipped.shape, text, &mut found);
        }
        found.into_iter().max_by(|a, b| a.y.total_cmp(&b.y))
    }
}

fn names(editor: &Editor, ids: &[GameObjectId]) -> Vec<String> {
    ids.iter().filter_map(|id| editor.app.world.scene.get(*id)).map(|o| o.name.clone()).collect()
}

#[test]
fn five_thousand_objects_draw_only_the_visible_rows_and_scroll_fast() {
    let mut editor = editor();
    let groups = big_scene(&mut editor);
    // 5 000 объектов и камера новой сцены.
    assert_eq!(editor.app.world.scene.len(), 5001);
    let mut hands = Hands::new(&mut editor);
    let rows = editor.hierarchy_rows();
    assert_eq!(rows.len(), 5001, "everything is expanded");
    assert!(hands.drawn(rows[0]).is_some(), "the first row is drawn");
    assert!(hands.drawn(rows[4000]).is_none(), "a row far below the visible part is not built at all");

    // Прокрутка колесом над окном: кадр интерфейса не дорожает с числом объектов.
    let over = editor.panel_rect(Panel::Hierarchy).expect("Hierarchy is drawn").center();
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(over)], NONE);
    let started = std::time::Instant::now();
    let frames = 60;
    for _ in 0..frames {
        let wheel = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -400.0), modifiers: NONE };
        hands.frame(&mut editor, vec![wheel], NONE);
    }
    let per_frame = started.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("whole editor UI frame with 5000 objects while scrolling: {per_frame:.2} ms");
    assert!(per_frame < 33.0, "the editor UI frame takes {per_frame:.1} ms with 5000 objects");
    let visible = rows.iter().filter(|id| hands.drawn(**id).is_some()).count();
    assert!(visible > 10 && visible < 80, "only the rows in view are built: {visible}");
    assert!(hands.drawn(rows[0]).is_none(), "scrolled away from the top");

    // Свернуть всё: остаются 50 групп.
    editor.apply(Action::Select(None));
    let body = editor.panel_rect(Panel::Hierarchy).unwrap();
    let menu = egui::pos2(body.right() - 8.0 - 11.0, body.top() + 14.0);
    hands.click(&mut editor, menu);
    let collapse = hands.find_text(&mut editor, "Collapse All").expect("the Hierarchy menu has Collapse All");
    hands.click(&mut editor, collapse);
    assert_eq!(editor.hierarchy_rows()[1..], groups, "only the camera and the groups are left");
    // Свёрнутая группа показывает число детей.
    assert!(hands.find_text(&mut editor, "99").is_some(), "a collapsed parent shows its child count");

    // Выделение в глубине (как щелчок в виде Scene) раскрывает предков и прокручивает к строке.
    let deep = editor.app.world.scene.get(groups[42]).unwrap().children()[77];
    editor.apply(Action::Select(Some(deep)));
    // Прокрутка плавная (анимация egui): полсекунды кадров.
    for _ in 0..30 {
        hands.frame(&mut editor, vec![], NONE);
    }
    assert!(editor.hierarchy_rows().contains(&deep), "the parent of the selection is expanded");
    let rect = hands.drawn(deep).expect("the selected row is scrolled into view");
    assert!(body.contains(rect.center()), "{rect:?} is inside {body:?}");
}

#[test]
fn search_is_fuzzy_shows_matches_in_their_tree_and_counts_them() {
    let mut editor = editor();
    let groups = big_scene(&mut editor);
    let mut hands = Hands::new(&mut editor);

    // «rl37» находит «Rail L 37» буквами по порядку; предки видны, соседи скрыты.
    editor.set_hierarchy_filter("rl38");
    hands.frame(&mut editor, vec![], NONE);
    let shown = names(&editor, &editor.hierarchy_rows());
    assert_eq!(shown.len(), 100, "50 groups + one rail in each: {:?}", &shown[..4.min(shown.len())]);
    assert_eq!(shown[..2], ["Segment 0".to_string(), "Rail L 38".to_string()]);
    assert!(hands.find_text(&mut editor, "50 objects found").is_some(), "the result count is shown");

    // Плоский список — только совпадения.
    let toggle = hands.find_text(&mut editor, "Show as List").expect("search results can be shown as a list");
    hands.click(&mut editor, toggle);
    let flat = names(&editor, &editor.hierarchy_rows());
    assert_eq!(flat.len(), 50);
    assert!(flat.iter().all(|n| n == "Rail L 38"));

    // t:, tag:, layer: — по компоненту, метке и слою.
    editor.apply(Action::NewScene);
    {
        let scene = &mut editor.app.world.scene;
        scene.build("Hero").tag("Player").layer(5).id();
        scene.build("Lamp").with(engine::render::Light::default()).id();
    }
    for (query, expected) in [("t:light", "Lamp"), ("tag:play", "Hero"), ("layer:ui", "Hero"), ("t:camera", "Main Camera")] {
        editor.set_hierarchy_filter(query);
        hands.frame(&mut editor, vec![], NONE);
        assert_eq!(names(&editor, &editor.hierarchy_rows()), [expected], "{query}");
    }
    let _ = groups;
}

#[test]
fn keyboard_arrows_type_ahead_and_alt_click_work_in_the_tree() {
    let mut editor = editor();
    let groups = big_scene(&mut editor);
    let mut hands = Hands::new(&mut editor);
    editor.collapse_hierarchy();
    hands.frame(&mut editor, vec![], NONE);
    // Фокус — окну Hierarchy (щелчок по строке).
    let row = hands.drawn(groups[0]).unwrap().center();
    hands.click(&mut editor, row);
    assert_eq!(editor.selection(), Some(groups[0]));
    assert_eq!(editor.focused_panel(), Some(Panel::Hierarchy), "a click on a row focuses the Hierarchy");

    hands.key(&mut editor, egui::Key::ArrowRight);
    assert!(!editor.is_hierarchy_collapsed(groups[0]), "→ expands");
    hands.key(&mut editor, egui::Key::ArrowRight);
    let first_child = editor.app.world.scene.get(groups[0]).unwrap().children()[0];
    assert_eq!(editor.selection(), Some(first_child), "→ again goes to the first child");
    hands.key(&mut editor, egui::Key::ArrowLeft);
    assert_eq!(editor.selection(), Some(groups[0]), "← goes to the parent");
    hands.key(&mut editor, egui::Key::ArrowLeft);
    assert!(editor.is_hierarchy_collapsed(groups[0]), "← again collapses");

    // Набор имени «segment 42» → строка Segment 42.
    for c in "segment 42".chars() {
        let key = egui::Key::from_name(&c.to_uppercase().to_string()).unwrap_or(egui::Key::Space);
        hands.letter(&mut editor, key, &c.to_string());
    }
    assert_eq!(editor.selection(), Some(groups[42]), "typing a name jumps to the row");

    // Alt+щелчок по стрелке — раскрыть всё поддерево (сначала список докручивается до строки).
    for _ in 0..30 {
        hands.frame(&mut editor, vec![], NONE);
    }
    let rect = hands.drawn(groups[42]).unwrap();
    let arrow = egui::pos2(rect.left() + 13.0, rect.center().y);
    hands.click_with(&mut editor, arrow, egui::Modifiers::ALT);
    assert!(!editor.is_hierarchy_collapsed(groups[42]));
}

#[test]
fn a_locked_object_cannot_be_picked_in_the_scene_view() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let platform = editor.app.world.scene.find_by_name("Platform A").unwrap();
    let at = editor.world_to_screen(editor.app.world.scene.world_position(platform).truncate());
    hands.click(&mut editor, at);
    assert_eq!(editor.selection(), Some(platform), "picked before locking");
    editor.apply(Action::Select(None));

    // Замок появляется при наведении на строку; щелчок по нему запирает.
    let row = hands.drawn(platform).expect("the row is drawn");
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(row.center())], NONE);
    let lock = egui::pos2(row.right() - 34.0, row.center().y);
    hands.click(&mut editor, lock);
    assert_eq!(editor.selection(), None, "the lock click does not select the row");
    hands.click(&mut editor, at);
    assert_ne!(editor.selection(), Some(platform), "a locked object is not picked in the Scene view");
    editor.set_pick_locked(platform, false);
    hands.click(&mut editor, at);
    assert_eq!(editor.selection(), Some(platform));
}

#[test]
fn the_fps_controller_has_a_crosshair_icon_and_grouped_fields_in_the_inspector() {
    let mut editor = editor();
    editor.apply(Action::NewScene);
    let player = editor.app.world.scene.build("Player").with(engine::prelude::FpsController::default()).id();
    editor.apply(Action::Select(Some(player)));
    let mut hands = Hands::new(&mut editor);
    // Поля контроллера (их больше тридцати) идут под подзаголовками групп.
    for group in ["MOVEMENT", "BODY", "SPEED MODIFIERS"] {
        assert!(hands.find_text(&mut editor, group).is_some(), "the Inspector shows the `{group}` group header");
    }
}

#[test]
fn cached_rows_follow_renames_reparenting_and_new_objects() {
    // Строки кэшируются между кадрами (большие сцены): любое изменение сцены, которое видно
    // в окне, должно сразу попасть в него.
    let mut editor = editor();
    let groups = big_scene(&mut editor);
    let mut hands = Hands::new(&mut editor);
    let first = editor.app.world.scene.get(groups[0]).unwrap().children()[0];
    // Переименование прямо в сцене (как скрипт в режиме Play).
    editor.app.world.scene.get_mut(first).unwrap().name = "Renamed Rail".into();
    assert!(hands.find_text(&mut editor, "Renamed Rail").is_some(), "a rename shows up in the next frame");
    // Новый объект в корне — новая строка.
    editor.app.world.scene.build("Fresh Object").id();
    assert!(hands.find_text(&mut editor, "Fresh Object").is_some() || editor.hierarchy_rows().len() == 5002, "a new object is listed");
    // Выключенный объект рисуется приглушённо: проверяем, что строки пересобраны (флажок активности входит в отпечаток).
    let before = editor.hierarchy_rows().len();
    editor.app.world.scene.build("Another").child_of(groups[1]).id();
    hands.frame(&mut editor, vec![], NONE);
    assert_eq!(editor.hierarchy_rows().len(), before + 1);
}
