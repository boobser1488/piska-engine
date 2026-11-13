//! Десктопная программа игры: `cargo run -p swarm`.
//! В браузере и на Android точку входа даёт `engine::game_main!` в `lib.rs`.

// В release-сборке на Windows — без окна консоли.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), engine::EngineError> {
    swarm::main()
}
