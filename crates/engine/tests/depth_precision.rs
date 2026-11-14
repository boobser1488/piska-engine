//! Точность глубины вдали: две большие параллельные плиты (ближняя красная, дальняя
//! синяя) с зазором 5 мм … 5 см на расстоянии 20 … 800 м, камера как в шутере от первого
//! лица (`near` 0.04, `far` 900). Ближняя плита должна закрывать дальнюю целиком, и
//! сдвиг камеры на 1 мм не должен перекрашивать пиксели (мерцание «наложения текстур»).
//! Плюс: свои матрицы с обычной глубиной рисуют то же, что и с обратной.

use engine::prelude::*;
use engine_render::{CameraView, PostSettings, Renderer};

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

const W: u32 = 320;
const H: u32 = 180;
const GAPS: [f32; 4] = [0.005, 0.01, 0.02, 0.05];
const DISTANCES: [f32; 6] = [20.0, 50.0, 100.0, 200.0, 400.0, 800.0];

/// Плита, повёрнутая к камере под углом `turn` (рад) вокруг вертикали; `offset` —
/// сдвиг назад вдоль её нормали. Наклонная плита даёт разную глубину по кадру — как
/// стена улицы или мостовая вдали.
fn slab(app: &mut App, name: &str, z: f32, size: f32, color: Color) -> GameObjectId {
    slab_at(app, name, z, 0.0, 0.0, size, color)
}

fn slab_at(app: &mut App, name: &str, z: f32, turn: f32, offset: f32, size: f32, color: Color) -> GameObjectId {
    let rotation = Quat::from_rotation_y(turn) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let normal = rotation * Vec3::Y;
    let at = Vec3::new(0.0, 0.0, z) - normal * offset;
    app.spawn(name)
        .at3(at.x, at.y, at.z)
        .rotation(rotation)
        .scale(size, 1.0, size)
        .with(MeshRenderer::plane().with_color(color).without_shadows())
        .id()
}

/// Пиксели в средней части кадра (ближняя плита занимает весь кадр): сколько синих
/// (видна дальняя плита) и сколько поменяли цвет после сдвига камеры на 1 мм.
fn measure(renderer: &mut Renderer, app: &mut App, camera: GameObjectId) -> (usize, usize) {
    let set = |app: &mut App, offset: Vec3| app.world.scene.get_mut(camera).unwrap().transform.position = offset;
    set(app, Vec3::ZERO);
    let a = renderer.capture(&app.world, W, H).unwrap();
    set(app, Vec3::new(0.001, 0.001, 0.0));
    let b = renderer.capture(&app.world, W, H).unwrap();
    set(app, Vec3::ZERO);
    let inside = |x: u32, y: u32| x > W / 8 && x < W * 7 / 8 && y > H / 8 && y < H * 7 / 8;
    let (mut wrong, mut flips) = (0, 0);
    for (x, y, p) in a.enumerate_pixels() {
        if !inside(x, y) {
            continue;
        }
        // Синяя плита: синий канал много больше красного, зелёный — как красный
        // (у неба зелёный заметно выше).
        if p[2] as i32 > p[0] as i32 + 60 && (p[1] as i32 - p[0] as i32).abs() < 30 {
            wrong += 1;
        }
        let q = b.get_pixel(x, y);
        if (0..3).any(|c| p[c].abs_diff(q[c]) > 40) {
            flips += 1;
        }
    }
    (wrong, flips)
}

#[test]
fn near_slab_hides_the_far_one_at_any_distance() {
    let _gpu = gpu_lock();
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = engine::app();
    app.spawn("Fog").with(Fog::off());
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.3, -0.6, 0.0)).with(Light { shadows: false, ..Light::default() });
    let camera = app
        .spawn("Camera")
        .with(Camera { near: 0.04, far: 900.0, ..Camera::perspective(60.0) })
        .id();
    let mut table = Vec::new();
    let mut failures = Vec::new();
    for &distance in &DISTANCES {
        // Плита во весь кадр с запасом.
        let size = distance * 5.0;
        let turn = 1.0f32; // ≈ 57°: глубина меняется по кадру
        let mut row = format!("{distance:>5} m:");
        for &gap in &GAPS {
            // Дальняя — первой: при равной глубине (точности не хватило) выиграла бы она.
            let far = slab_at(&mut app, "Far", -distance, turn, gap, size, Color::rgb(0.1, 0.1, 0.9));
            let near = slab_at(&mut app, "Near", -distance, turn, 0.0, size, Color::rgb(0.9, 0.1, 0.1));
            let (wrong, flips) = measure(&mut renderer, &mut app, camera);
            row += &format!("  {:>3} mm: {wrong:>5} wrong {flips:>5} flips", (gap * 1000.0).round());
            if wrong + flips > 0 {
                failures.push(format!("gap {gap} m at {distance} m: {wrong} pixels of the far slab, {flips} flips"));
            }
            app.world.scene.destroy(near);
            app.world.scene.destroy(far);
            app.world.flush_destroyed();
        }
        table.push(row);
    }
    println!("{}", table.join("\n"));
    assert!(failures.is_empty(), "depth precision: {failures:#?}");
}

/// Вид со своей матрицей: обычная глубина (`Mat4::perspective_rh`) и обратная
/// (`engine_render::perspective`) рисуют одно и то же, и ближнее закрывает дальнее.
/// Если бы рендерер принял обычную матрицу за обратную, победила бы дальняя плита.
#[test]
fn custom_matrices_with_either_depth_convention_render_the_same() {
    let _gpu = gpu_lock();
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut renderer = Renderer::headless().expect("GPU");
    let mut app = engine::app();
    app.spawn("Fog").with(Fog::off());
    slab(&mut app, "Far", -12.0, 60.0, Color::rgb(0.1, 0.1, 0.9));
    slab(&mut app, "Near", -10.0, 30.0, Color::rgb(0.9, 0.1, 0.1));
    let eye = Vec3::ZERO;
    let look = Mat4::look_at_rh(eye, Vec3::NEG_Z, Vec3::Y);
    let view = |projection: Mat4| CameraView {
        view_proj: projection * look,
        position: eye,
        clear_color: Color::BLACK,
        sky: false,
        post: Some(PostSettings::OFF),
        culling_mask: u32::MAX,
        overlay: false,
        grid: None,
        fog: false,
    };
    let aspect = W as f32 / H as f32;
    let (standard, _) = renderer.capture_view(&app.world, W, H, Some(&view(Mat4::perspective_rh(1.0, aspect, 0.1, 100.0)))).unwrap();
    let (reversed, _) = renderer.capture_view(&app.world, W, H, Some(&view(engine_render::perspective(1.0, aspect, 0.1, 100.0)))).unwrap();
    let centre = standard.get_pixel(W / 2, H / 2);
    assert!(centre[0] > 150 && centre[2] < 80, "the near (red) slab is in front: {centre:?}");
    let differ = standard.pixels().zip(reversed.pixels()).filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 3)).count();
    assert_eq!(differ, 0, "standard and reversed custom matrices give the same frame");
    // Камера сцены (матрица строится движком) — то же самое.
    app.spawn("Camera").with(Camera { near: 0.1, far: 100.0, ..Camera::perspective(1f32.to_degrees()) });
    let scene = renderer.capture(&app.world, W, H).unwrap();
    let p = scene.get_pixel(W / 2, H / 2);
    assert!(p[0] > 150 && p[2] < 80, "scene camera: near slab in front: {p:?}");
}
