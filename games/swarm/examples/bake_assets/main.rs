//! Запекатель ассетов «Роя»: пишет в `assets/` всё, что игра читает из файлов.
//!
//! `cargo run -p swarm --example bake_assets` (после `python tools/gen_assets.py` и
//! `python tools/gen_data.py`):
//!
//! * `data/waves.ron` — таблица волн 1-20 из генератора (после неё работает генератор);
//! * `audio/*.wav` — звуки и музыка из синтезатора движка;
//! * `prefabs/**` — дрон, реактор, враги, боссы, снаряды, обломки, эффекты;
//! * `ui/*.prefab` — экраны интерфейса;
//! * `scenes/arena.scene` — арена: базовые объекты, которые пишет `gen_assets.py`
//!   (камера, свечение, фон, пол), плюс дрон, реактор, менеджер игры и экраны интерфейса
//!   экземплярами префабов.
//!
//! Игра не использует этот код во время работы: после запекания она стартует из файлов.
//! Файлы можно править в редакторе; запекатель нужен, чтобы пересоздать их с нуля.

mod synth;
mod ui_build;

use std::path::{Path, PathBuf};

use engine::prelude::*;
use swarm::atlas::{self, Rect};
use swarm::data::{Kind, Pattern};
use swarm::defs::{self, bake_wave_table, Defs};
use swarm::scripts::bosses::Boss;
use swarm::scripts::drone::Drone;
use swarm::scripts::enemy_kinds::EnemyKind;
use swarm::scripts::game_manager::GameManager;
use swarm::scripts::hud::Hud;
use swarm::scripts::menus::Menus;
use swarm::scripts::reactor::ReactorCore;
use swarm::scripts::touch_controls::TouchControls;
use swarm::scripts::upgrades::Upgrades;
use swarm::scripts::waves::Waves;
use swarm::scripts::weapons::Weapons;

fn assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn main() -> Result<(), EngineError> {
    let assets = assets_dir();
    bake_waves(&assets);
    bake_audio(&assets);
    // Префабы и сцена читают базу ассетов; после записи файлов её открывают заново.
    let mut app = open_app(&assets)?;
    bake_prefabs(&mut app)?;
    let mut app = open_app(&assets)?;
    bake_ui(&mut app)?;
    let mut app = open_app(&assets)?;
    bake_scene(&mut app, &assets)?;
    println!("baked into {}", assets.display());
    Ok(())
}

fn open_app(assets: &Path) -> Result<App, EngineError> {
    let mut app = engine::app();
    swarm::register(&mut app);
    engine::mount_assets(&mut app, assets)?;
    Ok(app)
}

// ---------------------------------------------------------------------------
// Таблица волн и звук
// ---------------------------------------------------------------------------

fn bake_waves(assets: &Path) {
    // Генератор читает враги и баланс; берём их из файлов, а не из вшитой копии.
    let read = |name: &str| std::fs::read_to_string(assets.join("data").join(name)).unwrap_or_else(|e| panic!("data/{name}: {e}"));
    let defs = Defs::parse(&read("enemies.ron"), &read("upgrades.ron"), &read("balance.ron"), "(waves: [])").expect("data files are valid");
    defs::install(defs);
    let table = bake_wave_table(20);
    let cfg = ron::ser::PrettyConfig::new().indentor("    ".to_string());
    let body = ron::ser::to_string_pretty(&table, cfg).expect("wave table serializes");
    let header = "// Таблица волн кампании: что и когда появляется. Волны после 20-й (бесконечный режим)\n// строит генератор по `generator` из balance.ron. Запечено из генератора; править можно руками.\n";
    std::fs::write(assets.join("data/waves.ron"), format!("{header}{body}\n")).expect("write waves.ron");
    let groups: usize = table.waves.iter().map(|w| w.groups.len()).sum();
    println!("data/waves.ron: {} waves, {groups} spawn groups, patterns {:?}", table.waves.len(), [Pattern::Scatter, Pattern::Pack, Pattern::Ring]);
}

fn bake_audio(assets: &Path) {
    let dir = assets.join("audio");
    std::fs::create_dir_all(&dir).expect("create audio dir");
    let mut count = 0;
    synth::render_clips(|name, clip| {
        clip.save_wav(dir.join(format!("{name}.wav"))).unwrap_or_else(|e| panic!("{name}.wav: {e}"));
        count += 1;
    });
    println!("audio: {count} files");
}

// ---------------------------------------------------------------------------
// Префабы
// ---------------------------------------------------------------------------

const L_TELE: i32 = -5;
const L_PICKUP: i32 = 1;
const L_ENEMY: i32 = 2;
const L_BOSS: i32 = 3;
const L_PLAYER: i32 = 5;
const L_PBULLET: i32 = 6;
const L_FX: i32 = 7;
const L_PARTICLE: i32 = 8;
const L_DIGIT: i32 = 9;

/// Спрайт из прямоугольника атласа; `size` — мировой размер большей стороны.
fn sprite(tex: AssetHandle<Texture>, r: Rect, size: f32, layer: i32, order: i32) -> SpriteRenderer {
    let inset = 0.5;
    let k = size / r.w.max(r.h);
    SpriteRenderer {
        texture: tex,
        sorting_layer: layer,
        order,
        uv_offset: Vec2::new((r.x + inset) / atlas::ATLAS_W, (r.y + inset) / atlas::ATLAS_H),
        uv_size: Vec2::new((r.w - 2.0 * inset) / atlas::ATLAS_W, (r.h - 2.0 * inset) / atlas::ATLAS_H),
        size: Vec2::new(r.w * k, r.h * k),
        ..SpriteRenderer::default()
    }
}

struct Baker<'a> {
    app: &'a mut App,
    tex: AssetHandle<Texture>,
}

impl Baker<'_> {
    /// Собрать объект, сохранить как префаб и убрать из сцены.
    fn save(&mut self, path: &str, build: impl FnOnce(&mut Scene, AssetHandle<Texture>) -> GameObjectId) -> Result<(), EngineError> {
        let id = build(&mut self.app.world.scene, self.tex);
        let prefab = Prefab::from_object(&self.app.world.scene, id).expect("the object was just built");
        let file = assets_dir().join(path);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).expect("create prefab dir");
        }
        prefab.save(&file)?;
        self.app.world.scene.destroy(id);
        self.app.tick(0.0);
        Ok(())
    }
}

fn bake_prefabs(app: &mut App) -> Result<(), EngineError> {
    let tex = app.world.resources.get::<AssetDatabase>().expect("database").handle::<Texture>("sprites/atlas.png")?;
    let mut b = Baker { app, tex };
    let mut count = 0;

    // Враги и боссы: картинка и мелкие настройки вида; характеристики — в data/enemies.ron.
    let table: [(Kind, Rect, f32); 13] = [
        (Kind::Chaser, atlas::CHASER, 1.25),
        (Kind::Swarmling, atlas::SWARMLING, 0.85),
        (Kind::Shooter, atlas::SHOOTER, 1.5),
        (Kind::Tank, atlas::TANK, 2.4),
        (Kind::Splitter, atlas::SPLITTER, 1.7),
        (Kind::SplitterMini, atlas::SPLITTER_MINI, 1.0),
        (Kind::Bomber, atlas::BOMBER, 1.45),
        (Kind::MineLayer, atlas::MINELAYER, 1.75),
        (Kind::Mine, atlas::MINE, 1.0),
        (Kind::BossHive, atlas::BOSS_HIVE, 5.6),
        (Kind::BossRam, atlas::BOSS_RAM, 5.2),
        (Kind::BossConductor, atlas::BOSS_CONDUCTOR, 5.0),
        (Kind::BossOvermind, atlas::BOSS_OVERMIND, 6.2),
    ];
    for (kind, rect, size) in table {
        let def = kind.info().clone();
        let layer = if kind.is_boss() { L_BOSS } else if kind == Kind::Mine { L_ENEMY - 1 } else { L_ENEMY };
        let path = def.prefab.clone();
        b.save(&format!("{path}"), |scene, tex| {
            let id = scene.build(format!("{kind:?}")).tag(if kind.is_boss() { "Boss" } else { "Enemy" }).with(sprite(tex, rect, size, layer, 0)).id();
            let kind_comp = EnemyKind { kind: format!("{kind:?}"), wobble: if kind.is_boss() { 0.0 } else { 0.04 }, spawn_pop: 0.3 };
            scene.get_mut(id).expect("object").add(kind_comp);
            if kind.is_boss() {
                let c = def.rgb();
                scene.get_mut(id).expect("object").add(Boss {
                    intro: "Приближается босс: {boss}".to_string(),
                    telegraph_color: Color::rgb(c[0] * 1.2, c[1] * 1.2, c[2] * 1.2),
                });
            }
            id
        })?;
        count += 1;
    }

    // Снаряды.
    b.save("prefabs/projectiles/player_bullet.prefab", |scene, tex| {
        let id = scene.build("PlayerBullet").with(sprite(tex, atlas::PBULLET, 0.95, L_PBULLET, 0)).id();
        let trail = TrailRenderer { max_points: 24, ..TrailRenderer::glowing(Color::rgb(0.25, 1.5, 2.2), 0.2, 0.17) }.with_layer(L_PBULLET - 1, 0);
        scene.get_mut(id).expect("object").add(trail);
        id
    })?;
    b.save("prefabs/projectiles/missile.prefab", |scene, tex| {
        let id = scene.build("Missile").with(sprite(tex, atlas::MISSILE, 1.1, L_PBULLET, 0)).id();
        let trail = TrailRenderer { max_points: 24, ..TrailRenderer::glowing(Color::rgb(2.6, 1.2, 0.4), 0.34, 0.35) }.with_layer(L_PBULLET - 1, 0);
        scene.get_mut(id).expect("object").add(trail);
        id
    })?;
    b.save("prefabs/projectiles/enemy_bullet.prefab", |scene, tex| scene.build("EnemyBullet").with(sprite(tex, atlas::EBULLET, 1.0, 4, 0)).id())?;

    // Обломки.
    b.save("prefabs/pickups/scrap.prefab", |scene, tex| scene.build("Scrap").with(sprite(tex, atlas::SCRAP, 0.5, L_PICKUP, 0)).id())?;
    b.save("prefabs/pickups/repair.prefab", |scene, tex| scene.build("Repair").with(sprite(tex, atlas::REPAIR, 0.85, L_PICKUP, 0)).id())?;

    // Эффекты.
    b.save("prefabs/fx/ring.prefab", |scene, tex| scene.build("Ring").with(sprite(tex, atlas::RING, 1.0, L_FX, 0)).id())?;
    b.save("prefabs/fx/flash.prefab", |scene, tex| scene.build("Flash").with(sprite(tex, atlas::GLOW, 1.0, L_FX, 0)).id())?;
    b.save("prefabs/fx/digit.prefab", |scene, tex| scene.build("Digit").with(sprite(tex, atlas::DIGIT0, 0.5, L_DIGIT, 0)).id())?;
    b.save("prefabs/fx/marker.prefab", |scene, tex| scene.build("SpawnMarker").with(sprite(tex, atlas::TELEGRAPH, 1.0, L_TELE, 0)).id())?;
    b.save("prefabs/fx/shield.prefab", |scene, tex| scene.build("Shield").with(sprite(tex, atlas::SHIELD, 0.8, L_PLAYER, 0)).id())?;
    b.save("prefabs/fx/boss_ring.prefab", |scene, tex| scene.build("BossRing").with(sprite(tex, atlas::RING, 6.0, L_FX, 0)).id())?;
    b.save("prefabs/fx/boss_line.prefab", |scene, tex| scene.build("BossLine").with(sprite(tex, atlas::BAR, 1.0, L_TELE + 2, 0)).id())?;
    b.save("prefabs/fx/boss_mark.prefab", |scene, tex| scene.build("BossMark").with(sprite(tex, atlas::TELEGRAPH, 3.0, L_TELE + 3, 0)).id())?;
    b.save("prefabs/fx/spark.prefab", |scene, _| scene.build("SparkEmitter").with(spark_system()).id())?;
    b.save("prefabs/fx/boom.prefab", |scene, _| scene.build("BoomEmitter").with(boom_system()).id())?;
    b.save("prefabs/fx/scrap_sparkle.prefab", |scene, _| scene.build("ScrapSparkleEmitter").with(scrap_system()).id())?;

    // Дрон и реактор.
    b.save("prefabs/drone.prefab", |scene, tex| {
        let id = scene.build("Drone").tag("Player").with(sprite(tex, atlas::PLAYER, 1.45, L_PLAYER, 0)).with(Drone::default()).with(Weapons::default()).id();
        let muzzle = scene.build("Muzzle").child_of(id).with(sprite(tex, atlas::GLOW, 1.0, L_FX, 0)).inactive().id();
        let field = scene.build("SlowField").child_of(id).with(sprite(tex, atlas::FIELD, 6.0, L_TELE + 1, 0)).inactive().id();
        // Скрипты находят детей по ссылкам, а не по именам.
        let (muzzle, field) = (ObjectRef::new(scene, muzzle), ObjectRef::new(scene, field));
        if let Some(drone) = scene.get_component_mut::<Drone>(id) {
            drone.muzzle = muzzle;
            drone.slow_field = field;
        }
        id
    })?;
    b.save("prefabs/reactor_core.prefab", |scene, tex| {
        let id = scene.build("ReactorCore").with(sprite(tex, atlas::CORE, 2.7, 1, 0)).with(ReactorCore::default()).id();
        let ring = scene.build("Ring").child_of(id).with(sprite(tex, atlas::CORE_RING, 3.05, 1, 1)).id();
        let ring = ObjectRef::new(scene, ring);
        if let Some(core) = scene.get_component_mut::<ReactorCore>(id) {
            core.ring = ring;
        }
        id
    })?;
    println!("prefabs: {} enemies and bosses + projectiles, pickups, effects, drone, reactor", count);
    Ok(())
}

fn base_system() -> ParticleSystem {
    ParticleSystem {
        playing: false,
        looping: true,
        emission_rate: 0.0,
        duration: 1.0,
        shape: EmitterShape::Point,
        cone_angle: 180.0,
        flat: true,
        blend: ParticleBlend::Additive,
        particle_shape: ParticleShape::Soft,
        sorting_layer: L_PARTICLE,
        ..ParticleSystem::default()
    }
}

fn spark_system() -> ParticleSystem {
    ParticleSystem {
        max_particles: 220,
        lifetime: Vec2::new(0.22, 0.55),
        start_speed: Vec2::new(3.0, 9.0),
        start_size: Vec2::new(0.07, 0.17),
        start_color: Color::rgb(2.5, 2.0, 1.0),
        start_color_max: Color::rgb(1.4, 1.0, 0.5),
        drag: 3.0,
        velocity_stretch: 0.05,
        color_over_lifetime: vec![Color::WHITE, Color::WHITE.with_alpha(0.0)],
        size_over_lifetime: vec![1.0, 0.25],
        ..base_system()
    }
}

fn boom_system() -> ParticleSystem {
    ParticleSystem {
        max_particles: 320,
        lifetime: Vec2::new(0.3, 0.8),
        start_speed: Vec2::new(0.8, 6.0),
        start_size: Vec2::new(0.35, 0.9),
        start_color: Color::rgb(3.0, 1.6, 0.8),
        start_color_max: Color::rgb(1.6, 0.8, 0.4),
        drag: 3.2,
        color_over_lifetime: vec![Color::rgb(1.0, 1.0, 1.0).with_alpha(0.9), Color::rgb(0.7, 0.5, 0.4).with_alpha(0.5), Color::WHITE.with_alpha(0.0)],
        size_over_lifetime: vec![0.5, 1.0, 0.2],
        ..base_system()
    }
}

fn scrap_system() -> ParticleSystem {
    ParticleSystem {
        max_particles: 120,
        lifetime: Vec2::new(0.3, 0.6),
        start_speed: Vec2::new(0.5, 2.2),
        start_size: Vec2::new(0.08, 0.15),
        start_color: Color::rgb(0.6, 2.6, 1.6),
        start_color_max: Color::rgb(0.3, 1.4, 1.0),
        drag: 2.0,
        ..base_system()
    }
}

// ---------------------------------------------------------------------------
// Интерфейс и сцена
// ---------------------------------------------------------------------------

fn bake_ui(app: &mut App) -> Result<(), EngineError> {
    let db = app.world.resources.get::<AssetDatabase>().expect("database").clone();
    let canvases = ui_build::build_all(&mut app.world.scene, &db);
    for (name, id) in &canvases {
        let prefab = Prefab::from_object(&app.world.scene, *id).expect("canvas was just built");
        let file = assets_dir().join(format!("ui/{name}.prefab"));
        std::fs::create_dir_all(file.parent().expect("dir")).expect("create ui dir");
        prefab.save(&file)?;
    }
    println!("ui: {} prefabs", canvases.len());
    Ok(())
}

fn bake_scene(app: &mut App, assets: &Path) -> Result<(), EngineError> {
    engine::load_scene(app, "scenes/arena.scene")?;
    // Повторный запуск: убрать то, что запекатель добавил в прошлый раз.
    for name in ["Drone", "ReactorCore", "GameManager", "HUD", "MenuScreen", "ModeScreen", "OptionsScreen", "PauseScreen", "UpgradeScreen", "ResultsScreen"] {
        while let Some(id) = app.world.scene.find_by_name(name) {
            app.world.scene.destroy(id);
            app.tick(0.0);
        }
    }
    let drone = app.world.instantiate_at("prefabs/drone.prefab", Vec3::new(0.0, -2.8, 0.0), Quat::IDENTITY, None)?;
    let core = app.world.instantiate_at("prefabs/reactor_core.prefab", Vec3::ZERO, Quat::IDENTITY, None)?;
    // Экземпляр получает имя файла префаба; в сцене объекты называются как их ищет игра.
    for (id, name) in [(drone, "Drone"), (core, "ReactorCore")] {
        app.world.scene.get_mut(id).expect("instance").name = name.to_string();
    }
    let manager = app
        .world
        .scene
        .build("GameManager")
        .with(GameManager::default())
        .with(Menus::default())
        .with(TouchControls::default())
        .with(Waves::default())
        .with(Upgrades::default())
        .with(Hud::default())
        .id();
    let _ = manager;
    for (file, name) in [("hud", "HUD"), ("main_menu", "MenuScreen"), ("mode_select", "ModeScreen"), ("options", "OptionsScreen"), ("pause", "PauseScreen"), ("upgrade_cards", "UpgradeScreen"), ("results", "ResultsScreen")] {
        let id = app.world.instantiate(&format!("ui/{file}.prefab"))?;
        app.world.scene.get_mut(id).expect("instance").name = name.to_string();
    }
    app.world.save_scene(assets.join("scenes/arena.scene"))?;
    println!("scene: {} objects", app.world.scene.len());
    Ok(())
}
