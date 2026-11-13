//! Порядок выполнения скриптов (Script Execution Order): порядок типа из кода и из
//! настроек проекта, устойчивость к равным значениям, добавление и удаление компонентов
//! посреди кадра, смена сцены, быстрый путь без порядка.

use engine_core::prelude::*;

#[derive(Default)]
struct Log(Vec<String>);

fn note(ctx: &mut Ctx, hook: &str, who: &str) {
    let name = ctx.object().name.clone();
    ctx.resource_or_default::<Log>().0.push(format!("{who}:{hook}:{name}"));
}

fn take(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world.resources.get_or_insert_with(Log::default).0)
}

/// Компонент-«журналист»: пишет в журнал каждый хук, порядок из кода задаёт `$order`.
macro_rules! recorder {
    ($name:ident, $order:expr) => {
        component! { pub struct $name {} }
        impl Component for $name {
            fn execution_order(&self) -> i32 {
                $order
            }
            fn on_start(&mut self, ctx: &mut Ctx) {
                note(ctx, "start", stringify!($name));
            }
            fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
                note(ctx, "fixed", stringify!($name));
            }
            fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
                note(ctx, "update", stringify!($name));
            }
            fn on_late_update(&mut self, ctx: &mut Ctx, _dt: f32) {
                note(ctx, "late", stringify!($name));
            }
        }
    };
}

recorder!(Early, -10);
recorder!(Normal, 0);
recorder!(Late, 10);
recorder!(Peer, 0);

/// Три объекта с компонентами в разном порядке добавления.
fn interleaved() -> App {
    let mut app = App::new();
    let world = &mut app.world;
    let a = world.scene.spawn("a");
    let b = world.scene.spawn("b");
    let c = world.scene.spawn("c");
    let obj = world.scene.get_mut(a).unwrap();
    obj.add(Late::default());
    obj.add(Early::default());
    obj.add(Normal::default());
    let obj = world.scene.get_mut(b).unwrap();
    obj.add(Normal::default());
    obj.add(Late::default());
    obj.add(Early::default());
    let obj = world.scene.get_mut(c).unwrap();
    obj.add(Early::default());
    obj.add(Normal::default());
    app
}

fn hook(log: &[String], name: &str) -> Vec<String> {
    log.iter()
        .filter(|line| line.split(':').nth(1) == Some(name))
        .map(|line| {
            let mut parts = line.split(':');
            let who = parts.next().unwrap();
            let _ = parts.next();
            format!("{who}@{}", parts.next().unwrap())
        })
        .collect()
}

const EXPECTED: [&str; 8] = [
    "Early@a", "Early@b", "Early@c", "Normal@a", "Normal@b", "Normal@c", "Late@a", "Late@b",
];

#[test]
fn every_hook_runs_by_order_then_hierarchy() {
    let mut app = interleaved();
    app.world.start_pending();
    app.world.fixed_update(0.02);
    app.world.update(0.02);
    app.world.late_update(0.02);
    let log = take(&mut app);
    for name in ["start", "fixed", "update", "late"] {
        assert_eq!(hook(&log, name), EXPECTED, "hook {name}: {log:?}");
    }
}

#[test]
fn equal_orders_keep_hierarchy_and_component_order() {
    // Два Normal на одном объекте и объекты вперемешку: при равном порядке — как раньше.
    let mut app = App::new();
    let a = app.world.scene.spawn("a");
    let b = app.world.scene.spawn("b");
    let obj = app.world.scene.get_mut(a).unwrap();
    obj.add(Normal::default());
    obj.add(Late::default());
    let obj = app.world.scene.get_mut(b).unwrap();
    obj.add(Late::default());
    obj.add(Normal::default());
    app.world.start_pending();
    app.world.update(0.02);
    assert_eq!(hook(&take(&mut app), "update"), ["Normal@a", "Normal@b", "Late@a", "Late@b"]);

    // Ребёнок идёт после родителя, а не после соседа родителя.
    let child = app.world.scene.spawn("child");
    app.world.scene.set_parent(child, Some(a)).unwrap();
    app.world.scene.get_mut(child).unwrap().add(Normal::default());
    app.world.start_pending();
    take(&mut app);
    app.world.update(0.02);
    assert_eq!(
        hook(&take(&mut app), "update"),
        ["Normal@a", "Normal@child", "Normal@b", "Late@a", "Late@b"]
    );
}

#[test]
fn moving_a_component_changes_the_order_of_equal_numbers_only() {
    for with_orders in [false, true] {
        let mut app = App::new();
        let a = app.world.scene.spawn("a");
        let obj = app.world.scene.get_mut(a).unwrap();
        obj.add(Normal::default());
        obj.add(Peer::default());
        if with_orders {
            // Сцена в «порядковом» режиме: Late (10) на другом объекте.
            let b = app.world.scene.spawn("b");
            app.world.scene.get_mut(b).unwrap().add(Late::default());
        }
        app.world.start_pending();
        app.world.update(0.02);
        let first: Vec<String> = hook(&take(&mut app), "update").into_iter().filter(|l| l.ends_with("@a")).collect();
        assert_eq!(first, ["Normal@a", "Peer@a"]);

        assert!(app.world.scene.get_mut(a).unwrap().move_component(1, 0));
        assert!(!app.world.scene.get_mut(a).unwrap().move_component(1, 1), "same place");
        assert!(!app.world.scene.get_mut(a).unwrap().move_component(0, 5), "out of range");
        app.world.update(0.02);
        let moved: Vec<String> = hook(&take(&mut app), "update").into_iter().filter(|l| l.ends_with("@a")).collect();
        assert_eq!(moved, ["Peer@a", "Normal@a"], "with_orders = {with_orders}");
    }
}

#[test]
fn project_settings_override_the_code_default() {
    let mut app = interleaved();
    // Normal (0 в коде) → −20: теперь он первый. Late (10 в коде) → −10: равен Early,
    // значит идёт по иерархии вместе с ним.
    let mut settings = ProjectSettings::default();
    settings.set_order("Normal", -20);
    settings.set_order("Late", -10);
    app.world.apply_project_settings(settings);
    app.world.start_pending();
    app.world.update(0.02);
    let log = take(&mut app);
    assert_eq!(
        hook(&log, "update"),
        // Normal(-20): a, b, c; затем Early/Late(-10) по иерархии и слотам.
        ["Normal@a", "Normal@b", "Normal@c", "Late@a", "Early@a", "Late@b", "Early@b", "Early@c"],
        "{log:?}"
    );
}

#[test]
fn settings_apply_to_components_created_later_and_survive_scene_replacement() {
    let mut app = App::new();
    let mut settings = ProjectSettings::default();
    settings.set_order("Late", -5);
    app.world.apply_project_settings(settings);
    app.world.registry.register::<Late>().register::<Normal>();

    let text = r#"(name: "s", objects: [(name: "x", components: [(kind: "Normal"), (kind: "Late")])])"#;
    app.world.load_scene_str(text, "test").expect("scene loads");
    app.world.start_pending();
    app.world.update(0.02);
    assert_eq!(hook(&take(&mut app), "update"), ["Late@x", "Normal@x"]);
}

#[test]
fn changing_the_orders_at_runtime_resorts_once() {
    let mut app = interleaved();
    app.world.start_pending();
    app.world.update(0.02);
    assert_eq!(hook(&take(&mut app), "update"), EXPECTED);

    let mut settings = ProjectSettings::default();
    settings.set_order("Early", 100);
    app.world.apply_project_settings(settings);
    app.world.update(0.02);
    let log = take(&mut app);
    assert_eq!(
        hook(&log, "update"),
        ["Normal@a", "Normal@b", "Normal@c", "Late@a", "Late@b", "Early@a", "Early@b", "Early@c"]
    );

    // Сбросили настройки — вернулся порядок из кода.
    app.world.apply_project_settings(ProjectSettings::default());
    app.world.update(0.02);
    assert_eq!(hook(&take(&mut app), "update"), EXPECTED);
}

component! {
    /// Добавляет `Normal` объекту `a` и убирает `Late` с объекта `b` в своём `on_update`.
    pub struct Meddler {}
}
impl Component for Meddler {
    fn execution_order(&self) -> i32 {
        -100
    }
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let a = ctx.find_by_name("a").unwrap();
        let b = ctx.find_by_name("b").unwrap();
        if ctx.get_on::<Normal>(a).is_none() {
            ctx.world.scene.get_mut(a).unwrap().add(Normal::default());
        }
        ctx.world.scene.get_mut(b).unwrap().remove::<Late>();
    }
}

#[test]
fn components_added_or_removed_mid_frame() {
    let mut app = App::new();
    let a = app.world.scene.spawn("a");
    let b = app.world.scene.spawn("b");
    app.world.scene.get_mut(a).unwrap().add(Meddler::default());
    app.world.scene.get_mut(b).unwrap().add(Late::default());
    app.world.scene.get_mut(b).unwrap().add(Early::default());
    app.world.start_pending();
    take(&mut app);

    // Кадр 1: Meddler (−100) добавил Normal объекту a и убрал Late у b. Убранный
    // компонент в этом кадре уже не вызывается; добавленный начнёт со следующего кадра
    // (в сценах с порядком выполнения список вызова — снимок на начало фазы).
    app.world.update(0.02);
    assert_eq!(hook(&take(&mut app), "update"), ["Early@b"]);

    // Кадр 2: Normal на a получает on_start и уже обновляется.
    app.world.start_pending();
    app.world.update(0.02);
    let log = take(&mut app);
    assert_eq!(hook(&log, "start"), ["Normal@a"]);
    assert_eq!(hook(&log, "update"), ["Early@b", "Normal@a"]);
}

component! {
    /// В `on_start` добавляет `Early` объекту `later` (стоит дальше в иерархии).
    pub struct StartAdder {}
}
impl Component for StartAdder {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let target = ctx.find_by_name("later").unwrap();
        ctx.world.scene.get_mut(target).unwrap().add(Early::default());
    }
}

#[test]
fn components_added_in_on_start_start_in_the_same_phase() {
    let mut app = App::new();
    let first = app.world.scene.spawn("first");
    let later = app.world.scene.spawn("later");
    app.world.scene.get_mut(first).unwrap().add(StartAdder::default());
    app.world.scene.get_mut(later).unwrap().add(Late::default());
    app.world.start_pending();
    let log = take(&mut app);
    // Late (10) стартует в первом «круге», добавленный Early — во втором, в этой же фазе.
    assert!(hook(&log, "start").contains(&"Early@later".to_string()), "{log:?}");
    app.world.update(0.02);
    assert_eq!(hook(&take(&mut app), "update"), ["Early@later", "Late@later"]);
}

#[test]
fn settings_file_round_trip() {
    let mut settings = ProjectSettings::default();
    settings.set_order("Mover", -50);
    settings.set_order("Camera Follow", 100);
    let text = settings.to_ron();
    assert_eq!(ProjectSettings::from_ron(&text, "t").unwrap(), settings);
    assert_eq!(ProjectSettings::from_ron("", "t").unwrap(), ProjectSettings::default());
    assert_eq!(ProjectSettings::from_ron("()", "t").unwrap(), ProjectSettings::default());
    let error = ProjectSettings::from_ron("(script_order: 5)", "x.settings").unwrap_err().to_string();
    assert!(error.contains("x.settings") && error.contains("delete it"), "{error}");

    let dir = std::env::temp_dir().join(format!("piska_settings_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("project.settings");
    assert_eq!(ProjectSettings::load(&path).unwrap(), ProjectSettings::default(), "no file = defaults");
    settings.save(&path).unwrap();
    assert_eq!(ProjectSettings::load(&path).unwrap(), settings);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn registry_reports_code_defaults_and_categories() {
    let mut registry = TypeRegistry::default();
    registry.register::<Early>().register::<Normal>();
    registry.categorize(&["Early"], "Gameplay");
    assert_eq!(registry.default_order_of("Early"), Some(-10));
    assert_eq!(registry.default_order_of("Normal"), Some(0));
    assert_eq!(registry.default_order_of("Nope"), None);
    assert_eq!(registry.category_of("Early"), Some("Gameplay"));
    assert_eq!(registry.category_of("Normal"), None);
}

// ----- perf: порядок не возвращает работу «по всей сцене» -----

#[derive(Default)]
struct Counter(u64);

component! {
    pub struct Tick {}
}
impl Component for Tick {
    fn hooks(&self) -> Hooks {
        Hooks::UPDATE
    }
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        ctx.resource_or_default::<Counter>().0 += 1;
    }
}

component! {
    pub struct TickFirst {}
}
impl Component for TickFirst {
    fn hooks(&self) -> Hooks {
        Hooks::UPDATE
    }
    fn execution_order(&self) -> i32 {
        -1
    }
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        ctx.resource_or_default::<Counter>().0 += 1;
    }
}

component! {
    /// Данные без хуков: в фазах не участвует.
    pub struct Blob {}
}
impl Component for Blob {
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
    fn execution_order(&self) -> i32 {
        5
    }
}

#[test]
fn ordering_keeps_dispatch_proportional_to_components_with_the_hook() {
    let mut app = App::new();
    for i in 0..3000 {
        let id = app.world.scene.spawn(format!("o{i}"));
        let obj = app.world.scene.get_mut(id).unwrap();
        if i % 100 == 0 {
            obj.add(Tick::default());
        } else {
            obj.add(Blob::default());
        }
    }
    app.world.start_pending();
    app.world.update(0.02);
    let calls = app.world.resources.get::<Counter>().map_or(0, |c| c.0);
    assert_eq!(calls, 30, "only objects with the hook are visited, ordered scene or not");

    // Те же 30 вызовов при другом порядке: сортировка не умножает работу.
    let mut settings = ProjectSettings::default();
    settings.set_order("Tick", 7);
    app.world.apply_project_settings(settings);
    app.world.resources.get_mut::<Counter>().unwrap().0 = 0;
    app.world.update(0.02);
    assert_eq!(app.world.resources.get::<Counter>().unwrap().0, 30);

    // Быстрый путь: если ни у кого нет порядка, список объектов тот же, что раньше.
    let mut fast = App::new();
    for i in 0..3000 {
        let id = fast.world.scene.spawn(format!("o{i}"));
        let obj = fast.world.scene.get_mut(id).unwrap();
        if i % 100 == 0 {
            obj.add(Tick::default());
        }
    }
    fast.world.start_pending();
    fast.world.update(0.02);
    assert_eq!(fast.world.resources.get::<Counter>().unwrap().0, 30);

}

#[test]
fn ordered_frames_cost_about_the_same_as_plain_ones() {
    fn build(first: bool) -> App {
        let mut app = App::new();
        for i in 0..4000 {
            let id = app.world.scene.spawn(format!("o{i}"));
            let obj = app.world.scene.get_mut(id).unwrap();
            obj.add(Tick::default());
            if first && i % 2 == 0 {
                obj.add(TickFirst::default());
            }
        }
        app.world.start_pending();
        app
    }
    fn run(app: &mut App, frames: usize) -> std::time::Duration {
        app.world.update(0.02); // прогрев: кэши списков вызова построены
        let start = std::time::Instant::now();
        for _ in 0..frames {
            app.world.update(0.02);
        }
        start.elapsed()
    }
    let mut plain = build(false);
    let mut ordered = build(true);
    let t_plain = run(&mut plain, 200);
    let t_ordered = run(&mut ordered, 200);
    // В ordered на 50% компонентов больше (6000 против 4000 вызовов за кадр) —
    // даже с этой поправкой повторная сортировка каждый кадр дала бы кратный разрыв.
    assert!(
        t_ordered < t_plain * 4 + std::time::Duration::from_millis(50),
        "plain {t_plain:?}, ordered {t_ordered:?}"
    );
}
