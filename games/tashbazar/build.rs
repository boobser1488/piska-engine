//! Сборка игры: значок приложения в exe на Windows.

fn main() {
    // Значок и подсистема «окно» (без консоли и в отладочной сборке; `PISKA_CONSOLE=1` — с ней).
    engine_build::windows_app();
}
