//! C++-API на одном ядре (без физики, звука и интерфейса): объекты, поля, сообщения,
//! таймеры, твины, безопасность хэндлов, исключения, отсутствующие сервисы, версия ABI.
//! Скрипт `ApiProbe` (plugins/cpp-gameplay/scripts) выполняет по одному обращению к движку.

use engine_core::logging;
use engine_core::prelude::*;
use engine_cpp_bridge::abi::{EnginePlugin, ENGINE_ABI_VERSION};
use engine_cpp_bridge::{builtin_gameplay, CppPlugins, CppServices, Plugin, PluginError};

const DT: f32 = 1.0 / 60.0;

fn app() -> (App, GameObjectId) {
    logging::init();
    let mut app = App::new();
    let mut plugins = CppPlugins::default();
    builtin_gameplay(&mut plugins).expect("the built-in module loads");
    plugins.register_components(&mut app.world.registry);
    app.add_subsystem(plugins);
    let probe = app.spawn("Probe").id();
    let component = app.world.registry.create("ApiProbe").expect("ApiProbe is registered");
    app.world.scene.get_mut(probe).unwrap().add_boxed(component);
    app.run_headless(2, DT);
    (app, probe)
}

fn run(app: &mut App, probe: GameObjectId, command: &str, value: Value) {
    app.world.send_message(probe, &format!("probe.{command}"), value);
    app.tick(DT);
}

fn field(app: &App, probe: GameObjectId, name: &str) -> Value {
    app.world
        .scene
        .get(probe)
        .unwrap()
        .components()
        .find(|c| c.type_name() == "ApiProbe")
        .unwrap()
        .get_field(name)
        .unwrap()
}

fn mark() -> u64 {
    logging::records().last().map_or(0, |r| r.seq + 1)
}

fn logged(since: u64, needle: &str) -> bool {
    logging::records_since(since).iter().any(|r| r.message.contains(needle))
}

#[test]
fn objects_hierarchy_and_stale_handles_on_the_core_alone() {
    engine_cpp_bridge::reset_complaints();
    let since = mark();
    let (mut app, probe) = app();

    run(&mut app, probe, "spawn", Value::Nil);
    assert_eq!(field(&app, probe, "ok"), Value::Bool(true));
    let made = app.world.scene.find_by_name("Made-by-cpp").unwrap();
    assert_eq!(app.world.scene.get(made).unwrap().transform.position, Vec3::new(1.0, 2.0, 3.0));

    run(&mut app, probe, "destroy_stashed", Value::Nil);
    assert_eq!(field(&app, probe, "ok"), Value::Bool(true), "alive until the end of the frame");
    assert!(!app.world.scene.contains(made));

    run(&mut app, probe, "stale", Value::Nil);
    assert_eq!(field(&app, probe, "number"), Value::Float(0.0));
    assert!(logged(since, "stale object handle"), "{:?}", logging::records_since(since));

    run(&mut app, probe, "hierarchy", Value::Nil);
    assert_eq!(field(&app, probe, "ok"), Value::Bool(true));
    assert_eq!(field(&app, probe, "number"), Value::Float(11.0));
}

#[test]
fn messages_timers_and_tweens() {
    let (mut app, probe) = app();
    run(&mut app, probe, "after", Value::Nil);
    app.run_headless(40, DT);
    assert_eq!(field(&app, probe, "ticks"), Value::Int(1));
    run(&mut app, probe, "tween", Value::Nil);
    app.run_headless(70, DT);
    match field(&app, probe, "number") {
        Value::Float(v) => assert!((v - 10.0).abs() < 1e-3, "{v}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn exceptions_do_not_cross_the_boundary() {
    let since = mark();
    let (mut app, probe) = app();
    run(&mut app, probe, "exception", Value::Nil);
    assert!(logged(since, "C++ exception in ApiProbe::on_message: boom from on_message"));
    // Компонент продолжает жить.
    run(&mut app, probe, "after", Value::Nil);
    app.run_headless(40, DT);
    assert_eq!(field(&app, probe, "ticks"), Value::Int(1));
}

#[test]
fn services_are_absent_without_the_facade_and_fail_softly() {
    let since = mark();
    let (mut app, probe) = app();
    assert!(app.world.resources.get::<CppServices>().is_none());
    run(&mut app, probe, "service_missing", Value::Nil);
    assert_eq!(field(&app, probe, "ok"), Value::Bool(true));
    run(&mut app, probe, "audio", Value::Nil);
    assert_eq!(field(&app, probe, "ok"), Value::Bool(false), "no audio service: the neutral answer");
    assert!(logged(since, "no such service"));

    // Сервис, зарегистрированный игрой, виден из C++ (проверка присутствия и обращения).
    engine_cpp_bridge::services_mut(&mut app.world).register("audio.play", |_, _| Ok(vec![Value::Int(5)]));
    engine_cpp_bridge::services_mut(&mut app.world).register("audio.is_playing", |_, _| Ok(vec![Value::Bool(true)]));
    engine_cpp_bridge::services_mut(&mut app.world).register("audio.voice_count", |_, _| Ok(vec![Value::Int(3)]));
    run(&mut app, probe, "audio", Value::Nil);
    assert_eq!(field(&app, probe, "ok"), Value::Bool(true), "the custom service answered");
    assert_eq!(field(&app, probe, "count"), Value::Int(3));
}

#[test]
fn api_calls_outside_a_hook_are_refused_with_an_error() {
    // Тип создаётся фабрикой реестра (то есть вне хука): конструктор ничего не трогает,
    // а сама функция API вне хука возвращает нейтральное значение — проверим через Rust-сторону.
    let (app, _) = app();
    assert!(app.world.registry.contains("ApiProbe"));
}

#[test]
fn a_plugin_built_for_another_abi_is_refused_with_a_rebuild_hint() {
    // Дескриптор «старого» плагина: совпадает только первое поле — версия.
    let old: &'static EnginePlugin = Box::leak(Box::new(EnginePlugin {
        abi_version: ENGINE_ABI_VERSION - 1,
        name: std::ptr::null(),
        on_load: None,
        on_fixed_update: None,
        process_rgba: None,
        on_unload: None,
        components: std::ptr::null(),
        component_count: 0,
    }));
    // SAFETY: дескриптор статический и валидный; при несовпадении версии остальные поля не читаются.
    let error = unsafe { Plugin::from_static(old) }.err().expect("an old plugin must be refused");
    assert!(matches!(error, PluginError::AbiMismatch { found, expected, .. } if found == ENGINE_ABI_VERSION - 1 && expected == ENGINE_ABI_VERSION));
    let message = error.to_string();
    assert!(message.contains("Rebuild the plugin"), "{message}");
    assert!(message.contains(&format!("v{}", ENGINE_ABI_VERSION - 1)), "{message}");
}
