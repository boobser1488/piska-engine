//! Необязательный C++-пример: один компонент `CppSpinner`, статически влинкованный в игру.
//! Запуск: `cargo test -p marble --features cpp-example --test cpp_example`.
#![cfg(feature = "cpp-example")]

mod common;

use common::*;
use engine::prelude::*;

#[test]
fn the_cpp_component_is_linked_into_the_game_and_runs() {
    let mut app = lab();
    assert!(app.world.registry.contains("CppSpinner"), "the static C++ component is registered by name");
    let id = app.world.scene.spawn("Spinner");
    let mut spinner = app.world.registry.create("CppSpinner").expect("create by name");
    spinner.set_field("speed_deg", &Value::Float(120.0)).expect("a reflected C++ field");
    app.world.scene.get_mut(id).unwrap().add_boxed(spinner);
    app.run_headless(2, DT);
    let before = app.world.scene.get(id).unwrap().transform.rotation;
    app.run_headless(30, DT);
    let after = app.world.scene.get(id).unwrap().transform.rotation;
    let angle = before.angle_between(after).to_degrees();
    assert!((50.0..70.0).contains(&angle), "120 deg/s for half a second turns the object by 60 deg, got {angle}");
}
