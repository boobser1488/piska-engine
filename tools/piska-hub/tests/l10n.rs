//! Русский язык хаба без GPU: язык переключается на лету, настройка «Language» живёт в
//! Settings, на русском экране не остаётся английских надписей, формы множественного числа.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use engine_editor::kit::i18n::{self, Language};
use piska_hub::{paths, Hub, HubAction, HubOptions, Page};

const SCREEN: egui::Vec2 = egui::vec2(1440.0, 900.0);
const DT: f32 = 1.0 / 60.0;
const NOW: u64 = 1_763_040_000;

static LANGUAGE: Mutex<()> = Mutex::new(());

struct InLanguage(MutexGuard<'static, ()>);

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

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-l10n-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn staged_hub(config_dir: &Path, outside: &Path) -> Hub {
    let root = paths::find_engine_root().unwrap();
    let games = root.join("games");
    let mut config = piska_hub::config::HubConfig::default();
    config.touch(&games.join("lantern"), NOW - 2 * 3600);
    config.touch(&games.join("swarm"), NOW - 26 * 3600);
    config.touch(&games.join("template"), NOW - 7 * 60);
    config.entry_mut(&games.join("marble")).favorite = true;
    config.touch(&games.join("marble"), NOW - 3 * 86_400);
    config.touch(&outside.join("old_prototype"), NOW - 40 * 86_400);
    config.save(&config_dir.join(paths::CONFIG_FILE)).unwrap();
    let mut hub = Hub::new(HubOptions { config_dir: Some(config_dir.to_path_buf()), dry_run: true, ..HubOptions::default() });
    hub.set_now(NOW);
    hub
}

struct Probe {
    ctx: egui::Context,
    time: f64,
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Probe {
    fn new(hub: &mut Hub) -> Self {
        let mut probe = Self { ctx: egui::Context::default(), time: 0.0, shapes: Vec::new() };
        probe.settle(hub, 4);
        probe
    }

    fn settle(&mut self, hub: &mut Hub, frames: usize) {
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                time: Some(self.time),
                predicted_dt: DT,
                ..egui::RawInput::default()
            };
            self.time += f64::from(DT);
            hub.update();
            self.shapes = self.ctx.run(input, |ctx| hub.ui(ctx)).shapes;
        }
    }

    fn texts(&self) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &self.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }
}

fn is_data(text: &str) -> bool {
    let text = text.trim();
    const ALLOWED: &[&str] = &[
        "lantern", "swarm", "template", "marble", "old_prototype", "Lantern Keeper", "Marble Factory", "Swarm", "Piska", "Rust",
        "C++", "Windows", "Android", "Linux", "Cargo", "cargo", "OK", "GPU", "Starfall", "Piska Hub", "Piska Modeler", "Piska Editor",
        "Piska Player", "Piska Engine", "Flawed Game", "dest", "App", "app", "Letter", "letter", "English", "Debug", "Release", "Web", "UI", "FPS", "glTF", "Scenario", "RON",
    ];
    ALLOWED.iter().any(|a| text.starts_with(a))
        || text.contains(['/', '\\', '_', '{'])
        || text.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase())
        || text.starts_with(['#', '+', '-'])
        || text.rsplit('.').next().is_some_and(|e| text.contains('.') && e.len() <= 8 && !e.contains(' '))
}

fn leaks(probe: &Probe) -> BTreeSet<String> {
    probe
        .texts()
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && !t.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c)))
        .filter(|t| t.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 3 && !is_data(t))
        .collect()
}

#[test]
fn the_hub_switches_language_at_runtime() {
    let _lang = in_language(Language::English);
    let config = temp("switch-config");
    let outside = temp("switch-outside");
    let mut hub = staged_hub(&config, &outside);
    let mut probe = Probe::new(&mut hub);
    assert!(probe.texts().iter().any(|t| t == "Projects"), "English: {:?}", probe.texts());
    hub.apply(HubAction::SetLanguage(Some(Language::Russian)));
    probe.settle(&mut hub, 3);
    let russian = probe.texts();
    assert!(russian.iter().any(|t| t == "Проекты"), "Russian: {russian:?}");
    assert_eq!(hub.preferences().language, Some(Language::Russian));
    hub.apply(HubAction::SetLanguage(Some(Language::English)));
    probe.settle(&mut hub, 3);
    assert!(probe.texts().iter().any(|t| t == "Projects"));
}

#[test]
fn the_russian_hub_has_no_english_labels() {
    let _lang = in_language(Language::Russian);
    let config = temp("leaks-config");
    let outside = temp("leaks-outside");
    let mut hub = staged_hub(&config, &outside);
    let mut found = BTreeSet::new();
    let mut probe = Probe::new(&mut hub);
    found.extend(leaks(&probe).into_iter().map(|t| format!("{t}   [projects]")));
    for (page, name) in [(Page::NewProject, "new project"), (Page::Samples, "samples"), (Page::Tools, "tools"), (Page::Settings, "settings")] {
        hub.apply(HubAction::Navigate(page));
        probe.settle(&mut hub, 4);
        found.extend(leaks(&probe).into_iter().map(|t| format!("{t}   [{name}]")));
    }
    hub.apply(HubAction::Navigate(Page::Projects));
    hub.apply(HubAction::ShowAdd { locate: None });
    probe.settle(&mut hub, 4);
    found.extend(leaks(&probe).into_iter().map(|t| format!("{t}   [add existing]")));
    hub.apply(HubAction::Navigate(Page::Projects));
    let lantern = paths::find_engine_root().unwrap().join("games").join("lantern");
    hub.apply(HubAction::RebuildAndOpen(lantern));
    hub.apply(HubAction::ShowJob(true));
    if let Some(job) = hub.job_mut() {
        job.feed_stderr("   Compiling engine-core v0.0.1 (crates/engine-core)\r    Building [=======>       ] 120/320: engine-core, rapier2d");
    }
    probe.settle(&mut hub, 4);
    found.extend(leaks(&probe).into_iter().map(|t| format!("{t}   [build progress]")));
    if let Ok(path) = std::env::var("L10N_REPORT") {
        std::fs::write(path, found.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap();
    }
    assert!(found.is_empty(), "English text in the Russian hub ({}):\n{}", found.len(), found.iter().cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn relative_times_use_russian_plural_forms() {
    let _lang = in_language(Language::Russian);
    let now = 1_000_000_u64;
    let ago = |seconds: u64| piska_hub::config::ago(Some(now - seconds), now);
    assert_eq!(ago(60), "1 минуту назад");
    assert_eq!(ago(2 * 60), "2 минуты назад");
    assert_eq!(ago(5 * 60), "5 минут назад");
    assert_eq!(ago(21 * 60), "21 минуту назад");
    assert_eq!(ago(3600), "1 час назад");
    assert_eq!(ago(3 * 3600), "3 часа назад");
    assert_eq!(ago(11 * 3600), "11 часов назад");
    assert_eq!(ago(3 * 86_400), "3 дня назад");
    assert_eq!(ago(5 * 86_400), "5 дней назад");
}

/// Проект для страниц импорта: со всеми видами замечаний предварительного отчёта.
fn flawed_project(parent: &Path) -> PathBuf {
    let root = paths::find_engine_root().unwrap();
    let plan = piska_hub::templates::plan(
        &piska_hub::templates::NewProject {
            name: "flawed_game".into(),
            location: parent.to_path_buf(),
            template: piska_hub::templates::Template::Empty3D,
            cpp: true,
        },
        &root,
    )
    .unwrap();
    piska_hub::templates::create(&plan, &root).unwrap();
    let dir = plan.dir;
    std::fs::create_dir_all(dir.join("target/debug")).unwrap();
    std::fs::write(dir.join("target/debug/x"), "derived").unwrap();
    std::fs::write(dir.join("cpp/CMakeLists.txt"), "project(x)").unwrap();
    std::fs::write(dir.join("assets/loose.png"), [1, 2, 3]).unwrap();
    std::fs::write(dir.join("Cargo.lock"), "[[package]]\nname = \"engine\"\nversion = \"9.9.9\"\n").unwrap();
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), manifest.replace("edition = \"2021\"", "edition = \"2018\"\nrust-version = \"999.0\"")).unwrap();
    dir
}

#[test]
fn the_russian_import_and_save_screens_have_no_english_labels() {
    let _lang = in_language(Language::Russian);
    let config = temp("import-config");
    let outside = temp("import-outside");
    let project = flawed_project(&outside);
    let mut hub = staged_hub(&config, &outside);
    let mut probe = Probe::new(&mut hub);
    let mut found = BTreeSet::new();
    let mut check = |hub: &mut Hub, probe: &mut Probe, what: &str| {
        probe.settle(hub, 4);
        found.extend(leaks(probe).into_iter().map(|t| format!("{t}   [{what}]")));
    };
    // Пустая страница, затем с источником и отчётом; раздел «Advanced» раскрыт.
    hub.apply(HubAction::ShowImport);
    check(&mut hub, &mut probe, "import: empty");
    hub.apply(HubAction::ImportAdd(project.clone()));
    hub.wait_for_import();
    probe.ctx.data_mut(|d| d.insert_persisted(egui::Id::new(Hub::IMPORT_ADVANCED_ID), true));
    check(&mut hub, &mut probe, "import: report and settings");
    let all = hub.import_problems(0);
    assert!(all.len() >= 5, "the fixture has many findings: {all:?}");
    for problem in &all {
        let text = problem.text();
        assert!(text.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c)) || text.contains("Cargo.toml:"), "untranslated problem text: {text}");
    }
    hub.import_page_mut().settings.mode = piska_hub::import::Mode::InPlace;
    check(&mut hub, &mut probe, "import: in place");
    hub.import_page_mut().settings.mode = piska_hub::import::Mode::Copy;
    hub.import_page_mut().settings.location = outside.join("dest");
    // Результат.
    hub.apply(HubAction::ImportRun);
    check(&mut hub, &mut probe, "import: running");
    hub.wait_for_works();
    check(&mut hub, &mut probe, "import: result");
    let result = hub.import_page().result.clone().unwrap();
    for change in &result.outcomes[0].changes {
        assert!(!change.text().is_empty());
    }
    hub.apply(HubAction::ImportReset);
    hub.apply(HubAction::Navigate(Page::Projects));
    // Окно сохранения и строка задачи.
    hub.apply(HubAction::ShowPack(project.clone()));
    check(&mut hub, &mut probe, "save window");
    hub.apply(HubAction::Browse(piska_hub::BrowseTarget::PackFolder));
    check(&mut hub, &mut probe, "browse folder for the file");
    hub.apply(HubAction::CloseDialog);
    let Some(piska_hub::Dialog::Pack(pack)) = hub.dialog().cloned() else {
        panic!("closing the browser returns to the save window");
    };
    let mut pack = *pack;
    pack.out = outside.join("flawed.pks").display().to_string();
    hub.apply(HubAction::StartPack(Box::new(pack)));
    hub.wait_for_works();
    check(&mut hub, &mut probe, "task row");
    hub.apply(HubAction::Browse(piska_hub::BrowseTarget::ImportFile));
    check(&mut hub, &mut probe, "browse file");
    hub.apply(HubAction::CloseDialog);
    if let Ok(path) = std::env::var("L10N_REPORT") {
        std::fs::write(path, found.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap();
    }
    assert!(found.is_empty(), "English text on the Russian import screens ({}):\n{}", found.len(), found.iter().cloned().collect::<Vec<_>>().join("\n"));
    for d in [config, outside] {
        std::fs::remove_dir_all(d).unwrap();
    }
}
