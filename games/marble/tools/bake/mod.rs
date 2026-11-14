//! Инструмент сборки ассетов «Шарового завода»: `cargo run -p marble --example bake_assets`.
//!
//! Он строит сцены, префабы, интерфейс, материалы, звуки и данные кодом (`kit.rs`,
//! `courses.rs`, `lab.rs`, `ui_build.rs`, `sounds.rs`) и записывает их в `assets/` тем же
//! сериализатором, что и редактор. В игре этого кода нет — она стартует из одних файлов;
//! инструмент нужен только чтобы перегенерировать ассеты после правки раскладки цехов.
//! Предварительно: `python tools/gen_assets.py` (текстуры и `.obj`-модели).

// Строители шире, чем нужно сборке (часть помощников осталась от прежней игры в коде).
#![allow(dead_code, clippy::too_many_arguments, clippy::vec_init_then_push)]

pub mod courses;
pub mod kit;
pub mod lab;
pub mod sounds;
pub mod ui_build;

use std::error::Error;
use std::path::{Path, PathBuf};

use engine::prelude::*;
use marble::scripts::{ball::Ball, camera_rig::CameraRig, game_manager::GameManager};
use marble::{CourseInfo, Route};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Приложение с зарегистрированными скриптами и подключёнными ассетами.
fn bake_app(assets: &Path) -> Result<App> {
    let mut app = engine::app();
    marble::register(&mut app);
    engine::mount_assets(&mut app, assets)?;
    Ok(app)
}

pub fn run() -> Result<()> {
    let assets = assets_dir();
    for needed in ["textures/plate.png", "models/gear.obj"] {
        if !assets.join(needed).exists() {
            return Err(format!("`{needed}` is missing: run `python tools/gen_assets.py` first").into());
        }
    }

    // --- Фаза A: материалы, префабы, интерфейс, звуки, данные ---------------------------
    {
        let mut db = AssetDatabase::open(&assets)?;
        let mats = lab::write_materials(&mut db)?;
        let mut lab = lab::Lab::new(&assets, &db, mats)?;
        lab.track_blocks()?;
        lab.mechanisms()?;
        lab.ball_and_decor()?;
        println!("prefabs: {}", lab.saved.len());
        bake_ui(&assets)?;
    }
    bake_audio(&assets)?;
    bake_data(&assets)?;
    bake_settings(&assets)?;

    // --- Фаза B: сцены (префабы уже на диске, у них есть GUID) ---------------------------
    {
        let _ = AssetDatabase::open(&assets)?; // создаёт .meta для префабов и данных
    }
    let mut routes = Vec::new();
    for index in 0..courses::COUNT {
        let mut app = bake_app(&assets)?;
        let built = {
            let mut k = kit::Kit::new(&mut app.world);
            let built = courses::build(&mut k, index);
            println!("course {}: {} gears, {} checkpoints, {} bodies, {} route points", index + 1, built.gears, built.checkpoints, built.bodies, built.route.len());
            built
        };
        compose(&mut app.world, index as i32, &built, None)?;
        write_scene(&mut app.world, &assets, &format!("scenes/courses/course_{}.scene", index + 1), &format!("Course {}", index + 1))?;
        routes.push(built.route);
    }
    {
        let mut app = bake_app(&assets)?;
        let showcase = {
            let mut k = kit::Kit::new(&mut app.world);
            courses::build_showcase(&mut k)
        };
        let built = courses::Built { start: Vec3::ZERO, yaw_deg: 0.0, gears: 0, golden: false, checkpoints: 0, bodies: 0, kill_y: -50.0, route: Vec::new() };
        compose(&mut app.world, -1, &built, Some(showcase))?;
        write_scene(&mut app.world, &assets, "scenes/menu.scene", "Menu")?;
    }
    for (i, points) in routes.into_iter().enumerate() {
        let text = ron::ser::to_string_pretty(&Route { points }, ron::ser::PrettyConfig::new())?;
        write_text(&assets, &format!("data/routes/course_{}.ron", i + 1), &format!("// Маршрут цеха {} для бота тестов и скриншотов (точки и прыжки).\n{text}\n", i + 1))?;
    }
    // Ещё раз открываем базу: .meta для сцен и маршрутов.
    let _ = AssetDatabase::open(&assets)?;
    println!("done");
    Ok(())
}

// ----------------------------------------------------------------------
// Сцены
// ----------------------------------------------------------------------

/// Экземпляр префаба по пути (ссылка — GUID).
fn spawn(world: &mut World, rel: &str, name: &str, pos: Vec3) -> Result<GameObjectId> {
    let path = format!("{rel}.prefab");
    let guid = world
        .resources
        .get::<AssetDatabase>()
        .and_then(|db| db.guid_of(&path))
        .ok_or_else(|| format!("prefab `{path}` is missing"))?
        .to_string();
    let id = world.instantiate_at(&guid, pos, Quat::IDENTITY, None)?;
    if !name.is_empty() {
        world.scene.get_mut(id).ok_or("instance")?.name = name.to_string();
    }
    Ok(id)
}

/// Камера, солнце, атмосфера, менеджер игры, интерфейс и (в цехе) шар.
fn compose(world: &mut World, course: i32, built: &courses::Built, showcase: Option<(Vec3, f32)>) -> Result<()> {
    let camera = spawn(world, "prefabs/rig/camera", "Camera", Vec3::new(0.0, 8.0, 14.0))?;
    spawn(world, "prefabs/rig/sun", "Sun", Vec3::ZERO)?;
    spawn(world, "prefabs/rig/environment", "Environment", Vec3::ZERO)?;
    let game = spawn(world, "prefabs/rig/game", "Game", Vec3::ZERO)?;
    if let Some(manager) = world.scene.get_component_mut::<GameManager>(game) {
        manager.course = course;
        manager.start_yaw_deg = built.yaw_deg;
    }
    if let Some((center, distance)) = showcase {
        if let Some(rig) = world.scene.get_component_mut::<CameraRig>(camera) {
            rig.orbit = true;
            rig.orbit_center = center;
            rig.orbit_distance = distance;
        }
        for (ui, name) in [("main_menu", "Menu"), ("course_select", "Select"), ("options", "Options"), ("fade", "Fade")] {
            spawn(world, &format!("ui/{ui}"), name, Vec3::ZERO)?;
        }
    } else {
        let ball = spawn(world, "prefabs/ball", "Ball", built.start)?;
        if let Some(b) = world.scene.get_component_mut::<Ball>(ball) {
            b.kill_y = built.kill_y;
        }
        if let Some(rig) = world.scene.get_component_mut::<CameraRig>(camera) {
            rig.orbit = false;
        }
        for (ui, name) in [("hud", "HUD"), ("pause", "Pause"), ("results", "Results"), ("fade", "Fade")] {
            spawn(world, &format!("ui/{ui}"), name, Vec3::ZERO)?;
        }
    }
    Ok(())
}

fn write_scene(world: &mut World, assets: &Path, rel: &str, name: &str) -> Result<()> {
    world.scene.name = name.to_string();
    let path = assets.join(rel);
    std::fs::create_dir_all(path.parent().ok_or("scene dir")?)?;
    world.save_scene(&path)?;
    println!("wrote {rel} ({} objects)", world.scene.len());
    Ok(())
}

fn write_text(assets: &Path, rel: &str, text: &str) -> Result<()> {
    let path = assets.join(rel);
    std::fs::create_dir_all(path.parent().ok_or("dir")?)?;
    std::fs::write(path, text)?;
    Ok(())
}

// ----------------------------------------------------------------------
// Интерфейс, звук, данные, настройки
// ----------------------------------------------------------------------

/// Экраны интерфейса → `assets/ui/*.prefab`.
fn bake_ui(assets: &Path) -> Result<()> {
    let mut scene = Scene::new("ui");
    ui_build::build(&mut scene);
    for (canvas, file) in [
        ("Menu", "main_menu"),
        ("Select", "course_select"),
        ("Options", "options"),
        ("HUD", "hud"),
        ("Pause", "pause"),
        ("Results", "results"),
        ("Fade", "fade"),
    ] {
        let id = scene.find_by_name(canvas).ok_or_else(|| format!("canvas `{canvas}` was not built"))?;
        let prefab = Prefab::from_object(&scene, id).ok_or("canvas is gone")?;
        let path = assets.join("ui").join(format!("{file}.prefab"));
        std::fs::create_dir_all(path.parent().ok_or("dir")?)?;
        prefab.save(&path)?;
    }
    println!("ui prefabs: 7");
    Ok(())
}

/// Звуки игры из синтезатора → `assets/audio/*.wav` и список `data/sounds.ron`.
fn bake_audio(assets: &Path) -> Result<()> {
    let dir = assets.join("audio");
    std::fs::create_dir_all(&dir)?;
    let mut list = marble::game::data::SoundList::default();
    for (name, clip) in sounds::effects() {
        clip.save_wav(dir.join(format!("{name}.wav")))?;
        list.effects.push((name.to_string(), format!("audio/{name}.wav")));
    }
    for (name, clip) in [
        (marble::game::sounds::MUSIC_MENU, sounds::music_menu()),
        (marble::game::sounds::MUSIC_FACTORY_A, sounds::music_factory_a()),
        (marble::game::sounds::MUSIC_FACTORY_B, sounds::music_factory_b()),
    ] {
        clip.save_wav(dir.join(format!("{name}.wav")))?;
        list.music.push((name.to_string(), format!("audio/{name}.wav")));
    }
    let text = ron::ser::to_string_pretty(&list, ron::ser::PrettyConfig::new())?;
    write_text(assets, "data/sounds.ron", &format!("// Звуки игры: имя клипа -> файл. Файлы пишет tools/bake (из синтезатора); можно заменить своими .wav.\n{text}\n"))?;
    println!("audio: {} effects, {} music", list.effects.len(), list.music.len());
    Ok(())
}

/// Таблица цехов: названия и пороги медалей.
fn bake_data(assets: &Path) -> Result<()> {
    let table = marble::game::data::CourseTable {
        courses: courses::COURSES
            .iter()
            .map(|c| CourseInfo { name: c.name.to_string(), blurb: c.blurb.to_string(), gold: c.gold, silver: c.silver, bronze: c.bronze, limit: c.limit })
            .collect(),
    };
    let text = ron::ser::to_string_pretty(&table, ron::ser::PrettyConfig::new())?;
    write_text(assets, "data/courses.ron", &format!("// Цеха: названия, что нового, пороги медалей (секунды) и лимит смены.\n{text}\n"))
}

/// Настройки проекта: окно, гравитация, теги и слои, ввод (пауза).
fn bake_settings(assets: &Path) -> Result<()> {
    use engine::core::settings::ProjectSettings;
    use engine::core::settings_sections::InputAction;
    let mut s = ProjectSettings::default();
    s.player.title = "Шаровой завод".to_string();
    s.player.width = 1280;
    s.player.height = 720;
    s.player.default_scene = "scenes/menu.scene".to_string();
    s.physics3d.gravity = [0.0, -marble::GRAVITY, 0.0];
    s.tags_and_layers.add_tag("Ball");
    s.tags_and_layers.add_tag("Mover");
    s.tags_and_layers.add_tag("MainCamera");
    s.tags_and_layers.layers[8] = "Ball".to_string();
    s.tags_and_layers.layers[9] = "Mechanism".to_string();
    s.tags_and_layers.layers[10] = "Track".to_string();
    s.input.actions.push(InputAction::button("Pause", &[Binding::Key(Key::Escape), Binding::Pad(GamepadButton::Start)]));
    s.save(assets.join("project.settings"))?;
    Ok(())
}
