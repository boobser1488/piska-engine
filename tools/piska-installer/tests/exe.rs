//! Готовая программа: подсистема Windows «GUI» и значок (читаются из exe), справка и коды
//! выхода, режим без окна (`--unattended`) в сухом прогоне с настоящим скриптом — события
//! JSON по одному на строку. НИЧЕГО НЕ УСТАНАВЛИВАЕТСЯ: только `--dry-run`.

mod common;

use std::process::{Command, Stdio};

use common::*;

fn exe() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_piska-installer"));
    c.stdin(Stdio::null()).env("PISKA_INSTALLER_CONFIG_DIR", temp("exe-cfg"));
    c
}

#[test]
fn the_exe_is_a_gui_program_with_the_engine_icon() {
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_piska-installer")).unwrap();
    if cfg!(windows) {
        assert_eq!(engine_build::read_exe_subsystem(&bytes).unwrap(), engine_build::SUBSYSTEM_GUI, "no console window, ever");
        let icon = engine_build::read_exe_icon(&bytes).unwrap().expect("the exe has an icon");
        assert!(icon.iter().any(|i| i.width >= 256), "a large icon size: {:?}", icon.iter().map(|i| i.width).collect::<Vec<_>>());
    } else {
        assert!(!bytes.is_empty());
    }
}

#[test]
fn help_version_and_exit_codes() {
    let out = exe().arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--unattended") && text.contains("Exit codes"), "{text}");
    let out = exe().arg("--version").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
    let out = exe().arg("--bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(piska_installer::cli::EXIT_BAD_ARGS));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
    let out = exe().args(["--components", "nope", "--unattended"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn unattended_dry_run_prints_json_events_from_the_real_script() {
    let install = temp("exe-unattended").join("Install");
    let out = exe()
        .args(["--unattended", "--dry-run", "--components", "core,web", "--install-dir"])
        .arg(&install)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines.len() > 5, "{stdout}");
    for line in &lines {
        let value: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
        assert!(value["event"].is_string(), "{line}");
    }
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let last: serde_json::Value = serde_json::from_str(lines[lines.len() - 1]).unwrap();
    assert_eq!(first["event"], "hello");
    assert_eq!(first["dry_run"], true);
    assert_eq!(first["components"], serde_json::json!(["core", "web"]));
    assert_eq!((last["event"].as_str(), last["exit"].as_i64()), (Some("summary"), Some(0)));
    let plans = lines.iter().filter(|l| l.starts_with(r#"{"event":"plan""#)).count();
    assert!(plans >= 5, "every step is planned: {plans}");
    assert!(!install.exists(), "a dry run created the install folder");
}
