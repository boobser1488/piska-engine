//! Спрайт-листы, анимация, частицы, тайлмапы и вспомогательные компоненты 2D:
//! поведение — без окна и GPU, картинка — через `Renderer::headless().capture` с
//! проверкой настоящих пикселей.

use std::path::PathBuf;
use std::sync::OnceLock;

use engine::prelude::*;
use engine_render::{CameraView, Renderer};
use image::RgbaImage;

const DT: f32 = 1.0 / 16.0; // точное в двоичной записи: время кадров считается без накопления ошибки

component! {
    /// Записывает все сообщения `animation_finished`.
    pub struct Recorder {
        pub finished: Vec<String> = Vec::new(),
    }
}
impl Component for Recorder {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        if message.name == engine_render::ANIMATION_FINISHED {
            if let Value::Str(clip) = &message.value {
                self.finished.push(clip.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Тестовые ассеты: цветные плитки, чтобы по одному пикселю узнавать кадр
// ---------------------------------------------------------------------------

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// Цвета ячеек листа 4×2 (в порядке кадров).
const SHEET: [[u8; 4]; 8] = [
    RED,
    GREEN,
    BLUE,
    YELLOW,
    [0, 255, 255, 255],
    [255, 0, 255, 255],
    [255, 255, 255, 255],
    [255, 128, 0, 255],
];

fn assets_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("piska_features2d_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, image: &RgbaImage, guid: &str| {
            image.save(dir.join(name)).unwrap();
            std::fs::write(
                dir.join(format!("{name}.meta")),
                format!(
                    "(\n    guid: \"{guid}\",\n    importer: \"Texture\",\n    settings: {{\n        \"filter\": Enum(\"Nearest\"),\n    }},\n)\n"
                ),
            )
            .unwrap();
        };
        // Лист: 4 столбца × 2 строки, ячейка 8×8 пикселей.
        let sheet = RgbaImage::from_fn(32, 16, |x, y| image::Rgba(SHEET[((y / 8) * 4 + x / 8) as usize]));
        write("sheet.png", &sheet, "00000000000000000000000000000001");
        // Картинка 2×2 для проверки отражений: красный и зелёный сверху, синий и жёлтый снизу.
        let quad = RgbaImage::from_fn(2, 2, |x, y| image::Rgba([RED, GREEN, BLUE, YELLOW][(y * 2 + x) as usize]));
        write("quad.png", &quad, "00000000000000000000000000000002");
        dir
    })
}

fn app_with_assets() -> App {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets_dir()).expect("test assets");
    app
}

fn texture(app: &App, name: &str) -> AssetHandle<Texture> {
    app.world
        .resources
        .get::<AssetDatabase>()
        .unwrap()
        .handle(name)
        .unwrap()
}

/// Чёрный фон, камера 4×4 мировых единицы: 64 px → 16 px на единицу.
fn add_camera(app: &mut App) {
    app.spawn("Camera").with(Camera {
        clear_color: Color::BLACK,
        ..Camera::orthographic(2.0)
    });
}

const SIDE: u32 = 64;

/// Пиксель в мировой точке (камера смотрит в начало координат).
fn px(image: &RgbaImage, x: f32, y: f32) -> [u8; 4] {
    let col = ((x + 2.0) / 4.0 * SIDE as f32) as u32;
    let row = ((2.0 - y) / 4.0 * SIDE as f32) as u32;
    image.get_pixel(col.min(SIDE - 1), row.min(SIDE - 1)).0
}

/// Тесты с GPU идут по одному: одновременное создание нескольких устройств Vulkan в
/// параллельных потоках на некоторых драйверах зависает.
fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn capture(app: &App) -> RgbaImage {
    let _gpu = gpu_lock();
    Renderer::headless()
        .expect("a GPU adapter")
        .capture(&app.world, SIDE, SIDE)
        .expect("capture")
}

fn component<T: Component>(app: &App, id: GameObjectId) -> &T {
    app.world.scene.get_component::<T>(id).expect("component")
}

// ---------------------------------------------------------------------------
// Анимация
// ---------------------------------------------------------------------------

#[test]
fn animator_picks_the_frame_for_the_elapsed_time() {
    let mut app = engine::app();
    let hero = app
        .spawn("Hero")
        .with(SpriteRenderer::default())
        .with(SpriteAnimator::new().with_clip("run: 10-13 @8"))
        .id();
    let mut seen = Vec::new();
    for _ in 0..8 {
        app.tick(DT);
        seen.push(component::<SpriteRenderer>(&app, hero).frame);
    }
    // 8 кадров/с при шаге 1/16 с: каждый кадр клипа держится два тика.
    assert_eq!(seen, vec![10, 11, 11, 12, 12, 13, 13, 10]);
    assert_eq!(component::<SpriteAnimator>(&app, hero).current_clip(), "run");
}

#[test]
fn once_clip_stops_and_reports_exactly_once() {
    let mut app = engine::app();
    let hero = app
        .spawn("Hero")
        .with(SpriteRenderer::default())
        .with(SpriteAnimator::new().with_clip("die: 0-3 @8 once"))
        .with(Recorder::default())
        .id();
    let mut events = 0;
    for tick in 1..=14 {
        app.tick(DT);
        events += app.world.events.read::<AnimationFinished>().count();
        let animator = component::<SpriteAnimator>(&app, hero);
        if tick < 8 {
            assert!(!animator.is_finished(), "tick {tick}");
        } else {
            assert!(animator.is_finished(), "tick {tick}");
            assert_eq!(animator.current_frame(), 3, "stays on the last frame");
        }
    }
    assert_eq!(component::<Recorder>(&app, hero).finished, vec!["die".to_string()]);
    assert_eq!(events, 1);
}

#[test]
fn play_keeps_the_running_clip_and_restarts_on_change() {
    let mut animator = SpriteAnimator::new()
        .with_clip("idle: 0-1 @2")
        .with_clip("run: 4-7 @8");
    animator.play("run");
    animator.advance(0.3);
    let time = animator.time;
    animator.play("run");
    assert_eq!(animator.time, time, "calling play every frame must not restart the clip");
    animator.play("idle");
    assert_eq!(animator.time, 0.0);
    assert_eq!(animator.current_clip(), "idle");
    animator.play("no_such_clip");
    assert_eq!(animator.current_clip(), "idle", "an unknown clip is ignored");
    let mut broken = SpriteAnimator::new().with_clip("oops 1-2");
    assert_eq!(broken.clip_errors().len(), 1);
}

// ---------------------------------------------------------------------------
// Сцены и reflection
// ---------------------------------------------------------------------------

#[test]
fn new_components_survive_a_scene_round_trip() {
    let mut app = engine::app();
    app.spawn("Map").with(
        Tilemap::from_rows(&[". 2 .", "10*1"])
            .with_tile_size(0.5, 0.5)
            .with_solid_tiles(&[1])
            .with_one_way_tiles(&[2]),
    );
    app.spawn("Hero").with(SpriteRenderer {
        sheet_columns: 6,
        sheet_rows: 3,
        frame: 7,
        pivot: Vec2::new(0.5, 0.0),
        flip_y: true,
        ..SpriteRenderer::default()
    }).with(SpriteAnimator::new().with_clip("run: 6-11 @12").with_clip("idle: 0-3 @6 pingpong"));
    app.spawn("Fx").with(ParticleSystem {
        emission_rate: 33.0,
        bursts: vec![Vec2::new(0.5, 12.0)],
        blend: ParticleBlend::Additive,
        shape: EmitterShape::Box,
        color_over_lifetime: vec![Color::WHITE, Color::rgba(1.0, 0.5, 0.0, 0.5), Color::TRANSPARENT],
        ..ParticleSystem::default()
    });
    app.spawn("Trail").with(TrailRenderer::glowing(Color::rgb(2.0, 1.0, 0.5), 0.3, 0.7));
    app.spawn("Cam").with(Camera::default()).with(CameraController2D::following("Player"));

    let text = app.world.scene_to_string();
    let mut copy = engine::app();
    copy.world.load_scene_str(&text, "round trip").unwrap_or_else(|e| panic!("{e}\n{text}"));

    let map = component::<Tilemap>(&copy, copy.world.scene.find_by_name("Map").unwrap());
    assert_eq!(map.rows, vec![". 2 .".to_string(), "10*1".to_string()]);
    assert_eq!(map.solid_tiles, vec![1]);
    assert_eq!(map.one_way_tiles, vec![2]);
    assert_eq!(map.tile_size, Vec2::new(0.5, 0.5));
    let hero = copy.world.scene.find_by_name("Hero").unwrap();
    let sprite = component::<SpriteRenderer>(&copy, hero);
    assert_eq!((sprite.sheet_columns, sprite.sheet_rows, sprite.frame), (6, 3, 7));
    assert!(sprite.flip_y);
    assert_eq!(sprite.pivot, Vec2::new(0.5, 0.0));
    assert_eq!(component::<SpriteAnimator>(&copy, hero).clips.len(), 2);
    let fx = component::<ParticleSystem>(&copy, copy.world.scene.find_by_name("Fx").unwrap());
    assert_eq!(fx.emission_rate, 33.0);
    assert_eq!(fx.bursts, vec![Vec2::new(0.5, 12.0)]);
    assert_eq!(fx.blend, ParticleBlend::Additive);
    assert_eq!(fx.shape, EmitterShape::Box);
    assert_eq!(fx.color_over_lifetime.len(), 3);
    assert!(copy.world.scene.find_by_name("Trail").is_some());

    // Редактор правит поля через reflection: клипы и строки карты — обычные списки строк.
    let mut animator = SpriteAnimator::default();
    animator
        .set_field("clips", &Value::List(vec![Value::Str("walk: 0-3 @10".into())]))
        .unwrap();
    assert_eq!(animator.clip_named("walk").map(|c| c.frames.len()), Some(4));
}

#[test]
fn old_sprite_scenes_still_load_without_the_new_fields() {
    let mut app = engine::app();
    let scene = r#"(
        name: "old",
        objects: [(
            name: "Old",
            components: [(kind: "SpriteRenderer", fields: {
                "size": Vec2(2.0, 1.0),
                "flip_x": Bool(true),
            })],
        )],
    )"#;
    app.world.load_scene_str(scene, "old").unwrap_or_else(|e| panic!("{e}"));
    let id = app.world.scene.find_by_name("Old").unwrap();
    let sprite = component::<SpriteRenderer>(&app, id);
    assert!(sprite.flip_x);
    assert_eq!((sprite.sheet_columns, sprite.sheet_rows, sprite.frame), (1, 1, 0));
    assert_eq!(sprite.pivot, Vec2::new(0.5, 0.5));
    assert_eq!((sprite.uv_offset, sprite.uv_size), (Vec2::ZERO, Vec2::ONE));
}

// ---------------------------------------------------------------------------
// Частицы
// ---------------------------------------------------------------------------

fn emitter(app: &mut App, system: ParticleSystem) -> GameObjectId {
    app.spawn("Emitter").with(system).id()
}

#[test]
fn emission_rate_gives_the_expected_steady_state() {
    let mut app = engine::app();
    let id = emitter(
        &mut app,
        ParticleSystem {
            emission_rate: 100.0,
            lifetime: Vec2::new(1.0, 1.0),
            seed: 7,
            ..ParticleSystem::default()
        },
    );
    app.run_headless(32, DT); // 2 с > времени жизни
    let alive = component::<ParticleSystem>(&app, id).alive_count();
    assert!((98..=101).contains(&alive), "100 particles/s × 1 s lifetime, got {alive}");
}

#[test]
fn bursts_repeat_every_cycle_and_respect_the_cap() {
    let mut app = engine::app();
    let id = emitter(
        &mut app,
        ParticleSystem {
            emission_rate: 0.0,
            duration: 1.0,
            bursts: vec![Vec2::new(0.5, 10.0)],
            lifetime: Vec2::new(100.0, 100.0),
            seed: 3,
            ..ParticleSystem::default()
        },
    );
    app.run_headless(35, DT); // 2.19 с: залпы в 0.5 и 1.5
    assert_eq!(component::<ParticleSystem>(&app, id).alive_count(), 20);

    let capped = emitter(
        &mut app,
        ParticleSystem {
            emission_rate: 1000.0,
            max_particles: 50,
            lifetime: Vec2::new(10.0, 10.0),
            ..ParticleSystem::default()
        },
    );
    app.run_headless(16, DT);
    assert_eq!(component::<ParticleSystem>(&app, capped).alive_count(), 50);
}

#[test]
fn one_shot_effect_dies_and_removes_its_object() {
    let mut app = engine::app();
    let id = emitter(&mut app, ParticleSystem::explosion().with_seed(1));
    app.tick(DT);
    let system = component::<ParticleSystem>(&app, id);
    assert_eq!(system.alive_count(), 48, "the burst fires on the first frame");
    assert!(app.world.scene.contains(id));
    app.run_headless(30, DT); // 1.9 с > максимального времени жизни 0.8 с
    assert!(!app.world.scene.contains(id), "destroy_when_done removes the object");
}

#[test]
fn same_seed_same_particles_different_seed_different() {
    let run = |seed: i32| {
        let mut app = engine::app();
        let id = emitter(&mut app, ParticleSystem::sparks().with_seed(seed));
        app.run_headless(20, DT);
        component::<ParticleSystem>(&app, id).particles().to_vec()
    };
    let (a, b, c) = (run(42), run(42), run(43));
    assert!(a.len() > 20);
    assert_eq!(a, b, "a seeded system is deterministic");
    assert_ne!(a, c);
}

#[test]
fn gravity_and_drag_act_on_velocity() {
    let mut system = ParticleSystem {
        emission_rate: 0.0,
        bursts: vec![Vec2::new(0.0, 1.0)],
        start_speed: Vec2::new(4.0, 4.0),
        cone_angle: 0.0,
        gravity: Vec3::new(0.0, -10.0, 0.0),
        lifetime: Vec2::new(10.0, 10.0),
        seed: 5,
        ..ParticleSystem::default()
    };
    system.simulate(0.0, Mat4::IDENTITY);
    let v0 = system.particles()[0].velocity;
    assert!((v0 - Vec3::new(0.0, 4.0, 0.0)).length() < 1e-4, "cone angle 0 shoots along +Y: {v0}");
    for _ in 0..10 {
        system.simulate(0.05, Mat4::IDENTITY);
    }
    let v = system.particles()[0].velocity;
    assert!((v.y - (4.0 - 10.0 * 0.5)).abs() < 1e-3, "v = {v}");

    system.gravity = Vec3::ZERO;
    system.drag = 5.0;
    let before = system.particles()[0].velocity.length();
    system.simulate(0.1, Mat4::IDENTITY);
    let after = system.particles()[0].velocity.length();
    assert!((after - before / 1.5).abs() < 1e-3, "{before} → {after}");
}

#[test]
fn world_space_particles_stay_behind_but_local_ones_follow() {
    let positions = |space: SimulationSpace| {
        let mut app = engine::app();
        let id = emitter(
            &mut app,
            ParticleSystem {
                emission_rate: 40.0,
                start_speed: Vec2::ZERO,
                lifetime: Vec2::new(5.0, 5.0),
                space,
                flat: true,
                seed: 9,
                ..ParticleSystem::default()
            },
        );
        for _ in 0..32 {
            app.world.scene.get_mut(id).unwrap().transform.position.x += 0.125; // 2 м/с
            app.tick(DT);
        }
        component::<ParticleSystem>(&app, id)
            .particles()
            .iter()
            .map(|p| p.position.x)
            .collect::<Vec<_>>()
    };
    let world = positions(SimulationSpace::World);
    let spread = world.iter().cloned().fold(f32::MIN, f32::max) - world.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread > 3.0, "world-space particles are left along the path, spread {spread}");
    let local = positions(SimulationSpace::Local);
    assert!(local.iter().all(|x| x.abs() < 0.2), "local-space particles stay at the emitter");
}

#[test]
fn emitter_shapes_place_particles_where_documented() {
    let spawn = |shape: EmitterShape, size: Vec3| {
        let mut system = ParticleSystem {
            shape,
            shape_size: size,
            emission_rate: 0.0,
            bursts: vec![Vec2::new(0.0, 200.0)],
            start_speed: Vec2::new(1.0, 1.0),
            lifetime: Vec2::new(10.0, 10.0),
            flat: true,
            seed: 11,
            ..ParticleSystem::default()
        };
        system.simulate(0.0, Mat4::from_translation(Vec3::new(10.0, 20.0, 0.0)));
        system.particles().to_vec()
    };
    for p in spawn(EmitterShape::Circle, Vec3::splat(2.0)) {
        let offset = Vec2::new(p.position.x - 10.0, p.position.y - 20.0);
        assert!(offset.length() <= 2.001, "inside the disc");
        assert!(offset.dot(p.velocity.truncate()) >= -1e-3, "circle emits outward");
    }
    for p in spawn(EmitterShape::Box, Vec3::new(4.0, 1.0, 0.0)) {
        assert!((p.position.x - 10.0).abs() <= 2.001 && (p.position.y - 20.0).abs() <= 0.501);
    }
    for p in spawn(EmitterShape::Cone, Vec3::splat(0.1)) {
        assert!(p.velocity.y > 0.0, "cone fans out around +Y");
        assert_eq!(p.velocity.z, 0.0, "flat mode keeps particles in the XY plane");
    }
}

// ---------------------------------------------------------------------------
// Тайлмап: коллайдеры
// ---------------------------------------------------------------------------

fn colliders(app: &App, map: GameObjectId) -> Vec<(Vec2, Vec2, bool)> {
    app.world
        .scene
        .get(map)
        .unwrap()
        .children()
        .iter()
        .filter_map(|id| {
            let object = app.world.scene.get(*id)?;
            let collider = object.get::<Collider2D>()?;
            Some((object.transform.position.truncate(), collider.size, collider.one_way))
        })
        .collect()
}

#[test]
fn solid_tiles_become_a_few_merged_static_colliders() {
    let mut app = engine::app();
    let map = app
        .spawn("Map")
        .with(
            Tilemap::from_rows(&[
                ". . . . . .",
                "1 . . . . 1",
                "1 2 2 2 2 1",
                "1 1 1 1 1 1",
            ])
            .with_solid_tiles(&[1])
            .with_one_way_tiles(&[2]),
        )
        .id();
    app.tick(DT);
    let found = colliders(&app, map);
    let solid: Vec<_> = found.iter().filter(|c| !c.2).collect();
    let one_way: Vec<_> = found.iter().filter(|c| c.2).collect();
    // Две колонны 1×3 и пол 4×1 между ними — три прямоугольника вместо десяти тайлов.
    assert_eq!(solid.len(), 3, "{solid:?}");
    assert_eq!(solid.iter().map(|c| c.1.x * c.1.y).sum::<f32>(), 10.0);
    assert!(solid.iter().any(|c| c.1 == Vec2::new(4.0, 1.0) && c.0 == Vec2::new(3.0, 0.5)));
    assert!(solid.iter().any(|c| c.1 == Vec2::new(1.0, 3.0) && c.0 == Vec2::new(0.5, 1.5)));
    assert_eq!(one_way.len(), 1);
    assert_eq!(one_way[0].1, Vec2::new(4.0, 1.0));
    assert_eq!(one_way[0].0, Vec2::new(3.0, 1.5));

    // Правка карты во время игры пересобирает коллайдеры.
    app.world.scene.get_component_mut::<Tilemap>(map).unwrap().set_tile(0, 3, 0);
    app.run_headless(2, DT);
    let after = colliders(&app, map);
    assert!(
        after.iter().any(|c| !c.2 && c.1 == Vec2::new(1.0, 2.0) && c.0 == Vec2::new(0.5, 2.0)),
        "the left column lost its bottom tile and shrank: {after:?}"
    );
    assert!(!after.iter().any(|c| c.1 == Vec2::new(1.0, 3.0) && c.0.x == 0.5));
    // Старые коллайдеры удалены в конце кадра, живых детей ровно по числу прямоугольников.
    let alive = app.world.scene.get(map).unwrap().children().iter().filter(|c| app.world.scene.contains(**c)).count();
    assert_eq!(alive, after.len());
}

#[test]
fn a_body_comes_to_rest_on_tile_colliders_and_one_way_tiles_let_it_up() {
    let mut app = engine::app();
    app.spawn("Map").with(Tilemap::from_rows(&["10*.", "10*4", "10*.", "10*1"]).with_solid_tiles(&[1]).with_one_way_tiles(&[4]));
    // Полка на высоте y ∈ [2, 3], пол y ∈ [0, 1].
    let faller = app
        .spawn("Faller")
        .at(5.5, 6.0)
        .with(Rigidbody2D::dynamic().locked_rotation())
        .with(Collider2D::rect(0.8, 0.8))
        .id();
    app.run_headless(160, 1.0 / 60.0);
    let y = app.world.scene.world_position(faller).y;
    // Падает на полку: верх полки y = 3, половина высоты тела 0.4.
    assert!((y - 3.4).abs() < 0.06, "rests on the one-way shelf at y = {y}");

    // Тело под полкой подпрыгивает сквозь неё и садится сверху.
    let jumper = app
        .spawn("Jumper")
        .at(2.5, 1.5)
        .with(Rigidbody2D {
            velocity: Vec2::new(0.0, 9.0),
            ..Rigidbody2D::dynamic().locked_rotation()
        })
        .with(Collider2D::rect(0.8, 0.8))
        .id();
    app.run_headless(240, 1.0 / 60.0);
    let y = app.world.scene.world_position(jumper).y;
    assert!((y - 3.4).abs() < 0.06, "passed through the shelf from below, landed on top: y = {y}");

    // Тело на полу рядом: верх пола y = 1.
    let walker = app
        .spawn("Walker")
        .at(0.5, 1.45)
        .with(Rigidbody2D::dynamic().locked_rotation())
        .with(Collider2D::rect(0.8, 0.8))
        .id();
    app.run_headless(60, 1.0 / 60.0);
    // x = 0.5 — под левой кромкой полки? полка начинается с x = 0, поэтому тело встаёт на неё.
    let y = app.world.scene.world_position(walker).y;
    assert!(y > 1.3, "stays above the floor: {y}");
}

#[test]
fn a_tilemap_without_physics_registered_reports_instead_of_panicking() {
    let mut app = App::new();
    engine_render::register_components(&mut app.world.registry);
    app.spawn("Map").with(Tilemap::from_rows(&["1 1"]).with_solid_nonempty());
    app.run_headless(2, DT);
    let map = app.world.scene.find_by_name("Map").unwrap();
    assert!(app.world.scene.get(map).unwrap().children().is_empty());
}

// ---------------------------------------------------------------------------
// Картинка: настоящие пиксели
// ---------------------------------------------------------------------------

#[test]
fn a_sheet_frame_shows_exactly_its_cell() {
    let mut app = app_with_assets();
    add_camera(&mut app);
    let sheet = texture(&app, "sheet.png");
    for (i, x) in [(0u32, -1.5), (5, -0.5), (3, 0.5), (6, 1.5)] {
        app.spawn("Frame")
            .at(x, 0.0)
            .with(SpriteRenderer::sheet(sheet, 4, 2).with_frame(i).with_size(1.0, 1.0));
    }
    app.tick(DT);
    let image = capture(&app);
    for (i, x) in [(0usize, -1.5), (5, -0.5), (3, 0.5), (6, 1.5)] {
        assert_eq!(px(&image, x, 0.0), SHEET[i], "frame {i}");
        assert_eq!(px(&image, x, 0.3), SHEET[i], "the whole sprite is one cell, frame {i}");
    }
    assert_eq!(px(&image, 0.0, 1.5), BLACK);
}

#[test]
fn an_animated_sprite_draws_the_current_frame() {
    let mut app = app_with_assets();
    add_camera(&mut app);
    let sheet = texture(&app, "sheet.png");
    app.spawn("Hero")
        .with(SpriteRenderer::sheet(sheet, 4, 2).with_size(2.0, 2.0))
        .with(SpriteAnimator::new().with_clip("blink: 1,2 @8"));
    app.tick(DT); // время 1/16: кадр 1 (зелёный)
    assert_eq!(px(&capture(&app), 0.0, 0.0), SHEET[1]);
    app.tick(DT);
    app.tick(DT); // время 3/16 → шаг 1 → кадр 2 (синий)
    assert_eq!(px(&capture(&app), 0.0, 0.0), SHEET[2]);
}

#[test]
fn flips_and_pivot_move_the_right_pixels() {
    let mut app = app_with_assets();
    add_camera(&mut app);
    let quad = texture(&app, "quad.png");
    // Четыре копии по углам, у каждой своя комбинация отражений; размер 1.6 → пиксель-ячейки 0.8.
    let variants = [(-1.0, 1.0, false, false), (1.0, 1.0, true, false), (-1.0, -1.0, false, true), (1.0, -1.0, true, true)];
    for (x, y, fx, fy) in variants {
        app.spawn("Quad").at(x, y).with(SpriteRenderer {
            texture: quad,
            size: Vec2::new(1.6, 1.6),
            flip_x: fx,
            flip_y: fy,
            ..SpriteRenderer::default()
        });
    }
    app.tick(DT);
    let image = capture(&app);
    let corner = |cx: f32, cy: f32, dx: f32, dy: f32| px(&image, cx + dx * 0.4, cy + dy * 0.4);
    // Без отражений: сверху-слева красный, сверху-справа зелёный, снизу-слева синий, снизу-справа жёлтый.
    assert_eq!(corner(-1.0, 1.0, -1.0, 1.0), RED);
    assert_eq!(corner(-1.0, 1.0, 1.0, 1.0), GREEN);
    assert_eq!(corner(-1.0, 1.0, -1.0, -1.0), BLUE);
    assert_eq!(corner(-1.0, 1.0, 1.0, -1.0), YELLOW);
    // flip_x: красный и зелёный меняются местами.
    assert_eq!(corner(1.0, 1.0, -1.0, 1.0), GREEN);
    assert_eq!(corner(1.0, 1.0, 1.0, 1.0), RED);
    // flip_y: верх и низ меняются.
    assert_eq!(corner(-1.0, -1.0, -1.0, 1.0), BLUE);
    assert_eq!(corner(-1.0, -1.0, -1.0, -1.0), RED);
    // оба сразу
    assert_eq!(corner(1.0, -1.0, -1.0, 1.0), YELLOW);
    assert_eq!(corner(1.0, -1.0, 1.0, -1.0), RED);

    // Точка опоры: (0, 0) — объект в левом нижнем углу спрайта.
    let mut pivot_app = app_with_assets();
    add_camera(&mut pivot_app);
    pivot_app.spawn("Pivot").with(SpriteRenderer::colored(Color::rgb(1.0, 0.0, 0.0)).with_size(2.0, 2.0).with_pivot(0.0, 0.0));
    pivot_app.tick(DT);
    let image = capture(&pivot_app);
    assert_eq!(px(&image, 1.0, 1.0), RED);
    assert_eq!(px(&image, -1.0, -1.0), BLACK);
    // Спрайт без новых настроек рисуется по центру, как раньше.
    let mut plain = app_with_assets();
    add_camera(&mut plain);
    plain.spawn("Plain").with(SpriteRenderer::colored(Color::rgb(0.0, 1.0, 0.0)).with_size(2.0, 2.0));
    plain.tick(DT);
    let image = capture(&plain);
    assert_eq!(px(&image, -0.9, -0.9), GREEN);
    assert_eq!(px(&image, 0.9, 0.9), GREEN);
    assert_eq!(px(&image, 1.2, 0.0), BLACK);
}

#[test]
fn tiles_land_in_their_cells_and_two_maps_stack_by_layer() {
    let mut app = app_with_assets();
    add_camera(&mut app);
    let sheet = texture(&app, "sheet.png");
    // Карта 2×2 с левым нижним углом в (-1, -1): верхний ряд «1 2» → кадры 0 и 1.
    app.spawn("Ground").at(-1.0, -1.0).with(Tilemap::from_rows(&["1 2", "3 4"]).with_tileset(sheet, 4, 2));
    // Передний план рисуется поверх: один тайл 7 (белый) на месте правого верхнего.
    app.spawn("Front").at(0.0, 0.0).with(Tilemap::from_rows(&["7"]).with_tileset(sheet, 4, 2).with_layer(5, 0));
    app.tick(DT);
    let image = capture(&app);
    assert_eq!(px(&image, -0.5, 0.5), SHEET[0]);
    assert_eq!(px(&image, 0.5, 0.5), SHEET[6], "the foreground layer covers tile 2");
    assert_eq!(px(&image, -0.5, -0.5), SHEET[2]);
    assert_eq!(px(&image, 0.5, -0.5), SHEET[3]);
    assert_eq!(px(&image, -1.5, 0.0), BLACK);
    assert_eq!(px(&image, 1.5, 0.0), BLACK);
}

#[test]
fn a_huge_tilemap_submits_only_visible_tiles() {
    let mut app = app_with_assets();
    add_camera(&mut app);
    let sheet = texture(&app, "sheet.png");
    let rows: Vec<String> = (0..200).map(|_| "200*1".to_string()).collect();
    let mut map = Tilemap::default().with_tileset(sheet, 4, 2);
    map.rows = rows;
    // Карта 200×200 с центром в начале координат: видно около 4×4 тайлов.
    app.spawn("Huge").at(-100.0, -100.0).with(map);
    app.tick(DT);

    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let camera = CameraView::orthographic_2d(Vec2::ZERO, 2.0, 1.0, Color::BLACK);
    let (_, stats) = renderer.capture_view(&app.world, SIDE, SIDE, Some(&camera)).unwrap();
    assert!(stats.sprites >= 16 && stats.sprites <= 64, "{} tiles submitted of 40000", stats.sprites);
    assert_eq!(stats.draw_calls, 1, "the whole map is one batch");

    // Камера уехала за пределы карты — ничего не отправляется.
    let far = CameraView::orthographic_2d(Vec2::new(500.0, 0.0), 2.0, 1.0, Color::BLACK);
    assert_eq!(renderer.capture_view(&app.world, SIDE, SIDE, Some(&far)).unwrap().1.sprites, 0);
}

fn static_particles(blend: ParticleBlend, color: Color) -> ParticleSystem {
    ParticleSystem {
        emission_rate: 0.0,
        bursts: vec![Vec2::new(0.0, 1.0)],
        looping: false,
        lifetime: Vec2::new(100.0, 100.0),
        start_speed: Vec2::ZERO,
        start_size: Vec2::new(2.0, 2.0),
        start_color: color,
        start_color_max: color,
        color_over_lifetime: Vec::new(),
        particle_shape: ParticleShape::Square,
        blend,
        flat: true,
        seed: 1,
        ..ParticleSystem::default()
    }
}

#[test]
fn additive_particles_brighten_what_is_behind_them() {
    let shade = |blend| {
        let mut app = engine::app();
        app.spawn("Camera").with(Camera {
            clear_color: Color::rgb(0.2, 0.2, 0.2),
            ..Camera::orthographic(2.0)
        });
        app.spawn("Fx").with(static_particles(blend, Color::rgb(0.3, 0.3, 0.3)));
        app.run_headless(2, DT);
        let image = capture(&app);
        (px(&image, 0.0, 0.0)[0], px(&image, 1.9, 1.9)[0])
    };
    let (alpha, background) = shade(ParticleBlend::Alpha);
    let (additive, same_background) = shade(ParticleBlend::Additive);
    assert_eq!(background, same_background);
    // Линейные 0.2 → 0.3 (замещение) или 0.5 (сложение), на экране sRGB.
    assert!(alpha > background, "an opaque grey particle replaces the clear colour: {alpha} vs {background}");
    assert!(additive > alpha + 20, "additive adds light: {additive} vs {alpha}");
}

#[test]
fn particles_are_batched_not_drawn_one_by_one() {
    let mut app = engine::app();
    add_camera(&mut app);
    app.spawn("Sparks").with(ParticleSystem { emission_rate: 2000.0, max_particles: 5000, lifetime: Vec2::new(2.0, 2.0), ..ParticleSystem::sparks() }.with_seed(1));
    app.run_headless(16, DT);
    let alive = component::<ParticleSystem>(&app, app.world.scene.find_by_name("Sparks").unwrap()).alive_count();
    assert!(alive > 1000);
    let _gpu = gpu_lock();
    let (_, stats) = Renderer::headless()
        .expect("a GPU adapter")
        .capture_view(&app.world, SIDE, SIDE, None)
        .unwrap();
    assert!(stats.sprites as usize >= alive - 1, "{} quads for {alive} particles", stats.sprites);
    assert_eq!(stats.draw_calls, 1);
}

#[test]
fn particles_in_3d_are_billboards_facing_the_camera() {
    let shot = |position: Vec3, rotation: Quat| {
        let mut app = engine::app();
        app.spawn("Camera")
            .at3(position.x, position.y, position.z)
            .rotation(rotation)
            .with(Camera {
                clear_color: Color::BLACK,
                ..Camera::perspective(60.0)
            });
        app.spawn("Fx").with(ParticleSystem {
            flat: false,
            ..static_particles(ParticleBlend::Additive, Color::rgb(3.0, 3.0, 3.0))
        });
        app.run_headless(2, DT);
        capture(&app)
    };
    // Квад 2×2 на расстоянии 5: ширина ≈ высоте и спереди, и сбоку (поворот на 90°).
    let covered = |image: &RgbaImage| {
        let lit = |x: u32, y: u32| image.get_pixel(x, y)[0] > 40;
        let row = (0..SIDE).filter(|x| lit(*x, SIDE / 2)).count();
        let column = (0..SIDE).filter(|y| lit(SIDE / 2, *y)).count();
        (row, column)
    };
    let (w, h) = covered(&shot(Vec3::new(0.0, 0.0, 5.0), Quat::IDENTITY));
    assert!(w >= 12 && h >= 12, "visible from the front: {w}×{h}");
    assert!(w.abs_diff(h) <= 3, "square from the front: {w}×{h}");
    let (w, h) = covered(&shot(Vec3::new(5.0, 0.0, 0.0), Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)));
    assert!(w >= 12 && h >= 12, "a billboard does not vanish edge-on: {w}×{h}");
    assert!(w.abs_diff(h) <= 3, "square from the side: {w}×{h}");
}

#[test]
fn a_trail_follows_the_object_and_fades() {
    let mut app = engine::app();
    add_camera(&mut app);
    let id = app
        .spawn("Comet")
        .at(-1.8, 0.0)
        .with(TrailRenderer {
            lifetime: 2.0,
            start_width: 0.5,
            end_width: 0.5,
            start_color: Color::rgb(1.0, 0.0, 0.0),
            end_color: Color::rgb(1.0, 0.0, 0.0),
            min_vertex_distance: 0.05,
            ..TrailRenderer::default()
        })
        .id();
    for _ in 0..16 {
        app.world.scene.get_mut(id).unwrap().transform.position.x += 0.2;
        app.tick(DT);
    }
    let image = capture(&app);
    assert_eq!(px(&image, -1.0, 0.0), RED, "the ribbon covers the path");
    assert_eq!(px(&image, 0.0, 0.0), RED);
    assert_eq!(px(&image, 0.0, 0.8), BLACK, "and is only as wide as configured");
}

#[test]
fn camera_controller_follows_the_tagged_object_and_shakes() {
    let mut app = engine::app();
    let camera = app
        .spawn("Cam")
        .with(Camera::default())
        .with(CameraController2D::following("Player").with_offset(0.0, 0.0).with_smoothing(0.0))
        .id();
    let player = app.spawn("Player").tag("Player").at(3.0, 2.0).id();
    app.run_headless(2, DT);
    let at = app.world.scene.world_position(camera);
    assert!((at.truncate() - Vec2::new(3.0, 2.0)).length() < 1e-4);
    app.world.scene.get_mut(player).unwrap().transform.position.x = 6.0;
    app.world.send_message(camera, engine_render::SHAKE_MESSAGE, Value::Float(1.0));
    app.tick(DT);
    let moved = app.world.scene.world_position(camera).truncate();
    assert!((moved - Vec2::new(6.0, 2.0)).length() > 0.001, "shake displaces the camera");
    assert!((moved - Vec2::new(6.0, 2.0)).length() < 1.0);
    app.run_headless(80, DT);
    let rest = app.world.scene.world_position(camera).truncate();
    assert!((rest - Vec2::new(6.0, 2.0)).length() < 1e-4, "the shake settles and the camera rests on the target");
}
