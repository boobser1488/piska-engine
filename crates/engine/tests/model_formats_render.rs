//! Модель Blockbench в кадре: импорт движка → рендер без окна. Каждая сторона куба своего
//! цвета: с севера (−Z) виден красный, с востока (+X) — зелёный, с юга — синий, с запада —
//! жёлтый, сверху — бирюзовый. И швы атласа (как в `atlas_seams.rs`): две копии модели,
//! отличающиеся только текселями, которых не касается ни одна грань, рисуются одинаково
//! с фильтрами «ближайший» и «линейный».

use std::path::PathBuf;

use engine::prelude::*;
use engine_render::{CameraView, PostSettings, Renderer};
use image::RgbaImage;

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

struct TempAssets(PathBuf);

impl TempAssets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("piska_bbmodel_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Цвета сторон: north, east, south, west, up, down — квадраты 8×8 текстуры 32×16.
const COLORS: [[u8; 3]; 6] = [[220, 40, 40], [40, 200, 40], [40, 60, 220], [230, 210, 40], [40, 210, 210], [210, 40, 210]];
const SIDES: [&str; 6] = ["north", "east", "south", "west", "up", "down"];

fn texture(foreign: [u8; 4]) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(32, 24, image::Rgba(foreign));
    for (k, c) in COLORS.iter().enumerate() {
        let (x0, y0) = ((k % 4) as u32 * 8, (k / 4) as u32 * 8);
        for y in 0..8 {
            for x in 0..8 {
                image.put_pixel(x0 + x, y0 + y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
    image
}

fn bbmodel(image: &RgbaImage) -> String {
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let source = engine_assets::model_formats::png_data_uri(&engine_assets::model_formats::decode_png(&png).unwrap());
    let faces: Vec<String> = SIDES
        .iter()
        .enumerate()
        .map(|(k, side)| {
            let (x0, y0) = ((k % 4) * 8, (k / 4) * 8);
            format!("\"{side}\": {{\"uv\": [{x0}, {y0}, {}, {}], \"texture\": 0}}", x0 + 8, y0 + 8)
        })
        .collect();
    format!(
        "{{\"meta\": {{\"format_version\": \"4.5\", \"model_format\": \"free\", \"box_uv\": false}},
          \"resolution\": {{\"width\": 32, \"height\": 24}},
          \"elements\": [{{\"name\": \"cube\", \"type\": \"cube\", \"uuid\": \"c\", \"from\": [-8, 0, -8], \"to\": [8, 16, 8], \"origin\": [0, 0, 0], \"faces\": {{{}}}}}],
          \"outliner\": [\"c\"],
          \"textures\": [{{\"name\": \"t\", \"uuid\": \"t\", \"source\": \"{source}\"}}]}}",
        faces.join(", ")
    )
}

fn write_model(dir: &std::path::Path, file: &str, foreign: [u8; 4], filter: &str, guid: &str) {
    std::fs::write(dir.join(file), bbmodel(&texture(foreign))).unwrap();
    let meta = format!("(guid: \"{guid}\", importer: \"Mesh\", settings: {{\"filter\": Enum(\"{filter}\")}})");
    std::fs::write(dir.join(format!("{file}.meta")), meta).unwrap();
}

fn view(eye: Vec3) -> CameraView {
    let up_safe = if eye.x == 0.0 && eye.z == 0.0 { eye + Vec3::new(0.0, 0.0, 0.01) } else { eye };
    let mut view = CameraView::look_at(up_safe, Vec3::new(0.0, 0.5, 0.0), 40.0, 1.0, Color::BLACK, false);
    view.post = Some(PostSettings::OFF);
    view.fog = false;
    view
}

fn scene(dir: &std::path::Path, file: &str) -> (App, GameObjectId) {
    let mut app = engine::app();
    engine::mount_assets(&mut app, dir).expect("temp assets open");
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let mesh = db.handle::<engine_assets::Mesh>(file).unwrap();
    let id = app.spawn("Model").with(MeshRenderer::asset(mesh)).id();
    // Свет со всех сторон поровну не нужен: цвет стороны узнаётся по преобладающему каналу.
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.8, 0.0)).with(Light::default());
    (app, id)
}

#[test]
fn each_side_shows_its_colour_from_its_direction() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("sides");
    write_model(&assets.0, "cube.bbmodel", [0, 0, 0, 255], "Nearest", "b0b0000000000000000000000000c0be");
    let (app, _) = scene(&assets.0, "cube.bbmodel");
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    // (откуда смотрим, ожидаемый цвет стороны)
    let cases = [
        (Vec3::new(0.0, 0.5, -3.0), COLORS[0], "north (−Z)"),
        (Vec3::new(3.0, 0.5, 0.0), COLORS[1], "east (+X)"),
        (Vec3::new(0.0, 0.5, 3.0), COLORS[2], "south (+Z)"),
        (Vec3::new(-3.0, 0.5, 0.0), COLORS[3], "west (−X)"),
        (Vec3::new(0.0, 3.5, 0.0), COLORS[4], "up (+Y)"),
    ];
    for (eye, expected, name) in cases {
        let (shot, _) = renderer.capture_view(&app.world, 96, 96, Some(&view(eye))).unwrap();
        let p = shot.get_pixel(48, 48);
        // Свет и окружение меняют яркость (сторона в тени тёмная), но не то, какие каналы
        // у цвета сильные, а какие слабые: сильные каналы цвета стороны ярче слабых.
        let (strong, weak): (Vec<usize>, Vec<usize>) = (0..3).partition(|c| expected[*c] > 150);
        let low = strong.iter().map(|c| p[*c]).min().unwrap();
        let high = weak.iter().map(|c| p[*c]).max().unwrap();
        assert!(low as i32 >= high as i32 + 10, "{name}: centre pixel {p:?}, expected a shade of {expected:?}");
    }
}

#[test]
fn imported_atlas_has_no_seams() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    for (k, filter) in ["Nearest", "Linear"].iter().enumerate() {
        let assets = TempAssets::new(&format!("seams_{filter}"));
        write_model(&assets.0, "a.bbmodel", [255, 0, 255, 255], filter, &format!("5eab00000000000000000000000000a{k}"));
        write_model(&assets.0, "b.bbmodel", [0, 255, 0, 255], filter, &format!("5eab00000000000000000000000000b{k}"));
        let (mut app, id) = scene(&assets.0, "a.bbmodel");
        let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
        let meshes = ["a.bbmodel", "b.bbmodel"].map(|f| db.handle::<engine_assets::Mesh>(f).unwrap());
        let mut differing = 0;
        for i in 0..6 {
            let yaw = 0.5 + i as f32 * 1.13;
            let pitch: f32 = if i % 2 == 0 { 0.45 } else { -0.3 };
            let distance = if i < 4 { 2.6 } else { 12.0 };
            let eye = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos()) * distance + Vec3::new(0.0, 0.5, 0.0);
            let mut shots = Vec::new();
            for mesh in meshes {
                app.world.scene.get_component_mut::<MeshRenderer>(id).unwrap().mesh = mesh;
                shots.push(renderer.capture_view(&app.world, 160, 160, Some(&view(eye))).unwrap().0);
            }
            differing += shots[0].pixels().zip(shots[1].pixels()).filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > 24)).count();
        }
        assert_eq!(differing, 0, "{filter}: pixels took colour from texels no face uses");
    }
}
