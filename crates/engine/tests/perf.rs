//! Страховка производительности: проверяются не миллисекунды (они зависят от машины),
//! а структурные факты — сколько draw call'ов, что отсечено, сколько матриц
//! посчитано. Если оптимизация сломается, эти числа вырастут в разы.

use std::path::PathBuf;
use std::sync::OnceLock;

use engine::prelude::*;
use engine_render::{Overlay, RenderStats, Renderer};
use image::RgbaImage;

/// Тесты с GPU идут по одному: одновременная работа нескольких устройств Vulkan в
/// параллельных потоках на некоторых драйверах зависает.
fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn capture(app: &App, width: u32, height: u32) -> (RgbaImage, RenderStats) {
    let _gpu = gpu_lock();
    Renderer::headless()
        .expect("a GPU adapter")
        .capture_view(&app.world, width, height, None)
        .expect("capture")
}

const COLORS: [[u8; 4]; 4] = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]];

/// Четыре одноцветные текстуры: `c0.png` … `c3.png`.
fn assets_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("piska_perf_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (index, color) in COLORS.iter().enumerate() {
            let name = format!("c{index}.png");
            RgbaImage::from_pixel(4, 4, image::Rgba(*color)).save(dir.join(&name)).unwrap();
            std::fs::write(
                dir.join(format!("{name}.meta")),
                format!(
                    "(\n    guid: \"{:032x}\",\n    importer: \"Texture\",\n    settings: {{\n        \"filter\": Enum(\"Nearest\"),\n    }},\n)\n",
                    0xC0 + index
                ),
            )
            .unwrap();
        }
        dir
    })
}

fn textured_app() -> (App, Vec<AssetHandle<Texture>>) {
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets_dir()).expect("test assets");
    let database = app.world.resources.get::<AssetDatabase>().unwrap();
    let textures = (0..4).map(|i| database.handle(&format!("c{i}.png")).unwrap()).collect();
    (app, textures)
}

fn camera_2d(app: &mut App, half_height: f32) {
    app.spawn("Camera").with(Camera {
        clear_color: Color::BLACK,
        ..Camera::orthographic(half_height)
    });
}

#[test]
fn many_meshes_of_one_kind_take_one_draw_call() {
    let mut app = engine::app();
    app.spawn("Camera")
        .at3(0.0, 20.0, 40.0)
        .rotation(Quat::from_rotation_x(-0.45))
        .with(Camera::perspective(60.0));
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0))
        .with(Light::default());
    for i in 0..400 {
        let (x, z) = ((i % 20) as f32, (i / 20) as f32);
        app.spawn("Cube")
            .at3(x * 2.0 - 20.0, 0.5, -z * 2.0)
            .with(MeshRenderer::cube().with_color(Color::hex(0x4c8dff)));
    }
    app.run_headless(1, 1.0 / 60.0);
    let (_, stats) = capture(&app, 320, 180);
    assert_eq!(stats.meshes, 400, "all cubes are on screen");
    assert_eq!(stats.shadow_casters, 400);
    assert_eq!(stats.shadow_draw_calls, 1, "one instanced draw for the shadow map");
    // Небо + одна пачка кубов.
    assert_eq!(stats.draw_calls - stats.shadow_draw_calls, 2, "{stats:?}");
    assert_eq!(stats.triangles, 400 * 12);
}

#[test]
fn meshes_are_batched_by_kind_and_texture() {
    let (mut app, textures) = textured_app();
    app.spawn("Camera")
        .at3(0.0, 0.0, 30.0)
        .with(Camera::perspective(60.0));
    for i in 0..120 {
        let mesh = match i % 3 {
            0 => MeshRenderer::cube(),
            1 => MeshRenderer::sphere(),
            _ => MeshRenderer::cylinder(),
        };
        // Два вида текстур на каждый вид меша: 3 × 2 = 6 пачек.
        let mesh = mesh.with_texture(textures[i % 2], 0.0).without_shadows();
        app.spawn("Prop")
            .at3((i % 12) as f32 * 2.0 - 11.0, (i / 12) as f32 * 2.0 - 9.0, 0.0)
            .with(mesh);
    }
    app.run_headless(1, 1.0 / 60.0);
    let (_, stats) = capture(&app, 320, 180);
    assert_eq!(stats.meshes, 120);
    assert_eq!(stats.shadow_draw_calls, 0);
    // Небо + шесть пачек.
    assert_eq!(stats.draw_calls, 7, "{stats:?}");
}

#[test]
fn meshes_outside_the_camera_are_not_drawn() {
    let mut app = engine::app();
    app.spawn("Camera").at3(0.0, 0.0, 10.0).with(Camera::perspective(60.0));
    app.spawn("Visible").with(MeshRenderer::sphere().without_shadows());
    app.spawn("Behind").at3(0.0, 0.0, 30.0).with(MeshRenderer::cube().without_shadows());
    app.spawn("FarLeft").at3(-200.0, 0.0, 0.0).with(MeshRenderer::cube().without_shadows());
    app.spawn("BeyondFarPlane").at3(0.0, 0.0, -5000.0).with(MeshRenderer::cube().without_shadows());
    app.run_headless(1, 1.0 / 60.0);
    let (image, stats) = capture(&app, 64, 64);
    assert_eq!(stats.meshes, 1);
    assert_eq!(stats.culled_meshes, 3);
    // Видимая сфера по-прежнему нарисована в центре кадра.
    let center = image.get_pixel(32, 32).0;
    let corner = image.get_pixel(1, 1).0;
    assert_ne!(center, corner, "the sphere is drawn");
}

#[test]
fn shadow_pass_skips_casters_outside_the_sun_volume() {
    let mut app = engine::app();
    app.spawn("Camera").at3(0.0, 5.0, 12.0).with(Camera::perspective(60.0));
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.3, -1.2, 0.0))
        .with(Light::default());
    // Тени — только в 20 м вокруг камеры.
    app.spawn("Environment").with(Environment {
        shadow_distance: 20.0,
        ..Environment::default()
    });
    app.spawn("Near").with(MeshRenderer::cube());
    for i in 0..50 {
        app.spawn("FarAway")
            .at3(500.0 + i as f32 * 3.0, 0.5, -500.0)
            .with(MeshRenderer::cube());
    }
    app.run_headless(1, 1.0 / 60.0);
    let (_, stats) = capture(&app, 64, 64);
    assert_eq!(stats.shadow_casters, 1, "{stats:?}");
    assert_eq!(stats.culled_meshes, 50);
}

#[test]
fn sprites_outside_the_camera_are_not_drawn() {
    let mut app = engine::app();
    camera_2d(&mut app, 5.0);
    for i in 0..100 {
        app.spawn("OnScreen")
            .at((i % 10) as f32 * 0.8 - 4.0, (i / 10) as f32 * 0.8 - 4.0)
            .with(SpriteRenderer::colored(Color::WHITE).with_size(0.5, 0.5));
    }
    for i in 0..1000 {
        app.spawn("OffScreen")
            .at(100.0 + i as f32, 0.0)
            .with(SpriteRenderer::colored(Color::WHITE));
    }
    app.run_headless(1, 1.0 / 60.0);
    let (_, stats) = capture(&app, 64, 64);
    assert_eq!(stats.sprites, 100);
    assert_eq!(stats.culled_sprites, 1000);
    assert_eq!(stats.draw_calls, 1);
}

#[test]
fn interleaved_textures_on_disjoint_sprites_batch_per_texture() {
    let (mut app, textures) = textured_app();
    camera_2d(&mut app, 10.0);
    // 400 спрайтов, текстуры чередуются A B C D A B …, спрайты не пересекаются.
    for i in 0..400 {
        app.spawn("Sprite")
            .at((i % 20) as f32 - 9.5, (i / 20) as f32 - 9.5)
            .with(SpriteRenderer::textured(textures[i % 4]).with_size(0.8, 0.8));
    }
    app.run_headless(1, 1.0 / 60.0);
    let (image, stats) = capture(&app, 200, 200);
    assert_eq!(stats.sprites, 400);
    assert_eq!(stats.draw_calls, 4, "one draw call per texture, not per sprite");
    // Картинка та же: первый спрайт (левый нижний) красный, второй — зелёный.
    assert_eq!(image.get_pixel(5, 195).0, COLORS[0]);
    assert_eq!(image.get_pixel(15, 195).0, COLORS[1]);
}

#[test]
fn overlapping_sprites_keep_hierarchy_order_across_textures() {
    let (mut app, textures) = textured_app();
    camera_2d(&mut app, 2.0);
    // Три спрайта в одной точке: красный, зелёный, снова красный (меньше). Порядок
    // иерархии решает, кто сверху: сверху маленький красный, под ним зелёный.
    app.spawn("A").with(SpriteRenderer::textured(textures[0]).with_size(3.0, 3.0));
    app.spawn("B").with(SpriteRenderer::textured(textures[1]).with_size(2.0, 2.0));
    app.spawn("C").with(SpriteRenderer::textured(textures[0]).with_size(1.0, 1.0));
    app.run_headless(1, 1.0 / 60.0);
    let (image, stats) = capture(&app, 64, 64);
    assert_eq!(stats.draw_calls, 3, "overlapping sprites cannot be merged");
    assert_eq!(image.get_pixel(32, 32).0, COLORS[0], "small red on top");
    assert_eq!(image.get_pixel(32, 20).0, COLORS[1], "green ring");
    assert_eq!(image.get_pixel(32, 10).0, COLORS[0], "big red at the edge");
}

#[test]
fn big_tilemap_submits_only_visible_tiles() {
    let mut app = engine::app();
    app.spawn("Camera")
        .at(100.0, 100.0)
        .with(Camera {
            clear_color: Color::BLACK,
            ..Camera::orthographic(5.0)
        });
    let row = vec!["1"; 200].join(" ");
    let rows: Vec<&str> = (0..200).map(|_| row.as_str()).collect();
    app.spawn("Map").with(Tilemap::from_rows(&rows));
    app.run_headless(1, 1.0 / 60.0);
    let (_, stats) = capture(&app, 160, 90);
    // Видно ~18 × 10 тайлов (+ запас в тайл по краям), а не 40 000.
    assert!(stats.tiles > 100 && stats.tiles < 400, "{stats:?}");
    assert_eq!(stats.sprites, stats.tiles);
    assert_eq!(stats.draw_calls, 1);
}

#[test]
fn deep_hierarchy_computes_each_world_matrix_once() {
    let mut app = engine::app();
    camera_2d(&mut app, 5.0);
    let mut parent = app.spawn("Root").id();
    for _ in 0..50 {
        parent = app
            .spawn("Link")
            .at(0.01, 0.0)
            .with(SpriteRenderer::colored(Color::WHITE).with_size(0.1, 0.1))
            .child_of(parent)
            .id();
    }
    // Объекты без отрисовки матриц не требуют вовсе.
    for _ in 0..500 {
        app.spawn("Logic");
    }
    app.run_headless(1, 1.0 / 60.0);
    let (_, stats) = capture(&app, 64, 64);
    assert_eq!(stats.sprites, 50);
    // Корень + 50 звеньев: по матрице на объект, а не 1 + 2 + … + 51 = 1326.
    assert_eq!(stats.world_matrices, 51, "{stats:?}");
}

#[test]
fn neighbouring_particle_groups_with_one_texture_share_a_draw_call() {
    let mut app = engine::app();
    camera_2d(&mut app, 10.0);
    // Дым (order 5, Alpha), огонь (8, Additive) и искры (10, Additive) — как в `stress`:
    // огонь и искры с одной текстурой и смешиванием идут подряд — один draw call.
    for (i, system) in [ParticleSystem::smoke(), ParticleSystem::fire(), ParticleSystem::sparks()]
        .into_iter()
        .enumerate()
    {
        app.spawn("Particles").at(i as f32 * 3.0 - 3.0, 0.0).with(system.with_seed(i as i32 + 1));
    }
    app.run_headless(30, 1.0 / 60.0);
    let (_, stats) = capture(&app, 64, 64);
    assert!(stats.sprites > 0);
    assert_eq!(stats.draw_calls, 2, "{stats:?}");
}

#[test]
fn engine_data_components_declare_no_hooks() {
    // Если объявление пропадёт, такие компоненты снова будут стоить по вызову на
    // каждый хук каждого объекта в каждом кадре.
    assert_eq!(SpriteRenderer::default().hooks(), Hooks::NONE);
    assert_eq!(MeshRenderer::default().hooks(), Hooks::NONE);
    assert_eq!(Camera::default().hooks(), Hooks::NONE);
    assert_eq!(Light::default().hooks(), Hooks::NONE);
    assert_eq!(Collider2D::default().hooks(), Hooks::NONE);
    assert_eq!(Rigidbody2D::default().hooks(), Hooks::NONE);
    assert_eq!(Collider::default().hooks(), Hooks::NONE);
    assert_eq!(Rigidbody::default().hooks(), Hooks::NONE);
    assert_eq!(AudioSource::default().hooks(), Hooks::NONE);
    assert_eq!(Text::default().hooks(), Hooks::NONE);
    assert_eq!(Canvas::default().hooks(), Hooks::NONE);
    assert_eq!(ParticleSystem::default().hooks(), Hooks::UPDATE);
}

component! {
    /// Считает свои `on_update` в ресурсе `UpdateCount`.
    pub struct Counted {}
}
#[derive(Default)]
struct UpdateCount(u32);
impl Component for Counted {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        ctx.resource_or_default::<UpdateCount>().0 += 1;
    }
}

#[test]
fn a_big_scene_of_data_objects_finds_its_few_scripts_and_widgets() {
    let mut app = engine::app();
    camera_2d(&mut app, 10.0);
    for i in 0..20_000 {
        app.spawn("Data")
            .at((i % 200) as f32 * 0.1, (i / 200) as f32 * 0.1)
            .with(SpriteRenderer::colored(Color::WHITE));
    }
    for _ in 0..3 {
        app.spawn("Script").with(Counted {});
    }
    let canvas = app.world.scene.ui_canvas().id();
    app.world.scene.ui_text(canvas, "HUD");
    app.run_headless(2, 1.0 / 60.0);
    // Индекс видит ровно «свои» объекты.
    assert_eq!(app.world.scene.count_with::<Counted>(), 3);
    assert_eq!(app.world.scene.count_with::<Canvas>(), 1);
    assert_eq!(app.world.scene.count_with::<AudioSource>(), 0);
    assert_eq!(app.world.scene.count_with::<SpriteRenderer>(), 20_000);
    assert_eq!(app.world.resources.get::<UpdateCount>().unwrap().0, 6);
    // Интерфейс нашёл холст через индекс и нарисовал текст.
    assert!(!app.world.resources.get::<Overlay>().unwrap().is_empty());
}

#[test]
fn ui_overlay_is_drawn_above_world_sprites() {
    let mut app = engine::app();
    // Экран — как картинка (интерфейс раскладывается в пикселях экрана).
    app.world.input.set_screen_size(Vec2::new(128.0, 128.0));
    camera_2d(&mut app, 5.0);
    // Яркий спрайт на весь экран, включая место панели и надписи.
    app.spawn("Star")
        .with(SpriteRenderer::colored(Color::rgb(4.0, 4.0, 0.0)).with_size(40.0, 40.0).with_order(0, 100));
    let canvas = app.world.scene.ui_canvas().id();
    app.world
        .scene
        .ui_panel(canvas)
        .anchor(Anchor::TopLeft)
        .pos(0.0, 0.0)
        // Холст масштабируется от эталонного разрешения: 400 единиц — треть экрана.
        .size(400.0, 400.0)
        .color(Color::rgb(0.0, 0.0, 1.0))
        .radius(0.0);
    app.run_headless(2, 1.0 / 60.0);
    let (image, _) = capture(&app, 128, 128);
    // Панель — синяя поверх жёлтого спрайта.
    let panel = image.get_pixel(10, 10).0;
    assert!(panel[2] > 200 && panel[0] < 40 && panel[1] < 40, "panel pixel {panel:?}");
    let world = image.get_pixel(100, 100).0;
    assert!(world[0] > 200 && world[1] > 200 && world[2] < 60, "sprite pixel {world:?}");
}
