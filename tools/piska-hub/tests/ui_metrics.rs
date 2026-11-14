//! Замер интерфейса хаба без окна и GPU (`kit::metrics`, правила плотности
//! [`Rules::comfortable`]): каждый экран хаба × английский и русский × масштаб 100 / 125 / 150 %
//! × ширина окна от наименьшей (960 точек) до широкой.
//!
//! Падает, если: у контрола с рамкой высота не [`theme::CONTROL_H_COMFORTABLE`]; соседи строки
//! расходятся по верху или низу; промежуток между ними не из шкалы отступов; рамка рисуется за
//! пределами контрола; контрол выходит за свой контейнер (карточка, панель, диалог, полоса,
//! строка); текст контрола ближе 4 точек к его краю; чужой текст заходит под контрол. На
//! наименьшей ширине каждому контролу ещё прогоняются наведение, нажатие и фокус клавиатуры.
//!
//! `UI_METRICS_OUT=<файл>` — записать сводку (высоты, промежутки, нарушения) в Markdown.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use engine_editor::kit::i18n::{self, Language};
use engine_editor::kit::metrics::{self, Report, Rules};
use piska_hub::config::{SortOrder, ViewMode};
use piska_hub::tasks::{Work, WorkError, WorkKind};
use piska_hub::{paths, Hub, HubAction, HubOptions, Page};

const DT: f32 = 1.0 / 60.0;
const NOW: u64 = 1_763_040_000;
static LANGUAGE: Mutex<()> = Mutex::new(());

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-metrics-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_project(parent: &Path, name: &str) -> PathBuf {
    let root = paths::find_engine_root().unwrap();
    let plan = piska_hub::templates::plan(
        &piska_hub::templates::NewProject { name: name.into(), location: parent.to_path_buf(), template: piska_hub::templates::Template::Empty3D, cpp: true },
        &root,
    )
    .unwrap();
    piska_hub::templates::create(&plan, &root).unwrap();
    plan.dir
}

struct Fixture {
    config: PathBuf,
    outside: PathBuf,
    miner: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let config = temp("config");
        let outside = temp("outside");
        let miner = make_project(&outside, "space_miner");
        let games = paths::find_engine_root().unwrap().join("games");
        let mut list = piska_hub::config::HubConfig::default();
        for (i, g) in ["lantern", "swarm", "template", "marble"].iter().enumerate() {
            list.touch(&games.join(g), NOW - [13 * 3600, 26 * 3600, 7 * 60, 3 * 86_400][i]);
        }
        list.entry_mut(&games.join("marble")).favorite = true;
        list.touch(&outside.join("old_prototype"), NOW - 40 * 86_400);
        list.touch(&miner, NOW - 50 * 60);
        list.settings.project_roots.push(outside.clone());
        list.save(&config.join(paths::CONFIG_FILE)).unwrap();
        Fixture { config, outside, miner }
    }

    fn hub(&self, scale: f32) -> Hub {
        let mut hub = Hub::new(HubOptions { config_dir: Some(self.config.clone()), dry_run: true, tools_dir: Some(self.config.join("tools")), ..HubOptions::default() });
        hub.set_now(NOW);
        hub.apply(HubAction::SetUiScale(scale));
        hub.import_page_mut().settings.location = self.outside.join("projects");
        hub
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.config);
        let _ = std::fs::remove_dir_all(&self.outside);
    }
}

/// Задача, которая «идёт», пока её не отменят.
fn forever(id: u64, kind: WorkKind, title: &str) -> Work {
    let title = title.to_string();
    Work::spawn(id, kind, title, move |control, report| {
        while !control.is_cancelled() {
            let progress = engine_project::Progress {
                phase: engine_project::Phase::Working,
                files_done: 212,
                files_total: 640,
                bytes_done: 212 * 190_000,
                bytes_total: 640 * 190_000,
                current: "assets/models/hero/body_albedo.png".into(),
            };
            report("Copying files", &progress);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(WorkError::Cancelled)
    })
}

/// Экраны хаба: имя и подготовка (на свежем хабе).
fn screens() -> Vec<(&'static str, fn(&mut Hub, &Fixture))> {
    vec![
        ("projects-list", |_, _| {}),
        ("projects-grid", |hub, _| {
            hub.apply(HubAction::SetView(ViewMode::Grid));
            hub.apply(HubAction::SetSort(SortOrder::Name));
        }),
        ("new-project", |hub, f| {
            hub.apply(HubAction::Navigate(Page::NewProject));
            hub.wizard_mut().name = "asteroid_belt".into();
            hub.wizard_mut().location = f.outside.display().to_string();
            hub.wizard_mut().cpp = true;
        }),
        ("new-project-invalid", |hub, _| {
            hub.apply(HubAction::Navigate(Page::NewProject));
            hub.wizard_mut().name = "Bad Name!".into();
        }),
        ("build-progress", |hub, _| {
            let lantern = paths::find_engine_root().unwrap().join("games").join("lantern");
            hub.apply(HubAction::RebuildAndOpen(lantern));
            hub.apply(HubAction::ShowJob(true));
            if let Some(job) = hub.job_mut() {
                job.feed_stderr("   Compiling engine-render v0.0.1 (crates/engine-render)\r    Building [=============>          ] 214/356: engine-render");
            }
        }),
        ("build-in-background", |hub, _| {
            let lantern = paths::find_engine_root().unwrap().join("games").join("lantern");
            hub.apply(HubAction::RebuildAndOpen(lantern));
            hub.apply(HubAction::ShowJob(false));
        }),
        ("build-failed", |hub, _| {
            let lantern = paths::find_engine_root().unwrap().join("games").join("lantern");
            hub.apply(HubAction::RebuildAndOpen(lantern));
            hub.apply(HubAction::ShowJob(true));
            if let Some(job) = hub.job_mut() {
                job.feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///e/games/lantern#0.0.1","message":{"level":"error","message":"cannot find value `jump_heigth` in this scope","rendered":"error[E0425]: cannot find value `jump_heigth`\n","spans":[{"file_name":"games/lantern/src/player.rs","line_start":142,"column_start":27,"is_primary":true}]}}"#);
                job.feed_stdout(r#"{"reason":"build-finished","success":false}"#);
                job.simulate_exit(false);
            }
        }),
        ("samples", |hub, _| hub.apply(HubAction::Navigate(Page::Samples))),
        ("tools", |hub, _| hub.apply(HubAction::Navigate(Page::Tools))),
        ("settings", |hub, _| hub.apply(HubAction::Navigate(Page::Settings))),
        ("add-existing", |hub, _| hub.apply(HubAction::ShowAdd { locate: None })),
        ("delete-confirm", |hub, f| hub.apply(HubAction::AskDelete(f.miner.clone()))),
        ("import-empty", |hub, _| hub.apply(HubAction::ShowImport)),
        ("import-report", |hub, f| {
            hub.apply(HubAction::ImportAdd(f.miner.clone()));
            hub.wait_for_import();
        }),
        ("import-advanced", |hub, f| {
            hub.apply(HubAction::ImportAdd(f.miner.clone()));
            hub.wait_for_import();
            hub.import_page_mut().advanced = true;
        }),
        ("import-blocked", |hub, f| {
            hub.apply(HubAction::ImportAdd(f.miner.clone()));
            hub.wait_for_import();
            hub.import_page_mut().items[0].name = "Space Miner".into();
        }),
        ("import-running", |hub, f| {
            hub.apply(HubAction::ImportAdd(f.miner.clone()));
            hub.wait_for_import();
            hub.add_work(forever(1, WorkKind::Import, "Importing “Space Miner”"));
            hub.import_page_mut().stage = piska_hub::ImportStage::Running;
            hub.import_page_mut().work = Some(1);
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                hub.update();
            }
        }),
        ("task-strip", |hub, _| {
            hub.add_work(forever(2, WorkKind::Pack, "Saving “Space Miner” to a file"));
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                hub.update();
            }
        }),
        ("save-as-file", |hub, f| hub.apply(HubAction::ShowPack(f.miner.clone()))),
        ("folder-browser", |hub, f| {
            hub.apply(HubAction::Browse(piska_hub::BrowseTarget::ImportFile));
            if let Some(piska_hub::Dialog::Browse { browser, .. }) = hub.dialog_mut() {
                browser.enter(&f.outside);
            }
        }),
    ]
}

/// Один кадр хаба с событиями; фигуры кадра.
fn frame(ctx: &egui::Context, hub: &mut Hub, size: egui::Vec2, time: &mut f64, events: Vec<egui::Event>) -> Vec<egui::epaint::ClippedShape> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        time: Some(*time),
        predicted_dt: DT,
        events,
        ..egui::RawInput::default()
    };
    *time += f64::from(DT);
    hub.update();
    ctx.run(input, |ctx| hub.ui(ctx)).shapes
}

fn stop_works(hub: &mut Hub) {
    hub.apply(HubAction::CancelWork(1));
    hub.apply(HubAction::CancelWork(2));
    hub.wait_for_works();
}

#[test]
fn hub_screens_measure_up() {
    let _guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let mut total = Report::default();
    let mut screens_done = 0;
    let mut states_done = 0;
    for language in [Language::English, Language::Russian] {
        i18n::set(language);
        for scale in [1.0_f32, 1.25, 1.5] {
            for (name, setup) in screens() {
                let mut hub = fixture.hub(scale);
                setup(&mut hub, &fixture);
                let menu = false;
                for width in [960.0_f32, 1280.0, 1700.0] {
                    // Окно в физических пикселях; точек меньше при крупном масштабе.
                    let size = egui::vec2(width, 720.0);
                    let ctx = egui::Context::default();
                    let mut time = 0.0;
                    for _ in 0..5 {
                        frame(&ctx, &mut hub, size, &mut time, Vec::new());
                    }
                    let shapes = frame(&ctx, &mut hub, size, &mut time, Vec::new());
                    let screen = format!("{language:?} {scale}x {width}px {name}");
                    let rules = Rules { text_overlap: !menu, ..Rules::comfortable() };
                    total.merge(metrics::analyze_shapes(&screen, &metrics::widgets(&ctx), &shapes, rules));
                    screens_done += 1;
                    // Состояния — на наименьшей ширине, при 100 % и 150 %.
                    if width == 960.0 && scale != 1.25 {
                        let (report, states) = metrics::analyze_states(&screen, &ctx, rules, |events| frame(&ctx, &mut hub, size, &mut time, events));
                        total.merge(report);
                        states_done += states;
                    }
                }
                stop_works(&mut hub);
            }
        }
    }
    i18n::set(Language::English);
    let report = format!(
        "# Hub UI metrics\n\n{screens_done} screens, {states_done} control states.\n\n## Control heights (pt)\n\n| height | count |\n|---|---|\n{}\n\n## Gaps between row neighbours (pt)\n\n| gap | count |\n|---|---|\n{}\n\n## Offenders ({})\n\n{}\n",
        Report::table(&total.heights),
        Report::table(&total.gaps),
        total.offenders.len(),
        total.offenders.iter().map(|o| format!("* {o}")).collect::<Vec<_>>().join("\n")
    );
    if let Some(out) = std::env::var_os("UI_METRICS_OUT") {
        std::fs::write(out, &report).unwrap();
    }
    assert!(screens_done >= 300, "screens measured: {screens_done}");
    assert!(states_done > 500, "control states driven: {states_done}");
    assert!(total.offenders.is_empty(), "{} offenders, first ones:\n{}", total.offenders.len(), total.offenders.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
}
