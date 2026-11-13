//! Игра целиком из пака ассетов — как в браузере и на Android: каталог ассетов не читается.
//! Отдельный файл = отдельный процесс: установленный пак глобален для процесса.

use engine::prelude::*;
use swarm::flow::Game;
use swarm::ui::Screen;

#[test]
fn the_game_runs_from_an_asset_pack() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut builder = engine::assets::AssetPack::builder();
    builder.add_dir(engine::assets_dir!()).unwrap();
    engine::assets::install_pack(engine::assets::AssetPack::from_bytes(builder.to_bytes()).unwrap());

    let mut app = swarm::build_with(SaveData::in_memory()).unwrap();
    let database = app.world.resources.get::<AssetDatabase>().unwrap();
    assert!(database.is_packed(), "mount_assets took the installed pack");
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app.run_headless(30, 1.0 / 60.0);
    let game = app.world.resources.get::<Game>().expect("the director built the game from the pack");
    assert_eq!(game.screen, Screen::Menu);
    assert!(app.world.scene.find_by_name("ArenaFloor").is_some());
}
