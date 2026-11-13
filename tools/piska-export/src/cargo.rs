//! Запуск cargo: сборка игры под цель, поиск каталога `target`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

/// `cargo` того же тулчейна, что запустил экспорт (`$CARGO` внутри `cargo run`).
fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Каталог `target` рабочего пространства (учитывает `CARGO_TARGET_DIR`).
pub fn target_dir(root: &Path) -> Result<PathBuf> {
    let output = cargo()
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| Error(format!("cannot run `cargo metadata`: {e}. Is cargo on PATH?")))?;
    if !output.status.success() {
        return Err(Error(format!(
            "`cargo metadata` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Error("`cargo metadata` did not report target_directory".to_string()))
}

/// Выполнить cargo с аргументами в корне рабочего пространства; вывод — как есть
/// (его читает консоль редактора). `env` — дополнительные переменные окружения.
pub fn run(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    println!("piska-export: cargo {}", args.join(" "));
    let mut command = cargo();
    command.args(args).current_dir(root);
    for (key, value) in env {
        command.env(key, value);
    }
    let status = command
        .status()
        .map_err(|e| Error(format!("cannot run cargo: {e}. Is it on PATH?")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error(format!("cargo {} failed ({status}); see the compiler output above", args[0])))
    }
}

/// Установлена ли цель rustup (например, `wasm32-unknown-unknown`).
pub fn has_target(target: &str) -> bool {
    let Ok(output) = Command::new("rustup").args(["target", "list", "--installed"]).output() else {
        // Без rustup (тулчейн из пакетов ОС) проверить нельзя — пусть скажет cargo.
        return true;
    };
    String::from_utf8_lossy(&output.stdout).lines().any(|line| line.trim() == target)
}

/// Версия пакета `name` в `Cargo.lock` рабочего пространства.
pub fn locked_version(root: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let lock: toml::Table = text.parse().ok()?;
    lock.get("package")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("version")?
        .as_str()
        .map(str::to_string)
}
