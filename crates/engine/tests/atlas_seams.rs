//! Швы атласов: на рёбрах граней модели и между плитками карты не должно появляться
//! цветов из соседних, не принадлежащих грани областей атласа.
//!
//! Приём: две копии одной модели отличаются только текселями атласа, которых не касается
//! ни один треугольник (в одной они пурпурные, в другой зелёные). Всё остальное —
//! геометрия, UV, освещение, вид — одинаково, поэтому любой отличающийся пиксель взял
//! цвет «чужой» области атласа. Проверяется со сглаживанием MSAA (как всегда в движке),
//! с фильтрами «ближайший» и «линейный», вблизи и издали (уменьшение).

use std::path::{Path, PathBuf};

use engine::prelude::*;
use engine_render::{CameraView, PostSettings, Renderer};
use image::RgbaImage;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

/// Временный каталог ассетов; удаляется вместе со значением.
struct TempAssets(PathBuf);

impl TempAssets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("piska_seams_{name}_{}", std::process::id()));
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

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

/// Модель для движка: только раздел `baked`, как его пишет Piska Modeler.
struct Model {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

fn list<const N: usize>(items: &[[f32; N]]) -> String {
    items.iter().flat_map(|v| v.iter().map(|x| format!("{x:?}"))).collect::<Vec<_>>().join(",")
}

/// Записать `.pmodel` с атласом и `.meta` с фильтром; вернуть путь от корня ассетов.
fn write_pmodel(dir: &Path, name: &str, model: &Model, atlas: &RgbaImage, filter: &str, guid: &str) -> String {
    let indices = model.indices.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let text = format!(
        "(format: \"piska-model\", version: 1, baked: Some((positions: [{}], normals: [{}], uvs: [{}], indices: [{indices}], texture: Some(\"{}\"))))",
        list(&model.positions),
        list(&model.normals),
        list(&model.uvs),
        base64(&png(atlas)),
    );
    let file = format!("{name}.pmodel");
    std::fs::write(dir.join(&file), text).unwrap();
    let meta = format!("(\n    guid: \"{guid}\",\n    importer: \"Mesh\",\n    settings: {{\n        \"filter\": Enum(\"{filter}\"),\n    }},\n)");
    std::fs::write(dir.join(format!("{file}.meta")), meta).unwrap();
    file
}

/// Тексели, которых касается хотя бы один треугольник (по прямоугольнику его UV).
fn used_texels(model: &Model, width: u32, height: u32) -> Vec<bool> {
    let mut used = vec![false; (width * height) as usize];
    for t in model.indices.chunks_exact(3) {
        let uv = [t[0], t[1], t[2]].map(|i| model.uvs[i as usize]);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in uv {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let x0 = ((lo[0] * width as f32 + 1e-3).floor().max(0.0)) as u32;
        let x1 = ((hi[0] * width as f32 - 1e-3).ceil().min(width as f32)) as u32;
        let y0 = ((lo[1] * height as f32 + 1e-3).floor().max(0.0)) as u32;
        let y1 = ((hi[1] * height as f32 - 1e-3).ceil().min(height as f32)) as u32;
        for y in y0..y1.max(y0 + 1).min(height) {
            for x in x0..x1.max(x0 + 1).min(width) {
                used[(y * width + x) as usize] = true;
            }
        }
    }
    used
}

/// Куб 1 м: каждая грань — своя клетка 16×16 атласа 64×64 в шахматном порядке, клетки
/// между ними — «чужие».
fn atlas_cube() -> (Model, RgbaImage) {
    let faces: [([f32; 3], [f32; 3], [f32; 3], [u32; 2], [u8; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0, 0], [220, 40, 40]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [2, 0], [40, 70, 220]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1, 1], [60, 200, 60]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [3, 1], [230, 210, 50]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0, 2], [50, 210, 220]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [2, 2], [200, 200, 200]),
    ];
    let mut model = Model { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), indices: Vec::new() };
    let mut atlas = RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]));
    for (n, u, v, cell, color) in faces {
        let base = model.positions.len() as u32;
        let (x0, y0) = (cell[0] as f32 * 0.25, cell[1] as f32 * 0.25);
        for (su, sv, uv) in [(-1.0, -1.0, [x0, y0 + 0.25]), (1.0, -1.0, [x0 + 0.25, y0 + 0.25]), (1.0, 1.0, [x0 + 0.25, y0]), (-1.0, 1.0, [x0, y0])] {
            model.positions.push([0, 1, 2].map(|k| 0.5 * n[k] + 0.5 * su * u[k] + 0.5 * sv * v[k]));
            model.normals.push(n);
            model.uvs.push(uv);
        }
        model.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        for y in 0..16 {
            for x in 0..16 {
                // Лёгкий узор внутри грани — как у настоящего пиксель-арта.
                let shade = if (x + y) % 5 == 0 { 0.8 } else { 1.0 };
                let c = color.map(|c| (c as f32 * shade) as u8);
                atlas.put_pixel(cell[0] * 16 + x, cell[1] * 16 + y, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
    (model, atlas)
}

/// Модель из ассетов движка (`assets/models/*.pmodel`): меш и атлас.
fn shipped_model(name: &str) -> (Model, RgbaImage) {
    let db = AssetDatabase::open(ASSETS).expect("engine assets");
    let guid = db.guid_of(&format!("models/{name}.pmodel")).expect("model exists");
    let data = db.load_mesh(guid).unwrap();
    let atlas = image::load_from_memory(&db.read_image(guid).unwrap()).unwrap().to_rgba8();
    (Model { positions: data.positions, normals: data.normals, uvs: data.uvs, indices: data.indices }, atlas)
}

/// Две копии атласа: неиспользуемые тексели пурпурные и зелёные.
fn variants(model: &Model, atlas: &RgbaImage) -> [RgbaImage; 2] {
    let (w, h) = atlas.dimensions();
    let used = used_texels(model, w, h);
    [[255, 0, 255, 255], [0, 255, 0, 255]].map(|foreign| {
        let mut copy = atlas.clone();
        for (i, p) in copy.pixels_mut().enumerate() {
            if !used[i] {
                *p = image::Rgba(foreign);
            }
        }
        copy
    })
}

fn bounds(model: &Model) -> (Vec3, f32) {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &model.positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
}

/// Виды: шесть направлений вблизи (модель на весь кадр) и три издали (уменьшение).
fn views(center: Vec3, radius: f32, size: u32) -> Vec<CameraView> {
    let mut out = Vec::new();
    for k in 0..9 {
        let yaw = 0.5 + k as f32 * 1.13;
        let pitch: f32 = if k % 2 == 0 { 0.45 } else { -0.3 };
        let distance = radius * if k < 6 { 2.4 } else { 9.0 };
        let dir = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
        let mut view = CameraView::look_at(center + dir * distance, center, 40.0, 1.0, Color::BLACK, false);
        view.post = Some(PostSettings::OFF);
        view.fog = false;
        let _ = size;
        out.push(view);
    }
    out
}

/// Сколько пикселей различается между копиями с пурпурными и зелёными «чужими» текселями.
fn foreign_pixels(renderer: &mut Renderer, model: &Model, atlas: &RgbaImage, filter: &str, tag: &str) -> (usize, Vec<String>) {
    let assets = TempAssets::new(&format!("{tag}_{filter}"));
    let [a, b] = variants(model, atlas);
    // Свои GUID у каждой пары: рендерер кэширует меши и текстуры по GUID.
    let salt = tag.bytes().fold(filter.len() as u64, |h, c| h.wrapping_mul(31).wrapping_add(c as u64));
    let file_a = write_pmodel(&assets.0, "a", model, &a, filter, &format!("5eab{:012x}{:016x}", salt & 0xffff_ffff_ffff, 1));
    let file_b = write_pmodel(&assets.0, "b", model, &b, filter, &format!("5eab{:012x}{:016x}", salt & 0xffff_ffff_ffff, 2));
    let mut app = engine::app();
    engine::mount_assets(&mut app, &assets.0).expect("temp assets open");
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let id = app.spawn("Model").with(MeshRenderer::asset(db.handle::<engine_assets::Mesh>(&file_a).unwrap())).id();
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.8, 0.0)).with(Light::default());
    let mesh_b = db.handle::<engine_assets::Mesh>(&file_b).unwrap();
    let mesh_a = db.handle::<engine_assets::Mesh>(&file_a).unwrap();
    let (center, radius) = bounds(model);
    let size = 256;
    let mut total = 0;
    let mut notes = Vec::new();
    for (k, view) in views(center, radius, size).iter().enumerate() {
        let mut shots = Vec::new();
        for mesh in [mesh_a, mesh_b] {
            app.world.scene.get_component_mut::<MeshRenderer>(id).unwrap().mesh = mesh;
            shots.push(renderer.capture_view(&app.world, size, size, Some(view)).unwrap().0);
        }
        let differing = shots[0]
            .pixels()
            .zip(shots[1].pixels())
            .filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > 24))
            .count();
        if differing > 0 {
            notes.push(format!("{tag}/{filter} view {k}: {differing}"));
            if let Ok(dir) = std::env::var("RENDER_FIX_SHOTS") {
                shots[0].save(format!("{dir}/seam_{tag}_{filter}_{k}.png")).unwrap();
            }
        }
        total += differing;
    }
    (total, notes)
}

#[test]
fn model_atlases_do_not_bleed_across_face_edges() {
    let _gpu = gpu_lock();
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut renderer = Renderer::headless().expect("GPU");
    let mut cases = vec![("cube".to_string(), atlas_cube())];
    for name in ["rover", "crate", "lantern", "pine"] {
        cases.push((name.to_string(), shipped_model(name)));
    }
    let mut report = Vec::new();
    let mut counts = Vec::new();
    for (name, (model, atlas)) in &cases {
        for filter in ["Nearest", "Linear"] {
            let (total, notes) = foreign_pixels(&mut renderer, model, atlas, filter, name);
            counts.push(format!("{name}/{filter}: {total}"));
            // «Ближайший» фильтр — ни одного чужого пикселя (допуск на округление). У
            // линейного грань уже одного текселя (антенна ровера) всё равно задевает соседа:
            // отступать некуда, без полей в атласе это не лечится — допуск мал (до фикса —
            // тысячи пикселей на модель).
            let allowed = if filter == "Nearest" { 2 } else { 24 };
            if total > allowed {
                report.extend(notes);
            }
        }
    }
    println!("foreign-colour pixels: {counts:?}");
    assert!(report.is_empty(), "pixels coloured by atlas texels outside their face: {counts:?}\n{report:#?}");
}

/// Карта плиток при дробных сдвигах камеры и разных масштабах: между плитками нет ни
/// щелей (фон), ни цветов соседних кадров листа.
#[test]
fn tilemap_has_no_lines_between_tiles_at_fractional_offsets() {
    let _gpu = gpu_lock();
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut renderer = Renderer::headless().expect("GPU");
    let mut report = Vec::new();
    for filter in ["Nearest", "Linear"] {
        let assets = TempAssets::new(&format!("tiles_{filter}"));
        // Лист 4 × 4 кадра по 8 px: кадр 1 серый, все соседние — пурпурные.
        let sheet = RgbaImage::from_fn(32, 32, |x, y| {
            if x < 8 && y < 8 {
                image::Rgba([120, 128, 120, 255])
            } else {
                image::Rgba([255, 0, 255, 255])
            }
        });
        sheet.save(assets.0.join("sheet.png")).unwrap();
        std::fs::write(
            assets.0.join("sheet.png.meta"),
            format!("(\n    guid: \"5eab00000000000000000000000000{}\",\n    importer: \"Texture\",\n    settings: {{\n        \"filter\": Enum(\"{filter}\"),\n    }},\n)", if filter == "Linear" { 11 } else { 10 }),
        )
        .unwrap();
        let mut app = engine::app();
        engine::mount_assets(&mut app, &assets.0).expect("temp assets open");
        let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
        let texture = db.handle::<engine_assets::Texture>("sheet.png").unwrap();
        let rows: Vec<String> = (0..40).map(|_| "60*1".to_string()).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        app.spawn("Map").at(-30.0, -20.0).with(Tilemap::from_rows(&rows).with_tileset(texture, 4, 4));
        let mut bad = 0;
        for step in 0..24 {
            let offset = Vec2::new(0.137 * step as f32, 0.071 * step as f32 + 0.003);
            let half_height = [3.3, 4.0, 5.71, 7.77, 9.03, 12.5][step % 6];
            let view = CameraView::orthographic_2d(offset, half_height, 16.0 / 9.0, Color::BLACK);
            let (image, _) = renderer.capture_view(&app.world, 320, 180, Some(&view)).unwrap();
            let lines = image.pixels().filter(|p| (p[0] as i32 - p[1] as i32).abs() > 20 || p[1] < 100).count();
            if lines > 0 {
                bad += 1;
                report.push(format!("{filter} step {step} (offset {offset}, half height {half_height}): {lines} pixels"));
                if let Ok(dir) = std::env::var("RENDER_FIX_SHOTS") {
                    image.save(format!("{dir}/tiles_{filter}_{step}.png")).unwrap();
                }
            }
        }
        let _ = bad;
    }
    assert!(report.is_empty(), "lines between tiles: {report:#?}");
}
