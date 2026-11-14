//! Печь ассетов игры: префабы, интерфейс и звук записываются в `assets/`.
//!
//! Это инструмент разработчика, в игре он не используется: игра стартует только из файлов.
//! Запуск (из корня рабочего пространства):
//!
//! ```text
//! ENGINE_AUDIO=off cargo run -p lantern --example bake_assets            # всё
//! ENGINE_AUDIO=off cargo run -p lantern --example bake_assets -- ui      # prefabs | ui | audio
//! ```
//!
//! Сцены уровней и меню пишет `tools/gen_levels.py` (они ссылаются на эти префабы),
//! картинки — `tools/gen_assets.py`. Править готовые префабы можно и в редакторе; запускать
//! печь заново нужно, только чтобы пересоздать их с нуля.

mod audio;
mod menu_defs;
mod prefabs;
mod ui_defs;

use engine::prelude::*;
use std::path::PathBuf;

/// Текстуры игры по имени файла в `assets/sprites`.
pub struct Gfx {
    db: AssetDatabase,
}

impl Gfx {
    pub fn tex(&self, name: &str) -> AssetHandle<Texture> {
        self.db
            .handle::<Texture>(&format!("sprites/{name}.png"))
            .unwrap_or_else(|e| panic!("run tools/gen_assets.py first: {e}"))
    }
}

fn save_prefab(scene: &Scene, id: GameObjectId, dir: &std::path::Path, file: &str) {
    let prefab = Prefab::from_object(scene, id).expect("object exists");
    std::fs::create_dir_all(dir).expect("directory");
    prefab.save(dir.join(format!("{file}.prefab"))).expect("write prefab");
    println!("prefab {}/{file}", dir.file_name().and_then(|n| n.to_str()).unwrap_or(""));
}

fn main() -> Result<(), EngineError> {
    let what = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

    let mut app = engine::app();
    lantern::register(&mut app);
    engine::mount_assets(&mut app, &root)?;
    let database = app.world.resources.get::<AssetDatabase>().expect("database").clone();
    let gfx = Gfx { db: database };
    let scene = &mut app.world.scene;

    if what == "all" || what == "prefabs" {
        prefabs::bake(scene, &gfx, &root.join("prefabs"));
    }
    if what == "all" || what == "ui" {
        let ui = root.join("ui");
        let id = ui_defs::hud(scene, &gfx);
        save_prefab(scene, id, &ui, "hud");
        let id = ui_defs::pause(scene);
        save_prefab(scene, id, &ui, "pause");
        let id = ui_defs::results(scene);
        save_prefab(scene, id, &ui, "results");
        let canvas = scene.ui_canvas().name("MenuCanvas").sort_order(1).id();
        save_prefab(scene, canvas, &ui, "menu_canvas");
        let [main, levels, options, help] = menu_defs::screens(scene, &gfx);
        save_prefab(scene, main, &ui, "main_menu");
        save_prefab(scene, levels, &ui, "level_select");
        save_prefab(scene, options, &ui, "options");
        save_prefab(scene, help, &ui, "help");
    }
    if what == "all" || what == "audio" {
        let dir = root.join("audio");
        std::fs::create_dir_all(&dir).expect("audio directory");
        for (name, clip) in audio::render_all() {
            clip.save_wav(dir.join(format!("{name}.wav"))).unwrap_or_else(|e| panic!("cannot write {name}.wav: {e}"));
            println!("audio {name}");
        }
    }
    // Открытие базы создаёт недостающие `.meta` у новых файлов.
    AssetDatabase::open(&root)?;
    Ok(())
}
