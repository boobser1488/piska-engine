//! Окна настроек редактора: Preferences и Project Settings. Интерфейс гоняется без окна и
//! без GPU через `egui::Context` с синтетическим вводом (кроме галереи снимков — она
//! помечена `#[ignore]`).
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use engine::editor::{egui, Action, Editor, EditorConfig, Preferences, SettingsScope};
use engine::prelude::*;
use engine_core::settings::TimeSettings;

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 860.0);
const DT: f32 = 1.0 / 60.0;
const PREFS: SettingsScope = SettingsScope::Preferences;
const PROJECT: SettingsScope = SettingsScope::Project;

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

    fn click_id(&mut self, editor: &mut Editor, id: egui::Id) {
        let pos = self.center(id);
        self.click(editor, pos);
    }

    fn type_text(&mut self, editor: &mut Editor, text: &str) {
        self.frame(editor, vec![egui::Event::Text(text.to_string())]);
        self.frames(editor, 2);
    }

    fn center(&self, id: egui::Id) -> egui::Pos2 {
        self.ctx.read_response(id).unwrap_or_else(|| panic!("the widget {id:?} is drawn in the last frame")).rect.center()
    }

    fn drawn(&self, id: egui::Id) -> bool {
        self.ctx.read_response(id).is_some()
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

// ----- временный каталог ассетов -----

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "piska_settings_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/scenes")).unwrap();
        std::fs::write(root.join("assets/scenes/menu.scene"), "x").unwrap();
        std::fs::write(root.join("assets/scenes/level1.scene"), "x").unwrap();
        std::fs::write(root.join("assets/icon.png"), "x").unwrap();
        Self { root }
    }

    fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    fn settings_file(&self) -> PathBuf {
        self.assets().join("project.settings")
    }

    fn saved(&self) -> ProjectSettings {
        ProjectSettings::load(self.settings_file()).expect("project.settings parses")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn editor_over(assets: &Path) -> Editor {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    let hero = app.world.scene.spawn("Hero");
    let _ = hero;
    Editor::new(app, EditorConfig::default()).expect("the editor starts")
}

// ----- Preferences -----

#[test]
fn preferences_has_appearance_and_game_view_pages_that_edit_the_preferences() {
    let pages: Vec<&str> = Editor::settings_pages(PREFS).into_iter().map(|(id, _)| id).collect();
    assert_eq!(pages, ["general", "appearance", "scene-view", "game-view", "console", "build-tools", "shortcuts"]);

    let sandbox = Sandbox::new("prefs");
    let mut editor = editor_over(&sandbox.assets());
    editor.show_preferences(true);
    let mut hands = Hands::new(&mut editor);
    assert!(editor.is_preferences_open());
    assert_eq!(editor.settings_page(PREFS), "general", "the window opens on the first page");
    hands.click_id(&mut editor, Editor::settings_page_id(PREFS, "appearance"));
    assert_eq!(editor.settings_page(PREFS), "appearance");
    assert!(hands.find_text(&mut editor, "UI scale").is_some());
    assert!(hands.find_text(&mut editor, "Accent colour").is_some());

    // Game View щелчком в списке слева; флажок VSync — щелчком по нему.
    hands.click_id(&mut editor, Editor::settings_page_id(PREFS, "game-view"));
    assert_eq!(editor.settings_page(PREFS), "game-view");
    // Ввод в Play (фокус окна Game) и звук игры в редакторе — тоже на этой странице.
    assert_eq!(
        editor.settings_visible_rows(PREFS),
        ["Vertical sync", "Play mode limit", "Focus Game view on Play", "Redraw on demand", "Mute audio in editor", "Editor volume"]
    );
    let row = editor.settings_row_rect(PREFS, "Vertical sync").unwrap();
    assert!(editor.preferences().vsync, "vertical sync is on by default");
    assert!(!hands.drawn(Editor::settings_reset_id(PREFS, "Vertical sync")), "no reset arrow at the default");
    // Флажок — первый контрол справа от колонки подписей: ищем его по строке.
    let mut toggled = false;
    for x in (row.left() as i32 + 140..row.left() as i32 + 260).step_by(6) {
        hands.click(&mut editor, egui::pos2(x as f32, row.top() + 13.0));
        if !editor.preferences().vsync {
            toggled = true;
            break;
        }
    }
    assert!(toggled, "clicking the checkbox turns VSync off");

    // Стрелка сброса появилась и возвращает значение по умолчанию.
    hands.click_id(&mut editor, Editor::settings_reset_id(PREFS, "Vertical sync"));
    assert!(editor.preferences().vsync);
    assert!(!hands.drawn(Editor::settings_reset_id(PREFS, "Vertical sync")));
}

#[test]
fn search_finds_a_setting_on_another_page_and_hides_the_rest() {
    let sandbox = Sandbox::new("search");
    let mut editor = editor_over(&sandbox.assets());
    editor.show_settings_page(PREFS, "appearance");
    let mut hands = Hands::new(&mut editor);
    assert_eq!(editor.settings_page(PREFS), "appearance");

    // Печатаем в поле поиска: «monitor» есть только в описании Vertical sync (Game View).
    hands.click_id(&mut editor, Editor::settings_search_id(PREFS));
    hands.type_text(&mut editor, "monitor");
    assert_eq!(editor.settings_search_hits(PREFS), [("game-view", 1)]);
    assert_eq!(editor.settings_page(PREFS), "game-view", "the window jumps to the page with the match");
    assert_eq!(editor.settings_visible_rows(PREFS), ["Vertical sync"]);
    assert!(hands.find_text(&mut editor, "UI scale").is_none(), "rows that do not match are hidden");
    assert!(hands.find_text(&mut editor, "Appearance").is_none(), "pages without a match leave the list");

    // Project Settings: поиск по всем страницам сразу.
    editor.show_preferences(false);
    editor.show_project_settings(true);
    editor.set_settings_search(PROJECT, "gravity");
    hands.frames(&mut editor, 2);
    let pages: Vec<&str> = editor.settings_search_hits(PROJECT).into_iter().map(|(p, _)| p).collect();
    assert_eq!(pages, ["physics2d", "physics3d"]);
    editor.set_settings_search(PROJECT, "no such setting at all");
    hands.frames(&mut editor, 2);
    assert!(editor.settings_search_hits(PROJECT).is_empty());
    assert!(hands.find_text(&mut editor, "No settings match “no such setting at all”").is_some());
}

#[test]
fn reset_all_preferences_asks_first_and_keeps_window_layouts() {
    let sandbox = Sandbox::new("prefsreset");
    let mut editor = editor_over(&sandbox.assets());
    let mut prefs = editor.preferences().clone();
    prefs.ui_scale = 1.5;
    prefs.vsync = false; // не по умолчанию: сброс должен вернуть «включено»
    prefs.play_fps_cap = 60;
    prefs.accent = 2;
    prefs.saved_layouts = vec![("Mine".to_string(), "piska-layout 1; main -; maxplay 0".to_string())];
    editor.set_preferences(prefs);
    editor.show_preferences(true);
    let mut hands = Hands::new(&mut editor);

    // Cancel — ничего не меняется.
    hands.click_id(&mut editor, Editor::settings_reset_all_id(PREFS));
    assert!(hands.find_text(&mut editor, "Reset all preferences?").is_some());
    hands.click_id(&mut editor, Editor::settings_cancel_id());
    assert_eq!(editor.preferences().play_fps_cap, 60);

    hands.click_id(&mut editor, Editor::settings_reset_all_id(PREFS));
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    let defaults = Preferences::default();
    let now = editor.preferences();
    assert_eq!(
        (now.ui_scale, now.vsync, now.play_fps_cap, now.accent, now.redraw_on_demand),
        (defaults.ui_scale, defaults.vsync, defaults.play_fps_cap, defaults.accent, defaults.redraw_on_demand)
    );
    assert_eq!(now.saved_layouts.len(), 1, "layouts are not settings of these pages");
}

// ----- Project Settings -----

#[test]
fn project_settings_pages_are_registered_in_order() {
    let pages: Vec<&str> = Editor::settings_pages(PROJECT).into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        pages,
        ["player", "build", "physics2d", "physics3d", "tags-and-layers", "input", "time", "graphics", "audio", "script-order"]
    );
}

#[test]
fn a_changed_value_is_applied_written_and_reloads_equal() {
    let sandbox = Sandbox::new("write");
    let mut editor = editor_over(&sandbox.assets());
    assert!(!sandbox.settings_file().exists());
    editor
        .edit_project_settings(|s| {
            s.time.fixed_timestep = 0.02;
            s.player.title = "Lantern".to_string();
            s.graphics = GraphicsSettings::preset(QualityLevel::Low);
        })
        .expect("settings are saved");
    // Сразу действует в открытом мире…
    assert!((editor.app.world.time.fixed_delta - 0.02).abs() < 1e-6);
    editor.edit_project_settings(|s| s.physics2d.gravity = [0.0, -3.0]).unwrap();
    assert_eq!(editor.app.world.resources.get::<Physics2D>().unwrap().gravity, Vec2::new(0.0, -3.0), "gravity reaches the physics");
    assert_eq!(editor.app.world.resources.get::<GraphicsSettings>().unwrap().quality, QualityLevel::Low);
    // …и лежит в файле.
    let saved = sandbox.saved();
    assert_eq!(saved, editor.project_settings_draft());
    assert_eq!(saved, editor.app.world.project_settings());
    assert_eq!(saved.player.title, "Lantern");

    // Правка в окне: клетка матрицы слоёв, затем стрелка сброса строки.
    editor.show_project_settings_page("physics2d");
    let mut hands = Hands::new(&mut editor);
    hands.click_id(&mut editor, Editor::layer_matrix_cell_id(2, 0, 4));
    let matrix = &editor.project_settings_draft().physics2d.layer_matrix;
    assert!(!matrix.collides(0, 4) && !matrix.collides(4, 0), "the matrix stays symmetric");
    assert!(!sandbox.saved().physics2d.layer_matrix.collides(4, 0), "the click is written to the file");
    assert!(!editor.app.world.project_settings().physics2d.layer_matrix.collides(0, 4), "and applied to the world");
    let physics = editor.app.world.resources.get::<Physics2D>().expect("engine::app() inserts Physics2D");
    assert!(!physics.layer_matrix().collides(4, 0), "the physics hook picked the matrix up");
    hands.click_id(&mut editor, Editor::settings_reset_id(PROJECT, "Layer collision matrix"));
    assert!(sandbox.saved().physics2d.layer_matrix.is_default());
    assert_eq!(sandbox.saved(), editor.project_settings_draft());
}

#[test]
fn reset_page_asks_and_restores_the_page_defaults_only() {
    let sandbox = Sandbox::new("resetpage");
    let mut editor = editor_over(&sandbox.assets());
    editor
        .edit_project_settings(|s| {
            s.time.time_scale = 0.5;
            s.time.fixed_timestep = 0.01;
            s.audio.music_volume = 0.25;
        })
        .unwrap();
    editor.show_project_settings_page("time");
    let mut hands = Hands::new(&mut editor);
    assert!(hands.drawn(Editor::settings_reset_id(PROJECT, "Time scale")), "modified rows show the reset arrow");

    // Стрелка одной строки.
    hands.click_id(&mut editor, Editor::settings_reset_id(PROJECT, "Time scale"));
    assert_eq!(sandbox.saved().time.time_scale, 1.0);
    assert_eq!(sandbox.saved().time.fixed_timestep, 0.01);

    // Reset page с подтверждением: только Time.
    hands.click_id(&mut editor, Editor::settings_reset_page_id(PROJECT));
    assert!(hands.find_text(&mut editor, "Reset Time?").is_some());
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    let saved = sandbox.saved();
    assert_eq!(saved.time, TimeSettings::default());
    assert_eq!(saved.audio.music_volume, 0.25, "other pages are untouched");

    // Reset all — всё.
    hands.click_id(&mut editor, Editor::settings_reset_all_id(PROJECT));
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    assert_eq!(sandbox.saved(), ProjectSettings::default());
    assert_eq!(editor.app.world.project_settings(), ProjectSettings::default());
}

#[test]
fn graphics_quality_buttons_fill_the_fields() {
    let sandbox = Sandbox::new("quality");
    let mut editor = editor_over(&sandbox.assets());
    editor.show_project_settings_page("graphics");
    let mut hands = Hands::new(&mut editor);
    let low = hands.find_text(&mut editor, "Low").expect("the Low button is drawn");
    hands.click(&mut editor, low);
    assert_eq!(editor.project_settings_draft().graphics, GraphicsSettings::preset(QualityLevel::Low));
    assert_eq!(sandbox.saved().graphics, GraphicsSettings::preset(QualityLevel::Low));
    assert!(!editor.app.world.resources.get::<GraphicsSettings>().unwrap().bloom, "the renderer resource follows");
    let medium = hands.find_text(&mut editor, "Medium").unwrap();
    hands.click(&mut editor, medium);
    assert_eq!(sandbox.saved().graphics, GraphicsSettings::preset(QualityLevel::Medium));
}

#[test]
fn player_page_offers_png_icons_and_scenes_from_the_assets() {
    let sandbox = Sandbox::new("player");
    let mut editor = editor_over(&sandbox.assets());
    editor.show_project_settings_page("player");
    // Строка внизу страницы: поиск оставляет только её.
    editor.set_settings_search(PROJECT, "default scene");
    let mut hands = Hands::new(&mut editor);
    assert_eq!(editor.settings_visible_rows(PROJECT), ["Default scene"]);
    let default_scene = hands.find_text(&mut editor, "None").expect("the default scene list shows None");
    hands.click(&mut editor, default_scene);
    let level = hands.find_text(&mut editor, "scenes/level1.scene").expect("scenes from the assets are offered");
    assert!(hands.find_text(&mut editor, "scenes/menu.scene").is_some());
    hands.click(&mut editor, level);
    assert_eq!(sandbox.saved().player.default_scene, "scenes/level1.scene");
    editor.set_settings_search(PROJECT, "");
    let icon = hands.find_text(&mut editor, "None (icon.png)").unwrap();
    hands.click(&mut editor, icon);
    assert!(hands.find_text(&mut editor, "icon.png").is_some(), "png files are offered as icons");
}

#[test]
fn tags_added_in_project_settings_feed_the_inspector_dropdown() {
    let sandbox = Sandbox::new("tags");
    let mut editor = editor_over(&sandbox.assets());
    editor.show_project_settings_page("tags-and-layers");
    let mut hands = Hands::new(&mut editor);

    // Новый тег — вводом в поле и кнопкой Add Tag.
    hands.click_id(&mut editor, Editor::new_tag_field_id());
    hands.type_text(&mut editor, "Enemy");
    let add = hands.find_text(&mut editor, "Add Tag").expect("the Add Tag button");
    hands.click(&mut editor, add);
    assert_eq!(editor.project_tags(), ["Enemy"]);
    assert_eq!(sandbox.saved().tags_and_layers.tags, ["Enemy"]);

    // Имя слоя 8.
    editor.edit_project_settings(|s| s.tags_and_layers.layers[8] = "Enemies".to_string()).unwrap();

    // Inspector: Tag — выпадающий список с тегами проекта.
    editor.show_project_settings(false);
    let hero = editor.app.world.scene.find_by_name("Hero").unwrap();
    editor.apply(Action::Select(Some(hero)));
    hands.frames(&mut editor, 3);
    let tag = hands.find_text(&mut editor, "Untagged").expect("the Inspector shows the Tag dropdown");
    hands.click(&mut editor, tag);
    let enemy = hands.find_text(&mut editor, "Enemy").expect("the project tag is in the dropdown");
    hands.click(&mut editor, enemy);
    assert_eq!(editor.app.world.scene.get(hero).unwrap().tag, "Enemy");

    // Layer — имена слоёв.
    let layer = hands.find_text(&mut editor, "Default").expect("the Inspector shows the layer name");
    hands.click(&mut editor, layer);
    let enemies = hands.find_text(&mut editor, "8: Enemies").expect("named layers are in the dropdown");
    hands.click(&mut editor, enemies);
    assert_eq!(editor.app.world.scene.get(hero).unwrap().layer, 8);

    // «Add Tag…» открывает эту страницу.
    let tag = hands.find_text(&mut editor, "Enemy").unwrap();
    hands.click(&mut editor, tag);
    let more = hands.find_text(&mut editor, "Add Tag…").expect("the dropdown ends with Add Tag…");
    hands.click(&mut editor, more);
    assert!(editor.is_project_settings_open());
    assert_eq!(editor.settings_page(PROJECT), "tags-and-layers");

    // Имя слоя видно в матрице Physics 2D.
    editor.show_project_settings_page("physics2d");
    hands.frames(&mut editor, 2);
    assert!(hands.find_text(&mut editor, "Enemies").is_some(), "the matrix lists named layers");
    hands.click_id(&mut editor, Editor::layer_matrix_cell_id(2, 8, 0));
    let matrix = editor.app.world.project_settings().physics2d.layer_matrix;
    assert!(!matrix.collides(8, 0) && !matrix.collides(0, 8));
    assert!(matrix.collides(8, 8));
    let physics = editor.app.world.resources.get::<Physics2D>().unwrap();
    assert!(!physics.layer_matrix().collides(0, 8) && physics.layer_matrix().collides(8, 8));
}

#[test]
fn removing_a_used_tag_asks_and_objects_keep_their_tag_text() {
    let sandbox = Sandbox::new("removetag");
    let mut editor = editor_over(&sandbox.assets());
    editor.edit_project_settings(|s| {
        s.tags_and_layers.add_tag("Enemy");
        s.tags_and_layers.add_tag("Pickup");
    })
    .unwrap();
    let hero = editor.app.world.scene.find_by_name("Hero").unwrap();
    editor.app.world.scene.get_mut(hero).unwrap().tag = "Enemy".to_string();
    editor.show_project_settings_page("tags-and-layers");
    let mut hands = Hands::new(&mut editor);
    // Неиспользуемый — сразу.
    hands.click_id(&mut editor, Editor::remove_tag_button_id("Pickup"));
    assert_eq!(editor.project_tags(), ["Enemy"]);
    // Используемый — с вопросом.
    hands.click_id(&mut editor, Editor::remove_tag_button_id("Enemy"));
    assert!(hands.find_text(&mut editor, "Remove tag “Enemy”?").is_some());
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    assert!(editor.project_tags().is_empty());
    assert!(sandbox.saved().tags_and_layers.tags.is_empty());
    assert_eq!(editor.app.world.scene.get(hero).unwrap().tag, "Enemy", "the scene keeps what it stores");
}

#[test]
fn a_restarted_editor_shows_the_saved_values_and_old_files_still_load() {
    let sandbox = Sandbox::new("restart");
    // Старый файл: только порядок скриптов.
    std::fs::write(sandbox.settings_file(), "(\n    script_order: {\n        \"Mover\": -10,\n    },\n)\n").unwrap();
    let mut editor = editor_over(&sandbox.assets());
    let loaded = editor.project_settings_draft();
    assert_eq!(loaded.order_of("Mover"), Some(-10));
    assert_eq!(loaded.time, TimeSettings::default());

    editor
        .edit_project_settings(|s| {
            s.player.company = "Piska Games".to_string();
            s.physics3d.gravity = [0.0, -3.0, 0.0];
        })
        .unwrap();
    drop(editor);

    let mut next = editor_over(&sandbox.assets());
    let settings = next.project_settings_draft();
    assert_eq!(settings.player.company, "Piska Games");
    assert_eq!(settings.physics3d.gravity, [0.0, -3.0, 0.0]);
    assert_eq!(settings.order_of("Mover"), Some(-10), "the script order survives edits of other pages");
    next.show_project_settings_page("player");
    let mut hands = Hands::new(&mut next);
    assert!(hands.find_text(&mut next, "Piska Games").is_some(), "the window shows the saved value");
}

#[test]
fn settings_are_exported_and_imported_through_files() {
    let sandbox = Sandbox::new("io");
    let mut editor = editor_over(&sandbox.assets());
    editor.edit_project_settings(|s| s.audio.master_volume = 0.3).unwrap();
    let file = editor.export_settings_file(PROJECT, "exported/project.ron").expect("export writes the file");
    assert_eq!(file, sandbox.root.join("exported/project.ron"), "relative paths start in the project folder");
    editor.reset_settings(PROJECT, None).unwrap();
    assert_eq!(sandbox.saved().audio.master_volume, 1.0);
    editor.import_settings_file(PROJECT, "exported/project.ron").unwrap();
    assert_eq!(sandbox.saved().audio.master_volume, 0.3);
    assert!(editor.import_settings_file(PROJECT, "missing.ron").is_err());

    // Через окно: Export… → путь → Export.
    editor.show_project_settings_page("audio");
    let mut hands = Hands::new(&mut editor);
    hands.click_id(&mut editor, Editor::settings_export_id(PROJECT));
    assert!(hands.find_text(&mut editor, "Export Project Settings").is_some());
    hands.click_id(&mut editor, Editor::settings_confirm_id());
    let exported = ProjectSettings::load(sandbox.root.join("project-settings.ron")).unwrap();
    assert_eq!(exported.audio.master_volume, 0.3);

    // Preferences — свой формат.
    let mut prefs = editor.preferences().clone();
    prefs.play_fps_cap = 144;
    editor.set_preferences(prefs);
    editor.export_settings_file(PREFS, "prefs.ron").unwrap();
    editor.reset_settings(PREFS, None).unwrap();
    assert_eq!(editor.preferences().play_fps_cap, 0);
    editor.import_settings_file(PREFS, "prefs.ron").unwrap();
    assert_eq!(editor.preferences().play_fps_cap, 144);
}

#[test]
fn script_order_keeps_apply_while_other_pages_write_at_once() {
    let sandbox = Sandbox::new("order");
    let mut editor = editor_over(&sandbox.assets());
    editor.set_script_order("Mover", -5);
    assert!(editor.script_order_is_dirty());
    // Другая страница пишет файл — без неприменённого порядка.
    editor.edit_project_settings(|s| s.time.time_scale = 2.0).unwrap();
    assert!(sandbox.saved().script_order.is_empty());
    assert!(editor.script_order_is_dirty(), "the pending order is still pending");
    editor.apply_script_order().unwrap();
    let saved = sandbox.saved();
    assert_eq!(saved.order_of("Mover"), Some(-5));
    assert_eq!(saved.time.time_scale, 2.0);
}

// ----- галерея снимков (нужен GPU) -----

/// `ENGINE_SHOTS_DIR=out cargo test -p engine --features editor --test editor_settings -- --ignored gallery`
#[test]
#[ignore = "needs a GPU; writes PNG files into ENGINE_SHOTS_DIR"]
fn settings_screenshot_gallery() {
    use engine::editor::{capture, capture_with_input};
    let dir = PathBuf::from(std::env::var("ENGINE_SHOTS_DIR").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let sandbox = Sandbox::new("gallery");
    let mut editor = editor_over(&sandbox.assets());
    editor
        .edit_project_settings(|s| {
            s.player.title = "Lantern".to_string();
            s.player.company = "Piska Games".to_string();
            s.player.width = 1600;
            s.tags_and_layers.add_tag("Player");
            s.tags_and_layers.add_tag("Enemy");
            s.tags_and_layers.add_tag("Pickup");
            s.tags_and_layers.layers[8] = "Enemies".to_string();
            s.tags_and_layers.layers[9] = "Projectiles".to_string();
            s.physics2d.layer_matrix.set(8, 9, false);
            s.physics2d.layer_matrix.set(9, 9, false);
            s.physics2d.gravity = [0.0, -20.0];
            s.time.fixed_timestep = 0.02;
            s.audio.music_volume = 0.6;
            s.set_order("Mover", -10);
        })
        .unwrap();
    let mut prefs = editor.preferences().clone();
    prefs.play_fps_cap = 144;
    editor.set_preferences(prefs);

    for (id, _) in Editor::settings_pages(PREFS) {
        editor.show_settings_page(PREFS, id);
        capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join(format!("prefs_{id}.png"))).unwrap();
    }
    editor.show_preferences(false);
    for (id, _) in Editor::settings_pages(PROJECT) {
        editor.show_settings_page(PROJECT, id);
        capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join(format!("project_{id}.png"))).unwrap();
    }
    // Поиск с подсветкой.
    editor.set_settings_search(PROJECT, "sleep");
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("project_search.png")).unwrap();
    // Матрица слоёв целиком.
    editor.show_settings_page(PROJECT, "physics2d");
    editor.set_settings_search(PROJECT, "collision matrix");
    capture(&mut editor, 1440, 860, 10).unwrap().save(dir.join("project_layer_matrix.png")).unwrap();
    editor.set_settings_search(PROJECT, "");
    // Подтверждение Reset all.
    editor.show_settings_page(PROJECT, "time");
    let hands = Hands::new(&mut editor);
    let reset = hands.center(Editor::settings_reset_all_id(PROJECT));
    let image = capture_with_input(&mut editor, 1440, 860, 16, |frame, _| match frame {
        6 => vec![egui::Event::PointerMoved(reset)],
        8 => vec![Hands::button(reset, true)],
        9 => vec![Hands::button(reset, false)],
        _ => vec![],
    })
    .unwrap();
    image.save(dir.join("project_reset_confirm.png")).unwrap();

    // Inspector: выпадающий список Tag с тегами проекта.
    let mut fresh = editor_over(&sandbox.assets());
    let hero = fresh.app.world.scene.find_by_name("Hero").unwrap();
    fresh.apply(Action::Select(Some(hero)));
    let mut hands = Hands::new(&mut fresh);
    let tag = hands.find_text(&mut fresh, "Untagged").expect("the Tag dropdown");
    let image = capture_with_input(&mut fresh, 1440, 860, 16, |frame, _| match frame {
        6 => vec![egui::Event::PointerMoved(tag)],
        8 => vec![Hands::button(tag, true)],
        9 => vec![Hands::button(tag, false)],
        _ => vec![],
    })
    .unwrap();
    image.save(dir.join("inspector_tag_dropdown.png")).unwrap();
}
