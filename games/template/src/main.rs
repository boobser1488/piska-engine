//! Десктопная программа шаблона: `cargo run -p template`.
//! В браузере и на Android точку входа даёт `engine::game_main!` в `lib.rs`.

// В release-сборке на Windows — без окна консоли (как у обычной игры).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), engine::EngineError> {
    template::main()
}
