//! Десктопная программа игры: `cargo run -p marble`.
// В release-сборке на Windows — без окна консоли.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), engine::EngineError> {
    marble::main()
}
