//! Шаблон играет сам с собой без окна: звёзды падают, ловятся, игра кончается и
//! начинается заново, рекорд сохраняется.

use engine::prelude::*;
use template::GameManager;

const DT: f32 = 1.0 / 60.0;

fn game(save: SaveData) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    template::build_with(save).expect("the template builds from its assets directory")
}

fn manager(app: &App) -> &GameManager {
    let id = app.world.scene.find_by_name("Game").expect("Game object");
    app.world.scene.get_component::<GameManager>(id).expect("GameManager")
}

fn manager_mut(app: &mut App) -> &mut GameManager {
    let id = app.world.scene.find_by_name("Game").expect("Game object");
    app.world.scene.get_component_mut::<GameManager>(id).expect("GameManager")
}

fn text(app: &App, name: &str) -> String {
    let id = app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("no `{name}`"));
    app.world.scene.get_component::<Text>(id).expect("Text").text.clone()
}

fn stars(app: &App) -> usize {
    app.world.scene.find_all_by_tag("Star").len()
}

#[test]
fn falling_stars_cost_lives_and_end_the_game() {
    let mut app = game(SaveData::in_memory());
    app.run_headless(2, DT);
    assert_eq!(text(&app, "Lives"), "Lives: 3");
    assert!(!manager(&app).game_over);

    // Корзина стоит в центре; после интро (1.5 с) звёзды падают в случайных местах,
    // большинство мимо.
    app.run_headless(60 * 4, DT);
    assert!(manager(&app).spawned >= 2, "stars spawn from the prefab");
    assert!(stars(&app) >= 1, "stars are falling");

    app.run_headless(60 * 20, DT);
    let state = manager(&app);
    assert!(state.game_over, "three missed stars end the game");
    assert_eq!(state.lives, 0);
    assert_eq!(stars(&app), 0, "game over clears the sky");
    let panel = app.world.scene.find_by_name("GameOverPanel").unwrap();
    assert!(app.world.scene.get(panel).unwrap().active, "the game-over screen is shown");
    assert!(text(&app, "FinalScore").starts_with("Score: "));
}

#[test]
fn a_star_above_the_basket_is_caught_and_the_best_score_is_saved() {
    let backend = MemoryBackend::default();
    let mut app = game(SaveData::with_backend(backend.clone()));
    manager_mut(&mut app).spawn_interval = 1000.0; // только своя звезда
    app.run_headless(2, DT);

    let basket = app.world.scene.find_by_name("Player").unwrap();
    let above = app.world.scene.world_position(basket) + Vec3::new(0.0, 2.5, 0.0);
    app.world.instantiate_at("prefabs/star.prefab", above, Quat::IDENTITY, None).unwrap();
    app.run_headless(90, DT);
    assert_eq!(manager(&app).score, 1, "the star fell into the basket");
    assert_eq!(text(&app, "Score"), "Score: 1");
    assert_eq!(stars(&app), 0);

    // Три промаха подряд — конец игры, рекорд 1 записан в сохранение.
    for x in [-6.0, 6.0, -8.0] {
        app.world.instantiate_at("prefabs/star.prefab", Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, None).unwrap();
    }
    app.run_headless(60 * 4, DT);
    assert!(manager(&app).game_over);
    assert_eq!(manager(&app).best, 1);
    let saved = backend.contents().expect("the best score was saved");
    assert!(saved.contains("best"), "{saved}");

    // Enter — новая игра: жизни и счёт сброшены, экран «Game over» спрятан.
    app.world.input.set_key(Key::Enter, true);
    app.run_headless(1, DT);
    app.world.input.set_key(Key::Enter, false);
    app.run_headless(1, DT);
    let state = manager(&app);
    assert!(!state.game_over);
    assert_eq!((state.score, state.lives, state.best), (0, 3, 1));
    let panel = app.world.scene.find_by_name("GameOverPanel").unwrap();
    assert!(!app.world.scene.get(panel).unwrap().active);
    assert_eq!(text(&app, "Best"), "Best: 1");

    // Новый запуск читает рекорд из того же хранилища.
    let again = game(SaveData::with_backend(backend));
    let mut again = again;
    again.run_headless(2, DT);
    assert_eq!(manager(&again).best, 1);
}

/// Ассеты шаблона: каждая ссылка из сцены и префаба указывает на существующий ассет,
/// а пак (как в браузере) даёт те же байты, что каталог.
#[test]
fn template_assets_pack_and_load_without_a_filesystem() {
    let dir = engine::assets_dir!();
    let disk = AssetDatabase::open(&dir).unwrap();
    let mut builder = engine::assets::AssetPack::builder();
    builder.add_dir(&dir).unwrap();
    let packed = AssetDatabase::from_pack(engine::assets::AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
    assert_eq!(packed.len(), disk.len());
    for path in ["sprites/basket.png", "sprites/star.png", "sprites/sky.png", "sounds/catch.wav", "prefabs/star.prefab", "scenes/main.scene"] {
        let guid = packed.guid_of(path).unwrap_or_else(|| panic!("{path} is not in the pack"));
        assert_eq!(packed.read(guid).unwrap(), disk.read(guid).unwrap(), "{path}");
    }
}

/// Интро на Scenario: отсчёт «3… 2… 1… Лови!» до первой звезды, подсказка после первой
/// пойманной звезды.
#[test]
fn the_scenario_intro_counts_down_then_reacts_to_the_first_catch() {
    let mut app = game(SaveData::in_memory());
    app.run_headless(2, DT);
    assert_eq!(text(&app, "Hint"), "3…");
    app.run_headless(30, DT);
    assert_eq!(text(&app, "Hint"), "2…");
    app.run_headless(30, DT);
    assert_eq!(text(&app, "Hint"), "1…");
    assert_eq!(manager(&app).spawned, 0, "no stars during the countdown");
    assert!(manager(&app).wait_for_intro);
    app.run_headless(30, DT);
    assert_eq!(text(&app, "Hint"), "Лови!");
    app.run_headless(3, DT);
    assert!(!manager(&app).wait_for_intro, "the intro sent Go");
    app.run_headless(80, DT);
    assert!(text(&app, "Hint").starts_with("Корзина"), "{}", text(&app, "Hint"));

    manager_mut(&mut app).spawn_interval = 1000.0;
    let basket = app.world.scene.find_by_name("Player").unwrap();
    let above = app.world.scene.world_position(basket) + Vec3::new(0.0, 2.5, 0.0);
    app.world.instantiate_at("prefabs/star.prefab", above, Quat::IDENTITY, None).unwrap();
    app.run_headless(90, DT);
    assert_eq!(manager(&app).score, 1);
    assert_eq!(text(&app, "Hint"), "Есть первая! Так держать");
    app.run_headless(150, DT);
    let hint = app.world.scene.find_by_name("Hint").unwrap();
    assert!(!app.world.scene.get(hint).unwrap().active, "the hint is hidden");
    let errors = engine::scenario::take_errors(&mut app.world);
    assert!(errors.is_empty(), "{}", errors[0]);
}
