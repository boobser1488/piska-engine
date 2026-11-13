//! `engine-cpp-build` — помощник для `build.rs` игры, у которой есть собственные C++-скрипты.
//!
//! Игре с C++-логикой нужны три шага:
//!
//! ```toml
//! # Cargo.toml игры
//! [build-dependencies]
//! engine-cpp-build = { path = "../../crates/engine-cpp-build" }
//! ```
//!
//! ```text
//! // build.rs игры: все *.cpp из каталога `cpp/` рядом с Cargo.toml
//! fn main() {
//!     engine_cpp_build::compile("cpp");
//! }
//! ```
//!
//! ```ignore
//! // src/lib.rs игры
//! engine::add_static_cpp(&mut app, engine::cpp_scripts!())?;
//! ```
//!
//! Что делает [`compile`]:
//!
//! * собирает все `*.cpp` каталога (с подкаталогами, по алфавиту) **одной единицей
//!   трансляции**: компоненты регистрируют себя статическими объектами, а линкер выбросил бы
//!   из статической библиотеки объектный файл, на который никто не ссылается;
//! * добавляет в конец единицы точку входа плагина — `engine_game_<пакет>_entry`, так что в
//!   скриптах достаточно `ENGINE_COMPONENT(...)` (своего `ENGINE_PLUGIN_AUTO_ENTRY` писать не
//!   нужно);
//! * подключает `engine_component.hpp` из `engine-cpp-bridge/include` (C++17, `/EHsc` у MSVC);
//! * пишет в `OUT_DIR` файл `engine_cpp_scripts.rs`, который подключает макрос
//!   `engine::cpp_scripts!()`;
//! * сообщает Cargo, за какими файлами следить (`rerun-if-changed`);
//! * на `wasm32` ничего не компилирует (там нет стандартной библиотеки C++): файл для
//!   `cpp_scripts!()` всё равно создаётся, `engine::add_static_cpp` вернёт понятную ошибку,
//!   а у крейта игры не будет `cfg(engine_game_cpp)` — по нему игра может обойтись без C++.
//!
//! Работает с MSVC, GCC / Clang и clang из Android NDK (через `cargo ndk`): компиляцию ведёт
//! крейт `cc`.
#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

/// Ошибка сборки C++-скриптов игры.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("environment variable `{0}` is not set: call engine_cpp_build::compile from the build.rs of a game crate")]
    NotInCargo(&'static str),
    #[error("cannot read the C++ script directory `{path}`: {source}. Create it next to Cargo.toml and put the *.cpp files of your game there")]
    ReadDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("there are no *.cpp files in `{0}`. Add at least one script (see docs/api/cpp.md, section «C++ в своей игре»)")]
    NoSources(String),
    #[error("cannot write `{path}`: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("the C++ compiler failed: {0}. Check the compiler output above")]
    Compile(String),
}

/// Что получилось после [`Build::try_compile`].
#[derive(Debug, Clone)]
pub struct Built {
    /// Имя символа точки входа (`engine_game_<пакет>_entry`).
    pub entry_symbol: String,
    /// Собранные файлы (пусто на `wasm32`).
    pub sources: Vec<PathBuf>,
    /// C++ действительно скомпилирован (на `wasm32` — `false`).
    pub compiled: bool,
}

impl fmt::Display for Built {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} files)", self.entry_symbol, self.sources.len())
    }
}

/// Настройка сборки; для обычного случая хватает [`compile`].
#[derive(Debug, Clone)]
pub struct Build {
    dir: PathBuf,
    includes: Vec<PathBuf>,
    defines: Vec<(String, Option<String>)>,
}

impl Build {
    /// Каталог со скриптами — относительно каталога `Cargo.toml` игры.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self { dir: dir.as_ref().to_path_buf(), includes: Vec::new(), defines: Vec::new() }
    }

    /// Ещё один каталог для `#include <...>` (относительно `Cargo.toml` игры).
    pub fn include(mut self, dir: impl AsRef<Path>) -> Self {
        self.includes.push(dir.as_ref().to_path_buf());
        self
    }

    /// `-DNAME` / `-DNAME=value` для всех скриптов.
    pub fn define(mut self, name: &str, value: Option<&str>) -> Self {
        self.defines.push((name.to_string(), value.map(str::to_string)));
        self
    }

    /// Собрать; при ошибке напечатать её и остановить сборку игры (`panic` в `build.rs`).
    pub fn compile(self) {
        if let Err(error) = self.try_compile() {
            panic!("engine-cpp-build: {error}");
        }
    }

    /// Собрать и вернуть результат или ошибку.
    pub fn try_compile(self) -> Result<Built, BuildError> {
        let manifest = PathBuf::from(env("CARGO_MANIFEST_DIR")?);
        let out_dir = PathBuf::from(env("OUT_DIR")?);
        let package = env("CARGO_PKG_NAME")?;
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

        let symbol = entry_symbol(&package);
        println!("cargo::rustc-check-cfg=cfg(engine_game_cpp)");
        println!("cargo:rerun-if-changed=build.rs");

        let scripts_dir = manifest.join(&self.dir);
        let rust_file = out_dir.join("engine_cpp_scripts.rs");

        // wasm32: стандартной библиотеки C++ нет — модуль не собирается, но макрос
        // `cpp_scripts!()` обязан раскрываться во что-то компилируемое.
        if arch == "wasm32" {
            write(&rust_file, &rust_expr(&package, None))?;
            println!("cargo:rerun-if-changed={}", scripts_dir.display());
            return Ok(Built { entry_symbol: symbol, sources: Vec::new(), compiled: false });
        }

        let mut sources = Vec::new();
        collect(&scripts_dir, &mut sources)?;
        sources.sort();
        if sources.is_empty() {
            return Err(BuildError::NoSources(scripts_dir.display().to_string()));
        }

        // Единица трансляции. Пути — без «..»: на Windows каждая лишняя пара символов
        // приближает к пределу в 260.
        let mut unity = String::from("// Generated by engine-cpp-build: all game scripts in one translation unit.\n");
        unity.push_str("#include \"engine_component.hpp\"\n");
        for path in &sources {
            unity.push_str(&format!("#include \"{}\"\n", slashes(path)));
        }
        unity.push_str(&format!("ENGINE_PLUGIN_AUTO_ENTRY({symbol}, \"{package}\")\n"));
        let unity_path = out_dir.join("game_cpp_unity.cpp");
        write(&unity_path, &unity)?;

        // `…/crates/engine-cpp-build` → `…/crates/engine-cpp-bridge/include`.
        let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bridge_include = helper
            .parent()
            .map(|crates| crates.join("engine-cpp-bridge").join("include"))
            .unwrap_or_else(|| helper.join("engine-cpp-bridge").join("include"));

        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++17")
            // Исключения C++ ловятся в переходниках хуков (/EHsc у MSVC).
            .flag_if_supported("/EHsc")
            // Комментарии в скриптах — UTF-8 (MSVC по умолчанию читает их в кодовой странице).
            .flag_if_supported("/utf-8")
            .flag_if_supported("-Wno-invalid-offsetof")
            .include(&bridge_include)
            .include(&scripts_dir)
            .file(&unity_path);
        for include in &self.includes {
            build.include(manifest.join(include));
        }
        for (name, value) in &self.defines {
            build.define(name, value.as_deref());
        }
        let lib = format!("engine_cpp_{}", package.replace(['-', '.'], "_"));
        build.try_compile(&lib).map_err(|e| BuildError::Compile(e.to_string()))?;

        write(&rust_file, &rust_expr(&package, Some(&symbol)))?;
        println!("cargo:rustc-cfg=engine_game_cpp");
        println!("cargo:rerun-if-changed={}", scripts_dir.display());
        println!("cargo:rerun-if-changed={}", bridge_include.join("engine_component.hpp").display());
        println!("cargo:rerun-if-changed={}", bridge_include.join("engine_api.h").display());
        for path in &sources {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        Ok(Built { entry_symbol: symbol, sources, compiled: true })
    }
}

/// Собрать все `*.cpp` из `dir` (относительно `Cargo.toml` игры). Остановит сборку
/// игры с понятным сообщением, если не получилось. См. описание крейта.
pub fn compile(dir: impl AsRef<Path>) {
    Build::new(dir).compile();
}

/// Имя символа точки входа для пакета: `engine_game_<имя>_entry`.
pub fn entry_symbol(package: &str) -> String {
    let clean: String = package.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    format!("engine_game_{clean}_entry")
}

/// Rust-выражение, в которое раскрывается `engine::cpp_scripts!()`.
fn rust_expr(package: &str, symbol: Option<&str>) -> String {
    match symbol {
        Some(symbol) => format!(
            "{{\n    extern \"C\" {{\n        fn {symbol}() -> *const ::engine::cpp::abi::EnginePlugin;\n    }}\n    \
             // SAFETY: функция определена в C++-единице, собранной engine-cpp-build, и возвращает\n    \
             // указатель на статический EnginePlugin, оформленный ENGINE_PLUGIN_AUTO_ENTRY.\n    \
             #[allow(unsafe_code)]\n    let scripts = unsafe {{ ::engine::cpp::StaticScripts::from_entry({package:?}, {symbol}) }};\n    scripts\n}}\n"
        ),
        None => format!("::engine::cpp::StaticScripts::unavailable({package:?})\n"),
    }
}

fn env(name: &'static str) -> Result<String, BuildError> {
    std::env::var(name).map_err(|_| BuildError::NotInCargo(name))
}

fn write(path: &Path, text: &str) -> Result<(), BuildError> {
    std::fs::write(path, text).map_err(|source| BuildError::Write { path: path.display().to_string(), source })
}

fn slashes(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BuildError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|source| BuildError::ReadDir { path: dir.display().to_string(), source })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "cpp") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_is_a_valid_identifier() {
        assert_eq!(entry_symbol("marble"), "engine_game_marble_entry");
        assert_eq!(entry_symbol("my-game.v2"), "engine_game_my_game_v2_entry");
    }

    #[test]
    fn stub_for_wasm_has_no_extern() {
        assert!(!rust_expr("g", None).contains("extern"));
        assert!(rust_expr("g", Some("engine_game_g_entry")).contains("from_entry"));
    }
}
