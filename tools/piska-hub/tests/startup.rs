//! Запуск хаба без окна и GPU — тот же конечный автомат, что ведёт окно (`startup::start` /
//! `startup::frame`): первый показанный кадр — заставка в цветах темы с текстом шага; окно
//! (`Boot::themed`) не показывают раньше полного кадра с темой; хаб появляется, когда работа
//! кончилась; сбой при запуске — экран ошибки с текстом, а не зависшее окно.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use engine_editor::kit::boot::{painted_fills, painted_texts, Boot, BootState};
use engine_editor::kit::theme;
use piska_hub::startup;
use piska_hub::{Hub, HubOptions};

fn options(config_dir: &Path) -> HubOptions {
    let tools = config_dir.join("tools");
    let _ = std::fs::create_dir_all(&tools);
    HubOptions { config_dir: Some(config_dir.to_path_buf()), dry_run: true, tools_dir: Some(tools), ..HubOptions::default() }
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("piska-hub-startup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(ctx: &egui::Context, boot: &mut Boot<Hub>, time: f64) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
        time: Some(time),
        predicted_dt: 1.0 / 60.0,
        ..egui::RawInput::default()
    };
    ctx.run(input, |ctx| startup::frame(boot, ctx))
}

/// Гоняет кадры, пока `done`, не дольше `limit` секунд модельного времени.
fn until(ctx: &egui::Context, boot: &mut Boot<Hub>, time: &mut f64, limit: f64, mut done: impl FnMut(&Boot<Hub>) -> bool) -> egui::FullOutput {
    let mut full = run(ctx, boot, *time);
    let end = *time + limit;
    while !done(boot) && *time < end {
        *time += 0.016;
        std::thread::sleep(Duration::from_millis(4));
        full = run(ctx, boot, *time);
    }
    full
}

#[test]
fn the_first_presented_frame_is_the_themed_splash_with_a_status_line() {
    let dir = temp("splash");
    let (release, gate) = mpsc::channel::<()>();
    let opts = options(&dir);
    // Загрузка стоит, пока тест не отпустит: заставка видна сколько угодно кадров.
    let mut boot = startup::start_with(move |progress| {
        progress.set_step("Scanning the project folders…");
        gate.recv().ok();
        Ok(Hub::new(opts))
    });
    let ctx = egui::Context::default();
    // Кадр 0 только ставит тему и шрифты: окно ещё прятать.
    run(&ctx, &mut boot, 0.0);
    assert!(!boot.themed(), "the theme-setting frame is never shown");
    let full = run(&ctx, &mut boot, 0.016);
    assert!(boot.themed(), "a full themed frame: the window may be shown now");
    assert_eq!(boot.state(), BootState::Loading, "the hub is still being built");
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t == "Piska Hub"), "the name is on the splash: {texts:?}");
    assert!(texts.iter().any(|t| t.contains("Scanning the project folders")), "the real step is on the splash: {texts:?}");
    let (rect, fill) = painted_fills(&full)[0];
    assert!(rect.width() >= 1279.0 && rect.height() >= 799.0, "the first fill covers the window");
    assert_eq!(fill, theme::BG_APP, "the window background is the theme colour, never white or black");
    // Долго ждём — окно всё равно живое: кадры идут, заставка не пропадает.
    let mut time = 0.032;
    let full = until(&ctx, &mut boot, &mut time, 1.0, |_| false);
    assert!(painted_texts(&full).iter().any(|t| t.contains("Scanning the project folders")));
    assert_eq!(boot.state(), BootState::Loading);
    release.send(()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_hub_replaces_the_splash_when_the_work_completes() {
    let dir = temp("ready");
    let (release, gate) = mpsc::channel::<()>();
    let opts = options(&dir);
    let mut boot = startup::start_with(move |_| {
        gate.recv().ok();
        Ok(Hub::new(opts))
    });
    let ctx = egui::Context::default();
    run(&ctx, &mut boot, 0.0);
    run(&ctx, &mut boot, 0.016);
    release.send(()).unwrap();
    let mut time = 0.032;
    until(&ctx, &mut boot, &mut time, 20.0, |b| b.state() == BootState::Ready);
    assert_eq!(boot.state(), BootState::Ready, "the hub was built in the background");
    // Пока заставка растворяется, под ней уже главный экран; потом она уходит.
    let full = run(&ctx, &mut boot, time + 0.016);
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t == "Projects" || t == "New Project"), "the hub's own screen is drawn: {texts:?}");
    until(&ctx, &mut boot, &mut time, 30.0, |b| !b.splash_visible());
    assert!(!boot.splash_visible(), "the splash is gone once the project scan is done");
    assert!(time >= 0.6, "the splash is held for at least 0.6 s (gone at {time:.2} s)");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failure_while_loading_shows_an_error_screen_instead_of_hanging() {
    let mut boot = startup::start_with(|_| Err("cannot read `hub.ron`: access is denied".to_string()));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let full = until(&ctx, &mut boot, &mut time, 5.0, |b| b.state() == BootState::Failed);
    let _ = full;
    assert_eq!(boot.state(), BootState::Failed);
    let full = run(&ctx, &mut boot, time + 0.016);
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t.contains("Piska Hub could not start")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("access is denied")), "the message is on the screen: {texts:?}");
    assert!(texts.iter().any(|t| t == "Close") && texts.iter().any(|t| t == "Copy details"), "{texts:?}");
    assert!(boot.themed(), "the error screen is also a themed frame");
    assert!(!boot.quit_requested());
}

#[test]
fn a_panic_while_loading_is_an_error_screen_too() {
    let mut boot = startup::start_with(|_| panic!("the project list is corrupt"));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    until(&ctx, &mut boot, &mut time, 5.0, |b| b.state() == BootState::Failed);
    assert_eq!(boot.state(), BootState::Failed);
    assert!(boot.error().unwrap().contains("the project list is corrupt"));
}

#[test]
fn the_real_start_builds_the_hub_in_the_background() {
    let dir = temp("real");
    let mut boot = startup::start(options(&dir));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    until(&ctx, &mut boot, &mut time, 20.0, |b| b.state() != BootState::Loading);
    assert_eq!(boot.state(), BootState::Ready, "{:?}", boot.error());
    let hub = boot.value().unwrap();
    assert_eq!(hub.page(), piska_hub::Page::Projects);
    let _ = std::fs::remove_dir_all(&dir);
}
