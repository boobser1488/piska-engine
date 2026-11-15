//! Мастер руками без окна и GPU: щелчки и клавиши, настоящие события egui. Настоящий скрипт —
//! только в сухом прогоне (`-DryRun -Json`), всё остальное — поддельные скрипты
//! (записанные потоки событий): удача, сбой посередине, вопрос о лицензии, отказ в правах,
//! отмена, мусор в выводе, зависание, падение процесса.
//!
//! НИЧЕГО НЕ УСТАНАВЛИВАЕТСЯ: настоящий скрипт запускается только с `-DryRun`.

mod common;

use std::time::Duration;

use common::*;
use piska_installer::app::{Mode, Outcome, Page, Presence, Source, StepState};
use piska_installer::runner::Purpose;
use piska_installer::{Installer, InstallerOptions};

const LONG: Duration = Duration::from_secs(20);

fn install_through_fake(scenario: Scenario) -> (Hands, Installer, std::sync::Arc<std::sync::Mutex<piska_installer::runner::FakeLog>>) {
    let (mut installer, log) = fake_installer(scenario, 500 << 30, InstallerOptions::default());
    let mut hands = Hands::new(&mut installer);
    walk_to_review(&mut hands, &mut installer);
    assert!(installer.install_ready(), "the plan is ready and the licences are ticked");
    hands.click(&mut installer, "install");
    // Права администратора: окно с объяснением до запроса Windows.
    if installer.dialog.is_some() {
        assert!(hands.has_text("Administrator Rights"), "the elevation dialog explains first");
        hands.click(&mut installer, "dialog-ok");
    }
    // Поддельный скрипт без пауз успевает закончить установку уже за кадры щелчка: страница к этому
    // моменту «Installing» или уже «Finish» — проверка не зависит от скорости машины (раньше тут
    // стояло только `Installing`, и тест иногда падал).
    assert!(matches!(installer.page, Page::Installing | Page::Finish), "the install has started, the page is {:?}", installer.page);
    (hands, installer, log)
}

#[test]
fn typical_install_succeeds_with_the_fake_script_and_passes_every_choice_to_it() {
    let _serial = serial();
    let (mut hands, mut installer, log) = install_through_fake(Scenario::Success);
    hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
    let run = installer.run.as_ref().unwrap();
    assert_eq!(run.outcome, Outcome::Succeeded);
    assert!(run.steps.iter().any(|s| s.id == "core.buildtools" && s.state == StepState::Done));
    assert!(run.steps.iter().any(|s| s.id == "core.git" && s.state == StepState::Skipped));
    let requests = log.lock().unwrap().requests.clone();
    let install = requests.iter().find(|r| r.purpose == Purpose::Install).unwrap();
    assert_eq!(install.options.components, ["core", "cpp", "web"]);
    assert!(!install.options.dry_run);
    assert_eq!(install.options.accept_licences, ["vs-buildtools"], "the ticked licence, and only it");
    assert!(install.options.confirm_elevation, "elevation was confirmed in the dialog");
    assert_eq!(install.options.source_dir, None, "no engine component: no sources");
    // Итог: команды записываются, а не выполняются (sandbox).
    assert!(hands.has_text("is installed"));
    hands.click(&mut installer, "show-log");
    assert_eq!(installer.opened.last().map(String::as_str), Some("C:\\Fake\\logs\\install.log"));
    hands.click(&mut installer, "open-docs");
    assert!(installer.opened.last().unwrap().ends_with("install.md"));
}

#[test]
fn a_failure_in_the_middle_names_the_step_and_retry_resumes() {
    let _serial = serial();
    let (mut hands, mut installer, log) = install_through_fake(Scenario::FailInMiddle);
    hands.until(&mut installer, LONG, "the failure", |i| matches!(i.run.as_ref().map(|r| &r.outcome), Some(Outcome::Failed(_))));
    assert_eq!(installer.page, Page::Installing);
    let Some(Outcome::Failed(f)) = installer.run.as_ref().map(|r| r.outcome.clone()) else { unreachable!() };
    assert_eq!(f.step.as_deref(), Some("cpp.ninja"));
    assert!(f.error.contains("Ninja-build.Ninja"));
    assert!(hands.has_text("Step “Ninja” failed"), "{:?}", hands.texts());
    assert!(hands.has_text("winget: download failed"), "the last log lines are shown");
    assert!(installer.hit("skip").is_none(), "ninja is not skippable");
    hands.click(&mut installer, "copy-error");
    assert!(installer.clipboard.as_deref().unwrap().contains("Ninja-build.Ninja failed"));
    hands.click(&mut installer, "open-log");
    assert_eq!(installer.opened.last().map(String::as_str), Some("C:\\Fake\\logs\\install.log"));
    hands.click(&mut installer, "retry");
    hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
    let requests = log.lock().unwrap().requests.clone();
    let installs: Vec<_> = requests.iter().filter(|r| r.purpose == Purpose::Install).collect();
    assert_eq!(installs.len(), 2);
    assert!(!installs[0].options.resume && installs[1].options.resume, "Retry runs the script with -Resume");
}

#[test]
fn a_skippable_failure_offers_skip_and_passes_skip_steps() {
    let _serial = serial();
    // Подделка: упасть на шаге, который скрипт разрешает пропустить.
    let (backend, log) = piska_installer::runner::Backend::fake(|request| match request.purpose {
        Purpose::Install if request.options.skip_steps.is_empty() => piska_installer::runner::FakeScript::new()
            .out(hello(false, 1 << 40, &request.options.components))
            .out(r#"{"event":"step","phase":"start","step":"core.vulkan","n":1,"total":1,"title":"Vulkan runtime (GPU driver)"}"#)
            .out(r#"{"event":"step","phase":"failed","step":"core.vulkan","n":1,"total":1,"title":"Vulkan runtime (GPU driver)","error":"no driver","log":null,"skippable":true,"elevation_refused":false}"#)
            .exit(1),
        Purpose::Install => piska_installer::runner::FakeScript::new().out(summary(0, false, None, (1, 0, 1, 0))).exit(0),
        _ => dry(request, 1 << 40),
    });
    let mut installer = Installer::new(InstallerOptions {
        backend: Some(backend),
        sandbox: true,
        doctor: Some(vec![]),
        config_dir: Some(temp("skip-cfg")),
        engine_root: Some(repo()),
        components: Some(vec!["core".into()]),
        language: Some(engine_editor::kit::i18n::Language::English),
        ..InstallerOptions::default()
    });
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    installer.accepted.insert("vs-buildtools".into());
    installer.page = Page::Review;
    installer.apply(piska_installer::Action::RefreshPlan);
    hands.until(&mut installer, LONG, "the plan", |i| i.plan.as_ref().is_some_and(|p| !p.running()));
    installer.apply(piska_installer::Action::InstallConfirmed);
    hands.until(&mut installer, LONG, "the failure", |i| matches!(i.run.as_ref().map(|r| &r.outcome), Some(Outcome::Failed(_))));
    hands.click(&mut installer, "skip");
    hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
    let last = log.lock().unwrap().requests.last().cloned().unwrap();
    assert_eq!(last.options.skip_steps, ["core.vulkan"]);
    assert!(last.options.resume);
}

#[test]
fn an_unexpected_licence_prompt_opens_a_dialog_and_the_answer_reaches_the_script() {
    let _serial = serial();
    let (mut hands, mut installer, log) = install_through_fake(Scenario::LicencePrompt);
    hands.until(&mut installer, LONG, "the licence question", |i| i.dialog.is_some());
    assert!(hands.has_text("Do you accept the licence terms of Visual Studio Build Tools?"), "{:?}", hands.texts());
    hands.click(&mut installer, "dialog-ok");
    hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
    assert_eq!(log.lock().unwrap().answers, ["yes"]);
    assert!(installer.accepted.contains("other-licence"), "an accepted licence is remembered for Retry");
}

#[test]
fn declining_the_licence_stops_with_declined() {
    let _serial = serial();
    let (mut hands, mut installer, log) = install_through_fake(Scenario::LicencePrompt);
    hands.until(&mut installer, LONG, "the licence question", |i| i.dialog.is_some());
    hands.click(&mut installer, "dialog-cancel");
    hands.until(&mut installer, LONG, "the end", |i| !i.run.as_ref().unwrap().is_running());
    assert_eq!(installer.run.as_ref().unwrap().outcome, Outcome::Declined("licence".into()));
    assert_eq!(log.lock().unwrap().answers, ["no"]);
    assert!(hands.has_text("Stopped: a licence was not accepted."));
}

#[test]
fn a_refused_elevation_is_explained_and_can_be_retried() {
    let _serial = serial();
    let (mut hands, mut installer, _log) = install_through_fake(Scenario::ElevationRefused);
    hands.until(&mut installer, LONG, "the failure", |i| matches!(i.run.as_ref().map(|r| &r.outcome), Some(Outcome::Failed(_))));
    let Some(Outcome::Failed(f)) = installer.run.as_ref().map(|r| r.outcome.clone()) else { unreachable!() };
    assert!(f.elevation_refused);
    assert!(hands.has_text("Administrator rights were refused"), "{:?}", hands.texts());
    assert!(installer.hit("retry").is_some());
}

#[test]
fn cancel_asks_first_stops_the_script_and_leaves_resume() {
    let _serial = serial();
    let (mut hands, mut installer, log) = install_through_fake(Scenario::Cancel);
    hands.until(&mut installer, LONG, "the long step", |i| i.run.as_ref().is_some_and(|r| r.steps.iter().any(|s| s.id == "cpp.ninja" && s.state == StepState::Running)));
    hands.click(&mut installer, "cancel");
    assert!(hands.has_text("Stop the Installation?"));
    hands.click(&mut installer, "dialog-cancel");
    assert!(installer.run.as_ref().unwrap().is_running(), "Keep Installing keeps it running");
    hands.click(&mut installer, "cancel");
    hands.click(&mut installer, "dialog-ok");
    hands.until(&mut installer, LONG, "the stop", |i| i.run.as_ref().unwrap().outcome == Outcome::Cancelled);
    assert_eq!(log.lock().unwrap().cancelled, 1, "the script process was stopped");
    assert!(hands.has_text("Resume continues where it stopped"));
    assert_eq!(piska_installer::cli::exit_code_of(Some(&installer.run.as_ref().unwrap().outcome)), piska_installer::cli::EXIT_CANCELLED);
}

#[test]
fn malformed_output_is_shown_as_text_and_does_not_break_the_install() {
    let _serial = serial();
    let (mut hands, mut installer, _log) = install_through_fake(Scenario::Malformed);
    hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
    let run = installer.run.as_ref().unwrap();
    assert_eq!(run.outcome, Outcome::Succeeded);
    assert_eq!(run.malformed, 3, "three lines were not events");
    assert!(run.log.iter().any(|e| e.raw && e.text == "WARNING: this is not JSON"));
    assert!(run.log.iter().any(|e| e.raw && e.text == "a line on stderr"));
}

#[test]
fn a_silent_script_shows_still_working_and_is_not_killed() {
    let _serial = serial();
    let (mut installer, log) = fake_installer(Scenario::Hang, 500 << 30, InstallerOptions { still_working: Some(Duration::from_millis(400)), ..InstallerOptions::default() });
    let mut hands = Hands::new(&mut installer);
    walk_to_review(&mut hands, &mut installer);
    hands.click(&mut installer, "install");
    hands.click(&mut installer, "dialog-ok");
    hands.until(&mut installer, LONG, "the note", |i| i.hit("still-working").is_some());
    assert!(installer.run.as_ref().unwrap().is_running(), "not killed");
    assert!(hands.has_text("Still working"));
    assert_eq!(log.lock().unwrap().cancelled, 0);
    // Закрыть окно во время установки — спросить, затем остановить скрипт.
    installer.apply(piska_installer::Action::Close);
    assert!(!installer.quit);
    hands.frames(&mut installer, 2);
    hands.click(&mut installer, "dialog-ok");
    assert!(installer.quit);
    hands.until(&mut installer, LONG, "the stop", |i| !i.run.as_ref().unwrap().is_running());
    assert_eq!(log.lock().unwrap().cancelled, 1);
}

#[test]
fn a_crashing_script_is_a_failure_with_its_exit_code() {
    let _serial = serial();
    let (mut hands, mut installer, _log) = install_through_fake(Scenario::Crash);
    hands.until(&mut installer, LONG, "the failure", |i| matches!(i.run.as_ref().map(|r| &r.outcome), Some(Outcome::Failed(_))));
    let Some(Outcome::Failed(f)) = installer.run.as_ref().map(|r| r.outcome.clone()) else { unreachable!() };
    assert_eq!((f.step.clone(), f.exit), (None, Some(-1)));
    assert!(f.error.contains("something broke"), "{f:?}");
    assert!(hands.has_text("The installer script stopped unexpectedly (exit code -1)"));
}

#[test]
fn dependencies_are_ticked_automatically_with_an_explanation() {
    let _serial = serial();
    let (mut installer, _) = fake_installer(Scenario::Success, 500 << 30, InstallerOptions { components: Some(vec![]), ..InstallerOptions::default() });
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    hands.click(&mut installer, "next");
    assert!(installer.selected.is_empty());
    assert!(installer.hit("next").is_some() && !installer.can_advance(), "nothing selected: Next is disabled");
    hands.click(&mut installer, "comp-web");
    assert_eq!(installer.resolved(), ["core", "web"]);
    assert_eq!(installer.auto_selected("core"), ["web"]);
    assert!(hands.has_text("Core is selected automatically: Web export cannot be installed without it."), "{:?}", hands.texts());
    hands.click(&mut installer, "comp-core");
    assert_eq!(installer.resolved(), ["core", "web"], "a dependency cannot be unticked");
    hands.click(&mut installer, "comp-web");
    assert!(installer.resolved().is_empty());
    hands.click(&mut installer, "preset-everything");
    assert_eq!(installer.resolved().len(), 6);
    // Состояние на машине из проверки.
    assert_eq!(installer.presence("cpp"), Presence::Partial { missing: 1, total: 2 });
    assert_eq!(installer.presence("web"), Presence::Missing);
    assert!(matches!(installer.presence("engine"), Presence::Partial { .. }));
}

#[test]
fn low_disk_space_is_a_warning_on_components_and_review() {
    let _serial = serial();
    let (mut installer, _) = fake_installer(Scenario::Success, 100 << 20, InstallerOptions { free_bytes: Some(100 << 20), ..InstallerOptions::default() });
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    hands.click(&mut installer, "next");
    hands.click(&mut installer, "preset-typical");
    assert!(installer.low_disk());
    assert!(installer.hit("disk-warning").is_some());
    assert!(hands.has_text("Not enough free space"));
    let (mut roomy, _) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { free_bytes: Some(1 << 40), ..InstallerOptions::default() });
    let mut hands = Hands::new(&mut roomy);
    hands.until(&mut roomy, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    hands.click(&mut roomy, "next");
    assert!(!roomy.low_disk() && roomy.hit("disk-warning").is_none());
}

#[test]
fn install_is_disabled_until_every_licence_is_ticked() {
    let _serial = serial();
    let (mut installer, log) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { components: Some(vec!["core".into(), "android".into()]), ..InstallerOptions::default() });
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    for _ in 0..3 {
        hands.click(&mut installer, "next");
    }
    assert_eq!(installer.page, Page::Licences);
    let needed: Vec<String> = installer.licences_needed().iter().map(Installer::licence_key).collect();
    assert_eq!(needed, ["vs-buildtools", "android-sdk"]);
    assert!(!installer.can_advance());
    hands.click(&mut installer, "next");
    assert_eq!(installer.page, Page::Licences, "Next does nothing until the licences are accepted");
    hands.click(&mut installer, "licence-vs-buildtools");
    assert!(!installer.can_advance(), "one is not enough");
    hands.click(&mut installer, "licence-android-sdk");
    assert!(installer.can_advance());
    hands.click(&mut installer, "licence-link-android-sdk");
    assert_eq!(installer.opened.last().map(String::as_str), Some("https://example.invalid/licence/android-sdk"));
    hands.click(&mut installer, "next");
    hands.until(&mut installer, LONG, "the plan", |i| i.plan.as_ref().is_some_and(|p| !p.running()));
    assert!(installer.install_ready());
    // Снять галочку назад на странице лицензий — Install снова недоступна.
    installer.apply(piska_installer::Action::AcceptLicence("android-sdk".into(), false));
    hands.frames(&mut installer, 2);
    assert!(!installer.install_ready());
    hands.click(&mut installer, "install");
    assert_eq!(installer.page, Page::Review);
    assert!(log.lock().unwrap().requests.iter().all(|r| r.purpose != Purpose::Install), "nothing was started");
}

#[test]
fn the_private_repository_is_explained_and_an_existing_checkout_is_the_default() {
    let _serial = serial();
    let (mut installer, _) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { components: Some(vec!["engine".into()]), ..InstallerOptions::default() });
    assert_eq!(installer.source, Source::Existing, "started from inside a checkout: use it");
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, LONG, "the check", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    hands.click(&mut installer, "next");
    hands.click(&mut installer, "next");
    assert_eq!(installer.page, Page::Options);
    assert!(installer.hit("private-repo-note").is_none());
    assert!(hands.has_text("The copy of the engine this installer was started from."));
    hands.click(&mut installer, "source-download");
    assert_eq!(installer.source, Source::Download);
    assert!(installer.hit("private-repo-note").is_some());
    assert!(hands.has_text("The repository is private right now"));
    assert_eq!(installer.script_options().source_dir, None, "download: no -SourceDir");
    hands.click(&mut installer, "source-existing");
    assert_eq!(installer.script_options().source_dir.as_deref(), Some(repo().display().to_string().as_str()));
    // Не checkout — «Далее» нельзя.
    installer.source_dir = temp("not-a-checkout").display().to_string();
    hands.frames(&mut installer, 2);
    assert!(!installer.can_advance());
    assert!(hands.has_text("Not an engine checkout"));
    // Без checkout рядом по умолчанию — скачать.
    let mut far = InstallerOptions { components: Some(vec!["engine".into()]), ..InstallerOptions::default() };
    far.engine_root = Some(temp("no-checkout"));
    let (elsewhere, _) = fake_installer(Scenario::Success, 1 << 40, far);
    assert_eq!(elsewhere.source, Source::Download);
}

/// Манифест прежней установки (как его пишет install.ps1 / install.sh).
fn manifest_env(name: &str) -> Vec<(String, String)> {
    let root = temp(name);
    if cfg!(windows) {
        let dir = root.join("Local").join("PiskaEngine").join("install");
        std::fs::create_dir_all(&dir).unwrap();
        let victim = root.join("Piska Hub.lnk");
        std::fs::write(&victim, "x").unwrap();
        let manifest = serde_json::json!({"version":1,"entries":[
            {"kind":"winget","id":"Kitware.CMake","detail":"","previous":"","component":"cpp","time":"t"},
            {"kind":"shortcut","id":"piska-hub","detail":victim.display().to_string(),"previous":"","component":"engine","time":"t"}]});
        std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(dir.join("state.json"), r#"{"version":1,"components":["core","cpp"],"steps":[]}"#).unwrap();
        vec![("LOCALAPPDATA".into(), root.join("Local").display().to_string())]
    } else {
        let dir = root.join("state").join("PiskaEngine").join("install");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.tsv"), format!("rust-target\twasm32-unknown-unknown\t\t\tweb\tt\nfile\tpiska-hub.desktop\t{}\t\tengine\tt\n", root.join("hub.desktop").display())).unwrap();
        std::fs::write(dir.join("components"), "core cpp\n").unwrap();
        vec![("XDG_STATE_HOME".into(), root.join("state").display().to_string()), ("HOME".into(), root.display().to_string())]
    }
}

#[test]
fn an_install_manifest_offers_modify_repair_and_uninstall() {
    let _serial = serial();
    let env = manifest_env("modify");
    let (mut installer, log) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { env: env.clone(), ..InstallerOptions::default() });
    assert_eq!(installer.mode, Mode::Modify);
    assert_eq!(installer.selected, ["core", "cpp"], "Modify starts from what is installed");
    let mut hands = Hands::new(&mut installer);
    assert!(hands.has_text("EXISTING INSTALLATION"));
    hands.click(&mut installer, "mode-repair");
    assert_eq!(installer.pages(), [Page::Welcome, Page::Review, Page::Installing, Page::Finish]);
    hands.click(&mut installer, "mode-uninstall");
    hands.click(&mut installer, "next");
    assert_eq!(installer.page, Page::Uninstall);
    hands.until(&mut installer, LONG, "the uninstall plan", |i| i.uninstall_plan.as_ref().is_some_and(|p| !p.running()));
    assert!(hands.has_text("Uninstall the program (winget)"));
    // Оставить CMake: на вопрос скрипта уйдёт «нет».
    hands.click(&mut installer, "keep-1");
    assert!(installer.keep.contains("Kitware.CMake"));
    hands.click(&mut installer, "uninstall");
    hands.until(&mut installer, LONG, "the finish page", |i| i.page == Page::Finish);
    assert_eq!(log.lock().unwrap().answers, ["yes", "no"], "proceed: yes; remove CMake: no (kept)");
    assert!(hands.has_text("Uninstall finished"));
}

/// Настоящий скрипт: план удаления по манифесту-образцу в сухом прогоне (ничего не удаляется).
#[test]
fn the_real_script_lists_what_uninstall_would_remove_from_a_manifest_fixture() {
    let _serial = serial();
    let env = manifest_env("real-uninstall");
    let mut installer = Installer::new(InstallerOptions {
        env,
        uninstall: true,
        dry_run: true,
        sandbox: true,
        doctor: Some(vec![]),
        config_dir: Some(temp("real-uninstall-cfg")),
        engine_root: Some(repo()),
        language: Some(engine_editor::kit::i18n::Language::English),
        ..InstallerOptions::default()
    });
    assert_eq!((installer.mode, installer.page), (Mode::Uninstall, Page::Uninstall));
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, Duration::from_secs(120), "the uninstall plan", |i| i.uninstall_plan.as_ref().is_some_and(|p| !p.running()));
    let plan = installer.uninstall_plan.as_ref().unwrap();
    assert_eq!(plan.error, None, "{:?}", plan.notes);
    let items = plan.uninstall.clone().unwrap();
    let actions: Vec<&str> = items.iter().map(|i| i.action.as_str()).collect();
    if cfg!(windows) {
        assert_eq!(actions, ["delete-file", "winget-uninstall"]);
    } else {
        assert_eq!(actions, ["delete-file", "rustup-target-remove"]);
    }
    assert!(hands.has_text("Delete the file"));
}

#[test]
fn the_language_switch_is_live_and_settings_survive_a_restart() {
    let _serial = serial();
    let cfg = temp("settings");
    let (mut installer, _) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { config_dir: Some(cfg.clone()), language: Some(engine_editor::kit::i18n::Language::English), ..InstallerOptions::default() });
    let mut hands = Hands::new(&mut installer);
    assert!(hands.has_text("WHAT HAPPENS"));
    hands.click(&mut installer, "lang-ru");
    assert!(hands.has_text("ЧТО ПРОИЗОЙДЁТ"), "{:?}", hands.texts());
    assert_eq!(installer.language, engine_editor::kit::i18n::Language::Russian);
    hands.click(&mut installer, "next");
    hands.click(&mut installer, "preset-everything");
    installer.install_dir = "D:\\Games\\Engine".into();
    installer.desktop = true;
    installer.proxy = "http://proxy:3128".into();
    installer.window_size = Some((1200.0, 760.0));
    installer.save_settings();
    drop(installer);
    let (again, _) = fake_installer(Scenario::Success, 1 << 40, InstallerOptions { config_dir: Some(cfg), language: None, ..InstallerOptions::default() });
    assert_eq!(again.language, engine_editor::kit::i18n::Language::Russian);
    assert_eq!(again.resolved().len(), 6);
    assert_eq!(again.install_dir, "D:\\Games\\Engine");
    assert!(again.desktop);
    assert_eq!(again.proxy, "http://proxy:3128");
    assert_eq!(again.window_size, Some((1200.0, 760.0)));
    engine_editor::kit::i18n::set(engine_editor::kit::i18n::Language::English);
}

/// Настоящий скрипт в сухом прогоне: Typical, щелчками и клавишами (Enter / Esc). Проверка
/// машины только читает; каталог установки не создаётся.
#[test]
fn walking_the_wizard_for_typical_with_the_real_script_in_dry_run() {
    let _serial = serial();
    let install_dir = temp("real-dry").join("Install");
    let cfg = temp("real-dry-cfg");
    let mut installer = Installer::new(InstallerOptions {
        dry_run: true,
        install_dir: Some(install_dir.display().to_string()),
        config_dir: Some(cfg),
        engine_root: Some(repo()),
        sandbox: true,
        doctor: Some(vec![]),
        language: Some(engine_editor::kit::i18n::Language::English),
        ..InstallerOptions::default()
    });
    assert!(installer.backend_error.is_none(), "{:?}", installer.backend_error);
    let mut hands = Hands::new(&mut installer);
    hands.until(&mut installer, Duration::from_secs(180), "the check of this computer", |i| i.detect.as_ref().is_some_and(|d| !d.running()));
    let detect = installer.detect.as_ref().unwrap();
    assert_eq!(detect.error, None, "{:?}", detect.notes);
    assert!(detect.steps.iter().any(|s| s.id == "core.rustup"), "the check planned every component");
    hands.key(&mut installer, egui::Key::Enter);
    assert_eq!(installer.page, Page::Components, "Enter = Next");
    hands.key(&mut installer, egui::Key::Escape);
    assert_eq!(installer.page, Page::Welcome, "Esc = Back");
    hands.click(&mut installer, "next");
    hands.click(&mut installer, "preset-typical");
    assert_eq!(installer.resolved(), ["core", "cpp", "web"]);
    hands.click(&mut installer, "next");
    hands.click(&mut installer, "next");
    assert_eq!(installer.page, Page::Licences);
    for l in installer.licences_needed() {
        hands.click(&mut installer, &format!("licence-{}", Installer::licence_key(&l)));
    }
    hands.click(&mut installer, "next");
    assert_eq!(installer.page, Page::Review);
    hands.until(&mut installer, Duration::from_secs(180), "the exact plan", |i| i.plan.as_ref().is_some_and(|p| !p.running()));
    let plan = installer.plan.as_ref().unwrap();
    assert_eq!(plan.error, None, "{:?}", plan.notes);
    let components: std::collections::BTreeSet<&str> = plan.steps.iter().map(|s| s.component.as_str()).collect();
    assert!(components.contains("core") && components.contains("cpp") && components.contains("web"));
    assert!(!components.contains("android"));
    assert!(installer.plan_text().contains("STEP") || installer.plan_text().contains("SKIP"));
    hands.click(&mut installer, "copy-plan");
    assert!(installer.clipboard.as_deref().unwrap().starts_with("Components: core, cpp, web"));
    hands.click(&mut installer, "install");
    assert_eq!(installer.page, Page::Installing);
    hands.until(&mut installer, Duration::from_secs(180), "the dry run", |i| i.page == Page::Finish);
    let run = installer.run.as_ref().unwrap();
    assert!(run.options.dry_run);
    assert_eq!(run.outcome, Outcome::Succeeded);
    assert!(run.steps.iter().all(|s| matches!(s.state, StepState::Planned | StepState::WouldSkip)), "a dry run only plans");
    assert!(hands.has_text("Dry run finished: nothing was changed"));
    assert!(!install_dir.exists(), "the dry run created the install folder");
}
