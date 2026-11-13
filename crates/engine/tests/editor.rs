//! Редактор без окна: интерфейс гоняется через `egui::Context` с синтетическим
//! вводом, действия — через `Editor::apply`. GPU нужен только тесту со снимками,
//! он помечен `#[ignore]`.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine::editor::egui;
use engine::editor::{Action, Editor, Mode, ViewTab};
use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;

fn editor_with_assets(assets: &Path) -> Editor {
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene"))
        .expect("level1.scene loads")
}

fn editor() -> Editor {
    editor_with_assets(Path::new(ASSETS))
}

/// Копия каталога ассетов во временной папке — для тестов, которые пишут файлы.
fn temp_assets() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "piska_editor_test_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    copy(Path::new(ASSETS), &dir);
    dir
}

fn find(editor: &Editor, name: &str) -> GameObjectId {
    editor
        .app
        .world
        .scene
        .find_by_name(name)
        .unwrap_or_else(|| panic!("object `{name}` exists"))
}

fn position(editor: &Editor, name: &str) -> Vec2 {
    editor
        .app
        .world
        .scene
        .world_position(find(editor, name))
        .truncate()
}

/// «Руки» теста: мышь и клавиатура через egui.
struct Hands {
    ctx: egui::Context,
    time: f64,
    /// Что нарисовал последний кадр — по нему ищутся надписи (`find_text`).
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Hands {
    fn new(editor: &mut Editor) -> Self {
        let mut hands = Self {
            ctx: egui::Context::default(),
            time: 0.0,
            shapes: Vec::new(),
        };
        // Два кадра, чтобы раскладка панелей устоялась.
        hands.frame(editor, vec![]);
        hands.frame(editor, vec![]);
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
        // Как в настоящем кадре: сначала ввод уходит игре, потом строится интерфейс.
        editor.forward_game_input(&self.ctx, &input);
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    /// Центр надписи `text` в последнем кадре (кнопка, пункт меню, подпись). Если таких
    /// надписей несколько — самая верхняя на экране (строка меню выше панелей).
    fn find_text(&mut self, editor: &mut Editor, text: &str) -> Option<egui::Pos2> {
        self.frame(editor, vec![]);
        fn search(shape: &egui::Shape, text: &str, out: &mut Vec<egui::Pos2>) {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    out.push(t.pos + t.galley.rect.center().to_vec2());
                }
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

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(&mut self, editor: &mut Editor, pos: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        self.frame(editor, vec![Self::button(pos, true)]);
        self.frame(editor, vec![Self::button(pos, false)]);
        self.frame(editor, vec![]);
    }

    fn drag(&mut self, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        self.frame(editor, vec![egui::Event::PointerMoved(from)]);
        self.frame(editor, vec![Self::button(from, true)]);
        for step in 1..=8 {
            let pos = from + (to - from) * (step as f32 / 8.0);
            self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        self.frame(editor, vec![Self::button(to, false)]);
        self.frame(editor, vec![]);
    }

    fn key(&mut self, editor: &mut Editor, key: egui::Key, modifiers: egui::Modifiers, pressed: bool) {
        self.frame(
            editor,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            }],
        );
    }

    fn press(&mut self, editor: &mut Editor, key: egui::Key, modifiers: egui::Modifiers) {
        self.key(editor, key, modifiers, true);
        self.key(editor, key, modifiers, false);
    }

    fn row_center(&self, id: GameObjectId) -> egui::Pos2 {
        self.ctx
            .read_response(Editor::hierarchy_row_id(id))
            .expect("the object has a row in the Hierarchy")
            .rect
            .center()
    }
}

#[test]
fn opens_the_scene_in_edit_mode() {
    let editor = editor();
    assert_eq!(editor.mode(), Mode::Edit);
    assert_eq!(editor.scene_path(), Some("scenes/level1.scene"));
    assert_eq!(editor.selection(), None);
    assert!(!editor.is_dirty());
    assert!(!editor.can_undo());
    assert_eq!(editor.app.world.scene.name, "Level 1");
    assert_eq!(editor.window_title(), "Piska Editor — Level 1");
}

#[test]
fn clicking_a_hierarchy_row_selects_the_object() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = find(&editor, "Player");

    hands.click(&mut editor, hands.row_center(player));
    assert_eq!(editor.selection(), Some(player));

    let ground = find(&editor, "Ground");
    hands.click(&mut editor, hands.row_center(ground));
    assert_eq!(editor.selection(), Some(ground));
    assert!(!editor.is_dirty(), "selecting is not an edit");
}

#[test]
fn clicking_in_the_scene_view_picks_the_sprite_under_the_cursor() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = find(&editor, "Player");

    let on_player = editor.world_to_screen(position(&editor, "Player"));
    assert!(editor.viewport_rect().contains(on_player), "the player is in view");
    hands.click(&mut editor, on_player);
    assert_eq!(editor.selection(), Some(player));

    // Игрок стоит перед холмами (слой сортировки выше) — выбран он, а не фон.
    // Щелчок по пустому небу снимает выделение.
    let sky = editor.world_to_screen(Vec2::new(-7.0, 6.0));
    assert!(editor.viewport_rect().contains(sky));
    hands.click(&mut editor, sky);
    assert_eq!(editor.selection(), None);
}

#[test]
fn dragging_the_move_gizmo_moves_the_object_and_undo_restores_it() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = find(&editor, "Player");
    let before = position(&editor, "Player");
    editor.apply(Action::Select(Some(player)));
    hands.frame(&mut editor, vec![]);

    // Берёмся за ось X гизмо (в 40 точках правее центра) и тянем на 2 мировые единицы.
    let origin = editor.world_to_screen(before);
    let two_units = editor.world_to_screen(before + Vec2::new(2.0, 0.0)).x - origin.x;
    let grab = origin + egui::vec2(40.0, 0.0);
    let viewport = editor.viewport_rect();
    hands.drag(&mut editor, grab, grab + egui::vec2(two_units, 25.0));
    let moved_rect = editor.viewport_rect();
    assert!(
        (moved_rect.min - viewport.min).length() < 0.5 && (moved_rect.max - viewport.max).length() < 0.5,
        "panels keep their size while the Inspector shows changing numbers: {viewport:?} -> {moved_rect:?}"
    );

    let moved = position(&editor, "Player");
    assert!((moved.x - (before.x + 2.0)).abs() < 0.02, "moved 2 units along X: {moved}");
    assert!((moved.y - before.y).abs() < 1e-4, "the X handle ignores vertical motion");
    assert!(editor.is_dirty());
    assert!(editor.can_undo());

    // Весь жест — один шаг Undo.
    editor.apply(Action::Undo);
    assert!((position(&editor, "Player") - before).length() < 1e-4);
    assert!(!editor.can_undo());
    assert_eq!(
        editor.selection(),
        Some(find(&editor, "Player")),
        "the selection survives the scene being rebuilt"
    );

    editor.apply(Action::Redo);
    assert!((position(&editor, "Player").x - (before.x + 2.0)).abs() < 0.02);
}

#[test]
fn the_inspector_edits_fields_through_reflection() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = find(&editor, "Player");
    editor.apply(Action::Select(Some(player)));
    hands.frame(&mut editor, vec![]);
    hands.frame(&mut editor, vec![]);

    // Флажок: поле `bool`.
    let flip = editor
        .inspector_field_rect("SpriteRenderer", "flip_x")
        .expect("the field is shown in the Inspector");
    hands.click(&mut editor, flip.left_center() + egui::vec2(9.0, 0.0));
    let sprite = |editor: &Editor| {
        let id = find(editor, "Player");
        editor.app.world.scene.get_component::<SpriteRenderer>(id).unwrap().clone()
    };
    assert!(sprite(&editor).flip_x, "the checkbox wrote the field");
    assert!(editor.is_dirty());

    // Число: тянем значение мышью, как ползунок. Rigidbody 2D — ниже края окна,
    // прокручиваем инспектор колесом.
    let inspector = editor.inspector_field_rect("SpriteRenderer", "flip_x").unwrap().center();
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(inspector)]);
    hands.frame(
        &mut editor,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
        }],
    );
    for _ in 0..30 {
        hands.frame(&mut editor, vec![]);
    }
    let gravity = editor
        .inspector_field_rect("Rigidbody2D", "gravity_scale")
        .expect("the field is shown in the Inspector");
    let grab = gravity.left_center() + egui::vec2(14.0, 0.0);
    hands.drag(&mut editor, grab, grab + egui::vec2(60.0, 0.0));
    let body = |editor: &Editor| {
        let id = find(editor, "Player");
        editor.app.world.scene.get_component::<Rigidbody2D>(id).unwrap().gravity_scale
    };
    assert!(body(&editor) > 3.5, "dragging raised the value from 2.5: {}", body(&editor));

    // Два жеста — два шага Undo.
    editor.apply(Action::Undo);
    assert_eq!(body(&editor), 2.5);
    assert!(sprite(&editor).flip_x);
    editor.apply(Action::Undo);
    assert!(!sprite(&editor).flip_x);
    assert!(!editor.can_undo());
}

#[test]
fn dragging_a_hierarchy_row_onto_another_reparents_it() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let player = find(&editor, "Player");
    let platforms = find(&editor, "Platforms");
    let world_before = position(&editor, "Player");

    hands.drag(&mut editor, hands.row_center(player), hands.row_center(platforms));

    let scene = &editor.app.world.scene;
    assert_eq!(scene.get(player).unwrap().parent(), Some(platforms));
    assert!((position(&editor, "Player") - world_before).length() < 1e-4, "world pose is kept");
    assert!(editor.is_dirty());

    editor.apply(Action::Undo);
    let player = find(&editor, "Player");
    assert_eq!(editor.app.world.scene.get(player).unwrap().parent(), None);
}

#[test]
fn dragging_a_prefab_from_the_project_window_drops_an_instance_into_the_scene() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let count = editor.app.world.scene.len();
    // Окно Project показывает содержимое одного каталога: открываем каталог префаба.
    editor.select_asset("prefabs/coin.prefab");
    hands.frame(&mut editor, vec![]);
    hands.frame(&mut editor, vec![]);

    let row = editor
        .project_row_id("prefabs/coin.prefab")
        .and_then(|id| hands.ctx.read_response(id))
        .expect("the prefab has a row in the Project window")
        .rect;
    let target = Vec2::new(-9.0, 3.0);
    let drop_at = editor.world_to_screen(target);
    assert!(editor.viewport_rect().contains(drop_at));
    hands.drag(&mut editor, row.center(), drop_at);

    assert_eq!(editor.app.world.scene.len(), count + 2, "the coin and its Gem child");
    let coin = editor.selection().expect("the new instance is selected");
    let object = editor.app.world.scene.get(coin).unwrap();
    assert_eq!(object.prefab(), Some("prefabs/coin.prefab"));
    assert_eq!(object.name, "Coin");
    assert!(object.has::<scripts::Coin>());
    let dropped = object.transform.position.truncate();
    assert!((dropped - target).length() < 0.05, "dropped under the cursor: {dropped}");
}

#[test]
fn keyboard_shortcuts_drive_the_editor() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let count = editor.app.world.scene.len();
    editor.apply(Action::Select(Some(find(&editor, "Player"))));

    hands.press(&mut editor, egui::Key::E, egui::Modifiers::NONE);
    assert_eq!(editor.tool(), engine::editor::Tool::Rotate);

    hands.press(&mut editor, egui::Key::D, egui::Modifiers::COMMAND);
    assert_eq!(editor.app.world.scene.len(), count + 1, "Ctrl+D duplicates");
    let copy = editor.selection().expect("the copy is selected");
    assert_eq!(editor.app.world.scene.get(copy).unwrap().name, "Player (1)");

    hands.press(&mut editor, egui::Key::Delete, egui::Modifiers::NONE);
    assert_eq!(editor.app.world.scene.len(), count);
    assert_eq!(editor.selection(), None);

    hands.press(&mut editor, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(editor.app.world.scene.len(), count + 1, "Ctrl+Z brings the copy back");

    hands.press(&mut editor, egui::Key::P, egui::Modifiers::COMMAND);
    assert_eq!(editor.mode(), Mode::Playing);
    hands.press(&mut editor, egui::Key::P, egui::Modifiers::COMMAND);
    assert_eq!(editor.mode(), Mode::Edit);
}

#[test]
fn play_runs_the_game_with_keyboard_input_and_stop_restores_the_scene() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let start = position(&editor, "Player");
    let objects = editor.app.world.scene.len();
    editor.apply(Action::Select(Some(find(&editor, "Player"))));

    editor.apply(Action::Play);
    assert_eq!(editor.mode(), Mode::Playing);
    assert_eq!(editor.view_tab(), ViewTab::Game);

    // Держим «вправо» две секунды: ввод идёт через egui, как в настоящем окне.
    hands.key(&mut editor, egui::Key::D, egui::Modifiers::NONE, true);
    for _ in 0..120 {
        editor.advance(DT);
        hands.frame(&mut editor, vec![]);
    }
    hands.key(&mut editor, egui::Key::D, egui::Modifiers::NONE, false);

    let played = position(&editor, "Player");
    assert!(played.x > start.x + 5.0, "the player ran right: {start} → {played}");
    assert!(played.y < start.y - 0.5, "and fell onto the ground");
    assert!(!editor.is_dirty(), "playing does not dirty the scene");
    assert!(
        editor.app.world.scene.len() < objects,
        "a coin on the way was collected and destroyed"
    );

    editor.apply(Action::Stop);
    assert_eq!(editor.mode(), Mode::Edit);
    assert_eq!(editor.view_tab(), ViewTab::Scene);
    assert!((position(&editor, "Player") - start).length() < 1e-4, "the player is back");
    assert_eq!(editor.app.world.scene.len(), objects, "the coin is back");
    assert_eq!(editor.selection(), Some(find(&editor, "Player")));

    // Второй запуск начинается с чистого листа: физика не помнит прошлый.
    editor.apply(Action::Play);
    for _ in 0..60 {
        editor.advance(DT);
    }
    let second = position(&editor, "Player");
    assert!((second.x - start.x).abs() < 1e-3, "no leftover velocity: {second}");
    assert!(second.y < start.y - 0.5);
    editor.apply(Action::Stop);
}

#[test]
fn pause_freezes_the_game_and_step_advances_one_frame() {
    let mut editor = editor();
    editor.apply(Action::Play);
    for _ in 0..10 {
        editor.advance(DT);
    }
    let frame = editor.app.world.time.frame;
    assert_eq!(frame, 10);

    editor.apply(Action::Pause);
    assert_eq!(editor.mode(), Mode::Paused);
    for _ in 0..10 {
        editor.advance(DT);
    }
    assert_eq!(editor.app.world.time.frame, frame, "paused: no frames");

    let before = position(&editor, "Player");
    editor.apply(Action::Step);
    assert_eq!(editor.app.world.time.frame, frame + 1);
    assert_eq!(editor.mode(), Mode::Paused);
    assert!(position(&editor, "Player").y < before.y, "one step of falling");

    editor.apply(Action::Pause);
    assert_eq!(editor.mode(), Mode::Playing);
    editor.apply(Action::Stop);
    assert_eq!(editor.mode(), Mode::Edit);
}

#[test]
fn a_game_that_asks_to_quit_stops_play_mode_instead_of_closing_the_editor() {
    let mut editor = editor();
    editor.apply(Action::Play);
    editor.advance(DT);
    editor.app.world.request_quit();
    editor.advance(DT);
    assert_eq!(editor.mode(), Mode::Edit);
    assert!(!editor.should_quit());
    assert!(!editor.app.world.quit_requested(), "the request is cleared on Stop");
}

#[test]
fn objects_are_created_duplicated_reparented_and_deleted() {
    let mut editor = editor();
    let count = editor.app.world.scene.len();
    let platforms = find(&editor, "Platforms");

    editor.apply(Action::CreateSprite { parent: Some(platforms) });
    let sprite = editor.selection().expect("the new object is selected");
    {
        let scene = &editor.app.world.scene;
        let object = scene.get(sprite).unwrap();
        assert_eq!(object.name, "Sprite");
        assert_eq!(object.parent(), Some(platforms));
        assert!(object.has::<SpriteRenderer>());
    }

    // Смена родителя сохраняет мировую позу.
    let ruby = find(&editor, "Ruby");
    let world_before = editor.app.world.scene.world_matrix(ruby);
    editor.apply(Action::Reparent {
        child: ruby,
        parent: Some(find(&editor, "Platform C")),
    });
    let world_after = editor.app.world.scene.world_matrix(ruby);
    assert!(world_before.abs_diff_eq(world_after, 1e-4));

    // Цикл в иерархии — ошибка в консоли, а не паника и не испорченная сцена.
    let coins = find(&editor, "Coins");
    editor.apply(Action::Reparent {
        child: coins,
        parent: Some(find(&editor, "Coin 1")),
    });
    assert_eq!(editor.app.world.scene.get(coins).unwrap().parent(), None);

    editor.apply(Action::Delete(platforms));
    assert!(!editor.app.world.scene.contains(sprite), "children go with the parent");
    assert_eq!(editor.selection(), None);
    assert!(editor.app.world.scene.len() < count);
}

#[test]
fn components_are_added_and_removed_by_name() {
    let mut editor = editor();
    let ground = find(&editor, "Ground");
    let kinds = |editor: &Editor| -> Vec<&'static str> {
        editor
            .app
            .world
            .scene
            .get(ground)
            .unwrap()
            .components()
            .map(|c| c.type_name())
            .collect()
    };
    assert_eq!(kinds(&editor), ["SpriteRenderer", "Collider2D"]);

    editor.apply(Action::AddComponent {
        object: ground,
        kind: "Rigidbody2D".to_string(),
    });
    assert_eq!(kinds(&editor), ["SpriteRenderer", "Collider2D", "Rigidbody2D"]);

    // Опечатка в имени: сцена не меняется, шаг Undo не появляется.
    let undo_before = editor.can_undo();
    editor.apply(Action::AddComponent {
        object: ground,
        kind: "Rigidbdy2D".to_string(),
    });
    assert_eq!(kinds(&editor).len(), 3);
    assert_eq!(editor.can_undo(), undo_before);

    editor.apply(Action::RemoveComponent { object: ground, index: 1 });
    assert_eq!(kinds(&editor), ["SpriteRenderer", "Rigidbody2D"]);
}

#[test]
fn saving_keeps_prefab_links_and_what_was_changed_on_the_instance() {
    let assets = temp_assets();
    let mut editor = editor_with_assets(&assets);
    let coin = find(&editor, "Coin 1");

    // У экземпляра префаба: добавили компонент, убрали компонент, поменяли поле.
    editor.apply(Action::AddComponent {
        object: coin,
        kind: "Rigidbody2D".to_string(),
    });
    editor.apply(Action::RemoveComponent { object: coin, index: 0 });
    editor
        .app
        .world
        .scene
        .get_component_mut::<scripts::Coin>(coin)
        .unwrap()
        .value = 7;

    editor.apply(Action::SaveAs("scenes/copy".to_string()));
    assert!(!editor.is_dirty());
    assert_eq!(editor.scene_path(), Some("scenes/copy.scene"));

    let text = std::fs::read_to_string(assets.join("scenes/copy.scene")).unwrap();
    assert!(text.contains("prefab: Some(\"prefabs/coin.prefab\")"), "{text}");
    assert!(text.contains("removed: ["), "{text}");
    assert!(!text.contains("field: \"flip_x\""), "defaults omitted in the prefab file are not overrides: {text}");
    assert!(text.contains("\"Collider2D\""), "{text}");
    assert!(text.contains("kind: \"Rigidbody2D\""), "{text}");

    // Открываем заново: всё на месте.
    editor.apply(Action::OpenScene("scenes/copy.scene".to_string()));
    let coin = find(&editor, "Coin 1");
    let object = editor.app.world.scene.get(coin).unwrap();
    assert_eq!(object.prefab(), Some("prefabs/coin.prefab"));
    let kinds: Vec<_> = object.components().map(|c| c.type_name()).collect();
    assert_eq!(kinds, ["Coin", "Rigidbody2D"]);
    assert_eq!(object.get::<scripts::Coin>().unwrap().value, 7);
    // Нетронутый экземпляр остался обычной ссылкой на префаб.
    let other = editor.app.world.scene.get(find(&editor, "Coin 2")).unwrap();
    assert_eq!(other.components().count(), 2);

    let _ = std::fs::remove_dir_all(assets);
}

#[test]
fn an_object_can_be_saved_as_a_prefab_and_instantiated() {
    let assets = temp_assets();
    let mut editor = editor_with_assets(&assets);
    let platform = find(&editor, "Platform A");

    editor.apply(Action::SavePrefab(platform));
    assert!(assets.join("prefabs/platform_a.prefab").is_file());
    assert!(assets.join("prefabs/platform_a.prefab.meta").is_file(), "it got a GUID");
    assert_eq!(
        editor.app.world.scene.get(platform).unwrap().prefab(),
        Some("prefabs/platform_a.prefab")
    );

    let count = editor.app.world.scene.len();
    editor.apply(Action::Instantiate {
        reference: "prefabs/platform_a.prefab".to_string(),
        position: Some(Vec3::new(12.0, 3.0, 0.0)),
    });
    assert_eq!(editor.app.world.scene.len(), count + 1);
    let instance = editor.selection().unwrap();
    let object = editor.app.world.scene.get(instance).unwrap();
    assert!(object.has::<Collider2D>());
    assert_eq!(object.transform.position, Vec3::new(12.0, 3.0, 0.0));

    let _ = std::fs::remove_dir_all(assets);
}

#[test]
fn unsaved_changes_are_not_thrown_away_silently() {
    let mut editor = editor();
    editor.apply(Action::CreateEmpty { parent: None });
    assert!(editor.is_dirty());
    let count = editor.app.world.scene.len();

    editor.apply(Action::NewScene);
    assert!(editor.has_dialog(), "the editor asks first");
    assert_eq!(editor.app.world.scene.len(), count, "nothing was discarded");

    editor.apply(Action::Quit);
    assert!(!editor.should_quit());

    // Сохранять во время игры нельзя: в файл попало бы состояние посреди игры.
    let mut editor = self::editor();
    editor.apply(Action::Play);
    editor.apply(Action::Save);
    assert_eq!(editor.mode(), Mode::Playing);
    let last = engine::core::logging::records()
        .into_iter()
        .rev()
        .find(|record| record.level == engine::core::logging::Level::Error)
        .expect("the refusal is reported in the console");
    assert!(last.message.contains("press Stop before saving"), "{}", last.message);
}

// ----------------------------------------------------------------------
// Вкладка Code и сборка
// ----------------------------------------------------------------------

#[test]
fn the_code_tab_opens_edits_and_saves_source_files() {
    let assets = temp_assets();
    let source = assets.join("notes.rs");
    std::fs::write(&source, "fn main() {}\n").unwrap();
    let mut editor = {
        let mut app = engine::app();
        scripts::register(&mut app);
        engine::mount_assets(&mut app, &assets).unwrap();
        Editor::new(app, EditorConfig::default().code_dir(&assets)).unwrap()
    };
    let mut hands = Hands::new(&mut editor);

    editor.apply(Action::OpenCode(source.clone()));
    assert_eq!(editor.view_tab(), ViewTab::Code);
    let (path, text, dirty) = editor.code_document().expect("the file is open");
    assert_eq!((path, text, dirty), (source.as_path(), "fn main() {}\n", false));

    editor.set_code_text("fn main() { println!(\"hi\"); }\n");
    assert!(editor.code_document().unwrap().2, "edited text is marked unsaved");
    hands.frame(&mut editor, vec![]); // вкладка рисуется с подсветкой без паники

    // Ctrl+S во вкладке Code сохраняет файл, а не сцену.
    hands.press(&mut editor, egui::Key::S, egui::Modifiers::COMMAND);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "fn main() { println!(\"hi\"); }\n");
    assert!(!editor.code_document().unwrap().2);
    assert!(!editor.is_dirty(), "the scene was not touched");

    // Несуществующий файл — ошибка в консоли, вкладка остаётся как была.
    editor.apply(Action::OpenCode(assets.join("missing.cpp")));
    assert_eq!(editor.code_document().unwrap().0, source.as_path());
    let _ = std::fs::remove_dir_all(assets);
}

#[test]
fn build_runs_the_configured_commands_and_reports_the_result() {
    use engine::editor::{BuildCommand, BuildStatus};
    let wait = |editor: &Editor| {
        for _ in 0..600 {
            if editor.build_status() != BuildStatus::Running {
                return editor.build_status();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("the build did not finish in 30 s");
    };

    let config = EditorConfig::default().build_command(BuildCommand::new("Toolchain", "cargo").args(["--version"]));
    let mut editor = Editor::new(engine::app(), config).unwrap();
    assert_eq!(editor.build_status(), BuildStatus::Idle);
    editor.apply(Action::Build);
    assert!(matches!(wait(&editor), BuildStatus::Succeeded { .. }));
    let output = engine::core::logging::records();
    assert!(
        output.iter().any(|r| r.target == "build" && r.message.starts_with("cargo ")),
        "the output of the command is forwarded to the Console"
    );

    // Сборка останавливается на первой неудачной команде и называет её.
    let config = EditorConfig::default()
        .build_command(BuildCommand::new("Broken step", "cargo").args(["no-such-subcommand-piska"]))
        .build_command(BuildCommand::new("Never runs", "cargo").args(["--version"]));
    let mut editor = Editor::new(engine::app(), config).unwrap();
    editor.apply(Action::Build);
    assert_eq!(wait(&editor), BuildStatus::Failed { command: "Broken step".to_string() });

    // Программа не найдена — понятное сообщение, а не паника.
    let config = EditorConfig::default().build_command(BuildCommand::new("Ghost", "piska-no-such-tool"));
    let mut editor = Editor::new(engine::app(), config).unwrap();
    editor.apply(Action::Build);
    assert_eq!(wait(&editor), BuildStatus::Failed { command: "Ghost".to_string() });
    assert!(engine::core::logging::records()
        .iter()
        .any(|r| r.message.contains("cannot start `piska-no-such-tool`")));
}

/// Сквозной сценарий «логика на C++»: поправили исходник во вкладке Code, нажали Build —
/// плагин пересобрался и перезагрузился, не закрывая редактор. Нужны CMake, компилятор
/// C++ и собранный плагин:
/// `cargo test -p engine --test editor -- --ignored editing_cpp`.
/// Каталог сборки CMake берётся из `ENGINE_CPP_BUILD_DIR` (по умолчанию — `build` рядом с плагином).
#[test]
#[ignore = "needs cmake, a C++ compiler and plugins/cpp-gameplay built"]
fn editing_cpp_in_the_code_tab_and_building_reloads_the_plugin() {
    use engine::editor::{BuildCommand, BuildStatus};
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let plugin_dir = format!("{root}/plugins/cpp-gameplay");
    let source = PathBuf::from(format!("{plugin_dir}/gameplay.cpp"));
    let library = PathBuf::from(format!("{plugin_dir}/bin")).join(engine::cpp::plugin_file_name("piska_gameplay"));
    let build_dir = std::env::var("ENGINE_CPP_BUILD_DIR").unwrap_or_else(|_| format!("{plugin_dir}/build"));
    assert!(library.is_file(), "build the plugin first: {}", library.display());

    // Что бы ни случилось, исходник возвращается к прежнему виду.
    struct Restore(PathBuf, String);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, &self.1);
        }
    }
    let original = std::fs::read_to_string(&source).unwrap();
    let _restore = Restore(source.clone(), original.clone());

    let mut app = engine::app();
    engine::add_cpp_plugins(&mut app, [&library]).unwrap();
    let spinner = app.world.registry.create("Spinner").unwrap();
    let cube = app.spawn("Cube").id();
    app.world.scene.get_mut(cube).unwrap().add_boxed(spinner);
    let config = EditorConfig::default().code_dir(&plugin_dir).build_command(
        BuildCommand::new("C++ plugin", "cmake").args(["--build", build_dir.as_str(), "--config", "Release"]),
    );
    let mut editor = Editor::new(app, config).unwrap();

    let yaw_after_a_second = |editor: &mut Editor| {
        editor.apply(Action::Play);
        for _ in 0..60 {
            editor.advance(DT);
        }
        let id = find(editor, "Cube");
        let yaw = editor.app.world.scene.get(id).unwrap().transform.euler_deg().y;
        editor.apply(Action::Stop);
        yaw
    };
    let build = |editor: &mut Editor| {
        editor.apply(Action::Build);
        for _ in 0..3600 {
            if editor.build_status() != BuildStatus::Running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(matches!(editor.build_status(), BuildStatus::Succeeded { .. }), "{:?}", editor.build_status());
        // Редактор простаивает — подсистема плагинов замечает новый файл.
        for _ in 0..90 {
            editor.advance(DT);
        }
    };

    let before = yaw_after_a_second(&mut editor);
    assert!((before - 90.0).abs() < 1.0, "90 deg/s before the edit: {before}");

    // Поле, заданное в инспекторе, должно пережить перезагрузку плагина.
    let id = find(&editor, "Cube");
    let object = editor.app.world.scene.get_mut(id).unwrap();
    object.components_mut().next().unwrap().set_field("speed_deg", &Value::Float(30.0)).unwrap();

    editor.apply(Action::OpenCode(source.clone()));
    let edited = original.replace("rotation.y + speed_deg * dt", "rotation.y + 4.0f * speed_deg * dt");
    assert_ne!(edited, original, "the line to edit is still in gameplay.cpp");
    editor.set_code_text(&edited);
    build(&mut editor); // сохраняет файл, собирает, плагин перезагружается

    let id = find(&editor, "Cube");
    let speed = editor.app.world.scene.get(id).unwrap().components().next().unwrap().get_field("speed_deg");
    assert_eq!(speed, Some(Value::Float(30.0)), "the field value survived the reload");
    let after = yaw_after_a_second(&mut editor);
    assert!((after - 120.0).abs() < 1.5, "new code runs: 4 x 30 deg/s = 120 deg/s, got {after}");

    // Возвращаем исходник и собираем снова, чтобы библиотека соответствовала коду.
    editor.set_code_text(&original);
    build(&mut editor);
    let restored = yaw_after_a_second(&mut editor);
    assert!((restored - 30.0).abs() < 1.0, "back to 30 deg/s: {restored}");
}

// ----------------------------------------------------------------------
// 3D-вид сцены
// ----------------------------------------------------------------------

fn editor_3d() -> Editor {
    let mut app = engine::app();
    let mut camera = app.spawn("Camera").at3(0.0, 4.0, 10.0).with(Camera::perspective(50.0)).id();
    app.world.scene.get_mut(camera).unwrap().transform.look_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y);
    camera = app.spawn("Cube").at3(0.0, 0.5, 0.0).with(MeshRenderer::cube()).id();
    let _ = camera;
    app.spawn("Ball").at3(3.0, 0.5, 0.0).with(MeshRenderer::sphere());
    app.spawn("Sun").with(Light::default());
    Editor::new(app, EditorConfig::default()).unwrap()
}

#[test]
fn a_scene_with_a_perspective_camera_opens_in_the_3d_view() {
    let mut editor = editor_3d();
    assert!(editor.is_scene_3d());
    let mut hands = Hands::new(&mut editor);

    // Все объекты видны: камера редактора встала туда же, где камера игры.
    for name in ["Cube", "Ball"] {
        let id = find(&editor, name);
        let center = editor.app.world.scene.world_position(id);
        let on_screen = editor.world_to_screen_3d(center).expect("in front of the camera");
        assert!(editor.viewport_rect().contains(on_screen), "{name} is in view");
        // Щелчок по объекту выбирает его лучом.
        hands.click(&mut editor, on_screen);
        assert_eq!(editor.selection(), Some(id), "clicked {name}");
    }
    // Щелчок по небу снимает выделение.
    let sky = editor.viewport_rect().center_top() + egui::vec2(0.0, 12.0);
    hands.click(&mut editor, sky);
    assert_eq!(editor.selection(), None);

    // 2D-сцена платформера по-прежнему открывается в плоском виде.
    assert!(!self::editor().is_scene_3d());
}

#[test]
fn the_3d_move_gizmo_drags_along_a_world_axis() {
    let mut editor = editor_3d();
    let mut hands = Hands::new(&mut editor);
    let cube = find(&editor, "Cube");
    let start = editor.app.world.scene.world_position(cube);
    editor.apply(Action::Select(Some(cube)));
    hands.frame(&mut editor, vec![]);

    // Берёмся за ось X в метре от центра и ведём курсор к точке в трёх метрах.
    let grab = editor.world_to_screen_3d(start + Vec3::X).unwrap();
    let release = editor.world_to_screen_3d(start + Vec3::X * 3.0).unwrap();
    hands.drag(&mut editor, grab, release);

    let moved = editor.app.world.scene.world_position(cube);
    assert!((moved.x - (start.x + 2.0)).abs() < 0.08, "two units along X: {moved}");
    assert!((moved.y - start.y).abs() < 1e-4 && (moved.z - start.z).abs() < 1e-4, "only X changed: {moved}");

    editor.apply(Action::Undo);
    let cube = find(&editor, "Cube");
    assert!((editor.app.world.scene.world_position(cube) - start).length() < 1e-4);

    // Новые 3D-объекты появляются на земле под центром вида.
    editor.apply(Action::CreateMesh(MeshShape::Cylinder));
    let created = editor.selection().unwrap();
    let object = editor.app.world.scene.get(created).unwrap();
    assert_eq!(object.name, "Cylinder");
    assert_eq!(object.get::<MeshRenderer>().unwrap().shape, MeshShape::Cylinder);
    assert_eq!(object.transform.position.y, 0.5, "stands on the ground");
}

// ----------------------------------------------------------------------
// Новый скрипт из редактора
// ----------------------------------------------------------------------

component! {
    /// Стоит на месте настоящего C++-класса в тесте без компилятора.
    pub struct EnemyPatrol {
        pub speed: f32 = 1.0,
    }
}
impl Component for EnemyPatrol {}

#[test]
fn a_new_script_is_created_from_a_template_and_attached_once_it_is_built() {
    let project = temp_assets();
    let scripts = project.join("scripts");
    let mut editor = {
        let mut app = engine::app();
        engine::mount_assets(&mut app, &project).unwrap();
        app.spawn("Guard");
        Editor::new(app, EditorConfig::default().code_dir(&project).cpp_scripts(&scripts)).unwrap()
    };
    let guard = find(&editor, "Guard");

    // Имя должно быть именем класса C++: остальное отклоняется, файлов не появляется.
    for bad in ["enemy", "My Script", "9Lives", ""] {
        editor.apply(Action::NewScript { name: bad.to_string(), attach_to: Some(guard) });
    }
    assert!(!scripts.exists() || std::fs::read_dir(&scripts).unwrap().next().is_none());
    assert!(editor.pending_scripts().is_empty());

    editor.apply(Action::NewScript { name: "EnemyPatrol".to_string(), attach_to: Some(guard) });
    let file = scripts.join("EnemyPatrol.cpp");
    let source = std::fs::read_to_string(&file).expect("the script file is created");
    assert!(source.contains("struct EnemyPatrol : engine::Component"), "{source}");
    assert!(source.contains("ENGINE_COMPONENT(EnemyPatrol);"), "{source}");
    // Скрипт сразу открыт во вкладке Code и ждёт сборки, чтобы повеситься на объект.
    assert_eq!(editor.view_tab(), ViewTab::Code);
    assert_eq!(editor.code_document().unwrap().0, file.as_path());
    assert_eq!(editor.pending_scripts(), [("EnemyPatrol".to_string(), guard)]);
    assert_eq!(editor.app.world.scene.get(guard).unwrap().components().count(), 0);

    // Второй скрипт с тем же именем создать нельзя.
    editor.apply(Action::NewScript { name: "EnemyPatrol".to_string(), attach_to: None });
    assert_eq!(editor.pending_scripts().len(), 1);

    // Плагин «собрался»: тип появился в реестре — скрипт сам оказывается на объекте.
    editor.advance(DT);
    assert_eq!(editor.pending_scripts().len(), 1, "still waiting for the build");
    editor.app.world.registry.register::<EnemyPatrol>();
    editor.advance(DT);
    assert!(editor.pending_scripts().is_empty());
    let kinds: Vec<_> = editor.app.world.scene.get(guard).unwrap().components().map(|c| c.type_name()).collect();
    assert_eq!(kinds, ["EnemyPatrol"]);
    assert!(editor.can_undo(), "attaching is an undoable edit");

    // Без каталога скриптов редактор объясняет, как его задать.
    let mut plain = Editor::new(engine::app(), EditorConfig::default()).unwrap();
    plain.apply(Action::NewScript { name: "Anything".to_string(), attach_to: None });
    assert!(engine::core::logging::records().iter().any(|r| r.message.contains("cpp_scripts")));
    let _ = std::fs::remove_dir_all(project);
}

/// Сквозной сценарий «Add Component → New C++ Script»: файл из шаблона, сборка CMake,
/// перезагрузка плагина, скрипт на объекте и работает. Нужны CMake и компилятор C++.
#[test]
#[ignore = "needs cmake, a C++ compiler and plugins/cpp-gameplay built"]
fn a_new_cpp_script_is_built_loaded_and_attached() {
    use engine::editor::{BuildCommand, BuildStatus};
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let plugin_dir = format!("{root}/plugins/cpp-gameplay");
    let library = PathBuf::from(format!("{plugin_dir}/bin")).join(engine::cpp::plugin_file_name("piska_gameplay"));
    let build_dir = std::env::var("ENGINE_CPP_BUILD_DIR").unwrap_or_else(|_| format!("{plugin_dir}/build"));
    let script = PathBuf::from(format!("{plugin_dir}/scripts/ZzTestScript.cpp"));
    assert!(library.is_file(), "build the plugin first: {}", library.display());

    // Временный скрипт удаляется в любом случае.
    struct Remove(PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _ = std::fs::remove_file(&script);
    let cleanup = Remove(script.clone());

    let mut app = engine::app();
    engine::add_cpp_plugins(&mut app, [&library]).unwrap();
    app.spawn("Cube");
    let config = EditorConfig::default()
        .code_dir(&plugin_dir)
        .cpp_scripts(format!("{plugin_dir}/scripts"))
        .build_command(BuildCommand::new("C++ plugin", "cmake").args(["--build", build_dir.as_str(), "--config", "Release"]));
    let mut editor = Editor::new(app, config).unwrap();
    let wait = |editor: &mut Editor| {
        for _ in 0..3600 {
            if editor.build_status() != BuildStatus::Running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(matches!(editor.build_status(), BuildStatus::Succeeded { .. }), "{:?}", editor.build_status());
        for _ in 0..90 {
            editor.advance(DT);
        }
    };

    let cube = find(&editor, "Cube");
    editor.apply(Action::NewScript { name: "ZzTestScript".to_string(), attach_to: Some(cube) });
    wait(&mut editor); // сборка запущена самим действием

    assert!(editor.pending_scripts().is_empty(), "the script was attached after the reload");
    let cube = find(&editor, "Cube");
    let kinds: Vec<_> = editor.app.world.scene.get(cube).unwrap().components().map(|c| c.type_name()).collect();
    assert_eq!(kinds, ["ZzTestScript"]);

    // Скрипт из шаблона вращает объект: 30°/с при speed = 1.
    editor.apply(Action::Play);
    for _ in 0..60 {
        editor.advance(DT);
    }
    let cube = find(&editor, "Cube");
    let yaw = editor.app.world.scene.get(cube).unwrap().transform.euler_deg().y;
    assert!((yaw - 30.0).abs() < 1.0, "the template script runs: {yaw}");
    editor.apply(Action::Stop);

    // Убираем скрипт и пересобираем, чтобы библиотека снова соответствовала исходникам.
    drop(cleanup);
    editor.apply(Action::Build);
    wait(&mut editor);
}

// ----------------------------------------------------------------------
// Меню GameObject, иерархия, инспектор, окно Project
// ----------------------------------------------------------------------

#[test]
fn game_object_presets_create_ui_audio_and_effects_objects() {
    let mut editor = editor();
    let count = editor.app.world.scene.len();
    let kinds = |editor: &Editor, id: GameObjectId| -> Vec<&'static str> {
        editor.app.world.scene.get(id).unwrap().components().map(|c| c.type_name()).collect()
    };

    // Кнопка интерфейса: холста ещё нет — он создаётся, кнопка ложится под него с подписью.
    editor.apply(Action::Create { preset: "UI/Button".to_string(), parent: None });
    let button = editor.selection().expect("the button is selected");
    assert_eq!(kinds(&editor, button), ["RectTransform", "Image", "Button"]);
    let scene = &editor.app.world.scene;
    let canvas = scene.get(button).unwrap().parent().expect("the button lives under a canvas");
    assert_eq!(scene.get(canvas).unwrap().name, "Canvas");
    let label = scene.get(button).unwrap().children()[0];
    let text = scene.get(label).unwrap().components().find(|c| c.type_name() == "Text").unwrap();
    assert_eq!(text.get_field("text"), Some(Value::Str("Button".to_string())));
    assert_eq!(editor.app.world.scene.len(), count + 3, "canvas, button and its label");

    // Второй элемент интерфейса находит тот же холст.
    editor.apply(Action::Create { preset: "UI/Slider".to_string(), parent: None });
    let slider = editor.selection().unwrap();
    assert_eq!(editor.app.world.scene.get(slider).unwrap().parent(), Some(canvas));

    editor.apply(Action::Create { preset: "Audio/Audio Source".to_string(), parent: None });
    assert_eq!(kinds(&editor, editor.selection().unwrap()), ["AudioSource"]);
    editor.apply(Action::Create { preset: "Effects/Particle System".to_string(), parent: None });
    assert_eq!(kinds(&editor, editor.selection().unwrap()), ["ParticleSystem"]);

    // Всё создание — шаги Undo.
    for _ in 0..4 {
        editor.apply(Action::Undo);
    }
    assert_eq!(editor.app.world.scene.len(), count);

    // Незнакомая заготовка и заготовка без зарегистрированных компонентов — ошибка в
    // консоли, сцена не меняется.
    editor.apply(Action::Create { preset: "UI/Hologram".to_string(), parent: None });
    let mut bare = Editor::new(App::new(), EditorConfig::default()).unwrap();
    bare.apply(Action::Create { preset: "UI/Button".to_string(), parent: None });
    assert_eq!(bare.app.world.scene.len(), 0);
    assert!(engine::core::logging::records()
        .iter()
        .any(|r| r.message.contains("cannot create `Button`") && r.message.contains("not registered")));
}

#[test]
fn objects_can_be_toggled_reordered_and_components_reset() {
    let mut editor = editor();
    let player = find(&editor, "Player");

    editor.apply(Action::SetActive { id: player, active: false });
    assert!(!editor.app.world.scene.get(player).unwrap().active);
    assert!(editor.is_dirty());

    // На место среди детей другого родителя, с сохранением мировой позы.
    let platforms = find(&editor, "Platforms");
    let world = position(&editor, "Player");
    editor.apply(Action::ReparentAt { child: player, parent: Some(platforms), index: 0 });
    let scene = &editor.app.world.scene;
    assert_eq!(scene.get(platforms).unwrap().children()[0], player);
    assert!((position(&editor, "Player") - world).length() < 1e-4);

    // Сброс компонента к значениям по умолчанию.
    let gravity = |editor: &Editor| {
        let id = find(editor, "Player");
        editor.app.world.scene.get_component::<Rigidbody2D>(id).unwrap().gravity_scale
    };
    assert_eq!(gravity(&editor), 2.5);
    let index = editor
        .app
        .world
        .scene
        .get(player)
        .unwrap()
        .components()
        .position(|c| c.type_name() == "Rigidbody2D")
        .unwrap();
    editor.apply(Action::ResetComponent { object: player, index });
    assert_eq!(gravity(&editor), Rigidbody2D::default().gravity_scale);

    for _ in 0..3 {
        editor.apply(Action::Undo);
    }
    let player = find(&editor, "Player");
    assert!(editor.app.world.scene.get(player).unwrap().active);
    assert_eq!(editor.app.world.scene.get(player).unwrap().parent(), None);
    assert_eq!(gravity(&editor), 2.5);
}

#[test]
fn dropping_a_row_on_the_upper_edge_of_another_puts_it_before_that_row() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    let roots = editor.app.world.scene.roots().to_vec();
    assert!(roots.len() >= 3);
    let (first, last) = (roots[0], roots[2]);

    let target = hands.ctx.read_response(Editor::hierarchy_row_id(first)).unwrap().rect;
    let upper_edge = target.center_top() + egui::vec2(0.0, 3.0);
    hands.drag(&mut editor, hands.row_center(last), upper_edge);

    let roots = editor.app.world.scene.roots();
    assert_eq!(roots[0], last, "dropped above the first row");
    assert_eq!(roots[1], first);
    assert_eq!(editor.app.world.scene.get(last).unwrap().parent(), None, "not made a child");
}

#[test]
fn the_game_gets_scroll_text_and_the_size_of_the_game_view_in_play_mode() {
    let mut editor = editor();
    let mut hands = Hands::new(&mut editor);
    editor.apply(Action::Play);
    hands.frame(&mut editor, vec![]);
    let view = editor.viewport_rect();
    let inside = view.center();
    hands.frame(&mut editor, vec![egui::Event::PointerMoved(inside)]);
    hands.frame(
        &mut editor,
        vec![
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 2.0),
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Text("hi".to_string()),
        ],
    );
    let input = &editor.app.world.input;
    assert_eq!(input.scroll_delta(), Vec2::new(0.0, 2.0));
    assert_eq!(input.text_input(), "hi");
    let size = input.screen_size();
    assert!((size.x - view.width()).abs() < 1.5 && (size.y - view.height()).abs() < 1.5, "{size} vs {view:?}");
    let mouse = input.mouse_position();
    assert!((mouse.x - view.width() * 0.5).abs() < 1.5, "mouse in game pixels: {mouse}");

    // Кнопка мыши, отпущенная за пределами вида, всё равно отпускается в игре.
    hands.frame(&mut editor, vec![Hands::button(inside, true)]);
    assert!(editor.app.world.input.mouse_button(MouseButton::Left));
    hands.frame(&mut editor, vec![Hands::button(egui::pos2(5.0, 5.0), false)]);
    assert!(!editor.app.world.input.mouse_button(MouseButton::Left));
    editor.apply(Action::Stop);
}

#[test]
fn assets_and_folders_are_created_renamed_and_deleted_from_the_project_window() {
    let assets = temp_assets();
    let mut editor = editor_with_assets(&assets);
    let guid = std::fs::read_to_string(assets.join("prefabs/coin.prefab.meta")).unwrap();

    editor.apply(Action::CreateFolder("levels/bonus".to_string()));
    assert!(assets.join("levels/bonus").is_dir());

    // Переименование переносит и `.meta`: GUID тот же, ссылки по GUID живы.
    editor.apply(Action::RenameAsset {
        from: "prefabs/coin.prefab".to_string(),
        to: "levels/bonus/gold.prefab".to_string(),
    });
    assert!(!assets.join("prefabs/coin.prefab").exists());
    assert_eq!(std::fs::read_to_string(assets.join("levels/bonus/gold.prefab.meta")).unwrap(), guid);
    assert!(editor.project_row_id("levels/bonus/gold.prefab").is_some(), "the asset list was rescanned");

    // Имя занято — ошибка, файлы на месте.
    editor.apply(Action::RenameAsset {
        from: "levels/bonus/gold.prefab".to_string(),
        to: "prefabs/crystal.prefab".to_string(),
    });
    assert!(assets.join("levels/bonus/gold.prefab").is_file());

    // Открытая сцена переименовывается вместе с путём в редакторе.
    editor.apply(Action::RenameAsset {
        from: "scenes/level1.scene".to_string(),
        to: "scenes/first.scene".to_string(),
    });
    assert_eq!(editor.scene_path(), Some("scenes/first.scene"));

    editor.apply(Action::DeleteAsset("levels".to_string()));
    assert!(!assets.join("levels").exists());
    assert!(editor.project_row_id("levels/bonus/gold.prefab").is_none());
    let _ = std::fs::remove_dir_all(assets);
}

#[test]
fn the_editor_redraws_on_demand_in_edit_mode_and_continuously_in_play() {
    let mut editor = editor();
    assert!(!editor.wants_continuous_redraw(), "an idle editor does not keep the GPU busy");
    editor.apply(Action::Play);
    assert!(editor.wants_continuous_redraw());
    editor.apply(Action::Stop);
    let mut prefs = editor.preferences().clone();
    prefs.redraw_on_demand = false;
    editor.set_preferences(prefs);
    assert!(editor.wants_continuous_redraw());

    // Как часто перерисовывать в покое, egui узнаёт из `request_repaint_after`; сам
    // интервал проверить здесь нельзя: соседние тесты пишут в общий лог, а новые строки
    // консоли — законный повод для кадра.
}

#[test]
fn ui_scale_from_the_config_is_applied_to_the_interface() {
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let mut editor = Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene").ui_scale(1.5)).unwrap();
    assert_eq!(editor.preferences().ui_scale, 1.5);
    let ctx = egui::Context::default();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN / 1.5)),
            ..egui::RawInput::default()
        };
        let _ = ctx.run(input, |ctx| editor.ui(ctx));
    }
    assert!((ctx.zoom_factor() - 1.5).abs() < 1e-4);
}

/// Подробная галерея состояний для проверки внешнего вида: меню, всплывающие окна,
/// масштаб 150 %, Full HD. Нужен GPU:
/// `ENGINE_SHOTS_DIR=out cargo test -p engine --test editor -- --ignored gallery`.
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn editor_screenshot_gallery() {
    use engine::editor::{capture, capture_with_input, BottomTab, Preferences};
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    // Шаги «руки»: навести и щёлкнуть, или только навести (подменю раскрывается от наведения).
    let shoot = |editor: &mut Editor, name: &str, size: (u32, u32), steps: &[(egui::Pos2, bool)]| {
        let steps = steps.to_vec();
        let frames = 16 + steps.len() as u32 * 4;
        let image = capture_with_input(editor, size.0, size.1, frames, |frame, _| {
            let Some(at) = (frame as usize).checked_sub(4) else {
                return vec![];
            };
            match steps.get(at / 4) {
                Some((pos, _)) if at % 4 == 0 => vec![egui::Event::PointerMoved(*pos)],
                Some((pos, true)) if at % 4 == 1 => vec![Hands::button(*pos, true)],
                Some((pos, true)) if at % 4 == 2 => vec![Hands::button(*pos, false)],
                _ => vec![],
            }
        })
        .unwrap();
        image.save(dir.join(format!("{name}.png"))).unwrap();
    };

    let mut editor = editor();
    editor.apply(Action::Select(Some(find(&editor, "Player"))));
    shoot(&mut editor, "g_edit_1440", (1440, 860), &[]);
    shoot(&mut editor, "g_edit_1920", (1920, 1080), &[]);
    // Меню GameObject с открытым подменю UI.
    let env_pos = |name: &str, default: (f32, f32)| -> egui::Pos2 {
        std::env::var(name)
            .ok()
            .and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))
            })
            .unwrap_or(egui::pos2(default.0, default.1))
    };
    shoot(
        &mut editor,
        "g_menu_gameobject",
        (1440, 860),
        &[(env_pos("SHOT_MENU", (200.0, 15.0)), true), (env_pos("SHOT_SUB", (260.0, 210.0)), false)],
    );
    shoot(&mut editor, "g_menu_file", (1440, 860), &[(env_pos("SHOT_FILE", (96.0, 15.0)), true)]);
    editor.apply(Action::CreateEmpty { parent: None });
    shoot(&mut editor, "g_add_component", (1440, 860), &[(env_pos("SHOT_ADD", (1262.0, 374.0)), true)]);
    editor.apply(Action::Undo);
    editor.apply(Action::Select(Some(find(&editor, "Player"))));

    editor.set_bottom_tab(BottomTab::Project);
    editor.select_asset("sprites/player.png");
    let _ = capture(&mut editor, 1440, 860, 12).unwrap();
    shoot(&mut editor, "g_project_grid", (1440, 860), &[]);

    editor.apply(Action::AddComponent { object: find(&editor, "Player"), kind: "Rigidbdy2D".to_string() });
    log::warn!("a warning from the game, to show how the Console looks");
    editor.set_bottom_tab(BottomTab::Console);
    shoot(&mut editor, "g_console", (1440, 860), &[]);

    editor.show_preferences(true);
    shoot(&mut editor, "g_preferences", (1440, 860), &[]);
    editor.show_preferences(false);

    editor.apply(Action::Play);
    editor.app.world.input.set_key(Key::D, true);
    editor.set_bottom_tab(BottomTab::Profiler);
    let image = capture(&mut editor, 1440, 860, 90).unwrap();
    image.save(dir.join("g_play_profiler.png")).unwrap();
    editor.apply(Action::Stop);

    editor.apply(Action::CreateEmpty { parent: None });
    editor.apply(Action::NewScene);
    shoot(&mut editor, "g_dialog_unsaved", (1440, 860), &[]);

    // 150 % на Full HD.
    let mut big = self::editor();
    big.set_preferences(Preferences { ui_scale: 1.5, ..Default::default() });
    big.apply(Action::Select(Some(find(&big, "Player"))));
    shoot(&mut big, "g_edit_150_1920", (1920, 1080), &[]);
    shoot(&mut big, "g_edit_150_1440", (1440, 860), &[]);
}

/// Снимки редактора в трёх состояниях. Нужен GPU:
/// `ENGINE_SHOTS_DIR=out cargo test -p engine --test editor -- --ignored`.
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn editor_screenshots() {
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let (width, height) = (SCREEN.x as u32, SCREEN.y as u32);

    let mut editor = editor();
    editor.apply(Action::Select(Some(find(&editor, "Player"))));
    let image = engine::editor::capture(&mut editor, width, height, 3).unwrap();
    image.save(dir.join("editor_edit.png")).unwrap();

    editor.apply(Action::Play);
    editor.app.world.input.set_key(Key::D, true);
    let image = engine::editor::capture(&mut editor, width, height, 50).unwrap();
    image.save(dir.join("editor_play.png")).unwrap();

    editor.set_bottom_tab(engine::editor::BottomTab::Profiler);
    let image = engine::editor::capture(&mut editor, width, height, 120).unwrap();
    image.save(dir.join("editor_profiler.png")).unwrap();
    editor.apply(Action::Stop);

    // Консоль с ошибкой и вид Scene с инструментом поворота.
    editor.apply(Action::AddComponent {
        object: find(&editor, "Player"),
        kind: "Rigidbdy2D".to_string(),
    });
    log::warn!("a warning from the game, to show how the Console looks");
    editor.set_bottom_tab(engine::editor::BottomTab::Console);
    editor.set_tool(engine::editor::Tool::Rotate);
    editor.apply(Action::Select(Some(find(&editor, "Platform C"))));
    let image = engine::editor::capture(&mut editor, width, height, 3).unwrap();
    image.save(dir.join("editor_console.png")).unwrap();

    // Вкладка Code с исходником C++-плагина и 3D-сцена в редакторе.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut app = engine::app();
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let mut plugins = engine::cpp::CppPlugins::default();
    engine::cpp::builtin_gameplay(&mut plugins).unwrap();
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    component! {
        pub struct Orbit {
            pub target: Vec3 = Vec3::ZERO,
            pub distance: f32 = 9.5,
            pub height: f32 = 3.2,
        }
    }
    impl Component for Orbit {}
    app.register::<Orbit>();
    let config = EditorConfig::default()
        .open_scene("scenes/demo3d.scene")
        .code_dir(format!("{root}/plugins/cpp-gameplay"));
    let mut editor3d = Editor::new(app, config).unwrap();
    editor3d.apply(Action::Select(Some(find(&editor3d, "Crystal"))));
    let shot = engine::editor::capture(&mut editor3d, width, height, 3).unwrap();
    shot.save(dir.join("editor_3d.png")).unwrap();
    editor3d.apply(Action::OpenCode(PathBuf::from(format!("{root}/plugins/cpp-gameplay/gameplay.cpp"))));
    let shot = engine::editor::capture(&mut editor3d, width, height, 3).unwrap();
    shot.save(dir.join("editor_code.png")).unwrap();

    assert_eq!(image.width(), width);
    assert!(
        image.pixels().any(|pixel| pixel.0[..3] != image.get_pixel(0, 0).0[..3]),
        "the frame is not a flat fill"
    );
}

/// File → Build…: окно открывается действием и из меню, без крейта игры экспорт не
/// запускается, команда экспорта собирается из настроек окна, на этой машине
/// предлагаются только платформы, которые она умеет собрать.
#[test]
fn build_dialog_exports_the_configured_game_crate() {
    use engine::editor::{BuildStatus, ExportPlatform, ExportSettings};

    // Без крейта игры: экспорт не стартует, причина — в консоли.
    let mut plain = editor();
    plain.apply(Action::Export(ExportSettings::default()));
    assert_eq!(plain.build_status(), BuildStatus::Idle);
    let reason = plain.export_command(&ExportSettings::default()).unwrap_err().to_string();
    assert!(reason.contains("game_crate"), "{reason}");

    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let config = EditorConfig::default()
        .open_scene("scenes/level1.scene")
        .game_crate("template")
        .workspace(&workspace);
    let mut editor = Editor::new(app, config).unwrap();

    // Платформы: веб — везде, Windows — только на Windows.
    let platforms = ExportPlatform::available();
    assert_eq!(platforms[0], ExportPlatform::Web);
    assert_eq!(platforms.contains(&ExportPlatform::Windows), cfg!(windows));
    // Android — везде (недостающий SDK объясняет сам экспорт); Linux на Linux — всегда.
    assert!(platforms.contains(&ExportPlatform::Android));
    if cfg!(target_os = "linux") {
        assert!(platforms.contains(&ExportPlatform::Linux));
    }
    let android = ExportSettings {
        platform: ExportPlatform::Android,
        release: false,
        out_dir: String::new(),
    };
    let args = editor.export_command(&android).unwrap().args.join(" ");
    assert!(args.starts_with("run -q -p piska-export -- android --game template --out "), "{args}");
    assert!(editor
        .export_out_dir(&android)
        .unwrap()
        .ends_with(Path::new("dist").join("template").join("android")));
    if platforms.contains(&ExportPlatform::Linux) {
        let linux = ExportSettings { platform: ExportPlatform::Linux, ..android.clone() };
        assert!(editor.export_command(&linux).unwrap().args.contains(&"--tar".to_string()));
    } else {
        let linux = ExportSettings { platform: ExportPlatform::Linux, ..android.clone() };
        let reason = editor.export_command(&linux).unwrap_err().to_string();
        assert!(reason.contains("zig"), "{reason}");
    }

    // Команда: cargo run -p piska-export -- web --game template --out <dist/template/web> --release.
    let settings = ExportSettings {
        platform: ExportPlatform::Web,
        release: true,
        out_dir: String::new(),
    };
    let command = editor.export_command(&settings).unwrap();
    assert_eq!(command.program, "cargo");
    assert_eq!(command.dir.as_deref(), Some(workspace.as_path()));
    let args = command.args.join(" ");
    assert!(args.starts_with("run -q -p piska-export -- web --game template --out "), "{args}");
    assert!(args.ends_with(" --release"), "{args}");
    let out = editor.export_out_dir(&settings).unwrap();
    assert!(out.ends_with(Path::new("dist").join("template").join("web")), "{}", out.display());
    let custom = ExportSettings {
        out_dir: "build/site".to_string(),
        release: false,
        ..settings
    };
    assert_eq!(editor.export_out_dir(&custom).unwrap(), workspace.join("build/site"));
    assert!(!editor.export_command(&custom).unwrap().args.contains(&"--release".to_string()));

    // Окно: меню File → Build… открывает, «Close» закрывает; без экспорта нет кнопок
    // «Reveal» и «Run in Browser».
    let mut hands = Hands::new(&mut editor);
    assert!(!editor.export_dialog_open());
    let file_menu = hands.find_text(&mut editor, "File").expect("File menu");
    hands.click(&mut editor, file_menu);
    let item = hands.find_text(&mut editor, "Build…").expect("Build… item in the File menu");
    hands.click(&mut editor, item);
    hands.frame(&mut editor, vec![]);
    assert!(editor.export_dialog_open());
    assert!(hands.find_text(&mut editor, "games/template").is_some(), "the dialog names the game crate");
    assert!(hands.find_text(&mut editor, "Run in Browser").is_none(), "nothing exported yet");
    assert_eq!(hands.find_text(&mut editor, "Windows").is_some(), cfg!(windows));
    let release = hands.find_text(&mut editor, "Release").expect("Release option");
    hands.click(&mut editor, release);
    assert!(editor.export_settings().release, "the segmented control switched to Release");
    let close = hands.find_text(&mut editor, "Close").expect("Close button");
    hands.click(&mut editor, close);
    assert!(!editor.export_dialog_open());

    editor.apply(Action::ShowBuildDialog(true));
    assert!(editor.export_dialog_open());
    // Без экспорта показывать нечего.
    editor.apply(Action::RunInBrowser);
    assert_eq!(editor.build_status(), BuildStatus::Idle);
}

// ---------------------------------------------------------------------------
// Окна как в Unity: док, раскладки, перетаскивание вкладок, плавающие окна
// ---------------------------------------------------------------------------

mod dock {
    use super::*;
    use engine::editor::{DockSide, LayoutPreset, Panel};

    fn rect(editor: &Editor, panel: Panel) -> egui::Rect {
        editor
            .panel_rect(panel)
            .unwrap_or_else(|| panic!("{panel:?} was drawn in the last frame"))
    }

    /// Перетащить вкладку, как это делает человек: довести и чуть подержать над целью
    /// (док «прилипает» к прошлой цели на доли секунды, чтобы подсветка не мигала).
    fn drag_tab(hands: &mut Hands, editor: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
        hands.frame(editor, vec![egui::Event::PointerMoved(from)]);
        hands.frame(editor, vec![Hands::button(from, true)]);
        for step in 1..=12 {
            let pos = from + (to - from) * (step as f32 / 12.0);
            hands.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        }
        for _ in 0..30 {
            hands.frame(editor, vec![egui::Event::PointerMoved(to)]);
        }
        hands.frame(editor, vec![Hands::button(to, false)]);
        hands.frame(editor, vec![]);
    }

    fn tab(editor: &Editor, panel: Panel) -> egui::Pos2 {
        editor
            .panel_tab_rect(panel)
            .unwrap_or_else(|| panic!("{panel:?} has a tab"))
            .center()
    }

    #[test]
    fn the_default_layout_docks_every_window_like_unity() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        hands.frame(&mut editor, vec![]);
        let mut open = editor.open_panels();
        open.sort_by_key(|p| p.title());
        let mut all = Panel::ALL.to_vec();
        all.sort_by_key(|p| p.title());
        assert_eq!(open, all, "every window is open");

        // Hierarchy и Inspector по бокам во всю высоту, Scene в центре, Project под ним.
        let (hierarchy, scene, inspector, project) = (
            rect(&editor, Panel::Hierarchy),
            rect(&editor, Panel::Scene),
            rect(&editor, Panel::Inspector),
            rect(&editor, Panel::Project),
        );
        assert!(hierarchy.right() <= scene.left() && scene.right() <= inspector.left());
        assert!(scene.bottom() <= project.top() && hierarchy.right() <= project.left());
        assert!(inspector.height() > scene.height() + project.height() * 0.9, "the Inspector is full height");
        assert!((hierarchy.height() - inspector.height()).abs() < 1.0, "so is the Hierarchy");
        assert!(scene.width() > 600.0 && scene.height() > 350.0, "a roomy Scene view: {scene:?}");
        // Над видом Scene — его полоса (Gizmos, Camera), вид занимает остальное.
        let view = editor.viewport_rect();
        assert_eq!((view.left(), view.right(), view.bottom()), (scene.left(), scene.right(), scene.bottom()));
        assert!(view.top() > scene.top() && view.top() - scene.top() <= 32.0, "{view:?} in {scene:?}");
        // Game, Code, Console, Profiler — скрытые вкладки: не рисуются.
        assert!(editor.is_panel_visible(Panel::Scene) && !editor.is_panel_visible(Panel::Game));
        assert!(editor.panel_rect(Panel::Game).is_none());
        assert!(editor.panel_tab_rect(Panel::Game).is_some(), "but their tabs are there");
        // Полоса вкладок — плотная, но не мельче 22 точек (docs/design/ui-guidelines.md).
        let tab_height = editor.panel_tab_rect(Panel::Scene).unwrap().height();
        assert!((22.0..=26.0).contains(&tab_height), "{tab_height}");
        assert!(hands.find_text(&mut editor, "Inspector").is_some());
    }

    #[test]
    fn every_preset_is_a_valid_layout_with_all_windows() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        for preset in LayoutPreset::ALL {
            editor.set_layout(preset);
            hands.frame(&mut editor, vec![]);
            hands.frame(&mut editor, vec![]);
            assert_eq!(editor.open_panels().len(), Panel::ALL.len(), "{}", preset.name());
            assert!(editor.is_panel_visible(Panel::Scene), "{}", preset.name());
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN);
            let shown: Vec<(Panel, egui::Rect)> = Panel::ALL
                .into_iter()
                .filter_map(|panel| Some((panel, editor.panel_rect(panel)?)))
                .collect();
            for (panel, area) in &shown {
                assert!(screen.contains_rect(*area), "{} / {panel:?}: {area:?}", preset.name());
                assert!(
                    area.width() >= 100.0 && area.height() >= 80.0,
                    "{} / {panel:?} is usable: {area:?}",
                    preset.name()
                );
                for (other, other_area) in &shown {
                    if other != panel {
                        let overlap = area.intersect(*other_area);
                        assert!(
                            overlap.width() <= 0.5 || overlap.height() <= 0.5,
                            "{}: {panel:?} overlaps {other:?}",
                            preset.name()
                        );
                    }
                }
            }
        }
        // 2 by 3 показывает Scene и Game одновременно.
        editor.set_layout(LayoutPreset::TwoByThree);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Game) && editor.is_panel_visible(Panel::Scene));
        assert!(rect(&editor, Panel::Scene).bottom() <= rect(&editor, Panel::Game).top());
    }

    #[test]
    fn windows_are_docked_split_floated_closed_and_reopened_through_the_api() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);

        editor.dock_panel(Panel::Game, Panel::Scene, DockSide::Right).unwrap();
        hands.frame(&mut editor, vec![]);
        let (scene, game) = (rect(&editor, Panel::Scene), rect(&editor, Panel::Game));
        assert!(scene.right() <= game.left() && (scene.top() - game.top()).abs() < 1.0, "side by side");
        assert!(editor.dock_panel(Panel::Game, Panel::Game, DockSide::Left).is_err());

        editor.dock_panel(Panel::Console, Panel::Inspector, DockSide::Center).unwrap();
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Console) && !editor.is_panel_visible(Panel::Inspector));

        let at = egui::Rect::from_min_size(egui::pos2(500.0, 220.0), egui::vec2(360.0, 300.0));
        editor.float_panel(Panel::Hierarchy, Some(at));
        hands.frame(&mut editor, vec![]);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_floating(Panel::Hierarchy));
        let floating = rect(&editor, Panel::Hierarchy);
        assert!(
            at.expand(16.0).contains_rect(floating),
            "the floating window is where it was put: {floating:?}"
        );
        // Плавающее окно работает как обычное: щелчок по строке выделяет объект.
        let first = editor.app.world.scene.roots()[0];
        assert!(floating.contains(hands.row_center(first)));
        hands.click(&mut editor, hands.row_center(first));
        assert_eq!(editor.selection(), Some(first));
        // И пристыковывается обратно.
        editor.dock_panel(Panel::Hierarchy, Panel::Project, DockSide::Left).unwrap();
        hands.frame(&mut editor, vec![]);
        assert!(!editor.is_panel_floating(Panel::Hierarchy));
        assert!(rect(&editor, Panel::Hierarchy).right() <= rect(&editor, Panel::Project).left() + 1.0);

        // Закрыть последнюю вкладку области — область исчезает, соседи занимают место.
        let scene_before = rect(&editor, Panel::Scene);
        editor.close_panel(Panel::Game);
        hands.frame(&mut editor, vec![]);
        assert!(!editor.is_panel_open(Panel::Game));
        assert!(rect(&editor, Panel::Scene).width() > scene_before.width() + 100.0);
        // Ctrl+2 открывает Game снова — вкладкой рядом со Scene.
        hands.press(&mut editor, egui::Key::Num2, egui::Modifiers::COMMAND);
        assert!(editor.is_panel_open(Panel::Game) && editor.is_panel_visible(Panel::Game));
        assert!(!editor.is_panel_visible(Panel::Scene), "Game opened as a tab of the Scene area");
    }

    #[test]
    fn dragging_a_tab_onto_the_edge_of_an_area_splits_it() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        let scene = rect(&editor, Panel::Scene);
        let from = tab(&editor, Panel::Console);
        let to = egui::pos2(scene.right() - 20.0, scene.center().y);
        drag_tab(&mut hands, &mut editor, from, to);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Console), "Console got its own area");
        assert!(editor.is_panel_visible(Panel::Scene));
        let (scene, console) = (rect(&editor, Panel::Scene), rect(&editor, Panel::Console));
        assert!(scene.right() <= console.left(), "to the right of the Scene: {scene:?} {console:?}");
        assert!((scene.top() - console.top()).abs() < 1.0);
        assert!(editor.is_panel_visible(Panel::Project), "Project stays in the bottom area");
    }

    #[test]
    fn dragging_a_tab_over_the_tab_bar_of_another_area_moves_it_there() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        let from = tab(&editor, Panel::Profiler);
        let inspector_tab = editor.panel_tab_rect(Panel::Inspector).unwrap();
        let to = inspector_tab.right_center() + egui::vec2(40.0, 0.0);
        drag_tab(&mut hands, &mut editor, from, to);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Profiler));
        let profiler_tab = editor.panel_tab_rect(Panel::Profiler).unwrap();
        assert!(
            (profiler_tab.center().y - inspector_tab.center().y).abs() < 1.0,
            "in the Inspector's tab bar"
        );
        assert!(!editor.is_panel_visible(Panel::Inspector), "the moved tab is the active one");
    }

    #[test]
    fn dragging_a_tab_to_the_window_edge_docks_it_along_the_whole_side() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        let inspector = rect(&editor, Panel::Inspector);
        let from = tab(&editor, Panel::Console);
        // Поле у самого края окна редактора.
        let to = egui::pos2(SCREEN.x - 2.0, inspector.center().y);
        drag_tab(&mut hands, &mut editor, from, to);
        hands.frame(&mut editor, vec![]);
        let console = rect(&editor, Panel::Console);
        let inspector = rect(&editor, Panel::Inspector);
        assert!(console.left() >= inspector.right() - 1.0, "right of everything: {console:?}");
        assert!((console.height() - inspector.height()).abs() < 2.0, "full height");
    }

    #[test]
    fn dragging_a_tab_into_the_middle_of_an_area_makes_a_floating_window() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        let scene = rect(&editor, Panel::Scene);
        let from = tab(&editor, Panel::Inspector);
        drag_tab(&mut hands, &mut editor, from, scene.center());
        hands.frame(&mut editor, vec![]);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_floating(Panel::Inspector));
        let floating = rect(&editor, Panel::Inspector);
        assert!(floating.intersects(scene), "the window floats over the dock: {floating:?}");
        // Его вкладку можно утащить обратно на полосу вкладок области.
        let from = tab(&editor, Panel::Inspector);
        let to = editor.panel_tab_rect(Panel::Hierarchy).unwrap().right_center() + egui::vec2(40.0, 0.0);
        drag_tab(&mut hands, &mut editor, from, to);
        hands.frame(&mut editor, vec![]);
        assert!(!editor.is_panel_floating(Panel::Inspector), "docked back");
    }

    #[test]
    fn double_click_and_shift_space_maximize_and_restore() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        let normal = rect(&editor, Panel::Scene);
        let title = tab(&editor, Panel::Scene);
        let double_click = |hands: &mut Hands, editor: &mut Editor| {
            hands.frame(editor, vec![egui::Event::PointerMoved(title)]);
            for _ in 0..2 {
                hands.frame(editor, vec![Hands::button(title, true)]);
                hands.frame(editor, vec![Hands::button(title, false)]);
            }
            hands.frame(editor, vec![]);
        };
        double_click(&mut hands, &mut editor);
        assert_eq!(editor.maximized_panel(), Some(Panel::Scene));
        let big = rect(&editor, Panel::Scene);
        assert!(big.width() > normal.width() + 300.0 && big.height() > normal.height() + 150.0, "{big:?}");
        assert!(editor.panel_rect(Panel::Hierarchy).is_none(), "nothing else is drawn");
        // Полсекунды паузы: иначе следующий щелчок egui сочтёт продолжением этого.
        for _ in 0..30 {
            hands.frame(&mut editor, vec![]);
        }
        let title = tab(&editor, Panel::Scene);
        let double_click = |hands: &mut Hands, editor: &mut Editor| {
            hands.frame(editor, vec![egui::Event::PointerMoved(title)]);
            for _ in 0..2 {
                hands.frame(editor, vec![Hands::button(title, true)]);
                hands.frame(editor, vec![Hands::button(title, false)]);
            }
            hands.frame(editor, vec![]);
        };
        double_click(&mut hands, &mut editor);
        assert_eq!(editor.maximized_panel(), None);
        assert_eq!(rect(&editor, Panel::Scene), normal);

        // Shift+Space над окном Project.
        let project = rect(&editor, Panel::Project).center();
        hands.frame(&mut editor, vec![egui::Event::PointerMoved(project)]);
        hands.press(&mut editor, egui::Key::Space, egui::Modifiers::SHIFT);
        assert_eq!(editor.maximized_panel(), Some(Panel::Project));
        hands.press(&mut editor, egui::Key::Space, egui::Modifiers::SHIFT);
        assert_eq!(editor.maximized_panel(), None);
    }

    #[test]
    fn the_layout_survives_a_restart_and_a_broken_one_falls_back_to_default() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        let default_layout = editor.layout_text();
        editor.dock_panel(Panel::Game, Panel::Scene, DockSide::Bottom).unwrap();
        editor.float_panel(
            Panel::Profiler,
            Some(egui::Rect::from_min_size(egui::pos2(300.0, 300.0), egui::vec2(420.0, 260.0))),
        );
        editor.set_maximize_on_play(true);
        hands.frame(&mut editor, vec![]);
        hands.frame(&mut editor, vec![]);
        let prefs = editor.preferences().clone();
        let saved = prefs.layout.clone().expect("the layout is written into the preferences");
        assert_eq!(saved, editor.layout_text());

        // «Перезапуск»: новый редактор получает настройки с диска.
        let mut restarted = self::editor();
        restarted.set_preferences(prefs.clone());
        assert_eq!(restarted.layout_text(), saved);
        assert!(restarted.is_panel_floating(Panel::Profiler) && restarted.maximize_on_play());
        let mut hands = Hands::new(&mut restarted);
        hands.frame(&mut restarted, vec![]);
        assert!(restarted.is_panel_visible(Panel::Game) && restarted.is_panel_visible(Panel::Scene));

        // Испорченная или устаревшая раскладка — Default без диалога.
        for broken in ["garbage", "piska-layout 0; main (L 0 scene)", "piska-layout 1; main (L 0 scene scene)"] {
            let mut fresh = self::editor();
            fresh.dock_panel(Panel::Game, Panel::Scene, DockSide::Left).unwrap();
            let mut prefs = prefs.clone();
            prefs.layout = Some(broken.to_string());
            fresh.set_preferences(prefs);
            assert_eq!(fresh.layout_text(), default_layout, "{broken}");
            assert!(!fresh.has_dialog());
        }
    }

    #[test]
    fn named_layouts_are_saved_loaded_and_deleted() {
        let mut editor = editor();
        editor.set_layout(LayoutPreset::Tall);
        let tall = editor.layout_text();
        editor.save_layout("My: Tall").unwrap();
        assert_eq!(editor.saved_layouts(), vec!["My Tall".to_string()]);
        assert!(editor.save_layout("  ").is_err());
        editor.set_layout(LayoutPreset::Wide);
        assert_ne!(editor.layout_text(), tall);
        editor.load_layout("My Tall").unwrap();
        assert_eq!(editor.layout_text(), tall);
        assert!(editor.load_layout("nothing").is_err());
        // Именованные раскладки живут в настройках.
        assert_eq!(editor.preferences().saved_layouts.len(), 1);
        editor.delete_layout("My Tall").unwrap();
        assert!(editor.saved_layouts().is_empty());
    }

    #[test]
    fn the_window_menu_reopens_windows_and_switches_layouts() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        editor.close_panel(Panel::Console);
        hands.frame(&mut editor, vec![]);
        let menu = hands.find_text(&mut editor, "Window").expect("Window menu");
        hands.click(&mut editor, menu);
        let console = hands.find_text(&mut editor, "Console").expect("Console item");
        hands.click(&mut editor, console);
        assert!(editor.is_panel_open(Panel::Console) && editor.is_panel_visible(Panel::Console));

        hands.click(&mut editor, menu);
        let layouts = hands.find_text(&mut editor, "Layouts").expect("Layouts submenu");
        hands.frame(&mut editor, vec![egui::Event::PointerMoved(layouts)]);
        hands.frame(&mut editor, vec![]);
        let wide = hands.find_text(&mut editor, "Wide").expect("Wide preset");
        hands.click(&mut editor, wide);
        hands.frame(&mut editor, vec![]);
        let console = rect(&editor, Panel::Console);
        let scene = rect(&editor, Panel::Scene);
        assert!(console.top() >= scene.bottom() - 1.0, "Wide: Console below the views");
    }

    #[test]
    fn entering_play_brings_the_game_view_to_front_and_maximize_on_play_works() {
        let mut editor = editor();
        let mut hands = Hands::new(&mut editor);
        assert!(!editor.is_panel_visible(Panel::Game));
        editor.apply(Action::Play);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Game), "Play shows the Game tab");
        assert_eq!(editor.viewport_rect(), editor.game_view_rect());
        editor.apply(Action::Stop);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Scene));

        // Game рядом со Scene: Play не прячет Scene.
        editor.dock_panel(Panel::Game, Panel::Scene, DockSide::Right).unwrap();
        editor.focus_panel(Panel::Hierarchy);
        editor.apply(Action::Play);
        hands.frame(&mut editor, vec![]);
        assert!(editor.is_panel_visible(Panel::Game) && editor.is_panel_visible(Panel::Scene));
        editor.apply(Action::Stop);
        hands.frame(&mut editor, vec![]);

        // Maximize On Play — кнопка в окне Game.
        let toggle = hands
            .find_text(&mut editor, "Maximize On Play")
            .expect("toggle in the Game toolbar");
        hands.click(&mut editor, toggle);
        assert!(editor.maximize_on_play());
        editor.apply(Action::Play);
        hands.frame(&mut editor, vec![]);
        assert_eq!(editor.maximized_panel(), Some(Panel::Game));
        editor.apply(Action::Stop);
        hands.frame(&mut editor, vec![]);
        assert_eq!(editor.maximized_panel(), None, "Stop restores the layout");
        assert!(editor.is_panel_visible(Panel::Scene));
    }

    /// Scene и Game рядом рисуются оба, каждый в свою текстуру своего размера. Нужен GPU.
    #[test]
    #[ignore = "needs a GPU"]
    fn scene_and_game_side_by_side_both_render() {
        let mut editor = editor();
        editor.apply(Action::Play);
        let _ = engine::editor::capture(&mut editor, SCREEN.x as u32, SCREEN.y as u32, 60).unwrap();
        let one_view = editor.frame_timings().render_ms;
        editor.apply(Action::Stop);
        editor.dock_panel(Panel::Game, Panel::Scene, DockSide::Right).unwrap();
        editor.apply(Action::Play);
        let image = engine::editor::capture(&mut editor, SCREEN.x as u32, SCREEN.y as u32, 60).unwrap();
        let two_views = editor.frame_timings().render_ms;
        editor.apply(Action::Stop);
        println!("scene render CPU time: one view {one_view:.2} ms, Scene + Game {two_views:.2} ms");
        // Фон окна (BG_PANEL) — если вид не нарисован, там был бы он или BG_APP.
        let backgrounds = [[0x1e, 0x1f, 0x23], [0x10, 0x11, 0x13]];
        for panel in [Panel::Scene, Panel::Game] {
            let area = rect(&editor, panel).shrink(30.0);
            let mut differs = 0;
            let mut total = 0;
            for y in (area.top() as u32..area.bottom() as u32).step_by(7) {
                for x in (area.left() as u32..area.right() as u32).step_by(7) {
                    let pixel = image.get_pixel(x, y).0;
                    total += 1;
                    let far = backgrounds.iter().all(|bg| {
                        (0..3).map(|c| (pixel[c] as i32 - bg[c]).abs()).sum::<i32>() > 24
                    });
                    if far {
                        differs += 1;
                    }
                }
            }
            assert!(differs * 2 > total, "{panel:?} shows the rendered scene: {differs} of {total} pixels");
        }
        let (scene, game) = (rect(&editor, Panel::Scene), rect(&editor, Panel::Game));
        assert!(scene.width() > 300.0 && game.width() > 300.0);
    }

    /// Снимки раскладок для проверки глазами. Нужен GPU:
    /// `ENGINE_SHOTS_DIR=out cargo test -p engine --test editor -- --ignored dock_screenshots`.
    #[test]
    #[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
    fn dock_screenshots() {
        use engine::editor::{capture, capture_with_input};
        let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (SCREEN.x as u32, SCREEN.y as u32);
        let save = |image: image::RgbaImage, name: &str| image.save(dir.join(format!("{name}.png"))).unwrap();

        let mut editor = editor();
        editor.apply(Action::Select(Some(find(&editor, "Player"))));
        for (preset, name) in [
            (LayoutPreset::Default, "dock_default"),
            (LayoutPreset::TwoByThree, "dock_2by3"),
            (LayoutPreset::Tall, "dock_tall"),
            (LayoutPreset::Wide, "dock_wide"),
        ] {
            editor.set_layout(preset);
            save(capture(&mut editor, w, h, 4).unwrap(), name);
        }

        // Вкладку Console тянут к правому краю Scene: видна подсветка будущей области.
        editor.set_layout(LayoutPreset::Default);
        let _ = capture(&mut editor, w, h, 3).unwrap();
        let from = editor.panel_tab_rect(Panel::Console).unwrap().center();
        let scene = editor.panel_rect(Panel::Scene).unwrap();
        let to = egui::pos2(scene.right() - 30.0, scene.center().y);
        // Новый контекст egui: первые кадры уходят на шрифты и раскладку.
        let image = capture_with_input(&mut editor, w, h, 24, |frame, _| match frame {
            3 => vec![egui::Event::PointerMoved(from)],
            4 => vec![Hands::button(from, true)],
            5..=14 => vec![egui::Event::PointerMoved(from + (to - from) * ((frame - 4) as f32 / 10.0))],
            _ => vec![egui::Event::PointerMoved(to)],
        })
        .unwrap();
        save(image, "dock_drag_preview");
        // Отпустить кнопку (снимок сделан с зажатой кнопкой).
        let _ = capture_with_input(&mut editor, w, h, 3, |frame, _| {
            if frame == 0 {
                vec![Hands::button(to, false)]
            } else {
                vec![]
            }
        });

        editor.set_layout(LayoutPreset::Default);
        editor.float_panel(
            Panel::Inspector,
            Some(egui::Rect::from_min_size(egui::pos2(520.0, 170.0), egui::vec2(380.0, 470.0))),
        );
        save(capture(&mut editor, w, h, 4).unwrap(), "dock_floating_inspector");

        editor.set_layout(LayoutPreset::Default);
        editor.maximize_panel(Some(Panel::Scene));
        save(capture(&mut editor, w, h, 4).unwrap(), "dock_maximized_scene");
        editor.maximize_panel(None);

        editor.dock_panel(Panel::Game, Panel::Scene, DockSide::Right).unwrap();
        editor.apply(Action::Play);
        editor.app.world.input.set_key(Key::D, true);
        save(capture(&mut editor, w, h, 40).unwrap(), "dock_scene_game_play");
        editor.apply(Action::Stop);

        editor.set_layout(LayoutPreset::Default);
        let menu = std::env::var("SHOT_WINDOW_MENU")
            .ok()
            .and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))
            })
            .unwrap_or(egui::pos2(415.0, 15.0));
        let image = capture_with_input(&mut editor, w, h, 12, |frame, _| match frame {
            2 => vec![egui::Event::PointerMoved(menu)],
            3 => vec![Hands::button(menu, true)],
            4 => vec![Hands::button(menu, false)],
            _ => vec![],
        })
        .unwrap();
        save(image, "dock_window_menu");
    }
}

// ----------------------------------------------------------------------
// Скрипты на Rust: создание, mod.rs, сеанс перезапуска, потерянные скрипты
// ----------------------------------------------------------------------

/// Пустой каталог во временной папке для тестов скриптов на Rust.
fn rust_temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "piska_rust_scripts_{tag}_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn logged(text: &str) -> bool {
    engine::core::logging::records().iter().any(|r| r.message.contains(text))
}

#[test]
fn a_new_rust_script_is_written_listed_in_mod_rs_opened_and_built() {
    use engine::editor::{BuildStatus, Relaunch};
    let dir = rust_temp_dir("new");
    let scripts = dir.join("scripts");
    // Сборка не запускает настоящий cargo: такой программы нет, сборка сразу «не удаётся».
    let mut relaunch = Relaunch::example("engine", "demo3d");
    relaunch.build.program = "piska-test-no-such-cargo".to_string();
    let mut editor = {
        let mut app = engine::app();
        app.spawn("Guard");
        let config = EditorConfig::default().code_dir(&dir).rust_scripts(&scripts).relaunch(relaunch);
        Editor::new(app, config).unwrap()
    };
    let guard = find(&editor, "Guard");

    // Имя — имя типа на Rust в UpperCamelCase; остальное отклоняется, файлов нет.
    for bad in ["enemy", "Enemy Patrol", "Enemy_Patrol", "9Lives", "", "Type"] {
        editor.apply(Action::NewRustScript { name: bad.to_string(), attach_to: Some(guard) });
    }
    assert!(!scripts.exists() || std::fs::read_dir(&scripts).unwrap().next().is_none());
    assert!(editor.pending_scripts().is_empty());

    editor.apply(Action::NewRustScript { name: "EnemyPatrol".to_string(), attach_to: Some(guard) });
    let file = scripts.join("enemy_patrol.rs");
    let source = std::fs::read_to_string(&file).expect("the script file is created");
    assert!(source.contains("pub struct EnemyPatrol {"), "{source}");
    assert!(source.contains("impl Component for EnemyPatrol {"), "{source}");
    let mod_rs = std::fs::read_to_string(scripts.join("mod.rs")).expect("mod.rs is written");
    assert!(mod_rs.contains("pub mod enemy_patrol;"), "{mod_rs}");
    assert!(mod_rs.contains("app.register::<enemy_patrol::EnemyPatrol>();"), "{mod_rs}");
    // Файл открыт во вкладке Code, скрипт ждёт перезапуска, сборка запущена.
    assert_eq!(editor.view_tab(), ViewTab::Code);
    assert_eq!(editor.code_document().unwrap().0, file.as_path());
    assert_eq!(editor.pending_scripts(), [("EnemyPatrol".to_string(), guard)]);
    assert!(editor.is_rust_build_pending());
    assert_ne!(editor.build_status(), BuildStatus::Idle, "the build was started");
    for _ in 0..400 {
        if editor.build_status() != BuildStatus::Running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    // Сборка не удалась — редактор работает дальше с прежним кодом и не перезапускается.
    assert!(matches!(editor.build_status(), BuildStatus::Failed { .. }), "{:?}", editor.build_status());
    for _ in 0..3 {
        editor.advance(DT);
    }
    assert!(!editor.should_quit());
    assert!(editor.is_rust_build_pending(), "still not built");
    assert!(logged("piska-test-no-such-cargo"));

    // Второй скрипт с тем же именем создать нельзя.
    editor.apply(Action::NewRustScript { name: "EnemyPatrol".to_string(), attach_to: None });
    assert_eq!(editor.pending_scripts().len(), 1);

    // Переименование: mod.rs и открытый документ следуют за файлом.
    editor.apply(Action::RenameScript { from: file.clone(), to: "patrol.rs".to_string() });
    let renamed = scripts.join("patrol.rs");
    assert!(renamed.is_file() && !file.exists());
    let mod_rs = std::fs::read_to_string(scripts.join("mod.rs")).unwrap();
    assert!(mod_rs.contains("pub mod patrol;") && mod_rs.contains("app.register::<patrol::EnemyPatrol>();"), "{mod_rs}");
    assert!(!mod_rs.contains("enemy_patrol"), "{mod_rs}");
    assert_eq!(editor.code_document().unwrap().0, renamed.as_path());

    // Удаление: файл исчезает, mod.rs пуст, вкладка закрыта.
    editor.apply(Action::DeleteScript(renamed.clone()));
    assert!(!renamed.exists());
    let mod_rs = std::fs::read_to_string(scripts.join("mod.rs")).unwrap();
    assert!(!mod_rs.contains("pub mod") && mod_rs.contains("let _ = app;"), "{mod_rs}");
    assert!(editor.code_document().is_none());

    // Без настройки редактор объясняет, что задать.
    let mut plain = Editor::new(engine::app(), EditorConfig::default()).unwrap();
    plain.apply(Action::NewRustScript { name: "Anything".to_string(), attach_to: None });
    assert!(logged("rust_scripts("));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_new_script_dialog_offers_rust_and_remembers_the_language() {
    let dir = rust_temp_dir("dialog");
    let mut editor = {
        let mut app = engine::app();
        app.spawn("Guard");
        Editor::new(app, EditorConfig::default().code_dir(&dir).rust_scripts(dir.join("scripts"))).unwrap()
    };
    let guard = find(&editor, "Guard");
    editor.apply(Action::Select(Some(guard)));
    let mut hands = Hands::new(&mut editor);
    // Assets → New Rust Script…
    let assets = hands.find_text(&mut editor, "Assets").expect("the Assets menu");
    hands.click(&mut editor, assets);
    let item = hands.find_text(&mut editor, "New Rust Script…").expect("the menu item");
    hands.click(&mut editor, item);
    assert!(editor.has_dialog());
    assert!(hands.find_text(&mut editor, "New Rust Script").is_some(), "the dialog title");
    hands.frame(&mut editor, vec![egui::Event::Text("Wanderer".to_string())]);
    let create = hands.find_text(&mut editor, "Create and Build").expect("the confirm button");
    hands.click(&mut editor, create);
    assert!(!editor.has_dialog());
    let file = dir.join("scripts").join("wanderer.rs");
    assert!(file.is_file(), "the dialog created the Rust script");
    // Без команды перезапуска скрипт ждёт ручной сборки и висит в ожидании.
    assert_eq!(editor.pending_scripts(), [("Wanderer".to_string(), guard)]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_rebuilt_editor_restores_scene_edits_selection_files_and_pending_scripts() {
    let project = temp_assets();
    let code = rust_temp_dir("session");
    let notes = code.join("notes.rs");
    std::fs::write(&notes, "fn main() {}\n// two\n// three\n// four\n").unwrap();
    let cpp = code.join("cpp");
    let open = |extra: bool| {
        let mut app = engine::app();
        scripts::register(&mut app);
        if extra {
            // Новая программа: в ней появился компонент, который ждали.
            app.register::<EnemyPatrol>();
        }
        engine::mount_assets(&mut app, &project).unwrap();
        let config = EditorConfig::default().open_scene("scenes/level1.scene").code_dir(&code).cpp_scripts(&cpp);
        Editor::new(app, config).unwrap()
    };

    let mut before = open(false);
    let player = find(&before, "Player");
    before.apply(Action::Rename { id: player, name: "Hero".to_string() });
    before.apply(Action::CreateEmpty { parent: None });
    let spawner = before.selection().expect("the new object is selected");
    before.apply(Action::Rename { id: spawner, name: "Spawner".to_string() });
    let hero = find(&before, "Hero");
    // Скрипт ждёт сборки, чтобы повеситься на Hero (без команд сборки он просто ждёт).
    before.apply(Action::NewScript { name: "EnemyPatrol".to_string(), attach_to: Some(hero) });
    before.apply(Action::OpenCode(notes.clone()));
    before.apply(Action::Select(Some(spawner)));
    assert!(before.is_dirty());
    let session = code.join("session.txt");
    before.save_session(&session).unwrap();

    let mut after = open(true);
    assert!(after.app.world.scene.find_by_name("Hero").is_none(), "a fresh editor has the file's scene");
    after.resume_session(&session).unwrap();
    assert!(!session.exists(), "the session file is consumed: the old editor may close");
    // Несохранённые правки, путь сцены и флаг «изменено».
    assert!(after.is_dirty());
    assert_eq!(after.scene_path(), Some("scenes/level1.scene"));
    assert!(after.app.world.scene.find_by_name("Player").is_none());
    let hero = find(&after, "Hero");
    let selected = after.selection().expect("the selection is restored");
    assert_eq!(after.app.world.scene.get(selected).unwrap().name, "Spawner");
    // Ожидавший скрипт повешен, вкладка Code показывает тот же файл.
    let kinds: Vec<_> = after.app.world.scene.get(hero).unwrap().components().map(|c| c.type_name()).collect();
    assert!(kinds.contains(&"EnemyPatrol"), "{kinds:?}");
    assert!(after.pending_scripts().is_empty());
    assert_eq!(after.code_document().unwrap().0, notes.as_path());
    assert_eq!(after.view_tab(), ViewTab::Code);
    assert_eq!(after.mode(), Mode::Edit);
    assert!(logged("reloaded scripts: +EnemyPatrol"));
    // Сохраняется туда же, откуда сцену открыли.
    after.apply(Action::Save);
    let saved = std::fs::read_to_string(project.join("scenes/level1.scene")).unwrap();
    assert!(saved.contains("\"Hero\"") && saved.contains("\"Spawner\"") && saved.contains("EnemyPatrol"));
    let _ = std::fs::remove_dir_all(project);
    let _ = std::fs::remove_dir_all(code);
}

#[test]
fn a_scene_with_a_component_that_no_longer_exists_survives_load_and_save() {
    let project = temp_assets();
    let scene = r#"(
    name: "Gone",
    objects: [
        (
            name: "Keeper",
            components: [
                (kind: "VanishedScript", fields: {
                    "speed": Float(2.5),
                    "label": Str("hi"),
                }),
                (kind: "Camera"),
            ],
        ),
    ],
)
"#;
    std::fs::write(project.join("scenes/gone.scene"), scene).unwrap();
    let mut editor = {
        let mut app = engine::app();
        engine::mount_assets(&mut app, &project).unwrap();
        Editor::new(app, EditorConfig::default().open_scene("scenes/gone.scene")).expect("the scene opens")
    };
    let keeper = find(&editor, "Keeper");
    let kinds: Vec<_> = editor.app.world.scene.get(keeper).unwrap().components().map(|c| c.type_name()).collect();
    assert_eq!(kinds, ["VanishedScript", "Camera"]);
    assert!(logged("script `VanishedScript` is missing"));

    // Инспектор показывает предупреждение, поля — только для чтения.
    editor.apply(Action::Select(Some(keeper)));
    let mut hands = Hands::new(&mut editor);
    let warning = "Missing script: `VanishedScript` is not in this build. Its data is kept and saved unchanged; \
                   restore the type and build, or remove the component.";
    assert!(hands.find_text(&mut editor, warning).is_some(), "the inspector warns about the missing script");

    // Play / Stop и Undo не теряют его.
    editor.apply(Action::Play);
    for _ in 0..3 {
        editor.advance(DT);
    }
    editor.apply(Action::Stop);
    let keeper = find(&editor, "Keeper");
    let component = editor.app.world.scene.get(keeper).unwrap().components().next().unwrap();
    assert_eq!(component.type_name(), "VanishedScript");
    assert_eq!(component.get_field("speed"), Some(Value::Float(2.5)));

    // Сохранение пишет его ровно таким, каким прочитали.
    editor.apply(Action::Rename { id: keeper, name: "Keeper 2".to_string() });
    editor.apply(Action::Save);
    let saved = std::fs::read_to_string(project.join("scenes/gone.scene")).unwrap();
    assert!(saved.contains("kind: \"VanishedScript\""), "{saved}");
    assert!(saved.contains("\"speed\": Float(2.5)") && saved.contains("\"label\": Str(\"hi\")"), "{saved}");
    assert!(saved.contains("Keeper 2"));
    let _ = std::fs::remove_dir_all(project);
}

/// Сквозной сценарий на настоящем крейте `games/template`: New Rust Script →
/// `cargo build -p template --bin template-editor --features editor` → запуск
/// собранной программы с сеансом; она восстанавливает сцену, забирает сеанс, и этот
/// редактор закрывается. Затем собранная программа ещё раз запускается с сеансом,
/// и в её логе видно, что новый компонент зарегистрирован и повешен на объект.
/// Новый процесс работает без окна и звука (`ENGINE_SCREENSHOT`, `ENGINE_AUDIO=off`).
///
/// Сборка игры — от нескольких секунд до минут. Запуск:
/// `cargo test -p engine --test editor -- --ignored --exact a_new_rust_script_is_built_and_the_editor_relaunches --nocapture`.
#[test]
#[ignore = "builds games/template with cargo: a minute or more"]
fn a_new_rust_script_is_built_and_the_editor_relaunches() {
    use engine::editor::{BuildStatus, Relaunch};
    use std::time::{Duration, Instant};
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let game = root.join("games/template");
    let scripts_dir = game.join("src/scripts");
    let script = scripts_dir.join("zz_relaunch_probe.rs");
    let original_mod = std::fs::read_to_string(scripts_dir.join("mod.rs")).unwrap();
    // Шаблон игры возвращается к исходному виду в любом случае.
    struct Restore(PathBuf, PathBuf, String);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::write(&self.1, &self.2);
        }
    }
    let _ = std::fs::remove_file(&script);
    let _restore = Restore(script.clone(), scripts_dir.join("mod.rs"), original_mod);

    let out = rust_temp_dir("relaunch");
    // Перезапущенный редактор: кадр в файл и выход, без окна и звука.
    std::env::set_var("ENGINE_SCREENSHOT", out.join("relaunched.png"));
    std::env::set_var("ENGINE_FRAMES", "2");
    std::env::set_var("ENGINE_AUDIO", "off");

    let mut editor = {
        let mut app = engine::app();
        app.spawn("Probe Target");
        let config = EditorConfig::default()
            .code_dir(game.join("src"))
            .rust_scripts(&scripts_dir)
            .relaunch(Relaunch::bin("template", "template-editor", &["editor"]).dir(&game));
        Editor::new(app, config).unwrap()
    };
    let target = find(&editor, "Probe Target");
    let started = Instant::now();
    editor.apply(Action::NewRustScript { name: "ZzRelaunchProbe".to_string(), attach_to: Some(target) });
    assert!(script.is_file());
    let mut built = None;
    while started.elapsed() < Duration::from_secs(900) && !editor.should_quit() {
        editor.advance(DT);
        if built.is_none() && matches!(editor.build_status(), BuildStatus::Succeeded { .. }) {
            built = Some(started.elapsed());
        }
        if let BuildStatus::Failed { command } = editor.build_status() {
            panic!("the build failed at `{command}`; see the log above");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let took_over = started.elapsed();
    assert!(editor.should_quit(), "the rebuilt editor picked up the session and this one closes");
    let shot = out.join("relaunched.png");
    for _ in 0..600 {
        if shot.is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(shot.is_file(), "the relaunched editor ran");
    eprintln!(
        "create → build {:.1} s → new editor took over at {:.1} s (relaunch {:.1} s)",
        built.unwrap_or_default().as_secs_f32(),
        took_over.as_secs_f32(),
        (took_over - built.unwrap_or_default()).as_secs_f32()
    );

    // Та же программа с сеансом, написанным вручную: в логе — новый тип и прикрепление.
    let exe = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(|deps| deps.parent())
        .unwrap()
        .join(format!("template-editor{}", std::env::consts::EXE_SUFFIX));
    let session = out.join("session.txt");
    std::fs::write(&session, "piska-editor-session 1\npending=ZzRelaunchProbe 0\nregistered=Camera\n").unwrap();
    let output = std::process::Command::new(&exe)
        .arg("--editor-session")
        .arg(&session)
        .output()
        .expect("the rebuilt editor starts");
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    assert!(log.contains("reloaded scripts:") && log.contains("+ZzRelaunchProbe"), "{log}");
    assert!(log.contains("script `ZzRelaunchProbe` is built and attached"), "{log}");
    assert!(!session.exists(), "the session was consumed");
    let _ = std::fs::remove_dir_all(out);
}
