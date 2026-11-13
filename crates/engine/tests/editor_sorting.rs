//! Сортировка, фильтры и группировка в окне Project; порядок выполнения скриптов в
//! Project Settings; порядок компонентов и детей. Окно и GPU не нужны (кроме галереи
//! снимков, она помечена `#[ignore]`).
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use engine::editor::{egui, Action, Editor, EditorConfig, Mode, Preferences};
use engine::prelude::*;

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

// ----- «руки» теста -----

struct Hands {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
        hands.frames(editor, 3);
        hands
    }

    fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn frames(&mut self, editor: &mut Editor, count: usize) {
        for _ in 0..count {
            self.frame(editor, vec![]);
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::button(pos, true)]);
        self.frame(editor, vec![Self::button(pos, false)]);
        self.frames(editor, 2);
    }

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![Self::button(from, true)]);
        for step in 1..=8 {
            let pos = from + (to - from) * (step as f32 / 8.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        self.frame(editor, vec![Self::button(to, false)]);
        self.frames(editor, 2);
    }

    fn center(&self, id: egui::Id) -> egui::Pos2 {
        self.ctx.read_response(id).expect("the widget is drawn in the last frame").rect.center()
    }

    /// Центр надписи `text` в последнем кадре (самая верхняя, если их несколько).
    fn find_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
        self.frame(editor, vec![]);
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
        found.into_iter().min_by(|a, b| a.y.total_cmp(&b.y))
    }
}

// ----- временный каталог ассетов с известными именами, размерами и временем -----

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!("piska_sorting_{tag}_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// Файл `rel` из `size` байт, правленный `age` секунд назад.
    fn file(&self, rel: &str, size: usize, age: u64) -> &Self {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![b'x'; size]).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
        self
    }

    fn dir(&self, rel: &str) -> &Self {
        std::fs::create_dir_all(self.root.join(rel)).unwrap();
        self
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Папка из девяти файлов и двух каталогов: все типы фильтров и разные размеры и даты.
fn fixture(tag: &str) -> Sandbox {
    let sandbox = Sandbox::new(tag);
    sandbox.dir("zeta_folder").dir("Alpha_folder");
    sandbox
        .file("hero.png", 900, 5_000)
        .file("Level10.scene", 300, 100)
        .file("Level2.scene", 700, 9_000)
        .file("coin.prefab", 120, 20_000)
        .file("music.ogg", 5_000, 50)
        .file("player.rs", 4_000, 30)
        .file("enemy.cpp", 2_500, 70_000)
        .file("ai.scenario", 60, 400_000)
        .file("notes.txt", 10, 1_000_000);
    sandbox
}

fn editor_over(assets: &Path, config: EditorConfig) -> Editor {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    Editor::new(app, config).expect("the editor starts")
}

fn shown(editor: &Editor) -> Vec<String> {
    editor.project_shown().to_vec()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ----- сортировка -----

#[test]
fn every_sort_key_orders_a_prepared_folder() {
    let sandbox = fixture("keys");
    let mut editor = editor_over(&sandbox.root, EditorConfig::default());
    let mut hands = Hands::new(&mut editor);
    let order = |editor: &mut Editor, hands: &mut Hands, key: &str, desc: bool| {
        editor.set_project_sort(key, desc);
        hands.frames(editor, 2);
        shown(editor)
    };
    let files = |all: Vec<String>| all.into_iter().filter(|n| !n.ends_with("_folder")).collect::<Vec<_>>();

    assert_eq!(editor.project_sort(), ("name", false), "the default is Name, ascending");
    assert_eq!(
        order(&mut editor, &mut hands, "name", false),
        names(&[
            "Alpha_folder", "zeta_folder", "ai.scenario", "coin.prefab", "enemy.cpp", "hero.png", "Level2.scene",
            "Level10.scene", "music.ogg", "notes.txt", "player.rs",
        ]),
        "folders first, then natural order without case"
    );
    assert_eq!(
        order(&mut editor, &mut hands, "name", true),
        names(&[
            "zeta_folder", "Alpha_folder", "player.rs", "notes.txt", "music.ogg", "Level10.scene", "Level2.scene",
            "hero.png", "enemy.cpp", "coin.prefab", "ai.scenario",
        ]),
        "descending flips the files; folders stay on top"
    );
    assert_eq!(
        order(&mut editor, &mut hands, "size", false),
        names(&[
            "Alpha_folder", "zeta_folder", "notes.txt", "ai.scenario", "coin.prefab", "Level10.scene", "Level2.scene",
            "hero.png", "enemy.cpp", "player.rs", "music.ogg",
        ])
    );
    assert_eq!(
        files(order(&mut editor, &mut hands, "size", true)),
        names(&["music.ogg", "player.rs", "enemy.cpp", "hero.png", "Level2.scene", "Level10.scene", "coin.prefab", "ai.scenario", "notes.txt"])
    );
    // Время правки: старые раньше. У каталогов время не задано — их порядок не проверяем.
    assert_eq!(
        files(order(&mut editor, &mut hands, "modified", false)),
        names(&["notes.txt", "ai.scenario", "enemy.cpp", "coin.prefab", "Level2.scene", "hero.png", "Level10.scene", "music.ogg", "player.rs"])
    );
    assert_eq!(
        files(order(&mut editor, &mut hands, "modified", true)),
        names(&["player.rs", "music.ogg", "Level10.scene", "hero.png", "Level2.scene", "coin.prefab", "enemy.cpp", "ai.scenario", "notes.txt"])
    );
    // Тип: по подписи («Audio» < «C++ source» < «Prefab» < «Rust script» < «Scenario script»
    // < «Scene» < «Text» < «Texture»), внутри типа — по имени.
    assert_eq!(
        files(order(&mut editor, &mut hands, "type", false)),
        names(&["music.ogg", "enemy.cpp", "coin.prefab", "player.rs", "ai.scenario", "Level2.scene", "Level10.scene", "notes.txt", "hero.png"])
    );
}

#[test]
fn sort_and_filter_choices_survive_a_preferences_round_trip() {
    let sandbox = fixture("prefs");
    let mut editor = editor_over(&sandbox.root, EditorConfig::default());
    editor.set_project_sort("size", true);
    editor.set_project_type_filters(&["scripts", "audio", "nonsense"]);
    let prefs: Preferences = editor.preferences().clone();
    assert_eq!(prefs.project_sort, "size");
    assert!(prefs.project_sort_desc);
    assert_eq!(prefs.project_types, "scripts+audio", "unknown names are dropped");

    // Новый редактор, настройки из «файла»: выбор тот же, и окно показывает то же.
    let mut next = editor_over(&sandbox.root, EditorConfig::default());
    assert_eq!(next.project_sort(), ("name", false));
    next.set_preferences(prefs);
    assert_eq!(next.project_sort(), ("size", true));
    assert_eq!(next.project_type_filters(), ["scripts", "audio"]);
    let mut hands = Hands::new(&mut next);
    hands.frames(&mut next, 2);
    assert_eq!(
        shown(&next).into_iter().filter(|n| !n.ends_with("_folder")).collect::<Vec<_>>(),
        names(&["music.ogg", "player.rs", "enemy.cpp", "ai.scenario"])
    );
}

// ----- фильтры -----

#[test]
fn type_chips_hide_other_types_in_tiles_list_and_search() {
    let sandbox = fixture("chips");
    let mut editor = editor_over(&sandbox.root, EditorConfig::default());
    let mut hands = Hands::new(&mut editor);
    let files = |editor: &Editor| shown(editor).into_iter().filter(|n| !n.ends_with("_folder")).collect::<Vec<_>>();
    assert_eq!(files(&editor).len(), 9, "nothing is filtered at first");

    // Щелчок по чипу «Scripts» — Rust, C++ и Scenario.
    let scripts = hands.center(Editor::project_filter_chip_id("scripts"));
    hands.click(&mut editor, scripts);
    assert_eq!(editor.project_type_filters(), ["scripts"]);
    assert_eq!(files(&editor), names(&["ai.scenario", "enemy.cpp", "player.rs"]));
    assert!(shown(&editor).contains(&"Alpha_folder".to_string()), "folders stay so that one can keep browsing");

    // Второй чип добавляется (несколько типов сразу).
    let audio = hands.center(Editor::project_filter_chip_id("audio"));
    hands.click(&mut editor, audio);
    assert_eq!(editor.project_type_filters(), ["scripts", "audio"]);
    assert_eq!(files(&editor), names(&["ai.scenario", "enemy.cpp", "music.ogg", "player.rs"]));

    // Тот же фильтр в режиме списка и в результатах поиска.
    editor.set_project_list_view(true);
    hands.frames(&mut editor, 2);
    assert_eq!(files(&editor), names(&["ai.scenario", "enemy.cpp", "music.ogg", "player.rs"]));
    editor.set_project_search("scen");
    hands.frames(&mut editor, 2);
    assert_eq!(shown(&editor), names(&["ai.scenario"]), "search results obey the chips: only scripts match `scen`");
    editor.set_project_type_filters(&[]);
    hands.frames(&mut editor, 2);
    assert_eq!(shown(&editor), names(&["ai.scenario", "Level2.scene", "Level10.scene"]));
    editor.set_project_search("");

    // «All» снимает все чипы; тип без файлов показывает подсказку и ничего не рисует.
    let all = hands.center(Editor::project_filter_chip_id("all"));
    editor.set_project_type_filters(&["models"]);
    hands.frames(&mut editor, 2);
    assert!(files(&editor).is_empty());
    hands.click(&mut editor, all);
    assert!(editor.project_type_filters().is_empty());
    assert_eq!(files(&editor).len(), 9);
}

// ----- столбцы списка -----

#[test]
fn clicking_a_list_column_header_resorts() {
    let sandbox = fixture("header");
    let mut editor = editor_over(&sandbox.root, EditorConfig::default());
    let mut hands = Hands::new(&mut editor);
    editor.set_project_list_view(true);
    hands.frames(&mut editor, 3);
    let files = |editor: &Editor| shown(editor).into_iter().filter(|n| !n.ends_with("_folder")).collect::<Vec<_>>();
    assert_eq!(editor.project_sort(), ("name", false));

    // Размер: первый щелчок — самые большие вверху.
    let size = hands.center(Editor::project_sort_header_id("size"));
    hands.click(&mut editor, size);
    assert_eq!(editor.project_sort(), ("size", true));
    assert_eq!(files(&editor)[..3], names(&["music.ogg", "player.rs", "enemy.cpp"])[..]);
    // Повторный щелчок — направление меняется.
    hands.click(&mut editor, size);
    assert_eq!(editor.project_sort(), ("size", false));
    assert_eq!(files(&editor)[..3], names(&["notes.txt", "ai.scenario", "coin.prefab"])[..]);
    // Другой столбец — его ключ.
    let kind = hands.center(Editor::project_sort_header_id("type"));
    hands.click(&mut editor, kind);
    assert_eq!(editor.project_sort(), ("type", false));
    assert_eq!(files(&editor)[0], "music.ogg");
    let name = hands.center(Editor::project_sort_header_id("name"));
    hands.click(&mut editor, name);
    assert_eq!(editor.project_sort(), ("name", false));
    assert_eq!(files(&editor)[0], "ai.scenario");
    assert_eq!(editor.preferences().project_sort, "name", "the choice goes to the preferences");
}

// ----- группировка Code -----

#[test]
fn the_code_place_groups_scripts_by_language() {
    let assets = Sandbox::new("codeassets");
    assets.file("readme.txt", 5, 10).file("scripts/intro.scenario", 100, 10);
    let code = Sandbox::new("code");
    code.file("player.rs", 100, 10)
        .file("enemy.rs", 100, 10)
        .file("native/ai.cpp", 100, 10)
        .file("native/ai.h", 100, 10);
    let mut editor = editor_over(&assets.root, EditorConfig::default().code_dir(&code.root));
    let mut hands = Hands::new(&mut editor);
    let row = hands.center(Editor::project_tree_row_id("\u{1}code"));
    hands.click(&mut editor, row);
    hands.frames(&mut editor, 2);
    assert_eq!(editor.project_group_headings(), ["Rust  ·  2", "C++  ·  2", "Scenario  ·  1"]);
    // Внутри групп — по имени; группы — в порядке Rust, C++, Scenario.
    assert_eq!(shown(&editor), names(&["enemy.rs", "player.rs", "ai.cpp", "ai.h", "intro.scenario"]));
    editor.set_project_sort("name", true);
    hands.frames(&mut editor, 2);
    assert_eq!(shown(&editor), names(&["player.rs", "enemy.rs", "ai.h", "ai.cpp", "intro.scenario"]));
}

// ----- Add Component: категории из реестра -----

#[test]
fn engine_components_carry_their_menu_category() {
    let app = engine::app();
    let category = |name: &str| app.world.registry.category_of(name);
    assert_eq!(category("SpriteRenderer"), Some("Rendering"));
    assert_eq!(category("Camera"), Some("Rendering"));
    assert_eq!(category("Rigidbody2D"), Some("Physics 2D"));
    assert_eq!(category("Collider2D"), Some("Physics 2D"));
    assert_eq!(category("Rigidbody"), Some("Physics 3D"));
    assert_eq!(category("CharacterController"), Some("Physics 3D"));
    assert_eq!(category("AudioSource"), Some("Audio"));
    assert_eq!(category("Canvas"), Some("UI"));
    assert_eq!(category("Button"), Some("UI"));
    assert_eq!(category("ParticleSystem"), Some("Effects"));
    assert_eq!(category("ScenarioRunner"), Some("Scripts (Scenario)"));
}

// ----- порядок выполнения -----

#[derive(Default)]
struct Calls(Vec<&'static str>);

fn log(ctx: &mut Ctx, who: &'static str) {
    ctx.resource_or_default::<Calls>().0.push(who);
}

component! {
    /// Скрипт «Lead» для проверки порядка выполнения.
    pub struct Lead {}
}
impl Component for Lead {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        log(ctx, "Lead");
    }
}

component! {
    /// Скрипт «Tail»: порядок в коде 0, как у Lead.
    pub struct Tail {}
}
impl Component for Tail {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        log(ctx, "Tail");
    }
}

fn ordered_editor(assets: &Path) -> Editor {
    let mut app = engine::app();
    app.register::<Lead>();
    app.register::<Tail>();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    // Один объект, у которого Tail добавлен раньше Lead.
    let id = app.world.scene.spawn("x");
    let obj = app.world.scene.get_mut(id).unwrap();
    obj.add(Tail::default());
    obj.add(Lead::default());
    Editor::new(app, EditorConfig::default()).expect("the editor starts")
}

fn update_order(editor: &mut Editor) -> Vec<&'static str> {
    editor.app.world.resources.get_or_insert_with(Calls::default).0.clear();
    editor.advance(DT);
    editor.app.world.resources.get_or_insert_with(Calls::default).0.clone()
}

#[test]
fn execution_order_edits_change_the_runtime_order_and_the_settings_file() {
    let assets = Sandbox::new("order");
    assets.file("readme.txt", 5, 10);
    let mut editor = ordered_editor(&assets.root);
    editor.apply(Action::Play);
    assert_eq!(editor.mode(), Mode::Playing);
    editor.advance(DT);
    assert_eq!(update_order(&mut editor), ["Tail", "Lead"], "ties keep the order the components were added in");

    // Правка в черновике ничего не меняет, пока не нажат Apply.
    editor.set_script_order("Lead", -10);
    assert!(editor.script_order_is_dirty());
    assert_eq!(update_order(&mut editor), ["Tail", "Lead"]);
    assert!(!assets.root.join("project.settings").exists());

    // Apply: порядок меняется сразу, в Play, и записывается в файл.
    editor.apply_script_order().expect("settings are saved");
    assert!(!editor.script_order_is_dirty());
    assert_eq!(update_order(&mut editor), ["Lead", "Tail"]);
    let text = std::fs::read_to_string(assets.root.join("project.settings")).expect("the file is written");
    let saved = ProjectSettings::from_ron(&text, "project.settings").unwrap();
    assert_eq!(saved.order_of("Lead"), Some(-10));
    assert_eq!(editor.app.world.project_settings(), saved);

    // Порядок переживает Stop и новый Play.
    editor.apply(Action::Stop);
    editor.apply(Action::Play);
    editor.advance(DT);
    assert_eq!(update_order(&mut editor), ["Lead", "Tail"]);

    // Revert возвращает черновик к действующему; убрать тип — порядок из кода.
    editor.set_script_order("Tail", -50);
    editor.revert_script_order();
    assert_eq!(editor.script_order_draft(), [("Lead".to_string(), -10)]);
    editor.remove_script_order("Lead");
    editor.apply_script_order().unwrap();
    assert_eq!(update_order(&mut editor), ["Tail", "Lead"]);

    // Новый редактор над тем же каталогом читает файл при подключении ассетов.
    editor.set_script_order("Lead", 5);
    editor.apply_script_order().unwrap();
    editor.apply(Action::Stop);
    let mut next = ordered_editor(&assets.root);
    assert_eq!(next.app.world.project_settings().order_of("Lead"), Some(5), "mount_assets reads project.settings");
    next.apply(Action::Play);
    next.advance(DT);
    assert_eq!(update_order(&mut next), ["Tail", "Lead"]);
}

#[test]
fn the_settings_window_drags_rows_and_applies_with_a_click() {
    let assets = Sandbox::new("window");
    assets.file("readme.txt", 5, 10);
    let mut editor = ordered_editor(&assets.root);
    editor.show_project_settings_page("script-order");
    editor.set_script_order("Lead", -50);
    editor.set_script_order("Tail", 50);
    let mut hands = Hands::new(&mut editor);
    hands.frames(&mut editor, 3);
    assert!(editor.is_project_settings_open());
    assert_eq!(editor.script_order_draft(), [("Lead".to_string(), -50), ("Tail".to_string(), 50)]);

    // Тащим Tail за ручку выше Lead: ему достаётся число меньше, чем у Lead.
    let tail = editor.script_order_row_rect("Tail").expect("Tail has a row");
    let lead = editor.script_order_row_rect("Lead").expect("Lead has a row");
    assert!(lead.top() < tail.top(), "rows are listed by order");
    let handle = egui::pos2(tail.left() + 12.0, tail.center().y);
    hands.drag(&mut editor, handle, egui::pos2(lead.center().x, lead.top() + 2.0));
    assert_eq!(editor.script_order_draft(), [("Tail".to_string(), -150), ("Lead".to_string(), -50)]);
    assert!(editor.script_order_is_dirty());

    // Apply — настоящим щелчком по кнопке.
    let apply = hands.center(Editor::script_order_apply_id());
    hands.click(&mut editor, apply);
    assert!(!editor.script_order_is_dirty(), "the click applied the draft");
    let text = std::fs::read_to_string(assets.root.join("project.settings")).unwrap();
    assert!(text.contains("Tail") && text.contains("-150"), "{text}");
    editor.apply(Action::Play);
    editor.advance(DT);
    assert_eq!(update_order(&mut editor), ["Tail", "Lead"]);

    // Revert — щелчком: черновик возвращается к записанному.
    editor.set_script_order("Lead", -500);
    hands.frames(&mut editor, 2);
    let revert = hands.center(Editor::script_order_revert_id());
    hands.click(&mut editor, revert);
    assert_eq!(editor.script_order_draft()[1], ("Lead".to_string(), -50));
}

// ----- порядок компонентов и детей -----

#[test]
fn components_can_be_reordered_and_it_is_undoable() {
    let assets = Sandbox::new("components");
    assets.file("readme.txt", 5, 10);
    let mut editor = ordered_editor(&assets.root);
    // Объект находят заново: после Play и Stop сцена другая, старые id протухают.
    let kinds = |editor: &Editor| {
        let scene = &editor.app.world.scene;
        let id = scene.find_by_name("x").unwrap();
        scene.get(id).unwrap().components().map(|c| c.type_name()).collect::<Vec<_>>()
    };
    let id = editor.app.world.scene.find_by_name("x").unwrap();
    assert_eq!(kinds(&editor), ["Tail", "Lead"]);

    editor.apply(Action::MoveComponent { object: id, from: 1, to: 0 });
    assert_eq!(kinds(&editor), ["Lead", "Tail"]);
    // При равном порядке выполнения новый порядок компонентов определяет порядок вызова.
    editor.apply(Action::Play);
    editor.advance(DT);
    assert_eq!(update_order(&mut editor), ["Lead", "Tail"]);
    editor.apply(Action::Stop);
    let id = editor.app.world.scene.find_by_name("x").unwrap();

    // Выход за границы и «на то же место» — ничего не меняют.
    editor.apply(Action::MoveComponent { object: id, from: 0, to: 9 });
    editor.apply(Action::MoveComponent { object: id, from: 1, to: 1 });
    assert_eq!(kinds(&editor), ["Lead", "Tail"]);
    editor.apply(Action::Undo);
    assert_eq!(kinds(&editor), ["Tail", "Lead"], "undo restores the order");
}

#[test]
fn dragging_a_component_header_in_the_inspector_reorders_components() {
    let assets = Sandbox::new("inspector");
    assets.file("readme.txt", 5, 10);
    let mut editor = ordered_editor(&assets.root);
    let id = editor.app.world.scene.find_by_name("x").unwrap();
    editor.apply(Action::Select(Some(id)));
    let mut hands = Hands::new(&mut editor);
    hands.frames(&mut editor, 3);
    let kinds = |editor: &Editor| {
        let scene = &editor.app.world.scene;
        let id = scene.find_by_name("x").unwrap();
        scene.get(id).unwrap().components().map(|c| c.type_name()).collect::<Vec<_>>()
    };
    assert_eq!(kinds(&editor), ["Tail", "Lead"]);
    let tail = hands.find_text(&mut editor, "Tail").expect("the inspector shows a Tail card");
    let lead = hands.find_text(&mut editor, "Lead").expect("the inspector shows a Lead card");
    assert!(tail.y < lead.y);
    hands.drag(&mut editor, tail, lead);
    assert_eq!(kinds(&editor), ["Lead", "Tail"], "the dragged component took the place of the one it was dropped on");
    editor.apply(Action::Undo);
    assert_eq!(kinds(&editor), ["Tail", "Lead"]);
}

#[test]
fn children_sort_alphabetically_once_and_undo_restores() {
    let assets = Sandbox::new("children");
    assets.file("readme.txt", 5, 10);
    let mut app = engine::app();
    engine::mount_assets(&mut app, &assets.root).unwrap();
    let parent = app.world.scene.spawn("parent");
    for name in ["b", "a10", "a2", "A1"] {
        let child = app.world.scene.spawn(name);
        app.world.scene.set_parent(child, Some(parent)).unwrap();
    }
    let mut editor = Editor::new(app, EditorConfig::default()).unwrap();
    let child_names = |editor: &Editor| {
        let scene = &editor.app.world.scene;
        let parent = scene.find_by_name("parent").unwrap();
        scene.get(parent).unwrap().children().iter().map(|c| scene.get(*c).unwrap().name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(child_names(&editor), ["b", "a10", "a2", "A1"]);
    let parent = editor.app.world.scene.find_by_name("parent").unwrap();
    editor.apply(Action::SortChildren { parent: Some(parent) });
    assert_eq!(child_names(&editor), ["A1", "a2", "a10", "b"], "case is ignored, numbers compare by value");
    assert!(editor.is_dirty());
    editor.apply(Action::Undo);
    assert_eq!(child_names(&editor), ["b", "a10", "a2", "A1"]);
}

// ----- галерея снимков (нужен GPU) -----

/// `ENGINE_SHOTS_DIR=out cargo test -p engine --test editor_sorting -- --ignored gallery`
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn sorting_screenshot_gallery() {
    use engine::editor::{capture, capture_with_input, BottomTab};
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();

    let sandbox = fixture("gallery");
    let code = Sandbox::new("gallerycode");
    code.file("player.rs", 3_200, 30)
        .file("enemy.rs", 1_800, 600)
        .file("native/ai.cpp", 5_000, 4_000)
        .file("native/ai.h", 700, 4_000)
        .file("intro.scenario", 900, 90_000);
    let mut editor = editor_over(&sandbox.root, EditorConfig::default().code_dir(&code.root));
    editor.set_bottom_tab(BottomTab::Project);

    // 1. Список с заголовками, сортировка по размеру.
    editor.set_project_list_view(true);
    editor.set_project_sort("size", true);
    let image = capture(&mut editor, 1440, 860, 10).unwrap();
    image.save(dir.join("s_project_list_size_docked.png")).unwrap();
    // Дальше окно Project развёрнуто, чтобы были видны все строки.
    editor.apply(Action::Layout(engine::editor::LayoutAction::ToggleMaximize(engine::editor::Panel::Project)));
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("s_project_list_size.png")).unwrap();

    // 2. Чипы Scripts + Audio в плитках.
    editor.set_project_list_view(false);
    editor.set_project_sort("name", false);
    editor.set_project_type_filters(&["scripts", "audio"]);
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("s_project_chips.png")).unwrap();

    // 3. Code по языкам, список.
    editor.set_project_type_filters(&[]);
    editor.set_project_list_view(true);
    let mut hands = Hands::new(&mut editor);
    let row = hands.center(Editor::project_tree_row_id("\u{1}code"));
    hands.click(&mut editor, row);
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("s_project_code_groups.png")).unwrap();

    // 4. Окно Execution Order.
    let assets = Sandbox::new("galleryorder");
    assets.file("readme.txt", 5, 10);
    let mut ordered = ordered_editor(&assets.root);
    ordered.set_script_order("Lead", -100);
    ordered.set_script_order("Tail", 100);
    ordered.set_script_order("Rigidbody2D", -20);
    ordered.set_script_order("Ghost", 0);
    ordered.apply_script_order().unwrap();
    ordered.set_script_order("AudioSource", 40);
    ordered.show_project_settings_page("script-order");
    capture(&mut ordered, 1440, 860, 12).unwrap().save(dir.join("s_execution_order.png")).unwrap();

    // 5. Меню Add Component с категориями и «Recent».
    let mut menu = ordered_editor(&assets.root);
    menu.set_preferences(Preferences {
        recent_components: vec!["Rigidbody2D".to_string(), "Lead".to_string(), "AudioSource".to_string()],
        ..Preferences::default()
    });
    let x = menu.app.world.scene.find_by_name("x").unwrap();
    let extra = menu.app.world.scene.spawn("Empty");
    let _ = x;
    menu.apply(Action::Select(Some(extra)));
    menu.apply(Action::AddComponent { object: extra, kind: "Rigidbody2D".to_string() });
    let mut menu_hands = Hands::new(&mut menu);
    let button = menu_hands.find_text(&mut menu, "Add Component").expect("the inspector has the Add Component button");
    let image = capture_with_input(&mut menu, 1440, 860, 24, |frame, _| match frame {
        8 => vec![egui::Event::PointerMoved(button)],
        10 => vec![Hands::button(button, true)],
        11 => vec![Hands::button(button, false)],
        _ => vec![],
    })
    .unwrap();
    image.save(dir.join("s_add_component.png")).unwrap();
    // Поиск: лучшие совпадения первыми.
    let image = capture_with_input(&mut menu, 1440, 860, 30, |frame, _| match frame {
        8 => vec![egui::Event::PointerMoved(button)],
        10 => vec![Hands::button(button, true)],
        11 => vec![Hands::button(button, false)],
        16 => vec![egui::Event::Text("ren".to_string())],
        _ => vec![],
    })
    .unwrap();
    image.save(dir.join("s_add_component_search.png")).unwrap();
}
