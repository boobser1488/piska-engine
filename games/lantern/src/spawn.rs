//! Появление объектов уровня по строкам вида `вид x y ключ=значение … | текст`.
//!
//! Координаты — мировые: `x` — середина клетки, `y` — низ клетки для тех, кто стоит на земле
//! (игрок, враги, ворота, столбы), и середина клетки для парящих (светлячки, мыши).
//! Для платформ `y` — верхняя грань.

use crate::boss::Boss;
use crate::common::{layer, Gfx};
use crate::enemies::{Bat, Brute, Crawler, Spitter};
use crate::objects::{Checkpoint, CrumblePlatform, Exit, Firefly, Gate, Hint, MovingPlatform, Spring};
use crate::player;
use engine::prelude::*;

/// Что появилось.
pub struct Spawned {
    pub kind: String,
    pub pos: Vec2,
    pub id: GameObjectId,
}

/// Разобранная строка.
pub struct Def<'a> {
    pub kind: &'a str,
    pub x: f32,
    pub y: f32,
    args: Vec<(&'a str, &'a str)>,
    pub text: &'a str,
}

impl<'a> Def<'a> {
    pub fn parse(line: &'a str) -> Result<Def<'a>, String> {
        let (head, text) = match line.split_once('|') {
            Some((h, t)) => (h, t.trim()),
            None => (line, ""),
        };
        let mut it = head.split_whitespace();
        let kind = it.next().ok_or("empty line")?;
        let mut num = |what: &str| -> Result<f32, String> {
            it.next()
                .ok_or(format!("missing {what}"))?
                .parse::<f32>()
                .map_err(|e| format!("bad {what}: {e}"))
        };
        let x = num("x")?;
        let y = num("y")?;
        let args = it.filter_map(|kv| kv.split_once('=')).collect();
        Ok(Def { kind, x, y, args, text })
    }

    pub fn f(&self, key: &str, default: f32) -> f32 {
        self.args
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn at(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Лист спрайтов: `grid` — столбцов × строк, `size` — размер в мировых единицах.
fn sheet(gfx: &Gfx, name: &str, grid: (u32, u32), size: (f32, f32), pivot_y: f32, layer: i32) -> SpriteRenderer {
    let mut s = SpriteRenderer::sheet(gfx.tex(name), grid.0, grid.1).with_size(size.0, size.1).with_pivot(0.5, pivot_y);
    s.sorting_layer = layer;
    s
}

/// Создать объект уровня по строке. Игрок получает способности по прогрессу.
pub fn spawn(ctx: &mut Ctx, line: &str, double_jump: bool, dash: bool) -> Result<Spawned, String> {
    let def = Def::parse(line)?;
    let gfx = ctx.resource::<Gfx>().cloned().ok_or("textures are not loaded")?;
    let pos = def.at();
    if def.kind == "player" {
        let id = player::spawn(ctx, pos, double_jump, dash);
        return Ok(Spawned { kind: "player".to_string(), pos, id });
    }
    let scene = &mut ctx.world.scene;
    let id = match def.kind {
        "crawler" => scene
            .build("Crawler")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "crawler", (4, 1), (1.0, 1.0), 0.0, layer::ENEMIES))
            .with(SpriteAnimator::new().with_clip("walk: 0-3 @8").starting_with("walk"))
            .with(Crawler { dir: def.f("dir", -1.0), speed: def.f("speed", 1.6), range: def.f("range", 0.0), ..Crawler::default() })
            .id(),
        "brute" => scene
            .build("Brute")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "brute", (4, 1), (1.5, 1.5), 0.0, layer::ENEMIES))
            .with(SpriteAnimator::new().with_clip("walk: 0-3 @5").starting_with("walk"))
            .with(Brute { dir: def.f("dir", -1.0), speed: def.f("speed", 1.3), range: def.f("range", 0.0), ..Brute::default() })
            .id(),
        "bat" => scene
            .build("Bat")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "bat", (4, 1), (1.0, 1.0), 0.5, layer::ENEMIES))
            .with(SpriteAnimator::new().with_clip("fly: 0-3 @10").starting_with("fly"))
            .with(Bat {
                anchor: pos,
                range: def.f("range", 3.0),
                amp: def.f("amp", 1.5),
                speed: def.f("speed", 1.0),
                t: def.f("phase", 0.0),
            })
            .id(),
        "spitter" => scene
            .build("Spitter")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "spitter", (4, 1), (1.2, 1.2), 0.0, layer::ENEMIES))
            .with(
                SpriteAnimator::new()
                    .with_clip("idle: 0-1 @3")
                    .with_clip("windup: 2 @1")
                    .with_clip("shoot: 3 @1")
                    .starting_with("idle"),
            )
            .with(Spitter { period: def.f("period", 2.6), timer: def.f("delay", 1.5), ..Spitter::default() })
            .id(),
        "boss" => {
            let mut s = sheet(&gfx, "boss", (4, 2), (4.0, 4.0), 0.5, layer::ENEMIES);
            s.order = 2;
            scene
                .build("Boss")
                .at(pos.x, pos.y)
                .with(s)
                .with(
                    SpriteAnimator::new()
                        .with_clip("fly: 0-3 @8")
                        .with_clip("telegraph: 4 @1")
                        .with_clip("hurt: 5 @1")
                        .with_clip("die: 6-7 @8")
                        .starting_with("fly"),
                )
                .with(Boss {
                    arena_x: Vec2::new(def.f("x0", pos.x - 14.0), def.f("x1", pos.x + 14.0)),
                    floor_y: def.f("floor", 1.0),
                    ceiling_y: def.f("ceil", 20.0),
                    door: Vec3::new(def.f("doorx", 0.0), def.f("doory0", 0.0), 0.0),
                    door_y1: def.f("doory1", 0.0),
                    ..Boss::default()
                })
                .id()
        }
        "firefly" | "golden" => {
            let golden = def.kind == "golden";
            let mut s = sheet(&gfx, "firefly", (4, 1), (if golden { 1.5 } else { 0.9 }, if golden { 1.5 } else { 0.9 }), 0.5, layer::ITEMS);
            s.color = if golden { Color::rgb(2.6, 1.7, 0.45) } else { Color::rgb(1.5, 1.45, 0.95) };
            let trail = if golden { TrailRenderer::glowing(Color::rgb(3.0, 1.9, 0.4), 0.3, 0.9) } else { TrailRenderer::glowing(Color::rgb(1.6, 1.5, 0.6), 0.14, 0.5) };
            scene
                .build(if golden { "GoldenFirefly" } else { "Firefly" })
                .at(pos.x, pos.y)
                .with(s)
                .with(SpriteAnimator::new().with_clip("fly: 0-3 @12").starting_with("fly"))
                .with(trail.with_layer(layer::ITEMS, -1))
                .with(Firefly { golden, ..Firefly::default() })
                .id()
        }
        "checkpoint" => scene
            .build("Checkpoint")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "checkpoint", (2, 1), (1.0, 1.5), 0.0, layer::ITEMS))
            .with(Checkpoint::default())
            .id(),
        "spring" => scene
            .build("Spring")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "spring", (2, 1), (1.0, 1.0), 0.0, layer::ITEMS))
            .with(Spring { power: def.f("power", 21.0), ..Spring::default() })
            .id(),
        "mplat" => {
            let mut s = SpriteRenderer::textured(gfx.tex("platform")).with_size(3.0, 0.5).with_pivot(0.5, 1.0);
            s.sorting_layer = layer::ITEMS;
            scene
                .build("MovingPlatform")
                .at(pos.x, pos.y)
                .with(s)
                .with(MovingPlatform {
                    from: pos,
                    to: pos + Vec2::new(def.f("dx", 0.0), def.f("dy", 0.0)),
                    speed: def.f("speed", 1.0),
                    t: def.f("phase", 0.0),
                    width: 3.0,
                })
                .id()
        }
        "cplat" => {
            let mut s = SpriteRenderer::sheet(gfx.tex("crumble"), 1, 3).with_size(3.0, 0.5).with_pivot(0.5, 1.0);
            s.sorting_layer = layer::ITEMS;
            scene.build("CrumblePlatform").at(pos.x, pos.y).with(s).with(CrumblePlatform::default()).id()
        }
        "gate" => scene
            .build("Gate")
            .at(pos.x, pos.y)
            .with(sheet(&gfx, "gate", (1, 1), (1.0, 3.0), 0.0, layer::ITEMS))
            .with(Gate { need: def.f("need", 5.0) as i32, ..Gate::default() })
            .id(),
        "exit" => {
            let id = scene
                .build("Exit")
                .at(pos.x, pos.y)
                .with(sheet(&gfx, "exit", (5, 1), (2.0, 2.0), 0.0, layer::ITEMS))
                .with(
                    SpriteAnimator::new()
                        .with_clip("off: 0 @1")
                        .with_clip("on: 1-4 @8")
                        .starting_with("off"),
                )
                .with(Exit::default())
                .id();
            let mut glow = ParticleSystem {
                playing: false,
                emission_rate: 14.0,
                lifetime: Vec2::new(0.8, 1.4),
                start_speed: Vec2::new(0.4, 1.0),
                start_size: Vec2::new(0.1, 0.22),
                start_color: Color::rgb(3.0, 2.2, 0.7),
                start_color_max: Color::rgb(2.0, 1.4, 0.4),
                color_over_lifetime: vec![Color::WHITE, Color::WHITE, Color::WHITE.with_alpha(0.0)],
                shape: EmitterShape::Box,
                shape_size: Vec3::new(1.2, 1.6, 0.0),
                cone_angle: 30.0,
                flat: true,
                blend: ParticleBlend::Additive,
                space: SimulationSpace::World,
                seed: 5,
                ..ParticleSystem::default()
            };
            glow.sorting_layer = layer::FX;
            scene.build("ExitGlow").at(0.0, 1.0).child_of(id).with(glow).id();
            id
        }
        "hint" => scene
            .build("Hint")
            .at(pos.x, pos.y)
            .with(Hint { text: def.text.to_string(), size: Vec2::new(def.f("w", 2.0), def.f("h", 2.0)), shown: false })
            .id(),
        other => return Err(format!("unknown object kind `{other}`")),
    };
    Ok(Spawned { kind: def.kind.to_string(), pos, id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines() {
        let d = Def::parse("bat 12.5 8 range=4 amp=1.5").unwrap();
        assert_eq!((d.kind, d.x, d.y), ("bat", 12.5, 8.0));
        assert_eq!(d.f("range", 0.0), 4.0);
        assert_eq!(d.f("speed", 1.0), 1.0);
        let h = Def::parse("hint 3 4 w=2 h=3 | Привет, мир").unwrap();
        assert_eq!(h.text, "Привет, мир");
        assert!(Def::parse("bat 1").is_err());
        assert!(Def::parse("").is_err());
    }
}
