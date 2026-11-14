//! Снимки каждой страницы без окна (GPU) на английском и русском при масштабе 1.0 и 1.5:
//! картинки пишутся в `PISKA_INSTALLER_SHOTS` (если задан) — их смотрят глазами; тест
//! проверяет, что они нарисованы и что панели на своих местах. Скрипты — поддельные.
//!
//! Все обращения к GPU — по одному (драйвер не любит параллельные устройства).

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use engine_editor::kit::i18n::{self, Language};
use piska_installer::app::{BrowseTarget, Page, Source};
use piska_installer::diag::{Row, RowStatus};
use piska_installer::capture::Capturer;
use piska_installer::{Action, Installer, InstallerOptions};

/// Логический размер окна (как у окна по умолчанию).
const W: f32 = 1060.0;
const H: f32 = 700.0;

fn out_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("PISKA_INSTALLER_SHOTS")?);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn colors(image: &image::RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels().step_by(7) {
        seen.insert(pixel.0);
    }
    seen.len()
}

fn shoot(gpu: &mut Capturer, installer: &mut Installer, scale: f32, name: &str) {
    installer.ui_scale = scale;
    let (w, h) = ((W * scale) as u32, (H * scale) as u32);
    let image = gpu.capture(installer, w, h, 8, |_, _| vec![]).expect("GPU capture");
    // Левая колонка и страница — разного цвета; картинка не пустая.
    let nav = image.get_pixel((40.0 * scale) as u32, h / 2).0;
    let page = image.get_pixel(w - (20.0 * scale) as u32, (130.0 * scale) as u32).0;
    assert_ne!(nav, page, "{name}: the navigation column is drawn");
    assert!(colors(&image) > 40, "{name}: the picture has content");
    if let Some(dir) = out_dir() {
        image.save(dir.join(format!("{name}.png"))).unwrap();
    }
}

fn doctor_rows() -> Vec<Row> {
    let row = |component: &str, label: &str, status, version: Option<&str>, path: Option<&str>, detail: &str, fix: Option<&str>| Row {
        component: component.into(),
        label: label.into(),
        status,
        version: version.map(Into::into),
        path: path.map(Into::into),
        detail: detail.into(),
        fix: fix.map(Into::into),
    };
    vec![
        row("core", "rustc", RowStatus::Ok, Some("1.97.1"), Some("C:\\Users\\dev\\.cargo\\bin\\rustc.exe"), "", None),
        row("core", "cargo", RowStatus::Ok, Some("1.97.1"), Some("C:\\Users\\dev\\.cargo\\bin\\cargo.exe"), "", None),
        row("core", "MSVC C++ tools", RowStatus::Ok, Some("17.14"), Some("C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools"), "", None),
        row("core", "git", RowStatus::Ok, Some("2.55.0"), Some("C:\\Program Files\\Git\\cmd\\git.exe"), "", None),
        row("cpp", "cmake", RowStatus::Ok, Some("4.4.2"), Some("C:\\Program Files\\CMake\\bin\\cmake.exe"), "", None),
        row("cpp", "ninja", RowStatus::Missing, None, None, "not found in PATH", Some("install.ps1 -Components cpp")),
        row("web", "wasm32-unknown-unknown", RowStatus::Ok, None, None, "installed", None),
    ]
}

/// Все страницы одного языка и масштаба.
fn pages(gpu: &mut Capturer, language: Language, scale: f32) {
    let lang = language.code();
    let tag = |page: &str| format!("{page}-{lang}-{}", if scale > 1.2 { "150" } else { "100" });
    let (mut installer, _) = fake_installer(
        Scenario::Hang,
        1 << 40,
        InstallerOptions {
            language: Some(language),
            free_bytes: Some(120 << 30),
            doctor: Some(doctor_rows()),
            still_working: Some(Duration::from_millis(200)),
            install_dir: Some("C:\\Users\\dev\\AppData\\Local\\PiskaEngine".into()),
            ..InstallerOptions::default()
        },
    );
    installer.wait_idle(Duration::from_secs(20));
    shoot(gpu, &mut installer, scale, &tag("1-welcome"));
    installer.apply(Action::Next);
    installer.apply(Action::Preset(piska_installer::app::Preset::Typical));
    installer.apply(Action::Toggle("engine".into()));
    shoot(gpu, &mut installer, scale, &tag("2-components"));
    installer.apply(Action::Next);
    installer.apply(Action::SetSource(Source::Download));
    shoot(gpu, &mut installer, scale, &tag("3-options"));
    installer.apply(Action::SetSource(Source::Existing));
    installer.apply(Action::Next);
    shoot(gpu, &mut installer, scale, &tag("4-licences"));
    for l in installer.licences_needed() {
        installer.apply(Action::AcceptLicence(Installer::licence_key(&l), true));
    }
    installer.apply(Action::Next);
    installer.wait_idle(Duration::from_secs(20));
    shoot(gpu, &mut installer, scale, &tag("5-review"));
    installer.apply(Action::Install);
    shoot(gpu, &mut installer, scale, &tag("5b-elevation-dialog"));
    installer.apply(Action::InstallConfirmed);
    let end = std::time::Instant::now() + Duration::from_secs(20);
    while installer.run.as_ref().is_some_and(|r| !r.steps.iter().any(|s| s.id == "cpp.ninja" && s.state == piska_installer::app::StepState::Running)) {
        installer.update();
        assert!(std::time::Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(250));
    shoot(gpu, &mut installer, scale, &tag("6-installing"));
    installer.apply(Action::CancelConfirmed);
    installer.wait_idle(Duration::from_secs(20));
    shoot(gpu, &mut installer, scale, &tag("6b-cancelled"));
    installer.page = Page::Finish;
    shoot(gpu, &mut installer, scale, &tag("7-finish"));
    installer.page = Page::Options;
    installer.apply(Action::Browse(BrowseTarget::InstallDir));
    shoot(gpu, &mut installer, scale, &tag("3b-browse"));
    drop(installer);

    // Сбой посередине.
    let (mut failed, _) = fake_installer(Scenario::FailInMiddle, 1 << 40, InstallerOptions { language: Some(language), free_bytes: Some(120 << 30), ..InstallerOptions::default() });
    failed.wait_idle(Duration::from_secs(20));
    failed.apply(Action::Next);
    failed.apply(Action::Preset(piska_installer::app::Preset::Typical));
    failed.apply(Action::Next);
    failed.apply(Action::Next);
    failed.apply(Action::AcceptLicence("vs-buildtools".into(), true));
    failed.apply(Action::Next);
    failed.wait_idle(Duration::from_secs(20));
    failed.apply(Action::InstallConfirmed);
    failed.wait_idle(Duration::from_secs(20));
    shoot(gpu, &mut failed, scale, &tag("6c-failed"));
    drop(failed);

    // Прежняя установка: Welcome с выбором режима и страница удаления.
    let root = temp(&format!("shots-manifest-{lang}-{scale}"));
    let env = if cfg!(windows) {
        let dir = root.join("Local").join("PiskaEngine").join("install");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), r#"{"version":1,"entries":[{"kind":"winget","id":"Kitware.CMake","detail":"","previous":"","component":"cpp","time":"t"}]}"#).unwrap();
        std::fs::write(dir.join("state.json"), r#"{"version":1,"components":["core","cpp"],"steps":[]}"#).unwrap();
        vec![("LOCALAPPDATA".to_string(), root.join("Local").display().to_string())]
    } else {
        let dir = root.join("state").join("PiskaEngine").join("install");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.tsv"), "rust-target\twasm32-unknown-unknown\t\t\tweb\tt\n").unwrap();
        vec![("XDG_STATE_HOME".to_string(), root.join("state").display().to_string())]
    };
    let (mut existing, _) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { language: Some(language), env, free_bytes: Some(120 << 30), ..InstallerOptions::default() });
    existing.wait_idle(Duration::from_secs(20));
    shoot(gpu, &mut existing, scale, &tag("8-welcome-existing"));
    existing.apply(Action::SetMode(piska_installer::Mode::Uninstall));
    existing.apply(Action::Next);
    existing.wait_idle(Duration::from_secs(20));
    shoot(gpu, &mut existing, scale, &tag("9-uninstall"));
    drop(existing);
    i18n::set(Language::English);
}

#[test]
fn every_page_in_english_and_russian_at_scale_1_and_1_5() {
    let _serial = serial();
    let mut gpu = Capturer::new().expect("a GPU");
    for language in [Language::English, Language::Russian] {
        for scale in [1.0, 1.5] {
            pages(&mut gpu, language, scale);
        }
    }
}
