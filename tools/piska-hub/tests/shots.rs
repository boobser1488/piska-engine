//! Снимки хаба без окна (GPU): список, сетка, мастер, ход сборки, примеры, настройки,
//! диалог «Add Existing». Картинки пишутся в `PISKA_HUB_SHOTS` (если задан) — их
//! смотрят глазами; тест проверяет, что они нарисованы и что панели на своих местах.
//!
//! Все обращения к GPU — по одному (драйвер не любит параллельные устройства).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use piska_hub::config::{SortOrder, ViewMode};
use piska_hub::{capture, paths, Hub, HubAction, HubOptions, Page};

static GPU: Mutex<()> = Mutex::new(());

const W: u32 = 1440;
const H: u32 = 900;
/// «Сейчас» для подписей времени: снимки не зависят от часов.
const NOW: u64 = 1_763_040_000;

fn out_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("PISKA_HUB_SHOTS")?);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-shots-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(image: &image::RgbaImage, name: &str) {
    if let Some(dir) = out_dir() {
        image.save(dir.join(format!("{name}.png"))).unwrap();
    }
}

/// Цвет пикселя примерно равен ожидаемому.
fn near(image: &image::RgbaImage, x: u32, y: u32, want: egui::Color32) -> bool {
    let p = image.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
        .iter()
        .zip([want.r(), want.g(), want.b()])
        .all(|(a, b)| a.abs_diff(b) <= 6)
}

/// Различных цветов в картинке (пустой кадр — 1–2).
fn colors(image: &image::RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels().step_by(7) {
        seen.insert(pixel.0);
    }
    seen.len()
}

/// Хаб с предсказуемым списком: время «сейчас», открывавшиеся и закреплённые проекты,
/// один пропавший и один добавленный снаружи.
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
    let created = piska_hub::templates::plan(
        &piska_hub::templates::NewProject {
            name: "space_miner".into(),
            location: outside.to_path_buf(),
            template: piska_hub::templates::Template::Empty3D,
            cpp: true,
        },
        &root,
    )
    .unwrap();
    piska_hub::templates::create(&created, &root).unwrap();
    config.touch(&created.dir, NOW - 50 * 60);
    // Каталог, где «хаб создавал проекты»: из него можно удалять (снимок окна подтверждения).
    config.settings.project_roots.push(outside.to_path_buf());
    config.save(&config_dir.join(paths::CONFIG_FILE)).unwrap();
    let mut hub = Hub::new(HubOptions {
        config_dir: Some(config_dir.to_path_buf()),
        dry_run: true,
        ..HubOptions::default()
    });
    hub.set_now(NOW);
    hub
}

fn shoot(hub: &mut Hub, frames: u32, input: impl FnMut(u32, &mut Hub) -> Vec<egui::Event>) -> image::RgbaImage {
    capture(hub, W, H, frames, input).expect("the hub renders without a window")
}

#[test]
fn hub_screens_render() {
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    // PISKA_HUB_LANG=ru — те же снимки на русском (английский — по умолчанию).
    if let Some(language) = std::env::var("PISKA_HUB_LANG").ok().and_then(|code| engine_editor::kit::i18n::Language::from_code(&code)) {
        engine_editor::kit::i18n::set(language);
    }
    let config = temp("config");
    let outside = temp("outside");
    let mut hub = staged_hub(&config, &outside);
    let theme_header = engine_editor::kit::theme::BG_HEADER;
    let theme_panel = engine_editor::kit::theme::BG_PANEL;

    // 1. Список проектов (с пропавшим проектом).
    let list = shoot(&mut hub, 6, |_, _| vec![]);
    save(&list, "01_projects_list");
    assert!(near(&list, 40, H - 120, theme_header), "the navigation panel is on the left");
    assert!(near(&list, W - 10, 300, theme_panel) || colors(&list) > 50);
    assert!(colors(&list) > 200, "the list has thumbnails and text");

    // 1b. Наведение на строку и меню «⋯».
    let template = paths::find_engine_root().unwrap().join("games").join("template");
    let _ = template;
    let hovered = shoot(&mut hub, 20, |frame, _| {
        // Кнопка «⋯» второй строки (template): заголовок 76 + панель 44 + шапка 30 + строка 64.
        let pos = egui::pos2(W as f32 - 41.0, 76.0 + 44.0 + 30.0 + 64.0 + 32.0);
        match frame {
            2 => vec![egui::Event::PointerMoved(pos)],
            3 => vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            4 => vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            _ => vec![],
        }
    });
    save(&hovered, "02_projects_menu");

    // 2. Сетка, порядок по имени.
    hub.apply(HubAction::SetView(ViewMode::Grid));
    hub.apply(HubAction::SetSort(SortOrder::Name));
    let grid = shoot(&mut hub, 6, |_, _| vec![]);
    save(&grid, "03_projects_grid");
    assert!(colors(&grid) > 200);
    hub.apply(HubAction::SetView(ViewMode::List));
    hub.apply(HubAction::SetSort(SortOrder::LastOpened));

    // 3. Мастер нового проекта: проект снаружи движка — предупреждение о первой сборке.
    hub.apply(HubAction::Navigate(Page::NewProject));
    hub.wizard_mut().name = "asteroid_belt".into();
    hub.wizard_mut().location = outside.display().to_string();
    hub.wizard_mut().template = piska_hub::templates::Template::Empty3D;
    hub.wizard_mut().cpp = true;
    let wizard = shoot(&mut hub, 6, |_, _| vec![]);
    save(&wizard, "04_new_project");
    hub.wizard_mut().location = paths::find_engine_root().unwrap().join("games").display().to_string();
    hub.wizard_mut().template = piska_hub::templates::Template::Starfall;
    hub.wizard_mut().cpp = false;
    let wizard_ws = shoot(&mut hub, 6, |_, _| vec![]);
    save(&wizard_ws, "05_new_project_workspace");

    // 4. Ход сборки: открытие lantern, вывод cargo подаётся строками.
    hub.apply(HubAction::Navigate(Page::Projects));
    let lantern = paths::find_engine_root().unwrap().join("games").join("lantern");
    hub.apply(HubAction::RebuildAndOpen(lantern.clone()));
    hub.apply(HubAction::ShowJob(true));
    hub.job_mut().expect("Rebuild and Open starts a build").feed_stderr("    Blocking waiting for file lock on build directory");
    let waiting = shoot(&mut hub, 6, |_, _| vec![]);
    save(&waiting, "06a_build_waiting_for_lock");
    {
        let job = hub.job_mut().expect("the build goes on");
        for (i, name) in ["proc-macro2", "unicode-ident", "serde", "glam", "bytemuck", "wgpu-types", "naga", "wgpu-core", "rapier2d", "egui"].iter().enumerate() {
            job.feed_stderr(&format!("   Compiling {name} v1.0.{i}"));
            job.feed_stdout(&format!(
                r#"{{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#{name}@1.0.{i}","target":{{"name":"{name}"}},"fresh":{}}}"#,
                i < 6
            ));
        }
        job.feed_stdout(r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused variable: `t`","rendered":"warning: unused variable: `t`\n --> games/lantern/src/level.rs:88:13\n","spans":[{"file_name":"games/lantern/src/level.rs","line_start":88,"column_start":13,"is_primary":true}]}}"#);
        job.feed_stderr("   Compiling engine-render v0.0.1 (crates/engine-render)\r    Building [=============>          ] 214/356: engine-render, engine-physics, engine-ui");
    }
    let progress = shoot(&mut hub, 6, |_, _| vec![]);
    save(&progress, "06_build_progress");
    // Та же сборка в фоне: список работает, полоса задач внизу.
    hub.apply(HubAction::ShowJob(false));
    let background = shoot(&mut hub, 6, |_, _| vec![]);
    save(&background, "06b_build_in_background");
    hub.apply(HubAction::ShowJob(true));
    {
        let job = hub.job_mut().unwrap();
        job.feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///e/games/lantern#0.0.1","message":{"level":"error","message":"cannot find value `jump_heigth` in this scope","rendered":"error[E0425]: cannot find value `jump_heigth` in this scope\n  --> games/lantern/src/player.rs:142:27\n    |\n142 |         body.velocity.y = jump_heigth;\n    |                           ^^^^^^^^^^^ help: a field with a similar name exists: `jump_height`\n","spans":[{"file_name":"games/lantern/src/player.rs","line_start":142,"column_start":27,"is_primary":true}]}}"#);
        job.feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///e/games/lantern#0.0.1","message":{"level":"error","message":"mismatched types: expected `f32`, found `f64`","rendered":"error[E0308]: mismatched types\n  --> games/lantern/src/enemies.rs:57:20\n","spans":[{"file_name":"games/lantern/src/enemies.rs","line_start":57,"column_start":20,"is_primary":true}]}}"#);
        job.feed_stdout(r#"{"reason":"build-finished","success":false}"#);
        job.simulate_exit(false);
    }
    let failed = shoot(&mut hub, 6, |_, _| vec![]);
    save(&failed, "07_build_failed");

    // 4b. Движок сейчас не собирается — ошибка не в проекте.
    hub.apply(HubAction::RetryJob);
    hub.apply(HubAction::ShowJob(true));
    {
        let job = hub.job_mut().expect("Try Again rebuilds");
        job.feed_stderr("   Compiling engine-render v0.0.1 (crates/engine-render)");
        job.feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///e/crates/engine-render#0.0.1","message":{"level":"error","message":"cannot find type `TerrainPass` in module `renderer`","rendered":"error[E0412]: cannot find type `TerrainPass` in module `renderer`\n  --> crates/engine-render/src/renderer.rs:418:21\n","spans":[{"file_name":"crates/engine-render/src/renderer.rs","line_start":418,"column_start":21,"is_primary":true}]}}"#);
        job.feed_stdout(r#"{"reason":"build-finished","success":false}"#);
        job.simulate_exit(false);
    }
    let broken = shoot(&mut hub, 6, |_, _| vec![]);
    save(&broken, "07b_engine_does_not_compile");
    hub.apply(HubAction::CloseJob);

    // 4c. Prebuild all: общий ход, затем итог.
    hub.apply(HubAction::PrebuildAll);
    hub.apply(HubAction::ShowJob(true));
    if hub.prebuild().is_some() {
        if let Some(job) = hub.job_mut() {
            job.feed_stderr("   Compiling engine-core v0.0.1 (crates/engine-core)\r    Building [=======>       ] 120/320: engine-core, rapier2d");
        }
        let running = shoot(&mut hub, 6, |_, _| vec![]);
        save(&running, "12_prebuild_running");
        let mut step = 0;
        while hub.prebuild().is_some_and(|p| !p.finished) && step < 50 {
            step += 1;
            let job = hub.job_mut().unwrap();
            let ok = step != 2;
            if !ok {
                job.feed_stdout(r#"{"reason":"compiler-message","package_id":"path+file:///e/crates/engine-ui#0.0.1","message":{"level":"error","message":"unresolved import `crate::layout::Flex`","rendered":"error","spans":[{"file_name":"crates/engine-ui/src/lib.rs","line_start":31,"column_start":5,"is_primary":true}]}}"#);
            }
            job.feed_stdout(&format!(r#"{{"reason":"build-finished","success":{ok}}}"#));
            job.simulate_exit(ok);
            hub.update();
        }
        hub.apply(HubAction::ShowJob(true));
        let done = shoot(&mut hub, 6, |_, _| vec![]);
        save(&done, "13_prebuild_done");
        hub.apply(HubAction::CloseJob);
    }

    // 5. Примеры и документация.
    hub.apply(HubAction::Navigate(Page::Samples));
    let samples = shoot(&mut hub, 6, |_, _| vec![]);
    save(&samples, "08_samples");
    hub.apply(HubAction::Navigate(Page::Tools));
    let tools = shoot(&mut hub, 6, |_, _| vec![]);
    save(&tools, "14_tools");
    assert!(colors(&samples) > 1000, "sample pictures are drawn");

    // 6. Настройки и диалог «Add Existing».
    hub.apply(HubAction::Navigate(Page::Settings));
    let settings = shoot(&mut hub, 6, |_, _| vec![]);
    save(&settings, "09_settings");
    hub.apply(HubAction::Navigate(Page::Projects));
    hub.apply(HubAction::ShowAdd { locate: None });
    let add = shoot(&mut hub, 6, |_, _| vec![]);
    save(&add, "10_add_existing");
    hub.apply(HubAction::CloseDialog);
    hub.apply(HubAction::AskDelete(outside.join("space_miner")));
    let delete = shoot(&mut hub, 6, |_, _| vec![]);
    save(&delete, "11_delete_confirm");
    hub.apply(HubAction::CloseDialog);

    drop(hub);
    std::fs::remove_dir_all(&config).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}
