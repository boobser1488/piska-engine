//! Свои шейдеры (`.shader`, docs/api/shaders.md) на GPU: каждый вид рисует то, что говорит
//! его функция; параметры из файла материала и из скрипта; ошибка — строка пользователя и
//! стандартный материал вместо пурпурного; горячая перезагрузка; сдвиг вершин и вырез в
//! тенях; пачки спрайтов; порядок эффектов; без своих шейдеров — кадр тот же.
//!
//! Один рендерер (одно устройство GPU) на процесс, все тесты — через один замок.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use engine::prelude::*;
use engine_render::{CameraView, PostSettings, Renderer};
use image::RgbaImage;

const W: u32 = 320;
const H: u32 = 180;

/// Рендерер процесса под замком (тесты идут по одному).
fn renderer() -> MutexGuard<'static, Renderer> {
    static RENDERER: OnceLock<Mutex<Renderer>> = OnceLock::new();
    RENDERER
        .get_or_init(|| Mutex::new(Renderer::headless().expect("GPU")))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Каталог ассетов теста с файлами `files` (путь → текст).
fn assets_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_shaders_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn app_with(dir: &Path) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    engine::mount_assets(&mut app, dir).expect("assets open");
    app
}

fn db(app: &App) -> AssetDatabase {
    app.world.resources.get::<AssetDatabase>().unwrap().clone()
}

fn add_material(app: &mut App, material: Material) -> AssetHandle<Material> {
    app.world.resources.get_or_insert_with(Materials::default).add(material)
}

/// Вид сверху вниз (ортографический): мир x → вправо, мир z → вниз по кадру. Без
/// тонмаппинга и тумана — числа в кадре те же, что вернул шейдер (в sRGB).
fn top_view() -> CameraView {
    let projection = engine_render::orthographic(-8.0, 8.0, -4.5, 4.5, 0.1, 100.0);
    let view = Mat4::look_at_rh(Vec3::new(0.0, 30.0, 0.0), Vec3::ZERO, Vec3::NEG_Z);
    CameraView {
        view_proj: projection * view,
        position: Vec3::new(0.0, 30.0, 0.0),
        clear_color: Color::rgb(0.0, 0.0, 0.0),
        sky: false,
        post: Some(PostSettings::OFF),
        culling_mask: u32::MAX,
        overlay: false,
        grid: None,
        fog: false,
    }
}

/// Пиксель кадра, в который попадает мировая точка на земле (x, z) вида `top_view`.
fn ground_pixel(image: &RgbaImage, x: f32, z: f32) -> [u8; 4] {
    let px = ((x / 8.0 * 0.5 + 0.5) * W as f32) as u32;
    let py = ((z / 4.5 * 0.5 + 0.5) * H as f32) as u32;
    image.get_pixel(px.min(W - 1), py.min(H - 1)).0
}

fn luma(p: [u8; 4]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn near(p: [u8; 4], q: [u8; 3], tolerance: u8) -> bool {
    p[0].abs_diff(q[0]) <= tolerance && p[1].abs_diff(q[1]) <= tolerance && p[2].abs_diff(q[2]) <= tolerance
}

fn capture(renderer: &mut Renderer, app: &App, view: &CameraView) -> (RgbaImage, RenderStats) {
    renderer.capture_view(&app.world, W, H, Some(view)).expect("capture")
}

/// Цвет как есть: unlit-шейдер, возвращающий параметр.
const FLAT: &str = "//! kind: unlit
//! param color: color = (0.0, 1.0, 0.0, 1.0)
fn unlit(in: SurfaceIn) -> vec4<f32> {
    return vec4<f32>(params.color.rgb, 1.0);
}
";

/// Солнце под 45° (лучи идут вправо и вниз): тень предмета на высоте h — на h правее.
fn sun_scene(app: &mut App) {
    let dir = Vec3::new(1.0, -1.0, 0.0).normalize();
    app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, dir)).with(Light::default());
    app.spawn("Ground").scale(40.0, 1.0, 40.0).with(MeshRenderer::plane().with_color(Color::rgb(0.8, 0.8, 0.8)));
}

#[test]
fn unlit_and_material_file_params_draw_what_the_function_returns() {
    // Материал-файл с шейдером и значением параметра (красный), второй — из кода без значения.
    let dir = assets_dir("unlit", &[("flat.shader", FLAT)]);
    let guid = engine_assets::AssetDatabase::open(&dir).unwrap().guid_of("flat.shader").unwrap();
    std::fs::write(
        dir.join("red.material"),
        format!("(\n    shader: \"{guid}\",\n    shader_params: {{\"color\": Color(1.0, 0.0, 0.0, 1.0)}},\n)\n"),
    )
    .unwrap();
    let mut app = app_with(&dir);
    let db = db(&app);
    let red = db.handle::<Material>("red.material").unwrap();
    let shader = db.handle::<Shader>("flat.shader").unwrap();
    let green = add_material(&mut app, Material::new().shader(shader));
    app.spawn("Red").at3(-3.0, 0.0, 0.0).scale(3.0, 1.0, 3.0).with(MeshRenderer::plane().with_material_asset(red));
    app.spawn("Green").at3(3.0, 0.0, 0.0).scale(3.0, 1.0, 3.0).with(MeshRenderer::plane().with_material_asset(green));
    app.tick(1.0 / 60.0);
    let mut renderer = renderer();
    let (image, _) = capture(&mut renderer, &app, &top_view());
    assert!(near(ground_pixel(&image, -3.0, 0.0), [255, 0, 0], 2), "{:?}", ground_pixel(&image, -3.0, 0.0));
    assert!(near(ground_pixel(&image, 3.0, 0.0), [0, 255, 0], 2), "{:?}", ground_pixel(&image, 3.0, 0.0));

    // Из скрипта: значение в материале (у всех) и только у одного объекта (`MaterialParams`).
    app.world.resources.get_mut::<Materials>().unwrap().get_mut(green).unwrap().shader_params.set("color", Color::rgb(0.0, 0.0, 1.0));
    let (image, _) = capture(&mut renderer, &app, &top_view());
    assert!(near(ground_pixel(&image, 3.0, 0.0), [0, 0, 255], 2), "{:?}", ground_pixel(&image, 3.0, 0.0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn per_object_params_split_only_that_object_from_the_batch() {
    let dir = assets_dir("override", &[("flat.shader", FLAT)]);
    let mut app = app_with(&dir);
    let shader = db(&app).handle::<Shader>("flat.shader").unwrap();
    let material = add_material(&mut app, Material::new().shader(shader).shadows(false, false));
    let ids: Vec<_> = (0..4)
        .map(|i| app.spawn("Tile").at3(-6.0 + i as f32 * 4.0, 0.0, 0.0).scale(3.0, 1.0, 3.0).with(MeshRenderer::plane().with_material_asset(material)).id())
        .collect();
    app.tick(1.0 / 60.0);
    let mut renderer = renderer();
    let (_, before) = capture(&mut renderer, &app, &top_view());
    assert_eq!(before.draw_calls - before.shadow_draw_calls, 1, "four tiles with one material: one instanced draw");
    app.world.scene.get_mut(ids[2]).unwrap().add(MaterialParams::new().with("color", Color::rgb(1.0, 1.0, 0.0)));
    let (image, after) = capture(&mut renderer, &app, &top_view());
    assert_eq!(after.draw_calls - after.shadow_draw_calls, 2, "only the overridden tile leaves the batch");
    assert!(near(ground_pixel(&image, 2.0, 0.0), [255, 255, 0], 2), "{:?}", ground_pixel(&image, 2.0, 0.0));
    assert!(near(ground_pixel(&image, -2.0, 0.0), [0, 255, 0], 2), "{:?}", ground_pixel(&image, -2.0, 0.0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `default_surface(in)` без изменений рисует как стандартный материал (свет, тени, туман).
#[test]
fn default_surface_matches_the_standard_material() {
    let passthrough = "//! kind: surface\nfn surface(in: SurfaceIn) -> Surface {\n    return default_surface(in);\n}\n";
    let dir = assets_dir("passthrough", &[("pass.shader", passthrough)]);
    let build = |custom: bool| {
        let mut app = app_with(&dir);
        let shader = db(&app).handle::<Shader>("pass.shader").unwrap();
        sun_scene(&mut app);
        app.spawn("Point").at3(-2.0, 2.0, 2.0).with(Light::point(Color::rgb(1.0, 0.5, 0.2), 4.0, 8.0));
        let mut material = Material::new().color(Color::rgb(0.3, 0.6, 0.9)).metallic(0.4).roughness(0.35).emissive(Color::rgb(0.1, 0.0, 0.0), 1.0);
        if custom {
            material = material.shader(shader);
        }
        let handle = add_material(&mut app, material);
        app.spawn("Ball").at3(0.0, 1.5, 0.0).scale(2.0, 2.0, 2.0).with(MeshRenderer::sphere().with_material_asset(handle));
        app.spawn("Box").at3(-4.0, 1.0, 1.0).with(MeshRenderer::cube().with_material_asset(handle));
        app.tick(1.0 / 60.0);
        app
    };
    let view = CameraView::look_at(Vec3::new(0.0, 6.0, 9.0), Vec3::new(0.0, 1.0, 0.0), 50.0, W as f32 / H as f32, Color::rgb(0.1, 0.1, 0.12), true);
    let mut renderer = renderer();
    let (standard, _) = capture(&mut renderer, &build(false), &view);
    let (custom, _) = capture(&mut renderer, &build(true), &view);
    let mut worst = 0u8;
    let mut differing = 0;
    for (a, b) in standard.pixels().zip(custom.pixels()) {
        let d = a.0.iter().zip(&b.0).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        worst = worst.max(d);
        differing += usize::from(d > 2);
    }
    assert!(differing * 1000 <= (W * H) as usize, "{differing} pixels differ (worst {worst})");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn errors_name_the_users_line_and_fall_back_to_the_standard_material() {
    let broken = "//! kind: unlit\n//! param amount: float = 1.0\nfn unlit(in: SurfaceIn) -> vec4<f32> {\n    let a = params.amout;\n    return vec4<f32>(a);\n}\n";
    let found = engine_render::shaders::check(broken);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].line, 4);
    assert!(found[0].notes.iter().any(|n| n.contains("`amount`")), "{:?}", found[0]);
    let syntax = "//! kind: unlit\nfn unlit(in: SurfaceIn) -> vec4<f32> {\n    return vec4<f32>(1.0)\n}\n";
    assert_eq!(engine_render::shaders::check(syntax)[0].line, 3);

    let dir = assets_dir("broken", &[("broken.shader", broken)]);
    let mut app = app_with(&dir);
    let shader = db(&app).handle::<Shader>("broken.shader").unwrap();
    let material = add_material(&mut app, Material::new().shader(shader).color(Color::rgb(0.0, 0.0, 1.0)).unlit(true));
    app.spawn("Tile").scale(4.0, 1.0, 4.0).with(MeshRenderer::plane().with_material_asset(material));
    app.tick(1.0 / 60.0);
    let mut renderer = renderer();
    let (image, _) = capture(&mut renderer, &app, &top_view());
    // Стандартный материал (синий, без освещения), а не пурпурный «нет материала».
    assert!(near(ground_pixel(&image, 0.0, 0.0), [0, 0, 255], 2), "{:?}", ground_pixel(&image, 0.0, 0.0));
    assert_eq!(renderer.shader_revision(shader.guid()), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saving_the_file_reloads_the_shader_and_a_broken_save_keeps_the_last_good_one() {
    let dir = assets_dir("reload", &[("live.shader", FLAT)]);
    let mut app = app_with(&dir);
    let shader = db(&app).handle::<Shader>("live.shader").unwrap();
    let material = add_material(&mut app, Material::new().shader(shader));
    app.spawn("Tile").scale(4.0, 1.0, 4.0).with(MeshRenderer::plane().with_material_asset(material));
    app.tick(1.0 / 60.0);
    let mut renderer = renderer();
    let (image, _) = capture(&mut renderer, &app, &top_view());
    assert!(near(ground_pixel(&image, 0.0, 0.0), [0, 255, 0], 2));
    let first = renderer.shader_revision(shader.guid()).unwrap();

    let path = dir.join("live.shader");
    std::fs::write(&path, FLAT.replace("(0.0, 1.0, 0.0, 1.0)", "(1.0, 0.0, 1.0, 1.0)")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(350));
    let (image, _) = capture(&mut renderer, &app, &top_view());
    assert!(near(ground_pixel(&image, 0.0, 0.0), [255, 0, 255], 2), "{:?}", ground_pixel(&image, 0.0, 0.0));
    assert!(renderer.shader_revision(shader.guid()).unwrap() > first);

    std::fs::write(&path, "//! kind: unlit\nfn unlit(in: SurfaceIn) -> vec4<f32> { return oops; }\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(350));
    let (image, _) = capture(&mut renderer, &app, &top_view());
    assert!(near(ground_pixel(&image, 0.0, 0.0), [255, 0, 255], 2), "the last good version stays: {:?}", ground_pixel(&image, 0.0, 0.0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Сдвиг вершин (`vertex`) переносит и тень; `discard` прорезает тень.
#[test]
fn vertex_displacement_and_discard_reach_the_shadow_pass() {
    let shift = "//! kind: surface\nfn vertex(in: VertexIn) -> VertexOut {\n    var o = default_vertex(in);\n    o.world_position.z += 3.0;\n    return o;\n}\nfn surface(in: SurfaceIn) -> Surface {\n    return default_surface(in);\n}\n";
    let half = "//! kind: surface\nfn surface(in: SurfaceIn) -> Surface {\n    if (in.uv.x < 0.5) {\n        discard;\n    }\n    return default_surface(in);\n}\n";
    let dir = assets_dir("shadow", &[("shift.shader", shift), ("half.shader", half)]);
    let roof = |shader: Option<&str>| {
        let mut app = app_with(&dir);
        let db = db(&app);
        sun_scene(&mut app);
        let mut material = Material::new().double_sided(true);
        if let Some(path) = shader {
            material = material.shader(db.handle::<Shader>(path).unwrap());
        }
        let handle = add_material(&mut app, material);
        // Крыша 2 × 2 на высоте 3: её тень — на 3 м правее (x от 2 до 4).
        app.spawn("Roof").at3(0.0, 3.0, -2.0).scale(2.0, 1.0, 2.0).with(MeshRenderer::plane().with_material_asset(handle));
        app.tick(1.0 / 60.0);
        app
    };
    let mut renderer = renderer();
    let (plain, _) = capture(&mut renderer, &roof(None), &top_view());
    let (shifted, _) = capture(&mut renderer, &roof(Some("shift.shader")), &top_view());
    let (cut, _) = capture(&mut renderer, &roof(Some("half.shader")), &top_view());
    let lit = luma(ground_pixel(&plain, 3.0, 2.5));
    let shadow = luma(ground_pixel(&plain, 3.0, -2.0));
    assert!(lit > shadow * 1.5, "the plain roof casts a shadow: lit {lit}, shadow {shadow}");
    // Сдвинутая на 3 м по z крыша: тень ушла туда же.
    assert!(luma(ground_pixel(&shifted, 3.0, -2.0)) > shadow * 1.4, "the old shadow place is lit");
    assert!(luma(ground_pixel(&shifted, 3.0, 1.0)) < lit * 0.7, "the shadow moved with the vertices");
    // Половина крыши прорезана: тени — примерно вдвое меньше.
    let dark = |image: &RgbaImage| {
        let mut count = 0;
        for i in 0..7 {
            for j in 0..7 {
                let (x, z) = (2.15 + i as f32 * 0.28, -2.85 + j as f32 * 0.28);
                count += usize::from(luma(ground_pixel(image, x, z)) < (lit + shadow) * 0.5);
            }
        }
        count
    };
    let (whole, cut_half) = (dark(&plain), dark(&cut));
    assert!(whole >= 40, "the plain roof shadows its footprint: {whole} of 49");
    assert!(cut_half * 4 >= whole && cut_half * 4 <= whole * 3, "the cut roof casts about half: {cut_half} of {whole}");
    let _ = std::fs::remove_dir_all(&dir);
}

const RED_FLASH: &str = "//! kind: sprite
//! param flash: float = 1.0 [0, 1]
//! param flash_color: color = (1.0, 0.0, 0.0, 1.0)
fn sprite(in: SpriteIn) -> vec4<f32> {
    let base = default_sprite(in);
    return vec4<f32>(mix(base.rgb, params.flash_color.rgb, params.flash), base.a);
}
";

#[test]
fn sprite_shaders_draw_and_keep_batches() {
    let dir = assets_dir("sprites", &[("flash.shader", RED_FLASH)]);
    let mut app = app_with(&dir);
    let shader = db(&app).handle::<Shader>("flash.shader").unwrap();
    app.spawn("Camera").at3(0.0, 0.0, 10.0).with(Camera::default());
    let mut ids = Vec::new();
    for i in 0..10 {
        let id = app
            .spawn("Sprite")
            .at(-4.5 + i as f32, 0.0)
            .with(SpriteRenderer::colored(Color::WHITE))
            .with(SpriteShader::new(shader))
            .id();
        ids.push(id);
    }
    app.tick(1.0 / 60.0);
    let view = CameraView::orthographic_2d(Vec2::ZERO, 3.0, W as f32 / H as f32, Color::rgb(0.0, 0.0, 0.0));
    let mut renderer = renderer();
    let (image, stats) = renderer.capture_view(&app.world, W, H, Some(&view)).unwrap();
    assert_eq!(stats.draw_calls, 1, "ten sprites, one shader and one set of values: one batch");
    let center = image.get_pixel(W / 2 + (0.5 / 3.0 * H as f32 * 0.5) as u32, H / 2).0;
    assert!(near(center, [255, 0, 0], 2), "{center:?}");
    app.world.scene.get_mut(ids[3]).unwrap().get_mut::<SpriteShader>().unwrap().set("flash", 0.0);
    let (_, stats) = renderer.capture_view(&app.world, W, H, Some(&view)).unwrap();
    assert!(stats.draw_calls <= 3, "a sprite with its own values gets its own batch: {}", stats.draw_calls);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn post_effects_run_in_order_after_tone_mapping_or_before_it() {
    let half = "//! kind: post\nfn post(in: PostIn) -> vec4<f32> {\n    return vec4<f32>(in.color.rgb * 0.5, 1.0);\n}\n";
    let add = "//! kind: post\nfn post(in: PostIn) -> vec4<f32> {\n    return vec4<f32>(in.color.rgb + vec3<f32>(0.25), 1.0);\n}\n";
    let dir = assets_dir("post", &[("half.shader", half), ("add.shader", add)]);
    let db_handles = |app: &App| {
        let db = db(app);
        (db.handle::<Shader>("half.shader").unwrap(), db.handle::<Shader>("add.shader").unwrap())
    };
    let srgb = |linear: f32| (if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 } * 255.0).round() as u8;
    let mut renderer = renderer();
    for (first_half, expected) in [(true, 0.2 * 0.5 + 0.25), (false, (0.2 + 0.25) * 0.5)] {
        let mut app = app_with(&dir);
        let (half, add) = db_handles(&app);
        let (a, b) = if first_half { (0, 1) } else { (1, 0) };
        app.spawn("Camera")
            .at3(0.0, 0.0, 10.0)
            .with(Camera { clear_color: Color::rgb(0.2, 0.2, 0.2), ..Camera::default() })
            .with(PostEffect::new(half).order(a))
            .with(PostEffect::new(add).order(b));
        app.tick(1.0 / 60.0);
        let image = renderer.capture(&app.world, W, H).unwrap();
        let pixel = image.get_pixel(W / 2, H / 2).0;
        let want = srgb(expected);
        assert!(pixel[0].abs_diff(want) <= 2, "order {first_half}: {pixel:?}, want {want}");
    }
    // До тонмаппинга: эффект в HDR-кадре.
    let mut app = app_with(&dir);
    let (half, _) = db_handles(&app);
    app.spawn("Camera")
        .at3(0.0, 0.0, 10.0)
        .with(Camera { clear_color: Color::rgb(0.2, 0.2, 0.2), ..Camera::default() })
        .with(PostEffect { before_tonemap: true, ..PostEffect::new(half) });
    app.tick(1.0 / 60.0);
    let pixel = renderer.capture(&app.world, W, H).unwrap().get_pixel(W / 2, H / 2).0;
    assert!(pixel[0].abs_diff(srgb(0.1)) <= 2, "{pixel:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Без своих шейдеров кадр тот же — и после кадров, где они были (ничего не «прилипает»).
#[test]
fn without_custom_shaders_the_frame_is_unchanged() {
    let dir = assets_dir("plain", &[("flat.shader", FLAT)]);
    let scene = |custom: bool| {
        let mut app = app_with(&dir);
        sun_scene(&mut app);
        app.spawn("Box").at3(-2.0, 1.0, 0.0).with(MeshRenderer::cube().with_color(Color::rgb(0.8, 0.3, 0.2)));
        if custom {
            let shader = db(&app).handle::<Shader>("flat.shader").unwrap();
            let material = add_material(&mut app, Material::new().shader(shader));
            app.spawn("Custom").at3(3.0, 1.0, 0.0).with(MeshRenderer::cube().with_material_asset(material));
        }
        app.tick(1.0 / 60.0);
        app
    };
    let view = CameraView::look_at(Vec3::new(0.0, 5.0, 8.0), Vec3::ZERO, 50.0, W as f32 / H as f32, Color::rgb(0.1, 0.1, 0.12), true);
    let mut renderer = renderer();
    let plain = scene(false);
    let (before, stats_before) = capture(&mut renderer, &plain, &view);
    let _ = capture(&mut renderer, &scene(true), &view);
    let (after, stats_after) = capture(&mut renderer, &plain, &view);
    assert_eq!(before.as_raw(), after.as_raw());
    assert_eq!(stats_before.draw_calls, stats_after.draw_calls);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Надпись мелким растровым шрифтом 3×5 (буквы имён примеров, `_`, цифры) — подпись плитки.
fn label(image: &mut RgbaImage, x: u32, y: u32, text: &str) {
    fn glyph(c: char) -> [&'static str; 5] {
        match c {
            'a' => [".#.", "#.#", "###", "#.#", "#.#"],
            'c' => [".##", "#..", "#..", "#..", ".##"],
            'd' => ["##.", "#.#", "#.#", "#.#", "##."],
            'e' => ["###", "#..", "##.", "#..", "###"],
            'f' => ["###", "#..", "##.", "#..", "#.."],
            'g' => [".##", "#..", "#.#", "#.#", ".##"],
            'h' => ["#.#", "#.#", "###", "#.#", "#.#"],
            'i' => ["###", ".#.", ".#.", ".#.", "###"],
            'l' => ["#..", "#..", "#..", "#..", "###"],
            'm' => ["#.#", "###", "###", "#.#", "#.#"],
            'n' => ["##.", "#.#", "#.#", "#.#", "#.#"],
            'o' => [".#.", "#.#", "#.#", "#.#", ".#."],
            'p' => ["##.", "#.#", "##.", "#..", "#.."],
            'r' => ["##.", "#.#", "##.", "#.#", "#.#"],
            's' => [".##", "#..", ".#.", "..#", "##."],
            't' => ["###", ".#.", ".#.", ".#.", ".#."],
            'u' => ["#.#", "#.#", "#.#", "#.#", "###"],
            'v' => ["#.#", "#.#", "#.#", "#.#", ".#."],
            'w' => ["#.#", "#.#", "###", "###", "#.#"],
            'x' => ["#.#", "#.#", ".#.", "#.#", "#.#"],
            'y' => ["#.#", "#.#", ".#.", ".#.", ".#."],
            '_' => ["...", "...", "...", "...", "###"],
            '1' => [".#.", "##.", ".#.", ".#.", "###"],
            '2' => ["##.", "..#", ".#.", "#..", "###"],
            _ => ["...", "...", "...", "...", "..."],
        }
    }
    const SCALE: u32 = 2;
    let width = text.chars().count() as u32 * 4 * SCALE + 2 * SCALE;
    for py in y..(y + 7 * SCALE).min(image.height()) {
        for px in x..(x + width).min(image.width()) {
            image.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
        }
    }
    for (k, c) in text.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for (col, bit) in bits.chars().enumerate() {
                if bit != '#' {
                    continue;
                }
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        let px = x + SCALE + (k as u32 * 4 + col as u32) * SCALE + dx;
                        let py = y + SCALE + row as u32 * SCALE + dy;
                        if px < image.width() && py < image.height() {
                            image.put_pixel(px, py, image::Rgba([235, 235, 235, 255]));
                        }
                    }
                }
            }
        }
    }
}

/// Тёмная сцена-подложка листа примеров: тёмный пол, цветные кубы, вечернее солнце, синий
/// заполняющий свет.
fn dark_stage(app: &mut App) {
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.6, 0.0)).with(Light::default());
    app.spawn("Fill").at3(-3.0, 3.0, 3.0).with(Light::point(Color::rgb(0.35, 0.5, 1.0), 3.0, 12.0));
    app.spawn("Ground").scale(30.0, 1.0, 30.0).with(MeshRenderer::plane().with_color(Color::rgb(0.07, 0.075, 0.09)).with_material(0.0, 0.8));
    for (i, color) in [Color::rgb(0.85, 0.08, 0.05), Color::rgb(0.1, 0.35, 0.95), Color::rgb(0.95, 0.7, 0.05)].into_iter().enumerate() {
        app.spawn("Box").at3(-3.0 + i as f32 * 3.0, 0.6, -2.5).scale(1.2, 1.2, 1.2).with(MeshRenderer::cube().with_color(color).with_material(0.0, 0.5));
    }
}

/// Плитка шейдера материала на тёмной сцене в момент `time`.
fn material_tile(renderer: &mut Renderer, name: &str, alpha: AlphaMode, two_sided: bool, params: &[(&str, ParamValue)], time: f64, size: (u32, u32)) -> RgbaImage {
    const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let mut app = app_with(Path::new(ASSETS));
    let shader = db(&app).handle::<Shader>(&format!("shaders/{name}.shader")).unwrap();
    dark_stage(&mut app);
    let mut material = Material::new().shader(shader).double_sided(two_sided);
    material.alpha_mode = alpha;
    if name == "wind_sway" {
        material.base_color = Color::rgb(0.18, 0.45, 0.1);
    }
    for (key, value) in params {
        material.shader_params.set(key, value.clone());
    }
    let handle = add_material(&mut app, material);
    match name {
        "water" => {
            app.spawn("Water").at3(0.0, 0.05, 0.5).scale(9.0, 1.0, 5.0).with(MeshRenderer::plane().with_material_asset(handle));
        }
        "wind_sway" => {
            // Пучок травинок: тонкие высокие бруски с разным поворотом и высотой.
            for i in 0..14 {
                for j in 0..9 {
                    let x = -3.4 + i as f32 * 0.5 + ((j * 7 + i * 3) % 5) as f32 * 0.07;
                    let z = -0.6 + j as f32 * 0.35;
                    let h = 0.7 + ((i * 13 + j * 5) % 7) as f32 * 0.08;
                    app.spawn("Blade")
                        .at3(x, 0.0, z)
                        .scale(0.07, h, 0.03)
                        .rotation(Quat::from_rotation_y(i as f32 * 0.7 + j as f32))
                        .with(MeshRenderer::cube().with_material_asset(handle));
                }
            }
        }
        _ => {
            app.spawn("Ball").at3(-1.2, 1.2, 0.5).scale(2.0, 2.0, 2.0).with(MeshRenderer::sphere().with_material_asset(handle));
            app.spawn("Cube")
                .at3(1.6, 0.9, 0.8)
                .scale(1.5, 1.5, 1.5)
                .rotation(Quat::from_rotation_y(0.6))
                .with(MeshRenderer::cube().with_material_asset(handle));
        }
    }
    app.tick(1.0 / 60.0);
    app.world.time.elapsed = time;
    let view = CameraView::look_at(Vec3::new(0.0, 3.2, 6.5), Vec3::new(0.0, 0.8, 0.0), 50.0, size.0 as f32 / size.1 as f32, Color::rgb(0.02, 0.025, 0.04), false);
    renderer.capture_view(&app.world, size.0, size.1, Some(&view)).unwrap().0
}

/// Лист примеров `assets/shaders` для просмотра глазами: `SHADER_SHEET=<файл.png>` (без
/// переменной тест ничего не делает). Плитки подписаны именами шейдеров.
#[test]
fn example_sheet() {
    let Some(out) = std::env::var_os("SHADER_SHEET") else { return };
    const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    const TW: u32 = 320;
    const TH: u32 = 200;
    let mut tiles: Vec<(String, RgbaImage)> = Vec::new();
    let mut renderer = renderer();
    let materials: [(&str, AlphaMode, bool, Vec<(&str, ParamValue)>); 6] = [
        ("dissolve", AlphaMode::Opaque, false, vec![("amount", ParamValue::Float(0.45))]),
        ("hologram", AlphaMode::Blend, false, vec![]),
        ("toon", AlphaMode::Opaque, false, vec![]),
        ("water", AlphaMode::Opaque, false, vec![]),
        ("force_field", AlphaMode::Blend, true, vec![]),
        ("rim_light", AlphaMode::Opaque, false, vec![]),
    ];
    for (name, alpha, two_sided, params) in &materials {
        let image = material_tile(&mut renderer, name, *alpha, *two_sided, params, 1.0, (TW, TH));
        tiles.push((name.to_string(), image));
    }
    // Ветер: два момента рядом — видно, что трава качается.
    for (k, time) in [(1, 0.2), (2, 1.3)] {
        let image = material_tile(&mut renderer, "wind_sway", AlphaMode::Opaque, false, &[("strength", ParamValue::Float(0.45))], time, (TW, TH));
        tiles.push((format!("wind_sway_{k}"), image));
    }
    for name in ["sprite_outline", "sprite_flash", "sprite_palette"] {
        let mut app = app_with(Path::new(ASSETS));
        let db = db(&app);
        let shader = db.handle::<Shader>(&format!("shaders/{name}.shader")).unwrap();
        let texture = db.handle::<Texture>("sprites/player.png").unwrap();
        for i in 0..3 {
            let mut effect = SpriteShader::new(shader);
            match name {
                "sprite_flash" => effect.set("flash", i as f32 * 0.45),
                "sprite_outline" => effect.set("thickness", 1.0 + i as f32 * 0.5),
                _ => effect.set("new_color", [Color::rgb(0.2, 0.45, 0.95), Color::rgb(0.2, 0.8, 0.3), Color::rgb(0.75, 0.3, 0.9)][i]),
            }
            app.spawn("Sprite").at(-2.2 + i as f32 * 2.2, 0.0).scale(1.8, 1.8, 1.0).with(SpriteRenderer::textured(texture)).with(effect);
        }
        app.tick(1.0 / 60.0);
        let view = CameraView::orthographic_2d(Vec2::ZERO, 2.0, TW as f32 / TH as f32, Color::rgb(0.09, 0.1, 0.14));
        tiles.push((name.to_string(), renderer.capture_view(&app.world, TW, TH, Some(&view)).unwrap().0));
    }
    for name in ["vignette", "pixelate", "crt", "color_pop", "underwater"] {
        let mut app = app_with(Path::new(ASSETS));
        let shader = db(&app).handle::<Shader>(&format!("shaders/{name}.shader")).unwrap();
        dark_stage(&mut app);
        app.spawn("Ball").at3(0.0, 1.0, 0.5).scale(2.0, 2.0, 2.0).with(MeshRenderer::sphere().with_color(Color::rgb(0.9, 0.9, 0.95)).with_material(0.6, 0.25));
        let camera = app
            .spawn("Camera")
            .at3(0.0, 3.2, 6.5)
            .with(Camera { clear_color: Color::rgb(0.02, 0.025, 0.04), ..Camera::perspective(50.0) })
            .with(PostEffect::new(shader))
            .id();
        app.world.scene.get_mut(camera).unwrap().transform.look_at(Vec3::new(0.0, 0.8, 0.0), Vec3::Y);
        app.spawn("Environment").with(Environment::default());
        app.tick(1.0 / 60.0);
        app.world.time.elapsed = 1.0;
        tiles.push((name.to_string(), renderer.capture(&app.world, TW, TH).unwrap()));
    }
    let columns = 4u32;
    let rows = (tiles.len() as u32).div_ceil(columns);
    let mut sheet = RgbaImage::from_pixel(columns * (TW + 4) + 4, rows * (TH + 4) + 4, image::Rgba([24, 24, 28, 255]));
    for (i, (name, tile)) in tiles.iter().enumerate() {
        let (x, y) = (4 + (i as u32 % columns) * (TW + 4), 4 + (i as u32 / columns) * (TH + 4));
        image::imageops::replace(&mut sheet, tile, x as i64, y as i64);
        label(&mut sheet, x + 4, y + 4, name);
    }
    sheet.save(PathBuf::from(out)).unwrap();
}
