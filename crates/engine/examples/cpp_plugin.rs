//! C++ из Rust, оба способа:
//! 1. статический C++-модуль (шум fBm) через безопасную обёртку;
//! 2. динамический плагин (`.dll`/`.so`/`.dylib`) с CPU пост-эффектом.
//!
//! Сначала соберите плагин:
//!   cmake -S plugins/cpp-example -B plugins/cpp-example/build
//!   cmake --build plugins/cpp-example/build --config Release
//! Затем: `cargo run --example cpp_plugin [каталог_с_плагином] [out.png]`

use std::path::PathBuf;

use engine::cpp::{noise, plugin_file_name, Plugin};
use engine::EngineError;

fn main() -> Result<(), EngineError> {
    engine::init_logger();
    let mut args = std::env::args().skip(1);
    let plugin_dir = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/cpp-example/bin"))
    });
    let output = args.next().unwrap_or_else(|| "cpp_plugin_out.png".to_string());

    let (width, height) = (512u32, 320u32);
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    // 1. Статический C++-модуль: карта высот из фрактального шума.
    noise::fill_rgba(&mut pixels, width, height, 90.0, 7)?;
    println!("fbm(0.5, 0.5) = {:.4}", noise::fbm2(0.5, 0.5, 5, 7));

    // 2. Динамический плагин: раскрашивает карту и добавляет виньетку.
    let plugin = Plugin::load(plugin_dir.join(plugin_file_name("piska_vignette")))?;
    println!("loaded plugin `{}`", plugin.name());
    plugin.fixed_update(1.0 / 60.0);
    plugin.process_rgba(&mut pixels, width, height)?;

    image::save_buffer(&output, &pixels, width, height, image::ColorType::Rgba8).map_err(|e| {
        EngineError::Image {
            path: output.clone(),
            reason: e.to_string(),
        }
    })?;
    println!("saved {output}");
    Ok(())
}
