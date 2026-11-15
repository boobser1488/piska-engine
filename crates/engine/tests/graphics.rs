//! Новая графика (docs/api/graphics.md): каскадные мягкие тени, небо с солнцем и
//! облаками, время суток, экранное затенение, контур, FXAA. Каждый эффект проверяется
//! числом: то, что он обещает, видно в кадре, а без компонента кадр как раньше.
//! Нужна видеокарта; все проверки — на одном устройстве (драйвер не любит много устройств).

use std::sync::{Mutex, OnceLock};

use engine::prelude::*;
use engine_render::{CameraView, RenderStats, Renderer};
use image::RgbaImage;

const W: u32 = 320;
const H: u32 = 200;

fn renderer() -> std::sync::MutexGuard<'static, Renderer> {
    static RENDERER: OnceLock<Mutex<Renderer>> = OnceLock::new();
    RENDERER
        .get_or_init(|| Mutex::new(Renderer::headless().expect("a GPU adapter")))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn capture(app: &App, view: &CameraView) -> (RgbaImage, RenderStats) {
    let out = renderer().capture_view(&app.world, W, H, Some(view)).expect("capture");
    // `GRAPHICS_SHOTS=<каталог>` — сохранить кадры, чтобы посмотреть глазами.
    if let Ok(dir) = std::env::var("GRAPHICS_SHOTS") {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = std::fs::create_dir_all(&dir);
        let _ = out.0.save(std::path::Path::new(&dir).join(format!("shot_{n:03}.png")));
    }
    out
}

fn luminance(image: &RgbaImage, x: u32, y: u32) -> f32 {
    let p = image.get_pixel(x.min(W - 1), y.min(H - 1)).0;
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

/// Средняя яркость квадрата 3×3 вокруг проекции мировой точки.
fn luminance_at(image: &RgbaImage, view: &CameraView, point: Vec3) -> f32 {
    let clip = view.view_proj * point.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let x = ((ndc.x * 0.5 + 0.5) * W as f32) as i32;
    let y = ((0.5 - ndc.y * 0.5) * H as f32) as i32;
    let mut sum = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            sum += luminance(image, (x + dx).max(0) as u32, (y + dy).max(0) as u32);
        }
    }
    sum / 9.0
}

/// Цвет пикселя, куда проецируется мировая точка.
fn pixel_at(image: &RgbaImage, view: &CameraView, point: Vec3) -> [u8; 4] {
    let clip = view.view_proj * point.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let x = (((ndc.x * 0.5 + 0.5) * W as f32) as u32).min(W - 1);
    let y = (((0.5 - ndc.y * 0.5) * H as f32) as u32).min(H - 1);
    image.get_pixel(x, y).0
}

const SUN: Vec3 = Vec3::new(0.6, -0.8, 0.0);

/// Земля, три куба 4×4×4 на 6, 25 и 60 м перед камерой, солнце сбоку-сверху.
fn shadow_scene(soft: Option<SoftShadows>) -> App {
    let mut app = engine::app();
    app.spawn("Environment").with(Environment { shadow_distance: 120.0, fog_density: 0.0, bloom: 0.0, vignette: 0.0, ..Default::default() });
    let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, SUN.normalize());
    app.spawn("Sun").rotation(rotation).with(Light::directional(2.4));
    app.spawn("Ground").scale(400.0, 1.0, 400.0).with(MeshRenderer::plane().with_color(Color::rgb(0.7, 0.7, 0.7)));
    for z in [-6.0, -25.0, -60.0] {
        app.spawn("Box").at3(0.0, 2.0, z).scale(4.0, 4.0, 4.0).with(MeshRenderer::cube().with_color(Color::rgb(0.8, 0.3, 0.2)));
    }
    if let Some(soft) = soft {
        app.spawn("Shadows").with(soft);
    }
    app
}

fn shadow_view() -> CameraView {
    CameraView::look_at(Vec3::new(0.0, 18.0, 10.0), Vec3::new(0.0, 0.0, -35.0), 55.0, W as f32 / H as f32, Color::BLACK, true)
}

/// Отношение яркости земли в тени куба к земле на солнечной стороне.
fn shadow_ratios(image: &RgbaImage, view: &CameraView) -> Vec<f32> {
    [-6.0f32, -25.0, -60.0]
        .iter()
        .map(|&z| {
            // Солнце светит к +X: тень лежит за правой гранью куба, свет — у левой.
            let shadowed = luminance_at(image, view, Vec3::new(3.5, 0.0, z));
            let lit = luminance_at(image, view, Vec3::new(-3.5, 0.0, z));
            shadowed / lit.max(1.0)
        })
        .collect()
}

#[test]
fn cascaded_soft_shadows_reach_far_and_stay_dark_near() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let view = shadow_view();
    let (plain, plain_stats) = capture(&shadow_scene(None), &view);
    let (cascaded, stats) = capture(&shadow_scene(Some(SoftShadows::default())), &view);
    let ratios = shadow_ratios(&cascaded, &view);
    for (ratio, z) in ratios.iter().zip([6, 25, 60]) {
        assert!(*ratio < 0.75, "cascaded shadow behind the box at {z} m is visible: ratio {ratio} ({ratios:?})");
    }
    // Каждый каскад — свой проход теней; без компонента — один.
    assert!(stats.shadow_draw_calls >= 4, "{stats:?}");
    assert!(plain_stats.shadow_draw_calls <= 2, "{plain_stats:?}");
    // Без компонента тени тоже есть (одна карта), кадр не пустой.
    let plain_ratios = shadow_ratios(&plain, &view);
    assert!(plain_ratios[0] < 0.75, "single-map shadow still works: {plain_ratios:?}");
    // Выключенный компонент — кадр как без него.
    let (off, _) = capture(&shadow_scene(Some(SoftShadows { enabled: false, ..Default::default() })), &view);
    assert!(off == plain, "a disabled SoftShadows renders exactly like no component");
}

#[test]
fn shadow_filters_differ_and_preset_low_caps_cascades() {
    let view = shadow_view();
    let hard = capture(&shadow_scene(Some(SoftShadows { filter: ShadowFilter::Hard, ..Default::default() })), &view).0;
    let soft = capture(&shadow_scene(Some(SoftShadows { filter: ShadowFilter::Soft, softness: 4.0, ..Default::default() })), &view).0;
    let diff = hard.pixels().zip(soft.pixels()).filter(|(a, b)| a.0 != b.0).count();
    assert!(diff > 50, "the soft filter changes shadow edges: {diff} pixels");
    // Пресет Low: тени выключены совсем.
    let mut low = shadow_scene(Some(SoftShadows::default()));
    low.world.resources.insert(GraphicsSettings::preset(QualityLevel::Low));
    let (_, stats) = capture(&low, &view);
    assert_eq!(stats.shadow_draw_calls, 0, "{stats:?}");
    // Medium: не больше двух каскадов.
    let mut medium = shadow_scene(Some(SoftShadows::default()));
    medium.world.resources.insert(GraphicsSettings::preset(QualityLevel::Medium));
    let (_, medium_stats) = capture(&medium, &view);
    let (_, high_stats) = capture(&shadow_scene(Some(SoftShadows::default())), &view);
    assert!(medium_stats.shadow_draw_calls < high_stats.shadow_draw_calls, "{medium_stats:?} vs {high_stats:?}");
}

fn sky_scene(sky: Option<Sky>, sun: Vec3) -> App {
    let mut app = engine::app();
    app.spawn("Environment").with(Environment { bloom: 0.0, vignette: 0.0, ..Default::default() });
    app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, sun.normalize())).with(Light::directional(2.4));
    if let Some(sky) = sky {
        app.spawn("Sky").with(sky);
    }
    app
}

#[test]
fn sky_component_draws_sun_disc_clouds_and_leaves_old_sky_alone() {
    let sun = Vec3::new(0.0, -0.25, 1.0); // низкое солнце впереди (камера смотрит в −Z, солнце светит к +Z)
    let view = CameraView::look_at(Vec3::ZERO, Vec3::new(0.0, 0.25, -1.0), 70.0, W as f32 / H as f32, Color::BLACK, true);
    let to_sun = -sun.normalize();
    let (old, _) = capture(&sky_scene(None, sun), &view);
    let (clear, _) = capture(&sky_scene(Some(Sky { clouds: 0.0, ..Default::default() }), sun), &view);
    // Диск солнца ярче неба рядом с ним.
    let disc = luminance_at(&clear, &view, to_sun * 100.0);
    let beside = luminance_at(&clear, &view, (to_sun + Vec3::new(0.25, 0.1, 0.0)).normalize() * 100.0);
    assert!(disc > beside + 20.0, "sun disc {disc} vs sky {beside}");
    // Рассеяние у низкого солнца: горизонт на стороне солнца теплее (красный > синий).
    // Точка неба чуть выше горизонта в стороне от солнца (не в диске и не под горизонтом).
    let horizon_dir = (Vec3::new(to_sun.x, 0.0, to_sun.z).normalize() + Vec3::new(0.35, 0.06, 0.0)).normalize();
    let horizon = pixel_at(&clear, &view, horizon_dir * 100.0);
    let old_horizon = pixel_at(&old, &view, horizon_dir * 100.0);
    assert!(horizon[0] as i32 - horizon[2] as i32 > old_horizon[0] as i32 - old_horizon[2] as i32, "{horizon:?} vs {old_horizon:?}");
    // Сплошные облака: зенит серее (меньше разница синего и красного).
    let up_view = CameraView::look_at(Vec3::ZERO, Vec3::new(0.0, 1.0, -0.6), 60.0, W as f32 / H as f32, Color::BLACK, true);
    let (blue, _) = capture(&sky_scene(Some(Sky { clouds: 0.0, ..Default::default() }), Vec3::new(0.3, -0.9, 0.2)), &up_view);
    let (cloudy, _) = capture(&sky_scene(Some(Sky { clouds: 1.0, ..Default::default() }), Vec3::new(0.3, -0.9, 0.2)), &up_view);
    let tint = |image: &RgbaImage| {
        let p = image.get_pixel(W / 2, H / 3).0;
        p[2] as i32 - p[0] as i32
    };
    assert!(tint(&cloudy) < tint(&blue) - 10, "clouds cover the blue: {} vs {}", tint(&cloudy), tint(&blue));
    // Выключенный компонент — небо как раньше, до пикселя.
    let (off, _) = capture(&sky_scene(Some(Sky { enabled: false, ..Default::default() }), sun), &view);
    assert!(off == old, "a disabled Sky renders the old sky");
}

#[test]
fn night_sky_has_a_moon_and_stars() {
    let sun = Vec3::new(0.0, 0.5, 1.0); // солнце под горизонтом (светит снизу вверх)
    let view = CameraView::look_at(Vec3::ZERO, Vec3::new(0.0, 0.6, -1.0), 70.0, W as f32 / H as f32, Color::BLACK, true);
    let (night, _) = capture(&sky_scene(Some(Sky { clouds: 0.0, ..Default::default() }), sun), &view);
    let to_moon = sun.normalize();
    let moon = luminance_at(&night, &view, to_moon * 100.0);
    let sky = luminance_at(&night, &view, (to_moon + Vec3::new(0.3, 0.1, 0.0)).normalize() * 100.0);
    assert!(moon > sky + 25.0, "moon {moon} vs night sky {sky}");
    let bright = night.pixels().filter(|p| p.0[0] > 120 && p.0[1] > 120 && p.0[2] > 120).count();
    assert!(bright > 3, "stars are visible: {bright}");
}

/// Земля и куб на ней, солнце сверху-сбоку; камера сверху наискосок. Без блума и
/// виньетки — сравнение пикселей честное.
fn fx_scene(extra: impl FnOnce(&mut App)) -> App {
    let mut app = engine::app();
    app.spawn("Environment").with(Environment { bloom: 0.0, vignette: 0.0, fog_density: 0.0, ..Default::default() });
    app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(0.3, -0.9, 0.3).normalize())).with(Light::directional(2.4));
    app.spawn("Ground").scale(40.0, 1.0, 40.0).with(MeshRenderer::plane().with_color(Color::rgb(0.7, 0.7, 0.7)));
    app.spawn("Box").at3(0.0, 1.0, 0.0).scale(2.0, 2.0, 2.0).with(MeshRenderer::cube().with_color(Color::rgb(0.75, 0.75, 0.75)));
    extra(&mut app);
    app
}

fn fx_view() -> CameraView {
    CameraView { post: None, ..CameraView::look_at(Vec3::new(4.0, 5.0, 6.0), Vec3::new(0.0, 0.5, 0.0), 50.0, W as f32 / H as f32, Color::BLACK, true) }
}

fn mean_luminance(image: &RgbaImage) -> f32 {
    image.pixels().map(|p| 0.2126 * p.0[0] as f32 + 0.7152 * p.0[1] as f32 + 0.0722 * p.0[2] as f32).sum::<f32>() / (W * H) as f32
}

#[test]
fn ambient_occlusion_darkens_the_crease_and_leaves_open_ground() {
    let view = fx_view();
    let (plain, _) = capture(&fx_scene(|_| {}), &view);
    let (ao, _) = capture(&fx_scene(|app| { app.spawn("AO").with(AmbientOcclusion { intensity: 1.0, ..Default::default() }); }), &view);
    // Земля у основания куба (со стороны камеры) — в складке.
    let crease = Vec3::new(1.15, 0.0, 1.15);
    let open = Vec3::new(-6.0, 0.0, 4.0);
    let (c0, c1) = (luminance_at(&plain, &view, crease), luminance_at(&ao, &view, crease));
    let (o0, o1) = (luminance_at(&plain, &view, open), luminance_at(&ao, &view, open));
    assert!(c1 < c0 - 4.0, "the crease gets darker: {c0} -> {c1}");
    assert!((o1 - o0).abs() < 4.0, "open ground stays: {o0} -> {o1}");
    // Пресет Low затенение не делает.
    let mut low = fx_scene(|app| { app.spawn("AO").with(AmbientOcclusion::default()); });
    low.world.resources.insert(GraphicsSettings { shadows: true, ..GraphicsSettings::preset(QualityLevel::Low) });
    let mut low_plain = fx_scene(|_| {});
    low_plain.world.resources.insert(GraphicsSettings { shadows: true, ..GraphicsSettings::preset(QualityLevel::Low) });
    assert!(capture(&low, &view).0 == capture(&low_plain, &view).0, "Low skips ambient occlusion");
    // Выключенный компонент — кадр как без него.
    let (off, _) = capture(&fx_scene(|app| { app.spawn("AO").with(AmbientOcclusion { enabled: false, ..Default::default() }); }), &view);
    assert!(off == plain);
}

#[test]
fn outline_draws_dark_edges_and_fxaa_softens_them() {
    let view = fx_view();
    let (plain, _) = capture(&fx_scene(|_| {}), &view);
    let outline = |app: &mut App| { app.spawn("Outline").with(Outline::default()); };
    let (lined, _) = capture(&fx_scene(outline), &view);
    // Пиксели линии: заметно темнее, чем без контура.
    let lum = |p: &image::Rgba<u8>| 0.2126 * p.0[0] as f32 + 0.7152 * p.0[1] as f32 + 0.0722 * p.0[2] as f32;
    let darker: Vec<f32> = plain.pixels().zip(lined.pixels()).map(|(a, b)| lum(a) - lum(b)).filter(|d| *d > 4.0).collect();
    assert!(darker.len() > 100, "outline pixels: {}", darker.len());
    // Сглаженная линия: много промежуточных значений, а не только «полностью линия / нет».
    let full = darker.iter().cloned().fold(0.0f32, f32::max);
    let partial = darker.iter().filter(|d| **d < full * 0.6).count();
    assert!(partial * 4 > darker.len(), "anti-aliased edge pixels: {partial} of {} (max darkening {full})", darker.len());
    // Пол под скользящим углом без объектов: ни одной линии (горизонт тоже не обводится).
    let floor_view = CameraView { post: None, ..CameraView::look_at(Vec3::new(0.0, 0.6, 15.0), Vec3::new(0.0, 0.0, -30.0), 50.0, W as f32 / H as f32, Color::BLACK, true) };
    let bare = |extra: bool| {
        let mut app = engine::app();
        app.spawn("Environment").with(Environment { bloom: 0.0, vignette: 0.0, fog_density: 0.0, ..Default::default() });
        app.spawn("Ground").scale(400.0, 1.0, 400.0).with(MeshRenderer::plane().with_color(Color::rgb(0.6, 0.6, 0.6)));
        if extra {
            app.spawn("Outline").with(Outline::default());
        }
        capture(&app, &floor_view).0
    };
    let (floor, floor_lined) = (bare(false), bare(true));
    let lines = floor.pixels().zip(floor_lined.pixels()).filter(|(a, b)| lum(a) - lum(b) > 4.0).count();
    assert!(lines < 20, "a flat floor at a grazing angle gets no outline: {lines} pixels");
    // Середина грани куба и открытая земля — без линий.
    let face = Vec3::new(0.0, 1.0, 1.0);
    assert!((luminance_at(&lined, &view, face) - luminance_at(&plain, &view, face)).abs() < 3.0);
    // FXAA меняет только края: пиксели меняются, средняя яркость почти та же.
    let (smooth, _) = capture(&fx_scene(|app| { outline(app); app.spawn("Post").with(PostQuality::default()); }), &view);
    let changed = lined.pixels().zip(smooth.pixels()).filter(|(a, b)| a.0 != b.0).count();
    assert!(changed > 50 && changed < (W * H / 4) as usize, "FXAA touches edges only: {changed}");
    assert!((mean_luminance(&smooth) - mean_luminance(&lined)).abs() < 2.0);
}

#[test]
fn render_scale_keeps_the_frame_size_and_the_picture() {
    let view = fx_view();
    let (full, _) = capture(&fx_scene(|_| {}), &view);
    let mut half = fx_scene(|_| {});
    half.world.resources.insert(GraphicsSettings { render_scale: 0.5, ..GraphicsSettings::default() });
    let (scaled, _) = capture(&half, &view);
    assert_eq!(scaled.dimensions(), full.dimensions());
    assert!((mean_luminance(&scaled) - mean_luminance(&full)).abs() < 3.0, "{} vs {}", mean_luminance(&scaled), mean_luminance(&full));
    assert!(scaled != full, "the scene was rendered at a lower resolution");
}

// ---------------------------------------------------------------------------
// Лампы: прожекторы, много ламп, тени ламп, заполняющий свет (lights.rs)
// ---------------------------------------------------------------------------

/// Тёмная сцена (без солнца: в ней есть лампы) — земля 60×60, камера сверху наискосок.
fn lamp_scene(extra: impl FnOnce(&mut App)) -> App {
    let mut app = engine::app();
    app.spawn("Environment").with(Environment { bloom: 0.0, vignette: 0.0, fog_density: 0.0, ambient: 0.15, ..Default::default() });
    app.spawn("Ground").scale(60.0, 1.0, 60.0).with(MeshRenderer::plane().with_color(Color::rgb(0.7, 0.7, 0.7)));
    extra(&mut app);
    app
}

fn lamp_view() -> CameraView {
    CameraView { post: None, ..CameraView::look_at(Vec3::new(0.0, 14.0, 12.0), Vec3::new(0.0, 0.0, -2.0), 60.0, W as f32 / H as f32, Color::BLACK, false) }
}

#[test]
fn a_spot_light_lights_its_cone_only() {
    let view = lamp_view();
    let lamp = |app: &mut App, spot: bool| {
        // Над точкой (0, 0, 0), смотрит вниз (ось −Z объекта → вниз).
        let mut lamp = app.spawn("Lamp").at3(0.0, 5.0, 0.0).rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        lamp = lamp.with(Light::point(Color::WHITE, 30.0, 14.0));
        if spot {
            lamp.with(SpotLight { angle: 40.0, inner_angle: 25.0 });
        }
    };
    let (point, _) = capture(&lamp_scene(|app| lamp(app, false)), &view);
    let (spot, _) = capture(&lamp_scene(|app| lamp(app, true)), &view);
    let under = Vec3::ZERO;
    let aside = Vec3::new(5.0, 0.0, 0.0); // за краем конуса (tg 20° · 5 м ≈ 1.8 м)
    let (p_under, s_under) = (luminance_at(&point, &view, under), luminance_at(&spot, &view, under));
    let (p_aside, s_aside) = (luminance_at(&point, &view, aside), luminance_at(&spot, &view, aside));
    assert!((s_under - p_under).abs() < 6.0, "inside the cone a spot is as bright as the point light: {p_under} vs {s_under}");
    assert!(p_aside > s_aside + 15.0, "outside the cone it is dark: point {p_aside}, spot {s_aside}");
}

#[test]
fn many_lights_light_a_surface_with_more_than_four_lamps() {
    let view = lamp_view();
    let lamps = |app: &mut App| {
        for i in 0..10 {
            app.spawn("Lamp").at3(-13.5 + i as f32 * 3.0, 1.0, -2.0).with(Light::point(Color::rgb(1.0, 0.8, 0.5), 8.0, 4.0));
        }
    };
    let (four, _) = capture(&lamp_scene(lamps), &view);
    let (many, _) = capture(&lamp_scene(|app| { lamps(app); app.spawn("Lights").with(ManyLights::default()); }), &view);
    // Под каждой лампой земля освещена; без компонента — только у четырёх ближних к камере.
    let lit = |image: &RgbaImage| {
        (0..10).filter(|i| luminance_at(image, &view, Vec3::new(-13.5 + *i as f32 * 3.0, 0.0, -2.0)) > 60.0).count()
    };
    assert!(lit(&four) <= 5, "four nearest lamps without ManyLights: {}", lit(&four));
    assert_eq!(lit(&many), 10, "every lamp lights the ground with ManyLights");
    // Пресет Low: не больше 16 ламп — все 10 на месте; выключенный компонент — как без него.
    let (off, _) = capture(&lamp_scene(|app| { lamps(app); app.spawn("Lights").with(ManyLights { enabled: false, ..Default::default() }); }), &view);
    assert!(off == four, "a disabled ManyLights renders exactly like no component");
}

#[test]
fn the_lamp_variant_matches_the_old_four_lamp_lighting() {
    // Три лампы: без компонентов — старый путь (uniform), с ManyLights — кластеры. Кадр тот же.
    let view = lamp_view();
    let lamps = |app: &mut App| {
        for (i, x) in [-4.0f32, 0.0, 4.0].iter().enumerate() {
            app.spawn("Lamp").at3(*x, 1.5, -1.0 - i as f32).with(Light::point(Color::rgb(0.9, 0.7, 1.0), 10.0, 6.0));
        }
    };
    let (old, _) = capture(&lamp_scene(lamps), &view);
    let (new, _) = capture(&lamp_scene(|app| { lamps(app); app.spawn("Lights").with(ManyLights::default()); }), &view);
    let worst = old.pixels().zip(new.pixels()).map(|(a, b)| (0..3).map(|c| (a.0[c] as i32 - b.0[c] as i32).abs()).max().unwrap()).max().unwrap();
    assert!(worst <= 2, "clustered lights look like the uniform four: max channel difference {worst}");
}

#[test]
fn a_point_light_casts_shadows_with_light_shadows() {
    let view = lamp_view();
    let scene = |shadows: Option<LightShadows>, settings: Option<GraphicsSettings>| {
        let mut app = lamp_scene(|app| {
            app.spawn("Box").at3(0.0, 1.0, 0.0).scale(2.0, 2.0, 2.0).with(MeshRenderer::cube().with_color(Color::rgb(0.8, 0.3, 0.2)));
            let lamp = app.spawn("Lamp").at3(-3.0, 2.0, 0.0).with(Light::point(Color::WHITE, 30.0, 14.0));
            if let Some(s) = shadows {
                lamp.with(s);
            }
        });
        if let Some(s) = settings {
            app.world.resources.insert(s);
        }
        capture(&app, &view)
    };
    let (lit, _) = scene(None, None);
    let (shadowed, stats) = scene(Some(LightShadows::default()), None);
    // Земля за кубом (со стороны от лампы) — в тени.
    let behind = Vec3::new(3.0, 0.0, 0.0);
    let front = Vec3::new(-3.0, 0.0, 2.5);
    let (l0, l1) = (luminance_at(&lit, &view, behind), luminance_at(&shadowed, &view, behind));
    assert!(l1 < l0 * 0.6, "the box shadows the ground behind it: {l0} -> {l1}");
    let (f0, f1) = (luminance_at(&lit, &view, front), luminance_at(&shadowed, &view, front));
    assert!((f0 - f1).abs() < 6.0, "open ground in front of the lamp keeps its light: {f0} -> {f1}");
    assert!(stats.shadow_draw_calls >= 1, "{stats:?}");
    // Пресет Low — без теней ламп.
    let (low, low_stats) = scene(Some(LightShadows::default()), Some(GraphicsSettings::preset(QualityLevel::Low)));
    assert_eq!(low_stats.shadow_draw_calls, 0, "{low_stats:?}");
    assert!(luminance_at(&low, &view, behind) > l1 + 10.0);
}

#[test]
fn fill_light_tints_the_ambient_and_is_off_without_the_component() {
    let view = fx_view();
    let sun_scene = |fill: Option<FillLight>| capture(&fx_scene(|app| { if let Some(f) = fill { app.spawn("Fill").with(f); } }), &view).0;
    let plain = sun_scene(None);
    let warm = sun_scene(Some(FillLight { sky_tint: 0.0, sky_color: Color::rgb(1.0, 0.5, 0.2), intensity: 2.0, ..Default::default() }));
    // Грань куба в тени (освещена только ambient): с тёплым заполняющим светом краснее.
    let shaded = Vec3::new(1.0, 1.0, 0.0);
    let (a, b) = (pixel_at(&plain, &view, shaded), pixel_at(&warm, &view, shaded));
    assert!(b[0] as i32 - b[2] as i32 > a[0] as i32 - a[2] as i32 + 10, "{a:?} -> {b:?}");
    let off = sun_scene(Some(FillLight { enabled: false, ..Default::default() }));
    assert!(off == plain, "a disabled FillLight renders exactly like no component");
}

// ---------------------------------------------------------------------------
// Вода с эффектами (water_fx.rs)
// ---------------------------------------------------------------------------

/// Пруд: дно на нуле, вода на 1.5 м, под водой отмель (верх на 1.3 м), из воды торчит
/// серый столб, за прудом — высокая жёлтая стена (её отражение видно в воде).
fn pond(fx: Option<WaterEffects>, quality: Option<QualityLevel>) -> App {
    let mut app = engine::app();
    app.spawn("Environment").with(Environment { bloom: 0.0, vignette: 0.0, fog_density: 0.0, ..Default::default() });
    app.spawn("Sun").rotation(Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(0.2, -0.8, -0.5).normalize())).with(Light { shadows: false, ..Light::directional(2.4) });
    app.spawn("Floor").scale(40.0, 1.0, 40.0).with(MeshRenderer::plane().with_color(Color::rgb(0.55, 0.5, 0.4)));
    app.spawn("Shelf").at3(-2.0, 0.65, 2.0).scale(3.0, 1.3, 3.0).with(MeshRenderer::cube().with_color(Color::rgb(0.55, 0.5, 0.4)));
    app.spawn("Post").at3(2.5, 1.5, 2.0).scale(0.6, 3.0, 0.6).with(MeshRenderer::cube().with_color(Color::rgb(0.5, 0.5, 0.5)));
    app.spawn("Wall").at3(0.0, 4.0, -9.0).scale(16.0, 8.0, 1.0).with(MeshRenderer::cube().with_color(Color::rgb(1.0, 0.85, 0.1)));
    let water = app.spawn("Pond").at3(0.0, 1.5, 0.0).with(Water { size: 16.0, ..Default::default() });
    if let Some(fx) = fx {
        water.with(fx);
    }
    if let Some(q) = quality {
        app.world.resources.insert(GraphicsSettings::preset(q));
    }
    app
}

fn pond_view() -> CameraView {
    CameraView { post: None, ..CameraView::look_at(Vec3::new(0.0, 5.0, 10.0), Vec3::new(0.0, 0.0, 0.0), 60.0, W as f32 / H as f32, Color::BLACK, true) }
}

#[test]
fn water_effects_show_the_bottom_foam_and_reflections() {
    let view = pond_view();
    let (plain, _) = capture(&pond(None, None), &view);
    let (fx, _) = capture(&pond(Some(WaterEffects::default()), None), &view);
    // Цвет по глубине: над отмелью (20 см воды) светлее, чем над дном (1.5 м); без эффектов
    // вода везде почти одна.
    let shelf = Vec3::new(-2.0, 1.5, 2.0);
    let deep = Vec3::new(-2.0, 1.5, 5.5);
    let contrast = |image: &RgbaImage| luminance_at(image, &view, shelf) - luminance_at(image, &view, deep);
    let (a, b) = (contrast(&plain), contrast(&fx));
    assert!(b > a + 15.0, "shallow water over the shelf is lighter than deep water: contrast {a} -> {b}");
    // Пена у столба: вода вплотную к нему светлее, чем без эффектов.
    let at_post = Vec3::new(2.5, 1.5, 2.38);
    let (p0, p1) = (luminance_at(&plain, &view, at_post), luminance_at(&fx, &view, at_post));
    assert!(p1 > p0 + 15.0, "foam where the post meets the water: {p0} -> {p1}");
    // Отражение жёлтой стены в воде у дальнего края пруда (пресет High).
    let mirror = Vec3::new(0.0, 1.5, -6.5);
    let yellow = |p: [u8; 4]| (p[0] as i32 + p[1] as i32) / 2 - p[2] as i32;
    let mut medium = pond(Some(WaterEffects::default()), Some(QualityLevel::Medium));
    let (med, _) = capture(&medium, &view);
    medium.world.resources.insert(GraphicsSettings::preset(QualityLevel::High));
    let (high, _) = capture(&medium, &view);
    let (m, h) = (pixel_at(&med, &view, mirror), pixel_at(&high, &view, mirror));
    assert!(yellow(h) > yellow(m) + 10, "High reflects the wall in the water: {m:?} -> {h:?}");
    // Low: только волны и блик — кадр под водой не читается, плиты не видно.
    let (low, _) = capture(&pond(Some(WaterEffects::default()), Some(QualityLevel::Low)), &view);
    assert!(contrast(&low) < b - 10.0, "Low does not read the scene under water: {} vs {b}", contrast(&low));
}

#[test]
fn stylized_water_and_a_disabled_component() {
    let view = pond_view();
    let (plain, _) = capture(&pond(None, None), &view);
    let (off, _) = capture(&pond(Some(WaterEffects { enabled: false, ..Default::default() }), None), &view);
    assert!(off == plain, "a disabled WaterEffects renders exactly like no component");
    let (flat, _) = capture(&pond(Some(WaterEffects { stylized: true, ..Default::default() }), None), &view);
    let (smooth, _) = capture(&pond(Some(WaterEffects::default()), None), &view);
    let changed = flat.pixels().zip(smooth.pixels()).filter(|(a, b)| a.0 != b.0).count();
    assert!(changed > 2000, "the stylized mode looks different: {changed} pixels");
}
