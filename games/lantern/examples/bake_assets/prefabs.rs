//! Префабы персонажей, предметов, механизмов и эффектов: `assets/prefabs/*.prefab`.

use crate::Gfx;
use engine::prelude::*;
use lantern::common::layer;
use lantern::scripts::{
    bat::Bat, boss::Boss, brute::Brute, checkpoint::Checkpoint, crawler::Crawler, crumble_platform::CrumblePlatform, exit::Exit,
    firefly::Firefly, gate::Gate, moving_platform::MovingPlatform, player::Player, projectile::Projectile, rock::Rock,
    spitter::Spitter, spring::Spring,
};

/// Лист спрайтов: `grid` — столбцов × строк, `size` — размер в мировых единицах.
fn sheet(gfx: &Gfx, name: &str, grid: (u32, u32), size: (f32, f32), pivot_y: f32, layer: i32) -> SpriteRenderer {
    let mut s = SpriteRenderer::sheet(gfx.tex(name), grid.0, grid.1).with_size(size.0, size.1).with_pivot(0.5, pivot_y);
    s.sorting_layer = layer;
    s
}

/// Снять префаб с объекта `id` и записать в `<dir>/<file>.prefab`.
fn save(scene: &Scene, id: GameObjectId, dir: &std::path::Path, file: &str) {
    let prefab = Prefab::from_object(scene, id).expect("object exists");
    let path = dir.join(format!("{file}.prefab"));
    std::fs::create_dir_all(dir).expect("prefab directory");
    prefab.save(&path).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    println!("prefab {file}");
}

pub fn bake(scene: &mut Scene, gfx: &Gfx, dir: &std::path::Path) {
    // ---- игрок ----
    let animator = SpriteAnimator::new()
        .with_clip("idle: 0-3 @5")
        .with_clip("run: 8-13 @14")
        .with_clip("jump: 16 @1")
        .with_clip("fall: 17 @1")
        .with_clip("hurt: 18 @1")
        .with_clip("dash: 19 @1")
        .starting_with("idle");
    let mut sprite = sheet(gfx, "player", (8, 3), (1.15, 1.15), 0.0, layer::PLAYER);
    sprite.order = 5;
    let id = scene.build("Player").tag("Player").with(sprite).with(animator).with(Player::default()).id();
    // Свечение фонаря — позади героя: ниже слоя игрока и врагов.
    let mut glow = ParticleSystem {
        emission_rate: 9.0,
        lifetime: Vec2::new(0.5, 0.7),
        start_speed: Vec2::new(0.0, 0.0),
        start_size: Vec2::new(4.4, 5.2),
        start_color: Color::rgba(1.0, 0.62, 0.22, 0.05),
        start_color_max: Color::rgba(1.0, 0.7, 0.3, 0.065),
        color_over_lifetime: vec![Color::WHITE.with_alpha(0.0), Color::WHITE, Color::WHITE.with_alpha(0.0)],
        space: SimulationSpace::Local,
        blend: ParticleBlend::Additive,
        flat: true,
        max_particles: 12,
        seed: 3,
        ..ParticleSystem::default()
    };
    glow.sorting_layer = layer::ITEMS;
    glow.order = -5;
    scene.build("Glow").at(0.0, 0.55).child_of(id).with(glow).id();
    save(scene, id, dir, "player");

    // ---- враги ----
    let id = scene
        .build("Crawler")
        .tag("Enemy")
        .with(sheet(gfx, "crawler", (4, 1), (1.0, 1.0), 0.0, layer::ENEMIES))
        .with(SpriteAnimator::new().with_clip("walk: 0-3 @8").starting_with("walk"))
        .with(Crawler::default())
        .id();
    save(scene, id, dir, "crawler");

    let id = scene
        .build("Brute")
        .tag("Enemy")
        .with(sheet(gfx, "brute", (4, 1), (1.5, 1.5), 0.0, layer::ENEMIES))
        .with(SpriteAnimator::new().with_clip("walk: 0-3 @5").starting_with("walk"))
        .with(Brute::default())
        .id();
    save(scene, id, dir, "brute");

    let id = scene
        .build("Bat")
        .tag("Enemy")
        .with(sheet(gfx, "bat", (4, 1), (1.0, 1.0), 0.5, layer::ENEMIES))
        .with(SpriteAnimator::new().with_clip("fly: 0-3 @10").starting_with("fly"))
        .with(Bat::default())
        .id();
    save(scene, id, dir, "bat");

    let id = scene
        .build("Spitter")
        .tag("Enemy")
        .with(sheet(gfx, "spitter", (4, 1), (1.2, 1.2), 0.0, layer::ENEMIES))
        .with(
            SpriteAnimator::new()
                .with_clip("idle: 0-1 @3")
                .with_clip("windup: 2 @1")
                .with_clip("shoot: 3 @1")
                .starting_with("idle"),
        )
        .with(Spitter::default())
        .id();
    save(scene, id, dir, "spitter");

    let mut s = sheet(gfx, "boss", (4, 2), (4.0, 4.0), 0.5, layer::ENEMIES);
    s.order = 2;
    let id = scene
        .build("Boss")
        .tag("Enemy")
        .with(s)
        .with(
            SpriteAnimator::new()
                .with_clip("fly: 0-3 @8")
                .with_clip("telegraph: 4 @1")
                .with_clip("hurt: 5 @1")
                .with_clip("die: 6-7 @8")
                .starting_with("fly"),
        )
        .with(Boss::default())
        .id();
    save(scene, id, dir, "boss");

    // ---- светлячки ----
    for golden in [false, true] {
        let size = if golden { 1.5 } else { 0.9 };
        let mut s = sheet(gfx, "firefly", (4, 1), (size, size), 0.5, layer::ITEMS);
        s.color = if golden { Color::rgb(2.6, 1.7, 0.45) } else { Color::rgb(1.5, 1.45, 0.95) };
        let trail = if golden { TrailRenderer::glowing(Color::rgb(3.0, 1.9, 0.4), 0.3, 0.9) } else { TrailRenderer::glowing(Color::rgb(1.6, 1.5, 0.6), 0.14, 0.5) };
        let id = scene
            .build(if golden { "GoldenFirefly" } else { "Firefly" })
            .tag("Pickup")
            .with(s)
            .with(SpriteAnimator::new().with_clip("fly: 0-3 @12").starting_with("fly"))
            .with(trail.with_layer(layer::ITEMS, -1))
            .with(Firefly { golden, ..Firefly::default() })
            .id();
        save(scene, id, dir, if golden { "golden_firefly" } else { "firefly" });
    }

    // ---- механизмы ----
    let id = scene
        .build("Checkpoint")
        .with(sheet(gfx, "checkpoint", (2, 1), (1.0, 1.5), 0.0, layer::ITEMS))
        .with(Checkpoint::default())
        .id();
    save(scene, id, dir, "checkpoint");

    let id = scene
        .build("Spring")
        .with(sheet(gfx, "spring", (2, 1), (1.0, 1.0), 0.0, layer::ITEMS))
        .with(Spring { power: 21.0, ..Spring::default() })
        .id();
    save(scene, id, dir, "spring");

    let mut s = SpriteRenderer::textured(gfx.tex("platform")).with_size(3.0, 0.5).with_pivot(0.5, 1.0);
    s.sorting_layer = layer::ITEMS;
    let id = scene.build("MovingPlatform").with(s).with(MovingPlatform::default()).id();
    save(scene, id, dir, "moving_platform");

    let mut s = SpriteRenderer::sheet(gfx.tex("crumble"), 1, 3).with_size(3.0, 0.5).with_pivot(0.5, 1.0);
    s.sorting_layer = layer::ITEMS;
    let id = scene.build("CrumblePlatform").with(s).with(CrumblePlatform::default()).id();
    save(scene, id, dir, "crumble_platform");

    let id = scene
        .build("Gate")
        .with(sheet(gfx, "gate", (1, 1), (1.0, 3.0), 0.0, layer::ITEMS))
        .with(Gate::default())
        .id();
    save(scene, id, dir, "gate");

    let id = scene
        .build("Exit")
        .with(sheet(gfx, "exit", (5, 1), (2.0, 2.0), 0.0, layer::ITEMS))
        .with(SpriteAnimator::new().with_clip("off: 0 @1").with_clip("on: 1-4 @8").starting_with("off"))
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
    save(scene, id, dir, "exit");

    // ---- снаряды и камни ----
    let mut s = SpriteRenderer::textured(gfx.tex("projectile")).with_size(0.7, 0.7);
    s.sorting_layer = layer::ENEMIES;
    s.order = 3;
    let id = scene
        .build("Projectile")
        .with(s)
        .with(Projectile::default())
        .with(TrailRenderer::glowing(Color::rgb(2.2, 0.5, 1.2), 0.25, 0.35))
        .id();
    save(scene, id, dir, "projectile");

    let mut s = SpriteRenderer::textured(gfx.tex("rock")).with_size(1.0, 2.0).with_pivot(0.5, 1.0);
    s.sorting_layer = layer::ENEMIES;
    let id = scene.build("Rock").with(s).with(Rock::default()).id();
    save(scene, id, dir, "rock");

    // ---- эффекты (одноразовые частицы, удаляются сами) ----
    let mut p = ParticleSystem::dust();
    p.sorting_layer = layer::FX;
    let id = scene.build("FxDust").with(p).id();
    save(scene, id, dir, "fx_dust");
    let mut p = ParticleSystem::pickup_sparkle();
    p.sorting_layer = layer::FX;
    let id = scene.build("FxSparkle").with(p).id();
    save(scene, id, dir, "fx_sparkle");
    let mut p = ParticleSystem::explosion();
    p.sorting_layer = layer::FX;
    let id = scene.build("FxBurst").with(p).id();
    save(scene, id, dir, "fx_burst");
}
