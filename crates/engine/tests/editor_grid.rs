//! Сетка во вьюпорте редактора рисуется рендером внутри 3D-прохода (`CameraView::grid`):
//! переключатель «Grid» её убирает, модели закрывают её. Нужен GPU (как `features2d.rs`).
//! Картинки для просмотра: `GRID_SHOTS=<папка> cargo test -p engine --test editor_grid`.
#![cfg(feature = "editor")]

use std::path::PathBuf;

use engine::editor::{capture, Editor, EditorConfig, Orbit3D, ViewTab};
use engine::prelude::*;
use image::RgbaImage;

#[path = "../examples/platformer/scripts.rs"]
mod scripts;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SIZE: (u32, u32) = (1280, 800);

/// Тесты с GPU идут по одному (см. features2d.rs).
fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn save(image: &RgbaImage, name: &str) {
    if let Some(dir) = std::env::var_os("GRID_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image.save(dir.join(name)).unwrap();
    }
}

fn px(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

fn differs(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(&b).any(|(x, y)| x.abs_diff(*y) > 1)
}

fn orange(p: [u8; 4]) -> bool {
    p[0] > 100 && p[0] as i32 - p[2] as i32 > 95
}

/// Куб на земле перед камерой сцены; редактор встаёт туда же, где камера.
fn editor_with_cube(camera_at: Vec3) -> Editor {
    let mut app = engine::app();
    let camera = app.spawn("Camera").at3(camera_at.x, camera_at.y, camera_at.z).with(Camera::perspective(50.0)).id();
    app.world.scene.get_mut(camera).unwrap().transform.look_at(Vec3::new(0.0, 0.8, 0.0), Vec3::Y);
    app.spawn("Cube")
        .at3(0.0, 1.0, 0.0)
        .scale(2.0, 2.0, 2.0)
        .with(MeshRenderer::cube().with_color(Color::hex(0xff7a59)));
    app.spawn("Sun").with(Light::default());
    Editor::new(app, EditorConfig::default()).unwrap()
}

#[test]
fn the_grid_toggle_removes_the_grid_and_models_hide_it() {
    let _gpu = gpu_lock();
    let mut editor = editor_with_cube(Vec3::new(5.0, 4.0, 8.0));
    assert!(editor.is_scene_3d());

    let with_grid = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    editor.set_grid_visible(false);
    let without = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    save(&with_grid, "editor_cube_grid_on.png");
    save(&without, "editor_cube_grid_off.png");

    let rect = editor.viewport_rect();
    let (x0, y0) = (rect.min.x as u32, rect.min.y as u32);
    let (x1, y1) = (rect.max.x as u32, rect.max.y as u32);
    assert!(x1 > x0 + 250 && y1 > y0 + 200, "viewport {rect:?}");

    let mut changed = 0;
    let mut cube = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            if differs(px(&with_grid, x, y), px(&without, x, y)) {
                changed += 1;
            }
            // Силуэт куба без сетки, отступив от края: сглаживание смешивает цвета.
            let inside = (-2i32..=2).all(|dy| {
                (-2i32..=2).all(|dx| {
                    let (nx, ny) = ((x as i32 + dx) as u32, (y as i32 + dy) as u32);
                    nx < x1 && ny < y1 && orange(px(&without, nx, ny))
                })
            });
            if inside {
                cube.push((x, y));
            }
        }
    }
    assert!(changed > 800, "the Grid toggle changed only {changed} pixels: no grid in the Scene view");
    assert!(cube.len() > 1500, "cube silhouette is {} pixels", cube.len());
    for &(x, y) in &cube {
        assert!(
            !differs(px(&with_grid, x, y), px(&without, x, y)),
            "the grid shows through the cube at ({x}, {y})"
        );
    }

    // Выключенная сетка остаётся выключенной, включённая возвращается.
    editor.set_grid_visible(true);
    let again = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    let mut mismatched = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if differs(px(&again, x, y), px(&with_grid, x, y)) {
                mismatched += 1;
            }
        }
    }
    assert_eq!(mismatched, 0, "the grid did not come back identically in the Scene view");
}

#[test]
fn the_2d_scene_view_has_a_toggleable_grid_behind_the_sprites() {
    let _gpu = gpu_lock();
    let mut app = engine::app();
    engine::mount_assets(&mut app, ASSETS).unwrap();
    app.spawn("Camera").with(Camera::orthographic(4.0));
    app.spawn("Square").with(SpriteRenderer { color: Color::hex(0x2060ff), ..SpriteRenderer::default() }).scale(3.0, 3.0, 1.0);
    let mut editor = Editor::new(app, EditorConfig::default()).unwrap();
    assert!(!editor.is_scene_3d());
    assert_eq!(editor.view_tab(), ViewTab::Scene);

    let with_grid = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    editor.set_grid_visible(false);
    let without = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    save(&with_grid, "editor_2d_grid_on.png");
    save(&without, "editor_2d_grid_off.png");

    let rect = editor.viewport_rect();
    let center = rect.center();
    let mut changed = 0;
    for y in rect.min.y as u32..rect.max.y as u32 {
        for x in rect.min.x as u32..rect.max.x as u32 {
            if differs(px(&with_grid, x, y), px(&without, x, y)) {
                changed += 1;
            }
        }
    }
    assert!(changed > 400, "no grid in the 2D view ({changed})");
    // Спрайт закрывает сетку: пиксели в его середине те же.
    for dy in -20..=20 {
        for dx in -20..=20 {
            let (x, y) = ((center.x as i32 + dx) as u32, (center.y as i32 + dy) as u32);
            assert!(!differs(px(&with_grid, x, y), px(&without, x, y)), "grid over the sprite at ({x}, {y})");
        }
    }
}

/// Картинки для глаз: демо-сцена с двух высот, крупный план, взгляд из-под земли, 2D.
#[test]
#[ignore = "writes PNG files into GRID_SHOTS"]
fn grid_screenshots_of_the_demo_scene() {
    let _gpu = gpu_lock();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let make = |target: Vec3, yaw: f32, pitch: f32, distance: f32| {
        let mut app = engine::app();
        engine::mount_assets(&mut app, ASSETS).unwrap();
        let mut plugins = engine::cpp::CppPlugins::default();
        engine::cpp::builtin_gameplay(&mut plugins).unwrap();
        plugins.register_components(&mut app.world.registry);
        app.add_subsystem(plugins);
        component! {
            pub struct Orbit {
                pub target: Vec3 = Vec3::ZERO,
                pub distance: f32 = 9.5,
                pub height: f32 = 3.2,
            }
        }
        impl Component for Orbit {}
        app.register::<Orbit>();
        let config = EditorConfig::default().open_scene("scenes/demo3d.scene").code_dir(format!("{root}/plugins/cpp-gameplay"));
        let mut editor = Editor::new(app, config).unwrap();
        editor.set_orbit(Orbit3D { target, yaw, pitch, distance });
        editor
    };
    let shots: [(&str, Vec3, f32, f32, f32); 4] = [
        ("demo_high.png", Vec3::new(0.0, 0.5, 0.0), 0.6, 0.95, 16.0),
        ("demo_low.png", Vec3::new(0.0, 0.8, 0.0), 0.6, 0.18, 14.0),
        ("demo_closeup.png", Vec3::new(0.0, 0.7, 0.0), 0.5, 0.35, 4.5),
        ("demo_below.png", Vec3::new(0.0, 0.0, 0.0), 0.6, -0.5, 14.0),
    ];
    for (name, target, yaw, pitch, distance) in shots {
        let mut editor = make(target, yaw, pitch, distance);
        let image = capture(&mut editor, 1280, 800, 8).unwrap();
        save(&image, name);
    }

    // 2D-вид той же сцены на платформере.
    let mut app = engine::app();
    scripts::register(&mut app);
    engine::mount_assets(&mut app, ASSETS).unwrap();
    let mut platformer = Editor::new(app, EditorConfig::default().open_scene("scenes/level1.scene")).unwrap();
    let image = capture(&mut platformer, 1280, 800, 8).unwrap();
    save(&image, "level1_2d.png");
}
