//! Геймплейный тест платформера без окна и GPU: тот же уровень и те же скрипты,
//! что и в примере, ввод подаётся программно.

use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

use scripts::PlayerController;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const DT: f32 = 1.0 / 60.0;

fn level() -> (App, GameObjectId) {
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).expect("assets directory");
    app.world
        .load_scene(format!("{ASSETS}/scenes/level1.scene"))
        .unwrap_or_else(|e| panic!("{:?}", EngineError::from(e)));
    let player = app.world.scene.find_by_tag("Player").expect("player in the level");
    (app, player)
}

fn position(app: &App, id: GameObjectId) -> Vec3 {
    app.world.scene.world_position(id)
}

#[test]
fn player_falls_and_rests_on_the_ground() {
    let (mut app, player) = level();
    let start_y = position(&app, player).y;
    app.run_headless(120, DT);

    let rest = position(&app, player);
    assert!(rest.y < start_y, "gravity must pull the player down");
    // Верх земли y = -2.5, половина высоты коллайдера 0.45.
    assert!((rest.y - (-2.05)).abs() < 0.05, "player rests at y = {}", rest.y);
    let velocity = app.world.scene.get_component::<Rigidbody2D>(player).unwrap().velocity;
    assert!(velocity.length() < 0.05, "player must be at rest, v = {velocity}");
}

#[test]
fn player_runs_jumps_and_collects_a_coin() {
    let (mut app, player) = level();
    app.run_headless(60, DT);
    let coins_before = app.world.scene.find_all_by_tag("Pickup").len();
    assert!(coins_before >= 2);

    // Прыжок на стартовой площадке (над головой ничего нет): нажатие длиной в один кадр.
    // Ожидаемая высота v² / 2g = 11² / (2 · 9.81 · 2.5) ≈ 2.47 м.
    let ground_y = position(&app, player).y;
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    let mut peak = ground_y;
    for _ in 0..180 {
        app.tick(DT);
        peak = peak.max(position(&app, player).y);
    }
    let height = peak - ground_y;
    assert!((2.2..=2.7).contains(&height), "jump height {height} m");
    assert!((position(&app, player).y - ground_y).abs() < 0.05, "must land back");

    // В воздухе прыгнуть нельзя: повторное нажатие сразу после толчка игнорируется.
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    app.run_headless(10, DT);
    let rising = app.world.scene.get_component::<Rigidbody2D>(player).unwrap().velocity.y;
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    app.tick(DT);
    let after = app.world.scene.get_component::<Rigidbody2D>(player).unwrap().velocity.y;
    assert!(after < rising, "double jump must not be possible ({rising} → {after})");
    app.run_headless(120, DT); // приземлиться

    // Бег вправо одну секунду: ~6 м при скорости 6 м/с.
    let x0 = position(&app, player).x;
    app.world.input.set_key(Key::D, true);
    app.run_headless(60, DT);
    app.world.input.set_key(Key::D, false);
    let travelled = position(&app, player).x - x0;
    assert!((5.5..=6.5).contains(&travelled), "ran {travelled} m in 1 s");

    // По пути лежала монета: триггер → сообщение → счёт, монета уничтожена.
    let controller = app.world.scene.get_component::<PlayerController>(player).unwrap();
    assert_eq!(controller.coins, 1);
    assert_eq!(app.world.scene.find_all_by_tag("Pickup").len(), coins_before - 1);
}

#[test]
fn camera_follows_the_player() {
    let (mut app, player) = level();
    let camera = app.world.scene.find_by_name("Main Camera").unwrap();
    app.world.input.set_key(Key::D, true);
    app.run_headless(180, DT);
    let gap = (position(&app, camera).x - position(&app, player).x).abs();
    // При постоянной скорости экспоненциальное сглаживание отстаёт на v / k = 6 / 6 = 1 м.
    assert!(gap < 1.2, "camera lags {gap} m behind");
    assert!(position(&app, camera).x > 0.0, "camera must have moved right");
}

#[test]
fn saving_the_level_keeps_prefab_links() {
    let (app, _) = level();
    let saved = app.world.scene_to_string();
    assert!(saved.contains(r#"prefab: Some("prefabs/coin.prefab")"#), "{saved}");
    assert!(saved.contains(r#"kind: "PlayerController""#), "{saved}");

    // Сохранённая сцена загружается обратно и даёт тот же текст.
    let mut again = engine::app();
    scripts::register(&mut again);
    engine::mount_assets(&mut again, ASSETS).unwrap();
    again.world.load_scene_str(&saved, "saved").unwrap();
    assert_eq!(again.world.scene_to_string(), saved);
}
