//! Игровая логика на C++: компоненты из `plugins/cpp-gameplay/gameplay.cpp`
//! (здесь — вкомпилированные статически) ведут себя как компоненты на Rust.

use engine_core::prelude::*;
use engine_cpp_bridge::{builtin_gameplay, CppPlugins};

const DT: f32 = 1.0 / 60.0;

component! {
    /// Компонент на Rust, который слушает сообщения от C++.
    pub struct Listener {
        pub heard: i32 = 0,
        pub last: f32 = 0.0,
    }
}

impl Component for Listener {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        if let ("Jumped", Value::Float(count)) = (message.name.as_str(), &message.value) {
            self.heard += 1;
            self.last = *count;
        }
    }
}

fn app() -> App {
    let mut app = App::new();
    app.register::<Listener>();
    let mut plugins = CppPlugins::default();
    let plugin = builtin_gameplay(&mut plugins).expect("the built-in C++ module loads");
    assert_eq!(plugin.name(), "piska_gameplay");
    // Компоненты из gameplay.cpp и скрипты из папки scripts/ плагина.
    let names = plugin.component_names();
    assert!(names.starts_with(&["Spinner", "Bobber", "Mover"]), "{names:?}");
    assert!(names.contains(&"LightFlicker"), "{names:?}");
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    app
}

fn spawn_with(app: &mut App, name: &str, kind: &str) -> GameObjectId {
    let component = app.world.registry.create(kind).expect("the C++ component is registered");
    let id = app.spawn(name).id();
    app.world.scene.get_mut(id).unwrap().add_boxed(component);
    id
}

fn field(app: &App, id: GameObjectId, kind: &str, name: &str) -> Value {
    app.world
        .scene
        .get(id)
        .unwrap()
        .components()
        .find(|c| c.type_name() == kind)
        .unwrap_or_else(|| panic!("component `{kind}`"))
        .get_field(name)
        .unwrap_or_else(|| panic!("field `{kind}.{name}`"))
}

#[test]
fn cpp_fields_are_reflected_like_rust_fields() {
    let app = app();
    let mut bobber = app.world.registry.create("Bobber").unwrap();
    assert_eq!(bobber.type_name(), "Bobber");
    assert_eq!(bobber.field_names(), ["amplitude", "period", "enabled"]);
    // Значения по умолчанию заданы в C++-классе.
    assert_eq!(bobber.get_field("amplitude"), Some(Value::Vec3(0.0, 0.5, 0.0)));
    assert_eq!(bobber.get_field("period"), Some(Value::Float(2.0)));
    assert_eq!(bobber.get_field("enabled"), Some(Value::Bool(true)));

    bobber.set_field("period", &Value::Float(0.5)).unwrap();
    bobber.set_field("enabled", &Value::Bool(false)).unwrap();
    assert_eq!(bobber.get_field("period"), Some(Value::Float(0.5)));
    assert_eq!(bobber.get_field("enabled"), Some(Value::Bool(false)));

    // Те же понятные ошибки, что у компонентов на Rust.
    let error = bobber.set_field("period", &Value::Str("fast".into())).unwrap_err();
    assert_eq!(error.to_string(), "field `Bobber.period` expects Float, but got Str");
    let error = bobber.set_field("perod", &Value::Float(1.0)).unwrap_err();
    assert!(error.to_string().contains("amplitude, period, enabled"), "{error}");
}

#[test]
fn cpp_component_runs_its_hooks_every_frame() {
    let mut app = app();
    let cube = spawn_with(&mut app, "Cube", "Spinner");
    app.run_headless(60, DT);
    // 90°/с по умолчанию → через секунду 90°.
    let yaw = app.world.scene.get(cube).unwrap().transform.euler_deg().y;
    assert!((yaw - 90.0).abs() < 0.5, "yaw after one second: {yaw}");

    // Поле изменили «из инспектора» — C++ видит новое значение.
    let object = app.world.scene.get_mut(cube).unwrap();
    let spinner = object.components_mut().next().unwrap();
    spinner.set_field("speed_deg", &Value::Float(-180.0)).unwrap();
    app.run_headless(30, DT);
    let yaw = app.world.scene.get(cube).unwrap().transform.euler_deg().y;
    assert!(yaw.abs() < 0.5, "90° − 180°/с × 0.5 с = 0°, got {yaw}");
}

#[test]
fn cpp_reads_input_and_talks_to_rust_components() {
    let mut app = app();
    let player = spawn_with(&mut app, "Player", "Mover");
    app.world.scene.get_mut(player).unwrap().add(Listener::default());
    app.run_headless(2, DT);

    app.world.input.set_key(Key::D, true);
    app.run_headless(60, DT);
    app.world.input.set_key(Key::D, false);
    let x = app.world.scene.get(player).unwrap().transform.position.x;
    assert!((x - 4.0).abs() < 0.05, "4 m/s for a second: x = {x}");

    // Нажатие «Jump» C++ считает в своём поле и сообщает о нём компоненту на Rust.
    app.world.input.set_key(Key::Space, true);
    app.tick(DT);
    app.world.input.set_key(Key::Space, false);
    app.run_headless(2, DT);
    assert_eq!(field(&app, player, "Mover", "jumps"), Value::Int(1));
    let listener = app.world.scene.get_component::<Listener>(player).unwrap();
    assert_eq!((listener.heard, listener.last), (1, 1.0));
}

#[test]
fn cpp_components_are_saved_and_loaded_with_the_scene() {
    let mut app = app();
    let buoy = spawn_with(&mut app, "Buoy", "Bobber");
    let object = app.world.scene.get_mut(buoy).unwrap();
    let bobber = object.components_mut().next().unwrap();
    bobber.set_field("amplitude", &Value::Vec3(1.0, 0.0, 2.0)).unwrap();
    bobber.set_field("period", &Value::Float(4.0)).unwrap();

    let saved = app.world.scene_to_string();
    assert!(saved.contains(r#"kind: "Bobber""#), "{saved}");
    assert!(saved.contains(r#""amplitude": Vec3(1.0, 0.0, 2.0)"#), "{saved}");

    app.world.load_scene_str(&saved, "saved.scene").unwrap();
    let buoy = app.world.scene.find_by_name("Buoy").unwrap();
    assert_eq!(field(&app, buoy, "Bobber", "amplitude"), Value::Vec3(1.0, 0.0, 2.0));
    assert_eq!(field(&app, buoy, "Bobber", "period"), Value::Float(4.0));

    // И работает после загрузки: через четверть периода объект в крайней точке.
    app.run_headless(60, DT);
    let position = app.world.scene.get(buoy).unwrap().transform.position;
    assert!((position.x - 1.0).abs() < 0.05 && (position.z - 2.0).abs() < 0.1, "{position}");

    // Опечатка в имени типа — та же подсказка, что для компонентов на Rust.
    let error = app.world.registry.create("Spiner").err().unwrap().to_string();
    assert!(error.contains("Did you mean `Spinner`?"), "{error}");
}

#[test]
fn destroying_an_object_destroys_the_cpp_instance() {
    let mut app = app();
    let cube = spawn_with(&mut app, "Cube", "Spinner");
    app.run_headless(2, DT);
    app.world.scene.destroy(cube);
    app.run_headless(2, DT); // экземпляр удаляется деструктором C++, без падений и утечек
    assert!(!app.world.scene.contains(cube));
    assert_eq!(app.world.scene.len(), 0);
}
