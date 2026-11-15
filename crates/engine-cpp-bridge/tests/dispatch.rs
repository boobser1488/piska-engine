//! Цена вызова хуков C++-компонентов: диспетчер зовёт только те хуки, что класс
//! реализует (`Component::hooks` у `CppComponent`). Замер: `cargo test -p engine-cpp-bridge
//! --test dispatch -- --nocapture` печатает мкс на кадр для 3 000 `Spinner` (только
//! `on_update`).

use std::time::Instant;

use engine_core::prelude::*;
use engine_cpp_bridge::{builtin_gameplay, CppPlugins};

const DT: f32 = 1.0 / 60.0;
const COUNT: usize = 3_000;

fn app_with_spinners() -> (App, GameObjectId) {
    let mut app = App::new();
    let mut plugins = CppPlugins::default();
    builtin_gameplay(&mut plugins).expect("the built-in C++ module loads");
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    let mut first = None;
    for i in 0..COUNT {
        let component = app.world.registry.create("Spinner").expect("Spinner is registered");
        let id = app.spawn(format!("Spinner {i}")).id();
        app.world.scene.get_mut(id).unwrap().add_boxed(component);
        first.get_or_insert(id);
    }
    (app, first.expect("spawned"))
}

#[test]
fn cpp_components_only_get_the_hooks_they_implement() {
    let (app, id) = app_with_spinners();
    let object = app.world.scene.get(id).unwrap();
    let spinner = object.components().find(|c| c.type_name() == "Spinner").unwrap();
    // Spinner реализует только on_update.
    assert_eq!(spinner.hooks(), Hooks::UPDATE);
}

#[test]
fn dispatch_cost_of_three_thousand_cpp_spinners() {
    let (mut app, id) = app_with_spinners();
    app.run_headless(10, DT);
    let mut times = Vec::new();
    for _ in 0..5 {
        let clock = Instant::now();
        app.run_headless(60, DT);
        times.push(clock.elapsed().as_secs_f64() * 1e6 / 60.0);
    }
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    eprintln!("DISPATCH cpp_spinners_3000 median_us_per_frame={median:.1} runs={times:?}");
    // Спиннеры крутятся: on_update вызывается.
    let rotation = app.world.scene.get(id).unwrap().transform.rotation;
    assert!(rotation != Quat::IDENTITY);
    // Грубый потолок (отладочная сборка, общий процессор): 3 000 вызовов on_update за кадр.
    assert!(median < 20_000.0, "{median} us per frame");
}
