//! Контроллер от первого лица: классическая физика движения на фиксированном шаге.
//! Без окна и без звука, детерминированно: `cargo test -p engine --no-default-features
//! --features "physics3d audio audio-wav ui gamepad" --test fps_movement`.
#![cfg(feature = "physics3d")]

use std::cell::RefCell;
use std::rc::Rc;

use engine::fps::{units, vertical_fov_deg, FpsController, FpsRuntime, MoveState, UserCmd, UNIT};
use engine::prelude::*;

/// Классический шаг: 64 раза в секунду.
const TICK: f32 = 1.0 / 64.0;

struct Level {
    app: App,
    player: GameObjectId,
}

/// Уровень из ящиков: (центр, размер, поворот).
fn level(boxes: &[(Vec3, Vec3, Quat)], start: Vec3, setup: impl FnOnce(&mut FpsController)) -> Level {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    app.world.time.fixed_delta = TICK;
    if let Some(physics) = app.world.resources.get_mut::<Physics3D>() {
        physics.gravity = Vec3::new(0.0, -9.81, 0.0); // контроллер берёт гравитацию из своих полей
    }
    for (i, (center, size, rotation)) in boxes.iter().enumerate() {
        app.spawn(format!("Box {i}"))
            .at3(center.x, center.y, center.z)
            .rotation(*rotation)
            .with(Collider::cuboid(size.x, size.y, size.z));
    }
    let mut controller = FpsController::default();
    setup(&mut controller);
    let player = app.spawn("Player").at3(start.x, start.y, start.z).with(controller).id();
    Level { app, player }
}

fn floor() -> (Vec3, Vec3, Quat) {
    (Vec3::new(0.0, -0.5, 0.0), Vec3::new(200.0, 1.0, 200.0), Quat::IDENTITY)
}

impl Level {
    fn drive(&mut self, driver: impl FnMut(u64, &MoveState) -> UserCmd + 'static) {
        self.app.world.resources.get_or_insert_with(FpsRuntime::default).set_driver(self.player, driver);
    }

    fn state(&self) -> MoveState {
        *self.app.world.resources.get::<FpsRuntime>().and_then(|r| r.state(self.player)).expect("the controller has started")
    }

    fn controller(&self) -> FpsController {
        self.app.world.scene.get_component::<FpsController>(self.player).expect("controller").clone()
    }

    /// Прогнать `ticks` шагов (кадр = шаг) и вернуть состояния после каждого.
    fn run(&mut self, ticks: usize) -> Vec<MoveState> {
        let mut out = Vec::with_capacity(ticks);
        for _ in 0..ticks {
            self.app.tick(TICK);
            out.push(self.state());
        }
        out
    }

    /// Дать миру шаг, чтобы коллайдеры попали в запросы, и встать на пол.
    fn settle(&mut self) {
        self.drive(|_, _| UserCmd::default());
        self.run(16);
    }
}

fn speed_ups(state: &MoveState) -> f32 {
    state.units_per_second()
}

fn cmd(forward: f32, side: f32, yaw: f32) -> UserCmd {
    UserCmd { forward, side, yaw_deg: yaw, ..UserCmd::default() }
}

/// Поворот, при котором «вправо» перпендикулярно скорости (идеальный стрейф вправо).
fn perfect_strafe(state: &MoveState) -> UserCmd {
    let v = Vec2::new(state.velocity.x, state.velocity.z);
    let dir = v.normalize_or_zero();
    // right = (cos y, -sin y) в плоскости (x, z) должен быть перпендикулярен v и смотреть
    // «вправо» от неё: right = (-dir.z, dir.x) повёрнут на −90° → (dir.z·…); берём
    // right = (-dir.y, dir.x) в обозначениях (x, z).
    let right = Vec2::new(-dir.y, dir.x);
    let yaw = (-right.y).atan2(right.x).to_degrees();
    UserCmd { side: 1.0, yaw_deg: yaw, ..UserCmd::default() }
}

#[test]
fn ground_speed_converges_to_max_speed() {
    let mut l = level(&[floor()], Vec3::ZERO, |_| {});
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    let states = l.run(64 * 3);
    let last = states.last().unwrap();
    assert!((speed_ups(last) - 250.0).abs() < 0.5, "top speed {:.2} u/s", speed_ups(last));
    assert!(states.iter().all(|s| speed_ups(s) <= 250.01), "never faster than max speed on the ground");
    // Диагональ не быстрее прямой.
    l.drive(|_, _| cmd(1.0, 1.0, 0.0));
    let last = *l.run(64 * 2).last().unwrap();
    assert!((speed_ups(&last) - 250.0).abs() < 0.5, "diagonal speed {:.2}", speed_ups(&last));
    assert!(last.on_ground && last.origin.y.abs() < 0.01);
    // Шаг (Shift) и присед медленнее.
    l.drive(|_, _| UserCmd { walk: true, ..cmd(1.0, 0.0, 0.0) });
    let last = *l.run(64 * 2).last().unwrap();
    assert!((speed_ups(&last) - 130.0).abs() < 1.0, "walk speed {:.1}", speed_ups(&last));
    l.drive(|_, _| UserCmd { duck: true, ..cmd(1.0, 0.0, 0.0) });
    let last = *l.run(64 * 2).last().unwrap();
    assert!(last.ducked && (speed_ups(&last) - 85.0).abs() < 1.0, "duck speed {:.1}", speed_ups(&last));
}

#[test]
fn friction_stops_within_the_classic_distance() {
    let mut l = level(&[floor()], Vec3::ZERO, |_| {});
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    l.run(64 * 3);
    let start = l.state();
    l.drive(|_, _| cmd(0.0, 0.0, 0.0));
    let states = l.run(64);
    let stop = states.iter().position(|s| s.velocity.length() == 0.0).expect("stops within a second");
    let distance = (states[stop].origin - start.origin).length() / UNIT;
    // Та же формула трения в единицах, шаг за шагом.
    let (mut v, mut d) = (speed_ups(&start), 0.0f32);
    let mut ticks = 0;
    while v >= 1.0 {
        let control = v.max(80.0);
        v = (v - control * 5.2 * TICK).max(0.0);
        if v < 1.0 {
            v = 0.0;
        }
        d += v * TICK;
        ticks += 1;
    }
    assert!((distance - d).abs() < 1.0, "stop distance {distance:.2} u, expected {d:.2} u");
    assert!((stop as i32 + 1 - ticks).abs() <= 1, "stopped after {} ticks, expected {ticks}", stop + 1);
    assert!((34.0..48.0).contains(&distance), "about 40 u from full speed: {distance:.1}");
}

#[test]
fn jump_apex_is_the_jump_height() {
    let mut l = level(&[floor()], Vec3::ZERO, |_| {});
    l.settle();
    let start = l.state().origin.y;
    l.drive(|tick, _| UserCmd { jump: tick == 20, ..UserCmd::default() });
    let states = l.run(64 * 2);
    let apex = states.iter().map(|s| s.origin.y).fold(f32::MIN, f32::max) - start;
    assert!((apex / UNIT - 57.0).abs() < 1.0, "apex {:.2} u", apex / UNIT);
    assert!(states.last().unwrap().on_ground, "lands again");
    assert!(states.iter().any(|s| s.jumped) && states.iter().any(|s| s.landed));
}

#[test]
fn air_strafing_gains_speed_and_a_straight_jump_does_not() {
    // Прямой прыжок с удержанием «вперёд»: скорость не растёт.
    let mut l = level(&[floor()], Vec3::ZERO, |_| {});
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    l.run(64 * 2);
    l.drive(|tick, _| UserCmd { jump: tick == 64 * 2 + 16 + 1, ..cmd(1.0, 0.0, 0.0) });
    let states = l.run(30);
    let air: Vec<&MoveState> = states.iter().filter(|s| !s.on_ground).collect();
    assert!(air.len() > 20);
    let max_air = air.iter().map(|s| speed_ups(s)).fold(0.0, f32::max);
    assert!(max_air <= 250.5, "holding forward in the air does not add speed: {max_air:.2}");

    // Идеальный стрейф: желаемое направление перпендикулярно скорости.
    let mut l = level(&[floor()], Vec3::ZERO, |_| {});
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    l.run(64 * 2);
    let jump_at = l.app.world.resources.get::<FpsRuntime>().unwrap().ticks(l.player);
    l.drive(move |tick, s| if tick == jump_at { UserCmd { jump: true, ..cmd(1.0, 0.0, 0.0) } } else { perfect_strafe(s) });
    let states = l.run(40);
    let in_air: Vec<f32> = states.iter().take_while(|s| !s.landed).map(speed_ups).collect();
    let gained = in_air.last().unwrap() - 250.0;
    // Каждый шаг в воздухе добавляет 30 ед/с поперёк: v = √(v0² + n·30²).
    let expected = (250.0f32.powi(2) + (in_air.len() as f32 - 1.0) * 900.0).sqrt() - 250.0;
    assert!(gained > 40.0, "a perfect strafe gains speed: +{gained:.1} u/s over {} ticks", in_air.len());
    assert!((gained - expected).abs() < 6.0, "gain {gained:.1}, theory {expected:.1}");
}

#[test]
fn bunny_hopping_with_strafes_exceeds_ground_speed_and_forward_only_does_not() {
    let hop = |strafe: bool| {
        let mut l = level(&[floor()], Vec3::ZERO, |_| {});
        l.settle();
        l.drive(|_, _| cmd(1.0, 0.0, 0.0));
        l.run(64 * 2);
        l.drive(move |_, s| {
            if s.on_ground {
                UserCmd { jump: true, ..cmd(1.0, 0.0, 0.0) } // прыжок в тот же шаг, что приземлились
            } else if strafe {
                perfect_strafe(s)
            } else {
                cmd(1.0, 0.0, 0.0)
            }
        });
        let states = l.run(64 * 6);
        let hops = states.iter().filter(|s| s.jumped).count();
        (speed_ups(states.last().unwrap()), states.iter().map(speed_ups).fold(0.0, f32::max), hops)
    };
    let (_, fastest_forward, hops_forward) = hop(false);
    assert!(hops_forward >= 5, "the chain keeps hopping: {hops_forward}");
    assert!(fastest_forward <= 250.5, "hopping forward only never beats ground speed: {fastest_forward:.1}");
    let (last, _, hops) = hop(true);
    assert!(hops >= 5, "hops {hops}");
    assert!(last > 400.0, "a strafed bunny-hop chain is much faster than 250: {last:.0} u/s after {hops} hops");
}

#[test]
fn crouch_jump_clears_a_ledge_that_a_standing_jump_does_not() {
    let ledge_height = units(66.0);
    // Уступ от z = −2 вглубь на 20 м; разбег с z = 4, прыжок примерно за 1.7 м до края.
    let ledge = (Vec3::new(0.0, ledge_height * 0.5, -12.0), Vec3::new(6.0, ledge_height, 20.0), Quat::IDENTITY);
    let attempt = |crouch: bool| {
        let mut l = level(&[floor(), ledge], Vec3::new(0.0, 0.0, 4.0), |_| {});
        l.settle();
        l.drive(move |_, s| {
            let jump = s.on_ground && s.origin.z < 0.0 && s.origin.z > -1.0;
            let duck = crouch && !s.on_ground && s.origin.z < 0.2;
            UserCmd { jump, duck, ..cmd(1.0, 0.0, 0.0) }
        });
        let states = l.run(64 * 2);
        assert!(states.iter().filter(|s| s.jumped).count() == 1, "one jump");
        states.last().unwrap().origin
    };
    let standing = attempt(false);
    assert!(standing.y < 0.05 && standing.z > -2.0 + 0.3, "a standing jump stays below the 66 u ledge: {standing}");
    let crouched = attempt(true);
    assert!((crouched.y - ledge_height).abs() < 0.02 && crouched.z < -2.2, "a crouch-jump ends on top: {crouched}");
}

#[test]
fn steps_up_to_the_step_size_and_not_above() {
    let walk_to = |height_units: f32| {
        let h = units(height_units);
        let step = (Vec3::new(0.0, h * 0.5, -7.0), Vec3::new(4.0, h, 10.0), Quat::IDENTITY);
        let mut l = level(&[floor(), step], Vec3::new(0.0, 0.0, 0.0), |_| {});
        l.settle();
        l.drive(|_, _| cmd(1.0, 0.0, 0.0));
        l.run(64);
        l.state().origin
    };
    let at = walk_to(18.0);
    assert!((at.y - units(18.0)).abs() < 0.01 && at.z < -2.5, "climbs an 18 u step: {at}");
    let at = walk_to(16.0);
    assert!((at.y - units(16.0)).abs() < 0.01 && at.z < -2.5, "climbs a 16 u step: {at}");
    let at = walk_to(18.6);
    assert!(at.y < 0.01 && at.z > -2.0, "an 18.6 u step blocks: {at}");
}

/// Склон: плита, повёрнутая вокруг X на `degrees` (поднимается в сторону −Z).
fn slope(degrees: f32, length: f32) -> (Vec3, Vec3, Quat) {
    let a = degrees.to_radians();
    let rot = Quat::from_rotation_x(a);
    // Верхняя грань проходит через начало координат: центр — на полтолщины вглубь.
    let center = rot * Vec3::new(0.0, -0.25, 0.0) + Vec3::new(0.0, length * 0.5 * a.sin(), -length * 0.5 * a.cos());
    (center, Vec3::new(6.0, 0.5, length), rot)
}

#[test]
fn walkable_slopes_hold_still_and_steep_ones_slide() {
    // Пологий (30°): стоим без дрожи и заходим наверх.
    let (center, size, rot) = slope(30.0, 40.0);
    let mut l = level(&[floor(), (center, size, rot)], Vec3::new(0.0, 1.75, -2.6), |_| {});
    l.settle();
    l.run(32);
    let rest = l.state();
    assert!(rest.on_ground && (rest.ground_normal.y - 30f32.to_radians().cos()).abs() < 0.01, "stands on the 30° slope");
    let states = l.run(128);
    let drift = states.iter().map(|s| (s.origin - rest.origin).length()).fold(0.0, f32::max);
    assert!(drift < 1e-4, "no creeping or jitter on a walkable slope: {drift}");
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    let states = l.run(64 * 2);
    assert!(states.iter().all(|s| s.on_ground), "walks up the slope without leaving the ground");
    assert!(l.state().origin.y > 3.0, "climbed: {}", l.state().origin);

    // Крутой (60°): не земля — соскальзывает вниз.
    let (center, size, rot) = slope(60.0, 8.0);
    let mut l = level(&[floor(), (center, size, rot)], Vec3::new(0.0, 4.0, -1.9), |_| {});
    l.drive(|_, _| UserCmd::default());
    let states = l.run(64 * 3);
    let touched = states.iter().position(|s| s.origin.z > -2.3 && s.velocity.z > 0.5);
    assert!(touched.is_some(), "slides down and away from the 60° face");
    assert!(states[..states.len() - 20].iter().any(|s| !s.on_ground && s.origin.y > 1.0), "is not standing on the steep face");
    assert!(l.state().origin.y < 0.01 && l.state().origin.z > -1.0, "ends at the foot: {}", l.state().origin);
}

#[test]
fn surfing_along_a_steep_ramp_keeps_speed() {
    // Длинная крутая (60°) рампа вдоль Z: её грань смотрит на −X и вверх.
    let a = 60f32.to_radians();
    let rot = Quat::from_rotation_z(a);
    let ramp = (Vec3::new(0.0, 3.0, 0.0), Vec3::new(8.0, 0.5, 80.0), rot);
    let normal = rot * Vec3::Y;
    let start = Vec3::new(0.0, 3.0, 30.0) + normal * 1.6 - Vec3::Y * 0.6;
    let mut l = level(&[ramp], start, |c| c.velocity = Vec3::new(0.0, 0.0, -units(800.0)));
    // Смотрим вдоль −Z, давим вбок на рампу (её грань слева: нормаль к −X).
    l.drive(|_, _| cmd(0.0, 1.0, 0.0));
    let states = l.run(64 * 2);
    let along: Vec<f32> = states.iter().map(|s| -s.velocity.z / UNIT).collect();
    let min = along.iter().fold(f32::MAX, |a, b| a.min(*b));
    assert!(min > 790.0, "speed along the ramp never drops: min {min:.0}");
    let touched = states.iter().filter(|s| s.velocity.x > 0.0 || s.velocity.y > -1.0).count();
    assert!(touched > 64, "rides the face, not a free fall: {touched} ticks");
    let last = l.state();
    assert!(!last.on_ground, "surfing is not standing");
    let distance = (start.z - last.origin.z) / UNIT;
    assert!(distance > 1500.0, "travelled {distance:.0} u along the ramp");
    // Всё ещё на рампе (не свалились): точка у грани.
    let off_face = (last.origin - Vec3::new(0.0, 3.0, last.origin.z)).dot(normal);
    assert!(off_face < 1.5, "still on the ramp face: {off_face:.2} m off");
}

#[test]
fn cannot_stand_up_under_a_low_ceiling() {
    let ceiling_bottom = units(60.0);
    let ceiling = (Vec3::new(0.0, ceiling_bottom + 0.25, -4.0), Vec3::new(6.0, 0.5, 4.0), Quat::IDENTITY);
    let mut l = level(&[floor(), ceiling], Vec3::new(0.0, 0.0, 0.0), |_| {});
    l.settle();
    // Пригнувшись, заходим под потолок, отпускаем присед там.
    l.drive(|tick, _| UserCmd { duck: tick < 200, ..cmd(if tick < 200 { 1.0 } else { 0.0 }, 0.0, 0.0) });
    l.run(184);
    assert!(l.state().ducked && l.state().origin.z < -3.0, "crawled under the ceiling: {}", l.state().origin);
    l.run(64);
    assert!(l.state().ducked, "no head room: stays ducked");
    // Без приседа нельзя и пролезть: стоя упираемся.
    l.drive(|_, _| UserCmd { duck: false, ..cmd(-1.0, 0.0, 0.0) });
    l.run(64 * 3);
    let out = l.state();
    assert!(!out.ducked && out.origin.z > -1.5, "stands up once out from under: {}", out.origin);
    let mut l = level(&[floor(), ceiling], Vec3::new(0.0, 0.0, 0.0), |_| {});
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    l.run(64 * 2);
    assert!(l.state().origin.z > -2.0 + 0.3, "standing, the ceiling blocks: {}", l.state().origin);
}

#[test]
fn no_tunnelling_through_a_thin_wall_at_2000_units_per_second() {
    let wall = (Vec3::new(0.0, 1.0, -6.0), Vec3::new(8.0, 4.0, 0.02), Quat::IDENTITY);
    let mut l = level(&[floor(), wall], Vec3::new(0.0, 0.0, 0.0), |_| {});
    l.settle();
    l.app.world.scene.get_component_mut::<FpsController>(l.player).unwrap().velocity = Vec3::new(0.0, 0.0, -units(2000.0));
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    let states = l.run(64);
    assert!(states.iter().all(|s| s.origin.z > -6.0 + 0.3), "never passes the 2 cm wall: {}", l.state().origin);
    assert!(l.state().origin.z < -5.0, "reached the wall");
    // И по диагонали, в воздухе.
    l.app.world.scene.get_component_mut::<FpsController>(l.player).unwrap().velocity = Vec3::new(0.0, units(300.0), units(2000.0));
    l.app.world.scene.get_mut(l.player).unwrap().transform.position = Vec3::new(0.0, 0.0, -10.0);
    l.drive(|_, _| UserCmd::default());
    let states = l.run(64);
    assert!(states.iter().all(|s| s.origin.z < -6.0 - 0.3), "from the other side too: {}", l.state().origin);
}

#[test]
fn sliding_along_a_wall_of_many_boxes_and_across_floor_seams_does_not_snag() {
    let mut boxes = Vec::new();
    for i in 0..30 {
        // Пол из плит 1×1 м и стена из блоков 1 м — швы через каждый метр.
        let z = -(i as f32) - 0.5;
        for x in -2..=2 {
            boxes.push((Vec3::new(x as f32 + 0.5, -0.5, z), Vec3::ONE, Quat::IDENTITY));
        }
        boxes.push((Vec3::new(-2.0, 1.0, z), Vec3::new(1.0, 2.0, 1.0), Quat::IDENTITY));
    }
    let mut l = level(&boxes, Vec3::new(0.0, 0.0, -0.5), |_| {});
    l.settle();
    // Бежим вдоль стены, прижимаясь к ней под углом 15°.
    l.drive(|_, _| cmd(1.0, 0.0, 15.0));
    let states = l.run(64 * 4);
    let touching: Vec<&MoveState> = states.iter().filter(|s| s.origin.x < -1.5 + units(16.0) + 0.01).collect();
    assert!(touching.len() > 100, "pressed against the wall for most of the run");
    let after = &states[64..];
    let min = after.iter().map(speed_ups).fold(f32::MAX, f32::min);
    assert!(min > 230.0, "no snag on wall seams: min speed {min:.1}");
    assert!(after.iter().all(|s| s.on_ground && s.origin.y.abs() < 0.005), "no hops on floor seams");
    assert!(states.last().unwrap().origin.z < -14.0, "ran along: {}", states.last().unwrap().origin);
}

#[test]
fn spamming_jump_against_a_wall_does_not_climb_it() {
    let wall = (Vec3::new(0.0, 2.0, -2.0), Vec3::new(8.0, 4.0, 1.0), Quat::IDENTITY);
    let mut l = level(&[floor(), wall], Vec3::new(0.0, 0.0, 0.0), |_| {});
    l.settle();
    l.drive(|tick, _| UserCmd { jump: tick % 2 == 0, ..cmd(1.0, 0.0, 0.0) });
    let states = l.run(64 * 4);
    let apex = states.iter().map(|s| s.origin.y).fold(f32::MIN, f32::max);
    assert!(apex < units(57.5), "never higher than one jump: {:.1} u", apex / UNIT);
    assert!(states.iter().filter(|s| s.jumped).count() >= 3);
}

#[test]
fn standing_against_a_wall_is_still() {
    let wall = (Vec3::new(0.0, 2.0, -2.0), Vec3::new(8.0, 4.0, 1.0), Quat::IDENTITY);
    let mut l = level(&[floor(), wall], Vec3::new(0.0, 0.0, 0.0), |_| {});
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    l.run(64);
    let pressed = l.state();
    let states = l.run(128);
    let drift = states.iter().map(|s| (s.origin - pressed.origin).length()).fold(0.0, f32::max);
    assert!(drift < 1e-4 && pressed.origin.z > -1.5 - 0.31, "pressed into a wall: no jitter ({drift})");
}

component! {
    /// Подвижная платформа для теста: едет по X, поднимается и поворачивается.
    pub struct TestPlatform {
        pub speed: Vec3 = Vec3::ZERO,
        pub spin_deg: f32 = 0.0,
        pub time: f32 = 0.0,
    }
}
impl Component for TestPlatform {
    fn hooks(&self) -> Hooks {
        Hooks::FIXED_UPDATE
    }
    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.time += dt;
        let t = ctx.transform();
        t.position += self.speed * dt;
        t.rotation = Quat::from_rotation_y((self.spin_deg * dt).to_radians()) * t.rotation;
    }
}

#[test]
fn a_moving_platform_carries_the_player() {
    for (speed, spin) in [(Vec3::new(3.0, 0.0, 0.0), 0.0), (Vec3::new(0.0, 1.5, 0.0), 0.0), (Vec3::ZERO, 45.0), (Vec3::new(0.0, -1.0, 0.0), 0.0)] {
        let mut l = level(&[floor()], Vec3::new(0.5, 3.0, 0.0), |_| {});
        l.app.register::<TestPlatform>();
        let platform = l
            .app
            .spawn("Platform")
            .at3(0.0, 2.5, 0.0)
            .with(Rigidbody::kinematic())
            .with(Collider::cuboid(4.0, 1.0, 4.0))
            .id();
        l.settle();
        assert!(l.state().on_ground && (l.state().origin.y - 3.0).abs() < 0.01, "stands on the platform");
        l.app.world.scene.get_mut(platform).unwrap().add(TestPlatform { speed, spin_deg: spin, time: 0.0 });
        let before = l.state().origin;
        let states = l.run(128);
        let p = l.app.world.scene.world_position(platform);
        let on_top = states.iter().filter(|s| s.on_ground).count();
        assert!(on_top >= 126, "{speed} / {spin}°: stays on the platform ({on_top} of 128 ticks)");
        let after = l.state().origin;
        assert!((after.y - (p.y + 0.5)).abs() < 0.05, "{speed} / {spin}°: on top of it: {after} vs platform {p}");
        if speed.x != 0.0 {
            assert!((after.x - before.x - 2.0 * 3.0).abs() < 0.2, "carried along: {before} → {after}");
        }
        if spin != 0.0 {
            let radius = Vec2::new(after.x - p.x, after.z - p.z).length();
            assert!((radius - 0.5).abs() < 0.05, "rides the turn at the same radius ({radius})");
            assert!(Vec2::new(after.x, after.z).distance(Vec2::new(before.x, before.z)) > 0.3, "and is turned with it");
        }
    }
}

#[test]
fn the_same_commands_give_the_same_path_at_any_frame_rate() {
    let run = |fps: f32| {
        let boxes = [floor(), (Vec3::new(2.0, 0.3, -6.0), Vec3::new(2.0, 0.6, 2.0), Quat::IDENTITY), slope(20.0, 6.0)];
        let mut l = level(&boxes, Vec3::new(0.0, 0.0, 0.0), |_| {});
        let path: Rc<RefCell<Vec<Vec3>>> = Rc::default();
        let record = path.clone();
        l.drive(move |tick, s| {
            record.borrow_mut().push(s.origin);
            let t = tick as f32 * TICK;
            UserCmd {
                forward: 1.0,
                side: (t * 2.0).sin(),
                yaw_deg: 30.0 * (t * 0.7).sin(),
                jump: tick % 50 == 0 && tick > 0,
                duck: (tick / 90) % 3 == 1,
                ..UserCmd::default()
            }
        });
        let frames = (5.0 * fps) as usize;
        for _ in 0..frames {
            l.app.tick(1.0 / fps);
        }
        let out = path.borrow().clone();
        out
    };
    let reference = run(64.0);
    for fps in [30.0, 60.0, 144.0, 300.0] {
        let path = run(fps);
        let n = path.len().min(reference.len());
        assert!(n > 300, "{fps} fps: {} ticks", path.len());
        for i in 0..n {
            assert!(path[i] == reference[i], "{fps} fps: tick {i} differs: {} vs {}", path[i], reference[i]);
        }
    }
}

#[test]
fn stairs_lift_the_body_in_steps_but_the_camera_smoothly() {
    // Лестница из 8 ступеней по 0.25 м вдоль −Z, дальше площадка.
    let mut boxes = vec![floor()];
    for i in 0..8 {
        let top = 0.25 * (i + 1) as f32;
        let z0 = -2.0 - 0.45 * i as f32;
        boxes.push((Vec3::new(0.0, top * 0.5, z0 - 0.225), Vec3::new(3.0, top, 0.45), Quat::IDENTITY));
    }
    boxes.push((Vec3::new(0.0, 1.0, -9.6), Vec3::new(3.0, 2.0, 8.0), Quat::IDENTITY));
    let mut l = level(&boxes, Vec3::new(0.0, 0.0, 0.0), |_| {});
    let eyes = l.app.spawn("Eyes").child_of(l.player).with(Camera::perspective(60.0)).id();
    l.settle();
    l.drive(|_, _| cmd(1.0, 0.0, 0.0));
    let (mut body, mut camera) = (Vec::new(), Vec::new());
    for _ in 0..(144.0 * 2.0) as usize {
        l.app.tick(1.0 / 144.0);
        body.push(l.state().origin.y);
        camera.push(l.app.world.scene.world_position(eyes).y);
    }
    let body_jump = body.windows(2).map(|w| w[1] - w[0]).fold(0.0, f32::max);
    let camera_jump = camera.windows(2).map(|w| w[1] - w[0]).fold(0.0, f32::max);
    assert!((body.last().unwrap() - 2.0).abs() < 0.01, "climbed onto the landing: {}", body.last().unwrap());
    assert!(body_jump > 0.2, "the body steps up a whole riser per tick: {body_jump}");
    assert!(camera_jump < 0.09, "the eyes rise smoothly: largest per-frame rise {camera_jump}");
    assert!((camera.last().unwrap() - (2.0 + units(64.0))).abs() < 0.01, "and settle at eye height");
}

#[test]
fn mouse_look_uses_the_frame_delta_and_needs_a_captured_cursor() {
    let mut l = level(&[floor()], Vec3::ZERO, |c| c.sensitivity = 2.0);
    let camera = l.app.spawn("Eyes").child_of(l.player).with(Camera::perspective(60.0)).id();
    l.app.tick(TICK);
    // Курсор не захвачен — мышь не крутит взгляд.
    l.app.world.input.set_mouse_position(Vec2::new(100.0, 100.0));
    l.app.tick(TICK);
    l.app.world.input.set_mouse_position(Vec2::new(200.0, 100.0));
    l.app.tick(TICK);
    assert_eq!(l.controller().yaw_deg, 0.0);
    // Захват: просьба игры, хозяин подтверждает.
    l.app.world.input.set_cursor_captured(true);
    assert!(!l.app.world.input.is_cursor_captured());
    l.app.world.input.set_cursor_capture_state(true);
    // 100 отсчётов вправо за кадр, 3 кадра подряд — на любой частоте кадров то же.
    for dt in [1.0 / 30.0, 1.0 / 300.0, 1.0 / 144.0] {
        l.app.world.input.add_mouse_delta(Vec2::new(100.0, 0.0));
        l.app.tick(dt);
    }
    let yaw = l.controller().yaw_deg;
    assert!((yaw + 3.0 * 100.0 * 2.0 * 0.022).abs() < 1e-3, "yaw {yaw}");
    // Сдвиг курсора при захвате не считается (смещение — только сырое).
    l.app.world.input.set_mouse_position(Vec2::new(900.0, 100.0));
    l.app.tick(TICK);
    assert!((l.controller().yaw_deg - yaw).abs() < 1e-5);
    // Наклон ограничен ±89°.
    l.app.world.input.add_mouse_delta(Vec2::new(0.0, -100_000.0));
    l.app.tick(TICK);
    assert_eq!(l.controller().pitch_deg, 89.0);
    l.app.world.input.add_mouse_delta(Vec2::new(0.0, 100_000.0));
    l.app.tick(TICK);
    assert_eq!(l.controller().pitch_deg, -89.0);
    // Камера: высота глаз, наклон, угол обзора 90° по горизонтали на 4:3.
    let cam = l.app.world.scene.get(camera).unwrap();
    assert!((cam.transform.position.y - units(64.0)).abs() < 1e-4);
    assert!((cam.get::<Camera>().unwrap().fov_deg - 73.739_8).abs() < 0.01);
    assert!((vertical_fov_deg(90.0) - 73.739_8).abs() < 0.01);
    assert!((engine::fps::horizontal_fov_deg(90.0, 16.0 / 9.0) - 106.26).abs() < 0.01);
    // Потеря фокуса (release_all) снимает просьбу.
    l.app.world.input.release_all();
    assert!(!l.app.world.input.cursor_capture_requested());
}


/// Камера идёт ровно при любой частоте кадров: за каждый кадр она проходит путь,
/// пропорциональный длительности кадра. Если поза между шагами (64 в секунду) не
/// сглаживается или сглаживается неверно, камера при 60 / 75 / 144 Гц то стоит, то
/// прыгает на целый шаг — при нормальном FPS картинка идёт рывками.
#[test]
fn the_camera_moves_evenly_between_fixed_steps_at_any_refresh_rate() {
    let mut report = Vec::new();
    let mut worst = 0.0f32;
    for (name, pattern) in [
        ("60 Hz", vec![1.0 / 60.0]),
        ("59.94 Hz", vec![1.0 / 59.94]),
        ("75 Hz", vec![1.0 / 75.0]),
        ("144 Hz", vec![1.0 / 144.0]),
        ("165 Hz", vec![1.0 / 165.0]),
        // Настоящие кадры не идеально ровные.
        ("60 Hz with jitter", vec![0.0150, 0.0183, 0.0162, 0.0171, 0.0158, 0.0176]),
    ] {
        let mut l = level(&[floor()], Vec3::ZERO, |_| {});
        let eyes = l.app.spawn("Eyes").child_of(l.player).with(Camera::perspective(60.0)).id();
        l.settle();
        l.drive(|_, _| cmd(1.0, 0.0, 0.0));
        let mut frame = 0usize;
        let mut dt = || {
            frame += 1;
            pattern[frame % pattern.len()]
        };
        // Разгон до постоянной скорости.
        let mut t = 0.0;
        while t < 2.0 {
            let d = dt();
            l.app.tick(d);
            t += d;
        }
        let speed = l.state().velocity.length();
        let mut previous = l.app.world.scene.world_position(eyes);
        let (mut lo, mut hi) = (f32::MAX, 0.0f32);
        let mut t = 0.0;
        while t < 3.0 {
            let d = dt();
            l.app.tick(d);
            t += d;
            let now = l.app.world.scene.world_position(eyes);
            let apparent = (now - previous).length() / d;
            previous = now;
            lo = lo.min(apparent / speed);
            hi = hi.max(apparent / speed);
        }
        report.push(format!("{name}: per-frame speed {:.0}%..{:.0}% of the real one", lo * 100.0, hi * 100.0));
        worst = worst.max((1.0 - lo).abs()).max((hi - 1.0).abs());
    }
    assert!(worst < 0.03, "the camera does not move evenly:\n{}", report.join("\n"));
}
