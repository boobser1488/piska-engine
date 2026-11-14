//! Рывок и прыжок под потолком: игрок не должен вылетать за карту, застревать в тайлах
//! или разгоняться. Пробуем разные порядки нажатий, частоты кадров и низкие места уровней.

use engine::prelude::*;
use lantern::phys::{self, Body, Grid, Shared};
use lantern::{Player, LEVEL_COUNT};

const HALF: Vec2 = Vec2::new(0.32, 0.46);
/// Больше этого игрок не развивает ни при каких обстоятельствах (рывок 19, падение 24).
const SPEED_CAP: f32 = 30.0;

fn game() -> App {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ENGINE_AUDIO", "off"));
    let mut app = lantern::build_with(SaveData::in_memory()).expect("game builds");
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app
}

fn pid(app: &App) -> GameObjectId {
    app.world.scene.find_by_tag("Player").expect("Player")
}

fn feet(app: &App) -> Vec2 {
    app.world.scene.world_position(pid(app)).truncate()
}

fn overlaps_solid(g: &Grid, feet: Vec2) -> bool {
    let c = feet + Vec2::new(0.0, HALF.y);
    let (hx, hy) = (HALF.x - 0.02, HALF.y - 0.02);
    for cy in (c.y - hy).floor() as i32..=(c.y + hy).floor() as i32 {
        for cx in (c.x - hx).floor() as i32..=(c.x + hx).floor() as i32 {
            if cx >= 0 && cy >= 0 && cx < g.width && cy < g.height && g.is_solid(cx, cy) {
                return true;
            }
        }
    }
    false
}

/// Сценарий: на каких кадрах (по 1/60 с) жмём рывок и прыжок, куда держим стик.
#[derive(Clone, Copy, Debug)]
struct Script {
    dash_at: u32,
    jump_at: u32,
    jump_len: u32,
    dir: f32,
}

/// Прогнать сценарий из точки `at`. Возвращает первое нарушение.
fn run(app: &mut App, at: Vec2, s: Script, dt: f32) -> Option<String> {
    let id = pid(app);
    app.world.scene.get_mut(id).unwrap().transform.position = at.extend(0.0);
    {
        let p = app.world.scene.get_component_mut::<Player>(id).unwrap();
        p.vel = Vec2::ZERO;
        p.safe_pos = at;
        p.has_dash = true;
        p.can_dash = true;
        p.dash_time = 0.0;
        p.dash_cooldown = 0.0;
        p.hurt_time = 0.0;
        p.respawn_time = 0.0;
        p.dead = false;
        p.frozen = false;
        p.invuln = 100.0;
        p.grounded = true;
        p.coyote = 0.1;
    }
    let shared = app.world.resources.get::<Shared>().unwrap().clone();
    let (w, h) = (shared.grid.borrow().width as f32, shared.grid.borrow().height as f32);
    let total = 1.2f32;
    let mut t = 0.0f32;
    let mut frame = 0u32;
    let step_s = 1.0 / 60.0;
    let key_d = if s.dir > 0.0 { Key::D } else { Key::A };
    let mut fail = None;
    while t < total {
        let ts = frame as f32 * dt;
        let (da, ja) = (s.dash_at as f32 * step_s, s.jump_at as f32 * step_s);
        let dash_down = ts >= da && ts - dt < da;
        let jump_held = ts >= ja && ts < ja + s.jump_len as f32 * step_s;
        app.world.input.set_key(key_d, true);
        app.world.input.set_key(Key::LShift, dash_down);
        app.world.input.set_key(Key::Space, jump_held);
        app.tick(dt);
        frame += 1;
        t += dt;
        let f = feet(app);
        let p = app.world.scene.get_component::<Player>(id).unwrap();
        let speed = p.vel.length();
        let g = shared.grid.borrow();
        if !(f.x > 0.0 && f.x < w && f.y > -4.0 && f.y + 0.92 < h) {
            fail = Some(format!("left the map: feet {f:?} (map {w}x{h}) at t={t:.3} {s:?} dt={dt}"));
        } else if !speed.is_finite() || speed > SPEED_CAP {
            fail = Some(format!("speed {speed} at t={t:.3} {s:?} dt={dt}"));
        } else if overlaps_solid(&g, f) {
            fail = Some(format!("inside a wall: feet {f:?} at t={t:.3} {s:?} dt={dt}"));
        }
        if fail.is_some() {
            break;
        }
    }
    app.world.input.set_key(key_d, false);
    app.world.input.set_key(Key::LShift, false);
    app.world.input.set_key(Key::Space, false);
    app.tick(1.0 / 60.0);
    fail
}

/// Места, где над стоящим на земле игроком потолок не выше 4 клеток.
fn low_spots(g: &Grid) -> Vec<Vec2> {
    let mut out = Vec::new();
    for y in 1..g.height - 3 {
        for x in 1..g.width - 1 {
            if !g.is_solid(x, y - 1) || g.is_solid(x, y) {
                continue;
            }
            let ceil = (1..=4).find(|k| g.is_solid(x, y + k));
            if ceil.is_some() {
                out.push(Vec2::new(x as f32 + 0.5, y as f32));
            }
        }
    }
    out
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn dash_and_jump_under_a_ceiling_never_leave_the_map() {
    let mut app = game();
    let mut rng = Rng(0x5eed_1234_abcd_9876);
    let mut bad = Vec::new();
    let mut tried = 0;
    let dts = [1.0 / 30.0, 1.0 / 60.0, 1.0 / 144.0, 0.25];
    for n in 0..LEVEL_COUNT {
        lantern::goto_level(&mut app, n);
        app.run_headless(4, 1.0 / 60.0);
        let spots = {
            let shared = app.world.resources.get::<Shared>().unwrap();
            let g = shared.grid.borrow();
            low_spots(&g)
        };
        eprintln!("level {n}: {} low spots", spots.len());
        for _ in 0..40.min(spots.len()) {
            let base = spots[rng.below(spots.len() as u64) as usize];
            for dt in dts {
                for k in 0..8u32 {
                    for order in 0..3 {
                        // 0: рывок, потом прыжок через k кадров; 1: прыжок, потом рывок; 2: вместе
                        let (dash_at, jump_at) = match order {
                            0 => (2, 2 + k),
                            1 => (2 + k, 2),
                            _ => (2, 2),
                        };
                        let s = Script {
                            dash_at,
                            jump_at,
                            jump_len: 1 + rng.below(30) as u32,
                            dir: if rng.below(2) == 0 { 1.0 } else { -1.0 },
                        };
                        let at = base + Vec2::new((rng.below(60) as f32 - 30.0) / 100.0, 0.0);
                        tried += 1;
                        if let Some(msg) = run(&mut app, at, s, dt) {
                            bad.push(format!("level {n} spot {at:?}: {msg}"));
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {tried} runs broke; first: {:#?}", bad.len(), &bad[..bad.len().min(6)]);
}

/// Чистая физика: тело стоит на границе двух тайлов потолка и прыгает вверх.
#[test]
fn jumping_into_a_ceiling_spanning_two_tiles_stops_below_it() {
    let mut g = Grid::empty(10, 8);
    for x in 0..10 {
        g.set_flags(x, 0, phys::SOLID);
        g.set_flags(x, 3, phys::SOLID);
    }
    for dt in [1.0 / 30.0, 1.0 / 60.0, 1.0 / 120.0, 1.0 / 144.0] {
        let mut b = Body { pos: Vec2::new(5.0, 1.0 + HALF.y), half: HALF, vel: Vec2::new(0.0, 15.2) };
        for _ in 0..40 {
            b.vel.y -= 42.0 * dt;
            phys::step(&g, &[], &mut b, dt, false);
            assert!(b.pos.y + HALF.y <= 3.0 + 1e-3, "dt {dt}: body went through the ceiling: y={}", b.pos.y);
        }
    }
}
