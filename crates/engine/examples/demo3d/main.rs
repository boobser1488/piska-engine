//! 3D-сцена из файла с игровой логикой на двух языках: `Orbit` — скрипт на Rust
//! (`scripts/orbit.rs`), `Spinner` и `Bobber` — на C++ (`plugins/cpp-gameplay/gameplay.cpp`).
//!
//! Игра:     `cargo run --example demo3d`            (стрелки вращают камеру)
//! Редактор: `cargo run --example demo3d -- --editor`
//!
//! В редакторе: Assets → New Rust Script… кладёт новый скрипт в `scripts/` и
//! переписывает `scripts/mod.rs`; Build (Ctrl+B) пересобирает пример и перезапускает
//! редактор с новым кодом — сцена, выделение и открытые файлы остаются.
//!
//! C++-плагин собирается так (один раз; дальше — кнопкой Build в редакторе):
//! `cmake -S plugins/cpp-gameplay -B plugins/cpp-gameplay/build`
//! `cmake --build plugins/cpp-gameplay/build --config Release`
//! Пока игра или редактор работают, пересобранный плагин подхватывается на лету.

mod scripts;

use std::path::Path;

use engine::cpp::{builtin_gameplay, plugin_file_name, CppPlugins};
use engine::prelude::*;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo3d");

fn main() -> Result<(), EngineError> {
    let assets = format!("{ROOT}/assets");
    let plugin_dir = format!("{ROOT}/plugins/cpp-gameplay");

    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, &assets)?;

    // Логика на C++: динамический плагин (перезагружается при пересборке), а если он
    // ещё не собран — те же компоненты, вкомпилированные в программу.
    let plugin = Path::new(&plugin_dir).join("bin").join(plugin_file_name("piska_gameplay"));
    if plugin.is_file() {
        engine::add_cpp_plugins(&mut app, [&plugin])?;
    } else {
        log::warn!(
            "`{}` is not built yet — using the copy of the C++ components compiled into this \
             program. Build the plugin with CMake to edit C++ without restarting",
            plugin.display()
        );
        let mut plugins = CppPlugins::default();
        builtin_gameplay(&mut plugins)?;
        plugins.register_components(&mut app.world.registry);
        app.add_subsystem(plugins);
    }

    if std::env::args().any(|arg| arg == "--editor") {
        // Каталог сборки CMake можно вынести (на Windows — если путь к проекту слишком длинный).
        let build_dir =
            std::env::var("ENGINE_CPP_BUILD_DIR").unwrap_or_else(|_| format!("{plugin_dir}/build"));
        // Build: сначала C++-плагин, затем пример целиком — и редактор перезапускается.
        let mut relaunch = Relaunch::example("engine", "demo3d").dir(ROOT);
        if let Some(target_dir) = option_env!("CARGO_TARGET_DIR") {
            relaunch = relaunch.target_dir(target_dir);
        }
        return engine::edit(
            app,
            EditorConfig::titled("Piska Editor")
                .open_scene("scenes/demo3d.scene")
                .code_dir(&plugin_dir)
                .cpp_scripts(format!("{plugin_dir}/scripts"))
                .code_dir(EXAMPLE)
                .rust_scripts(format!("{EXAMPLE}/scripts"))
                .build_command(
                    BuildCommand::new("C++ plugin: configure", "cmake").args(["-S", &plugin_dir, "-B", &build_dir]),
                )
                .build_command(
                    BuildCommand::new("C++ plugin: build", "cmake").args(["--build", &build_dir, "--config", "Release"]),
                )
                .relaunch(relaunch),
        );
    }

    app.world.load_scene(format!("{assets}/scenes/demo3d.scene"))?;
    engine::run(app, WindowConfig::titled("Demo 3D"))
}
