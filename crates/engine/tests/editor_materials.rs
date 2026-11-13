//! Материалы в редакторе: миниатюры `.material` в окне Project, редактор материала в
//! инспекторе (правка видна в сцене, Undo её отменяет, файл переписывается), слоты
//! `MeshRenderer`, извлечение материалов из модели. Всё через настоящий GPU, поэтому каждый
//! тест берёт общий замок. Картинки: `MATERIALS_SHOTS=<папка>`.
#![cfg(feature = "editor")]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use engine::editor::{capture, capture_with_input, egui, Action, Editor, EditorConfig, Orbit3D, Panel};
use engine::prelude::*;
use image::RgbaImage;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SIZE: (u32, u32) = (1440, 860);

fn gpu_lock() -> MutexGuard<'static, ()> {
    static GPU: Mutex<()> = Mutex::new(());
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn save(image: &RgbaImage, name: &str) {
    if let Some(dir) = std::env::var_os("MATERIALS_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image.save(dir.join(name)).unwrap();
    }
}

/// Копия демо-ассетов (материалы и модели) во временном каталоге: тесты пишут файлы.
struct TempAssets(PathBuf);

impl TempAssets {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("piska_editor_materials_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["materials", "models"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
            for entry in std::fs::read_dir(Path::new(ASSETS).join(folder)).unwrap().flatten() {
                std::fs::copy(entry.path(), dir.join(folder).join(entry.file_name())).unwrap();
            }
        }
        Self(dir)
    }
}

impl Drop for TempAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app_on(assets: &Path) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = engine::app();
    engine::mount_assets(&mut app, assets).expect("assets directory opens");
    app
}

fn handle<T>(app: &App, path: &str) -> AssetHandle<T> {
    app.world.resources.get::<AssetDatabase>().unwrap().handle(path).unwrap()
}

fn crop(image: &RgbaImage, rect: egui::Rect) -> RgbaImage {
    image::imageops::crop_imm(image, rect.min.x as u32, rect.min.y as u32, rect.width() as u32, rect.height() as u32).to_image()
}

fn unique_colors(image: &RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels() {
        seen.insert([pixel[0] >> 2, pixel[1] >> 2, pixel[2] >> 2]);
    }
    seen.len()
}

fn px(image: &RgbaImage, point: egui::Pos2) -> [u8; 4] {
    image.get_pixel(point.x as u32, point.y as u32).0
}

const SAMPLES: [&str; 6] = [
    "materials/painted_metal.material",
    "materials/rough_stone.material",
    "materials/emissive_lamp.material",
    "materials/glass.material",
    "materials/foliage.material",
    "materials/checker.material",
];

#[test]
fn material_tiles_show_rendered_spheres_and_the_details_view_switches_shape() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("tiles");
    let mut editor = Editor::new(app_on(&assets.0), EditorConfig::default()).expect("the editor starts");
    editor.select_asset("materials/painted_metal.material");
    editor.maximize_panel(Some(Panel::Project));
    editor.set_project_tile_size(120.0);
    let image = capture_with_input(&mut editor, SIZE.0, SIZE.1, 40, |_, _| Vec::new()).unwrap();
    save(&image, "editor_material_tiles.png");

    let mut crops = Vec::new();
    for path in SAMPLES {
        let tile = editor.project_entry_rect(path).unwrap_or_else(|| panic!("`{path}` has a tile"));
        let thumb = egui::Rect::from_min_size(tile.min + egui::vec2(8.0, 8.0), egui::vec2(104.0, 104.0));
        let picture = crop(&image, thumb);
        let colors = unique_colors(&picture);
        assert!(colors > 80, "`{path}`: only {colors} colours, that is a flat icon, not a rendered sphere");
        crops.push(picture);
    }
    for a in 0..crops.len() {
        for b in a + 1..crops.len() {
            assert_ne!(crops[a], crops[b], "`{}` and `{}` look the same", SAMPLES[a], SAMPLES[b]);
        }
    }
    assert!(editor.preview_stats().thumbnail_renders >= SAMPLES.len() as u64);

    // Большой вид выделенного материала есть.
    let preview = editor.project_preview_rect().expect("a selected material has a big preview");
    let sphere = crop(&image, preview);
    assert!(unique_colors(&sphere) > 200, "the big view shows a lit sphere");
}

/// Сцена: камера и шар с материалом `materials/checker.material` прямо перед ней.
fn editor_with_ball(assets: &TempAssets, material: &str) -> (Editor, GameObjectId) {
    let mut app = app_on(&assets.0);
    let camera = app.spawn("Camera").at3(0.0, 0.0, 4.0).with(Camera::perspective(40.0)).id();
    app.world.scene.get_mut(camera).unwrap().transform.look_at(Vec3::ZERO, Vec3::Y);
    app.spawn("Sun").rotation(Quat::from_rotation_x(-0.6)).with(Light::default());
    let material = handle::<Material>(&app, material);
    let ball = app.spawn("Ball").scale(2.0, 2.0, 2.0).with(MeshRenderer::sphere().with_material_asset(material)).id();
    let mut editor = Editor::new(app, EditorConfig::default()).expect("the editor starts");
    // Камера вида Scene смотрит прямо на шар: он в центре вьюпорта.
    editor.set_orbit(Orbit3D { target: Vec3::ZERO, yaw: 0.0, pitch: 0.15, distance: 6.0 });
    (editor, ball)
}

#[test]
fn editing_a_material_in_the_inspector_changes_the_scene_and_undo_restores_it() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("edit");
    let path = "materials/checker.material";
    let (mut editor, _) = editor_with_ball(&assets, path);
    assert!(editor.inspect_material(path));
    // Простой цвет без текстуры — пиксель в центре шара проверяется по цвету.
    assert!(editor.edit_material(|m| {
        m.base_color_texture = AssetHandle::none();
        m.base_color = Color::rgb(0.9, 0.05, 0.05);
        m.unlit = true;
    }));
    let before = capture(&mut editor, SIZE.0, SIZE.1, 8).unwrap();
    save(&before, "editor_material_inspector.png");
    // Рядом с центром: в самом центре — значок солнца (гизмо вида Scene).
    let center = editor.viewport_rect().center() + egui::vec2(-40.0, 30.0);
    let red = px(&before, center);
    assert!(red[0] as i32 - red[2] as i32 > 100 && red[0] as i32 - red[1] as i32 > 100, "the ball is red: {red:?}");
    let file = std::fs::read_to_string(assets.0.join(path)).unwrap();
    assert!(file.contains("unlit: true"), "the edit is saved to the file:\n{file}");

    // Правка из инспектора: синий. Сцена перерисована с новым цветом.
    assert!(editor.edit_material(|m| m.base_color = Color::rgb(0.05, 0.1, 0.9)));
    let after = capture(&mut editor, SIZE.0, SIZE.1, 4).unwrap();
    let blue = px(&after, center);
    assert!(blue[2] as i32 - blue[0] as i32 > 100, "the ball turned blue: {blue:?}");

    // Ctrl+Z, пока открыт материал, отменяет правку материала.
    assert!(editor.can_undo());
    editor.apply(Action::Undo);
    assert_eq!(editor.inspected_material().unwrap().base_color, Color::rgb(0.9, 0.05, 0.05), "undo restores the material");
    let undone = capture(&mut editor, SIZE.0, SIZE.1, 4).unwrap();
    assert_eq!(px(&undone, center), red, "undo restores the red ball");
    assert_eq!(editor.inspected_material().unwrap().base_color, Color::rgb(0.9, 0.05, 0.05));
    let file = std::fs::read_to_string(assets.0.join(path)).unwrap();
    assert!(file.contains("0.9"), "undo rewrites the file too:\n{file}");
    assert!(editor.redo_material());
    assert_eq!(editor.inspected_material().unwrap().base_color, Color::rgb(0.05, 0.1, 0.9));

    // Файл, изменённый снаружи, подхватывается при пересканировании ассетов (окно в фокусе).
    let mut external = Material::from_ron(&std::fs::read_to_string(assets.0.join(path)).unwrap()).unwrap();
    external.base_color = Color::rgb(0.05, 0.9, 0.05);
    std::fs::write(assets.0.join(path), external.to_ron()).unwrap();
    editor.apply(Action::RefreshAssets);
    let reloaded = capture(&mut editor, SIZE.0, SIZE.1, 4).unwrap();
    let green = px(&reloaded, center);
    assert!(green[1] as i32 - green[0] as i32 > 100, "hot reload: {green:?}");
}

#[test]
fn the_inspector_shows_material_slots_and_hides_the_inline_material_once_assigned() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("slots");
    let mut app = app_on(&assets.0);
    let stairs = handle::<Mesh>(&app, "models/stairs.glb");
    let object = app.spawn("Stairs").with(MeshRenderer::asset(stairs)).id();
    app.spawn("Sun").with(Light::default());
    let mut editor = Editor::new(app, EditorConfig::default()).expect("the editor starts");
    editor.apply(Action::Select(Some(object)));
    let image = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    save(&image, "editor_mesh_renderer_legacy.png");
    assert!(editor.inspector_field_rect("MeshRenderer", "color").is_some(), "no materials: the inline material is shown");
    assert!(editor.inspector_field_rect("MeshRenderer", "tint").is_some());

    // Извлечь материалы из модели: два файла рядом с моделью, два слота.
    editor.apply(Action::ExtractMaterials(object));
    let created: Vec<_> = std::fs::read_dir(assets.0.join("models"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("stairs_") && name.ends_with(".material"))
        .collect();
    assert_eq!(created.len(), 2, "{created:?}");
    let renderer = editor.app.world.scene.get(object).unwrap().get::<MeshRenderer>().unwrap().clone();
    assert_eq!(renderer.materials.len(), 2);
    assert_ne!(renderer.materials[0], renderer.materials[1]);
    let image = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    save(&image, "editor_mesh_renderer_materials.png");
    assert!(editor.inspector_field_rect("MeshRenderer", "color").is_none(), "assigned materials hide the inline material");

    // Назначение материала слоту (так делает перетаскивание на объект) и Undo сцены.
    let glass = handle::<Material>(&editor.app, "materials/glass.material");
    editor.apply(Action::AssignMaterial { object, material: glass.guid(), slot: 1 });
    let renderer = editor.app.world.scene.get(object).unwrap().get::<MeshRenderer>().unwrap().clone();
    assert_eq!(renderer.materials[1], glass);
    capture(&mut editor, SIZE.0, SIZE.1, 2).unwrap();
    editor.apply(Action::Undo);
    // Undo восстанавливает сцену из снимка: объект ищется заново по имени.
    let object = editor.app.world.scene.find_by_name("Stairs").expect("the object is still there");
    let renderer = editor.app.world.scene.get(object).unwrap().get::<MeshRenderer>().unwrap().clone();
    assert_ne!(renderer.materials[1], glass, "scene undo reverts the assignment");
}

#[test]
fn create_material_makes_a_file_and_opens_it() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("create");
    let mut editor = Editor::new(app_on(&assets.0), EditorConfig::default()).expect("the editor starts");
    editor.apply(Action::CreateMaterial { folder: "materials".to_string() });
    editor.apply(Action::CreateMaterial { folder: "materials".to_string() });
    assert!(assets.0.join("materials/New Material.material").is_file());
    assert!(assets.0.join("materials/New Material 1.material.meta").is_file());
    let material = editor.inspected_material().expect("the new material is open in the inspector");
    assert_eq!(material.alpha_mode, AlphaMode::Opaque);
    let image = capture(&mut editor, SIZE.0, SIZE.1, 4).unwrap();
    save(&image, "editor_new_material.png");
    // Выделение объекта возвращает инспектор к объекту.
    let id = editor.app.spawn("Thing").id();
    editor.apply(Action::Select(Some(id)));
    assert!(editor.inspected_material().is_none());
}

/// Миниатюра плитки `path` (как её рисует окно Project) из снимка редактора.
fn tile_picture(image: &RgbaImage, editor: &Editor, path: &str) -> RgbaImage {
    let tile = editor.project_entry_rect(path).unwrap_or_else(|| panic!("`{path}` has a tile"));
    crop(image, egui::Rect::from_min_size(tile.min + egui::vec2(8.0, 8.0), egui::vec2(104.0, 104.0)))
}

/// Сколько пикселей явно красные.
fn red_pixels(image: &RgbaImage) -> usize {
    image.pixels().filter(|p| p[0] as i32 - p[1] as i32 > 80 && p[0] as i32 - p[2] as i32 > 80).count()
}

fn missing_texture_warnings(since: u64) -> Vec<String> {
    engine::core::logging::records_since(since)
        .into_iter()
        .filter(|r| r.message.contains("Missing texture"))
        .map(|r| r.message)
        .collect()
}

#[test]
fn material_previews_survive_rescans_and_a_new_gpu() {
    let _gpu = gpu_lock();
    let assets = TempAssets::new("rescan");
    let mut editor = Editor::new(app_on(&assets.0), EditorConfig::default()).expect("the editor starts");
    editor.select_asset("materials/painted_metal.material");
    editor.maximize_panel(Some(Panel::Project));
    editor.set_project_tile_size(120.0);
    let since = engine::core::logging::records().last().map_or(0, |r| r.seq + 1);
    let checker = assets.0.join("materials/checker.material");
    let mut renders_before_edit = 0;
    let mut renders_after_edit = 0;
    // Кадр 20: материал изменили снаружи и пересканировали ассеты; кадр 30: файловая
    // операция (новый файл) и ещё одно пересканирование. Снимок — следующий за ним кадр.
    let image = capture_with_input(&mut editor, SIZE.0, SIZE.1, 31, |frame, editor| {
        if frame == 20 {
            renders_before_edit = editor.preview_stats().thumbnail_renders;
            let mut material = Material::from_ron(&std::fs::read_to_string(&checker).unwrap()).unwrap();
            material.base_color_texture = AssetHandle::none();
            material.base_color = Color::rgb(0.9, 0.05, 0.05);
            std::fs::write(&checker, material.to_ron()).unwrap();
            editor.apply(Action::RefreshAssets);
        }
        if frame == 30 {
            renders_after_edit = editor.preview_stats().thumbnail_renders;
            std::fs::write(assets.0.join("materials/notes.txt"), "a file operation").unwrap();
            editor.apply(Action::RefreshAssets);
        }
        Vec::new()
    })
    .unwrap();
    save(&image, "editor_material_tiles_after_rescan.png");
    for path in SAMPLES {
        let colors = unique_colors(&tile_picture(&image, &editor, path));
        assert!(colors > 80, "`{path}` right after a rescan: {colors} colours — the tile lost its picture");
    }
    let red = red_pixels(&tile_picture(&image, &editor, "materials/checker.material"));
    assert!(red > 1000, "the edited material shows its new colour ({red} red pixels)");
    assert_eq!(renders_after_edit, renders_before_edit + 1, "only the edited material is drawn again");
    assert_eq!(editor.preview_stats().thumbnail_renders, renders_after_edit, "a rescan without changes redraws nothing");
    let big = crop(&image, editor.project_preview_rect().expect("the selected material has a big view"));
    assert!(unique_colors(&big) > 200, "the big view survives the rescan");

    // Новая GPU-часть (новый захват, пересозданное окно): старые id egui ей чужие.
    let image = capture(&mut editor, SIZE.0, SIZE.1, 6).unwrap();
    save(&image, "editor_material_tiles_new_gpu.png");
    for path in SAMPLES {
        let colors = unique_colors(&tile_picture(&image, &editor, path));
        assert!(colors > 80, "`{path}` with a new GPU: {colors} colours");
    }
    let big = crop(&image, editor.project_preview_rect().expect("big view"));
    assert!(unique_colors(&big) > 200, "the big view is drawn again for the new GPU");
    let warnings = missing_texture_warnings(since);
    assert!(warnings.is_empty(), "a freed or foreign egui texture was painted: {warnings:?}");
}
