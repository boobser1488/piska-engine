//! Разделы `project.settings`: формат файла, обратная совместимость и то, что ядро
//! применяет само (время, действия ввода).

use engine_core::prelude::*;
use engine_core::settings::*;

#[test]
fn an_old_file_with_only_script_order_still_loads() {
    let settings = ProjectSettings::from_ron("(script_order: {\"Mover\": -5})", "test").unwrap();
    assert_eq!(settings.order_of("Mover"), Some(-5));
    assert_eq!(settings.player, PlayerSettings::default());
    assert_eq!(settings.physics3d.gravity, [0.0, -9.81, 0.0]);
    assert_eq!(settings.input, InputSettings::default());
}

#[test]
fn unknown_fields_do_not_break_loading() {
    let settings = ProjectSettings::from_ron("(from_the_future: 3, time: (time_scale: 0.5))", "test").unwrap();
    assert_eq!(settings.time.time_scale, 0.5);
}

#[test]
fn every_section_round_trips_through_ron() {
    let mut settings = ProjectSettings::default();
    settings.player.title = "My Game".into();
    settings.player.fullscreen = FullscreenMode::Borderless;
    settings.player.default_scene = "scenes/menu.scene".into();
    settings.time.fixed_timestep = 0.02;
    settings.physics2d.gravity = [1.0, -3.0];
    settings.physics3d.layer_matrix.set(3, 7, false);
    settings.tags_and_layers.add_tag("Enemy");
    settings.tags_and_layers.layers[8] = "Enemies".into();
    settings.input.actions.push(InputAction::button("Dash", &[Binding::Key(Key::Q)]));
    settings.graphics = GraphicsSettings::preset(QualityLevel::Low);
    settings.audio.music_volume = 0.25;
    settings.set_order("Mover", 7);
    let text = settings.to_ron();
    assert_eq!(ProjectSettings::from_ron(&text, "test").unwrap(), settings);
}

#[test]
fn the_layer_matrix_is_symmetric() {
    let mut matrix = LayerMatrix::default();
    assert!(matrix.collides(2, 9));
    matrix.set(2, 9, false);
    assert!(!matrix.collides(9, 2));
    assert!(!matrix.collides(2, 9));
    assert!(matrix.collides(2, 2));
    assert_eq!(matrix.mask(2) & (1 << 9), 0);
    // Вручную испорченный файл (разрешено только в одной строке) приводится к запрету.
    let mut rows = vec![u32::MAX; 32];
    rows[4] &= !(1 << 5);
    let fixed = LayerMatrix::from(rows);
    assert!(!fixed.collides(5, 4));
}

#[test]
fn layers_and_tags_have_stable_names() {
    let mut tags = TagsAndLayers::default();
    assert_eq!(tags.layer_name(0), "Default");
    assert_eq!(tags.layer_name(9), "Layer 9");
    assert_eq!(tags.layer_by_name("UI"), Some(5));
    assert!(tags.has_tag("Untagged"));
    assert!(!tags.has_tag("Boss"));
    assert!(tags.add_tag("Boss"));
    assert!(!tags.add_tag("Boss"));
    assert!(!tags.add_tag("  "));
    assert!(tags.has_tag("Boss"));
    assert_eq!(TagsAndLayers { tags: vec![], layers: vec![] }.normalized().layers.len(), LAYER_COUNT);
}

#[test]
fn default_input_settings_match_the_built_in_actions() {
    let mut input = Input::default();
    let before_axes = input.axis_names().len();
    InputSettings::default().apply(&mut input);
    assert_eq!(input.axis_names().len(), before_axes);
    assert_eq!(input.button_bindings("Jump"), Input::default().button_bindings("Jump"));
    assert_eq!(input.axis_bindings("Horizontal"), Input::default().axis_bindings("Horizontal"));
}

#[test]
fn an_action_edited_in_settings_drives_input() {
    let mut settings = ProjectSettings::default();
    let jump = settings.input.actions.iter_mut().find(|a| a.name == "Jump").unwrap();
    jump.bindings = vec![Binding::Key(Key::J).to_text()];
    settings.input.actions.push(InputAction::button("Dash", &[Binding::Key(Key::Q)]));
    let mut app = App::new();
    app.world.apply_project_settings(settings);
    app.world.input.set_key(Key::J, true);
    assert!(app.world.input.button("Jump"));
    app.world.input.set_key(Key::J, false);
    app.world.input.set_key(Key::Space, true);
    assert!(!app.world.input.button("Jump"), "the old binding is gone");
    app.world.input.set_key(Key::Q, true);
    assert!(app.world.input.button("Dash"));
}

#[test]
fn time_settings_reach_the_world_and_change_the_step_count() {
    let mut settings = ProjectSettings::default();
    settings.time.fixed_timestep = 0.02;
    settings.time.max_timestep = 0.05;
    settings.time.time_scale = 0.5;
    let mut app = App::new();
    app.world.apply_project_settings(settings);
    assert_eq!(app.world.time.fixed_delta, 0.02);
    assert_eq!(app.world.time.time_scale, 0.5);
    // Подвисание в целую секунду обрезается до максимума, потом масштабируется.
    app.tick(1.0);
    assert!((app.world.time.unscaled_delta - 0.05).abs() < 1e-6);
    assert!((app.world.time.delta - 0.025).abs() < 1e-6);
    assert!(app.profile().fixed_steps >= 1);

    let mut app = App::new();
    let mut fast = ProjectSettings::default();
    fast.time.fixed_timestep = 0.005;
    app.world.apply_project_settings(fast);
    app.tick(1.0 / 60.0);
    assert!(app.profile().fixed_steps >= 3, "a shorter step runs more often");
}

#[test]
fn bad_time_values_are_clamped() {
    let mut settings = ProjectSettings::default();
    settings.time.fixed_timestep = 0.0;
    settings.time.time_scale = f32::NAN;
    let mut app = App::new();
    app.world.apply_project_settings(settings);
    assert!(app.world.time.fixed_delta >= 0.001);
    assert_eq!(app.world.time.time_scale, 1.0);
}

#[test]
fn quality_presets_differ() {
    let low = GraphicsSettings::preset(QualityLevel::Low);
    let high = GraphicsSettings::preset(QualityLevel::High);
    assert!(!low.shadows && high.shadows);
    assert!(!low.bloom && high.bloom);
    assert_eq!(GraphicsSettings::default(), high);
}
