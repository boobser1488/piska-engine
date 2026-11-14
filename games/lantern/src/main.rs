//! Десктопная программа игры: `cargo run -p lantern`.
//! В браузере и на Android точку входа даёт `engine::game_main!` в `lib.rs`.

// Подсистема «окно» (без консоли и в отладочной сборке) — build.rs: engine_build::windows_app().

fn main() -> Result<(), engine::EngineError> {
    lantern::main()
}
