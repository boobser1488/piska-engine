//! Предпросмотр 3D-моделей в окне Project: миниатюры плиток и строк, большой вид справа
//! с орбитой. Всё идёт через настоящий GPU (`capture_with_input`), поэтому каждый тест
//! берёт общий замок: несколько устройств wgpu одновременно на этом драйвере ломаются.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use engine::editor::{capture_with_input, egui, Action, BottomTab, Editor, EditorConfig, Panel};
use image::RgbaImage;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const BROKEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/broken.obj");
const SIZE: (u32, u32) = (1440, 860);
/// Модели каталога `models` в демо-ассетах.
const MODELS: [&str; 4] = ["models/crystal.obj", "models/torus.obj", "models/vase.gltf", "models/stairs.glb"];

fn gpu_lock() -> MutexGuard<'static, ()> {
    static GPU: Mutex<()> = Mutex::new(());
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn editor_for(assets: &Path) -> Editor {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    Editor::new(app, EditorConfig::default()).expect("the editor starts")
}

/// Сколько моделей лежит в каталоге `models` демо-ассетов: у каждой своя миниатюра.
/// (Считается по файлам: набор образцов растёт, тесты не должны знать его наизусть.)
fn model_count() -> u64 {
    std::fs::read_dir(Path::new(ASSETS).join("models"))
        .expect("the models folder exists")
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            [".obj", ".gltf", ".glb", ".pmodel"].iter().any(|ext| name.ends_with(ext))
        })
        .count() as u64
}

/// Редактор на демо-ассетах с открытым каталогом `models`.
fn models_editor() -> Editor {
    let mut editor = editor_for(Path::new(ASSETS));
    editor.select_asset("models/crystal.obj");
    // Окно Project на всю область дока: ему хватает места и плиткам, и панели сведений.
    editor.maximize_panel(Some(Panel::Project));
    editor
}

fn run(editor: &mut Editor, frames: u32, input: impl FnMut(u32, &mut Editor) -> Vec<egui::Event>) -> RgbaImage {
    capture_with_input(editor, SIZE.0, SIZE.1, frames, input).expect("the editor renders")
}

/// Временный каталог ассетов; удаляется вместе со значением.
struct TempAssets(PathBuf);

impl TempAssets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("piska_previews_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn copy(&self, from: &str, to: &str) {
        std::fs::copy(from, self.0.join(to)).unwrap();
    }
}

impl Drop for TempAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn crop(image: &RgbaImage, rect: egui::Rect) -> RgbaImage {
    image::imageops::crop_imm(image, rect.min.x as u32, rect.min.y as u32, rect.width() as u32, rect.height() as u32).to_image()
}

/// Часть плитки, где лежит миниатюра (как её рисует окно Project).
fn thumb_rect(tile: egui::Rect, size: f32) -> egui::Rect {
    egui::Rect::from_min_size(tile.min + egui::vec2(8.0, 8.0), egui::vec2(size - 16.0, size - 16.0))
}

fn unique_colors(image: &RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels() {
        seen.insert([pixel[0] >> 2, pixel[1] >> 2, pixel[2] >> 2]);
    }
    seen.len()
}

/// Доля пикселей, заметно отличных от фона (угла картинки): сколько занимает модель.
fn covered(image: &RgbaImage) -> f32 {
    let corner = image.get_pixel(6, 6).0;
    let far = image
        .pixels()
        .filter(|p| (0..3).map(|c| (p[c] as i32 - corner[c] as i32).abs()).sum::<i32>() > 60)
        .count();
    far as f32 / (image.width() * image.height()) as f32
}

fn mean_difference(a: &RgbaImage, b: &RgbaImage) -> f32 {
    assert_eq!(a.dimensions(), b.dimensions());
    let total: u64 = a
        .pixels()
        .zip(b.pixels())
        .map(|(p, q)| (0..3).map(|c| (p[c] as i32 - q[c] as i32).unsigned_abs() as u64).sum::<u64>())
        .sum();
    total as f32 / (a.width() * a.height() * 3) as f32
}

fn tile_size(editor: &mut Editor, size: f32) -> f32 {
    editor.set_project_tile_size(size);
    size
}

fn pointer(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(pos)
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

/// Тянуть мышью внутри большого вида: кадры `start..` — наведение, нажатие, пять шагов, отпускание.
fn drag_in_preview(frame: u32, start: u32, editor: &Editor, delta: egui::Vec2) -> Vec<egui::Event> {
    let Some(rect) = editor.project_preview_rect() else {
        return Vec::new();
    };
    let from = rect.center();
    match frame.checked_sub(start) {
        Some(0) => vec![pointer(from)],
        Some(1) => vec![button(from, true)],
        Some(step @ 2..=6) => vec![pointer(from + delta * ((step - 1) as f32 / 5.0))],
        Some(7) => vec![button(from + delta, false)],
        _ => Vec::new(),
    }
}

#[test]
fn mesh_tiles_show_lit_thumbnails_instead_of_the_icon() {
    let _gpu = gpu_lock();
    let mut editor = models_editor();
    let tile = tile_size(&mut editor, 120.0);
    let image = run(&mut editor, 40, |_, _| Vec::new());

    let mut crops = Vec::new();
    for path in MODELS {
        let rect = editor.project_entry_rect(path).unwrap_or_else(|| panic!("`{path}` has a tile"));
        let picture = crop(&image, thumb_rect(rect, tile));
        let colors = unique_colors(&picture);
        let area = covered(&picture);
        assert!(colors > 150, "`{path}`: only {colors} colours, that is a flat icon, not a lit model");
        assert!((0.05..0.9).contains(&area), "`{path}`: the model covers {area:.2} of the tile");
        crops.push(picture);
    }
    for a in 0..crops.len() {
        for b in a + 1..crops.len() {
            let difference = mean_difference(&crops[a], &crops[b]);
            assert!(difference > 3.0, "`{}` and `{}` look the same ({difference:.2})", MODELS[a], MODELS[b]);
        }
    }
    let stats = editor.preview_stats();
    assert_eq!(stats.thumbnail_renders, model_count(), "one render per visible model");
    assert_eq!(stats.failures, 0);
}

#[test]
fn a_cached_thumbnail_costs_nothing_on_later_frames() {
    let _gpu = gpu_lock();
    let mut editor = models_editor();
    let mut early = None;
    let mut late = None;
    run(&mut editor, 70, |frame, editor| {
        if frame == 25 {
            early = Some(editor.preview_stats().thumbnail_renders);
        }
        if frame == 68 {
            late = Some(editor.preview_stats().thumbnail_renders);
        }
        Vec::new()
    });
    assert_eq!(early, Some(model_count()));
    assert_eq!(late, early, "thumbnails were drawn again although nothing changed");
}

#[test]
fn a_changed_model_file_is_rendered_again() {
    let _gpu = gpu_lock();
    let dir = TempAssets::new("changed");
    dir.copy(&format!("{ASSETS}/models/crystal.obj"), "model.obj");
    let tile = 120.0;

    let mut before = editor_for(&dir.0);
    before.set_project_tile_size(tile);
    let still = run(&mut before, 30, |_, _| Vec::new());
    assert_eq!(before.preview_stats().thumbnail_renders, 1);
    let rect = thumb_rect(before.project_entry_rect("model.obj").expect("the model has a tile"), tile);

    let mut editor = editor_for(&dir.0);
    editor.set_project_tile_size(tile);
    let mut counts = Vec::new();
    let path = dir.0.join("model.obj");
    let changed = run(&mut editor, 60, |frame, editor| {
        if frame == 25 {
            counts.push(editor.preview_stats().thumbnail_renders);
            // Другая модель — куб — в тот же файл, затем пересканирование, как после Ctrl+R.
            std::fs::write(&path, "o cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
                f 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n").unwrap();
            editor.apply(Action::RefreshAssets);
        }
        if frame == 59 {
            counts.push(editor.preview_stats().thumbnail_renders);
        }
        Vec::new()
    });
    assert_eq!(counts, [1, 2], "the model was drawn once, then once more after its file changed");
    let difference = mean_difference(&crop(&still, rect), &crop(&changed, rect));
    assert!(difference > 3.0, "the thumbnail still shows the old model ({difference:.2})");
}

#[test]
fn a_broken_model_gets_the_icon_and_one_warning() {
    let _gpu = gpu_lock();
    engine::init_logger();
    let mark = engine::core::logging::records().last().map_or(0, |r| r.seq + 1);
    let dir = TempAssets::new("broken");
    dir.copy(BROKEN, "broken.obj");
    dir.copy(&format!("{ASSETS}/models/crystal.obj"), "good.obj");
    let tile = 120.0;

    let mut editor = editor_for(&dir.0);
    editor.set_project_tile_size(tile);
    editor.select_asset("broken.obj");
    let mut failures = Vec::new();
    let image = run(&mut editor, 60, |frame, editor| {
        if frame == 20 || frame == 58 {
            failures.push(editor.preview_stats().failures);
        }
        Vec::new()
    });
    // Две попытки (плитка и большой вид) — но предупреждение одно, и дальше без повторов.
    let warnings: Vec<_> = engine::core::logging::records_since(mark)
        .into_iter()
        .filter(|r| r.message.contains("cannot preview") && r.message.contains("broken.obj"))
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].message.contains("out of range"), "the warning says why: {}", warnings[0].message);
    assert_eq!(failures.first(), failures.last(), "no endless retries: {failures:?}");
    let stats = editor.preview_stats();
    assert_eq!(stats.thumbnail_renders, 1, "the good model next to it is still drawn");

    let good = crop(&image, thumb_rect(editor.project_entry_rect("good.obj").unwrap(), tile));
    let bad = crop(&image, thumb_rect(editor.project_entry_rect("broken.obj").unwrap(), tile));
    assert!(unique_colors(&good) > 150);
    assert!(unique_colors(&bad) < 120, "the broken model shows a flat icon, got {} colours", unique_colors(&bad));
}

#[test]
fn dragging_the_details_preview_orbits_the_model() {
    let _gpu = gpu_lock();
    // Два вида одной модели, повёрнутой мышью на разное расстояние.
    let mut images = Vec::new();
    let mut detail_renders = Vec::new();
    for delta in [60.0, 150.0] {
        let mut editor = models_editor();
        editor.select_asset("models/torus.obj");
        let mut counts = Vec::new();
        let image = run(&mut editor, 70, |frame, editor| {
            if frame == 50 || frame == 69 {
                counts.push(editor.preview_stats().detail_renders);
            }
            drag_in_preview(frame, 30, editor, egui::vec2(delta, 0.0))
        });
        let rect = editor.project_preview_rect().expect("a selected model shows the big preview");
        assert!(rect.width() > 150.0 && rect.height() > 120.0, "the preview is large: {rect:?}");
        let picture = crop(&image, rect.shrink(4.0));
        assert!(unique_colors(&picture) > 300, "the big preview shows a lit model");
        images.push(picture);
        // Мышь взяла вид: вращение само остановилось, перерисовок больше нет.
        assert_eq!(counts[0], counts[1], "the preview was redrawn without any change: {counts:?}");
        detail_renders.push(counts[0]);
    }
    let difference = mean_difference(&images[0], &images[1]);
    assert!(difference > 2.0, "different drags give the same picture ({difference:.2})");
}

#[test]
fn the_big_preview_turns_by_itself_until_it_is_touched() {
    let _gpu = gpu_lock();
    let mut editor = models_editor();
    let mut counts = Vec::new();
    run(&mut editor, 80, |frame, editor| {
        if matches!(frame, 30 | 50) {
            counts.push(editor.preview_stats().detail_renders);
        }
        // Колесо над видом — «коснулись»: приближение и конец автовращения.
        match (frame, editor.project_preview_rect()) {
            (55, Some(rect)) => vec![pointer(rect.center())],
            (56, Some(rect)) => vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 40.0),
                modifiers: egui::Modifiers::NONE,
            }]
            .into_iter()
            .chain([pointer(rect.center())])
            .collect(),
            (79, _) => {
                counts.push(editor.preview_stats().detail_renders);
                Vec::new()
            }
            (70, _) => {
                counts.push(editor.preview_stats().detail_renders);
                Vec::new()
            }
            _ => Vec::new(),
        }
    });
    assert!(counts[1] > counts[0] + 3, "the preview does not rotate by itself: {counts:?}");
    assert!(counts[2] > counts[1], "the wheel changes the zoom, so it redraws: {counts:?}");
    assert_eq!(counts[2..].len(), 2);
    assert_eq!(counts[2], counts[3], "after the wheel the auto-rotation is over and nothing is redrawn: {counts:?}");
}

#[test]
fn nothing_is_rendered_while_the_project_window_is_hidden() {
    let _gpu = gpu_lock();
    let mut editor = models_editor();
    editor.set_bottom_tab(BottomTab::Console);
    run(&mut editor, 30, |_, _| Vec::new());
    let stats = editor.preview_stats();
    assert_eq!((stats.thumbnail_renders, stats.detail_renders), (0, 0), "{stats:?}");

    // Окно открыли — модели нарисовались.
    let mut editor = models_editor();
    let mut at_open = None;
    editor.set_bottom_tab(BottomTab::Console);
    run(&mut editor, 40, |frame, editor| {
        if frame == 15 {
            at_open = Some(editor.preview_stats().thumbnail_renders);
            editor.set_bottom_tab(BottomTab::Project);
        }
        Vec::new()
    });
    assert_eq!(at_open, Some(0));
    // В обычной раскладке окно Project невысокое: рисуются только плитки, которые видны.
    let rendered = editor.preview_stats().thumbnail_renders;
    assert!((1..=model_count()).contains(&rendered), "visible model tiles are rendered once the tab opens: {rendered}");
}

#[test]
fn a_folder_with_many_models_draws_only_what_is_on_screen() {
    let _gpu = gpu_lock();
    let dir = TempAssets::new("many");
    for index in 0..120 {
        dir.copy(&format!("{ASSETS}/models/crystal.obj"), &format!("m{index:03}.obj"));
    }
    let mut editor = editor_for(&dir.0);
    editor.maximize_panel(Some(Panel::Project));
    editor.set_project_tile_size(160.0);
    let mut early = 0;
    run(&mut editor, 60, |frame, editor| {
        if frame == 3 {
            early = editor.preview_stats().thumbnail_renders;
        }
        Vec::new()
    });
    let drawn = editor.preview_stats().thumbnail_renders;
    assert!(early <= 3 * 4, "the window must stay responsive: {early} thumbnails in the first frames");
    assert!(drawn > 8, "the visible tiles are drawn: {drawn}");
    assert!(drawn < 60, "tiles below the fold must not be drawn: {drawn} of 120");
    let last_visible = (0..120).filter(|i| editor.project_entry_rect(&format!("m{i:03}.obj")).is_some()).count();
    assert!(last_visible >= 8 && last_visible < 60, "only some tiles are on screen: {last_visible}");
}

#[test]
fn the_list_view_shows_thumbnails_too() {
    let _gpu = gpu_lock();
    let mut editor = models_editor();
    editor.set_project_list_view(true);
    let image = run(&mut editor, 40, |_, _| Vec::new());
    assert_eq!(editor.preview_stats().thumbnail_renders, model_count());
    let row = editor.project_entry_rect("models/torus.obj").expect("the row is on screen");
    let icon = egui::Rect::from_min_size(row.min + egui::vec2(6.0, 4.0), egui::vec2(20.0, 18.0));
    assert!(unique_colors(&crop(&image, icon)) > 25, "the row shows a rendered model, not a flat icon");
}

/// Не тест, а съёмка кадров для просмотра глазами: `PREVIEW_SHOTS=<каталог> cargo test
/// --test editor_previews gallery -- --ignored`.
#[test]
#[ignore]
fn gallery() {
    let _gpu = gpu_lock();
    let dir = PathBuf::from(std::env::var("PREVIEW_SHOTS").expect("set PREVIEW_SHOTS to a directory"));
    std::fs::create_dir_all(&dir).unwrap();
    let save = |name: &str, image: &RgbaImage| image.save(dir.join(name)).unwrap();

    for (name, size) in [("tiles_88", 88.0), ("tiles_150", 150.0)] {
        let mut editor = models_editor();
        editor.set_project_tile_size(size);
        save(&format!("{name}.png"), &run(&mut editor, 50, |_, _| Vec::new()));
        eprintln!("{name}: {:?}", editor.preview_stats());
    }
    let mut editor = models_editor();
    editor.set_project_list_view(true);
    save("list.png", &run(&mut editor, 50, |_, _| Vec::new()));

    let mut editor = models_editor();
    editor.select_asset("models/torus.obj");
    save("selected_torus.png", &run(&mut editor, 60, |_, _| Vec::new()));

    let mut editor = models_editor();
    editor.select_asset("models/vase.gltf");
    save(
        "selected_vase_dragged.png",
        &run(&mut editor, 80, |frame, editor| drag_in_preview(frame, 40, editor, egui::vec2(-90.0, 50.0))),
    );
    let mut editor = models_editor();
    editor.select_asset("models/stairs.glb");
    save("selected_stairs.png", &run(&mut editor, 60, |_, _| Vec::new()));
}
