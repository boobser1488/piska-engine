//! Запуск установщика без окна и GPU — тот же конечный автомат, что ведёт окно
//! (`startup::start` / `startup::frame`): первый показанный кадр — заставка в цветах темы с
//! текстом шага; окно не показывают раньше полного кадра с темой; мастер появляется, когда
//! работа кончилась; сбой при запуске — экран ошибки с текстом, а не зависшее окно.
//!
//! НИЧЕГО НЕ УСТАНАВЛИВАЕТСЯ: установщик строится на поддельном скрипте.

mod common;

use std::sync::mpsc;
use std::time::Duration;

use common::*;
use engine_editor::kit::boot::{painted_fills, painted_texts, Boot, BootState};
use engine_editor::kit::theme;
use piska_installer::startup;
use piska_installer::{Installer, InstallerOptions, Page};

fn run(ctx: &egui::Context, boot: &mut Boot<Installer>, time: f64) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        time: Some(time),
        predicted_dt: 1.0 / 60.0,
        ..egui::RawInput::default()
    };
    ctx.run(input, |ctx| startup::frame(boot, ctx))
}

fn until(ctx: &egui::Context, boot: &mut Boot<Installer>, time: &mut f64, limit: f64, mut done: impl FnMut(&Boot<Installer>) -> bool) -> egui::FullOutput {
    let mut full = run(ctx, boot, *time);
    let end = *time + limit;
    while !done(boot) && *time < end {
        *time += 0.016;
        std::thread::sleep(Duration::from_millis(4));
        if let Some(installer) = boot.value_mut() {
            installer.update();
        }
        full = run(ctx, boot, *time);
    }
    full
}

#[test]
fn the_first_presented_frame_is_the_themed_splash_with_a_status_line() {
    let _serial = serial();
    let (release, gate) = mpsc::channel::<()>();
    let mut boot = startup::start_with(move |progress| {
        progress.set_step("Looking for installed tools…");
        gate.recv().ok();
        Ok(fake_installer(Scenario::Success, 500 << 30, InstallerOptions::default()).0)
    });
    let ctx = egui::Context::default();
    run(&ctx, &mut boot, 0.0);
    assert!(!boot.themed(), "the theme-setting frame is never shown");
    let full = run(&ctx, &mut boot, 0.016);
    assert!(boot.themed(), "a full themed frame: the window may be shown now");
    assert_eq!(boot.state(), BootState::Loading);
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t.contains("Setup")), "the name is on the splash: {texts:?}");
    assert!(texts.iter().any(|t| t.contains("Looking for installed tools")), "the real step is on the splash: {texts:?}");
    let (rect, fill) = painted_fills(&full)[0];
    assert!(rect.width() >= SCREEN.x - 1.0 && rect.height() >= SCREEN.y - 1.0);
    assert_eq!(fill, theme::BG_APP, "the window background is the theme colour, never white or black");
    let mut time = 0.032;
    let full = until(&ctx, &mut boot, &mut time, 1.0, |_| false);
    assert!(painted_texts(&full).iter().any(|t| t.contains("Looking for installed tools")), "the window keeps drawing while it waits");
    release.send(()).unwrap();
}

#[test]
fn the_wizard_replaces_the_splash_when_the_work_completes() {
    let _serial = serial();
    let (release, gate) = mpsc::channel::<()>();
    let mut boot = startup::start_with(move |_| {
        gate.recv().ok();
        Ok(fake_installer(Scenario::Success, 500 << 30, InstallerOptions::default()).0)
    });
    let ctx = egui::Context::default();
    run(&ctx, &mut boot, 0.0);
    run(&ctx, &mut boot, 0.016);
    release.send(()).unwrap();
    let mut time = 0.032;
    until(&ctx, &mut boot, &mut time, 20.0, |b| b.state() == BootState::Ready);
    assert_eq!(boot.state(), BootState::Ready, "the installer was built in the background");
    assert_eq!(boot.value().unwrap().page, Page::Welcome);
    let full = run(&ctx, &mut boot, time + 0.016);
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t.contains("Welcome") || t.contains("Install")), "the wizard's own screen is drawn: {texts:?}");
    until(&ctx, &mut boot, &mut time, 30.0, |b| !b.splash_visible());
    assert!(!boot.splash_visible());
    assert!(time >= 0.6, "the splash is held for at least 0.6 s (gone at {time:.2} s)");
}

#[test]
fn a_failure_while_loading_shows_an_error_screen_instead_of_hanging() {
    let _serial = serial();
    let mut boot = startup::start_with(|_| Err("cannot read the install manifest: access is denied".to_string()));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    until(&ctx, &mut boot, &mut time, 5.0, |b| b.state() == BootState::Failed);
    assert_eq!(boot.state(), BootState::Failed);
    let full = run(&ctx, &mut boot, time + 0.016);
    let texts = painted_texts(&full);
    assert!(texts.iter().any(|t| t.contains("could not start")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("access is denied")), "the message is on the screen: {texts:?}");
    assert!(texts.iter().any(|t| t == "Close"), "{texts:?}");
    assert!(boot.themed());
}

#[test]
fn a_panic_while_loading_is_an_error_screen_too() {
    let _serial = serial();
    let mut boot = startup::start_with(|_| panic!("the package list is corrupt"));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    until(&ctx, &mut boot, &mut time, 5.0, |b| b.state() == BootState::Failed);
    assert!(boot.error().unwrap().contains("the package list is corrupt"));
}
