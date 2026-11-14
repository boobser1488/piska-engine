//! Снимки страницы «Import Project», окна «Save Project As File…» и строк задач без окна
//! (GPU). Картинки пишутся в `PISKA_HUB_SHOTS/import/` (если задан); язык — `PISKA_HUB_LANG=ru`,
//! масштаб интерфейса — `PISKA_HUB_SCALE=1.5`. Их смотрят глазами; тест проверяет, что
//! кадры нарисованы.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use engine_project::{Control, PackOptions};
use piska_hub::tasks::{Work, WorkError, WorkKind};
use piska_hub::{capture, paths, Hub, HubAction, HubOptions, Page};

static GPU: Mutex<()> = Mutex::new(());

const W: u32 = 1440;
const H: u32 = 900;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-shots-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_project(parent: &Path, name: &str, cpp: bool) -> PathBuf {
    let root = paths::find_engine_root().unwrap();
    let plan = piska_hub::templates::plan(
        &piska_hub::templates::NewProject {
            name: name.into(),
            location: parent.to_path_buf(),
            template: piska_hub::templates::Template::Empty3D,
            cpp,
        },
        &root,
    )
    .unwrap();
    piska_hub::templates::create(&plan, &root).unwrap();
    plan.dir
}

/// Проект с замечаниями предварительного отчёта.
fn flawed(parent: &Path) -> PathBuf {
    let dir = make_project(parent, "space_miner", true);
    std::fs::create_dir_all(dir.join("target/debug")).unwrap();
    std::fs::write(dir.join("target/debug/x"), "derived").unwrap();
    std::fs::create_dir_all(dir.join("dist/web")).unwrap();
    std::fs::write(dir.join("dist/web/index.html"), vec![b'x'; 300_000]).unwrap();
    std::fs::write(dir.join("assets/loose.png"), [1, 2, 3]).unwrap();
    std::fs::write(dir.join("cpp/CMakeLists.txt"), "project(x)").unwrap();
    std::fs::write(dir.join("Cargo.lock"), "[[package]]\nname = \"engine\"\nversion = \"0.0.0\"\n").unwrap();
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        manifest.replace("edition = \"2021\"", "edition = \"2018\"\nrust-version = \"999.0\"")
            + "\n[dependencies.shared_ui]\npath = \"C:/Users/somebody/shared_ui\"\n",
    )
    .unwrap();
    dir
}

fn save(image: &image::RgbaImage, name: &str) {
    let Some(base) = std::env::var_os("PISKA_HUB_SHOTS") else {
        return;
    };
    let lang = std::env::var("PISKA_HUB_LANG").unwrap_or_else(|_| "en".into());
    let scale = std::env::var("PISKA_HUB_SCALE").unwrap_or_else(|_| "1.0".into());
    let dir = PathBuf::from(base).join("import");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(format!("{name}_{lang}_{scale}.png"))).unwrap();
}

fn colors(image: &image::RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels().step_by(5) {
        seen.insert(pixel.0);
    }
    seen.len()
}

fn shoot(hub: &mut Hub, name: &str) -> image::RgbaImage {
    let image = capture(hub, W, H, 8, |_, _| vec![]).expect("the hub renders without a window");
    assert!(colors(&image) > 60, "{name} is drawn");
    save(&image, name);
    image
}

/// Снимок после прокрутки правой колонки (раздел «Advanced» лежит ниже первого экрана).
fn shoot_scrolled(hub: &mut Hub, name: &str, scale: f32) -> image::RgbaImage {
    let at = egui::pos2(W as f32 * 0.7 / scale, H as f32 * 0.55 / scale);
    let image = capture(hub, W, H, 12, |frame, _| match frame {
        3 => vec![egui::Event::PointerMoved(at)],
        4..=8 => vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
        }],
        _ => vec![],
    })
    .expect("the hub renders without a window");
    assert!(colors(&image) > 60, "{name} is drawn");
    save(&image, name);
    image
}

/// Задача, которая «идёт», пока её не отменят: для снимка хода.
fn forever(id: u64, kind: WorkKind, title: &str, files: (u64, u64)) -> Work {
    let title = title.to_string();
    Work::spawn(id, kind, title, move |control: &Control, report| {
        let mut done = files.0;
        while !control.is_cancelled() {
            let progress = engine_project::Progress {
                phase: engine_project::Phase::Working,
                files_done: done,
                files_total: files.1,
                bytes_done: done * 190_000,
                bytes_total: files.1 * 190_000,
                current: "assets/models/hero/body_albedo.png".into(),
            };
            report("Copying files", &progress);
            done = files.0;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(WorkError::Cancelled)
    })
}

#[test]
fn import_screens_render() {
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(language) = std::env::var("PISKA_HUB_LANG").ok().and_then(|code| engine_editor::kit::i18n::Language::from_code(&code)) {
        engine_editor::kit::i18n::set(language);
    }
    let scale: f32 = std::env::var("PISKA_HUB_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let config = temp("config");
    let root = temp("root");
    let flawed_dir = flawed(&root);
    let library = root.join("library");
    let a = make_project(&library, "lantern_keeper", false);
    make_project(&library.join("arcade"), "marble_run", true);
    make_project(&library.join("arcade"), "swarm_wars", false);
    let archive = root.join("my_platformer.pks");
    {
        let src = make_project(&root.join("src"), "my_platformer", false);
        engine_project::pack(&src, &archive, &PackOptions { created: Some(1_763_040_000), ..PackOptions::default() }, &Control::new(), &mut |_| {}).unwrap();
    }

    let new_hub = || {
        let mut hub = Hub::new(HubOptions {
            config_dir: Some(config.clone()),
            dry_run: true,
            tools_dir: Some(config.join("tools")),
            ..HubOptions::default()
        });
        hub.set_now(1_763_040_000);
        hub.apply(HubAction::SetUiScale(scale));
        hub.import_page_mut().settings.location = root.join("projects");
        hub
    };

    // 1. Пустая страница.
    let mut hub = new_hub();
    hub.apply(HubAction::ShowImport);
    shoot(&mut hub, "01_empty");

    // 2. Один проект с замечаниями, «Advanced» закрыт и открыт.
    let mut hub = new_hub();
    hub.apply(HubAction::ImportAdd(flawed_dir.clone()));
    hub.wait_for_import();
    shoot(&mut hub, "02_report");
    hub.import_page_mut().advanced = true;
    shoot_scrolled(&mut hub, "03_advanced", scale);

    // 3. Несколько источников: поиск в каталоге и файл проекта.
    let mut hub = new_hub();
    hub.apply(HubAction::ImportAdd(library.clone()));
    hub.apply(HubAction::ImportAdd(archive.clone()));
    hub.wait_for_import();
    hub.import_page_mut().selected = 1;
    hub.import_page_mut().items[2].ticked = false;
    shoot(&mut hub, "04_several_sources");
    hub.import_page_mut().selected = 3;
    shoot(&mut hub, "05_archive_selected");

    // 4. Блокирующие проблемы: имя и каталог.
    let mut hub = new_hub();
    hub.apply(HubAction::ImportAdd(a.clone()));
    hub.wait_for_import();
    hub.import_page_mut().items[0].name = "Lantern Keeper".into();
    shoot(&mut hub, "06_blocked");

    // 5. Ход импорта (задача «идёт» до отмены) и полоса задач.
    let mut hub = new_hub();
    hub.apply(HubAction::ImportAdd(flawed_dir.clone()));
    hub.wait_for_import();
    hub.add_work(forever(1, WorkKind::Import, "Importing “Space Miner”", (212, 640)));
    hub.import_page_mut().stage = piska_hub::ImportStage::Running;
    hub.import_page_mut().work = Some(1);
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        hub.update();
    }
    shoot(&mut hub, "07_running");
    hub.apply(HubAction::Navigate(Page::Projects));
    shoot(&mut hub, "08_task_strip");
    hub.apply(HubAction::CancelWork(1));
    hub.wait_for_works();

    // 6. Результат.
    let mut hub = new_hub();
    hub.apply(HubAction::ImportAdd(flawed_dir.clone()));
    hub.wait_for_import();
    {
        let page = hub.import_page_mut();
        page.items[0].name = "asteroid_miner".into();
        page.items[0].name_touched = true;
        page.settings.regenerate_guids = true;
        page.settings.test = true;
    }
    hub.apply(HubAction::ImportRun);
    hub.wait_for_works();
    shoot(&mut hub, "09_result");

    // 7. Окно «Save Project As File…» и выбор файла.
    let mut hub = new_hub();
    hub.config.entry_mut(&flawed_dir).last_opened = Some(1_763_000_000);
    hub.refresh();
    hub.apply(HubAction::ShowPack(flawed_dir.clone()));
    shoot(&mut hub, "10_save_window");
    hub.apply(HubAction::CloseDialog);
    hub.apply(HubAction::Browse(piska_hub::BrowseTarget::ImportFile));
    if let Some(piska_hub::Dialog::Browse { browser, .. }) = hub.dialog_mut() {
        browser.enter(&root);
    }
    shoot(&mut hub, "11_browse_file");
    hub.apply(HubAction::CloseDialog);

    // 8. Строки задач: сохранение идёт и закончилось.
    hub.config.entry_mut(&flawed_dir).last_opened = Some(1_763_000_000);
    hub.refresh();
    hub.add_work(forever(2, WorkKind::Pack, "Saving “Space Miner” to a file", (88, 301)));
    hub.apply(HubAction::ShowPack(flawed_dir.clone()));
    let Some(piska_hub::Dialog::Pack(pack)) = hub.dialog().cloned() else {
        panic!();
    };
    let mut pack = *pack;
    pack.out = root.join("space_miner.pks").display().to_string();
    hub.apply(HubAction::StartPack(Box::new(pack)));
    hub.wait_for_works_except(2);
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        hub.update();
    }
    shoot(&mut hub, "12_task_rows");
    hub.apply(HubAction::CancelWork(2));
    hub.wait_for_works();

    drop(hub);
    for d in [config, root] {
        let _ = std::fs::remove_dir_all(d);
    }
}
