//! Перегенерация ассетов игры: сцены, префабы, интерфейс, материалы, звуки, данные.
//! `python tools/gen_assets.py && cargo run -p marble --example bake_assets`
//! (подробно — tools/bake/mod.rs). В игре не используется.

#[path = "../tools/bake/mod.rs"]
mod bake;

fn main() {
    if let Err(error) = bake::run() {
        eprintln!("bake_assets: {error}");
        std::process::exit(1);
    }
}
