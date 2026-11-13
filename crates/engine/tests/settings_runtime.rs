//! Настройки проекта (`project.settings`) действительно меняют поведение игры: физика
//! 2D / 3D (гравитация, решатель, засыпание, матрица слоёв), звук (шины, фокус),
//! графика (тени, блум, туман, пресеты), окно (Player), сцена по умолчанию, время и
//! ввод. Всё без окна; картинки — `Renderer::headless().capture` под одним замком GPU.

use std::path::{Path, PathBuf};

use engine::prelude::*;
use engine_core::settings::{InputAction, WindowFocus};
use engine_render::Renderer;
use image::RgbaImage;

const DT: f32 = 1.0 / 60.0;

/// Свой каталог ассетов на тест (тесты идут параллельно).
fn temp_assets(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("piska_settings_runtime_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_settings(dir: &Path, settings: &ProjectSettings) {
    settings.save(dir.join(engine_core::settings::PROJECT_SETTINGS_FILE)).unwrap();
}

/// Приложение движка с применёнными настройками — тот же путь, что у редактора
/// (`World::apply_project_settings` вызывает хуки подсистем).
fn app_with(settings: ProjectSettings) -> App {
    let mut app = engine::app();
    app.world.apply_project_settings(settings);
    app
}

fn y_of(app: &App, id: GameObjectId) -> f32 {
    app.world.scene.world_position(id).y
}

// ---------------------------------------------------------------------------
// Физика 2D
// ---------------------------------------------------------------------------

fn ground_2d(app: &mut App, layer: u32) {
    app.spawn("Ground").at(0.0, -0.5).layer(layer).with(Collider2D::rect(40.0, 1.0));
}

fn ball_2d(app: &mut App, name: &str, x: f32, layer: u32) -> GameObjectId {
    app.spawn(name).at(x, 2.0).layer(layer).with(Rigidbody2D::dynamic()).with(Collider2D::circle(0.25)).id()
}

#[test]
fn physics2d_layer_matrix_lets_two_layers_pass_through() {
    let mut settings = ProjectSettings::default();
    settings.physics2d.layer_matrix.set(8, 9, false);
    let mut app = app_with(settings);
    ground_2d(&mut app, 8);
    let ghost = ball_2d(&mut app, "Ghost", -2.0, 9);
    let solid = ball_2d(&mut app, "Solid", 2.0, 10);
    app.run_headless(120, DT);
    assert!(y_of(&app, ghost) < -3.0, "layer 9 falls through layer 8: y = {}", y_of(&app, ghost));
    assert!((y_of(&app, solid) - 0.25).abs() < 0.05, "layer 10 still lands: y = {}", y_of(&app, solid));
}

#[test]
fn physics2d_matrix_edited_later_rebuilds_existing_bodies() {
    // Редактор меняет настройки, когда тела уже созданы.
    let mut app = engine::app();
    ground_2d(&mut app, 8);
    let ball = ball_2d(&mut app, "Ball", 0.0, 9);
    app.run_headless(90, DT);
    assert!((y_of(&app, ball) - 0.25).abs() < 0.05, "default matrix: everything collides");

    let mut settings = ProjectSettings::default();
    settings.physics2d.layer_matrix.set(9, 8, false);
    app.world.apply_project_settings(settings);
    app.run_headless(90, DT);
    assert!(y_of(&app, ball) < -2.0, "after the edit the ball drops through: y = {}", y_of(&app, ball));
}

#[test]
fn physics2d_gravity_iterations_and_sleep_come_from_settings() {
    let fall = |gravity: f32| {
        let mut settings = ProjectSettings::default();
        settings.physics2d.gravity = [0.0, gravity];
        let mut app = app_with(settings);
        let ball = app.spawn("Ball").at(0.0, 100.0).with(Rigidbody2D::dynamic()).with(Collider2D::circle(0.25)).id();
        app.run_headless(30, DT);
        100.0 - y_of(&app, ball)
    };
    let (normal, double) = (fall(-9.81), fall(-19.62));
    assert!(normal > 1.0);
    assert!((double / normal - 2.0).abs() < 0.1, "twice the gravity, twice the fall: {normal} vs {double}");

    let mut settings = ProjectSettings::default();
    settings.physics2d.solver_iterations = 12;
    settings.physics2d.sleep_linear_threshold = 0.1;
    settings.physics2d.sleep_angular_threshold = 0.2;
    let app = app_with(settings);
    let physics = app.world.resources.get::<Physics2D>().unwrap();
    assert_eq!(physics.solver_iterations(), 12);
    assert_eq!(physics.sleep_thresholds(), (0.1, 0.2));
    assert!(physics.layer_matrix().is_default());
}

#[test]
fn physics2d_gravity_set_by_code_after_settings_wins() {
    let mut settings = ProjectSettings::default();
    settings.physics2d.gravity = [0.0, -30.0];
    let mut app = app_with(settings);
    // Игра после `mount_assets` задаёт свою гравитацию — шаги физики её не трогают.
    app.world.resources.get_mut::<Physics2D>().unwrap().gravity = Vec2::new(0.0, 5.0);
    let ball = app.spawn("Ball").at(0.0, 0.0).with(Rigidbody2D::dynamic()).with(Collider2D::circle(0.25)).id();
    app.run_headless(30, DT);
    assert!(y_of(&app, ball) > 0.5, "the ball floats up with the code gravity");
    assert_eq!(app.world.resources.get::<Physics2D>().unwrap().gravity, Vec2::new(0.0, 5.0));
}

// ---------------------------------------------------------------------------
// Физика 3D
// ---------------------------------------------------------------------------

fn crate_3d(app: &mut App, name: &str, x: f32, layer: u32) -> GameObjectId {
    app.spawn(name)
        .at3(x, 2.0, 0.0)
        .layer(layer)
        .with(Rigidbody::dynamic())
        .with(Collider::cuboid(1.0, 1.0, 1.0))
        .id()
}

#[test]
fn physics3d_layer_matrix_filters_bodies_characters_and_terrain() {
    let mut settings = ProjectSettings::default();
    settings.physics3d.layer_matrix.set(8, 9, false);
    let mut app = app_with(settings);
    app.spawn("Ground").at3(0.0, -0.5, 0.0).layer(8).with(Collider::cuboid(40.0, 1.0, 40.0));
    let ghost = crate_3d(&mut app, "Ghost", -3.0, 9);
    let solid = crate_3d(&mut app, "Solid", 3.0, 0);
    let walker = app.spawn("Walker").at3(0.0, 1.0, 5.0).layer(9).with(CharacterController::default()).id();
    let player = app.spawn("Player").at3(0.0, 1.0, -5.0).layer(3).with(CharacterController::default()).id();
    app.run_headless(120, DT);
    assert!(y_of(&app, ghost) < -3.0, "layer 9 crate falls through: y = {}", y_of(&app, ghost));
    assert!((y_of(&app, solid) - 0.5).abs() < 0.05, "layer 0 crate rests: y = {}", y_of(&app, solid));
    assert!(y_of(&app, walker) < -2.0, "layer 9 character falls through: y = {}", y_of(&app, walker));
    assert!(y_of(&app, player).abs() < 0.1, "layer 3 character stands: y = {}", y_of(&app, player));

    // Плитка рельефа на слое 8: слой 9 сквозь неё проваливается, остальные стоят.
    let mut app = app_with({
        let mut settings = ProjectSettings::default();
        settings.physics3d.layer_matrix.set(8, 9, false);
        settings
    });
    // Плоский рельеф 128 × 128 м от начала координат (плитки карт высот строит подсистема рельефа).
    app.spawn("Terrain").layer(8).with(Terrain { size: 128.0, height_scale: 0.0, ..Terrain::default() });
    let spawn = |app: &mut App, name: &str, x: f32, layer: u32| {
        app.spawn(name)
            .at3(x, 2.0, 30.0)
            .layer(layer)
            .with(Rigidbody::dynamic())
            .with(Collider::cuboid(1.0, 1.0, 1.0))
            .id()
    };
    let ghost = spawn(&mut app, "Ghost", 30.0, 9);
    let solid = spawn(&mut app, "Solid", 34.0, 0);
    app.run_headless(120, DT);
    assert!(y_of(&app, ghost) < -3.0, "falls through the terrain: y = {}", y_of(&app, ghost));
    assert!((y_of(&app, solid) - 0.5).abs() < 0.08, "rests on the terrain: y = {}", y_of(&app, solid));
}

#[test]
fn physics3d_matrix_edited_later_rebuilds_existing_colliders() {
    let mut app = engine::app();
    app.spawn("Ground").at3(0.0, -0.5, 0.0).layer(8).with(Collider::cuboid(40.0, 1.0, 40.0));
    let crate_id = crate_3d(&mut app, "Crate", 0.0, 9);
    app.run_headless(90, DT);
    assert!((y_of(&app, crate_id) - 0.5).abs() < 0.05);
    let mut settings = ProjectSettings::default();
    settings.physics3d.layer_matrix.set(8, 9, false);
    app.world.apply_project_settings(settings);
    app.run_headless(90, DT);
    assert!(y_of(&app, crate_id) < -2.0, "the crate drops through after the edit");
}

#[test]
fn physics3d_gravity_solver_and_sleep_thresholds() {
    let fall = |gravity: f32| {
        let mut settings = ProjectSettings::default();
        settings.physics3d.gravity = [0.0, gravity, 0.0];
        let mut app = app_with(settings);
        let id = app.spawn("Ball").at3(0.0, 100.0, 0.0).with(Rigidbody::dynamic()).with(Collider::sphere(0.25)).id();
        app.run_headless(30, DT);
        100.0 - y_of(&app, id)
    };
    let (normal, half) = (fall(-9.81), fall(-4.905));
    assert!((normal / half - 2.0).abs() < 0.1, "{normal} vs {half}");

    // Порог засыпания 0 — тело на земле не засыпает никогда; по умолчанию засыпает.
    let rests = |threshold: Option<f32>, iterations: u32| {
        let mut settings = ProjectSettings::default();
        settings.physics3d.solver_iterations = iterations;
        if let Some(t) = threshold {
            settings.physics3d.sleep_linear_threshold = t;
            settings.physics3d.sleep_angular_threshold = t;
        }
        let mut app = app_with(settings);
        app.spawn("Ground").at3(0.0, -0.5, 0.0).with(Collider::cuboid(40.0, 1.0, 40.0));
        let id = app.spawn("Crate").at3(0.0, 0.5, 0.0).with(Rigidbody::dynamic()).with(Collider::cuboid(1.0, 1.0, 1.0)).id();
        app.run_headless(300, DT);
        let physics = app.world.resources.get::<Physics3D>().unwrap();
        assert_eq!(physics.solver_iterations(), iterations);
        (physics.is_sleeping(id), y_of(&app, id))
    };
    let (asleep, y) = rests(None, 4);
    assert!(asleep && (y - 0.5).abs() < 0.05);
    let (asleep, y) = rests(Some(0.0), 1);
    assert!(!asleep, "zero thresholds keep the body awake");
    assert!((y - 0.5).abs() < 0.05, "one solver iteration still holds a box: y = {y}");
    let (_, y) = rests(Some(0.4), 16);
    assert!((y - 0.5).abs() < 0.05);
}

// ---------------------------------------------------------------------------
// Звук
// ---------------------------------------------------------------------------

#[test]
fn audio_bus_volumes_come_from_the_settings_file() {
    let dir = temp_assets("audio");
    let mut settings = ProjectSettings::default();
    settings.audio.master_volume = 0.5;
    settings.audio.music_volume = 0.25;
    settings.audio.sfx_volume = 0.75;
    settings.audio.ui_volume = 0.0;
    settings.audio.mute_when_unfocused = true;
    write_settings(&dir, &settings);

    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    {
        let audio = app.world.resources.get::<Audio>().unwrap();
        assert_eq!(audio.bus_volume(Bus::Master), 0.5);
        assert_eq!(audio.bus_volume(Bus::Music), 0.25);
        assert_eq!(audio.bus_volume(Bus::Sfx), 0.75);
        assert_eq!(audio.bus_volume(Bus::Ui), 0.0);
    }

    // Без окна (ресурса фокуса нет) звук не глушится.
    app.tick(DT);
    assert!(!app.world.resources.get::<Audio>().unwrap().is_focus_muted());
    // Окно потеряло фокус → тишина; вернулся → звук.
    app.world.resources.insert(WindowFocus { focused: false });
    app.tick(DT);
    assert!(app.world.resources.get::<Audio>().unwrap().is_focus_muted());
    app.world.resources.insert(WindowFocus { focused: true });
    app.tick(DT);
    assert!(!app.world.resources.get::<Audio>().unwrap().is_focus_muted());

    // Настройка выключена — потеря фокуса звук не трогает; громкость, заданная кодом
    // позже, побеждает.
    settings.audio.mute_when_unfocused = false;
    app.world.apply_project_settings(settings);
    app.world.resources.get_mut::<Audio>().unwrap().set_bus_volume(Bus::Music, 0.9);
    app.world.resources.insert(WindowFocus { focused: false });
    app.tick(DT);
    let audio = app.world.resources.get::<Audio>().unwrap();
    assert!(!audio.is_focus_muted());
    assert_eq!(audio.bus_volume(Bus::Music), 0.9);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Время и ввод — через `mount_assets` и настоящий файл
// ---------------------------------------------------------------------------

component! {
    /// Считает фиксированные шаги.
    pub struct StepCounter {
        pub steps: u32 = 0,
    }
}
impl Component for StepCounter {
    fn on_fixed_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        self.steps += 1;
    }
}

#[test]
fn time_and_input_settings_apply_through_mount_assets() {
    let steps = |fixed: Option<f32>| {
        let dir = temp_assets(&format!("time_{}", fixed.map_or(0, |f| (1.0 / f) as u32)));
        if let Some(fixed) = fixed {
            let mut settings = ProjectSettings::default();
            settings.time.fixed_timestep = fixed;
            write_settings(&dir, &settings);
        }
        let mut app = engine::app();
        engine::mount_assets(&mut app, &dir).unwrap();
        let id = app.spawn("Counter").with(StepCounter::default()).id();
        app.run_headless(60, DT);
        let _ = std::fs::remove_dir_all(&dir);
        app.world.scene.get_component::<StepCounter>(id).unwrap().steps
    };
    let default = steps(None);
    let slow = steps(Some(1.0 / 30.0));
    assert!((59..=61).contains(&default), "{default}");
    assert!((29..=31).contains(&slow), "a 1/30 s timestep halves the steps: {slow}");

    // Ввод: «Jump» перепривязан в настройках на J.
    let dir = temp_assets("input");
    let mut settings = ProjectSettings::default();
    let jump = settings.input.actions.iter_mut().find(|a| a.name == "Jump").unwrap();
    *jump = InputAction::button("Jump", &[Binding::Key(Key::J)]);
    write_settings(&dir, &settings);
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    assert!(!app.world.input.button("Jump"), "Space no longer jumps");
    app.world.input.set_key(Key::Space, false);
    app.world.input.set_key(Key::J, true);
    app.tick(DT);
    assert!(app.world.input.button("Jump"), "J from the settings jumps");
    app.world.input.set_key(Key::J, false);
    app.tick(DT);

    // Код игры, привязавший кнопку после `mount_assets`, побеждает.
    app.world.input.set_button_bindings("Jump", vec![Binding::Key(Key::K)]);
    app.world.input.set_key(Key::J, true);
    app.tick(DT);
    assert!(!app.world.input.button("Jump"));
    app.world.input.set_key(Key::K, true);
    app.tick(DT);
    assert!(app.world.input.button("Jump"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Окно (Player) и сцена по умолчанию
// ---------------------------------------------------------------------------

#[test]
fn player_settings_reach_the_window_config() {
    let dir = temp_assets("player");
    let png = {
        let mut bytes = Vec::new();
        RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    };
    std::fs::create_dir_all(dir.join("art")).unwrap();
    std::fs::write(dir.join("art/game_icon.png"), &png).unwrap();
    let mut settings = ProjectSettings::default();
    settings.player.title = "From Settings".into();
    settings.player.width = 1024;
    settings.player.height = 768;
    settings.player.resizable = false;
    settings.player.fullscreen = FullscreenMode::Borderless;
    settings.player.vsync = true;
    settings.player.fps_cap = 30;
    settings.player.icon = "art/game_icon.png".into();
    write_settings(&dir, &settings);

    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    let code = WindowConfig::titled("From Code").with_size(800, 600);
    let config = engine::player_window_config(&app.world, code.clone());
    assert_eq!(config.title, "From Settings");
    assert_eq!((config.width, config.height), (1024, 768));
    assert!(!config.resizable);
    assert_eq!(config.fullscreen, FullscreenMode::Borderless);
    assert!(config.vsync);
    assert_eq!(config.fps_cap, 30);
    assert_eq!(config.frame_period(), Some(std::time::Duration::from_secs_f64(1.0 / 30.0)));
    assert_eq!(config.icon.as_deref(), Some(png.as_slice()));

    // Только изменённые поля: пустой заголовок и размер по умолчанию оставляют код.
    let partial = engine_core::settings::PlayerSettings { fps_cap: 144, ..Default::default() };
    let config = code.clone().with_player_settings(&partial);
    assert_eq!(config.title, "From Code");
    assert_eq!((config.width, config.height), (800, 600));
    assert!(config.resizable && !config.vsync);
    assert_eq!(config.fps_cap, 144);

    // Старый файл без раздела `player` — окно такое, как задал код.
    std::fs::write(dir.join("project.settings"), "(\n    script_order: {},\n)\n").unwrap();
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    let config = engine::player_window_config(&app.world, code.clone().with_vsync(true));
    assert_eq!(config.title, "From Code");
    assert_eq!((config.width, config.height), (800, 600));
    assert!(config.vsync && config.resizable && config.fps_cap == 0 && config.icon.is_none());
    assert_eq!(config.fullscreen, FullscreenMode::Windowed);
    // Без `mount_assets` настроек нет вовсе.
    let config = engine::player_window_config(&engine::app().world, code);
    assert_eq!(config.title, "From Code");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_scene_loads_only_into_an_empty_world() {
    let dir = temp_assets("default_scene");
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    {
        let mut author = engine::app();
        author.spawn("FromScene").at(1.0, 2.0);
        author.world.save_scene(dir.join("scenes/start.scene")).unwrap();
    }
    let mut settings = ProjectSettings::default();
    settings.player.default_scene = "scenes/start.scene".into();
    write_settings(&dir, &settings);

    // `build()` ничего не создала → загружается сцена из настроек.
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    assert!(engine::load_default_scene(&mut app).unwrap());
    let loaded = app.world.scene.find_by_name("FromScene").expect("the default scene is loaded");
    assert_eq!(app.world.scene.world_position(loaded), Vec3::new(1.0, 2.0, 0.0));

    // Сцену собрал код игры → настройка её не заменяет.
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    app.spawn("GameBuilt");
    assert!(!engine::load_default_scene(&mut app).unwrap());
    assert!(app.world.scene.find_by_name("FromScene").is_none());
    assert!(app.world.scene.find_by_name("GameBuilt").is_some());

    // Нет настроек — ничего не делается.
    let mut app = engine::app();
    assert!(!engine::load_default_scene(&mut app).unwrap());

    // Неверный путь — понятная ошибка, а не пустой экран.
    settings.player.default_scene = "scenes/missing.scene".into();
    write_settings(&dir, &settings);
    let mut app = engine::app();
    engine::mount_assets(&mut app, &dir).unwrap();
    assert!(engine::load_default_scene(&mut app).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Графика: ресурс `GraphicsSettings`, который рендер читает каждый кадр
// ---------------------------------------------------------------------------

/// Тесты с GPU идут по одному (драйвер не любит параллельные устройства).
fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

const SIDE: u32 = 96;

/// Ярко освещённый куб на плоскости: тень, блум, туман.
fn lit_scene() -> App {
    let mut app = engine::app();
    app.spawn("Camera")
        .at3(0.0, 3.0, 7.0)
        .rotation(Quat::from_rotation_x(-0.4))
        .with(Camera::perspective(60.0));
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.8, 0.0))
        .with(Light::directional(12.0));
    app.spawn("Environment").with(Environment { bloom: 0.5, fog_density: 0.03, ..Environment::default() });
    app.spawn("Ground").scale(20.0, 1.0, 20.0).with(MeshRenderer::plane());
    app.spawn("Cube").at3(0.0, 0.5, 0.0).with(MeshRenderer::cube().with_color(Color::WHITE));
    app.spawn("Far").at3(0.0, 2.0, -60.0).scale(6.0, 6.0, 6.0).with(MeshRenderer::cube());
    app.run_headless(1, DT);
    app
}

fn render(renderer: &mut Renderer, app: &App) -> (RgbaImage, RenderStats) {
    renderer.capture_view(&app.world, SIDE, SIDE, None).expect("capture")
}

fn differing_pixels(a: &RgbaImage, b: &RgbaImage) -> usize {
    a.pixels().zip(b.pixels()).filter(|(p, q)| p.0.iter().zip(q.0).any(|(x, y)| x.abs_diff(y) > 2)).count()
}

#[test]
fn graphics_settings_change_the_rendered_frame() {
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().expect("a GPU adapter");
    let mut app = lit_scene();

    // Нет ресурса — как раньше; пресет High совпадает с этим кадром.
    let (plain, plain_stats) = render(&mut renderer, &app);
    assert!(plain_stats.shadow_casters > 0, "{plain_stats:?}");
    app.world.resources.insert(GraphicsSettings::preset(QualityLevel::High));
    let (high, high_stats) = render(&mut renderer, &app);
    assert_eq!(differing_pixels(&plain, &high), 0, "High = the old behaviour");
    assert_eq!(high_stats.shadow_casters, plain_stats.shadow_casters);

    // Тени выключены: проход теней пуст, картинка другая.
    app.world.resources.insert(GraphicsSettings { shadows: false, ..GraphicsSettings::preset(QualityLevel::High) });
    let (no_shadows, stats) = render(&mut renderer, &app);
    assert_eq!(stats.shadow_casters, 0, "{stats:?}");
    assert_eq!(stats.shadow_draw_calls, 0, "{stats:?}");
    assert!(differing_pixels(&high, &no_shadows) > 20, "the shadow disappears");

    // Блум выключен / усилен.
    app.world.resources.insert(GraphicsSettings { bloom: false, ..GraphicsSettings::preset(QualityLevel::High) });
    let (no_bloom, _) = render(&mut renderer, &app);
    assert!(differing_pixels(&high, &no_bloom) > 20, "bloom off changes the glow");
    app.world.resources.insert(GraphicsSettings { bloom_intensity: 4.0, ..GraphicsSettings::preset(QualityLevel::High) });
    let (strong_bloom, _) = render(&mut renderer, &app);
    assert!(differing_pixels(&high, &strong_bloom) > 20, "intensity multiplies the glow");
    app.world.resources.insert(GraphicsSettings { bloom_threshold: 50.0, ..GraphicsSettings::preset(QualityLevel::High) });
    let (high_threshold, _) = render(&mut renderer, &app);
    assert!(differing_pixels(&high, &high_threshold) > 20, "the threshold limits what glows");

    // Туман: множитель 0 убирает его.
    app.world.resources.insert(GraphicsSettings { fog_scale: 0.0, ..GraphicsSettings::preset(QualityLevel::High) });
    let (no_fog, _) = render(&mut renderer, &app);
    assert!(differing_pixels(&high, &no_fog) > 20, "fog scale changes the far cube");

    // Дальность теней: потолок 1 м вокруг камеры — далёкий от камеры куб тень не даёт.
    app.world.resources.insert(GraphicsSettings { shadow_distance: 1.0, ..GraphicsSettings::preset(QualityLevel::High) });
    let (_, capped) = render(&mut renderer, &app);
    assert!(capped.shadow_casters < high_stats.shadow_casters, "{capped:?} vs {high_stats:?}");

    // Пресеты Low и High дают разные кадры.
    app.world.resources.insert(GraphicsSettings::preset(QualityLevel::Low));
    let (low, low_stats) = render(&mut renderer, &app);
    assert_eq!(low_stats.shadow_casters, 0);
    assert!(differing_pixels(&high, &low) > 20);
}

component! {
    /// Переключает качество из игрового кода.
    pub struct QualitySwitch {
        pub seen: String = String::new(),
    }
}
impl Component for QualitySwitch {
    fn on_start(&mut self, ctx: &mut Ctx) {
        ctx.set_quality(QualityLevel::Low);
        self.seen = ctx.quality().label().to_string();
    }
}

#[test]
fn quality_is_switched_from_game_code() {
    let mut app = app_with(ProjectSettings::default());
    assert_eq!(app.world.resources.get::<GraphicsSettings>().unwrap().quality, QualityLevel::High);
    let id = app.spawn("Switch").with(QualitySwitch::default()).id();
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<QualitySwitch>(id).unwrap().seen, "Low");
    let graphics = app.world.resources.get::<GraphicsSettings>().unwrap();
    assert_eq!(*graphics, GraphicsSettings::preset(QualityLevel::Low));
    assert!(!graphics.shadows && !graphics.bloom);
}

#[test]
fn subsystems_register_their_settings_hooks() {
    let app = engine::app();
    let hooks = app.world.settings_hooks();
    for name in ["physics2d", "physics3d", "audio"] {
        assert!(hooks.contains(&name), "{name} in {hooks:?}");
    }
}
