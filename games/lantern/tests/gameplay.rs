//! Игра без окна: уровни, прохождение ботом, враги, шипы, контрольные точки, пауза, меню,
//! сохранения. Всё детерминировано: шаг 1/60 с, ввод пишется в `world.input` вручную.

use engine::prelude::*;
use lantern::objects::{Checkpoint, Exit, Firefly, Gate, MovingPlatform};
use lantern::phys::Shared;
use lantern::{LevelManager, LevelState, Player, Progress, LEVELS, LEVEL_COUNT};

const DT: f32 = 1.0 / 60.0;

fn quiet() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ENGINE_AUDIO", "off"));
}

fn game_with(save: SaveData) -> App {
    quiet();
    let mut app = lantern::build_with(save).expect("the game builds from its assets directory");
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app
}

fn game() -> App {
    game_with(SaveData::in_memory())
}

/// Загрузить уровень `n` (с нуля) и прогнать пару кадров: объекты уровня появляются на первом.
fn level(app: &mut App, n: usize) {
    lantern::goto_level(app, n);
    app.run_headless(4, DT);
    assert!(app.world.scene.find_by_name("Level").is_some(), "level {n} has a manager");
}

fn mgr(app: &App) -> &LevelManager {
    let id = app.world.scene.find_by_name("Level").expect("Level object");
    app.world.scene.get_component::<LevelManager>(id).expect("LevelManager")
}

fn mgr_mut(app: &mut App) -> &mut LevelManager {
    let id = app.world.scene.find_by_name("Level").expect("Level object");
    app.world.scene.get_component_mut::<LevelManager>(id).expect("LevelManager")
}

fn player_id(app: &App) -> GameObjectId {
    app.world.scene.find_by_tag("Player").expect("Player")
}

fn player(app: &App) -> &Player {
    app.world.scene.get_component::<Player>(player_id(app)).unwrap()
}

fn player_mut(app: &mut App) -> &mut Player {
    let id = player_id(app);
    app.world.scene.get_component_mut::<Player>(id).unwrap()
}

fn feet(app: &App) -> Vec2 {
    app.world.scene.world_position(player_id(app)).truncate()
}

fn teleport(app: &mut App, to: Vec2) {
    let id = player_id(app);
    app.world.scene.get_mut(id).unwrap().transform.position = to.extend(0.0);
    let p = player_mut(app);
    p.vel = Vec2::ZERO;
    p.safe_pos = to;
}

fn count<T: Component>(app: &App) -> usize {
    app.world.scene.find_with::<T>().len()
}

/// Все «ножки» объектов с компонентом `T`.
fn positions<T: Component>(app: &App) -> Vec<Vec2> {
    app.world.scene.find_with::<T>().into_iter().map(|id| app.world.scene.world_position(id).truncate()).collect()
}

fn tick_n(app: &mut App, n: u32) {
    app.run_headless(n, DT);
}

fn key(app: &mut App, k: Key, frames: u32) {
    app.world.input.set_key(k, true);
    tick_n(app, frames);
    app.world.input.set_key(k, false);
    tick_n(app, 1);
}

/// Фокус клавиатурой на кнопку с данным именем и Enter.
fn press_button(app: &mut App, name: &str) {
    for _ in 0..30 {
        let focused = app.world.ui().focused().and_then(|id| app.world.scene.get(id)).map(|o| o.name.clone());
        if focused.as_deref() == Some(name) {
            key(app, Key::Enter, 1);
            return;
        }
        key(app, Key::Tab, 1);
    }
    panic!("button `{name}` is not reachable with Tab");
}

// ------------------------------------------------------------------ уровни

#[test]
fn every_level_loads_and_can_be_finished_in_principle() {
    let mut app = game();
    for n in 0..LEVEL_COUNT {
        level(&mut app, n);
        let m = mgr(&app);
        assert_eq!(m.index as usize, n);
        assert!(!m.title.is_empty());
        assert!(m.total > 0 && m.need > 0, "level {n} has fireflies");
        assert!(m.need <= m.total, "level {n}: the exit needs {} but only {} fireflies exist", m.need, m.total);

        let id = app.world.scene.find_by_name("Tiles").expect("Tiles layer");
        let (w, h) = app.world.scene.get_component::<Tilemap>(id).unwrap().size();
        assert!(w >= 120 && h >= 30, "level {n} is {w}x{h}");
        let shared = app.world.resources.get::<Shared>().unwrap();
        assert_eq!((shared.grid.borrow().width as usize, shared.grid.borrow().height as usize), (w, h));

        assert_eq!(count::<Player>(&app), 1);
        assert_eq!(count::<Exit>(&app), 1, "level {n} has one exit");
        assert!(count::<Checkpoint>(&app) >= 1, "level {n} has checkpoints");
        let golden = app.world.scene.find_with::<Firefly>().iter().filter(|&&id| app.world.scene.get_component::<Firefly>(id).unwrap().golden).count();
        assert_eq!(golden, 1, "level {n} hides exactly one golden firefly");
        // ворота не требуют больше светлячков, чем есть
        for id in app.world.scene.find_with::<Gate>() {
            let need = app.world.scene.get_component::<Gate>(id).unwrap().need;
            assert!(need <= m.total, "level {n}: gate needs {need} of {}", m.total);
        }
        // игрок не внутри стены, под ногами опора
        let f = feet(&app);
        let g = shared.grid.borrow();
        assert!(!g.solid_at(f + Vec2::new(0.0, 0.5)), "level {n}: the player starts inside a wall");
    }
}

#[test]
fn level_content_grows_with_the_curve() {
    let mut app = game();
    let mut kinds = Vec::new();
    for n in 0..LEVEL_COUNT {
        level(&mut app, n);
        let enemies = count::<lantern::enemies::Crawler>(&app)
            + count::<lantern::enemies::Bat>(&app)
            + count::<lantern::enemies::Spitter>(&app)
            + count::<lantern::enemies::Brute>(&app);
        kinds.push(enemies);
    }
    assert!(kinds[5] > kinds[0] && kinds[3] > kinds[0], "{kinds:?}");
    level(&mut app, 5);
    assert_eq!(count::<lantern::boss::Boss>(&app), 1, "the last level has the boss");
    level(&mut app, 3);
    assert!(count::<MovingPlatform>(&app) >= 2);
}

// ------------------------------------------------------------------ бот проходит первый уровень

struct Bot {
    hold: u32,
    gap: u32,
}

/// Бежит вправо, прыгает перед стеной, ямой, шипами и врагами.
fn drive(app: &mut App, bot: &mut Bot) {
    let id = player_id(app);
    let (grounded, vel_y) = {
        let p = app.world.scene.get_component::<Player>(id).unwrap();
        (p.grounded, p.vel.y)
    };
    let f = app.world.scene.world_position(id).truncate();
    let mut want_jump = false;
    {
        let shared = app.world.resources.get::<Shared>().unwrap().clone();
        let grid = shared.grid.borrow();
        if grounded && bot.gap == 0 {
            for dx in [1.0f32, 1.8, 2.5] {
                for dy in [0.5f32, 1.5] {
                    let c = f + Vec2::new(dx, dy);
                    if grid.solid_at(c) {
                        want_jump = true;
                    }
                }
                let c = f + Vec2::new(dx, 0.5);
                if grid.spike_tile(c.x.floor() as i32, c.y.floor() as i32) != 0 {
                    want_jump = true;
                }
            }
            if !grid.support_at(f + Vec2::new(0.55, -0.4)) {
                want_jump = true;
            }
        }
    }
    if grounded && bot.gap == 0 {
        let near = |p: Vec2| p.x > f.x + 0.3 && p.x < f.x + 3.2 && (p.y - f.y).abs() < 2.5;
        if positions::<lantern::enemies::Crawler>(app).into_iter().any(near)
            || positions::<lantern::enemies::Bat>(app).into_iter().any(near)
            || positions::<lantern::objects::Projectile>(app).into_iter().any(near)
        {
            want_jump = true;
        }
    }
    if want_jump {
        bot.hold = 24;
        bot.gap = 12;
    }
    bot.gap = bot.gap.saturating_sub(1);
    let _ = vel_y;
    app.world.input.set_key(Key::D, true);
    app.world.input.set_key(Key::Space, bot.hold > 0);
    bot.hold = bot.hold.saturating_sub(1);
}

#[test]
fn a_scripted_run_completes_the_first_level() {
    let mut app = game();
    level(&mut app, 0);
    let mut bot = Bot { hold: 0, gap: 0 };
    let mut frames = 0;
    let mut trail: Vec<(u32, f32, f32, i32)> = Vec::new();
    while mgr(&app).state != LevelState::Won && frames < 60 * 150 {
        drive(&mut app, &mut bot);
        tick_n(&mut app, 1);
        frames += 1;
        if frames % 6 == 0 {
            trail.push((frames, feet(&app).x, feet(&app).y, player(&app).hp));
        }
        if mgr(&app).state == LevelState::Lost {
            // бот может ошибиться: засчитываем только честное прохождение, но не зависаем
            let tail = &trail[trail.len().saturating_sub(14)..];
            panic!("the bot lost at x={:.1} after {frames} frames (hp {}); trail (frame,x,y,hp) {:?}", feet(&app).x, player(&app).hp, tail);
        }
    }
    let m = mgr(&app);
    assert_eq!(m.state, LevelState::Won, "bot did not reach the exit in time (x={:.1})", feet(&app).x);
    assert!(m.fireflies >= m.need, "{} of {} fireflies", m.fireflies, m.need);
    app.world.input.set_key(Key::D, false);
    app.world.input.set_key(Key::Space, false);
    tick_n(&mut app, 120);
    // результаты записаны и следующий уровень открыт
    let progress = app.world.resources.get::<Progress>().unwrap();
    assert!(progress.completed[0] && progress.unlocked == 2);
    assert!(progress.stars[0] >= 1);
    assert!(progress.best_time[0] > 5.0);
    // экран результатов показан
    let dim = app.world.scene.find_by_name("ResultsDim").unwrap();
    assert!(app.world.scene.get(dim).unwrap().active);
    assert_eq!(app.world.time.time_scale, 0.0);
}

// ------------------------------------------------------------------ механики

#[test]
fn stomping_a_crawler_kills_it_and_bounces_the_player() {
    let mut app = game();
    level(&mut app, 0);
    let crawler = positions::<lantern::enemies::Crawler>(&app)[0];
    let before = count::<lantern::enemies::Crawler>(&app);
    teleport(&mut app, crawler + Vec2::new(0.0, 2.2));
    player_mut(&mut app).vel = Vec2::new(0.0, -6.0);
    let mut bounced = false;
    for _ in 0..40 {
        tick_n(&mut app, 1);
        bounced |= player(&app).vel.y > 8.0;
    }
    assert_eq!(count::<lantern::enemies::Crawler>(&app), before - 1, "the crawler was stomped");
    assert!(bounced, "the player bounces off the enemy");
    assert_eq!(player(&app).hp, 3, "stomping does not hurt");
    assert_eq!(mgr(&app).kills, 1);
}

#[test]
fn touching_an_enemy_from_the_side_hurts() {
    let mut app = game();
    level(&mut app, 0);
    let crawler = positions::<lantern::enemies::Crawler>(&app)[0];
    teleport(&mut app, crawler + Vec2::new(-0.5, 0.0));
    tick_n(&mut app, 4);
    assert_eq!(player(&app).hp, 2);
    assert!(player(&app).invuln > 1.0, "brief invulnerability after a hit");
    // повторное касание в период неуязвимости не ранит
    tick_n(&mut app, 10);
    assert_eq!(player(&app).hp, 2);
}

#[test]
fn spikes_hurt_and_send_the_player_back_to_safe_ground() {
    let mut app = game();
    level(&mut app, 0);
    // шипы на втором уступе уровня 1: клетки (56..57, 7)
    let safe = Vec2::new(54.5, 7.0);
    teleport(&mut app, safe);
    tick_n(&mut app, 30);
    assert!(player(&app).grounded);
    assert_eq!(player(&app).safe_pos.x.round(), 55.0_f32.min(player(&app).safe_pos.x.round()));
    teleport(&mut app, Vec2::new(56.5, 7.0));
    player_mut(&mut app).safe_pos = safe;
    tick_n(&mut app, 3);
    assert_eq!(player(&app).hp, 2, "spikes cost a heart");
    let f = feet(&app);
    assert!((f - safe).length() < 0.5, "the player is returned to safe ground, now at {f}");
}

#[test]
fn falling_into_a_pit_costs_a_heart_and_returns_to_safe_ground() {
    let mut app = game();
    level(&mut app, 0);
    let safe = feet(&app);
    teleport(&mut app, Vec2::new(29.5, 3.0)); // над первой ямой (x=28..31)
    player_mut(&mut app).safe_pos = safe;
    tick_n(&mut app, 90);
    assert_eq!(player(&app).hp, 2);
    assert!((feet(&app) - safe).length() < 1.0);
}

#[test]
fn checkpoint_sets_the_respawn_and_defeat_returns_there() {
    let mut app = game();
    level(&mut app, 0);
    let cp = positions::<Checkpoint>(&app)[0];
    teleport(&mut app, cp);
    tick_n(&mut app, 3);
    assert_eq!(mgr(&app).checkpoint, cp, "touching the lamp stores the respawn point");

    // проиграть: три ранения подряд
    teleport(&mut app, cp + Vec2::new(8.0, 0.0));
    player_mut(&mut app).hp = 1;
    player_mut(&mut app).invuln = 0.0;
    let id = player_id(&app);
    app.world.send_message(id, "Hurt", Value::Vec3(1.0, 0.0, 0.0));
    tick_n(&mut app, 90);
    assert_eq!(mgr(&app).state, LevelState::Lost);
    assert_eq!(mgr(&app).deaths, 1);
    let dim = app.world.scene.find_by_name("ResultsDim").unwrap();
    assert!(app.world.scene.get(dim).unwrap().active, "defeat screen");
    assert_eq!(app.world.time.time_scale, 0.0);

    // «Продолжить с контрольной точки»
    press_button(&mut app, "retry_cp");
    tick_n(&mut app, 3);
    assert_eq!(mgr(&app).state, LevelState::Playing);
    assert_eq!(app.world.time.time_scale, 1.0);
    assert!(!player(&app).dead);
    assert_eq!(player(&app).hp, 3);
    assert!((feet(&app) - cp).length() < 1.0, "respawned at the checkpoint, now at {}", feet(&app));
}

#[test]
fn double_jump_is_locked_until_level_two_is_finished() {
    // без прогресса второго прыжка нет
    let mut app = game();
    level(&mut app, 0);
    assert!(!player(&app).has_double_jump);
    tick_n(&mut app, 30);
    key(&mut app, Key::Space, 6);
    tick_n(&mut app, 12);
    assert!(!player(&app).grounded);
    let apex_vel = player(&app).vel.y;
    key(&mut app, Key::Space, 2);
    assert!(player(&app).vel.y <= apex_vel + 0.5, "no second jump without the ability: {} -> {}", apex_vel, player(&app).vel.y);

    // после прохождения второго уровня — есть
    let mut app = game();
    app.world.resources.get_mut::<Progress>().unwrap().record(1, &lantern::RunResult { time: 60.0, fireflies: 10, total: 10, golden: false, deaths: 0 }, 100.0);
    level(&mut app, 2);
    assert!(player(&app).has_double_jump);
    tick_n(&mut app, 30);
    key(&mut app, Key::Space, 6);
    tick_n(&mut app, 14);
    assert!(!player(&app).grounded);
    let before = player(&app).vel.y;
    key(&mut app, Key::Space, 2);
    assert!(player(&app).vel.y > before + 5.0, "second jump in the air: {} -> {}", before, player(&app).vel.y);
    // и только один
    tick_n(&mut app, 6);
    let v = player(&app).vel.y;
    key(&mut app, Key::Space, 2);
    assert!(player(&app).vel.y <= v + 0.5, "no third jump");
}

#[test]
fn dash_passes_through_a_brute_only_when_unlocked() {
    let mut app = game();
    app.world.resources.get_mut::<Progress>().unwrap().record(3, &lantern::RunResult { time: 90.0, fireflies: 10, total: 10, golden: false, deaths: 0 }, 100.0);
    level(&mut app, 4);
    assert!(player(&app).has_dash);
    // туннель с тяжеляком: пол на высоте 6, тяжеляк около x=48
    let brute = positions::<lantern::enemies::Brute>(&app)[0];
    // ставим игрока перед тяжеляком и рывок: враг не ранит
    teleport(&mut app, Vec2::new(brute.x - 2.0, 6.0));
    tick_n(&mut app, 10);
    app.world.input.set_key(Key::D, true);
    key(&mut app, Key::LShift, 1);
    let start = feet(&app).x;
    let mut hurt = false;
    for _ in 0..14 {
        tick_n(&mut app, 1);
        hurt |= player(&app).hp < 3;
    }
    app.world.input.set_key(Key::D, false);
    assert!(!hurt, "a dash is invulnerable");
    assert!(feet(&app).x > start + 2.5, "the dash covers ground: {} -> {}", start, feet(&app).x);

    // без рывка тяжеляк ранит
    let mut app = game();
    level(&mut app, 4);
    assert!(!player(&app).has_dash);
    let brute = positions::<lantern::enemies::Brute>(&app)[0];
    teleport(&mut app, Vec2::new(brute.x - 0.6, 6.0));
    tick_n(&mut app, 8);
    assert!(player(&app).hp < 3, "a brute hurts");
}

#[test]
fn a_moving_platform_carries_the_player() {
    let mut app = game();
    level(&mut app, 3);
    let (id, from) = {
        let id = app.world.scene.find_with::<MovingPlatform>()[0];
        let p = app.world.scene.get_component::<MovingPlatform>(id).unwrap();
        (id, p.from)
    };
    let start_x = from.x;
    teleport(&mut app, Vec2::new(start_x, from.y + 0.2));
    tick_n(&mut app, 20);
    assert!(player(&app).grounded && player(&app).platform >= 0, "standing on the platform");
    let x0 = feet(&app).x;
    tick_n(&mut app, 120);
    let moved_platform = app.world.scene.world_position(id).x - start_x;
    assert!(moved_platform > 1.0, "the platform moved {moved_platform}");
    assert!((feet(&app).x - x0 - (app.world.scene.world_position(id).x - start_x - (x0 - start_x))).abs() < 0.6);
    assert!(feet(&app).x > x0 + 0.8, "the rider went with it: {} -> {}", x0, feet(&app).x);
    assert!((feet(&app).y - app.world.scene.world_position(id).y).abs() < 0.2);
}

#[test]
fn the_gate_opens_only_with_enough_fireflies() {
    let mut app = game();
    level(&mut app, 1);
    let gate = app.world.scene.find_with::<Gate>()[0];
    let pos = app.world.scene.world_position(gate).truncate();
    let need = app.world.scene.get_component::<Gate>(gate).unwrap().need;
    teleport(&mut app, pos + Vec2::new(-1.2, 0.0));
    tick_n(&mut app, 20);
    assert!(!app.world.scene.get_component::<Gate>(gate).unwrap().open, "closed gate");
    // игрок не проходит
    app.world.input.set_key(Key::D, true);
    tick_n(&mut app, 40);
    app.world.input.set_key(Key::D, false);
    assert!(feet(&app).x < pos.x, "the gate blocks the way: {} vs {}", feet(&app).x, pos.x);
    mgr_mut(&mut app).fireflies = need;
    tick_n(&mut app, 5);
    assert!(app.world.scene.get_component::<Gate>(gate).is_none_or(|g| g.open), "open with enough fireflies");
}

#[test]
fn collecting_fireflies_lights_the_exit() {
    let mut app = game();
    level(&mut app, 0);
    let m = mgr(&app);
    assert!(!m.exit_ready());
    let (need, total) = (m.need, m.total);
    let fireflies = positions::<Firefly>(&app);
    assert_eq!(fireflies.len() as i32, total + 1, "regular fireflies plus the golden one");
    for p in fireflies.iter().take(need as usize) {
        teleport(&mut app, *p - Vec2::new(0.0, 0.5));
        tick_n(&mut app, 2);
    }
    assert!(mgr(&app).fireflies >= need || mgr(&app).golden);
    for _ in 0..40 {
        if mgr(&app).fireflies >= need {
            break;
        }
        let rest = positions::<Firefly>(&app);
        teleport(&mut app, rest[0] - Vec2::new(0.0, 0.5));
        tick_n(&mut app, 2);
    }
    assert!(mgr(&app).exit_ready());
    tick_n(&mut app, 3);
    let exit = app.world.scene.find_with::<Exit>()[0];
    assert!(app.world.scene.get_component::<Exit>(exit).unwrap().active);
}

// ------------------------------------------------------------------ пауза

#[test]
fn pause_freezes_the_game() {
    let mut app = game();
    level(&mut app, 0);
    app.world.input.set_key(Key::D, true);
    tick_n(&mut app, 30);
    let x = feet(&app).x;
    assert!(x > 5.0, "running: {x}");
    // Esc — пауза
    key(&mut app, Key::Escape, 1);
    assert_eq!(mgr(&app).state, LevelState::Paused);
    assert_eq!(app.world.time.time_scale, 0.0);
    let (t0, x0) = (mgr(&app).time, feet(&app).x);
    tick_n(&mut app, 120);
    assert_eq!(mgr(&app).time, t0, "the clock stands");
    assert_eq!(feet(&app).x, x0, "the player stands");
    let dim = app.world.scene.find_by_name("PauseDim").unwrap();
    assert!(app.world.scene.get(dim).unwrap().active);
    // враги тоже стоят
    let crawlers = positions::<lantern::enemies::Crawler>(&app);
    tick_n(&mut app, 30);
    assert_eq!(positions::<lantern::enemies::Crawler>(&app), crawlers);
    // Esc ещё раз — продолжаем
    key(&mut app, Key::Escape, 1);
    assert_eq!(mgr(&app).state, LevelState::Playing);
    assert_eq!(app.world.time.time_scale, 1.0);
    tick_n(&mut app, 30);
    assert!(feet(&app).x > x0 + 1.0);
    app.world.input.set_key(Key::D, false);
}

#[test]
fn pause_menu_buttons_work() {
    let mut app = game();
    level(&mut app, 0);
    key(&mut app, Key::Escape, 1);
    press_button(&mut app, "resume");
    tick_n(&mut app, 2);
    assert_eq!(mgr(&app).state, LevelState::Playing);
    // «В главное меню»
    key(&mut app, Key::Escape, 1);
    press_button(&mut app, "menu");
    tick_n(&mut app, 4);
    assert!(app.world.scene.find_by_name("Menu").is_some(), "back in the menu");
    assert_eq!(app.world.time.time_scale, 1.0);
}

// ------------------------------------------------------------------ меню

#[test]
fn the_menu_leads_into_the_game() {
    let mut app = game();
    app.run_headless(4, DT);
    assert!(app.world.scene.find_by_name("Menu").is_some());
    assert!(app.world.scene.find_by_name("Level").is_none());
    press_button(&mut app, "play");
    tick_n(&mut app, 4);
    assert!(app.world.scene.find_by_name("Level").is_some(), "the first level is running");
    assert_eq!(mgr(&app).index, 0);
}

#[test]
fn level_select_shows_progress_and_locks_the_rest() {
    let mut app = game();
    {
        let p = app.world.resources.get_mut::<Progress>().unwrap();
        p.record(0, &lantern::RunResult { time: 70.0, fireflies: 40, total: 45, golden: true, deaths: 0 }, 100.0);
    }
    // меню строится при загрузке сцены: перезагрузим его
    app.world.scenes().load("menu");
    tick_n(&mut app, 4);
    press_button(&mut app, "play"); // у игрока есть прогресс: «Уровни»
    tick_n(&mut app, 2);
    let lvl2 = app.world.scene.find_by_name("level_1").unwrap();
    let lvl3 = app.world.scene.find_by_name("level_2").unwrap();
    assert!(app.world.scene.get_component::<Button>(lvl2).unwrap().interactable, "level 2 is open");
    assert!(!app.world.scene.get_component::<Button>(lvl3).unwrap().interactable, "level 3 is locked");
    // выбрать второй уровень
    press_button(&mut app, "level_1");
    tick_n(&mut app, 4);
    assert_eq!(mgr(&app).index, 1);
}

#[test]
fn options_change_the_volumes_and_are_saved() {
    let backend = MemoryBackend::default();
    let mut app = game_with(SaveData::with_backend(backend.clone()));
    app.run_headless(3, DT);
    press_button(&mut app, "options");
    tick_n(&mut app, 2);
    // слайдер музыки: стрелка влево уменьшает значение
    press_slider_left(&mut app, "music_vol");
    let music = app.world.resources.get::<Progress>().unwrap().music;
    assert!(music < 0.6, "music volume went down: {music}");
    let audio = app.world.resources.get::<Audio>().unwrap();
    assert!((audio.bus_volume(Bus::Music) - music).abs() < 0.02);
    press_button(&mut app, "back_options");
    tick_n(&mut app, 2);
    let text = backend.contents().expect("the options were written");
    assert!(text.contains("opt.music"), "{text}");
    // новый запуск читает громкость
    let again = game_with(SaveData::with_backend(backend));
    assert!((again.world.resources.get::<Progress>().unwrap().music - music).abs() < 0.02);
}

fn press_slider_left(app: &mut App, name: &str) {
    for _ in 0..40 {
        let focused = app.world.ui().focused().and_then(|id| app.world.scene.get(id)).map(|o| o.name.clone());
        if focused.as_deref() == Some(name) {
            key(app, Key::Left, 1);
            key(app, Key::Left, 1);
            return;
        }
        key(app, Key::Tab, 1);
    }
    panic!("slider `{name}` is not reachable");
}

// ------------------------------------------------------------------ босс

#[test]
fn the_boss_fight_can_be_won() {
    use lantern::boss::{st, Boss, BOSS_HP};
    let mut app = game();
    app.world.resources.get_mut::<Progress>().unwrap().record(3, &lantern::RunResult { time: 90.0, fireflies: 10, total: 10, golden: false, deaths: 0 }, 100.0);
    level(&mut app, 5);
    let boss_id = app.world.scene.find_with::<Boss>()[0];
    // Игрок входит в арену: дверь закрывается, бой начинается
    teleport(&mut app, Vec2::new(131.0, 6.0));
    tick_n(&mut app, 5);
    assert_eq!(app.world.scene.get_component::<Boss>(boss_id).unwrap().state, st::INTRO);
    let shared = app.world.resources.get::<Shared>().unwrap().clone();
    assert!(shared.grid.borrow().is_solid(125, 7), "the arena door is shut");
    assert!(mgr(&app).checkpoint.x > 126.0, "respawn moved inside the arena");
    tick_n(&mut app, 150);
    assert_eq!(app.world.scene.get_component::<Boss>(boss_id).unwrap().state, st::HOVER);

    // девять прыжков на голову
    for i in 0..BOSS_HP {
        // дождаться, пока босс уязвим
        for _ in 0..600 {
            let b = app.world.scene.get_component::<Boss>(boss_id).unwrap();
            if b.invuln <= 0.0 && (b.state == st::HOVER || b.state == st::DAZED) {
                break;
            }
            player_mut(&mut app).hp = 3;
            player_mut(&mut app).invuln = 5.0;
            tick_n(&mut app, 1);
        }
        let boss_pos = app.world.scene.world_position(boss_id).truncate();
        teleport(&mut app, boss_pos + Vec2::new(0.0, 1.3));
        player_mut(&mut app).vel = Vec2::new(0.0, -4.0);
        player_mut(&mut app).invuln = 5.0;
        tick_n(&mut app, 8);
        let hp = app.world.scene.get_component::<Boss>(boss_id).map_or(0, |b| b.hp);
        assert_eq!(hp, BOSS_HP - 1 - i, "stomp {i} counted");
    }
    tick_n(&mut app, 200);
    assert!(mgr(&app).boss_dead, "the boss is defeated");
    assert!(app.world.scene.get_component::<Boss>(boss_id).is_none());
    mgr_mut(&mut app).fireflies = mgr(&app).need;
    assert!(mgr(&app).exit_ready());
}

// ------------------------------------------------------------------ сохранение целиком

#[test]
fn finishing_a_level_unlocks_the_next_one_across_restarts() {
    let backend = MemoryBackend::default();
    let mut app = game_with(SaveData::with_backend(backend.clone()));
    level(&mut app, 0);
    // победа вручную: все светлячки собраны, игрок у выхода
    {
        let m = mgr_mut(&mut app);
        m.fireflies = m.need + 3;
        m.time = 80.0;
    }
    let exit = positions::<Exit>(&app)[0];
    teleport(&mut app, exit);
    tick_n(&mut app, 10);
    assert_eq!(mgr(&app).state, LevelState::Won);
    let again = game_with(SaveData::with_backend(backend));
    let p = again.world.resources.get::<Progress>().unwrap();
    assert_eq!(p.unlocked, 2);
    assert!(p.completed[0]);
    assert!(p.best_time[0] > 0.0);
    assert!(LEVELS[1].scene == "level2");
}

// ------------------------------------------------------------------ касания

/// Касание пальцем: Began в одном кадре, Ended в следующем (первое касание ведёт и мышь).
fn tap(app: &mut App, at: Vec2) {
    app.world.input.set_touch(1, at, TouchPhase::Began);
    tick_n(app, 1);
    app.world.input.set_touch(1, at, TouchPhase::Ended);
    tick_n(app, 2);
}

fn screen_active(app: &App, name: &str) -> bool {
    let id = app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("no `{name}`"));
    app.world.scene.get(id).unwrap().active
}

#[test]
fn touches_tap_menu_buttons_and_level_cards() {
    let mut app = game();
    app.run_headless(4, DT);
    // «Настройки» (вторая кнопка, пока прогресса нет) открывают настройки, а не игру
    tap(&mut app, Vec2::new(640.0, 491.0));
    assert!(app.world.scene.find_by_name("Level").is_none(), "a tap on Options must not start the game");
    assert!(screen_active(&app, "ScreenOptions"));
    assert!(!screen_active(&app, "ScreenMain"));
    // «Назад»
    press_button(&mut app, "back_options");
    tick_n(&mut app, 2);

    // с прогрессом: «Уровни» -> карточка второго уровня
    app.world.resources.get_mut::<Progress>().unwrap().record(0, &lantern::RunResult { time: 70.0, fireflies: 40, total: 45, golden: true, deaths: 0 }, 100.0);
    app.world.scenes().load("menu");
    tick_n(&mut app, 4);
    tap(&mut app, Vec2::new(640.0, 491.0));
    assert!(screen_active(&app, "ScreenLevels"), "the Levels button opens level select");
    tap(&mut app, Vec2::new(640.0, 251.0));
    tick_n(&mut app, 4);
    assert_eq!(mgr(&app).index, 1, "a tap on the second card starts level 2");
}

#[test]
fn touch_pads_drive_the_player_without_clicking_the_ui() {
    let mut app = game();
    level(&mut app, 0);
    // кнопка «вправо» — вторая слева (120×120, отступ 28, зазор 16 в единицах холста)
    let right_pad = Vec2::new(28.0 + 136.0 + 60.0, 720.0 - 28.0 - 60.0);
    let x0 = feet(&app).x;
    app.world.input.set_touch(7, right_pad, TouchPhase::Began);
    tick_n(&mut app, 60);
    app.world.input.set_touch(7, right_pad, TouchPhase::Ended);
    tick_n(&mut app, 2);
    assert!(feet(&app).x > x0 + 3.0, "the right pad runs: {} -> {}", x0, feet(&app).x);
    assert_eq!(mgr(&app).state, LevelState::Playing, "pads do not press UI buttons");
    // кнопка паузы HUD — настоящая кнопка: касание ставит игру на паузу
    tap(&mut app, Vec2::new(1280.0 - 24.0 - 30.0, 18.0 + 26.0));
    assert_eq!(mgr(&app).state, LevelState::Paused);
}
