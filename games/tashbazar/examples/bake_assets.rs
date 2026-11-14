//! Перегенерация ассетов игры: текстуры, материалы, модели, префабы, интерфейс, звуки,
//! настройки, сцены карты и полигона, маршруты ботов.
//! `cargo run -p tashbazar --example bake_assets` (подробно — tools/bake/mod.rs).
//! В игре не используется.

#[path = "../tools/bake/mod.rs"]
mod bake;

fn main() {
    std::env::set_var("ENGINE_AUDIO", "off");
    if let Err(error) = bake::run() {
        eprintln!("bake_assets: {error}");
        std::process::exit(1);
    }
}
