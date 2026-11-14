//! Русский установщик без GPU: на каждой странице и в каждом окне нет английских надписей
//! (кроме данных: имён программ, путей, команд — они моноширинные — и т. п.).

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use common::*;
use engine_editor::kit::i18n::{self, brand, Language};
use piska_installer::app::{Dialog, Page};
use piska_installer::diag::{Row, RowStatus};
use piska_installer::{Action, Installer, InstallerOptions};

fn is_data(text: &str) -> bool {
    let text = text.trim();
    const ALLOWED: &[&str] = &[
        "Git", "Rust", "CMake", "Ninja", "JDK", "Visual Studio Build Tools", "Android SDK", "Windows", "Linux", "macOS", "PowerShell", "MIT",
        "Apache-2.0", "English", "zig", "Core", "UAC", "OK", "GPU", "PATH", "Vulkan",
    ];
    ALLOWED.iter().any(|a| text.starts_with(a))
        || [brand::ENGINE_NAME, brand::HUB_NAME, brand::EDITOR_NAME, brand::MODELER_NAME].iter().any(|b| text.starts_with(b))
        || text.contains(['/', '\\', '_', '{'])
        || text.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_uppercase())
        || text.starts_with(['#', '+', '-', '.', '<', '·', '→'])
}

fn leaks(hands: &Hands) -> BTreeSet<String> {
    hands
        .texts()
        .into_iter()
        .filter(|(_, mono)| !mono)
        .map(|(t, _)| t.trim().to_string())
        .filter(|t| !t.is_empty() && !t.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c)))
        .filter(|t| t.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 3 && !is_data(t))
        .collect()
}

fn doctor_rows() -> Vec<Row> {
    vec![
        Row { component: "core".into(), label: "rustc".into(), status: RowStatus::Ok, version: Some("1.97.1".into()), path: Some("C:\\Users\\x\\.cargo\\bin\\rustc.exe".into()), detail: String::new(), fix: None },
        Row { component: "cpp".into(), label: "ninja".into(), status: RowStatus::Missing, version: None, path: None, detail: "not found in PATH".into(), fix: Some("install.ps1 -Components cpp".into()) },
        Row { component: "web".into(), label: "wasm32-unknown-unknown".into(), status: RowStatus::Old, version: Some("1.70".into()), path: None, detail: String::new(), fix: Some("rustup update".into()) },
    ]
}

#[test]
fn the_russian_installer_has_no_english_labels() {
    let _serial = serial();
    let env = {
        // Прежняя установка: страница Welcome с выбором режима.
        let root = temp("l10n-manifest");
        let (key, dir) = if cfg!(windows) {
            ("LOCALAPPDATA", root.join("Local").join("PiskaEngine").join("install"))
        } else {
            ("XDG_STATE_HOME", root.join("state").join("PiskaEngine").join("install"))
        };
        std::fs::create_dir_all(&dir).unwrap();
        if cfg!(windows) {
            std::fs::write(dir.join("manifest.json"), r#"{"version":1,"entries":[{"kind":"winget","id":"Kitware.CMake","detail":"","previous":"","component":"cpp","time":"t"}]}"#).unwrap();
        } else {
            std::fs::write(dir.join("manifest.tsv"), "rust-target\twasm32-unknown-unknown\t\t\tweb\tt\n").unwrap();
        }
        vec![(key.to_string(), if cfg!(windows) { root.join("Local") } else { root.join("state") }.display().to_string())]
    };
    let (mut installer, _) = fake_installer(
        Scenario::Hang,
        100 << 20,
        InstallerOptions {
            env,
            language: Some(Language::Russian),
            free_bytes: Some(100 << 20),
            doctor: Some(doctor_rows()),
            still_working: Some(Duration::from_millis(300)),
            components: Some(vec!["core".into(), "android".into(), "engine".into()]),
            ..InstallerOptions::default()
        },
    );
    let mut hands = Hands::new(&mut installer);
    let mut found = BTreeSet::new();
    let check = |found: &mut BTreeSet<String>, hands: &mut Hands, installer: &mut Installer, what: &str| {
        hands.frames(installer, 3);
        found.extend(leaks(hands).into_iter().map(|t| format!("{t}   [{what}]")));
    };
    hands.until(&mut installer, Duration::from_secs(20), "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    check(&mut found, &mut hands, &mut installer, "welcome");
    installer.apply(Action::SetMode(piska_installer::Mode::Install));
    installer.apply(Action::Next);
    check(&mut found, &mut hands, &mut installer, "components");
    installer.apply(Action::Next);
    hands.ctx.data_mut(|d| d.insert_temp(egui::Id::new("installer-advanced"), true));
    check(&mut found, &mut hands, &mut installer, "options");
    installer.apply(Action::SetSource(piska_installer::app::Source::Download));
    check(&mut found, &mut hands, &mut installer, "options: download");
    installer.apply(Action::SetSource(piska_installer::app::Source::Existing));
    installer.apply(Action::Next);
    check(&mut found, &mut hands, &mut installer, "licences");
    for l in installer.licences_needed() {
        installer.apply(Action::AcceptLicence(Installer::licence_key(&l), true));
    }
    installer.apply(Action::Next);
    hands.until(&mut installer, Duration::from_secs(20), "the plan", |i| i.plan.as_ref().is_some_and(|p| !p.running()));
    check(&mut found, &mut hands, &mut installer, "review");
    installer.apply(Action::Install);
    check(&mut found, &mut hands, &mut installer, "dialog: elevation");
    installer.apply(Action::InstallConfirmed);
    hands.until(&mut installer, Duration::from_secs(20), "the long step", |i| i.hit("still-working").is_some());
    hands.ctx.data_mut(|d| d.insert_temp(egui::Id::new("installer-log-open"), true));
    check(&mut found, &mut hands, &mut installer, "installing");
    installer.apply(Action::Cancel);
    check(&mut found, &mut hands, &mut installer, "dialog: cancel");
    installer.apply(Action::CancelConfirmed);
    hands.until(&mut installer, Duration::from_secs(20), "the stop", |i| !i.run.as_ref().unwrap().is_running());
    check(&mut found, &mut hands, &mut installer, "cancelled");
    // Вопрос скрипта, обзор каталогов.
    installer.dialog = Some(Dialog::Prompt(Box::new(piska_installer::protocol::Prompt {
        kind: "confirm".into(),
        key: Some("prefs-overwrite".into()),
        subject: Some("tool_cargo".into()),
        ..Default::default()
    })));
    check(&mut found, &mut hands, &mut installer, "dialog: question");
    installer.dialog = None;
    installer.page = Page::Options;
    installer.apply(Action::Browse(piska_installer::app::BrowseTarget::InstallDir));
    hands.frames(&mut installer, 3);
    // Имена каталогов в обзоре — данные.
    let folders: Vec<String> = match &installer.dialog {
        Some(Dialog::Browse(_, b)) => b.entries.iter().map(|(name, _, _)| name.clone()).collect(),
        _ => Vec::new(),
    };
    found.extend(leaks(&hands).into_iter().filter(|t| !folders.contains(t)).map(|t| format!("{t}   [dialog: browse]")));
    installer.apply(Action::CloseDialog);
    // Итог с таблицей doctor.
    installer.page = Page::Finish;
    check(&mut found, &mut hands, &mut installer, "finish");
    // Удаление.
    installer.mode = piska_installer::Mode::Uninstall;
    installer.apply(Action::GoTo(Page::Welcome));
    installer.page = Page::Uninstall;
    installer.apply(Action::SetMode(piska_installer::Mode::Uninstall));
    hands.until(&mut installer, Duration::from_secs(20), "the uninstall plan", |i| i.uninstall_plan.as_ref().is_some_and(|p| !p.running()));
    check(&mut found, &mut hands, &mut installer, "uninstall");
    drop(installer);
    i18n::set(Language::English);
    assert!(found.is_empty(), "English text in the Russian installer ({}):\n{}", found.len(), found.iter().cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn every_component_and_step_has_a_russian_name() {
    let _serial = serial();
    i18n::set(Language::Russian);
    for c in piska_installer::packages::Packages::embedded().components {
        let name = piska_installer::component_name(&c.id);
        assert!(name.chars().any(|ch| ('\u{0400}'..='\u{04ff}').contains(&ch)) || is_data(name), "{} -> {name}", c.id);
    }
    for (id, title) in [("core.rustup", "Rust (rustup, stable, MSVC)"), ("engine.build", "Build hub, editor, export tool and modeler (release)"), ("web.target", "Rust target wasm32-unknown-unknown")] {
        let t = piska_installer::step_title(id, title);
        assert!(t.chars().any(|ch| ('\u{0400}'..='\u{04ff}').contains(&ch)), "{id} -> {t}");
    }
    i18n::set(Language::English);
    assert_eq!(piska_installer::step_title("web.target", "Rust target wasm32-unknown-unknown"), "Rust target wasm32-unknown-unknown");
}
