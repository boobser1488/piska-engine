//! Витрина движка: низкополигональные сцены (остров на закате, ночная поляна,
//! мастерская-диорама, долина с тысячами деревьев) из набора моделей, построенного кодом.
//!
//! * `cargo run --example showcase` — облёт сцен в окне: 1…4 — сцена, пробел — пауза
//!   камеры, Esc — выход. Звука нет.
//! * `cargo run --example showcase -- --generate` — заново построить набор (палитра,
//!   материалы, модели `.obj`) и сцены `assets/scenes/showcase/*.scene` (одинаковые байты
//!   при каждом запуске).
//! * `cargo run --example showcase -- --editor island` — открыть сцену в редакторе.
//! * Снимок без окна: `SHOWCASE_SCENE=island SHOWCASE_SHOT=hero ENGINE_SCREENSHOT=out.png
//!   cargo run --example showcase` (`SHOWCASE_SIZE=1920x1080`, `SHOWCASE_SS=2` —
//!   сверхвыборка: кадр вдвое больше и уменьшается фильтром Ланцоша).
//! * `cargo run --example showcase -- --render docs/renders [--size 1920x1080] [--ss 2]
//!   [--shots island:hero,clearing:hero]` — снимки закладок и `contact.png` одним запуском.

mod geo;
mod ground;
mod kit;
mod palette;
mod scenes;
mod tour;

use std::path::{Path, PathBuf};
use std::time::Instant;

use engine::prelude::*;
use engine::render::Renderer;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Сцены витрины в порядке облёта.
pub const SCENES: [&str; 4] = ["island", "clearing", "workshop", "valley"];

/// Главные снимки (закладки) для `--render` по умолчанию.
pub const HERO_SHOTS: &[(&str, &str)] = &[
    ("island", "hero"),
    ("island", "windmill"),
    ("island", "village"),
    ("clearing", "hero"),
    ("clearing", "fox"),
    ("clearing", "owl"),
    ("workshop", "hero"),
    ("workshop", "robot"),
    ("valley", "hero"),
    ("valley", "high"),
];

fn main() -> Result<(), EngineError> {
    // Витрина всегда без звука.
    std::env::set_var("ENGINE_AUDIO", "off");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = PathBuf::from(format!("{ROOT}/assets"));

    if args.iter().any(|a| a == "--generate") {
        let written = generate(&assets).map_err(|e| EngineError::Image { path: assets.display().to_string(), reason: e.to_string() })?;
        println!("showcase: wrote {written} files under {}", assets.display());
        return Ok(());
    }
    if args.iter().any(|a| a == "--kit") {
        // Таблица набора для docs/api/showcase.md.
        for m in kit::all().into_iter().chain(ground::all()) {
            println!("| `{}` | {} | {} | {} |", m.name, m.theme.name(), m.triangles(), m.budget);
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--render") {
        let out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_else(|| format!("{ROOT}/docs/renders")));
        let size = flag(&args, "--size").map_or((1920, 1080), |s| parse_size(&s));
        let ss = flag(&args, "--ss").and_then(|s| s.parse().ok()).unwrap_or(2u32);
        let shots: Vec<(String, String)> = match flag(&args, "--shots") {
            Some(list) => list.split(',').filter_map(|s| s.split_once(':')).map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            None => HERO_SHOTS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        };
        return render_all(&assets, &out, &shots, size, ss, args.iter().any(|a| a == "--contact"));
    }
    if let Some(i) = args.iter().position(|a| a == "--editor") {
        let name = args.get(i + 1).cloned().unwrap_or_else(|| "island".to_string());
        let mut app = engine::app();
        tour::register(&mut app);
        engine::mount_assets(&mut app, &assets)?;
        return engine::edit(app, EditorConfig::titled("Piska Showcase").open_scene(&scenes::scene_path(&name)));
    }
    if let (Ok(scene), Ok(path)) = (std::env::var("SHOWCASE_SCENE"), std::env::var("ENGINE_SCREENSHOT")) {
        let shot = std::env::var("SHOWCASE_SHOT").unwrap_or_else(|_| "hero".to_string());
        let size = std::env::var("SHOWCASE_SIZE").map_or((1920, 1080), |s| parse_size(&s));
        let ss = std::env::var("SHOWCASE_SS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
        let mut renderer = Renderer::headless()?;
        let shot = render_shot(&mut renderer, &assets, &scene, &shot, size, ss)?;
        save_png(&shot.image, Path::new(&path))?;
        println!("{}", shot.summary(&scene));
        return Ok(());
    }

    let mut app = engine::app();
    tour::register(&mut app);
    engine::mount_assets(&mut app, &assets)?;
    app.add_subsystem(tour::Tour::new(SCENES.iter().map(|s| scenes::scene_path(s)).collect()));
    engine::run(app, WindowConfig::titled("Piska Engine — Showcase").with_size(1600, 900))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn parse_size(text: &str) -> (u32, u32) {
    text.split_once('x')
        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        .unwrap_or((1920, 1080))
}

// ---------------------------------------------------------------------
// Генерация набора и сцен
// ---------------------------------------------------------------------

fn meta(guid: &str, importer: &str, nearest: bool) -> String {
    if nearest {
        format!("(\n    guid: \"{guid}\",\n    importer: \"{importer}\",\n    settings: {{\n        \"filter\": Enum(\"Nearest\"),\n    }},\n)")
    } else {
        format!("(\n    guid: \"{guid}\",\n    importer: \"{importer}\",\n)")
    }
}

fn material_text(m: kit::Mat) -> String {
    let palette = scenes::guid(scenes::PALETTE);
    let body = match m {
        kit::Mat::Kit => format!("    base_color: (1.0, 1.0, 1.0, 1.0),\n    base_color_texture: \"{palette}\",\n    roughness: 0.85,\n"),
        kit::Mat::Glow => format!(
            "    base_color: (1.0, 1.0, 1.0, 1.0),\n    base_color_texture: \"{palette}\",\n    roughness: 0.6,\n    \
             emissive: (1.0, 1.0, 1.0),\n    emissive_texture: \"{palette}\",\n    emissive_intensity: 4.0,\n    cast_shadows: false,\n"
        ),
        kit::Mat::Metal => format!("    base_color: (1.0, 1.0, 1.0, 1.0),\n    base_color_texture: \"{palette}\",\n    metallic: 0.6,\n    roughness: 0.38,\n"),
        kit::Mat::Glass => format!(
            "    base_color: (0.8, 0.9, 1.0, 0.3),\n    base_color_texture: \"{palette}\",\n    roughness: 0.05,\n    alpha_mode: Blend,\n"
        ),
    };
    format!(
        "// Material (Piska Engine). Textures are referenced by GUID.\n// Showcase kit material `{}`: generated by `cargo run --example showcase -- --generate`.\n(\n{body})\n",
        m.name()
    )
}

/// Все файлы набора и сцен: (путь от корня ассетов, байты).
pub fn files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |path: String, bytes: Vec<u8>, importer: &str, nearest: bool| {
        let guid = scenes::guid(&path);
        out.push((format!("{path}.meta"), meta(&guid, importer, nearest).into_bytes()));
        out.push((path, bytes));
    };
    add(scenes::PALETTE.to_string(), palette::png(), "Texture", true);
    for m in kit::Mat::ALL {
        add(scenes::material_path(m), material_text(m).into_bytes(), "Material", false);
    }
    for model in kit::all().into_iter().chain(ground::all()) {
        add(scenes::model_path(model.name), model.to_obj().into_bytes(), "Mesh", true);
    }
    let kit_prefabs = scenes::Kit::new();
    for model in kit::all().into_iter().chain(ground::all()) {
        let text = format!(
            "// Showcase kit prefab `{}`. Generated by `cargo run --example showcase -- --generate`.\n{}",
            model.name,
            scenes::prefab_text(&kit_prefabs.prefab(model.name))
        );
        add(scenes::prefab_path(model.name), text.into_bytes(), "Prefab", false);
    }
    for (name, data) in scenes::all() {
        let text = format!(
            "// Piska Engine showcase scene. Generated by `cargo run --example showcase -- --generate`;\n\
             // safe to edit in the editor (the next --generate overwrites it).\n{}",
            scenes::scene_text(&data)
        );
        add(scenes::scene_path(name), text.into_bytes(), "Scene", false);
    }
    out
}

fn generate(assets: &Path) -> std::io::Result<usize> {
    let files = files();
    for (path, bytes) in &files {
        let full = assets.join(path);
        if let Some(dir) = full.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(full, bytes)?;
    }
    Ok(files.len())
}

// ---------------------------------------------------------------------
// Снимки
// ---------------------------------------------------------------------

pub struct Shot {
    pub image: image::RgbaImage,
    pub stats: RenderStats,
    /// Время кадра в итоговом разрешении (рендер + чтение в память), мс.
    pub frame_ms: f32,
    pub objects: usize,
}

impl Shot {
    pub fn summary(&self, name: &str) -> String {
        format!(
            "{name}: {}x{} | {:.1} ms/frame | draw calls {} (shadow {}) | meshes {} | triangles {} | culled {} | objects {}",
            self.image.width(),
            self.image.height(),
            self.frame_ms,
            self.stats.draw_calls,
            self.stats.shadow_draw_calls,
            self.stats.meshes,
            self.stats.triangles,
            self.stats.culled_meshes,
            self.objects
        )
    }
}

/// Загрузить сцену витрины в новое приложение и включить закладку `shot`.
pub fn load_showcase(assets: &Path, scene: &str, shot: &str) -> Result<App, EngineError> {
    let mut app = engine::app();
    tour::register(&mut app);
    engine::mount_assets(&mut app, assets)?;
    engine::load_scene(&mut app, &scenes::scene_path(scene))?;
    if !tour::activate_shot(&mut app.world, shot) {
        log::warn!("showcase: scene `{scene}` has no shot `{shot}`; using `hero`");
    }
    Ok(app)
}

/// Снимок закладки: 2 с симуляции (дым, искры, крылья), кадр `size × ss`, уменьшение.
pub fn render_shot(
    renderer: &mut Renderer,
    assets: &Path,
    scene: &str,
    shot: &str,
    size: (u32, u32),
    ss: u32,
) -> Result<Shot, EngineError> {
    let mut app = load_showcase(assets, scene, shot)?;
    let frames = std::env::var("SHOWCASE_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
    app.run_headless(frames, 1.0 / 60.0);
    let ss = ss.clamp(1, 3);
    let (w, h) = (size.0 * ss, size.1 * ss);
    // Первый кадр строит рельеф и грузит ассеты; время — по второму.
    let _ = renderer.capture_view(&app.world, w, h, None)?;
    let clock = Instant::now();
    let (big, stats) = renderer.capture_view(&app.world, w, h, None)?;
    let frame_ms = clock.elapsed().as_secs_f32() * 1000.0;
    let image = if ss > 1 {
        image::imageops::resize(&big, size.0, size.1, image::imageops::FilterType::Lanczos3)
    } else {
        big
    };
    Ok(Shot { image, stats, frame_ms, objects: app.world.scene.len() })
}

fn save_png(image: &image::RgbaImage, path: &Path) -> Result<(), EngineError> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    image.save(path).map_err(|e| EngineError::Image { path: path.display().to_string(), reason: e.to_string() })
}

fn render_all(assets: &Path, out: &Path, shots: &[(String, String)], size: (u32, u32), ss: u32, contact: bool) -> Result<(), EngineError> {
    let mut renderer = Renderer::headless()?;
    let mut thumbs = Vec::new();
    for (scene, shot) in shots {
        let result = render_shot(&mut renderer, assets, scene, shot, size, ss)?;
        let name = format!("{scene}_{shot}_{}x{}", size.0, size.1);
        save_png(&result.image, &out.join(format!("{name}.png")))?;
        println!("{}", result.summary(&name));
        if contact {
            // Вариант для README: 1280 px, JPEG.
            let small = image::imageops::resize(&result.image, 1280, 1280 * size.1 / size.0, image::imageops::FilterType::Lanczos3);
            let path = out.join(format!("{scene}_{shot}.jpg"));
            let rgb = image::DynamicImage::ImageRgba8(small).to_rgb8();
            rgb.save_with_format(&path, image::ImageFormat::Jpeg)
                .map_err(|e| EngineError::Image { path: path.display().to_string(), reason: e.to_string() })?;
        }
        thumbs.push(image::imageops::resize(&result.image, 480, 270, image::imageops::FilterType::Triangle));
    }
    if contact && !thumbs.is_empty() {
        let columns = 3u32;
        let rows = (thumbs.len() as u32).div_ceil(columns);
        let gap = 8;
        let mut sheet = image::RgbaImage::from_pixel(columns * (480 + gap) + gap, rows * (270 + gap) + gap, image::Rgba([24, 26, 30, 255]));
        for (i, t) in thumbs.iter().enumerate() {
            let (c, r) = (i as u32 % columns, i as u32 / columns);
            image::imageops::overlay(&mut sheet, t, (gap + c * (480 + gap)) as i64, (gap + r * (270 + gap)) as i64);
        }
        save_png(&sheet, &out.join("contact.png"))?;
    }
    Ok(())
}

/// Для тестов: (имя, треугольники, бюджет) каждой модели набора и земли.
pub fn kit_models() -> Vec<(&'static str, usize, usize)> {
    kit::all().into_iter().chain(ground::all()).map(|m| (m.name, m.triangles(), m.budget)).collect()
}

/// Для тестов: имена камер-закладок загруженной сцены.
pub fn tour_shots(world: &World) -> Vec<String> {
    tour::shots(world).into_iter().map(|(name, ..)| name).collect()
}
