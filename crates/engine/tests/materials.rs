//! Материалы: старые сцены (материал прямо в `MeshRenderer`) рисуются так же, как до
//! появления ассетов `.material`. Эталонные кадры сняты до изменения рендерера и лежат
//! в `tests/data/materials_before/`; переснять: `MATERIALS_BASELINE=write`.
//! Все тесты с GPU идут через один замок (см. features2d.rs).

use std::path::{Path, PathBuf};

use engine::prelude::*;
use engine_render::{CameraView, Renderer};
use image::RgbaImage;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const BASELINE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/materials_before");
const W: u32 = 480;
const H: u32 = 270;

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

/// Картинки для просмотра глазами: `MATERIALS_SHOTS=<папка>`.
fn save_shot(image: &RgbaImage, name: &str) {
    if let Some(dir) = std::env::var_os("MATERIALS_SHOTS") {
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        image.save(dir.join(name)).unwrap();
    }
}

fn assets_app() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    engine::mount_assets(&mut app, Path::new(ASSETS)).expect("demo assets open");
    app
}

/// Демо-сцена как есть (компоненты на C++ и Orbit здесь не загружены — объекты стоят).
fn demo_scene() -> App {
    let mut app = assets_app();
    engine_core::missing::keep_missing_scripts(true);
    // Копия демо-сцены на момент снятия эталона (саму сцену потом одели в материалы).
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/demo3d_before_materials.scene")).unwrap();
    app.world.load_scene_str(&text, "scenes/demo3d.scene").expect("demo scene loads");
    app.tick(1.0 / 60.0);
    app
}

/// Все старые возможности встроенного материала: цвет, металл/шероховатость, свечение,
/// текстура по UV и в мировых координатах, dev-сетка, без теней, модели из файлов.
fn legacy_scene() -> App {
    let mut app = assets_app();
    let db = app.world.resources.get::<engine_assets::AssetDatabase>().unwrap().clone();
    let checker = db.handle::<engine_assets::Texture>("models/torus_checker.png").unwrap();
    let mesh = |path: &str| db.handle::<engine_assets::Mesh>(path).unwrap();
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.0))
        .with(Light::default());
    app.spawn("Point").at3(-2.0, 1.5, 2.0).with(Light::point(Color::rgb(1.0, 0.4, 0.1), 6.0, 6.0));
    app.spawn("Ground")
        .scale(30.0, 1.0, 30.0)
        .with(MeshRenderer::plane().with_color(Color::hex(0x707a68)).with_material(0.0, 0.9).with_grid().without_shadows());
    app.spawn("Red").at3(-3.0, 0.5, 0.0).with(MeshRenderer::cube().with_color(Color::rgb(0.9, 0.15, 0.1)).with_material(0.0, 0.35));
    app.spawn("Gold").at3(-1.5, 0.6, -1.0).scale(1.2, 1.2, 1.2).with(MeshRenderer::sphere().with_color(Color::rgb(1.0, 0.6, 0.1)).with_material(1.0, 0.25));
    app.spawn("Glow").at3(0.0, 0.4, 1.5).scale(0.6, 0.6, 0.6).with(MeshRenderer::sphere().with_emissive(Color::rgb(4.0, 1.5, 0.3)).without_shadows());
    app.spawn("UvBox").at3(1.5, 0.5, 0.0).with(MeshRenderer::cube().with_texture(checker, 0.0));
    app.spawn("WorldBox").at3(3.2, 1.0, -1.0).scale(1.5, 2.0, 1.5).with(MeshRenderer::cube().with_texture(checker, 0.5).with_color(Color::rgb(0.8, 0.9, 1.0)));
    app.spawn("Cylinder").at3(0.0, 0.75, -2.5).scale(0.8, 1.5, 0.8).with(MeshRenderer::cylinder().with_grid());
    app.spawn("Torus").at3(-3.0, 1.6, -2.0).with(MeshRenderer::asset(mesh("models/torus.obj")).with_texture(checker, 0.0));
    app.spawn("Vase").at3(2.0, 0.0, 2.0).with(MeshRenderer::asset(mesh("models/vase.gltf")).with_color(Color::rgb(0.3, 0.6, 0.9)));
    app.spawn("Stairs").at3(4.5, 0.0, 1.5).scale(0.5, 0.5, 0.5).with(MeshRenderer::asset(mesh("models/stairs.glb")));
    app.spawn("Crystal").at3(-0.5, 1.6, -1.0).scale(0.6, 0.6, 0.6).with(MeshRenderer::asset(mesh("models/crystal.obj")).with_color(Color::rgb(0.2, 0.7, 0.9)).with_material(0.2, 0.2));
    app.tick(1.0 / 60.0);
    app
}

fn legacy_view() -> CameraView {
    CameraView::look_at(Vec3::new(0.5, 4.5, 8.5), Vec3::new(0.3, 0.6, 0.0), 55.0, W as f32 / H as f32, Color::hex(0x1b2433), true)
}

/// Сравнить с эталоном: допускается разница в 3 единицы на канал у отдельных пикселей
/// (порядок сложения на GPU), заметно отличающихся — не больше 0,2 %.
fn compare_with_baseline(image: &RgbaImage, name: &str) {
    let path = Path::new(BASELINE).join(name);
    if std::env::var("MATERIALS_BASELINE").as_deref() == Ok("write") {
        std::fs::create_dir_all(BASELINE).unwrap();
        image.save(&path).unwrap();
        return;
    }
    let before = image::open(&path)
        .unwrap_or_else(|e| panic!("baseline {} is missing ({e}); run with MATERIALS_BASELINE=write", path.display()))
        .to_rgba8();
    assert_eq!(before.dimensions(), image.dimensions());
    let mut worst = 0u8;
    let mut differing = 0usize;
    for (a, b) in before.pixels().zip(image.pixels()) {
        let diff = a.0.iter().zip(&b.0).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        worst = worst.max(diff);
        if diff > 3 {
            differing += 1;
        }
    }
    let total = (image.width() * image.height()) as usize;
    assert!(
        differing * 500 <= total,
        "{name}: {differing} of {total} pixels differ from the render made before materials (worst {worst})"
    );
}

#[test]
fn old_scenes_render_like_before_materials() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");

    let demo = demo_scene();
    let image = renderer.capture(&demo.world, W, H).expect("capture");
    save_shot(&image, "before_demo3d.png");
    compare_with_baseline(&image, "demo3d.png");

    let legacy = legacy_scene();
    let (image, _) = renderer.capture_view(&legacy.world, W, H, Some(&legacy_view())).expect("capture");
    save_shot(&image, "before_legacy.png");
    compare_with_baseline(&image, "legacy.png");
}

// ----------------------------------------------------------------------------------
// Материалы: настоящие пиксели
// ----------------------------------------------------------------------------------

/// Временный каталог ассетов с картинками; удаляется вместе со значением.
struct TempAssets(PathBuf);

impl TempAssets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("piska_materials_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn image(&self, name: &str, size: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) {
        RgbaImage::from_fn(size, size, |x, y| image::Rgba(pixel(x, y))).save(self.0.join(name)).unwrap();
    }
}

impl Drop for TempAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app_with(assets: &TempAssets) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    engine::mount_assets(&mut app, &assets.0).expect("temp assets open");
    app
}

fn db(app: &App) -> AssetDatabase {
    app.world.resources.get::<AssetDatabase>().unwrap().clone()
}

fn add_material(app: &mut App, material: Material) -> AssetHandle<Material> {
    app.world.resources.get_or_insert_with(Materials::default).add(material)
}

fn sun(app: &mut App, rotation: Quat) {
    app.spawn("Sun").rotation(rotation).with(Light::default());
}

/// Солнце светит строго вниз.
fn sun_down() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

const SIZE: u32 = 200;

fn shot(renderer: &mut Renderer, app: &mut App, eye: Vec3, target: Vec3, name: &str) -> (RgbaImage, RenderStats) {
    app.tick(1.0 / 60.0);
    let view = CameraView::look_at(eye, target, 50.0, 1.0, Color::BLACK, false);
    let (image, stats) = renderer.capture_view(&app.world, SIZE, SIZE, Some(&view)).expect("capture");
    save_shot(&image, name);
    (image, stats)
}

fn luma(p: [u8; 4]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn px(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

/// Средняя яркость квадрата 9×9 вокруг точки.
fn mean_luma(image: &RgbaImage, cx: u32, cy: u32) -> f32 {
    let mut sum = 0.0;
    for y in cy - 4..=cy + 4 {
        for x in cx - 4..=cx + 4 {
            sum += luma(px(image, x, y));
        }
    }
    sum / 81.0
}

/// Карта нормалей с постоянным наклоном (OpenGL: зелёный — вверх по картинке).
fn tilted_normal(x: f32, y: f32) -> [u8; 4] {
    let z = (1.0 - x * x - y * y).max(0.0).sqrt();
    let to = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
    [to(x), to(y), to(z), 255]
}

#[test]
fn a_normal_map_changes_the_shading_of_a_flat_plane_in_the_right_direction() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("normals");
    // Наклон к +U (вправо по картинке) и к «верху» картинки.
    assets.image("right.png", 4, |_, _| tilted_normal(0.6, 0.0));
    assets.image("up.png", 4, |_, _| tilted_normal(0.0, 0.6));
    let mut renderer = Renderer::headless().expect("GPU");

    // Плоскость: U растёт вдоль +X, «верх» картинки — вдоль −Z.
    let brightness = |normal: Option<&str>, sun_toward: Vec3, name: &str, renderer: &mut Renderer| {
        let mut app = app_with(&assets);
        let database = db(&app);
        let mut material = Material::new().color(Color::rgb(0.7, 0.7, 0.7)).roughness(0.9);
        if let Some(file) = normal {
            material = material.normal_map(database.handle(file).unwrap(), 1.0);
        }
        let handle = add_material(&mut app, material);
        // Солнце светит вдоль `sun_toward` (направление полёта лучей).
        app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, sun_toward.normalize())).with(Light::default());
        app.spawn("Plane").scale(6.0, 1.0, 6.0).with(MeshRenderer::plane().with_material_asset(handle));
        let (image, _) = shot(renderer, &mut app, Vec3::new(0.0, 5.0, 0.01), Vec3::ZERO, name);
        mean_luma(&image, SIZE / 2, SIZE / 2)
    };
    // Лучи идут к −X (солнце со стороны +X), сильно наклонно.
    let from_right = Vec3::new(-1.0, -0.6, 0.0);
    let flat = brightness(None, from_right, "normal_flat.png", &mut renderer);
    let toward_sun = brightness(Some("right.png"), from_right, "normal_right.png", &mut renderer);
    let away = brightness(Some("right.png"), Vec3::new(1.0, -0.6, 0.0), "normal_right_away.png", &mut renderer);
    let flat_away = brightness(None, Vec3::new(1.0, -0.6, 0.0), "normal_flat_away.png", &mut renderer);
    assert!(toward_sun > flat + 8.0, "a normal tilted toward the sun must be brighter: {toward_sun} vs flat {flat}");
    assert!(away < flat_away - 8.0, "a normal tilted away from the sun must be darker: {away} vs flat {flat_away}");

    // Зелёный канал: «вверх» по картинке — это −Z плоскости. Солнце со стороны −Z.
    let from_far = Vec3::new(0.0, -0.6, 1.0);
    let flat = brightness(None, from_far, "normal_flat_far.png", &mut renderer);
    let up = brightness(Some("up.png"), from_far, "normal_up.png", &mut renderer);
    assert!(up > flat + 8.0, "green = up the image must tilt toward −Z on this plane: {up} vs flat {flat}");
}

#[test]
fn cutout_makes_holes_and_the_shadow_has_the_same_holes() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("cutout");
    // Полосы вдоль U: чётные четверти непрозрачны, нечётные — дыры.
    assets.image("stripes.png", 64, |x, _| if (x / 16) % 2 == 0 { [255, 255, 255, 255] } else { [255, 255, 255, 0] });
    let mut renderer = Renderer::headless().expect("GPU");

    let scene = |cutout: bool, eye: Vec3, target: Vec3, name: &str, renderer: &mut Renderer| {
        let mut app = app_with(&assets);
        let stripes = db(&app).handle("stripes.png").unwrap();
        let mut leaf = Material::new().color(Color::rgb(0.9, 0.1, 0.1)).texture(stripes).double_sided(true);
        if cutout {
            leaf = leaf.cutout(0.5);
        }
        let leaf = add_material(&mut app, leaf);
        sun(&mut app, sun_down());
        app.spawn("Ground").scale(40.0, 1.0, 40.0).with(MeshRenderer::plane().with_color(Color::rgb(0.8, 0.8, 0.8)));
        app.spawn("Leaf").at3(0.0, 4.0, 0.0).scale(4.0, 1.0, 4.0).with(MeshRenderer::plane().with_material_asset(leaf));
        shot(renderer, &mut app, eye, target, name).0
    };

    // Сверху: сквозь дыры видна серая земля, на полосах — красный лист.
    let above = (Vec3::new(0.0, 12.0, 0.01), Vec3::ZERO);
    let image = scene(true, above.0, above.1, "cutout_above.png", &mut renderer);
    let samples: Vec<[u8; 4]> = (0..4).map(|quarter| px(&image, SIZE / 2 - 30 + quarter * 20, SIZE / 2)).collect();
    let red = |p: [u8; 4]| p[0] as i32 - p[1] as i32 > 50;
    assert!(red(samples[0]) && red(samples[2]), "opaque stripes are red: {samples:?}");
    assert!(!red(samples[1]) && !red(samples[3]), "holes show the ground: {samples:?}");
    let solid = scene(false, above.0, above.1, "cutout_off_above.png", &mut renderer);
    assert!(red(px(&solid, SIZE / 2 - 10, SIZE / 2)), "without cutout there are no holes");

    // Снизу, под листом: тень на земле полосатая так же; без выреза — сплошная.
    let below = (Vec3::new(0.0, 2.2, 7.0), Vec3::new(0.0, 0.0, 0.0));
    let striped = scene(true, below.0, below.1, "cutout_shadow.png", &mut renderer);
    let solid = scene(false, below.0, below.1, "cutout_off_shadow.png", &mut renderer);
    // Строка земли под центром листа: в сплошной тени темно везде, в полосатой — чередуется.
    // Камера смотрит в центр тени: строка через середину кадра пересекает полосы.
    let row = |image: &RgbaImage| -> Vec<f32> { (0..SIZE).map(|x| luma(px(image, x, SIZE / 2))).collect() };
    let (striped_row, solid_row) = (row(&striped), row(&solid));
    let bright = |values: &[f32]| values.iter().copied().fold(0.0, f32::max);
    let dark = |values: &[f32]| values.iter().copied().fold(f32::MAX, f32::min);
    // В середине строки (там тень) у сплошной тени нет светлых полос, у полосатой — есть.
    let middle = |values: &[f32]| values[(SIZE / 2 - 30) as usize..(SIZE / 2 + 30) as usize].to_vec();
    let (s, o) = (middle(&striped_row), middle(&solid_row));
    assert!(bright(&o) - dark(&o) < 25.0, "a solid leaf casts an even shadow: {o:?}");
    assert!(bright(&s) - dark(&s) > 40.0, "a cut-out leaf casts a striped shadow: {s:?}");
}

#[test]
fn blend_shows_what_is_behind_and_draws_after_opaque() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("blend");
    let mut renderer = Renderer::headless().expect("GPU");
    let scene = |alpha: f32, blend: bool, name: &str, renderer: &mut Renderer| {
        let mut app = app_with(&assets);
        let mut glass = Material::new().color(Color::rgba(0.0, 0.2, 1.0, alpha)).unlit(true);
        if blend {
            glass = glass.transparent();
        }
        let glass = add_material(&mut app, glass);
        let red = add_material(&mut app, Material::new().color(Color::rgb(1.0, 0.0, 0.0)).unlit(true));
        sun(&mut app, sun_down());
        // Стекло стоит ближе к камере и в иерархии раньше красного куба — порядок
        // отрисовки решает рендерер, а не иерархия.
        app.spawn("Glass").at3(0.0, 0.0, 2.0).scale(3.0, 3.0, 0.1).with(MeshRenderer::cube().with_material_asset(glass));
        app.spawn("Red").scale(1.5, 1.5, 1.5).with(MeshRenderer::cube().with_material_asset(red));
        shot(renderer, &mut app, Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, name)
    };
    let (image, stats) = scene(0.4, true, "blend.png", &mut renderer);
    let center = px(&image, SIZE / 2, SIZE / 2);
    assert!(center[0] > 80 && center[2] > 80, "red cube is seen through blue glass: {center:?}");
    assert_eq!(stats.transparent_meshes, 1);
    let (opaque, _) = scene(0.4, false, "blend_off.png", &mut renderer);
    let center = px(&opaque, SIZE / 2, SIZE / 2);
    assert!(center[0] < 40 && center[2] > 120, "opaque glass hides the cube: {center:?}");
}

#[test]
fn a_double_sided_plane_is_drawn_and_lit_from_both_sides() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("double");
    let mut renderer = Renderer::headless().expect("GPU");
    // Плоскость повёрнута лицом к +Z; камера смотрит на неё сзади (с −Z).
    let scene = |double: bool, rays: Vec3, name: &str, renderer: &mut Renderer| {
        let mut app = app_with(&assets);
        let material = add_material(&mut app, Material::new().color(Color::rgb(0.8, 0.8, 0.8)).roughness(0.9).double_sided(double));
        app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, rays.normalize())).with(Light::default());
        app.spawn("Panel")
            .rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
            .scale(3.0, 1.0, 3.0)
            .with(MeshRenderer::plane().with_material_asset(material).without_shadows());
        mean_luma(&shot(renderer, &mut app, Vec3::new(0.0, 0.0, -6.0), Vec3::ZERO, name).0, SIZE / 2, SIZE / 2)
    };
    // Лучи летят к +Z: освещена сторона, смотрящая на −Z (обратная, к камере).
    let lit_back = scene(true, Vec3::new(0.0, -0.3, 1.0), "double_lit.png", &mut renderer);
    let unlit_back = scene(true, Vec3::new(0.0, -0.3, -1.0), "double_unlit.png", &mut renderer);
    let culled = scene(false, Vec3::new(0.0, -0.3, 1.0), "single_culled.png", &mut renderer);
    assert!(culled < 10.0, "a single-sided plane is invisible from behind: {culled}");
    assert!(lit_back > 40.0, "the back side is drawn: {lit_back}");
    assert!(lit_back > unlit_back + 30.0, "the back side is lit by light falling on it: {lit_back} vs {unlit_back}");
}

#[test]
fn two_submeshes_get_two_materials_and_model_materials_come_from_the_file() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("GPU");
    let eye = Vec3::new(3.0, 3.0, 4.0);
    let render = |setup: &dyn Fn(&mut App, AssetHandle<Mesh>) -> MeshRenderer, name: &str, renderer: &mut Renderer| {
        let mut app = assets_app();
        let stairs = db(&app).handle::<Mesh>("models/stairs.glb").unwrap();
        let mesh = setup(&mut app, stairs);
        app.spawn("Stairs").with(mesh.without_shadows());
        let bounds = db(&app).load_mesh(stairs.guid()).unwrap().bounds();
        let center = (bounds.0 + bounds.1) * 0.5;
        let size = (bounds.1 - bounds.0).length();
        shot(renderer, &mut app, center + eye.normalize() * size * 1.3, center, name)
    };
    let model = assets_app();
    let parts = db(&model).load_mesh(db(&model).guid_of("models/stairs.glb").unwrap()).unwrap().submeshes;
    assert_eq!(parts.len(), 2, "stairs.glb has two material groups: {parts:?}");

    // Два материала в слотах — два цвета и два вызова.
    let (image, stats) = render(
        &|app, stairs| {
            let red = add_material(app, Material::new().color(Color::rgb(1.0, 0.0, 0.0)).unlit(true));
            let green = add_material(app, Material::new().color(Color::rgb(0.0, 1.0, 0.0)).unlit(true));
            MeshRenderer::asset(stairs).with_materials(vec![red, green])
        },
        "stairs_slots.png",
        &mut renderer,
    );
    let count = |image: &RgbaImage, f: &dyn Fn([u8; 4]) -> bool| image.pixels().filter(|p| f(p.0)).count();
    let reds = count(&image, &|p| p[0] > 150 && p[1] < 60);
    let greens = count(&image, &|p| p[1] > 150 && p[0] < 60);
    assert!(reds > 200 && greens > 200, "both parts are visible in their colours: {reds} red, {greens} green");
    assert_eq!(stats.draw_calls, 2, "one draw call per part: {stats:?}");

    // Один слот — весь меш одним вызовом.
    let (_, stats) = render(
        &|app, stairs| MeshRenderer::asset(stairs).with_material_asset(add_material(app, Material::new())),
        "stairs_one.png",
        &mut renderer,
    );
    assert_eq!(stats.draw_calls, 1, "parts with the same material are drawn together: {stats:?}");

    // Материалы файла: цвета частей различаются (у встроенного — один серый на всё).
    let file_materials = db(&model).model_materials(db(&model).guid_of("models/stairs.glb").unwrap()).unwrap();
    assert_eq!(file_materials.len(), 2);
    assert_ne!(file_materials[0].base_color, file_materials[1].base_color);
    let (image, stats) = render(&|_, stairs| MeshRenderer::model(stairs), "stairs_model.png", &mut renderer);
    assert_eq!(stats.draw_calls, 2, "{stats:?}");
    let (legacy, legacy_stats) = render(&|_, stairs| MeshRenderer::asset(stairs), "stairs_legacy.png", &mut renderer);
    assert_eq!(legacy_stats.draw_calls, 1, "an old renderer without materials stays one draw call");
    assert_ne!(image, legacy, "file materials change the picture");
}

#[test]
fn material_files_load_save_and_hot_reload() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("reload");
    let mut app = app_with(&assets);
    let mut database = db(&app);
    let handle = database
        .save_material("materials/paint.material", &Material::new().color(Color::rgb(1.0, 0.0, 0.0)).unlit(true))
        .expect("saved");
    assert!(assets.0.join("materials/paint.material.meta").is_file());
    assert_eq!(database.meta(handle.guid()).unwrap().importer, "Material");
    // Перечитанный каталог знает файл под тем же GUID, и материал читается обратно.
    let reopened = AssetDatabase::open(&assets.0).unwrap();
    assert_eq!(reopened.guid_of("materials/paint.material"), Some(handle.guid()));
    assert_eq!(reopened.load_material(handle.guid()).unwrap().base_color, Color::rgb(1.0, 0.0, 0.0));
    app.world.resources.insert(reopened);

    let mut renderer = Renderer::headless().expect("GPU");
    sun(&mut app, sun_down());
    app.spawn("Ball").with(MeshRenderer::sphere().with_material_asset(handle));
    let eye = Vec3::new(0.0, 0.0, 3.0);
    let (image, _) = shot(&mut renderer, &mut app, eye, Vec3::ZERO, "reload_before.png");
    let before = px(&image, SIZE / 2, SIZE / 2);
    assert!(before[0] > 200 && before[2] < 30, "{before:?}");

    // Файл изменили (как это делает инспектор): после `reload_material` — новый цвет.
    database.save_material("materials/paint.material", &Material::new().color(Color::rgb(0.0, 0.0, 1.0)).unlit(true)).unwrap();
    let (image, _) = shot(&mut renderer, &mut app, eye, Vec3::ZERO, "reload_cached.png");
    assert_eq!(px(&image, SIZE / 2, SIZE / 2), before, "the cached material is used until reloaded");
    renderer.reload_material(handle.guid());
    let (image, _) = shot(&mut renderer, &mut app, eye, Vec3::ZERO, "reload_after.png");
    let after = px(&image, SIZE / 2, SIZE / 2);
    assert!(after[2] > 200 && after[0] < 30, "{after:?}");

    // Материал из кода под тем же GUID подменяет файл, его правки видны сразу.
    app.world.resources.get_or_insert_with(Materials::default).set(handle, Material::new().color(Color::rgb(0.0, 1.0, 0.0)).unlit(true));
    let (image, _) = shot(&mut renderer, &mut app, eye, Vec3::ZERO, "reload_runtime.png");
    assert!(px(&image, SIZE / 2, SIZE / 2)[1] > 200);
    app.world.resources.get_mut::<Materials>().unwrap().get_mut(handle).unwrap().base_color = Color::rgb(1.0, 1.0, 0.0);
    let (image, _) = shot(&mut renderer, &mut app, eye, Vec3::ZERO, "reload_runtime2.png");
    let yellow = px(&image, SIZE / 2, SIZE / 2);
    assert!(yellow[0] > 200 && yellow[1] > 200 && yellow[2] < 30, "{yellow:?}");

    // Сломанный файл — пурпурный объект и ошибка в логе, а не падение.
    std::fs::write(assets.0.join("materials/paint.material"), "(metallic: \"shiny\")").unwrap();
    app.world.resources.remove::<Materials>();
    renderer.reload_material(handle.guid());
    let (image, _) = shot(&mut renderer, &mut app, eye, Vec3::ZERO, "reload_broken.png");
    let magenta = px(&image, SIZE / 2, SIZE / 2);
    assert!(magenta[0] > 100 && magenta[2] > 100 && magenta[1] < 60, "{magenta:?}");
}

#[test]
fn materials_that_differ_only_in_colour_batch_together() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("batch");
    assets.image("a.png", 4, |_, _| [200, 100, 50, 255]);
    assets.image("b.png", 4, |_, _| [50, 100, 200, 255]);
    let mut app = app_with(&assets);
    let database = db(&app);
    let (a, b) = (database.handle::<Texture>("a.png").unwrap(), database.handle::<Texture>("b.png").unwrap());
    sun(&mut app, sun_down());
    // 100 кубов: четыре материала с одной текстурой `a` и разными цветами/металлом,
    // плюс оттенок у каждого объекта, — одна пачка. Ещё 20 кубов с текстурой `b` — вторая.
    let colours: Vec<AssetHandle<Material>> = (0..4)
        .map(|i| add_material(&mut app, Material::new().texture(a).color(Color::rgb(0.2 * i as f32, 0.5, 0.5)).metallic(i as f32 / 4.0)))
        .collect();
    let other = add_material(&mut app, Material::new().texture(b));
    for i in 0..120 {
        let (x, z) = ((i % 12) as f32 - 6.0, (i / 12) as f32 - 5.0);
        let material = if i < 100 { colours[i % 4] } else { other };
        app.spawn("Cube")
            .at3(x * 1.5, 0.0, z * 1.5)
            .with(MeshRenderer::cube().with_material_asset(material).with_tint(Color::rgb(1.0, 0.9, 0.8)).without_shadows());
    }
    let mut renderer = Renderer::headless().expect("GPU");
    let (_, stats) = shot(&mut renderer, &mut app, Vec3::new(0.0, 40.0, 1.0), Vec3::ZERO, "batch.png");
    assert_eq!(stats.meshes, 120);
    assert_eq!(stats.draw_calls, 2, "colour, metallic and tint do not split batches: {stats:?}");
    assert_eq!(stats.material_bind_groups, 2, "{stats:?}");

    // Другая карта нормалей или тайлинг — уже другая группа и ещё один вызов.
    let tiled = add_material(&mut app, Material::new().texture(a).tiling(2.0, 2.0));
    app.spawn("Tiled").at3(0.0, 0.0, 9.0).with(MeshRenderer::cube().with_material_asset(tiled).without_shadows());
    let (_, stats) = shot(&mut renderer, &mut app, Vec3::new(0.0, 40.0, 1.0), Vec3::ZERO, "batch2.png");
    assert_eq!(stats.draw_calls, 3, "{stats:?}");
}

#[test]
fn set_color_tints_one_object_without_touching_the_shared_material() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("tint");
    let mut app = app_with(&assets);
    let white = add_material(&mut app, Material::new().unlit(true));
    sun(&mut app, sun_down());
    let left = app.spawn("Left").at3(-1.0, 0.0, 0.0).with(MeshRenderer::cube().with_material_asset(white)).id();
    app.spawn("Right").at3(1.0, 0.0, 0.0).with(MeshRenderer::cube().with_material_asset(white));
    app.world.scene.get_mut(left).unwrap().get_mut::<MeshRenderer>().unwrap().set_color(Color::rgb(1.0, 0.0, 0.0));
    let mut renderer = Renderer::headless().expect("GPU");
    let (image, stats) = shot(&mut renderer, &mut app, Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, "tint.png");
    let (l, r) = (px(&image, SIZE / 2 - 30, SIZE / 2), px(&image, SIZE / 2 + 30, SIZE / 2));
    assert!(l[0] > 200 && l[1] < 30, "tinted: {l:?}");
    assert!(r[0] > 200 && r[1] > 200, "the other object keeps the material colour: {r:?}");
    assert_eq!(stats.draw_calls - stats.shadow_draw_calls, 1, "{stats:?}");
}

#[test]
fn sample_materials_load_and_the_demo_scene_wears_them() {
    let _gpu = gpu_lock();
    let names = ["painted_metal", "rough_stone", "emissive_lamp", "glass", "foliage", "checker"];
    let mut app = assets_app();
    let database = db(&app);
    sun(&mut app, Quat::from_euler(EulerRot::YXZ, 0.6, -0.8, 0.0));
    app.spawn("Floor").at3(0.0, -0.75, 0.0).scale(20.0, 1.0, 6.0).with(MeshRenderer::plane().with_color(Color::rgb(0.35, 0.36, 0.38)));
    for (index, name) in names.iter().enumerate() {
        let path = format!("materials/{name}.material");
        let material = database.load_material(database.guid_of(&path).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"));
        for texture in material.textures() {
            assert!(database.relative_path_of(texture).is_some(), "{path} points at a missing texture");
        }
        let handle = database.handle::<Material>(&path).unwrap();
        app.spawn(*name).at3(index as f32 * 1.6 - 4.0, 0.0, 0.0).scale(1.4, 1.4, 1.4).with(MeshRenderer::sphere().with_material_asset(handle));
    }
    let mut renderer = Renderer::headless().expect("GPU");
    app.tick(1.0 / 60.0);
    let view = CameraView::look_at(Vec3::new(0.0, 1.6, 6.5), Vec3::new(0.0, 0.0, 0.0), 50.0, 2.4, Color::BLACK, true);
    let (image, stats) = renderer.capture_view(&app.world, 960, 400, Some(&view)).unwrap();
    save_shot(&image, "gallery.png");
    assert_eq!(stats.transparent_meshes, 1, "the glass sphere is transparent");
    let magenta = image.pixels().filter(|p| p[0] > 200 && p[2] > 200 && p[1] < 40).count();
    assert!(magenta < 50, "no material failed to load ({magenta} magenta pixels)");

    let mut demo = assets_app();
    engine_core::missing::keep_missing_scripts(true);
    engine::load_scene(&mut demo, "scenes/demo3d.scene").expect("demo scene loads");
    demo.tick(1.0 / 60.0);
    let (image, stats) = renderer.capture_view(&demo.world, 960, 540, None).unwrap();
    save_shot(&image, "demo3d_materials.png");
    assert!(stats.material_bind_groups >= 5, "{stats:?}");
}

/// Правило свечения: у объекта с материалами-ассетами `MeshRenderer::emissive` добавляется
/// к свечению материала (раньше молча игнорировалось); чёрный — ничего не меняет.
#[test]
fn mesh_renderer_emissive_adds_to_the_material_glow() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("emissive");
    let mut renderer = Renderer::headless().expect("GPU");
    let scene = |extra: Color, name: &str, renderer: &mut Renderer| {
        let mut app = app_with(&assets);
        let dark = add_material(&mut app, Material::new().color(Color::rgb(0.05, 0.05, 0.05)).roughness(1.0));
        app.spawn("Cube").scale(3.0, 3.0, 3.0).with(MeshRenderer::cube().with_material_asset(dark).with_emissive(extra));
        let (image, _) = shot(renderer, &mut app, Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, name);
        px(&image, SIZE / 2, SIZE / 2)
    };
    let plain = scene(Color::BLACK, "emissive_off.png", &mut renderer);
    let glowing = scene(Color::rgb(2.0, 0.0, 0.0), "emissive_on.png", &mut renderer);
    assert!(plain[0] < 60, "a dark material without glow stays dark: {plain:?}");
    assert!(glowing[0] > plain[0] + 80 && glowing[0] > glowing[1] + 60, "the renderer glow is added: {glowing:?} vs {plain:?}");
}
