//! Модели, собранные в Piska Modeler новыми инструментами (тест `modeler/crates/engine-modeler/
//! tests/modeling_ui.rs`: кувшин — вращение профиля, масштаб петли, вставка, выдавливание
//! вглубь, мазок кистью; тележка — куб, вставка, выдавливание, петлевой разрез, колесо-
//! цилиндр), сохранены в `.pmodel` (`tests/fixtures/props`, их пишет тот тест с
//! `MODELER_PROPS_OUT`). Движок читает их: поверхность замкнута и согласована, нормали
//! наружу, UV внутри атласа, а в кадре виден нарисованный мазок.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use engine::prelude::*;
use engine_render::{CameraView, PostSettings, Renderer};

const PROPS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/props");

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn load(name: &str) -> engine_assets::MeshData {
    let bytes = std::fs::read(Path::new(PROPS).join(format!("{name}.pmodel"))).unwrap();
    engine_assets::load_mesh_bytes(&format!("{name}.pmodel"), &bytes, &|_| None).unwrap()
}

#[test]
fn modeled_props_are_closed_outward_and_uv_sane() {
    for name in ["jar", "cart"] {
        let data = load(name);
        let expected: usize = std::fs::read_to_string(Path::new(PROPS).join(format!("{name}.txt"))).unwrap().trim().parse().unwrap();
        assert_eq!(data.triangle_count(), expected, "{name}: the triangles the Modeler baked");
        assert!(data.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])), "{name}: UVs inside the atlas");
        // Сварить вершины по положению: каждое ребро — у двух треугольников, навстречу.
        let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
        let welded: Vec<u32> = data
            .positions
            .iter()
            .map(|p| {
                let key = p.map(|v| (v * 1e4).round() as i64);
                let next = ids.len() as u32;
                *ids.entry(key).or_insert(next)
            })
            .collect();
        let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
        let mut volume = 0.0f64;
        for t in data.indices.chunks_exact(3) {
            let v = [welded[t[0] as usize], welded[t[1] as usize], welded[t[2] as usize]];
            for k in 0..3 {
                *directed.entry((v[k], v[(k + 1) % 3])).or_default() += 1;
            }
            let p = [t[0], t[1], t[2]].map(|i| Vec3::from(data.positions[i as usize]));
            volume += p[0].dot(p[1].cross(p[2])) as f64 / 6.0;
            // Нормаль вершины совпадает с обходом треугольника (против часовой — наружу).
            let geometric = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let stored = Vec3::from(data.normals[t[0] as usize]);
            assert!(geometric.dot(stored) > 0.99, "{name}: a normal disagrees with its triangle");
        }
        let open = directed.iter().filter(|((a, b), n)| **n != 1 || directed.get(&(*b, *a)) != Some(&1)).count();
        assert_eq!(open, 0, "{name}: every edge has exactly one opposite partner (closed, consistently wound)");
        assert!(volume > 0.0, "{name}: outward faces give a positive volume ({volume})");
    }
}

struct TempAssets(PathBuf);

impl Drop for TempAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_modeled_jar_renders_with_its_painted_stroke() {
    let _gpu = gpu_lock();
    let dir = TempAssets(std::env::temp_dir().join(format!("piska_modeled_props_{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    std::fs::copy(Path::new(PROPS).join("jar.pmodel"), dir.0.join("jar.pmodel")).unwrap();
    std::fs::write(dir.0.join("jar.pmodel.meta"), "(guid: \"7a40000000000000000000000000ba11\", importer: \"Mesh\", settings: {\"filter\": Enum(\"Nearest\")})").unwrap();
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir.0).expect("temp assets open");
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let mesh = db.handle::<engine_assets::Mesh>("jar.pmodel").unwrap();
    app.spawn("Jar").with(MeshRenderer::asset(mesh));
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.8, 0.0)).with(Light::default());
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let center = Vec3::new(0.0, 0.3, 0.0);
    let mut red = 0;
    for eye in [Vec3::new(0.0, 0.35, 1.6), Vec3::new(0.0, 0.35, -1.6), Vec3::new(1.6, 0.35, 0.0), Vec3::new(-1.6, 0.35, 0.0)] {
        let mut view = CameraView::look_at(eye, center, 40.0, 1.0, Color::BLACK, false);
        view.post = Some(PostSettings::OFF);
        view.fog = false;
        let (shot, _) = renderer.capture_view(&app.world, 128, 128, Some(&view)).unwrap();
        let covered = shot.pixels().filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30).count();
        assert!(covered > 128 * 128 / 10, "the jar fills part of the frame from {eye}: {covered} pixels");
        red += shot.pixels().filter(|p| p[0] > 60 && p[0] as u32 > 2 * p[1] as u32 && p[0] as u32 > 2 * p[2] as u32).count();
    }
    assert!(red > 0, "the red stroke painted in the Modeler shows on the jar");
}

/// Куб, собранный в Modeler редактором UV (тест `modeler/crates/engine-modeler/tests/
/// uv_editor_ui.rs`): остров грани, смотрящей на камеру, сдвинут в пустое место атласа
/// (G Y 20), и кистью в 3D-виде нарисован один красный тексель — он попал туда, где теперь
/// UV. Движок читает `.pmodel` и рисует красный тексель на этой грани и больше нигде.
#[test]
fn the_uv_edited_cube_shows_its_marker_only_on_the_face_whose_island_was_moved() {
    let _gpu = gpu_lock();
    let data = load("uvedit");
    let note = std::fs::read_to_string(Path::new(PROPS).join("uvedit.txt")).unwrap();
    let n: Vec<f32> = note.split_whitespace().map(|v| v.parse().unwrap()).collect();
    let marker = [(n[0] + 0.5) / n[2], (n[1] + 0.5) / n[3]];
    // Треугольник, в который попадает тексель метки в атласе, и нормаль его грани.
    let inside = |t: &[u32]| {
        let uv = |i: u32| data.uvs[i as usize];
        let s = |a: [f32; 2], b: [f32; 2]| (a[0] - marker[0]) * (b[1] - marker[1]) - (b[0] - marker[0]) * (a[1] - marker[1]);
        let d = [s(uv(t[0]), uv(t[1])), s(uv(t[1]), uv(t[2])), s(uv(t[2]), uv(t[0]))];
        d.iter().all(|v| *v >= -1e-9) || d.iter().all(|v| *v <= 1e-9)
    };
    let tri: Vec<&[u32]> = data.indices.chunks_exact(3).filter(|t| inside(t)).collect();
    assert!(!tri.is_empty(), "a triangle covers the marker texel in the atlas");
    let normal = Vec3::from(data.normals[tri[0][0] as usize]);
    assert!(tri.iter().all(|t| Vec3::from(data.normals[t[0] as usize]).dot(normal) > 0.99), "one face owns the marker");
    let (min, max) = data.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(Vec3::from(*p)), b.max(Vec3::from(*p))));
    let center = (min + max) * 0.5;

    let dir = std::env::temp_dir().join(format!("piska_uvedit_{}", std::process::id()));
    let dir = TempAssets(dir);
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    std::fs::copy(Path::new(PROPS).join("uvedit.pmodel"), dir.0.join("uvedit.pmodel")).unwrap();
    std::fs::write(dir.0.join("uvedit.pmodel.meta"), "(guid: \"7a40000000000000000000000000ba12\", importer: \"Mesh\", settings: {\"filter\": Enum(\"Nearest\")})").unwrap();
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir.0).expect("temp assets open");
    let db = app.world.resources.get::<AssetDatabase>().unwrap().clone();
    let mesh = db.handle::<engine_assets::Mesh>("uvedit.pmodel").unwrap();
    app.spawn("Cube").with(MeshRenderer::asset(mesh));
    app.spawn("Sun").rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.8, 0.0)).with(Light::default());
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let mut expected_seen = 0;
    for axis in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
        // Сверху и снизу взгляд чуть сдвинут: «вверх» камеры не должен совпасть с осью взгляда.
        let tilt = if axis.y.abs() > 0.5 { Vec3::new(0.0, 0.0, 0.05) } else { Vec3::ZERO };
        let mut view = CameraView::look_at(center + axis * 1.6 + tilt, center, 40.0, 1.0, Color::BLACK, false);
        view.post = Some(PostSettings::OFF);
        view.fog = false;
        let (shot, _) = renderer.capture_view(&app.world, 128, 128, Some(&view)).unwrap();
        let red = shot.pixels().filter(|p| p[0] > 30 && p[0] as u32 > 2 * p[1] as u32 && p[0] as u32 > 2 * p[2] as u32).count();
        if axis.dot(normal) > 0.9 {
            assert!(red > 12, "the marker shows on the face the island belongs to, seen from {axis}: {red} red pixels");
            expected_seen += 1;
        } else {
            assert!(red <= 2, "no marker seen from {axis} (the face it belongs to looks along {normal}): {red} red pixels");
        }
    }
    assert_eq!(expected_seen, 1);
}
