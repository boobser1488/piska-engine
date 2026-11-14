//! Инструмент сборки ассетов «Ташбазара»: `cargo run -p tashbazar --example bake_assets`.
//!
//! Рисует текстуры (`textures.rs`), копирует модели набора витрины (`assets/showcase/models`),
//! пишет материалы и префабы (`kit.rs`), экраны интерфейса (`ui_build.rs`), звуки
//! (`sounds.rs`), настройки проекта, карту (`map.rs`) и полигон (`gym.rs`) — тем же
//! сериализатором, что и редактор. В игре этого кода нет: она стартует из файлов.
//!
//! Сцены правят в редакторе, поэтому генератор их **не перезаписывает**, а сливает свежий
//! результат с файлом (`merge.rs`): правленные, удалённые и добавленные руками объекты
//! остаются как есть. `-- --overwrite-scene` — записать сцены заново (печатает, что
//! пропадёт). Свои сгенерированные файлы генератор перечисляет в `bake_state/files.txt`
//! рядом с `assets/` и удаляет из них только те, что перестал создавать; чужие файлы
//! (модели из Modeler, свои префабы) не трогает. `.meta` не пересоздаются — GUID постоянны.

pub mod build;
pub mod gym;
pub mod kit;
pub mod map;
pub mod merge;
pub mod meshes;
pub mod sounds;
pub mod textures;
pub mod ui_build;

use std::error::Error;
use std::path::{Path, PathBuf};

use engine::prelude::*;
use tashbazar::scripts::checkpoint::Checkpoint;
use tashbazar::scripts::game_manager::GameManager;
use tashbazar::scripts::hud::Hud;
use tashbazar::scripts::menu_camera::MenuCamera;
use tashbazar::scripts::movement_course::MovementCourse;
use tashbazar::RouteTable;

use build::Builder;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Куда писать: `assets/` игры или каталог из `TASHBAZAR_BAKE_OUT` (пробный прогон, сравнение).
fn assets_dir() -> PathBuf {
    match std::env::var_os("TASHBAZAR_BAKE_OUT") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
    }
}

fn bake_app(assets: &Path) -> Result<App> {
    let mut app = engine::app();
    tashbazar::register(&mut app);
    engine::mount_assets(&mut app, assets)?;
    Ok(app)
}

/// Каталог состояния генератора: рядом с `assets/` (в игру не попадает).
fn state_dir(assets: &Path) -> PathBuf {
    assets.parent().unwrap_or(assets).join("bake_state")
}

/// Копия файла с новой датой изменения (копия сохраняет старую, а по дате генератор
/// узнаёт свои файлы).
fn copy_fresh(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)?;
    std::fs::File::options().write(true).open(to)?.set_modified(std::time::SystemTime::now())?;
    Ok(())
}

/// Файлы (без `.meta`) в каталогах, куда пишет генератор, изменённые не раньше `since`.
fn written_since(assets: &Path, since: std::time::SystemTime) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, since: std::time::SystemTime, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, since, out);
            } else if path.extension().is_none_or(|e| e != "meta")
                && entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= since)
            {
                out.push(path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    for dir in ["textures", "materials", "models", "prefabs", "ui", "audio", "data"] {
        walk(&assets.join(dir), assets, since, &mut out);
    }
    out.push("project.settings".to_string());
    out.sort();
    out
}

/// Убрать файлы, которые генератор создавал в прошлый раз, а теперь не создаёт, и
/// записать новый список. Файлы не из списка (свои модели, префабы) не трогаются.
fn prune_stale(assets: &Path, written: &[String]) -> Result<()> {
    let list = state_dir(assets).join("files.txt");
    if let Ok(old) = std::fs::read_to_string(&list) {
        for rel in old.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if !written.iter().any(|w| w == rel) {
                let path = assets.join(rel);
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file(format!("{}.meta", path.display()));
                println!("removed stale {rel}");
            }
        }
    }
    std::fs::create_dir_all(state_dir(assets))?;
    std::fs::write(&list, format!("# Файлы, которые пишет генератор ассетов (tools/bake). Остальные он не трогает.\n{}\n", written.join("\n")))?;
    Ok(())
}

pub fn run() -> Result<()> {
    let assets = assets_dir();
    let overwrite = std::env::args().any(|a| a == "--overwrite-scene");
    std::fs::create_dir_all(&assets)?;
    let started = std::time::SystemTime::now() - std::time::Duration::from_secs(2);

    // --- Фаза A: текстуры, модели, материалы, префабы, интерфейс, звуки, данные -------------
    // Файлы пишутся поверх, `.meta` остаются (GUID не меняются — сцены и правки в редакторе
    // ссылаются на них); устаревшие свои файлы убираются в конце (`prune_stale`).
    let names = textures::write_all(&assets)?;
    let showcase = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/showcase");
    copy_fresh(&showcase.join("palette.png"), &assets.join("textures/palette.png"))?;
    std::fs::create_dir_all(assets.join("models"))?;
    for model in kit::MODELS {
        copy_fresh(&showcase.join(format!("models/{model}.obj")), &assets.join(format!("models/{model}.obj")))?;
    }
    // Свои модели с развёрткой и атласы к ним: ящики (один атлас на три размера) и телега.
    meshes::crate_atlas(&assets.join("textures"))?;
    for (i, (name, size)) in meshes::CRATES.iter().enumerate() {
        let mesh = meshes::crate_mesh(*size, 31 + i as u32);
        mesh.write_obj(&assets.join(format!("models/{name}.obj")), &format!("{name}: ящик {} x {} x {} м, атлас textures/crate.png (tools/bake/meshes.rs)", size.x, size.y, size.z))?;
        println!("model {name}: {} triangles", mesh.triangles());
    }
    meshes::cart_atlas(&assets.join("textures"))?;
    let cart = meshes::cart_mesh();
    cart.write_obj(&assets.join("models/cart.obj"), "cart: телега, атлас textures/cart.png (tools/bake/meshes.rs)")?;
    println!("model cart: {} triangles", cart.triangles());
    println!("textures: {} + palette, models: {}", names.len(), kit::MODELS.len());
    // `.meta` новых картинок и фильтр `Pixel` у всех (резко вблизи, мипмапы вдали).
    let _ = AssetDatabase::open(&assets)?;
    textures::pixel_filter(&assets)?;
    bake_settings(&assets)?;
    {
        let mut db = AssetDatabase::open(&assets)?;
        let mats = kit::write_materials(&mut db)?;
        let mut lab = kit::Lab::new(&assets, &db, mats)?;
        lab.blocks()?;
        lab.props()?;
        lab.shapes()?;
        lab.facade_parts()?;
        lab.course()?;
        lab.markers()?;
        lab.rig()?;
        println!("prefabs: {}", lab.saved.len());
        println!("ui prefabs: {}", ui_build::bake(&assets)?);
    }
    bake_audio(&assets)?;
    let _ = AssetDatabase::open(&assets)?; // .meta для префабов, звуков и интерфейса

    // --- Фаза B: сцены ---------------------------------------------------------------------
    let routes = {
        let mut app = bake_app(&assets)?;
        let built = {
            let mut b = Builder::new(&mut app.world);
            let built = map::build(&mut b)?;
            for (name, at, spread) in &b.uneven {
                println!("uneven ground under {name} at {at} (spread {spread:.3} m)");
            }
            for note in &b.notes {
                println!("facade: {note}");
            }
            println!("windows: {:?}", b.window_kinds);
            built
        };
        compose_map(&mut app.world)?;
        write_scene(&mut app.world, &assets, tashbazar::MAP_SCENE, "Tashbazar", overwrite)?;
        println!("map: {} pieces", built.objects);
        built.routes
    };
    {
        let mut app = bake_app(&assets)?;
        {
            let mut b = Builder::new(&mut app.world);
            gym::build(&mut b)?;
        }
        compose_gym(&mut app.world)?;
        write_scene(&mut app.world, &assets, tashbazar::GYM_SCENE, "Gym", overwrite)?;
    }
    let table = RouteTable { routes };
    for r in &table.routes {
        println!("route {:14} {:5.1} m  {:5.1} s  {}", r.id, r.length(), r.seconds, r.title);
    }
    let text = ron::ser::to_string_pretty(&table, ron::ser::PrettyConfig::new())?;
    write_text(
        &assets,
        "data/routes.ron",
        &format!("// Маршруты карты: по ним ходят боты тестов; `seconds` — расчётное время бегом (250 ед/с).\n{text}\n"),
    )?;
    prune_stale(&assets, &written_since(&assets, started))?;
    let _ = AssetDatabase::open(&assets)?;
    if std::env::var("TASHBAZAR_PLAN").is_ok() {
        print!("{}", map::ascii_plan());
    }
    println!("done");
    Ok(())
}

fn write_text(assets: &Path, rel: &str, text: &str) -> Result<()> {
    let path = assets.join(rel);
    std::fs::create_dir_all(path.parent().ok_or("dir")?)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Записать сцену: постоянные номера объектов, слияние с файлом на диске (правки в
/// редакторе остаются), запись атомарно (временный файл → переименование) и запись о том,
/// что поставил генератор (`bake_state/<сцена>.baked.ron`).
fn write_scene(world: &mut World, assets: &Path, rel: &str, name: &str, overwrite: bool) -> Result<()> {
    world.scene.name = name.to_string();
    let path = assets.join(rel);
    std::fs::create_dir_all(path.parent().ok_or("scene dir")?)?;
    let db = world.resources.get::<AssetDatabase>().ok_or("asset database")?.clone();
    let prefab_path = |guid: &str| {
        Guid::parse(guid).and_then(|g| db.relative_path_of(g)).map(str::to_string).unwrap_or_else(|| guid.to_string())
    };
    let fresh = SceneData::from_ron(&world.scene_to_string(), rel)?;
    let fresh = merge::stable_ids(&fresh, &prefab_path);
    let stem = Path::new(rel).file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
    let record_path = state_dir(assets).join(format!("{stem}.baked.ron"));
    let record: Option<merge::Record> = std::fs::read_to_string(&record_path).ok().and_then(|t| ron::from_str(&t).ok());
    let on_disk = std::fs::read_to_string(&path).ok().and_then(|t| SceneData::from_ron(&t, rel).ok());
    let out = match (on_disk, &record) {
        (Some(user), Some(record)) if !overwrite => {
            let (merged, report) = merge::merge(&user, record, &fresh);
            report.print(rel);
            merged
        }
        (Some(_), None) if !overwrite => {
            return Err(format!(
                "{rel}: на диске есть сцена, а записи генератора ({}) нет — правки в ней не отличить \
                 от сгенерированного. Запустите с `-- --overwrite-scene`, если сцену можно перезаписать.",
                record_path.display()
            )
            .into());
        }
        (user, record) => {
            if let (Some(user), Some(record)) = (&user, record) {
                let lost = merge::losses(user, record);
                println!("{rel}: --overwrite-scene: {} manual change(s) dropped", lost.len());
                for line in lost {
                    println!("  dropped {line}");
                }
            }
            fresh.clone()
        }
    };
    let tmp = path.with_extension("scene.tmp");
    std::fs::write(&tmp, out.to_ron())?;
    std::fs::rename(&tmp, &path)?;
    std::fs::create_dir_all(state_dir(assets))?;
    let record = merge::record(&fresh);
    std::fs::write(&record_path, ron::ser::to_string_pretty(&record, ron::ser::PrettyConfig::new())?)?;
    println!("wrote {rel} ({} objects baked)", record.objects.len());
    Ok(())
}

fn spawn(world: &mut World, prefab: &str, name: &str, pos: Vec3, rot: Quat) -> Result<GameObjectId> {
    let path = if prefab.starts_with("ui/") { format!("{prefab}.prefab") } else { format!("prefabs/{prefab}.prefab") };
    let guid = world
        .resources
        .get::<AssetDatabase>()
        .and_then(|db| db.guid_of(&path))
        .ok_or_else(|| format!("prefab `{path}` is missing"))?
        .to_string();
    let id = world.instantiate_at(&guid, pos, rot, None)?;
    world.scene.get_mut(id).ok_or("instance")?.name = name.to_string();
    Ok(id)
}

/// Общие объекты сцены: свет, атмосфера, игрок, экраны, менеджер. Возвращает (игрок, менеджер, HUD).
fn compose_common(world: &mut World, player_at: Vec3, player_yaw: f32) -> Result<(GameObjectId, GameObjectId, GameObjectId)> {
    spawn(world, "rig/environment", "Environment", Vec3::ZERO, Quat::IDENTITY)?;
    spawn(world, "rig/sun", "Sun", Vec3::new(0.0, 60.0, 0.0), Quat::from_rotation_y(-35f32.to_radians()) * Quat::from_rotation_x(-52f32.to_radians()))?;
    let player = spawn(world, "rig/player", "Player", player_at, Quat::IDENTITY)?;
    if let Some(c) = world.scene.get_component_mut::<FpsController>(player) {
        c.yaw_deg = player_yaw;
    }
    let mut ui = Vec::new();
    for (prefab, name) in [("ui/main_menu", "Main Menu"), ("ui/options", "Options"), ("ui/pause", "Pause"), ("ui/results", "Results"), ("ui/hud", "HUD")] {
        ui.push(spawn(world, prefab, name, Vec3::ZERO, Quat::IDENTITY)?);
    }
    let game = spawn(world, "rig/game", "Game", Vec3::ZERO, Quat::IDENTITY)?;
    let r = |world: &World, id| ObjectRef::new(&world.scene, id);
    let refs = (r(world, player), r(world, ui[0]), r(world, ui[1]), r(world, ui[2]), r(world, ui[3]), r(world, ui[4]));
    if let Some(m) = world.scene.get_component_mut::<GameManager>(game) {
        m.player = refs.0;
        m.main_menu = refs.1;
        m.options_menu = refs.2;
        m.pause_menu = refs.3;
        m.results = refs.4;
        m.hud = refs.5;
    }
    let hud = ui[4];
    let player_ref = r(world, player);
    if let Some(h) = world.scene.get_component_mut::<Hud>(hud) {
        h.player = player_ref;
    }
    Ok((player, game, hud))
}

/// Карта: камера меню, полоса препятствий, игрок (выключен до «Играть»).
fn compose_map(world: &mut World) -> Result<()> {
    let menu = spawn(world, "rig/menu_camera", "Menu Camera", Vec3::new(0.0, 30.0, 50.0), Quat::IDENTITY)?;
    if let Some(c) = world.scene.get_component_mut::<MenuCamera>(menu) {
        c.center = Vec3::new(-4.0, 0.0, -10.0);
        c.radius = 64.0;
        c.height = 36.0;
        c.speed_deg = 2.5;
        c.angle_deg = 215.0;
    }
    let (player, game, hud) = compose_common(world, Vec3::new(-6.0, 0.0, -51.0), 180.0)?;
    world.scene.set_active(player, false);

    // Полоса препятствий: старт, четыре отметки и финиш.
    let course = world.scene.build("Movement Course").id();
    let marks: [(&str, &str, Vec3, Vec3, f32); 6] = [
        ("course/checkpoint", "Старт: ящики у сторожки", Vec3::new(-9.0, 0.0, -48.5), Vec3::new(3.0, 2.5, 3.0), 180.0),
        ("course/checkpoint", "Крыша сторожки", Vec3::new(-9.0, 3.95, -36.0), Vec3::new(5.0, 2.5, 5.0), 180.0),
        ("course/checkpoint", "Крыша дома у канала", Vec3::new(-9.0, 3.45, -21.0), Vec3::new(6.0, 2.5, 6.0), 90.0),
        ("course/checkpoint", "Акведук за проломом", Vec3::new(-30.5, 3.0, -18.5), Vec3::new(3.0, 2.5, 3.0), 90.0),
        ("course/checkpoint", "Водонапорная башня", Vec3::new(-34.5, 4.25, 4.5), Vec3::new(4.0, 2.5, 4.0), 180.0),
        ("course/finish", "Финиш: цистерна", Vec3::new(-39.4, 1.25, 12.0), Vec3::new(2.4, 2.5, 3.0), 90.0),
    ];
    let mut refs = Vec::new();
    for (prefab, title, pos, size, yaw) in marks {
        let id = spawn(world, prefab, title, pos, Quat::IDENTITY)?;
        world.scene.set_parent(id, Some(course))?;
        if let Some(c) = world.scene.get_component_mut::<Checkpoint>(id) {
            c.title = title.to_string();
            c.size = size;
            c.respawn_yaw_deg = yaw;
        }
        refs.push(ObjectRef::new(&world.scene, id));
    }
    let player_ref = ObjectRef::new(&world.scene, player);
    world.scene.get_mut(course).ok_or("course")?.add(MovementCourse {
        checkpoints: refs,
        player: player_ref,
        fail_from: 2,
        fail_to: 4,
        ..MovementCourse::default()
    });
    let course_ref = ObjectRef::new(&world.scene, course);
    let menu_ref = ObjectRef::new(&world.scene, menu);
    if let Some(m) = world.scene.get_component_mut::<GameManager>(game) {
        m.course = course_ref;
        m.menu_camera = menu_ref;
    }
    if let Some(h) = world.scene.get_component_mut::<Hud>(hud) {
        h.course = course_ref;
    }
    Ok(())
}

/// Полигон: сразу игра, «В меню» — сцена карты.
fn compose_gym(world: &mut World) -> Result<()> {
    let spawn_point = spawn(world, "markers/spawn_attackers", "Spawn", Vec3::new(-10.0, 0.0, 16.0), Quat::IDENTITY)?;
    let _ = spawn_point;
    let (_, game, _) = compose_common(world, Vec3::new(-10.0, 0.0, 16.0), 0.0)?;
    if let Some(m) = world.scene.get_component_mut::<GameManager>(game) {
        m.start_playing = true;
        m.menu_scene = "tashbazar".to_string();
    }
    Ok(())
}

/// Звуки игры из синтезатора → `assets/audio/*.wav` и список `data/sounds.ron`.
fn bake_audio(assets: &Path) -> Result<()> {
    let dir = assets.join("audio");
    std::fs::create_dir_all(&dir)?;
    let mut list = tashbazar::game::data::SoundList::default();
    for (name, clip) in sounds::effects() {
        clip.save_wav(dir.join(format!("{name}.wav")))?;
        list.effects.push((name.to_string(), format!("audio/{name}.wav")));
    }
    let text = ron::ser::to_string_pretty(&list, ron::ser::PrettyConfig::new())?;
    write_text(assets, "data/sounds.ron", &format!("// Звуки игры: имя клипа -> файл (синтезатор движка, эффекты — пик −2 дБFS). Можно заменить своими .wav.\n{text}\n"))?;
    println!("audio: {} effects", list.effects.len());
    Ok(())
}

/// Настройки проекта: окно, шаг 1/64 с (классический «тикрейт»), ввод, слои.
fn bake_settings(assets: &Path) -> Result<()> {
    use engine::core::settings::ProjectSettings;
    use engine::core::settings_sections::InputAction;
    let mut s = ProjectSettings::default();
    s.player.title = "Ташбазар".to_string();
    s.player.width = 1280;
    s.player.height = 720;
    s.player.default_scene = tashbazar::MAP_SCENE.to_string();
    s.time.fixed_timestep = 1.0 / 64.0;
    s.tags_and_layers.add_tag("Player");
    s.input.actions.push(InputAction::button("Duck", &[Binding::Key(Key::LCtrl), Binding::Key(Key::C), Binding::Pad(GamepadButton::East)]));
    s.input.actions.push(InputAction::button("Walk", &[Binding::Key(Key::LShift), Binding::Pad(GamepadButton::LeftStick)]));
    s.input.actions.push(InputAction::button("Sprint", &[Binding::Key(Key::LAlt)]));
    s.input.actions.push(InputAction::button("Pause", &[Binding::Key(Key::Escape), Binding::Pad(GamepadButton::Start)]));
    s.save(assets.join("project.settings"))?;
    Ok(())
}
