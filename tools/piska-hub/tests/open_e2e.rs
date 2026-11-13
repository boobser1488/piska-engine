//! Open по-настоящему: хаб собирает программу редактора `games/template` через cargo,
//! разбирает вывод, запускает редактор, прячется, следит за ним и появляется снова,
//! когда редактор закрылся.
//!
//! Открывает окно редактора на `ENGINE_FRAMES` кадров (без звука), поэтому `#[ignore]`:
//!
//! ```text
//! cargo test -p piska-hub --test open_e2e -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use piska_hub::{paths, Hub, HubAction, HubOptions};

#[test]
#[ignore = "builds and opens the template's editor window for a few seconds; run with --ignored"]
fn open_builds_the_editor_hides_the_hub_and_comes_back() {
    // Редактор наследует окружение хаба: закроется сам и без звука.
    std::env::set_var("ENGINE_FRAMES", "90");
    std::env::set_var("ENGINE_AUDIO", "off");
    let config = std::env::temp_dir().join(format!("piska-hub-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    let template = paths::find_engine_root().unwrap().join("games").join("template");
    let mut hub = Hub::new(HubOptions {
        config_dir: Some(config.clone()),
        ..HubOptions::default()
    });
    hub.apply(HubAction::RebuildAndOpen(template.clone()));
    assert!(hub.job().is_some_and(|job| job.is_building()), "{:?}", hub.status().map(|s| &s.text));

    // Сборка (может быть долгой: cargo ждёт общий каталог сборки).
    let started = Instant::now();
    let mut last_report = Instant::now();
    let mut max_fraction = 0.0_f32;
    while hub.job().is_some_and(|job| job.is_building()) {
        hub.update();
        if let Some(fraction) = hub.job().and_then(|job| job.progress.fraction()) {
            max_fraction = max_fraction.max(fraction);
        }
        if last_report.elapsed() > Duration::from_secs(20) {
            last_report = Instant::now();
            let job = hub.job().unwrap();
            eprintln!(
                "[{:>4}s] {} artifacts / {:?} total, current {:?}",
                started.elapsed().as_secs(),
                job.progress.artifacts,
                job.progress.total,
                job.progress.current
            );
        }
        assert!(started.elapsed() < Duration::from_secs(40 * 60), "the build did not finish in 40 minutes");
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(job) = hub.job() {
        let tail: Vec<&String> = job.progress.tail.iter().rev().take(40).collect();
        panic!("the editor did not start: {:?}\n{tail:#?}", job.state);
    }
    eprintln!("built and launched in {:.0} s (max progress {:.0} %)", started.elapsed().as_secs_f32(), max_fraction * 100.0);
    let request = hub.window_request();
    assert!(!request.visible, "the hub hides while the editor is open");
    assert!(hub.status().unwrap().text.contains("open in the editor"), "{}", hub.status().unwrap().text);

    // Второй раз тот же проект не открывается.
    hub.apply(HubAction::Open(template.clone()));
    assert!(hub.status().unwrap().text.contains("already open"), "{}", hub.status().unwrap().text);
    assert!(hub.job().is_none());

    // Редактор закрывается сам после ENGINE_FRAMES кадров — хаб возвращается.
    let waiting = Instant::now();
    loop {
        hub.update();
        let request = hub.window_request();
        if request.visible {
            assert!(request.focus, "the hub comes to the front");
            break;
        }
        assert!(waiting.elapsed() < Duration::from_secs(180), "the editor did not exit after ENGINE_FRAMES");
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("editor ran {:.1} s; the hub is visible again", waiting.elapsed().as_secs_f32());
    let (saved, _) = piska_hub::config::HubConfig::load(&config.join(paths::CONFIG_FILE));
    let entry = saved.entry(&template).expect("the opened project is remembered");
    assert!(entry.last_opened.is_some());
    assert!(entry.build_units.is_some_and(|n| n > 10), "the unit count is kept for the next progress bar");
    drop(hub);
    std::fs::remove_dir_all(&config).unwrap();
}

/// «Open Last Build»: готовая программа редактора шаблона запускается сразу, без cargo;
/// хаб прячется и возвращается, когда редактор закрылся (`ENGINE_FRAMES`, без звука).
#[test]
#[ignore = "opens the template's editor window for a few seconds; run with --ignored"]
fn open_last_build_starts_the_editor_without_cargo() {
    std::env::set_var("ENGINE_FRAMES", "60");
    std::env::set_var("ENGINE_AUDIO", "off");
    let config = std::env::temp_dir().join(format!("piska-hub-e2e-last-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    let template = paths::find_engine_root().unwrap().join("games").join("template");
    let mut hub = Hub::new(HubOptions {
        config_dir: Some(config.clone()),
        ..HubOptions::default()
    });
    hub.wait_for_refresh();
    let exe = hub
        .last_build(&template, &piska_hub::jobs::JobKind::Open)
        .expect("template-editor was built before (run open_e2e first)");
    eprintln!("last build: {}", exe.display());
    let started = Instant::now();
    hub.apply(HubAction::OpenLastBuild(template.clone()));
    let launch = started.elapsed();
    assert!(hub.job().is_none(), "no cargo for Open Last Build");
    assert!(hub.status().unwrap().text.contains("open in the editor"), "{}", hub.status().unwrap().text);
    assert!(!hub.window_request().visible, "the hub hides while the editor is open");
    eprintln!("editor process started {:.0} ms after the click", launch.as_secs_f64() * 1000.0);
    let waiting = Instant::now();
    loop {
        hub.update();
        if hub.window_request().visible {
            break;
        }
        assert!(waiting.elapsed() < Duration::from_secs(180), "the editor did not exit after ENGINE_FRAMES");
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("editor ran {:.1} s; the hub is visible again", waiting.elapsed().as_secs_f32());
    drop(hub);
    std::fs::remove_dir_all(&config).unwrap();
}
