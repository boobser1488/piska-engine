//! Индекс сцены и диспетчер хуков: хуки зовутся только тем, у кого они есть, в
//! порядке иерархии; `find_with` и др. совпадают с полным обходом сцены после любых
//! изменений (создание, удаление, смена родителя, добавление и удаление компонентов).

use engine_core::prelude::*;

/// Журнал вызовов: «имя объекта:хук».
#[derive(Default)]
struct Calls(Vec<String>);

fn record(ctx: &mut Ctx, hook: &str) {
    let name = ctx.object().name.clone();
    ctx.resource_or_default::<Calls>().0.push(format!("{name}:{hook}"));
}

fn calls(app: &mut App) -> Vec<String> {
    std::mem::take(&mut app.world.resources.get_or_insert_with(Calls::default).0)
}

component! {
    /// Скрипт с `on_update` и `on_late_update` (ничего не объявляет — получает всё).
    pub struct Script {}
}
impl Component for Script {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        record(ctx, "update");
    }
    fn on_late_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        record(ctx, "late");
    }
}

component! {
    /// Объявляет только `on_update`; его `on_late_update` движок звать не должен.
    pub struct UpdateOnly {}
}
impl Component for UpdateOnly {
    fn hooks(&self) -> Hooks {
        Hooks::UPDATE
    }
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        record(ctx, "update");
    }
    fn on_late_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        panic!("on_late_update is not declared in hooks()");
    }
}

component! {
    /// «Чистые данные»: хуков нет.
    pub struct Data {
        pub value: i32 = 0,
    }
}
impl Component for Data {
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
    fn on_start(&mut self, _ctx: &mut Ctx) {
        panic!("a data component must not be started");
    }
    fn on_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        panic!("a data component must not be updated");
    }
    fn on_destroy(&mut self, _ctx: &mut Ctx) {
        panic!("a data component must not get on_destroy");
    }
    fn on_message(&mut self, _ctx: &mut Ctx, _message: &Message) {
        panic!("a data component must not get messages");
    }
}

component! {
    /// В `on_start` добавляет `Starter` объекту с именем `target`.
    pub struct Adder {
        pub target: String = String::new(),
    }
}
impl Component for Adder {
    fn on_start(&mut self, ctx: &mut Ctx) {
        record(ctx, "start");
        if let Some(id) = ctx.find_by_name(&self.target) {
            ctx.scene_mut().get_mut(id).unwrap().add(Starter {});
        }
    }
}

component! {
    pub struct Starter {}
}
impl Component for Starter {
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::DESTROY
    }
    fn on_start(&mut self, ctx: &mut Ctx) {
        record(ctx, "start");
    }
    fn on_destroy(&mut self, ctx: &mut Ctx) {
        record(ctx, "destroy");
    }
}

#[test]
fn hooks_debug_and_set_operations() {
    assert_eq!(format!("{:?}", Hooks::UPDATE | Hooks::START), "Hooks(START | UPDATE)");
    assert_eq!(format!("{:?}", Hooks::NONE), "Hooks(NONE)");
    assert_eq!(format!("{:?}", Hooks::default()), "Hooks(ALL)");
    assert!(Hooks::ALL.contains(Hooks::TRIGGER_ENTER));
    assert!(!Hooks::UPDATE.intersects(Hooks::LATE_UPDATE));
    assert!(Hooks::NONE.is_empty());
}

#[test]
fn undeclared_hooks_are_skipped_and_order_is_hierarchy_order() {
    let mut app = App::new();
    let a = app.spawn("A").with(Script {}).with(Data::default()).id();
    app.spawn("B").with(Data::default());
    let c = app.spawn("C").with(UpdateOnly {}).id();
    app.spawn("A1").with(Script {}).child_of(a);
    app.spawn("C1").with(Script {}).child_of(c);
    app.run_headless(1, 1.0 / 60.0);
    assert_eq!(
        calls(&mut app),
        ["A:update", "A1:update", "C:update", "C1:update", "A:late", "A1:late", "C1:late"]
    );
    // Смена порядка братьев и родителя — порядок вызовов следует за иерархией.
    app.world.scene.set_sibling_index(c, 0);
    let a1 = app.world.scene.find_by_name("A1").unwrap();
    app.world.scene.set_parent(a1, None).unwrap();
    app.run_headless(1, 1.0 / 60.0);
    assert_eq!(
        calls(&mut app),
        ["C:update", "C1:update", "A:update", "A1:update", "C1:late", "A:late", "A1:late"]
    );
    // Сообщения и уничтожение тоже не доходят до «данных».
    app.world.send_message(a, "Ping", Value::Nil);
    app.world.flush_messages();
    app.world.scene.destroy(a);
    app.run_headless(1, 1.0 / 60.0);
    assert!(app.world.scene.find_by_name("A").is_none());
}

#[test]
fn components_added_in_on_start_start_in_the_same_phase_if_later_in_hierarchy() {
    let mut app = App::new();
    app.spawn("Early");
    app.spawn("Adder").with(Adder { target: "Late".into() });
    app.spawn("Adder2").with(Adder { target: "Early".into() });
    app.spawn("Late");
    app.run_headless(1, 1.0 / 60.0);
    // Как при обходе всей сцены: «Late» ниже по иерархии — стартует в этой же фазе,
    // «Early» выше — в следующем кадре.
    assert_eq!(calls(&mut app), ["Adder:start", "Adder2:start", "Late:start"]);
    app.run_headless(1, 1.0 / 60.0);
    assert_eq!(calls(&mut app), ["Early:start"]);
    // `on_destroy` — по объявленным хукам.
    let early = app.world.scene.find_by_name("Early").unwrap();
    app.world.scene.destroy(early);
    app.run_headless(1, 1.0 / 60.0);
    assert_eq!(calls(&mut app), ["Early:destroy"]);
}

#[test]
fn inactive_objects_start_when_activated() {
    let mut app = App::new();
    let hidden = app.spawn("Hidden").inactive().with(Starter {}).id();
    app.run_headless(3, 1.0 / 60.0);
    assert!(calls(&mut app).is_empty());
    app.world.scene.set_active(hidden, true);
    app.run_headless(1, 1.0 / 60.0);
    assert_eq!(calls(&mut app), ["Hidden:start"]);
    app.run_headless(1, 1.0 / 60.0);
    assert!(calls(&mut app).is_empty(), "started exactly once");
}

/// Полный обход сцены — эталон для индекса.
fn brute_force<T: Component>(scene: &Scene) -> Vec<GameObjectId> {
    scene.ids().into_iter().filter(|id| scene.get(*id).unwrap().has::<T>()).collect()
}

fn check(scene: &Scene) {
    assert_eq!(scene.find_with::<Data>(), brute_force::<Data>(scene));
    assert_eq!(scene.find_with::<Script>(), brute_force::<Script>(scene));
    assert_eq!(scene.count_with::<Data>(), brute_force::<Data>(scene).len());
    let mut visited = Vec::new();
    scene.visit_active_with::<Script>(|id, _| visited.push(id));
    let active: Vec<_> = brute_force::<Script>(scene).into_iter().filter(|id| scene.is_active(*id)).collect();
    assert_eq!(visited, active);
    let mut shuffled: Vec<_> = scene.ids().into_iter().rev().collect();
    scene.sort_by_hierarchy(&mut shuffled);
    assert_eq!(shuffled, scene.ids());
}

#[test]
fn index_matches_a_full_scan_after_any_change() {
    // Детерминированная «случайная» последовательность операций.
    let mut state = 0x2545_f491_u64;
    let mut next = move |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    let mut app = App::new();
    let mut live: Vec<GameObjectId> = Vec::new();
    for step in 0..600 {
        let scene = &mut app.world.scene;
        match next(9) {
            0 | 1 => {
                let id = scene.spawn(format!("O{step}"));
                if next(2) == 0 {
                    scene.get_mut(id).unwrap().add(Data::default());
                }
                if next(3) == 0 {
                    scene.get_mut(id).unwrap().add(Script {});
                }
                live.push(id);
            }
            2 if !live.is_empty() => {
                // Ребёнок только что созданного объекта (быстрый путь порядка) или любого.
                let parent = if next(2) == 0 { *live.last().unwrap() } else { live[next(live.len())] };
                let id = scene.build(format!("C{step}")).with(Script {}).id();
                scene.set_parent(id, Some(parent)).unwrap();
                live.push(id);
            }
            3 if live.len() > 1 => {
                let (a, b) = (live[next(live.len())], live[next(live.len())]);
                let _ = scene.set_parent(a, Some(b)); // цикл — ошибка, сцена не меняется
            }
            4 if !live.is_empty() => {
                let id = live[next(live.len())];
                scene.set_sibling_index(id, next(4));
            }
            5 if !live.is_empty() => {
                let id = live[next(live.len())];
                let object = scene.get_mut(id).unwrap();
                if !object.remove::<Data>() {
                    object.add(Data::default());
                }
            }
            6 if !live.is_empty() => {
                let id = live[next(live.len())];
                scene.set_active(id, next(2) == 0);
            }
            7 if !live.is_empty() => {
                let id = live[next(live.len())];
                scene.destroy(id);
                app.world.end_frame();
                live.retain(|id| app.world.scene.contains(*id));
            }
            _ => {
                app.tick(1.0 / 60.0);
                calls(&mut app);
            }
        }
        check(&app.world.scene);
    }
    assert!(!live.is_empty());
}

#[test]
fn update_order_matches_a_full_scan_after_reparenting() {
    let mut app = App::new();
    let mut ids = Vec::new();
    for i in 0..30 {
        let id = app.spawn(format!("N{i}")).with(Script {}).id();
        if i % 3 == 1 {
            app.world.scene.set_parent(id, Some(ids[i - 1])).unwrap();
        }
        ids.push(id);
    }
    app.world.scene.set_parent(ids[4], Some(ids[20])).unwrap();
    app.world.scene.set_sibling_index(ids[9], 0);
    app.run_headless(1, 1.0 / 60.0);
    let expected: Vec<String> = app
        .world
        .scene
        .ids()
        .into_iter()
        .map(|id| format!("{}:update", app.world.scene.get(id).unwrap().name))
        .collect();
    let got: Vec<String> = calls(&mut app).into_iter().filter(|c| c.ends_with(":update")).collect();
    assert_eq!(got, expected);
}

#[test]
fn removed_component_leaves_the_index_at_once_and_scene_replacement_resets_it() {
    let mut app = App::new();
    let id = app.spawn("A").with(Data { value: 1 }).id();
    assert_eq!(app.world.scene.count_with::<Data>(), 1);
    app.world.scene.get_mut(id).unwrap().remove::<Data>();
    assert_eq!(app.world.scene.count_with::<Data>(), 0);
    assert!(app.world.scene.find_with::<Data>().is_empty());
    app.world.scene.get_mut(id).unwrap().add(Data { value: 2 });
    let snapshot = app.world.snapshot();
    let before = app.world.scene.instance_id();
    app.world.registry.register::<Data>();
    app.world.restore(&snapshot).unwrap();
    assert_ne!(app.world.scene.instance_id(), before, "a restored scene is a new scene");
    let found = app.world.scene.find_with::<Data>();
    assert_eq!(found.len(), 1);
    assert_eq!(app.world.scene.get_component::<Data>(found[0]).unwrap().value, 2);
}
