//! Русский язык редактора: каталог совпадает с кодом, язык переключается на лету, на
//! русском экране не остаётся английских надписей, надписи не вылезают из окон.
//!
//! * без GPU: проверка каталога (`tools/extract_ui_strings.py --check`), переключение
//!   языка, поиск английских «протечек» по всем окнам и меню (`L10N_REPORT=файл` — список);
//! * с GPU (`--ignored`): галерея PNG на английском и русском в `L10N_SHOTS`.
//!
//! `L10N_SHOTS=out cargo test -p engine --test editor_l10n -- --ignored --test-threads=1`
#![cfg(feature = "editor")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use engine::editor::egui;
use engine::editor::kit::i18n::{self, tr, Language};
use engine::editor::{capture_with_input, Action, BottomTab, Editor, EditorConfig, Panel, SettingsScope};
use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SIZE: (u32, u32) = (1440, 860);
const DT: f32 = 1.0 / 60.0;

/// Язык процесса общий: тесты этого файла идут по очереди и возвращают английский.
static LANGUAGE: Mutex<()> = Mutex::new(());

struct InLanguage(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for InLanguage {
    fn drop(&mut self) {
        i18n::set(Language::English);
    }
}

fn in_language(language: Language) -> InLanguage {
    let guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    i18n::set(language);
    InLanguage(guard)
}

fn level1() -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let config = EditorConfig::default().open_scene("scenes/level1.scene").game_crate("template").workspace(&workspace);
    Editor::new(app, config).unwrap()
}

fn find(editor: &Editor, name: &str) -> GameObjectId {
    editor.app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("object `{name}` exists"))
}

// ---------------------------------------------------------------------------
// Пробный контекст: кадры без GPU, поиск и сбор надписей
// ---------------------------------------------------------------------------

struct Probe {
    ctx: egui::Context,
    time: f64,
    size: egui::Vec2,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Probe {
    fn new(editor: &mut Editor, size: egui::Vec2) -> Self {
        let mut probe = Self { ctx: egui::Context::default(), time: 0.0, size, shapes: Vec::new() };
        for _ in 0..4 {
            probe.frame(editor, vec![]);
        }
        probe
    }

    fn frame(&mut self, editor: &mut Editor, events: Vec<egui::Event>) {
        let zoom = editor.preferences().ui_scale;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size / zoom)),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..egui::RawInput::default()
        };
        self.time += DT as f64;
        self.shapes = self.ctx.run(input, |ctx| editor.ui(ctx)).shapes;
    }

    fn settle(&mut self, editor: &mut Editor, frames: usize) {
        for _ in 0..frames {
            self.frame(editor, vec![]);
        }
    }

    /// Надписи кадра: текст и прямоугольник на экране.
    fn texts(&self) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(String, egui::Rect, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()), clip)),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, clip, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &self.shapes {
            walk(&clipped.shape, clipped.clip_rect, &mut out);
        }
        out.into_iter().map(|(text, rect, _)| (text, rect)).collect()
    }

    fn find(&self, text: &str) -> Option<egui::Pos2> {
        self.texts().into_iter().rev().find(|(t, _)| t == text).map(|(_, rect)| rect.center())
    }

    /// Шаг сценария: щёлкнуть / навести / щёлкнуть правой по надписи. `false` — надписи нет.
    fn step(&mut self, editor: &mut Editor, step: Step) -> bool {
        let (Step::Click(text) | Step::Right(text) | Step::Hover(text)) = step;
        let Some(pos) = self.find(text) else {
            return false;
        };
        let button = match step {
            Step::Right(_) => Some(egui::PointerButton::Secondary),
            Step::Click(_) => Some(egui::PointerButton::Primary),
            Step::Hover(_) => None,
        };
        self.frame(editor, vec![egui::Event::PointerMoved(pos)]);
        if let Some(button) = button {
            self.frame(editor, vec![press(pos, button, true)]);
            self.frame(editor, vec![press(pos, button, false)]);
        }
        self.settle(editor, 3);
        true
    }
}

#[derive(Clone, Copy)]
enum Step<'a> {
    Click(&'a str),
    Right(&'a str),
    Hover(&'a str),
}

fn press(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
}

// ---------------------------------------------------------------------------
// Каталог и переключение
// ---------------------------------------------------------------------------

/// Каждая строка кода есть в `ru.po`, в каталоге нет лишнего, подстановки совпадают.
#[test]
fn the_catalogue_matches_the_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = root.join("tools/extract_ui_strings.py");
    if !script.is_file() {
        eprintln!("no tools/extract_ui_strings.py next to the crate: skipped");
        return;
    }
    let Ok(output) = std::process::Command::new("python").arg(&script).arg("--check").current_dir(&root).output() else {
        eprintln!("python is not available: skipped");
        return;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "tools/extract_ui_strings.py --check found problems:\n{text}");
}

#[test]
fn the_language_switches_at_runtime_and_back() {
    let _lang = in_language(Language::English);
    let mut editor = level1();
    let mut probe = Probe::new(&mut editor, egui::vec2(1440.0, 860.0));
    let english: BTreeSet<String> = probe.texts().into_iter().map(|(t, _)| t).collect();
    for word in ["Hierarchy", "Inspector", "Project", "Console"] {
        assert!(english.contains(word), "`{word}` is on the English screen: {english:?}");
    }
    // Тот же редактор, без перезапуска.
    i18n::set(Language::Russian);
    probe.settle(&mut editor, 3);
    let russian: BTreeSet<String> = probe.texts().into_iter().map(|(t, _)| t).collect();
    for word in ["Иерархия", "Инспектор", "Проект", "Консоль"] {
        assert!(russian.contains(word), "`{word}` is on the Russian screen: {russian:?}");
    }
    for word in ["Hierarchy", "Inspector", "Console"] {
        assert!(!russian.contains(word), "`{word}` is not on the Russian screen");
    }
    i18n::set(Language::English);
    probe.settle(&mut editor, 3);
    let back: BTreeSet<String> = probe.texts().into_iter().map(|(t, _)| t).collect();
    assert!(back.contains("Hierarchy") && !back.contains("Иерархия"));
}

/// Предпочтение «Language» в Preferences действует на весь процесс и сохраняется.
#[test]
fn the_language_preference_applies_and_round_trips() {
    use engine::editor::Preferences;
    let _lang = in_language(Language::English);
    let prefs = Preferences { language: Some(Language::Russian), ..Preferences::default() };
    prefs.apply_language();
    assert_eq!(i18n::current(), Language::Russian);
    assert_eq!(Preferences::parse(&prefs.to_text()).language, Some(Language::Russian));
    let auto = Preferences { language: None, ..Preferences::default() };
    assert_eq!(Preferences::parse(&auto.to_text()).language, None);
    assert_eq!(auto.resolved_language(), Language::system());
    Preferences { language: Some(Language::English), ..Preferences::default() }.apply_language();
    assert_eq!(i18n::current(), Language::English);
}

// ---------------------------------------------------------------------------
// Английские «протечки» на русском экране
// ---------------------------------------------------------------------------

/// Латинская надпись, которую переводить не надо: имена типов, форматы, единицы, бренды.
fn is_allowed_latin(text: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Piska", "Rust", "C++", "RON", "Scenario", "Windows", "Android", "Linux", "glTF", "Cargo", "Cargo.toml", "FPS", "UI", "GPU",
        "Rigidbody", "Rigidbody2D", "Collider", "Transform", "Player", "Camera", "Text", "Image", "OK", "ID", "GUID", "SDK", "NDK",
        "JDK", "CMake", "WebGPU", "WebGL2", "HDR", "UV", "PNG", "OBJ", "GLB", "RGB", "RGBA", "Debug", "Release", "Web", "Alt", "Ctrl",
        "Shift", "Del", "Esc", "Tab", "Enter", "Space", "Left", "Right", "Up", "Down", "Mono", "px", "ms", "min", "None",
    ];
    let words: Vec<&str> = text.split(|c: char| !c.is_ascii_alphanumeric() && c != '+').filter(|w| !w.is_empty()).collect();
    words.iter().all(|w| ALLOWED.contains(w) || w.len() < 3 || w.chars().all(|c| c.is_ascii_digit()))
}

/// Данные, а не интерфейс: имена полей и типов компонентов (из кода игры), имена объектов
/// сцены, слои, действия ввода, пути, сочетания клавиш, строки журнала.
fn data_names(editor: &Editor) -> BTreeSet<String> {
    use engine::editor::kit::theme::{pretty_name, pretty_type};
    let mut data = BTreeSet::new();
    for kind in editor.app.world.registry.names() {
        data.insert(pretty_type(kind));
        data.insert(kind.to_string());
        if let Ok(component) = editor.app.world.registry.create(kind) {
            for field in component.field_names() {
                data.insert(pretty_name(field));
            }
        }
    }
    for id in editor.app.world.scene.ids() {
        if let Some(object) = editor.app.world.scene.get(id) {
            data.insert(object.name.clone());
        }
    }
    // Папки и файлы ассетов.
    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            out.insert(entry.file_name().to_string_lossy().to_string());
            if let Some(stem) = path.file_stem() {
                out.insert(stem.to_string_lossy().to_string());
            }
            if path.is_dir() {
                walk(&path, out);
            }
        }
    }
    walk(Path::new(ASSETS), &mut data);
    for name in [
        "cargo", "dist", "zig", "template", "Box", "Circle", "Dynamic", "Static", "Kinematic", "Procedural", "Untitled", "Default", "TransparentFX", "Ignore Raycast", "Water", "UI", "Untagged", "Fire1", "Jump", "Horizontal", "Vertical", "Submit",
        "Cancel", "D-Pad Left", "D-Pad Right", "GameObject", "Terrain", "Level 1", "Java (JDK)", "Main Camera",
    ] {
        data.insert(name.to_string());
    }
    data
}

fn looks_like_data(text: &str, data: &BTreeSet<String>) -> bool {
    let text = text.trim().trim_end_matches('*');
    data.contains(text)
        || text.split(", ").all(|part| data.contains(part))
        || text.starts_with(['#', '+', '-'])
        || text.contains(['/', '\\', '{'])
        || text.starts_with("Ctrl")
        || text.starts_with("Shift")
        || text.starts_with("Alt")
        || ["stop:", "play:", "a warning", "editor_l10n"].iter().any(|p| text.starts_with(p))
        || text.rsplit('.').next().is_some_and(|ext| text.contains('.') && ext.len() <= 8 && !ext.contains(' '))
        || text.ends_with(" fps")
        || (text.contains('…') && !text.contains(' '))
        || text.chars().all(|c| !c.is_alphabetic() || c.is_ascii_uppercase())
}

fn leaks(texts: &[(String, egui::Rect)], data: &BTreeSet<String>) -> BTreeSet<String> {
    texts
        .iter()
        .map(|(t, _)| t.trim().to_string())
        .filter(|t| !t.is_empty() && !t.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c)))
        .filter(|t| t.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 3)
        .filter(|t| !is_allowed_latin(t) && !looks_like_data(t, data))
        .collect()
}

fn collect(found: &mut BTreeSet<String>, data: &BTreeSet<String>, probe: &Probe, label: &str) {
    for leak in leaks(&probe.texts(), data) {
        found.insert(format!("{leak}   [{label}]"));
    }
}

/// Пройти по окнам, меню и диалогам на русском; вернуть латинские надписи без перевода.
fn russian_leaks() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let size = egui::vec2(1440.0, 860.0);
    let data = data_names(&level1());
    let mut e = level1();
    e.apply(Action::Select(Some(find(&e, "Player"))));
    let mut p = Probe::new(&mut e, size);
    p.settle(&mut e, 8);
    collect(&mut found, &data, &p, "main");

    // Меню главной панели: каждое открывается щелчком, вложенные — наведением.
    let menus = ["File", "Edit", "GameObject", "View", "Assets", "Tools", "Window", "Help"];
    for menu in menus {
        let mut e = level1();
        let mut p = Probe::new(&mut e, size);
        let title = match i18n::tr_ctx("menu", menu) {
            t if t == menu => tr(menu).to_string(),
            t => t.to_string(),
        };
        if p.step(&mut e, Step::Click(&title)) {
            collect(&mut found, &data, &p, &format!("menu {menu}"));
        } else {
            found.insert(format!("(menu `{menu}` is not on the screen)"));
        }
    }
    {
        let mut e = level1();
        let mut p = Probe::new(&mut e, size);
        p.step(&mut e, Step::Click(&tr("GameObject")));
        for sub in ["3D Object", "2D", "Audio", "UI", "Effects", "Physics", "Animation", "Light"] {
            if p.step(&mut e, Step::Hover(&tr(sub))) {
                collect(&mut found, &data, &p, &format!("GameObject → {sub}"));
            }
        }
    }
    {
        let mut e = level1();
        let mut p = Probe::new(&mut e, size);
        p.step(&mut e, Step::Click(&tr("Window")));
        for sub in ["Layouts"] {
            if p.step(&mut e, Step::Hover(&tr(sub))) {
                collect(&mut found, &data, &p, "Window → Layouts");
            }
        }
    }
    // Контекстные меню иерархии и окна Project.
    {
        let mut e = level1();
        let mut p = Probe::new(&mut e, size);
        if p.step(&mut e, Step::Right("Platform A")) {
            collect(&mut found, &data, &p, "hierarchy context menu");
        }
    }
    {
        let mut e = level1();
        e.set_bottom_tab(BottomTab::Project);
        e.select_asset("sprites/player.png");
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 20);
        collect(&mut found, &data, &p, "project");
        if p.step(&mut e, Step::Right("player.png")) {
            collect(&mut found, &data, &p, "project asset context menu");
        }
    }
    // Add Component, Консоль, Профайлер, Game.
    {
        let mut e = level1();
        e.apply(Action::Select(Some(find(&e, "Player"))));
        let mut p = Probe::new(&mut e, size);
        if p.step(&mut e, Step::Click(&tr("Add Component"))) {
            collect(&mut found, &data, &p, "add component");
        }
    }
    for (tab, label) in [(BottomTab::Console, "console"), (BottomTab::Profiler, "profiler")] {
        let mut e = level1();
        log::warn!("a warning, only to fill the Console");
        e.set_bottom_tab(tab);
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 12);
        collect(&mut found, &data, &p, label);
    }
    {
        let mut e = level1();
        e.apply(Action::Play);
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 12);
        collect(&mut found, &data, &p, "play mode");
        e.apply(Action::Stop);
    }
    // Окна: Preferences и Project Settings, каждая страница; Build, Getting Started, Shortcuts.
    for scope in [SettingsScope::Preferences, SettingsScope::Project] {
        for (page, _) in Editor::settings_pages(scope) {
            let mut e = level1();
            match scope {
                SettingsScope::Preferences => {
                    e.show_preferences(true);
                    e.show_settings_page(scope, page);
                }
                SettingsScope::Project => {
                    e.show_project_settings(true);
                    e.show_project_settings_page(page);
                }
            }
            let mut p = Probe::new(&mut e, size);
            p.settle(&mut e, 6);
            collect(&mut found, &data, &p, &format!("{scope:?} → {page}"));
        }
    }
    {
        let mut e = level1();
        e.apply(Action::ShowBuildDialog(true));
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 6);
        collect(&mut found, &data, &p, "build dialog");
    }
    {
        let mut e = level1();
        e.show_getting_started(true);
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 6);
        collect(&mut found, &data, &p, "getting started");
    }
    {
        let mut e = level1();
        e.show_shortcuts_help(true);
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 6);
        collect(&mut found, &data, &p, "shortcuts help");
    }
    {
        let mut e = level1();
        e.apply(Action::CreateEmpty { parent: None });
        e.apply(Action::NewScene);
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 6);
        collect(&mut found, &data, &p, "unsaved dialog");
    }
    {
        let mut e = level1();
        let ids: Vec<GameObjectId> = ["Platform A", "Platform B", "Platform C"].iter().map(|n| find(&e, n)).collect();
        e.select_many(&ids, Some(ids[0]));
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 6);
        collect(&mut found, &data, &p, "multi selection");
    }
    {
        let mut e = level1();
        e.select_asset("materials/painted_metal.material");
        e.set_bottom_tab(BottomTab::Project);
        let mut p = Probe::new(&mut e, size);
        p.settle(&mut e, 30);
        collect(&mut found, &data, &p, "material");
    }
    {
        let mut t = Editor::new(engine::app(), EditorConfig::titled("Terrain")).unwrap();
        let id = t.create_terrain();
        t.set_scene_3d(true);
        t.apply(Action::Select(Some(id)));
        t.set_terrain_tool(Some(BrushTool::Raise));
        let mut p = Probe::new(&mut t, size);
        p.settle(&mut t, 6);
        collect(&mut found, &data, &p, "terrain");
    }
    found
}

#[test]
fn the_russian_screen_has_no_english_labels() {
    let _lang = in_language(Language::Russian);
    let found = russian_leaks();
    if let Ok(path) = std::env::var("L10N_REPORT") {
        std::fs::write(path, found.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap();
    }
    assert!(found.is_empty(), "English text on the Russian screen ({}):\n{}", found.len(), found.iter().cloned().collect::<Vec<_>>().join("\n"));
}

/// Русские надписи не длиннее места: ни одна не выходит за своё окно по ширине кадра.
#[test]
fn russian_labels_stay_inside_the_screen() {
    let _lang = in_language(Language::Russian);
    let mut e = level1();
    e.apply(Action::Select(Some(find(&e, "Player"))));
    for scale in [1.0, 1.5] {
        let mut prefs = e.preferences().clone();
        prefs.ui_scale = scale;
        e.set_preferences(prefs);
        let mut p = Probe::new(&mut e, egui::vec2(1440.0, 860.0));
        p.settle(&mut e, 6);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 860.0));
        for (text, rect) in p.texts() {
            assert!(
                rect.right() <= screen.right() * scale + 1.0 && rect.left() >= -1.0,
                "at {scale}: `{text}` sticks out of the screen: {rect:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Галерея снимков (GPU)
// ---------------------------------------------------------------------------

fn shoot(editor: &mut Editor, dir: &Path, name: &str, steps: &[Step], frames: u32) {
    let mut probe = Probe::new(editor, egui::vec2(SIZE.0 as f32, SIZE.1 as f32));
    let mut script: Vec<Vec<egui::Event>> = Vec::new();
    for step in steps {
        let (Step::Click(text) | Step::Right(text) | Step::Hover(text)) = *step;
        let Some(pos) = probe.find(text) else {
            eprintln!("[{name}] text `{text}` not found; the shot goes without this step");
            break;
        };
        let button = match step {
            Step::Right(_) => Some(egui::PointerButton::Secondary),
            Step::Click(_) => Some(egui::PointerButton::Primary),
            Step::Hover(_) => None,
        };
        let mut events: Vec<Vec<egui::Event>> = vec![vec![egui::Event::PointerMoved(pos)]];
        if let Some(b) = button {
            events.push(vec![press(pos, b, true)]);
            events.push(vec![press(pos, b, false)]);
        }
        events.push(vec![]);
        for ev in events {
            probe.frame(editor, ev.clone());
            script.push(ev);
        }
    }
    let start = 6u32;
    let total = frames.max(start + script.len() as u32 + 4);
    let image = capture_with_input(editor, SIZE.0, SIZE.1, total, |frame, _| {
        frame.checked_sub(start).and_then(|i| script.get(i as usize).cloned()).unwrap_or_default()
    })
    .unwrap();
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

/// Одни и те же состояния на обоих языках: `<имя>_en.png`, `<имя>_ru.png`.
#[test]
#[ignore = "needs a GPU; writes PNG files into L10N_SHOTS"]
fn l10n_gallery() {
    let dir = PathBuf::from(std::env::var("L10N_SHOTS").unwrap_or_else(|_| ".".to_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("L10N_ONLY").unwrap_or_default();
    let want = |name: &str| only.is_empty() || only.split(',').any(|n| name.starts_with(n));
    for (language, suffix) in [(Language::English, "en"), (Language::Russian, "ru")] {
        let _lang = in_language(language);
        let t = |s: &'static str| tr(s).to_string();
        let name = |base: &str| format!("{base}_{suffix}");
        if want("01_main") {
            let mut e = level1();
            e.apply(Action::Select(Some(find(&e, "Player"))));
            shoot(&mut e, &dir, &name("01_main"), &[], 10);
        }
        if want("02_preferences") {
            let mut e = level1();
            e.show_preferences(true);
            shoot(&mut e, &dir, &name("02_preferences"), &[], 8);
        }
        if want("03_project_settings") {
            let mut e = level1();
            e.show_project_settings(true);
            shoot(&mut e, &dir, &name("03_project_settings"), &[], 8);
        }
        if want("04_context_menu") {
            let mut e = level1();
            shoot(&mut e, &dir, &name("04_context_menu"), &[Step::Right("Platform A")], 10);
        }
        if want("05_menu_file") {
            let mut e = level1();
            let file = t("File");
            shoot(&mut e, &dir, &name("05_menu_file"), &[Step::Click(&file)], 8);
        }
        if want("06_project") {
            let mut e = level1();
            e.set_bottom_tab(BottomTab::Project);
            e.select_asset("sprites/player.png");
            shoot(&mut e, &dir, &name("06_project"), &[], 24);
        }
        if want("07_getting_started") {
            let mut e = level1();
            e.show_getting_started(true);
            shoot(&mut e, &dir, &name("07_getting_started"), &[], 8);
        }
        if want("08_shortcuts") {
            let mut e = level1();
            e.show_shortcuts_help(true);
            shoot(&mut e, &dir, &name("08_shortcuts"), &[], 8);
        }
        if want("09_multi") {
            let mut e = level1();
            let ids: Vec<GameObjectId> = ["Platform A", "Platform B", "Platform C"].iter().map(|n| find(&e, n)).collect();
            e.select_many(&ids, Some(ids[0]));
            shoot(&mut e, &dir, &name("09_multi"), &[], 8);
        }
        if want("10_build") {
            let mut e = level1();
            e.apply(Action::ShowBuildDialog(true));
            shoot(&mut e, &dir, &name("10_build"), &[], 8);
        }
    }
    let _ = Panel::ALL;
}
