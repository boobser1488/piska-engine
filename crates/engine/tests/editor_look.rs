//! Внешний вид редактора: галерея состояний для проверки оформления глазами (нужен GPU,
//! пишет PNG в `LOOK_SHOTS`) и проверки самой визуальной системы без GPU — шкала
//! плотности, контраст текста, нейтральность фона.
//!
//! `LOOK_SHOTS=out cargo test -p engine --test editor_look -- --ignored --test-threads=1`
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};

use engine::editor::egui;
use engine::editor::kit::theme;
use engine::editor::{capture_with_input, Action, BottomTab, Editor, EditorConfig, Panel};
use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SIZE: (u32, u32) = (1440, 860);
const DT: f32 = 1.0 / 60.0;

fn level1() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let config = EditorConfig::default()
        .open_scene("scenes/level1.scene")
        .game_crate("template")
        .workspace(&workspace);
    Editor::new(app, config).unwrap()
}

fn demo3d() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
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
    Editor::new(app, config).unwrap()
}

fn find(editor: &Editor, name: &str) -> GameObjectId {
    editor.app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("object `{name}` exists"))
}

/// Шаг сценария снимка.
#[derive(Clone, Copy)]
enum Step<'a> {
    /// Щёлкнуть по надписи.
    Click(&'a str),
    /// Щёлкнуть правой кнопкой по надписи.
    Right(&'a str),
    /// Навести на надпись (подменю раскрывается от наведения).
    Hover(&'a str),
}

/// «Пробный» контекст без GPU: та же раскладка, по нему ищутся надписи.
struct Probe {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Probe {
    fn new(editor: &mut Editor) -> Self {
        let mut probe = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
        for _ in 0..3 {
            probe.frame(editor, vec![]);
        }
        probe
    }

    fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.0 as f32, SIZE.1 as f32))),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn find(&self, text: &str) -> Option<egui::Pos2> {
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
        // Последнее нарисованное — сверху (всплывающее меню поверх панелей).
        found.last().copied()
    }
}

fn press(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
}

/// События одного шага: наведение, нажатие, отпускание, пустой кадр.
fn step_events(step: Step, pos: egui::Pos2) -> [Vec<egui::Event>; 4] {
    let button = match step {
        Step::Right(_) => Some(egui::PointerButton::Secondary),
        Step::Click(_) => Some(egui::PointerButton::Primary),
        Step::Hover(_) => None,
    };
    match button {
        Some(b) => [vec![egui::Event::PointerMoved(pos)], vec![press(pos, b, true)], vec![press(pos, b, false)], vec![]],
        None => [vec![egui::Event::PointerMoved(pos)], vec![], vec![], vec![]],
    }
}

/// Снять редактор: шаги сначала проигрываются на пробном контексте (так находятся
/// координаты надписей во всплывающих меню), потом — те же события в снимке.
fn shoot(editor: &mut Editor, dir: &Path, name: &str, steps: &[Step], frames: u32) {
    let mut probe = Probe::new(editor);
    let mut script: Vec<Vec<egui::Event>> = Vec::new();
    for step in steps {
        let text = match step {
            Step::Click(t) | Step::Right(t) | Step::Hover(t) => *t,
        };
        let Some(pos) = probe.find(text) else {
            eprintln!("[{name}] text `{text}` not found; the shot goes without this step");
            break;
        };
        for events in step_events(*step, pos) {
            probe.frame(editor, events.clone());
            script.push(events);
        }
    }
    // Пробный контекст мог поменять состояние редактора теми же щелчками (открыть
    // окно) — это и нужно; меню же живут в контексте и в снимке открываются заново.
    let start = 6u32;
    let total = frames.max(start + script.len() as u32 + 4);
    let image = capture_with_input(editor, SIZE.0, SIZE.1, total, |frame, _| {
        frame.checked_sub(start).and_then(|i| script.get(i as usize).cloned()).unwrap_or_default()
    })
    .unwrap();
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

/// Галерея всех состояний, по которой оценивается оформление.
#[test]
#[ignore = "needs a GPU; writes PNG files into LOOK_SHOTS"]
fn look_gallery() {
    let dir = PathBuf::from(std::env::var("LOOK_SHOTS").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("LOOK_ONLY").unwrap_or_default();
    let want = |name: &str| only.is_empty() || only.split(',').any(|n| name.starts_with(n));

    if want("01_3d") || want("04_code") || want("13_add_component") {
        let mut e = demo3d();
        e.apply(Action::Select(Some(find(&e, "Crystal"))));
        if want("01_3d") {
            shoot(&mut e, &dir, "01_3d", &[], 12);
        }
        if want("13_add_component") {
            e.apply(Action::CreateEmpty { parent: None });
            shoot(&mut e, &dir, "13_add_component", &[Step::Click("Add Component")], 12);
            e.apply(Action::Undo);
            e.apply(Action::Select(Some(find(&e, "Crystal"))));
        }
        if want("04_code") {
            let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
            e.apply(Action::OpenCode(PathBuf::from(format!("{root}/plugins/cpp-gameplay/gameplay.cpp"))));
            shoot(&mut e, &dir, "04_code", &[], 6);
        }
    }

    let mut e = level1();
    e.apply(Action::Select(Some(find(&e, "Player"))));
    if want("02_2d") {
        shoot(&mut e, &dir, "02_2d", &[], 8);
    }
    if want("05_inspector_long") {
        let player = find(&e, "Player");
        for kind in ["Camera", "AudioSource", "ParticleSystem", "Rigidbody2D", "BoxCollider2D"] {
            e.apply(Action::AddComponent { object: player, kind: kind.to_string() });
        }
        e.maximize_panel(Some(Panel::Inspector));
        shoot(&mut e, &dir, "05_inspector_long", &[], 8);
        e.maximize_panel(None);
        for _ in 0..5 {
            e.apply(Action::Undo);
        }
    }
    if want("06_project_tiles") {
        e.set_bottom_tab(BottomTab::Project);
        e.set_project_list_view(false);
        e.select_asset("sprites/player.png");
        shoot(&mut e, &dir, "06_project_tiles", &[], 24);
    }
    if want("07_project_list") {
        e.set_bottom_tab(BottomTab::Project);
        e.set_project_list_view(true);
        shoot(&mut e, &dir, "07_project_list", &[], 24);
        e.set_project_list_view(false);
    }
    if want("08_console") {
        e.apply(Action::AddComponent { object: find(&e, "Player"), kind: "Rigidbdy2D".to_string() });
        log::warn!("a warning from the game, to show how the Console looks");
        log::info!("level loaded in 12 ms");
        log::error!("Texture `sprites/missing.png` not found: check the path in the SpriteRenderer");
        e.set_bottom_tab(BottomTab::Console);
        shoot(&mut e, &dir, "08_console", &[], 8);
    }
    if want("10_preferences") {
        e.show_preferences(true);
        shoot(&mut e, &dir, "10_preferences", &[], 8);
        e.show_preferences(false);
    }
    if want("14_context_menu") {
        shoot(&mut e, &dir, "14_context_menu", &[Step::Right("Platform A")], 10);
    }
    if want("15_menu_gameobject") {
        shoot(&mut e, &dir, "15_menu_gameobject", &[Step::Click("GameObject"), Step::Hover("3D Object")], 10);
    }
    if want("16_menu_file") {
        shoot(&mut e, &dir, "16_menu_file", &[Step::Click("File")], 8);
    }
    if want("03_play") || want("09_profiler") {
        e.apply(Action::Play);
        e.app.world.input.set_key(Key::D, true);
        if want("03_play") {
            shoot(&mut e, &dir, "03_play", &[], 40);
        }
        if want("09_profiler") {
            e.set_bottom_tab(BottomTab::Profiler);
            shoot(&mut e, &dir, "09_profiler", &[], 90);
        }
        e.app.world.input.set_key(Key::D, false);
        e.apply(Action::Stop);
    }
    if want("17_scale150") {
        let mut prefs = e.preferences().clone();
        prefs.ui_scale = 1.5;
        e.set_preferences(prefs);
        let image = capture_with_input(&mut e, 1920, 1080, 8, |_, _| Vec::new()).unwrap();
        image.save(dir.join("17_scale150.png")).unwrap();
    }

    if want("11_project_settings") {
        let mut s = level1();
        s.show_project_settings(true);
        shoot(&mut s, &dir, "11_project_settings", &[], 10);
    }
    if want("12_build") {
        let mut b = level1();
        b.apply(Action::ShowBuildDialog(true));
        shoot(&mut b, &dir, "12_build", &[], 8);
    }
    if want("20_dialog") {
        let mut d = level1();
        d.apply(Action::CreateEmpty { parent: None });
        d.apply(Action::NewScene);
        shoot(&mut d, &dir, "20_dialog", &[], 8);
    }
    if want("18_material") {
        let mut m = level1();
        m.select_asset("materials/painted_metal.material");
        m.set_bottom_tab(BottomTab::Project);
        shoot(&mut m, &dir, "18_material", &[], 40);
    }
    if want("19_terrain") {
        let mut t = Editor::new(engine::app(), EditorConfig::titled("Terrain")).unwrap();
        let id = t.create_terrain();
        t.set_scene_3d(true);
        t.apply(Action::Select(Some(id)));
        t.set_terrain_tool(Some(BrushTool::Raise));
        shoot(&mut t, &dir, "19_terrain", &[], 8);
    }
}

/// Шкала плотности: строки и вкладки в пределах, о которых договорились в
/// `docs/design/ui-guidelines.md`; акцент по умолчанию — не «конфетно-синий».
#[test]
fn design_tokens_follow_the_guidelines() {
    assert!((20.0..=22.0).contains(&theme::ROW), "row height {}", theme::ROW);
    assert!((22.0..=24.0).contains(&theme::TAB_BAR), "tab strip {}", theme::TAB_BAR);
    assert!((11.5..=13.0).contains(&theme::FONT_BODY), "body text {}", theme::FONT_BODY);
    assert!(theme::R_S <= 3.0 && theme::R_M <= 4.0, "radii {} / {}", theme::R_S, theme::R_M);
    // Фон без синего оттенка: каналы отличаются не больше чем на 3.
    for color in [theme::BG_APP, theme::BG_HEADER, theme::BG_PANEL, theme::BG_RAISED, theme::BG_FIELD, theme::BG_INSET] {
        let (r, g, b) = (i32::from(color.r()), i32::from(color.g()), i32::from(color.b()));
        assert!((r - b).abs() <= 3 && (g - b).abs() <= 3, "background {color:?} is tinted");
    }
    // Уровни фона идут по возрастанию и близко друг к другу.
    let levels = [theme::BG_APP, theme::BG_HEADER, theme::BG_PANEL, theme::BG_RAISED];
    for pair in levels.windows(2) {
        let step = i32::from(pair[1].r()) - i32::from(pair[0].r());
        assert!((0..=12).contains(&step), "elevation step {step}");
    }
}

/// Текст читается на любом уровне фона: контраст основного текста ≥ 7:1, вторичного ≥ 4.5:1.
#[test]
fn text_contrast_is_readable_on_every_surface() {
    fn luminance(c: egui::Color32) -> f32 {
        let channel = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.039_28 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }
    for surface in [theme::BG_APP, theme::BG_HEADER, theme::BG_PANEL, theme::BG_RAISED, theme::BG_FIELD, theme::BG_INSET] {
        assert!(contrast(theme::TEXT, surface) >= 7.0, "text on {surface:?}");
        assert!(contrast(theme::TEXT_MUTED, surface) >= 4.5, "muted text on {surface:?}");
    }
    assert!(contrast(theme::TEXT, theme::selection_fill()) >= 4.5, "text on the selection");
}
