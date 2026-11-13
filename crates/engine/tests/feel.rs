//! «Ощущение» игры: плавность на мониторах не 60 Гц, отклик на ввод, поведение у стен.
//! Каждый тест воспроизводит жалобу игрока числом, а не на глаз.

use engine::prelude::*;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

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

fn position(app: &App, id: GameObjectId) -> Vec2 {
    app.world.scene.world_position(id).truncate()
}

/// Монитор 75 Гц, физика 60 Гц: бегущий игрок должен сдвигаться на экране на одну и
/// ту же величину каждый кадр, а не «0, 0.1, 0.1, 0.1, 0, …».
#[test]
fn motion_is_smooth_when_the_display_is_not_60_hz() {
    for hz in [75.0_f32, 144.0, 50.0] {
        let dt = 1.0 / hz;
        let (mut app, player) = level();
        app.run_headless((hz * 1.5) as u32, dt);
        app.world.input.set_key(Key::D, true);
        app.run_headless((hz * 0.4) as u32, dt);

        let expected = 6.0 * dt; // скорость игрока 6 м/с
        let mut previous = position(&app, player).x;
        for frame in 0..(hz as u32) / 2 {
            app.tick(dt);
            let x = position(&app, player).x;
            let step = x - previous;
            assert!(
                (step - expected).abs() < expected * 0.15,
                "{hz} Hz, frame {frame}: moved {step:.4} instead of {expected:.4}"
            );
            previous = x;
        }

        // Камера следит за игроком так же ровно: расстояние между ними не «дышит».
        let camera = app.world.scene.find_by_tag("MainCamera").unwrap();
        let mut gaps = Vec::new();
        for _ in 0..(hz as u32) / 4 {
            app.tick(dt);
            gaps.push(position(&app, player).x - position(&app, camera).x);
        }
        let jumps = gaps.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
        assert!(jumps < 0.02, "{hz} Hz: the camera lags by a changing amount, up to {jumps:.4}");
    }
}

/// Нажатие прыжка должно быть видно в том же кадре, в котором оно пришло.
#[test]
fn a_jump_is_visible_in_the_frame_the_key_arrives() {
    for hz in [60.0_f32, 75.0, 144.0] {
        let dt = 1.0 / hz;
        let (mut app, player) = level();
        app.run_headless((hz * 1.5) as u32, dt);
        let ground = position(&app, player).y;

        app.world.input.set_key(Key::Space, true);
        let mut frames = 0;
        loop {
            app.tick(dt);
            frames += 1;
            if position(&app, player).y > ground + 1e-3 {
                break;
            }
            assert!(frames < 30, "{hz} Hz: the player never left the ground");
        }
        app.world.input.set_key(Key::Space, false);
        // На 144 Гц шаг физики случается не в каждом кадре, поэтому допускаем три.
        let allowed = if hz > 60.0 { (hz / 60.0).ceil() as u32 } else { 1 };
        assert!(frames <= allowed, "{hz} Hz: the jump showed up after {frames} frames");
    }
}

/// Короткое нажатие (одно на кадр) не теряется, даже если в этом кадре не было шага физики.
#[test]
fn a_one_frame_tap_is_never_lost() {
    let dt = 1.0 / 144.0;
    for offset in 0..12 {
        let (mut app, player) = level();
        app.run_headless(220 + offset, dt);
        let ground = position(&app, player).y;
        app.world.input.set_key(Key::Space, true);
        app.tick(dt);
        app.world.input.set_key(Key::Space, false);
        let mut peak = ground;
        for _ in 0..144 {
            app.tick(dt);
            peak = peak.max(position(&app, player).y);
        }
        assert!(peak > ground + 2.0, "tap at frame {offset} was lost: peak {peak:.2}");
    }
}

component! {
    /// Всё время давит вправо, как игрок, зажавший клавишу.
    pub struct Pusher {
        pub speed: f32 = 6.0,
    }
}

impl Component for Pusher {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if let Some(body) = ctx.get_mut::<Rigidbody2D>() {
            body.velocity.x = self.speed;
        }
    }
}

/// Игрок, прижатый к стене в прыжке, должен падать, а не висеть на ней.
#[test]
fn a_body_pushed_into_a_wall_keeps_falling() {
    let mut app = engine::app();
    app.spawn("Wall").at(2.0, 0.0).with(Collider2D::rect(1.0, 40.0));
    let player = app
        .spawn("Player")
        .at(0.9, 10.0)
        .with(Rigidbody2D::dynamic().locked_rotation())
        .with(Collider2D::rect(0.8, 0.9).with_friction(0.0))
        .with(Pusher::default())
        .id();

    app.run_headless(60, 1.0 / 60.0);
    let body = app.world.scene.get_component::<Rigidbody2D>(player).unwrap();
    let y = position(&app, player).y;
    // Секунда свободного падения: v ≈ 9.8 м/с, путь ≈ 4.9 м.
    assert!(body.velocity.y < -8.0, "the wall holds the body: vy = {}", body.velocity.y);
    assert!(y < 6.0, "the body barely moved down: y = {y}");
    let x = position(&app, player).x;
    assert!((x - 1.1).abs() < 0.03, "it rests against the wall face without sinking in: x = {x}");
}

/// Сквозь одностороннюю платформу можно запрыгнуть снизу и встать на неё сверху.
#[test]
fn one_way_platforms_let_the_player_jump_up_through_them() {
    let (mut app, player) = level();
    let dt = 1.0 / 60.0;
    // «Platform A» висит над дорожкой: бежим под неё и прыгаем.
    app.world.input.set_key(Key::D, true);
    for _ in 0..600 {
        app.tick(dt);
        if position(&app, player).x > -1.6 {
            break;
        }
    }
    app.world.input.set_key(Key::D, false);
    app.run_headless(20, dt);
    let platform = app.world.scene.find_by_name("Platform A").unwrap();
    let top = position(&app, platform).y + 0.2;
    assert!((position(&app, player).x - position(&app, platform).x).abs() < 1.0);
    assert!(position(&app, player).y < top, "the player starts below the platform");

    app.world.input.set_key(Key::Space, true);
    app.tick(dt);
    app.world.input.set_key(Key::Space, false);
    app.run_headless(150, dt);

    let rest = position(&app, player).y;
    assert!(
        (rest - (top + 0.45)).abs() < 0.06,
        "the player stands on the platform (y = {rest:.2}, expected {:.2})",
        top + 0.45
    );
}
