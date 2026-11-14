//! Слой отображения: превращает состояние [`Sim`] в спрайты, частицы, числа урона,
//! тряску камеры и звук.
//!
//! У каждого слота симуляции (враг, пуля, обломок) есть «парный» объект сцены: объекты
//! **создаются из префабов** (`assets/prefabs/*`) и переиспользуются (включаются /
//! выключаются), а не создаются и не удаляются на каждый выстрел. Дрон и реактор — объекты
//! сцены `arena.scene`. Вид каждого врага берётся из его префаба (`SpriteRenderer` +
//! `EnemyKind`), все спрайты — из одного атласа, поэтому рендер склеивает их в немногие
//! вызовы отрисовки.

use engine::prelude::*;

use crate::atlas::{self, Rect};
use crate::data::*;
use crate::defs::defs;
use crate::profile::Options;
use crate::rng::Rng;
use crate::scripts::bosses::{Boss, TELE_BLINK, TELE_LINE};
use crate::scripts::drone::Drone;
use crate::scripts::enemy_kinds::EnemyKind;
use crate::scripts::reactor::ReactorCore;
use crate::scripts::weapons::Weapons;
use crate::sim::{Event, PickupKind, Sim};
use crate::sound;

// Префабы, из которых строятся пулы.
const P_PBULLET: &str = "prefabs/projectiles/player_bullet.prefab";
const P_MISSILE: &str = "prefabs/projectiles/missile.prefab";
const P_EBULLET: &str = "prefabs/projectiles/enemy_bullet.prefab";
const P_SCRAP: &str = "prefabs/pickups/scrap.prefab";
const P_REPAIR: &str = "prefabs/pickups/repair.prefab";
const P_SHIELD: &str = "prefabs/fx/shield.prefab";
const P_MARKER: &str = "prefabs/fx/marker.prefab";
const P_RING: &str = "prefabs/fx/ring.prefab";
const P_FLASH: &str = "prefabs/fx/flash.prefab";
const P_DIGIT: &str = "prefabs/fx/digit.prefab";
const P_BOSS_RING: &str = "prefabs/fx/boss_ring.prefab";
const P_BOSS_LINE: &str = "prefabs/fx/boss_line.prefab";
const P_BOSS_MARK: &str = "prefabs/fx/boss_mark.prefab";
const P_SPARK: &str = "prefabs/fx/spark.prefab";
const P_BOOM: &str = "prefabs/fx/boom.prefab";
const P_SCRAP_FX: &str = "prefabs/fx/scrap_sparkle.prefab";

const FX_RING_POOL: usize = 48;
const FX_FLASH_POOL: usize = 24;
const DIGIT_POOL: usize = 160;
const MARK_POOL: usize = 128;
const EMITTERS: usize = 14;

#[derive(Clone, Copy)]
struct Slot {
    id: GameObjectId,
    active: bool,
    kind: u8,
    serial: u32,
}

struct Fx {
    id: GameObjectId,
    active: bool,
    age: f32,
    life: f32,
    s0: f32,
    s1: f32,
    color: Color,
    spin: f32,
}

struct Digit {
    id: GameObjectId,
    active: bool,
    age: f32,
    life: f32,
    pos: Vec2,
    vel: Vec2,
    color: Color,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Emit {
    Spark = 0,
    Boom = 1,
    Scrap = 2,
}

/// Вид врага или босса, прочитанный из его префаба.
#[derive(Clone)]
struct KindVisual {
    sprite: SpriteRenderer,
    wobble: f32,
    spawn_pop: f32,
    intro: String,
    tele_color: Color,
}

pub struct View {
    pub cam: GameObjectId,
    player: GameObjectId,
    core: GameObjectId,
    core_ring: GameObjectId,
    field: GameObjectId,
    muzzle: GameObjectId,
    boss_ring: GameObjectId,
    boss_line: GameObjectId,
    boss_mark: GameObjectId,
    enemies: Vec<Slot>,
    pbul: Vec<Slot>,
    ebul: Vec<Slot>,
    picks: Vec<Slot>,
    marks: Vec<Slot>,
    shields: Vec<Slot>,
    fx: Vec<Fx>,
    fx_next_ring: usize,
    fx_next_flash: usize,
    digits: Vec<Digit>,
    digit_next: usize,
    emit: [Vec<GameObjectId>; 3],
    emit_next: [usize; 3],
    visuals: Vec<KindVisual>,
    missile_sprite: SpriteRenderer,
    bullet_sprite: SpriteRenderer,
    scrap_sprite: SpriteRenderer,
    repair_sprite: SpriteRenderer,
    ebullet_size: f32,
    drone: Drone,
    weapons: Weapons,
    core_cfg: ReactorCore,
    rng: Rng,
    /// Счётчики звуков за кадр (чтобы сотня попаданий не превращалась в кашу).
    snd: [u8; 6],
    numbers_this_frame: u8,
    core_sound_cd: f32,
    pick_streak: f32,
    pick_streak_t: f32,
    last_boss_tele: f32,
    player_pos: Vec2,
}

// ---------------------------------------------------------------------------
// Мелкие помощники
// ---------------------------------------------------------------------------

fn uv_of(r: Rect) -> (Vec2, Vec2) {
    // Полтекселя внутрь: линейная фильтрация не цепляет соседние спрайты атласа.
    let inset = 0.5;
    (
        Vec2::new((r.x + inset) / atlas::ATLAS_W, (r.y + inset) / atlas::ATLAS_H),
        Vec2::new((r.w - 2.0 * inset) / atlas::ATLAS_W, (r.h - 2.0 * inset) / atlas::ATLAS_H),
    )
}

/// Назначить спрайту прямоугольник атласа; `size` — мировой размер большей стороны.
fn set_rect(sr: &mut SpriteRenderer, r: Rect, size: f32) {
    let (off, sz) = uv_of(r);
    sr.uv_offset = off;
    sr.uv_size = sz;
    let k = size / r.w.max(r.h);
    sr.size = Vec2::new(r.w * k, r.h * k);
}

/// Перенять у спрайта-образца картинку, размер и слой (цвет остаётся прежним).
fn apply_sprite(dst: &mut SpriteRenderer, src: &SpriteRenderer) {
    dst.uv_offset = src.uv_offset;
    dst.uv_size = src.uv_size;
    dst.size = src.size;
    dst.sorting_layer = src.sorting_layer;
    dst.order = src.order;
    dst.pivot = src.pivot;
    dst.flip_x = src.flip_x;
    dst.flip_y = src.flip_y;
}

/// Создать объект из префаба и сразу выключить (пул).
fn spawn_pooled(world: &mut World, path: &str) -> GameObjectId {
    let id = match world.instantiate(path) {
        Ok(id) => id,
        Err(e) => {
            log::error!("cannot instantiate `{path}`: {e}. Re-bake assets: cargo run -p swarm --example bake_assets");
            world.scene.spawn("Missing prefab")
        }
    };
    world.scene.set_active(id, false);
    id
}

/// Создать объект из префаба, снять с него нужное и убрать.
fn read_prefab<T>(world: &mut World, path: &str, read: impl FnOnce(&Scene, GameObjectId) -> T) -> T {
    let id = spawn_pooled(world, path);
    let out = read(&world.scene, id);
    world.scene.destroy(id);
    out
}

fn set_xform(scene: &mut Scene, id: GameObjectId, pos: Vec2, angle: f32, scale: Vec2) {
    if let Some(o) = scene.get_mut(id) {
        o.transform.position = Vec3::new(pos.x, pos.y, 0.0);
        o.transform.rotation = Quat::from_rotation_z(angle);
        o.transform.scale = Vec3::new(scale.x, scale.y, 1.0);
    }
}

fn set_color(scene: &mut Scene, id: GameObjectId, c: Color) {
    if let Some(sr) = scene.get_component_mut::<SpriteRenderer>(id) {
        sr.color = c;
    }
}

fn set_on(scene: &mut Scene, slot: &mut Slot, on: bool) {
    if slot.active != on {
        slot.active = on;
        scene.set_active(slot.id, on);
    }
}

fn kind_color(k: Kind, boost: f32) -> Color {
    let c = k.info().rgb();
    Color::rgb(c[0] * boost, c[1] * boost, c[2] * boost)
}

fn named(scene: &Scene, name: &str) -> GameObjectId {
    scene.find_by_name(name).unwrap_or_else(|| {
        log::error!("object `{name}` is missing from the scene: re-bake assets (cargo run -p swarm --example bake_assets)");
        GameObjectId::from_bits(0)
    })
}

impl View {
    /// Объекты, которые вид взял из полей-ссылок префабов: кольцо реактора, вспышка у
    /// ствола, поле замедления (для тестов).
    pub fn linked_parts(&self) -> [GameObjectId; 3] {
        [self.core_ring, self.muzzle, self.field]
    }

    pub fn new(world: &mut World, seed: u64) -> View {
        let cam = world.scene.find_by_tag("MainCamera").unwrap_or_else(|| world.scene.spawn("Main Camera"));
        let player = named(&world.scene, "Drone");
        let core = named(&world.scene, "ReactorCore");
        let drone = world.scene.get_component::<Drone>(player).cloned().unwrap_or_default();
        let weapons = world.scene.get_component::<Weapons>(player).cloned().unwrap_or_default();
        let core_cfg = world.scene.get_component::<ReactorCore>(core).cloned().unwrap_or_default();
        // Дети префабов — по полям-ссылкам (их можно переименовать в редакторе).
        let core_ring = world.scene.resolve(core_cfg.ring).unwrap_or(core);
        let muzzle = world.scene.resolve(drone.muzzle).unwrap_or(player);
        let field = world.scene.resolve(drone.slow_field).unwrap_or(player);
        for id in [muzzle, field] {
            world.scene.set_active(id, false);
        }

        // Вид каждого вида врагов — из его префаба.
        let visuals: Vec<KindVisual> = Kind::ALL
            .iter()
            .map(|k| {
                let path = k.info().prefab.clone();
                read_prefab(world, &path, |scene, id| {
                    let kind = scene.get_component::<EnemyKind>(id).cloned().unwrap_or_default();
                    let boss = scene.get_component::<Boss>(id).cloned().unwrap_or_default();
                    KindVisual {
                        sprite: scene.get_component::<SpriteRenderer>(id).cloned().unwrap_or_default(),
                        wobble: kind.wobble,
                        spawn_pop: kind.spawn_pop.max(0.01),
                        intro: boss.intro,
                        tele_color: boss.telegraph_color,
                    }
                })
            })
            .collect();
        let sprite_of = |scene: &Scene, id: GameObjectId| scene.get_component::<SpriteRenderer>(id).cloned().unwrap_or_default();
        let missile_sprite = read_prefab(world, P_MISSILE, sprite_of);
        let bullet_sprite = read_prefab(world, P_PBULLET, sprite_of);
        let scrap_sprite = read_prefab(world, P_SCRAP, sprite_of);
        let repair_sprite = read_prefab(world, P_REPAIR, sprite_of);
        let ebullet_size = read_prefab(world, P_EBULLET, sprite_of).size.max_element().max(0.01);

        let boss_ring = spawn_pooled(world, P_BOSS_RING);
        let boss_line = spawn_pooled(world, P_BOSS_LINE);
        let boss_mark = spawn_pooled(world, P_BOSS_MARK);
        let mut v = View {
            cam,
            player,
            core,
            core_ring,
            field,
            muzzle,
            boss_ring,
            boss_line,
            boss_mark,
            enemies: Vec::new(),
            pbul: Vec::new(),
            ebul: Vec::new(),
            picks: Vec::new(),
            marks: Vec::new(),
            shields: Vec::new(),
            fx: Vec::new(),
            fx_next_ring: 0,
            fx_next_flash: 0,
            digits: Vec::new(),
            digit_next: 0,
            emit: [Vec::new(), Vec::new(), Vec::new()],
            emit_next: [0; 3],
            visuals,
            missile_sprite,
            bullet_sprite,
            scrap_sprite,
            repair_sprite,
            ebullet_size,
            drone,
            weapons,
            core_cfg,
            rng: Rng::new(seed ^ 0x51CE),
            snd: [0; 6],
            numbers_this_frame: 0,
            core_sound_cd: 0.0,
            pick_streak: 0.0,
            pick_streak_t: 0.0,
            last_boss_tele: 0.0,
            player_pos: Vec2::ZERO,
        };
        for i in 0..(FX_RING_POOL + FX_FLASH_POOL) {
            let id = spawn_pooled(world, if i < FX_RING_POOL { P_RING } else { P_FLASH });
            v.fx.push(Fx { id, active: false, age: 0.0, life: 1.0, s0: 1.0, s1: 1.0, color: Color::WHITE, spin: 0.0 });
        }
        for _ in 0..DIGIT_POOL {
            let id = spawn_pooled(world, P_DIGIT);
            v.digits.push(Digit { id, active: false, age: 0.0, life: 1.0, pos: Vec2::ZERO, vel: Vec2::ZERO, color: Color::WHITE });
        }
        for i in 0..EMITTERS {
            let (kind, path) = match i % 7 {
                0..=3 => (Emit::Spark, P_SPARK),
                4 | 5 => (Emit::Boom, P_BOOM),
                _ => (Emit::Scrap, P_SCRAP_FX),
            };
            let id = world.instantiate(path).unwrap_or_else(|e| {
                log::error!("cannot instantiate `{path}`: {e}");
                world.scene.spawn("Missing prefab")
            });
            v.emit[kind as usize].push(id);
        }
        v.grow_enemies(world, 96);
        v
    }

    fn make_slot(world: &mut World, path: &str) -> Slot {
        Slot { id: spawn_pooled(world, path), active: false, kind: 255, serial: 0 }
    }

    fn grow_enemies(&mut self, world: &mut World, n: usize) {
        // Слот врага — любой префаб врага: его вид подменяется по виду из симуляции.
        let path = Kind::Chaser.info().prefab.clone();
        while self.enemies.len() < n {
            self.enemies.push(Self::make_slot(world, &path));
        }
    }

    fn grow_pbul(&mut self, world: &mut World, n: usize) {
        while self.pbul.len() < n {
            self.pbul.push(Self::make_slot(world, P_PBULLET));
        }
    }

    fn grow_simple(&mut self, world: &mut World, which: u8, n: usize) {
        match which {
            0 => {
                while self.ebul.len() < n {
                    self.ebul.push(Self::make_slot(world, P_EBULLET));
                }
            }
            1 => {
                while self.picks.len() < n {
                    self.picks.push(Self::make_slot(world, P_SCRAP));
                }
            }
            2 => {
                while self.marks.len() < n {
                    self.marks.push(Self::make_slot(world, P_MARKER));
                }
            }
            _ => {
                while self.shields.len() < n {
                    self.shields.push(Self::make_slot(world, P_SHIELD));
                }
            }
        }
    }

    /// Выключить все объекты пулов (новый забег / выход в меню).
    pub fn reset(&mut self, scene: &mut Scene) {
        for pool in [&mut self.enemies, &mut self.pbul, &mut self.ebul, &mut self.picks, &mut self.marks, &mut self.shields] {
            for s in pool.iter_mut() {
                set_on(scene, s, false);
                s.serial = 0;
            }
        }
        for f in &mut self.fx {
            if f.active {
                f.active = false;
                scene.set_active(f.id, false);
            }
        }
        for d in &mut self.digits {
            if d.active {
                d.active = false;
                scene.set_active(d.id, false);
            }
        }
        for id in [self.field, self.muzzle, self.boss_ring, self.boss_line, self.boss_mark] {
            scene.set_active(id, false);
        }
        for list in &self.emit {
            for id in list {
                if let Some(ps) = scene.get_component_mut::<ParticleSystem>(*id) {
                    ps.clear();
                }
            }
        }
    }

    pub fn show_actors(&mut self, scene: &mut Scene, on: bool) {
        for id in [self.player, self.core, self.core_ring] {
            scene.set_active(id, on);
        }
    }

    /// Строка вступления босса (поле `Boss.intro` префаба).
    pub fn boss_intro(&self, kind: Kind) -> String {
        self.visuals[Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0)].intro.clone()
    }

    fn visual(&self, kind: Kind) -> &KindVisual {
        &self.visuals[Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0)]
    }

    // -----------------------------------------------------------------------
    // Эффекты
    // -----------------------------------------------------------------------

    fn spawn_fx(&mut self, scene: &mut Scene, pos: Vec2, ring: bool, s0: f32, s1: f32, life: f32, color: Color) {
        let i = if ring {
            let i = self.fx_next_ring;
            self.fx_next_ring = (i + 1) % FX_RING_POOL;
            i
        } else {
            let i = self.fx_next_flash;
            self.fx_next_flash = (i + 1) % FX_FLASH_POOL;
            FX_RING_POOL + i
        };
        let f = &mut self.fx[i];
        f.active = true;
        f.age = 0.0;
        f.life = life.max(0.01);
        f.s0 = s0;
        f.s1 = s1;
        f.color = color;
        f.spin = 0.0;
        let id = f.id;
        scene.set_active(id, true);
        if let Some(o) = scene.get_mut(id) {
            o.transform.position = Vec3::new(pos.x, pos.y, 0.0);
            o.transform.rotation = Quat::from_rotation_z(0.0);
            if let Some(sr) = o.get_mut::<SpriteRenderer>() {
                sr.color = color;
            }
        }
    }

    fn ring(&mut self, scene: &mut Scene, pos: Vec2, from: f32, to: f32, life: f32, color: Color) {
        self.spawn_fx(scene, pos, true, from, to, life, color);
    }

    fn flash(&mut self, scene: &mut Scene, pos: Vec2, size: f32, life: f32, color: Color) {
        self.spawn_fx(scene, pos, false, size * 0.5, size, life, color);
    }

    fn burst(&mut self, scene: &mut Scene, kind: Emit, pos: Vec2, n: u32, color: Color) {
        let list = &self.emit[kind as usize];
        if list.is_empty() {
            return;
        }
        let i = self.emit_next[kind as usize] % list.len();
        self.emit_next[kind as usize] += 1;
        let id = list[i];
        if let Some(o) = scene.get_mut(id) {
            o.transform.position = Vec3::new(pos.x, pos.y, 0.0);
            if let Some(ps) = o.get_mut::<ParticleSystem>() {
                ps.start_color = color;
                ps.start_color_max = Color::rgb(color.r * 0.55, color.g * 0.55, color.b * 0.55);
                ps.burst(n);
            }
        }
    }

    fn number(&mut self, scene: &mut Scene, pos: Vec2, value: f32, crit: bool, big: bool) {
        if self.numbers_this_frame >= 7 {
            return;
        }
        self.numbers_this_frame += 1;
        let v = value.round().max(1.0) as u32;
        let text = v.to_string();
        let n = text.len().min(5);
        let (size, color) = if crit {
            (0.78, Color::rgb(3.0, 2.2, 0.35))
        } else if big {
            (0.62, Color::rgb(2.2, 2.2, 2.4))
        } else {
            (0.46, Color::rgb(1.6, 1.8, 2.0))
        };
        let width = size * 32.0 / 48.0 * 0.82;
        let vel = Vec2::new(self.rng.range(-0.5, 0.5), self.rng.range(1.5, 2.3));
        let base = pos + Vec2::new(self.rng.range(-0.25, 0.25), 0.5);
        for (k, ch) in text.chars().take(n).enumerate() {
            let d = ch.to_digit(10).unwrap_or(0) as usize;
            let i = self.digit_next;
            self.digit_next = (self.digit_next + 1) % self.digits.len();
            let dg = &mut self.digits[i];
            dg.active = true;
            dg.age = 0.0;
            dg.life = if crit { 0.85 } else { 0.6 };
            dg.pos = base + Vec2::new((k as f32 - (n as f32 - 1.0) * 0.5) * width, 0.0);
            dg.vel = vel;
            dg.color = color;
            let id = dg.id;
            scene.set_active(id, true);
            if let Some(sr) = scene.get_component_mut::<SpriteRenderer>(id) {
                set_rect(sr, atlas::DIGITS[d], size);
                sr.color = color;
            }
        }
    }

    pub fn shake(&self, scene: &mut Scene, amount: f32, opts: &Options) {
        if !opts.shake {
            return;
        }
        if let Some(c) = scene.get_component_mut::<CameraController2D>(self.cam) {
            c.add_shake(amount);
        }
    }

    // -----------------------------------------------------------------------
    // События: частицы, числа, звук, тряска
    // -----------------------------------------------------------------------

    pub fn handle_events(&mut self, scene: &mut Scene, mut audio: Option<&mut Audio>, events: &[Event], opts: &Options, dt: f32) {
        self.snd = [0; 6];
        self.numbers_this_frame = 0;
        self.core_sound_cd = (self.core_sound_cd - dt).max(0.0);
        self.pick_streak_t -= dt;
        if self.pick_streak_t <= 0.0 {
            self.pick_streak = 0.0;
        }
        // Громкий вызов звука с ограничением числа одинаковых за кадр. Лимиты малы
        // нарочно: десятки одинаковых звуков за секунду сливаются в шум и перегружают
        // выход (замер: бой с прежними лимитами давал −5 LUFS и клиппирование).
        let play = |audio: &mut Option<&mut Audio>, snd: &mut [u8; 6], slot: usize, limit: u8, clip: &str, vol: f32, pitch: f32| {
            if snd[slot] >= limit {
                return;
            }
            snd[slot] += 1;
            if let Some(a) = audio.as_deref_mut() {
                sound::play(a, clip, vol, pitch);
            }
        };
        for ev in events {
            match ev {
                Event::Shot { pos, dir } => {
                    let p = self.rng.range(0.92, 1.1);
                    play(&mut audio, &mut self.snd, 0, 1, "shoot", 0.32, p);
                    self.flash(scene, *pos, 0.9, 0.06, Color::rgb(0.5, 2.0, 3.0));
                    let _ = dir;
                }
                Event::EnemyShot { pos } => {
                    let p = self.rng.range(0.9, 1.1);
                    play(&mut audio, &mut self.snd, 1, 1, "enemy_shot", 0.3, p);
                    self.flash(scene, *pos, 0.9, 0.1, Color::rgb(3.0, 0.5, 1.0));
                }
                Event::EnemyHit { pos, dmg, crit, kind } => {
                    if opts.numbers {
                        self.number(scene, *pos, *dmg, *crit, kind.is_boss());
                    }
                    self.burst(scene, Emit::Spark, *pos, if *crit { 9 } else { 4 }, kind_color(*kind, 1.4));
                    let p = self.rng.range(0.9, 1.25);
                    if *crit {
                        play(&mut audio, &mut self.snd, 2, 1, "crit", 0.4, p);
                    } else {
                        play(&mut audio, &mut self.snd, 2, 1, "hit", 0.32, p);
                    }
                }
                Event::EnemyDied { pos, kind } => {
                    let info = kind.info();
                    let boss = kind.is_boss();
                    self.burst(scene, Emit::Boom, *pos, if boss { 70 } else { (10.0 + info.radius * 30.0) as u32 }, kind_color(*kind, 1.2));
                    self.burst(scene, Emit::Spark, *pos, if boss { 50 } else { 12 }, kind_color(*kind, 1.6));
                    self.ring(scene, *pos, info.radius, info.radius * 4.0, 0.3, kind_color(*kind, 1.0).with_alpha(0.8));
                    if !boss {
                        let p = self.rng.range(0.85, 1.2) / (0.6 + info.radius);
                        play(&mut audio, &mut self.snd, 3, 2, "die", 0.5, p.clamp(0.5, 1.4));
                        if *kind == Kind::Tank {
                            self.shake(scene, 0.25, opts);
                        }
                    }
                }
                Event::Explosion { pos, radius, hostile } => {
                    let c = if *hostile { Color::rgb(3.0, 0.7, 0.5) } else { Color::rgb(2.6, 1.8, 0.7) };
                    self.ring(scene, *pos, *radius * 0.4, *radius * 2.2, 0.35, c.with_alpha(0.9));
                    self.flash(scene, *pos, *radius * 2.4, 0.2, c);
                    self.burst(scene, Emit::Boom, *pos, (12.0 + radius * 12.0) as u32, c);
                    let p = self.rng.range(0.9, 1.1);
                    if *radius > 1.0 {
                        play(&mut audio, &mut self.snd, 4, 1, "boom", 0.6, p);
                    }
                    self.shake(scene, (radius * 0.12).min(0.45), opts);
                }
                Event::PlayerHurt { dmg } => {
                    self.shake(scene, 0.3 + dmg / 60.0, opts);
                    play(&mut audio, &mut self.snd, 5, 1, "player_hurt", 0.7, 1.0);
                }
                Event::CoreHurt { dmg } => {
                    self.shake(scene, 0.25 + dmg / 80.0, opts);
                    if self.core_sound_cd <= 0.0 {
                        self.core_sound_cd = 0.25;
                        if let Some(a) = audio.as_deref_mut() {
                            sound::play(a, "core_hurt", 0.7, 1.0);
                        }
                    }
                    let at = self.rng.unit() * 1.1;
                    self.burst(scene, Emit::Spark, at, 8, Color::rgb(3.0, 1.2, 0.4));
                }
                Event::PlayerDied => {
                    self.shake(scene, 0.7, opts);
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "player_die", 0.8, 1.0);
                    }
                }
                Event::PlayerRespawn => {
                    self.ring(scene, Vec2::new(0.0, -2.8), 0.5, 5.0, 0.5, Color::rgb(0.5, 2.0, 3.0));
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "respawn", 0.6, 1.0);
                    }
                }
                Event::ScrapPicked { .. } => {
                    self.pick_streak = (self.pick_streak + 1.0).min(16.0);
                    self.pick_streak_t = 0.35;
                    let p = 0.85 + self.pick_streak * 0.04;
                    if self.snd[3] < 3 {
                        let at = self.player_pos;
                        self.burst(scene, Emit::Scrap, at, 5, Color::rgb(0.6, 2.6, 1.6));
                    }
                    play(&mut audio, &mut self.snd, 3, 3, "pickup", 0.3, p);
                }
                Event::RepairPicked => {
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "repair", 0.6, 1.0);
                    }
                }
                Event::LevelUp => {
                    self.ring(scene, Vec2::ZERO, 1.0, 14.0, 0.6, Color::rgb(0.6, 2.4, 2.6));
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "levelup", 0.7, 1.0);
                    }
                }
                Event::UpgradeChosen(_) => {
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "ui_confirm", 0.8, 1.0);
                    }
                }
                Event::WaveStart { boss, .. } => {
                    if boss.is_none() {
                        if let Some(a) = audio.as_deref_mut() {
                            sound::play(a, "wave_start", 0.6, 1.0);
                        }
                    }
                }
                Event::WaveCleared { .. } => {
                    self.ring(scene, Vec2::ZERO, 1.0, 18.0, 0.8, Color::rgb(1.0, 2.6, 1.4));
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "wave_clear", 0.7, 1.0);
                    }
                }
                Event::BossAppear { .. } => {
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "boss_warn", 0.8, 1.0);
                    }
                }
                Event::BossTelegraph { pos, kind } => {
                    let c = if *kind == TELE_LINE { Color::rgb(3.0, 0.6, 0.4) } else { Color::rgb(2.4, 1.0, 2.4) };
                    self.ring(scene, *pos, 7.0, 1.0, 0.7, c.with_alpha(0.85));
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "enemy_shot", 0.4, 0.6);
                    }
                }
                Event::BossDied { pos, .. } => {
                    for k in 0..4 {
                        let off = Vec2::new((k as f32 - 1.5) * 1.4, (k % 2) as f32 * 1.2 - 0.6);
                        self.burst(scene, Emit::Boom, *pos + off, 60, Color::rgb(3.0, 1.6, 0.8));
                    }
                    self.ring(scene, *pos, 1.0, 30.0, 1.1, Color::rgb(3.0, 2.2, 1.2));
                    self.flash(scene, *pos, 14.0, 0.5, Color::rgb(3.0, 2.6, 2.0));
                    self.shake(scene, 1.0, opts);
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "boss_die", 0.9, 1.0);
                    }
                }
                Event::BossImpact { pos } => {
                    self.ring(scene, *pos, 1.0, 12.0, 0.45, Color::rgb(3.0, 1.8, 0.8));
                    self.burst(scene, Emit::Spark, *pos, 40, Color::rgb(3.0, 1.8, 0.8));
                    self.shake(scene, 0.75, opts);
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "big_boom", 0.7, 1.1);
                    }
                }
                Event::Dash { pos, .. } => {
                    self.ring(scene, *pos, 0.6, 2.4, 0.25, Color::rgb(0.5, 2.0, 3.0).with_alpha(0.8));
                    self.burst(scene, Emit::Spark, *pos, 14, Color::rgb(0.6, 2.2, 3.0));
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "dash", 0.6, 1.0);
                    }
                }
                Event::ShieldHit { pos } => {
                    self.burst(scene, Emit::Spark, *pos, 5, Color::rgb(0.6, 2.2, 3.0));
                    let p = self.rng.range(0.9, 1.2);
                    play(&mut audio, &mut self.snd, 4, 2, "shield", 0.4, p);
                }
                Event::MissileLaunch { .. } => {
                    play(&mut audio, &mut self.snd, 1, 2, "missile", 0.5, 1.0);
                }
                Event::Ricochet { pos } => {
                    self.burst(scene, Emit::Spark, *pos, 4, Color::rgb(0.8, 2.0, 3.0));
                    let p = self.rng.range(0.9, 1.3);
                    play(&mut audio, &mut self.snd, 4, 2, "ricochet", 0.35, p);
                }
                Event::BulletWall { pos, hostile } => {
                    let c = if *hostile { Color::rgb(3.0, 0.6, 1.2) } else { Color::rgb(0.8, 2.0, 3.0) };
                    self.burst(scene, Emit::Spark, *pos, 3, c);
                }
                Event::MinePlaced { pos } => {
                    play(&mut audio, &mut self.snd, 1, 2, "mine", 0.5, 1.0);
                    self.ring(scene, *pos, 0.3, 1.2, 0.3, Color::rgb(2.4, 0.5, 0.7));
                }
                Event::MineArmed { pos } => {
                    self.ring(scene, *pos, 0.3, 0.9, 0.2, Color::rgb(3.0, 0.5, 0.6).with_alpha(0.6));
                }
                Event::Victory => {
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "win", 0.9, 1.0);
                    }
                }
                Event::Defeat => {
                    self.shake(scene, 1.0, opts);
                    self.burst(scene, Emit::Boom, Vec2::ZERO, 120, Color::rgb(3.0, 1.4, 0.5));
                    self.ring(scene, Vec2::ZERO, 1.0, 22.0, 0.9, Color::rgb(3.0, 1.4, 0.5));
                    if let Some(a) = audio.as_deref_mut() {
                        sound::play(a, "big_boom", 0.9, 0.8);
                        sound::play(a, "lose", 0.8, 1.0);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Синхронизация сцены с состоянием симуляции
    // -----------------------------------------------------------------------

    pub fn sync(&mut self, world: &mut World, sim: &Sim, dt: f32, t: f32) {
        // Рост пулов (новые объекты — из префабов).
        if sim.enemies.len() > self.enemies.len() {
            self.grow_enemies(world, sim.enemies.len() + 32);
        }
        if sim.pbullets.len() > self.pbul.len() {
            self.grow_pbul(world, sim.pbullets.len() + 16);
        }
        if sim.ebullets.len() > self.ebul.len() {
            self.grow_simple(world, 0, sim.ebullets.len() + 16);
        }
        if sim.pickups.len() > self.picks.len() {
            self.grow_simple(world, 1, sim.pickups.len() + 16);
        }
        if sim.telegraphs.len() > self.marks.len() {
            self.grow_simple(world, 2, (sim.telegraphs.len() + 16).min(MARK_POOL));
        }
        let shield_count = sim.stats.shield_count as usize;
        if shield_count > self.shields.len() {
            self.grow_simple(world, 3, shield_count);
        }
        let scene = &mut world.scene;
        // --- реактор ---
        let hp_frac = sim.core.hp / sim.core.max_hp.max(1.0);
        let pulse = 1.0 + self.core_cfg.pulse_amount * (t * self.core_cfg.pulse_speed).sin() + if sim.core.flash > 0.0 { self.core_cfg.hit_pulse } else { 0.0 };
        set_xform(scene, self.core, Vec2::ZERO, 0.0, Vec2::splat(pulse));
        let low = (1.0 - hp_frac * 1.6).clamp(0.0, 1.0);
        let mut c = Color::rgb(1.0, 1.0 - 0.55 * low, 1.0 - 0.7 * low);
        if sim.core.flash > 0.0 {
            c = self.core_cfg.hit_color;
        }
        set_color(scene, self.core, c);
        set_xform(scene, self.core_ring, Vec2::ZERO, t * self.core_cfg.ring_spin, Vec2::ONE);

        // --- дрон ---
        let p = &sim.player;
        self.player_pos = p.pos;
        let show_player = p.alive;
        scene.set_active(self.player, show_player);
        if show_player {
            let angle = p.aim.y.atan2(p.aim.x);
            let stretch = if p.dash_t > 0.0 { self.drone.dash_stretch } else { Vec2::ONE };
            set_xform(scene, self.player, p.pos, angle, stretch);
            let mut col = Color::WHITE;
            if p.invuln > 0.0 && p.dash_t <= 0.0 && (t * self.drone.blink_hz).sin() > 0.0 {
                col = Color::rgb(1.0, 1.0, 1.0).with_alpha(0.35);
            }
            if p.flash > 0.0 {
                col = self.drone.hurt_color;
            }
            if p.dash_t > 0.0 {
                col = self.drone.dash_color;
            }
            set_color(scene, self.player, col);
        }
        // Вспышка у ствола.
        let muzzle_on = p.muzzle > 0.0 && show_player;
        scene.set_active(self.muzzle, muzzle_on);
        if muzzle_on {
            set_xform(scene, self.muzzle, p.pos + p.aim * 0.75, 0.0, Vec2::ONE);
            set_color(scene, self.muzzle, self.drone.muzzle_color);
        }
        // Замедляющее поле.
        let field_on = sim.stats.slow_radius > 0.0 && show_player;
        scene.set_active(self.field, field_on);
        if field_on {
            let s = sim.stats.slow_radius * 2.0 / 6.0 * 1.0;
            set_xform(scene, self.field, p.pos, t * 0.5, Vec2::splat(s * 1.0));
            if let Some(sr) = scene.get_component_mut::<SpriteRenderer>(self.field) {
                sr.color = Color::rgb(0.5, 1.2, 2.0).with_alpha(0.55);
            }
        }

        // --- щиты ---
        let shield_pos = sim.shield_positions();
        for (i, s) in self.shields.iter_mut().enumerate() {
            if let (Some(pos), true) = (shield_pos.get(i), show_player) {
                set_on(scene, s, true);
                set_xform(scene, s.id, *pos, t * 4.0, Vec2::ONE);
            } else {
                set_on(scene, s, false);
            }
        }

        // --- враги ---
        for i in 0..sim.enemies.len() {
            let e = &sim.enemies[i];
            let mut slot = self.enemies[i];
            if !e.alive {
                set_on(scene, &mut slot, false);
                self.enemies[i] = slot;
                continue;
            }
            let fresh = !slot.active;
            set_on(scene, &mut slot, true);
            let info = e.kind.info();
            let kind_id = e.kind as u8;
            if slot.kind != kind_id {
                slot.kind = kind_id;
                let sprite = self.visual(e.kind).sprite.clone();
                if let Some(sr) = scene.get_component_mut::<SpriteRenderer>(slot.id) {
                    apply_sprite(sr, &sprite);
                }
            }
            if fresh && e.age < 0.2 {
                let col = kind_color(e.kind, 1.0).with_alpha(0.7);
                self.pending_fx_ring(scene, e.pos, info.radius, col);
            }
            // Вид: мигание при попадании, замедление, заряд, телеграф босса.
            let mut col = Color::WHITE;
            if e.slow < 1.0 {
                col = Color::rgb(0.65, 0.9, 1.35);
            }
            match e.kind {
                Kind::Shooter if e.st == 1 => col = Color::rgb(2.6, 1.4, 3.0),
                Kind::Bomber => {
                    let k = 0.5 + 0.5 * (e.age * (6.0 + e.age * 2.0)).sin();
                    col = Color::rgb(1.0 + 1.6 * k, 1.0 + 0.2 * k, 1.0);
                }
                Kind::Mine => {
                    let armed = e.timer <= 0.0;
                    let k = if armed { 0.5 + 0.5 * (e.age * 9.0).sin() } else { 0.25 };
                    col = Color::rgb(0.5 + 2.0 * k, 0.5, 0.5);
                }
                _ => {}
            }
            if e.vulnerable {
                col = Color::rgb(2.2, 2.0, 0.8);
            }
            if e.flash > 0.0 {
                col = Color::rgb(3.2, 3.2, 3.2);
            }
            set_color(scene, slot.id, col);
            let (wobble, pop) = {
                let v = self.visual(e.kind);
                (v.wobble, v.spawn_pop)
            };
            let mut s = 1.0;
            if e.age < pop {
                s = 0.3 + 0.7 * (e.age / pop);
            }
            if e.tele > 0.0 {
                s *= 1.0 + 0.05 * (e.age * 26.0).sin();
            }
            if !e.kind.is_boss() {
                s *= 1.0 + wobble * (e.age * 9.0 + e.uid as f32).sin();
            }
            set_xform(scene, slot.id, e.pos, e.heading, Vec2::splat(s));
            self.enemies[i] = slot;
        }

        // --- пули игрока ---
        for i in 0..sim.pbullets.len() {
            let b = &sim.pbullets[i];
            let mut slot = self.pbul[i];
            if !b.alive {
                set_on(scene, &mut slot, false);
                self.pbul[i] = slot;
                continue;
            }
            let fresh = !slot.active || slot.serial != b.serial;
            let flag = u8::from(b.missile);
            set_on(scene, &mut slot, true);
            if fresh || slot.kind != flag {
                slot.serial = b.serial;
                slot.kind = flag;
                if let Some(o) = scene.get_mut(slot.id) {
                    if let Some(sr) = o.get_mut::<SpriteRenderer>() {
                        apply_sprite(sr, if b.missile { &self.missile_sprite } else { &self.bullet_sprite });
                        sr.color = if b.crit { Color::rgb(3.0, 2.2, 0.8) } else { Color::WHITE };
                    }
                    if let Some(tr) = o.get_mut::<TrailRenderer>() {
                        tr.clear();
                        tr.emitting = true;
                        let c = if b.missile {
                            self.weapons.missile_trail
                        } else if b.crit {
                            self.weapons.crit_trail
                        } else {
                            self.weapons.bullet_trail
                        };
                        tr.start_color = c;
                        tr.end_color = c.with_alpha(0.0);
                        tr.start_width = if b.missile { 0.34 } else { 0.2 };
                        tr.lifetime = if b.missile { 0.35 } else { 0.17 };
                    }
                }
            }
            let angle = b.vel.y.atan2(b.vel.x);
            let sc = if b.crit && !b.missile { self.weapons.crit_scale } else { 1.0 };
            set_xform(scene, slot.id, b.pos, angle, Vec2::splat(sc));
            self.pbul[i] = slot;
        }

        // --- пули врагов ---
        for i in 0..sim.ebullets.len() {
            let b = &sim.ebullets[i];
            let mut slot = self.ebul[i];
            if !b.alive {
                set_on(scene, &mut slot, false);
                self.ebul[i] = slot;
                continue;
            }
            let fresh = !slot.active || slot.serial != b.serial;
            set_on(scene, &mut slot, true);
            if fresh {
                slot.serial = b.serial;
            }
            let pulse = (1.0 + 0.12 * (t * 18.0 + i as f32).sin()) * b.size * 1.5 / self.ebullet_size;
            set_xform(scene, slot.id, b.pos, 0.0, Vec2::splat(pulse));
            self.ebul[i] = slot;
        }

        // --- обломки ---
        for i in 0..sim.pickups.len() {
            let k = &sim.pickups[i];
            let mut slot = self.picks[i];
            if !k.alive {
                set_on(scene, &mut slot, false);
                self.picks[i] = slot;
                continue;
            }
            set_on(scene, &mut slot, true);
            let kind = k.kind as u8;
            if slot.serial != k.serial {
                slot.serial = k.serial;
                if let Some(sr) = scene.get_component_mut::<SpriteRenderer>(slot.id) {
                    apply_sprite(sr, if k.kind == PickupKind::Scrap { &self.scrap_sprite } else { &self.repair_sprite });
                }
            }
            let _ = kind;
            let bob = 1.0 + 0.12 * (t * 7.0 + i as f32 * 1.7).sin();
            let spin = if k.kind == PickupKind::Scrap { t * 3.0 + i as f32 } else { 0.0 };
            let big = if k.kind == PickupKind::Scrap && k.value > 1.5 { 1.4 } else { 1.0 };
            set_xform(scene, slot.id, k.pos, spin, Vec2::splat(bob * big));
            self.picks[i] = slot;
        }

        // --- появление врагов (метки на краю арены) ---
        for i in 0..self.marks.len() {
            let mut slot = self.marks[i];
            if let Some(tg) = sim.telegraphs.get(i) {
                set_on(scene, &mut slot, true);
                let frac = 1.0 - (tg.t / tg.total.max(0.01)).clamp(0.0, 1.0);
                let boss = tg.kind.is_boss();
                let size = if boss { 4.6 } else { 1.0 + tg.kind.info().radius * 1.6 };
                let s = size * (1.7 - 0.7 * frac);
                set_xform(scene, slot.id, tg.pos, frac * 5.0, Vec2::splat(s));
                let c = kind_color(tg.kind, 1.0 + 0.8 * frac);
                set_color(scene, slot.id, c.with_alpha(0.35 + 0.65 * frac));
            } else {
                set_on(scene, &mut slot, false);
            }
            self.marks[i] = slot;
        }

        // --- предупреждения боссов ---
        self.sync_boss_tele(scene, sim, t);

        // --- вспышки и числа ---
        self.update_fx(scene, dt);
        self.update_digits(scene, dt);

        // След дрона при рывке.
        if p.dash_t > 0.0 {
            self.burst(scene, Emit::Spark, p.pos, 3, Color::rgb(0.5, 1.8, 3.0));
        }
    }

    fn pending_fx_ring(&mut self, scene: &mut Scene, pos: Vec2, r: f32, color: Color) {
        self.ring(scene, pos, r * 0.6, r * 3.0, 0.25, color);
    }

    fn sync_boss_tele(&mut self, scene: &mut Scene, sim: &Sim, t: f32) {
        let boss = sim.boss().filter(|b| b.tele > 0.0);
        let (mut ring_on, mut line_on, mut mark_on) = (false, false, false);
        if let Some(b) = boss {
            let frac = (b.tele / 1.0).clamp(0.0, 1.0);
            let c = self.visual(b.kind).tele_color;
            match b.tele_kind {
                TELE_LINE => {
                    line_on = true;
                    let len = 20.0;
                    let dir = b.dir;
                    set_xform(scene, self.boss_line, b.pos + dir * len * 0.5, dir.y.atan2(dir.x), Vec2::new(len, 1.0 + 1.4 * (1.0 - frac)));
                    let a = 0.35 + 0.3 * (t * 30.0).sin().abs();
                    set_color(scene, self.boss_line, Color::rgb(3.0, 0.5, 0.35).with_alpha(a));
                }
                TELE_BLINK => {
                    mark_on = true;
                    set_xform(scene, self.boss_mark, b.aux_pos, t * 3.0, Vec2::splat(1.2 + 0.8 * frac));
                    set_color(scene, self.boss_mark, c.with_alpha(0.9));
                    ring_on = true;
                    set_xform(scene, self.boss_ring, b.pos, 0.0, Vec2::splat(b.r * 2.0 * (0.8 + frac * 1.2) / 6.0));
                    set_color(scene, self.boss_ring, c.with_alpha(0.7));
                }
                _ => {
                    ring_on = true;
                    let size = b.r * 2.0 * (1.0 + 1.6 * frac) / 6.0;
                    set_xform(scene, self.boss_ring, b.pos, 0.0, Vec2::splat(size));
                    set_color(scene, self.boss_ring, c.with_alpha(0.85 - 0.5 * frac));
                }
            }
        }
        for (id, on) in [(self.boss_ring, ring_on), (self.boss_line, line_on), (self.boss_mark, mark_on)] {
            scene.set_active(id, on);
        }
        self.last_boss_tele = boss.map_or(0.0, |b| b.tele);
    }

    fn update_fx(&mut self, scene: &mut Scene, dt: f32) {
        for f in &mut self.fx {
            if !f.active {
                continue;
            }
            f.age += dt;
            let t = f.age / f.life;
            if t >= 1.0 {
                f.active = false;
                scene.set_active(f.id, false);
                continue;
            }
            let e = 1.0 - (1.0 - t) * (1.0 - t);
            let s = f.s0 + (f.s1 - f.s0) * e;
            let a = f.color.a * (1.0 - t);
            if let Some(o) = scene.get_mut(f.id) {
                o.transform.scale = Vec3::new(s, s, 1.0);
                o.transform.rotation = Quat::from_rotation_z(f.spin * f.age);
                if let Some(sr) = o.get_mut::<SpriteRenderer>() {
                    sr.color = f.color.with_alpha(a);
                }
            }
        }
    }

    fn update_digits(&mut self, scene: &mut Scene, dt: f32) {
        for d in &mut self.digits {
            if !d.active {
                continue;
            }
            d.age += dt;
            let t = d.age / d.life;
            if t >= 1.0 {
                d.active = false;
                scene.set_active(d.id, false);
                continue;
            }
            d.vel.y *= (-2.5 * dt).exp();
            d.pos += d.vel * dt;
            let pop = if t < 0.15 { 0.6 + 0.4 * (t / 0.15) + 0.25 * (1.0 - t / 0.15) } else { 1.0 };
            if let Some(o) = scene.get_mut(d.id) {
                o.transform.position = Vec3::new(d.pos.x, d.pos.y, 0.0);
                o.transform.scale = Vec3::new(pop, pop, 1.0);
                if let Some(sr) = o.get_mut::<SpriteRenderer>() {
                    let a = if t > 0.6 { 1.0 - (t - 0.6) / 0.4 } else { 1.0 };
                    sr.color = d.color.with_alpha(a);
                }
            }
        }
    }

    /// Сколько спрайтов игры сейчас включено (для замеров и тестов).
    pub fn active_sprites(&self) -> usize {
        [&self.enemies, &self.pbul, &self.ebul, &self.picks, &self.marks, &self.shields].iter().map(|p| p.iter().filter(|s| s.active).count()).sum()
    }
}
