//! Десктопная программа игры: `cargo run -p tashbazar`.
// Подсистема «окно» (без консоли и в отладочной сборке) — build.rs: engine_build::windows_app().

fn main() -> Result<(), engine::EngineError> {
    tashbazar::main()
}
