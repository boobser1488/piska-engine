//! Сборка границы Rust ↔ C++.
//!
//! 1. Rust → C: `cbindgen` генерирует `include/engine_api.h` из `src/abi.rs`.
//!    Этот заголовок включают C++-плагины.
//! 2. C++ → Rust: `cc` компилирует `cpp/*.cpp` в статическую библиотеку,
//!    которая линкуется в крейт. Объявления её функций — в `src/ffi.rs`.

use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));

    // --- 1. Rust → C --------------------------------------------------------
    let header = crate_dir.join("include").join("engine_api.h");
    match cbindgen::generate(&crate_dir) {
        // `write_to_file` не трогает файл, если содержимое не изменилось.
        Ok(bindings) => {
            bindings.write_to_file(&header);
        }
        // Заголовок лежит в репозитории, поэтому сбой генерации не ломает сборку.
        Err(err) => println!("cargo:warning=cbindgen: {err}; using the checked-in engine_api.h"),
    }

    // --- 2. C++ → Rust ------------------------------------------------------
    // `engine_static_cpp` — статические C++-модули вкомпилированы (ffi, noise,
    // builtin_gameplay доступны). В браузере (wasm32-unknown-unknown) их нет: у этой
    // цели нет стандартной библиотеки C++ (libc++) и libc, а `cc` нечем собирать.
    // Что нужно, чтобы включить, — docs/api/export.md, раздел «C++ в браузере».
    println!("cargo::rustc-check-cfg=cfg(engine_static_cpp)");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "wasm32") {
        return;
    }
    println!("cargo:rustc-cfg=engine_static_cpp");

    // Игровые компоненты из примера плагина — тем же кодом, но статически и под
    // другим именем точки входа.
    //
    // Все файлы плагина включаются в одну единицу трансляции: компоненты регистрируют
    // себя статическими объектами, а линкер выбросил бы из статической библиотеки
    // объектный файл, на который никто не ссылается.
    // Путь без «..»: на Windows каждая лишняя пара символов приближает к пределу в 260.
    let workspace = crate_dir.parent().and_then(|p| p.parent()).unwrap_or(&crate_dir);
    let plugin_dir = workspace.join("plugins").join("cpp-gameplay");
    let mut sources = vec![plugin_dir.join("gameplay.cpp")];
    if let Ok(entries) = std::fs::read_dir(plugin_dir.join("scripts")) {
        let mut scripts: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "cpp"))
            .collect();
        scripts.sort();
        sources.extend(scripts);
    }
    let unity: String = sources
        .iter()
        .map(|path| format!("#include \"{}\"\n", path.display().to_string().replace('\\', "/")))
        .collect();
    let gameplay = PathBuf::from(std::env::var("OUT_DIR").expect("set by cargo")).join("gameplay_unity.cpp");
    std::fs::write(&gameplay, unity).expect("OUT_DIR is writable");
    println!("cargo:rerun-if-changed=../../plugins/cpp-gameplay/scripts");
    for path in &sources {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    // Исключения C++ должны ловиться в переходниках хуков (/EHsc у MSVC).
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag_if_supported("/EHsc")
        .include(crate_dir.join("include"))
        .file(crate_dir.join("cpp").join("noise.cpp"))
        .file(&gameplay)
        .define("ENGINE_GAMEPLAY_ENTRY", "engine_builtin_gameplay_entry")
        .flag_if_supported("-Wno-invalid-offsetof")
        .compile("engine_cpp_modules");

    for path in [
        "src/abi.rs",
        "cbindgen.toml",
        "cpp/noise.cpp",
        "include/engine_noise.h",
        "include/engine_component.hpp",
        "../../plugins/cpp-gameplay/gameplay.cpp",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
