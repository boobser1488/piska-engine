//! Сборка игры: значок приложения в exe на Windows и (только с возможностью
//! `cpp-example`) один необязательный C++-скрипт из `cpp/`.

fn main() {
    #[cfg(feature = "cpp-example")]
    engine_cpp_build::compile("cpp");
    // Значок и подсистема «окно» (без консоли и в отладочной сборке; `PISKA_CONSOLE=1` — с ней).
    engine_build::windows_app();
}
