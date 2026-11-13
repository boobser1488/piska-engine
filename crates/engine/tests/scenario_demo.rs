//! Демо `examples/scenario_demo.rs` без окна: сценарий доигрывает до конца, кубы
//! стоят в ряд, надпись сменилась, лампа пульсирует; кадр рендерится.

use engine::prelude::*;

#[path = "../examples/scenario_demo.rs"]
mod demo;

fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn caption(app: &App) -> String {
    let id = app.world.scene.find_by_name("Caption").unwrap();
    app.world.scene.get_component::<Text>(id).unwrap().text.clone()
}

#[test]
fn the_demo_scenario_plays_to_the_end() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = demo::build().unwrap();
    app.run_headless(30, 1.0 / 60.0);
    assert_eq!(caption(&app), "2…", "counting down");
    let lamp = app.world.scene.find_by_name("Lamp").unwrap();
    let mut intensities = Vec::new();
    for _ in 0..240 {
        app.run_headless(1, 1.0 / 60.0);
        intensities.push(app.world.scene.get_component::<Light>(lamp).unwrap().intensity);
    }
    let errors = engine::scenario::take_errors(&mut app.world);
    assert!(errors.is_empty(), "{}", errors[0]);
    assert_eq!(caption(&app), "Готово: сценарий доиграл");
    for (i, name) in ["Red", "Gold", "Blue"].iter().enumerate() {
        let p = app.world.scene.get(app.world.scene.find_by_name(name).unwrap()).unwrap().transform.position;
        assert!((p - Vec3::new(-2.5 + i as f32 * 2.5, 0.6, 1.5)).length() < 1e-3, "{name} at {p}");
    }
    let (lo, hi) = intensities.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    assert!(lo < 12.0 && hi > 35.0, "the lamp pulses: {lo}..{hi}");
}

#[test]
fn the_demo_renders() {
    std::env::set_var("ENGINE_AUDIO", "off");
    let _gpu = gpu_lock();
    let mut app = demo::build().unwrap();
    app.run_headless(150, 1.0 / 60.0);
    let mut renderer = engine::render::Renderer::headless().unwrap();
    let image = renderer.capture(&app.world, 640, 360).unwrap();
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("scenario_demo.png");
    image.save(&out).unwrap();
    // Кадр не пустой: есть и тёмные, и светлые пиксели.
    let lum: Vec<u32> = image.pixels().map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).collect();
    assert!(lum.iter().any(|&l| l > 500) && lum.iter().any(|&l| l < 300));
}
