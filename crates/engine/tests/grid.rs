//! Сетка вида редактора рисуется внутри 3D-прохода: модели закрывают её, игровые камеры
//! (без `grid`) не меняются. Проверка настоящих пикселей через `Renderer::headless()`.

use engine::prelude::*;
use engine_render::{CameraView, Renderer, SceneGrid};
use image::RgbaImage;

const W: u32 = 320;
const H: u32 = 240;
const CLEAR: Color = Color { r: 0.02, g: 0.02, b: 0.03, a: 1.0 };

/// Тесты с GPU идут по одному (см. features2d.rs).
fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn out_dir() -> Option<std::path::PathBuf> {
    // Картинки для просмотра глазами: GRID_SHOTS=<папка>.
    let dir = std::path::PathBuf::from(std::env::var_os("GRID_SHOTS")?);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn save(image: &RgbaImage, name: &str) {
    if let Some(dir) = out_dir() {
        image.save(dir.join(name)).unwrap();
    }
}

/// Куб 2×2×2 на земле (низ на y = 0) в начале координат.
fn cube_scene(ground: bool) -> App {
    let mut app = engine::app();
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.85, 0.0))
        .with(Light::default());
    if ground {
        app.spawn("Ground")
            .scale(100.0, 1.0, 100.0)
            .with(MeshRenderer::plane().with_color(Color::hex(0x8a8f7c)).with_material(0.0, 0.9).without_shadows());
    }
    app.spawn("Cube")
        .at3(0.0, 1.0, 0.0)
        .scale(2.0, 2.0, 2.0)
        .with(MeshRenderer::cube().with_color(Color::hex(0xff7a59)));
    app.tick(1.0 / 60.0);
    app
}

fn view(eye: Vec3, target: Vec3, grid: Option<SceneGrid>) -> CameraView {
    let mut view = CameraView::look_at(eye, target, 60.0, W as f32 / H as f32, CLEAR, false);
    view.grid = grid;
    view
}

fn render(renderer: &mut Renderer, app: &App, view: &CameraView) -> RgbaImage {
    renderer.capture_view(&app.world, W, H, Some(view)).expect("capture").0
}

fn px(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

fn differs(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(&b).any(|(x, y)| x.abs_diff(*y) > 1)
}

fn orange(p: [u8; 4]) -> bool {
    p[0] > 150 && p[0] as i32 - p[2] as i32 > 60
}

/// Пиксели цвета куба, отступив от краёв на `margin` пикселей (у краёв сглаживание
/// смешивает цвета).
fn solid_mask(image: &RgbaImage, margin: u32) -> Vec<(u32, u32)> {
    let covered = |x: u32, y: u32| orange(px(image, x, y));
    let mut out = Vec::new();
    for y in margin..H - margin {
        for x in margin..W - margin {
            let mut inside = true;
            'scan: for dy in -(margin as i32)..=margin as i32 {
                for dx in -(margin as i32)..=margin as i32 {
                    if !covered((x as i32 + dx) as u32, (y as i32 + dy) as u32) {
                        inside = false;
                        break 'scan;
                    }
                }
            }
            if inside {
                out.push((x, y));
            }
        }
    }
    out
}

#[test]
fn a_cube_on_the_ground_hides_the_grid_behind_it() {
    let app = cube_scene(false);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let eye = Vec3::new(4.0, 3.0, 6.0);
    let target = Vec3::new(0.0, 0.8, 0.0);

    let plain = render(&mut renderer, &app, &view(eye, target, None));
    let gridded = render(&mut renderer, &app, &view(eye, target, Some(SceneGrid::ground())));
    save(&plain, "cube_no_grid.png");
    save(&gridded, "cube_grid.png");

    // Силуэт куба: всё, что не фон, без полосы у краёв (там сглаживание смешивает цвета).
    let cube = solid_mask(&plain, 2);
    assert!(cube.len() > 3000, "the cube covers {} pixels", cube.len());
    // Сетка не изменила ни одного пикселя внутри силуэта: сквозь модель её не видно.
    for &(x, y) in &cube {
        assert!(
            !differs(px(&plain, x, y), px(&gridded, x, y)),
            "grid shows through the cube at ({x}, {y}): {:?} vs {:?}",
            px(&plain, x, y),
            px(&gridded, x, y)
        );
    }

    // Зато на пустой земле линии есть: пиксели вне куба изменились.
    let changed_outside = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| differs(px(&plain, x, y), px(&gridded, x, y)))
        .count();
    assert!(changed_outside > 600, "only {changed_outside} grid pixels on the empty ground");

    // Сетка стоит на земле и подходит вплотную к кубу: у основания видны линии
    // по обе стороны от него (значит, куб не закрыт «заплаткой», а закрывает сам).
    let near_cube = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| differs(px(&plain, x, y), px(&gridded, x, y)))
        .filter(|&(x, y)| {
            (-6i32..=6).any(|d| {
                let (nx, ny) = (x as i32 + d, y as i32 + d / 2);
                nx >= 0 && ny >= 0 && (nx as u32) < W && (ny as u32) < H && cube.contains(&(nx as u32, ny as u32))
            })
        })
        .count();
    assert!(near_cube > 20, "grid lines do not reach the cube ({near_cube})");
}

#[test]
fn grid_none_renders_the_same_as_before_and_geometry_in_front_is_untouched() {
    let app = cube_scene(true);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let eye = Vec3::new(4.0, 3.0, 6.0);
    let target = Vec3::new(0.0, 0.8, 0.0);

    // Игровая камера: поле `grid` у неё `None`, и два кадра одинаковы бит в бит.
    let first = render(&mut renderer, &app, &view(eye, target, None));
    let second = render(&mut renderer, &app, &view(eye, target, None));
    assert_eq!(first.as_raw(), second.as_raw(), "rendering is not deterministic without the grid");
    assert!(CameraView::look_at(eye, target, 60.0, 1.0, CLEAR, false).grid.is_none());
    assert!(CameraView::orthographic_2d(Vec2::ZERO, 2.0, 1.0, CLEAR).grid.is_none());

    // С сеткой: куб в целом не тронут, а на сером полу (меш на y = 0) сетка видна —
    // смещение глубины не даёт полу её съесть.
    let gridded = render(&mut renderer, &app, &view(eye, target, Some(SceneGrid::ground())));
    save(&first, "ground_no_grid.png");
    save(&gridded, "ground_grid.png");
    // Куб на сером полу: берём пиксели цвета куба (оранжевые) внутри кадра.
    let mut cube_pixels = 0;
    for (x, y) in solid_mask(&first, 2) {
        cube_pixels += 1;
        assert!(
            !differs(px(&first, x, y), px(&gridded, x, y)),
            "grid changed a cube pixel at ({x}, {y})"
        );
    }
    assert!(cube_pixels > 3000, "{cube_pixels} cube pixels");
    let floor_changed = (150..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| !orange(px(&first, x, y)) && differs(px(&first, x, y), px(&gridded, x, y)))
        .count();
    assert!(floor_changed > 800, "the grid is not visible over the floor mesh ({floor_changed})");
}

#[test]
fn the_grid_is_visible_on_empty_ground_and_from_below() {
    let app = cube_scene(false);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let target = Vec3::new(0.0, 0.0, 0.0);

    let mut count_lines = |eye: Vec3, name: &str| {
        let plain = render(&mut renderer, &app, &view(eye, target, None));
        let gridded = render(&mut renderer, &app, &view(eye, target, Some(SceneGrid::ground())));
        save(&gridded, name);
        let mut strength = 0u32;
        let mut changed = 0;
        for y in 0..H {
            for x in 0..W {
                let (a, b) = (px(&plain, x, y), px(&gridded, x, y));
                if differs(a, b) {
                    changed += 1;
                    strength += (0..3).map(|c| (b[c] as i32 - a[c] as i32).unsigned_abs()).sum::<u32>();
                }
            }
        }
        (changed, strength)
    };
    // Сверху — вид «с высоты», куб заслоняет часть, но линий всё равно много.
    let (above, above_strength) = count_lines(Vec3::new(8.0, 12.0, 14.0), "high_view.png");
    let (low, _) = count_lines(Vec3::new(6.0, 1.5, 9.0), "low_view.png");
    // Снизу земля с обратной стороны: сетка двусторонняя, но тусклее.
    let (below, below_strength) = count_lines(Vec3::new(8.0, -12.0, 14.0), "below_view.png");
    assert!(above > 1500, "{above} grid pixels from above");
    assert!(low > 600, "{low} grid pixels from a low camera");
    assert!(below > 1000, "{below} grid pixels from below");
    assert!(below_strength < above_strength, "the underside is not dimmer ({below_strength} vs {above_strength})");
}

/// Одиночные «искры»: освещённый пиксель, вокруг которого (8 соседей) нет других.
fn isolated_dots(image: &RgbaImage) -> usize {
    let lit = |x: u32, y: u32| differs(px(image, x, y), px(image, 0, 0)) && {
        let p = px(image, x, y);
        let c = px(image, 0, 0);
        (0..3).any(|i| p[i].abs_diff(c[i]) > 6)
    };
    let mut dots = 0;
    for y in 1..H - 1 {
        for x in 1..W - 1 {
            if !lit(x, y) {
                continue;
            }
            let neighbours = (-1i32..=1)
                .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| (dx, dy) != (0, 0) && lit((x as i32 + dx) as u32, (y as i32 + dy) as u32))
                .count();
            if neighbours == 0 {
                dots += 1;
            }
        }
    }
    dots
}

/// Под острым углом сетка остаётся линиями и не рассыпается в ряды точек там, где она
/// переходит на более крупный шаг: толщина линии считается от размера пикселя на
/// плоскости, а не производной от координаты клетки (та рвётся на границе уровней).
#[test]
fn the_grid_has_no_rows_of_dots_where_the_cell_size_changes() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("gpu");
    let mut app = engine::app();
    app.tick(1.0 / 60.0);
    let mut worst = (0usize, String::new());
    for (i, (eye, look)) in [
        (Vec3::new(-640.0, 14.0, 410.0), Vec3::new(300.0, -14.0, -220.0)),
        (Vec3::new(905.0, 6.0, -1220.0), Vec3::new(-180.0, -6.0, 260.0)),
        (Vec3::new(-26.0, 30.0, -82.0), Vec3::new(400.0, -30.0, 90.0)),
        (Vec3::new(2400.0, 45.0, 2400.0), Vec3::new(-500.0, -45.0, -350.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let image = render(&mut renderer, &app, &view(eye, eye + look, Some(SceneGrid::ground())));
        save(&image, &format!("speckles_{i}.png"));
        let dots = isolated_dots(&image);
        if dots > worst.0 {
            worst = (dots, format!("view {i} from {eye:?}"));
        }
    }
    // До исправления — 6–11 точек на вид; после остаются 1–2 пикселя на концах гаснущих линий.
    assert!(worst.0 <= 3, "{} isolated grid pixels in {}", worst.0, worst.1);
}

#[test]
fn the_grid_stays_behind_sprites_in_the_2d_view() {
    let mut app = engine::app();
    app.spawn("Square").at(0.0, 0.0).scale(2.0, 2.0, 1.0).with(SpriteRenderer {
        color: Color::hex(0x2060ff),
        ..SpriteRenderer::default()
    });
    app.tick(1.0 / 60.0);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let mut camera = CameraView::orthographic_2d(Vec2::ZERO, 3.0, W as f32 / H as f32, Color::BLACK);
    let plain = render(&mut renderer, &app, &camera);
    camera.grid = Some(SceneGrid::wall());
    let gridded = render(&mut renderer, &app, &camera);
    save(&gridded, "view2d_grid.png");
    // Центр спрайта: пиксели внутри квадрата те же — сетка за спрайтом.
    let mut inside = 0;
    for y in 100..140 {
        for x in 140..180 {
            inside += 1;
            assert!(!differs(px(&plain, x, y), px(&gridded, x, y)), "grid over the sprite at ({x}, {y})");
        }
    }
    assert_eq!(inside, 1600);
    let changed = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| differs(px(&plain, x, y), px(&gridded, x, y)))
        .count();
    assert!(changed > 400, "no grid in the 2D view ({changed})");
}

/// Цена сетки: кадр 320×240 с readback с сеткой и без. Печатает среднее время.
/// `cargo test -p engine --test grid grid_cost -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, prints timings"]
fn grid_cost() {
    let app = cube_scene(true);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let eye = Vec3::new(4.0, 3.0, 6.0);
    let target = Vec3::new(0.0, 0.8, 0.0);
    for (name, grid) in [("no grid", None), ("grid", Some(SceneGrid::ground())), ("no grid", None), ("grid", Some(SceneGrid::ground()))] {
        let camera = view(eye, target, grid);
        for _ in 0..10 {
            render(&mut renderer, &app, &camera);
        }
        let start = std::time::Instant::now();
        let mut draw_calls = 0;
        for _ in 0..100 {
            draw_calls = renderer.capture_view(&app.world, W, H, Some(&camera)).unwrap().1.draw_calls;
        }
        println!("{name}: {:.3} ms/frame, {draw_calls} draw calls", start.elapsed().as_secs_f64() * 10.0);
    }
}
