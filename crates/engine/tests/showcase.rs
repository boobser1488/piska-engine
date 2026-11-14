//! Витрина (`examples/showcase`): генератор детерминирован и совпадает с файлами в
//! репозитории, модели в бюджете, сцены грузятся с камерами-закладками и рисуются не
//! пустыми; картинки README существуют. Новые компоненты рендера `SkyColors` и
//! `ColorGrading` меняют кадр так, как обещано.

#[path = "../examples/showcase/main.rs"]
#[allow(dead_code)]
mod showcase;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use engine::prelude::*;
use engine::render::Renderer;

/// GPU — по одному тесту за раз (драйвер не любит несколько рендереров сразу).
static GPU: Mutex<()> = Mutex::new(());

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn generator_is_deterministic_and_matches_the_checked_in_files() {
    let a = showcase::files();
    let b = showcase::files();
    assert_eq!(a.len(), b.len());
    for ((pa, ba), (pb, bb)) in a.iter().zip(&b) {
        assert_eq!(pa, pb);
        assert!(ba == bb, "`{pa}` differs between two runs");
    }
    let assets = root().join("assets");
    for (path, bytes) in &a {
        let on_disk = std::fs::read(assets.join(path)).unwrap_or_else(|e| panic!("{path}: {e}; run `cargo run --example showcase -- --generate`"));
        let normalize = |b: &[u8]| String::from_utf8_lossy(b).replace("\r\n", "\n").into_bytes();
        let same = if path.ends_with(".png") { on_disk == *bytes } else { normalize(&on_disk) == normalize(bytes) };
        assert!(same, "`{path}` is out of date; run `cargo run --example showcase -- --generate`");
    }
}

#[test]
fn compact_scene_text_reads_back_as_the_same_scene() {
    for (path, bytes) in showcase::files().iter().filter(|(p, _)| p.ends_with(".scene")) {
        let text = String::from_utf8_lossy(bytes);
        let data = engine::core::serialize::SceneData::from_ron(&text, path).expect("valid scene RON");
        assert!(!data.objects.is_empty(), "{path}");
        assert!(text.contains("prefab: Some(\"showcase/prefabs/"), "{path}: kit objects are prefab instances");
    }
}

#[test]
fn kit_has_forty_models_within_their_budgets() {
    let models = showcase::kit_models();
    assert!(models.len() >= 40, "{} models", models.len());
    for (name, triangles, budget) in models {
        assert!(triangles <= budget, "{name}: {triangles} > {budget}");
    }
}

#[test]
fn every_scene_loads_with_cameras_and_renders_a_non_flat_image() {
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("ENGINE_AUDIO", "off");
    std::env::set_var("SHOWCASE_FRAMES", "10");
    let assets = root().join("assets");
    let mut renderer = Renderer::headless().expect("GPU");
    for scene in showcase::SCENES {
        let app = showcase::load_showcase(&assets, scene, "hero").expect("scene loads");
        let shots = showcase_shots(&app);
        assert!(shots.len() >= 3, "{scene}: {shots:?}");
        assert!(shots.contains(&"hero".to_string()));
        let clock = Instant::now();
        let shot = showcase::render_shot(&mut renderer, &assets, scene, "hero", (320, 180), 1).expect("render");
        let seconds = clock.elapsed().as_secs_f32();
        assert!(seconds < 60.0, "{scene}: {seconds} s");
        assert!(shot.frame_ms < 2000.0, "{scene}: {} ms per frame", shot.frame_ms);
        // Не пустой кадр: заметный разброс яркости.
        let lum: Vec<f32> = shot.image.pixels().map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).collect();
        let mean = lum.iter().sum::<f32>() / lum.len() as f32;
        let var = lum.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / lum.len() as f32;
        assert!(var.sqrt() > 12.0, "{scene}: flat image (sd {})", var.sqrt());
        assert!(shot.stats.triangles > 1000, "{scene}: {:?}", shot.stats);
    }
}

fn showcase_shots(app: &App) -> Vec<String> {
    showcase::tour_shots(&app.world)
}

#[test]
fn readme_gallery_images_exist() {
    let readme = std::fs::read_to_string(root().join("README.md")).unwrap();
    let links: Vec<&str> = readme.match_indices("docs/renders/").map(|(i, _)| {
        let rest = &readme[i..];
        let end = rest.find([')', '"', ' ']).unwrap_or(rest.len());
        &rest[..end]
    }).filter(|l| l.ends_with(".jpg") || l.ends_with(".png")).collect();
    assert!(links.len() >= 4, "README gallery has {} images", links.len());
    for link in links {
        let path = root().join(link);
        let size = std::fs::metadata(&path).unwrap_or_else(|e| panic!("{link}: {e}")).len();
        if link.ends_with(".jpg") {
            assert!(size <= 600 * 1024, "{link}: {size} bytes");
        }
    }
}

/// Сцена для проверки неба и цветокоррекции: только камера, глядящая в небо.
fn sky_world(extra: impl FnOnce(&mut App)) -> App {
    let mut app = engine::app();
    app.spawn("Camera").rotation(Quat::from_rotation_x(0.6)).with(Camera::perspective(60.0));
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.0, -0.8, 0.0)).with(Light::default());
    app.spawn("Env").with(Environment { vignette: 0.0, bloom: 0.0, ..Environment::default() });
    extra(&mut app);
    app
}

fn center(renderer: &mut Renderer, app: &App) -> [f32; 3] {
    let image = renderer.capture(&app.world, 64, 36).expect("capture");
    let p = image.get_pixel(32, 18);
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

#[test]
fn sky_colors_and_color_grading_change_the_frame() {
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let mut renderer = Renderer::headless().expect("GPU");
    let plain = center(&mut renderer, &sky_world(|_| {}));
    assert!(plain[2] > plain[0], "default day sky is blue: {plain:?}");

    let red = center(&mut renderer, &sky_world(|app| {
        app.spawn("Sky").with(SkyColors { zenith: Color::rgb(0.8, 0.1, 0.05), horizon: Color::rgb(0.8, 0.1, 0.05), ..SkyColors::default() });
    }));
    assert!(red[0] > red[2] + 40.0, "SkyColors makes the sky red: {red:?}");

    let grey = center(&mut renderer, &sky_world(|app| {
        app.spawn("Grade").with(ColorGrading { saturation: 0.0, ..ColorGrading::default() });
    }));
    assert!((grey[0] - grey[2]).abs() < 4.0 && (grey[1] - grey[2]).abs() < 4.0, "saturation 0 is grey: {grey:?}");

    let warm = center(&mut renderer, &sky_world(|app| {
        app.spawn("Grade").with(ColorGrading { temperature: 1.0, ..ColorGrading::default() });
    }));
    assert!(warm[0] - warm[2] > plain[0] - plain[2] + 10.0, "warmer: {warm:?} vs {plain:?}");

    let off = center(&mut renderer, &sky_world(|app| {
        app.spawn("Grade").with(ColorGrading { enabled: false, saturation: 0.0, ..ColorGrading::default() });
    }));
    for c in 0..3 {
        assert!((off[c] - plain[c]).abs() < 2.0, "disabled grading changes nothing: {off:?} vs {plain:?}");
    }
}
