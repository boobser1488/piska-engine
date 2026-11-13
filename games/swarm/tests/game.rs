//! Игра целиком без окна: меню, переходы, пауза, настоящий ввод (клавиатура, мышь,
//! геймпад, касания) → симуляция → интерфейс, сохранения.

use engine::prelude::*;
use swarm::bot::bot_input;
use swarm::flow::Game;
use swarm::profile::*;
use swarm::sim::{Mode, Phase};
use swarm::ui::Screen;

const DT: f32 = 1.0 / 60.0;

fn make(save: SaveData) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = swarm::build_with(save).expect("the game builds from its assets directory");
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app.run_headless(3, DT);
    app
}

fn game(app: &App) -> &Game {
    app.world.resources.get::<Game>().expect("Game resource exists after the first frames")
}

fn game_mut(app: &mut App) -> &mut Game {
    app.world.resources.get_mut::<Game>().expect("Game")
}

fn press(app: &mut App, key: Key) {
    app.world.input.set_key(key, true);
    app.tick(DT);
    app.world.input.set_key(key, false);
    app.tick(DT);
}

/// Меню → «Играть» → «Кампания» только клавиатурой.
fn start_campaign(app: &mut App) {
    assert_eq!(game(app).screen, Screen::Menu);
    press(app, Key::Tab);
    press(app, Key::Enter);
    assert_eq!(game(app).screen, Screen::ModeSelect, "Play opens the mode select");
    press(app, Key::Tab);
    press(app, Key::Enter);
    assert_eq!(game(app).screen, Screen::Playing, "Campaign starts the game");
}

/// Один кадр, в котором за игрока играет бот через настоящие устройства ввода:
/// левый стик геймпада — движение, мышь — прицел и огонь, пробел — рывок.
fn feed(app: &mut App) {
    let (mv, target, fire, dash) = {
        let g = game(app);
        let input = bot_input(&g.sim);
        let from = g.sim.player.pos;
        (input.mv, from + input.aim * 6.0, input.fire, input.dash)
    };
    let ortho = 10.4;
    let screen = Vec2::new(1280.0, 720.0);
    let px = Vec2::new(screen.x * 0.5 + target.x / (2.0 * ortho) * screen.y, screen.y * 0.5 - target.y / (2.0 * ortho) * screen.y);
    let input = &mut app.world.input;
    input.set_gamepad_axis(GamepadId(0), GamepadAxis::LeftStickX, mv.x);
    input.set_gamepad_axis(GamepadId(0), GamepadAxis::LeftStickY, mv.y);
    input.set_mouse_position(px);
    input.set_mouse_button(MouseButton::Left, fire);
    input.set_key(Key::Space, dash);
}

fn text(app: &App, name: &str) -> String {
    let id = app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("no `{name}`"));
    app.world.scene.get_component::<Text>(id).expect("Text").text.clone()
}

#[test]
fn the_game_builds_and_shows_the_main_menu() {
    let app = make(SaveData::in_memory());
    let g = game(&app);
    assert_eq!(g.screen, Screen::Menu);
    assert!(app.world.scene.find_by_name("Main Camera").is_some());
    assert!(app.world.scene.find_by_name("ArenaFloor").is_some());
    assert!(app.world.scene.len() > 300, "pools and screens were built: {}", app.world.scene.len());
    assert_eq!(text(&app, "Title"), "РОЙ");
    // Главное меню видно, HUD и игровые экраны скрыты.
    let active = |name: &str| app.world.scene.find_by_name(name).is_some_and(|id| app.world.scene.get(id).unwrap().active);
    assert!(active("MenuScreen") && !active("HUD") && !active("PauseScreen") && !active("UpgradeScreen") && !active("ResultsScreen"));
    // Бесконечный режим закрыт, пока не пройдена 10-я волна.
    assert!(!g.progress.endless_unlocked());
}

#[test]
fn menu_navigation_reaches_gameplay_by_keyboard() {
    let mut app = make(SaveData::in_memory());
    start_campaign(&mut app);
    app.run_headless(5, DT);
    let g = game(&app);
    assert_eq!(g.sim.mode, Mode::Campaign);
    assert!(g.sim.time > 0.0, "the simulation is running");
    assert!(app.world.scene.get(g.ui.hud).unwrap().active, "the HUD is shown");
    assert_eq!(text(&app, "WaveText"), "ВОЛНА 1 / 20");
}

#[test]
fn options_screen_is_navigable_and_settings_are_saved() {
    let backend = MemoryBackend::default();
    let mut app = make(SaveData::with_backend(backend.clone()));
    // Главное меню: Tab → Играть, Tab → Настройки.
    press(&mut app, Key::Tab);
    press(&mut app, Key::Tab);
    press(&mut app, Key::Enter);
    assert_eq!(game(&app).screen, Screen::Options);
    let before = game(&app).opts.music;
    // Фокус на первом ползунке (музыка); стрелка влево уменьшает значение.
    press(&mut app, Key::Tab);
    for _ in 0..4 {
        press(&mut app, Key::Left);
    }
    let after = game(&app).opts.music;
    assert!(after < before, "the slider moved: {before} -> {after}");
    // Escape возвращает в меню и записывает настройки.
    press(&mut app, Key::Escape);
    assert_eq!(game(&app).screen, Screen::Menu);
    let saved = backend.contents().expect("settings were written");
    assert!(saved.contains("opt.music"), "{saved}");
    let again = SaveData::with_backend(backend);
    assert!((Options::load(&again).music - after).abs() < 1e-4);
    // Громкость шины музыки применена.
    let audio = app.world.resources.get::<Audio>().unwrap();
    assert!((audio.bus_volume(Bus::Music) - after).abs() < 0.02, "{}", audio.bus_volume(Bus::Music));
}

#[test]
fn pause_freezes_the_game() {
    let mut app = make(SaveData::in_memory());
    start_campaign(&mut app);
    // Дадим игре развернуться: волна идёт, враги появились.
    for _ in 0..(60 * 8) {
        app.tick(DT);
    }
    assert!(game(&app).sim.alive_enemies() > 0);
    press(&mut app, Key::Escape);
    assert_eq!(game(&app).screen, Screen::Paused);
    app.run_headless(3, DT);
    assert_eq!(app.world.time.time_scale, 0.0, "time stops on pause");
    let (t, hp, pos): (f32, f32, Vec<Vec2>) = {
        let s = &game(&app).sim;
        (s.time, s.core.hp, s.enemies.iter().filter(|e| e.alive).map(|e| e.pos).collect())
    };
    app.run_headless(180, DT);
    let s = &game(&app).sim;
    assert_eq!(s.time, t, "no simulation time passes while paused");
    assert_eq!(s.core.hp, hp);
    let now: Vec<Vec2> = s.enemies.iter().filter(|e| e.alive).map(|e| e.pos).collect();
    assert_eq!(now, pos, "enemies stand still");
    // Esc снова — игра идёт.
    press(&mut app, Key::Escape);
    assert_eq!(game(&app).screen, Screen::Playing);
    app.run_headless(30, DT);
    assert!(game(&app).sim.time > t, "the game resumed");
    assert_eq!(app.world.time.time_scale, 1.0);
}

#[test]
fn a_scripted_input_run_wins_the_first_wave_and_picks_an_upgrade() {
    let mut app = make(SaveData::in_memory());
    start_campaign(&mut app);
    let mut frames = 0;
    let mut chosen = false;
    loop {
        if game(&app).screen == Screen::Upgrade {
            // Карточки: клавиша 1.
            app.world.input.set_key(Key::Digit1, true);
            app.tick(DT);
            app.world.input.set_key(Key::Digit1, false);
            chosen = true;
        }
        feed(&mut app);
        app.tick(DT);
        frames += 1;
        let g = game(&app);
        if chosen && g.sim.wave >= 2 && g.sim.phase == Phase::Wave {
            break;
        }
        assert!(frames < 60 * 240, "wave 2 begins: wave {} phase {:?} screen {:?}", g.sim.wave, g.sim.phase, g.screen);
        assert!(!g.sim.is_over(), "the bot did not lose");
    }
    let g = game(&app);
    assert!(g.sim.run.kills >= 8, "the first wave was fought: {} kills", g.sim.run.kills);
    assert!(g.upgrade_screens >= 1 && g.sim.run.upgrades >= 1, "an upgrade was picked on the cards screen");
    assert!(g.sim.score > 0 && g.sim.xp >= 0.0);
    assert!(text(&app, "ScoreText") != "0", "the HUD shows the score");
    // Контрольная точка записана: «Продолжить» начнёт со 2-й волны.
    let cp = load_checkpoint(app.world.resources.get::<SaveData>().unwrap()).expect("checkpoint after wave 1");
    assert_eq!(cp.next_wave, 2);
}

#[test]
fn losing_shows_results_records_the_run_and_can_restart() {
    let backend = MemoryBackend::default();
    let mut app = make(SaveData::with_backend(backend.clone()));
    start_campaign(&mut app);
    for _ in 0..120 {
        app.tick(DT);
    }
    game_mut(&mut app).sim.hurt_core(1.0e6);
    for _ in 0..(60 * 4) {
        app.tick(DT);
        if game(&app).screen == Screen::Results {
            break;
        }
    }
    assert_eq!(game(&app).screen, Screen::Results, "the core is gone: results are shown");
    let g = game(&app);
    assert!(!g.result_won);
    assert_eq!(g.progress.runs, 1);
    let save = app.world.resources.get::<SaveData>().unwrap();
    assert_eq!(load_leaderboard(save).len(), 1, "the run is on the leaderboard");
    assert!(load_checkpoint(save).is_none());
    assert!(backend.contents().unwrap().contains("board"));
    // «Играть снова» — Tab → первая кнопка, Enter.
    press(&mut app, Key::Tab);
    press(&mut app, Key::Enter);
    assert_eq!(game(&app).screen, Screen::Playing);
    app.run_headless(3, DT);
    let s = &game(&app).sim;
    assert_eq!(s.core.hp, s.core.max_hp, "a fresh run");
    assert_eq!(s.score, 0);
}

#[test]
fn reaching_wave_ten_unlocks_endless_mode_and_it_is_saved() {
    let backend = MemoryBackend::default();
    let mut app = make(SaveData::with_backend(backend.clone()));
    start_campaign(&mut app);
    game_mut(&mut app).sim.start_wave(10);
    app.run_headless(5, DT);
    assert!(game(&app).progress.endless_unlocked(), "wave 10 unlocks it");
    assert!(backend.contents().unwrap().contains("prog.max_wave"));
    // На экране выбора режима кнопка бесконечного режима активна.
    game_mut(&mut app).sim.hurt_core(1.0e6);
    for _ in 0..(60 * 4) {
        app.tick(DT);
    }
    press(&mut app, Key::Down);
    press(&mut app, Key::Down); // «В главное меню»
    press(&mut app, Key::Enter);
    assert_eq!(game(&app).screen, Screen::Menu);
    press(&mut app, Key::Tab);
    press(&mut app, Key::Enter);
    assert_eq!(game(&app).screen, Screen::ModeSelect);
    let endless = game(&app).ui.endless_btn;
    assert!(app.world.scene.get_component::<Button>(endless).unwrap().interactable);
    // Второй вход — и новая игра в бесконечном режиме: Tab, Tab → «Бесконечный режим».
    press(&mut app, Key::Tab);
    press(&mut app, Key::Tab);
    press(&mut app, Key::Enter);
    assert_eq!(game(&app).screen, Screen::Playing);
    assert_eq!(game(&app).sim.mode, Mode::Endless);
    assert_eq!(text(&app, "WaveText"), "ВОЛНА 1");
}

#[test]
fn touch_sticks_move_aim_and_fire() {
    let mut app = make(SaveData::in_memory());
    start_campaign(&mut app);
    for _ in 0..180 {
        app.tick(DT);
    }
    let start = game(&app).sim.player.pos;
    // Левый палец тянет вправо (движение), правый — вправо (прицел и огонь).
    app.world.input.set_touch(1, Vec2::new(200.0, 500.0), TouchPhase::Began);
    app.world.input.set_touch(2, Vec2::new(900.0, 400.0), TouchPhase::Began);
    app.tick(DT);
    for _ in 0..40 {
        app.world.input.set_touch(1, Vec2::new(280.0, 500.0), TouchPhase::Moved);
        app.world.input.set_touch(2, Vec2::new(990.0, 400.0), TouchPhase::Moved);
        app.tick(DT);
    }
    let g = game(&app);
    assert!(g.sim.player.pos.x > start.x + 0.5, "the left stick moved the drone: {:?} -> {:?}", start, g.sim.player.pos);
    assert!(g.sim.player.aim.x > 0.9, "the right stick aims right: {:?}", g.sim.player.aim);
    assert!(g.sim.run.shots > 0, "and fires");
    let dash = app.world.scene.get(g.ui.dash_btn).unwrap();
    assert!(dash.active, "the touch controls are shown once a touch is seen");
}

#[test]
fn gamepad_start_pauses_and_south_dashes() {
    let mut app = make(SaveData::in_memory());
    start_campaign(&mut app);
    for _ in 0..60 {
        app.tick(DT);
    }
    app.world.input.set_gamepad_axis(GamepadId(0), GamepadAxis::LeftStickX, 1.0);
    app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::South, true);
    app.tick(DT);
    app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::South, false);
    app.tick(DT);
    assert!(game(&app).sim.player.dash_cd > 0.0, "the South button dashes");
    app.world.input.set_gamepad_axis(GamepadId(0), GamepadAxis::LeftStickX, 0.0);
    app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::Start, true);
    app.tick(DT);
    app.world.input.set_gamepad_button(GamepadId(0), GamepadButton::Start, false);
    app.tick(DT);
    assert_eq!(game(&app).screen, Screen::Paused, "Start pauses");
}

#[test]
fn boss_waves_show_a_boss_bar_and_music_changes() {
    let mut app = make(SaveData::in_memory());
    start_campaign(&mut app);
    game_mut(&mut app).sim.start_wave(5);
    for _ in 0..(60 * 6) {
        app.tick(DT);
    }
    let g = game(&app);
    assert!(g.sim.boss().is_some(), "wave 5 has a boss");
    assert_eq!(g.track, swarm::flow::Track::Boss, "the boss theme plays");
    let bar = app.world.scene.find_by_name("BossBar").unwrap();
    assert!(app.world.scene.get(bar).unwrap().active, "the boss health bar is visible");
}
