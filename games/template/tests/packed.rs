//! Игра целиком из пака — как в браузере и на Android: каталог ассетов не читается.
//! Отдельный файл = отдельный процесс: установленный пак глобален для процесса.

use engine::prelude::*;

#[test]
fn the_template_runs_from_an_asset_pack() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut builder = engine::assets::AssetPack::builder();
    builder.add_dir(engine::assets_dir!()).unwrap();
    engine::assets::install_pack(engine::assets::AssetPack::from_bytes(builder.to_bytes()).unwrap());

    let mut app = template::build_with(SaveData::in_memory()).unwrap();
    let database = app.world.resources.get::<AssetDatabase>().unwrap();
    assert!(database.is_packed(), "mount_assets took the installed pack");
    assert!(database.path_of(database.guid_of("sprites/star.png").unwrap()).unwrap().starts_with(engine::assets::PACK_ROOT));

    // Сцена и префаб пришли из пака: корзина есть, звёзды появляются.
    assert!(app.world.scene.find_by_name("Player").is_some());
    app.run_headless(60 * 3, 1.0 / 60.0);
    assert!(!app.world.scene.find_all_by_tag("Star").is_empty());
    // `ctx.load_scene("main")` тоже ищет в паке.
    app.world.scenes().load("main");
    app.run_headless(2, 1.0 / 60.0);
    assert_eq!(app.world.scene.name, "Main");
    assert_eq!(app.world.scenes().loaded_name(), Some("main"));
}
