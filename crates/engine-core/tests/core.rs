//! Интеграционные тесты ядра: только публичный API, без окна.

use engine_core::prelude::*;
use engine_core::serialize::MemoryPrefabs;

/// Журнал вызовов хуков — глобальный ресурс мира.
#[derive(Default)]
struct Log(Vec<String>);

component! {
    /// Пишет в журнал каждый свой хук.
    pub struct Probe {
        pub label: String = String::from("probe"),
        pub updates: i32 = 0,
        pub fixed_updates: i32 = 0,
    }
}

impl Probe {
    fn log(&self, ctx: &mut Ctx, what: &str) {
        let line = format!("{}:{what}", self.label);
        ctx.resource_mut::<Log>().unwrap().0.push(line);
    }
}

impl Component for Probe {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.log(ctx, "start");
    }
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        self.updates += 1;
        self.log(ctx, "update");
    }
    fn on_fixed_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        self.fixed_updates += 1;
    }
    fn on_destroy(&mut self, ctx: &mut Ctx) {
        self.log(ctx, "destroy");
    }
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        self.log(ctx, &format!("message {}={:?}", message.name, message.value));
    }
}

component! {
    pub struct Health {
        pub hp: i32 = 100,
        pub regen: f32 = 0.5,
        pub tint: Color = Color::WHITE,
    }
}
impl Component for Health {}

reflect_enum! {
    pub enum Team { Red, Blue }
}

component! {
    pub struct Unit {
        pub team: Team = Team::Red,
        pub offset: Vec3 = Vec3::ZERO,
    }
}
impl Component for Unit {}

fn app() -> App {
    let mut app = App::new();
    app.register::<Probe>().register::<Health>().register::<Unit>();
    app.world.resources.insert(Log::default());
    app
}

fn log(app: &App) -> Vec<String> {
    app.world.resources.get::<Log>().unwrap().0.clone()
}

#[test]
fn child_follows_parent_transform() {
    let mut scene = Scene::new("t");
    let parent = scene.build("Parent").at(1.0, 0.0).id();
    let child = scene.build("Child").at(0.0, 2.0).child_of(parent).id();

    assert!(scene.world_position(child).abs_diff_eq(Vec3::new(1.0, 2.0, 0.0), 1e-5));

    // Поворот родителя на 90° вокруг Z уводит ребёнка влево.
    scene.get_mut(parent).unwrap().transform.rotate_z(std::f32::consts::FRAC_PI_2);
    assert!(scene.world_position(child).abs_diff_eq(Vec3::new(-1.0, 0.0, 0.0), 1e-5));

    assert_eq!(scene.get(parent).unwrap().children(), &[child]);
    assert_eq!(scene.get(child).unwrap().parent(), Some(parent));
}

#[test]
fn reparenting_into_own_child_is_a_clear_error() {
    let mut scene = Scene::new("t");
    let a = scene.build("A").id();
    let b = scene.build("B").child_of(a).id();
    let err = scene.set_parent(a, Some(b)).unwrap_err().to_string();
    assert!(err.contains("cycle"), "{err}");
    assert_eq!(scene.get(a).unwrap().parent(), None, "hierarchy must be unchanged");
}

#[test]
fn lifecycle_order_start_update_destroy() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();

    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    assert_eq!(log(&app), ["probe:start", "probe:update", "probe:update"]);

    // destroy отложен: объект жив до конца кадра и ещё получает update.
    app.world.scene.destroy(id);
    assert!(app.world.scene.contains(id));
    app.tick(1.0 / 60.0);
    assert!(!app.world.scene.contains(id));
    assert_eq!(log(&app).last().unwrap(), "probe:destroy");
    assert!(app.world.scene.is_empty());
}

#[test]
fn fixed_update_runs_at_fixed_rate_regardless_of_frame_time() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();
    // 10 кадров по 1/30 c при шаге 1/60 c → 20 фиксированных шагов.
    app.run_headless(10, 1.0 / 30.0);
    let probe = app.world.scene.get_component::<Probe>(id).unwrap();
    assert_eq!(probe.updates, 10);
    assert_eq!(probe.fixed_updates, 20);
}

#[test]
fn inactive_objects_do_not_run_and_start_late() {
    let mut app = app();
    let parent = app.spawn("Parent").inactive().id();
    app.spawn("Child").with(Probe::default()).child_of(parent);
    app.run_headless(3, 1.0 / 60.0);
    assert!(log(&app).is_empty(), "child of an inactive parent must not run");

    app.world.scene.get_mut(parent).unwrap().active = true;
    app.tick(1.0 / 60.0);
    assert_eq!(log(&app), ["probe:start", "probe:update"]);
}

#[test]
fn get_component_find_by_tag_and_send_message() {
    let mut app = app();
    let player = app
        .spawn("Player")
        .tag("Player")
        .with(Health::default())
        .with(Probe::default())
        .id();
    app.spawn("Enemy").tag("Enemy");

    assert_eq!(app.world.scene.find_by_tag("Player"), Some(player));
    assert_eq!(app.world.scene.find_all_by_tag("Enemy").len(), 1);
    assert_eq!(app.world.scene.find_with::<Health>(), [player]);
    assert_eq!(app.world.scene.find_by_name("Nobody"), None);

    app.world.scene.get_component_mut::<Health>(player).unwrap().hp -= 30;
    assert_eq!(app.world.scene.get_component::<Health>(player).unwrap().hp, 70);

    app.tick(1.0 / 60.0);
    app.world.send_message(player, "Damage", Value::Int(5));
    app.tick(1.0 / 60.0);
    assert!(log(&app).contains(&"probe:message Damage=Int(5)".to_string()));
}

#[test]
fn remove_component_takes_effect_immediately() {
    let mut app = app();
    let id = app.spawn("P").with(Health::default()).with(Probe::default()).id();
    app.tick(1.0 / 60.0);
    assert!(app.world.scene.get_mut(id).unwrap().remove::<Health>());
    assert!(!app.world.scene.get(id).unwrap().has::<Health>());
    app.tick(1.0 / 60.0);
    assert_eq!(app.world.scene.get(id).unwrap().components().count(), 1);
}

#[test]
fn events_are_visible_exactly_one_frame_later() {
    struct Scored(u32);
    let mut app = app();
    app.world.events.emit(Scored(7));
    assert_eq!(app.world.events.read::<Scored>().count(), 0);
    app.tick(1.0 / 60.0);
    assert_eq!(app.world.events.read::<Scored>().map(|e| e.0).collect::<Vec<_>>(), [7]);
    app.tick(1.0 / 60.0);
    assert_eq!(app.world.events.read::<Scored>().count(), 0);
}

#[test]
fn scene_roundtrips_through_ron() {
    let mut app = app();
    let root = app
        .spawn("Squad")
        .tag("Units")
        .layer(3)
        .at3(1.0, 2.0, 3.0)
        .with(Health { hp: 42, regen: 1.5, tint: Color::rgba(1.0, 0.5, 0.25, 1.0) })
        .id();
    app.spawn("Scout")
        .at(0.0, 1.0)
        .with(Unit { team: Team::Blue, offset: Vec3::new(0.0, 0.5, 0.0) })
        .child_of(root);
    app.world.scene.get_mut(root).unwrap().transform.set_euler_deg(Vec3::new(0.0, 0.0, 30.0));

    let text = app.world.scene_to_string();
    assert!(text.contains(r#"kind: "Health""#), "{text}");
    assert!(text.contains(r#""team": Enum("Blue")"#), "{text}");

    let mut loaded = self::app();
    loaded.world.load_scene_str(&text, "memory").unwrap();
    assert_eq!(loaded.world.scene_to_string(), text, "save → load → save must be stable");

    let squad = loaded.world.scene.find_by_name("Squad").unwrap();
    let scout = loaded.world.scene.find_by_name("Scout").unwrap();
    assert_eq!(loaded.world.scene.get(squad).unwrap().layer, 3);
    assert_eq!(loaded.world.scene.get_component::<Health>(squad).unwrap().hp, 42);
    assert_eq!(loaded.world.scene.get_component::<Unit>(scout).unwrap().team, Team::Blue);
    assert_eq!(loaded.world.scene.get(scout).unwrap().parent(), Some(squad));
}

fn coin_prefab() -> Prefab {
    Prefab::from_ron(
        r#"(
            name: "Coin",
            tag: "Pickup",
            components: [
                (kind: "Health", fields: { "hp": Int(1) }),
            ],
            children: [
                (name: "Glow", transform: (position: (0.0, 0.5, 0.0))),
            ],
        )"#,
        "coin.prefab",
    )
    .unwrap()
}

#[test]
fn prefab_instances_keep_link_and_overrides() {
    let mut app = app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("coin.prefab", coin_prefab()));

    let scene = r#"(
        name: "Level",
        objects: [
            (name: "Coin A", prefab: Some("coin.prefab"), transform: (position: (3.0, 1.0, 0.0))),
            (
                name: "Coin B",
                prefab: Some("coin.prefab"),
                overrides: [
                    (component: "Health", field: "hp", value: Int(5)),
                    (path: "Glow", component: "Transform", field: "position", value: Vec3(0.0, 2.0, 0.0)),
                ],
            ),
        ],
    )"#;
    app.world.load_scene_str(scene, "level.scene").unwrap();

    let a = app.world.scene.find_by_name("Coin A").unwrap();
    let b = app.world.scene.find_by_name("Coin B").unwrap();
    assert_eq!(app.world.scene.get(a).unwrap().tag, "Pickup");
    assert_eq!(app.world.scene.get(a).unwrap().prefab(), Some("coin.prefab"));
    assert_eq!(app.world.scene.get_component::<Health>(a).unwrap().hp, 1);
    assert_eq!(app.world.scene.get_component::<Health>(b).unwrap().hp, 5);
    let glow_b = app.world.scene.get(b).unwrap().children()[0];
    assert_eq!(app.world.scene.get(glow_b).unwrap().transform.position.y, 2.0);

    // Правка в рантайме превращается в переопределение при сохранении.
    app.world.scene.get_component_mut::<Health>(a).unwrap().regen = 9.0;
    let saved = app.world.scene_to_string();
    assert!(saved.contains(r#"prefab: Some("coin.prefab")"#), "{saved}");
    assert!(saved.contains(r#"field: "regen""#), "{saved}");
    assert!(!saved.contains("kind:"), "instances must not be inlined:\n{saved}");

    // Instantiate в рантайме.
    let c = app.world.instantiate("coin.prefab").unwrap();
    // Имя экземпляра — имя корня префаба (раньше бралось имя файла).
    assert_eq!(app.world.scene.get(c).unwrap().name, "Coin");
    assert_eq!(app.world.scene.get(c).unwrap().children().len(), 1);
}

#[test]
fn load_errors_explain_what_to_do() {
    let mut app = app();
    app.spawn("Keep me");

    let unknown = r#"(name: "S", objects: [(name: "X", components: [(kind: "Helth")])])"#;
    let err = app.world.load_scene_str(unknown, "s.scene").unwrap_err().to_string();
    assert!(err.contains("in object `X`"), "{err}");

    let err = app.world.registry.create("Helth").err().unwrap().to_string();
    assert!(err.contains("Did you mean `Health`?"), "{err}");
    assert!(err.contains("app.register::<Helth>()"), "{err}");

    let wrong_type = r#"(name: "S", objects: [(name: "X", components: [
        (kind: "Health", fields: { "hp": Str("many") }),
    ])])"#;
    let err = app.world.load_scene_str(wrong_type, "s.scene").unwrap_err();
    let text = format!("{err}: {}", std::error::Error::source(&err).unwrap());
    assert!(text.contains("field `Health.hp` expects Int, but got Str"), "{text}");

    let bad_syntax = app.world.load_scene_str("(name: ", "broken.scene").unwrap_err().to_string();
    assert!(bad_syntax.contains("cannot parse `broken.scene`"), "{bad_syntax}");

    // Неудачная загрузка не разрушает текущую сцену.
    assert!(app.world.scene.find_by_name("Keep me").is_some());
    assert_eq!(app.world.scene.len(), 1);
}

#[test]
fn input_axes_and_buttons_work_like_unity_input_manager() {
    let mut input = Input::default();
    assert_eq!(input.axis("Horizontal"), 0.0);
    input.set_key(Key::D, true);
    input.set_key(Key::Space, true);
    assert_eq!(input.axis("Horizontal"), 1.0);
    assert!(input.button_down("Jump") && input.button("Jump"));

    input.end_frame();
    assert!(!input.button_down("Jump") && input.button("Jump"));

    input.set_key(Key::Left, true); // D и ← одновременно гасят друг друга
    assert_eq!(input.axis("Horizontal"), 0.0);
}

// ----------------------------------------------------------------------
// То, на чём стоит редактор: метаданные полей, снимки, порядок в иерархии
// ----------------------------------------------------------------------

component! {
    pub struct Documented {
        /// Speed in metres per second.
        /// Negative values move backwards.
        pub speed: f32 = 1.0,
        pub plain: i32 = 0,
    }
}
impl Component for Documented {}

#[test]
fn reflection_exposes_docs_and_enum_variants_for_the_inspector() {
    let documented = Documented::default();
    let doc: Vec<&str> = documented.field_doc("speed").lines().map(str::trim).collect();
    assert_eq!(doc, ["Speed in metres per second.", "Negative values move backwards."]);
    assert_eq!(documented.field_doc("plain"), "");
    assert_eq!(documented.field_doc("missing"), "");

    let unit = Unit::default();
    assert_eq!(unit.field_meta("team").variants, ["Red", "Blue"]);
    assert!(unit.field_meta("offset").variants.is_empty());
    assert_eq!(unit.field_meta("team").asset_kind, "");
}

#[test]
fn prefab_instances_save_only_real_differences() {
    let mut app = app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("coin.prefab", coin_prefab()));
    let scene = r#"(
        name: "Level",
        objects: [(name: "Coin A", prefab: Some("coin.prefab"))],
    )"#;
    app.world.load_scene_str(scene, "level.scene").unwrap();

    // Префаб не перечисляет `regen` и `tint` (они по умолчанию) — это не переопределения.
    let saved = app.world.scene_to_string();
    assert!(!saved.contains("overrides"), "an untouched instance has no overrides:\n{saved}");

    // Компонент добавили, компонент префаба убрали — оба факта попадают в файл.
    let coin = app.world.scene.find_by_name("Coin A").unwrap();
    let object = app.world.scene.get_mut(coin).unwrap();
    object.add(Unit {
        team: Team::Blue,
        ..Unit::default()
    });
    assert!(object.remove::<Health>());
    app.world.flush_destroyed();

    let saved = app.world.scene_to_string();
    assert!(saved.contains(r#"prefab: Some("coin.prefab")"#), "{saved}");
    assert!(saved.contains(r#""Health""#) && saved.contains("removed"), "{saved}");
    assert!(saved.contains(r#"kind: "Unit""#), "{saved}");

    app.world.load_scene_str(&saved, "saved.scene").unwrap();
    let coin = app.world.scene.find_by_name("Coin A").unwrap();
    let object = app.world.scene.get(coin).unwrap();
    assert_eq!(object.prefab(), Some("coin.prefab"));
    assert!(!object.has::<Health>());
    assert_eq!(object.get::<Unit>().unwrap().team, Team::Blue);
    assert_eq!(object.children().len(), 1, "children still come from the prefab");
}

#[test]
fn snapshot_restores_the_scene_and_invalidates_old_ids() {
    let mut app = app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("coin.prefab", coin_prefab()));
    let hero = app
        .spawn("Hero")
        .at(1.0, 2.0)
        .with(Health::default())
        .id();
    let hat = app.spawn("Hat").at(0.0, 1.0).child_of(hero).id();
    let coin = app.world.instantiate("coin.prefab").unwrap();
    app.world.scene.get_mut(coin).unwrap().add(Unit::default());

    let snapshot = app.world.snapshot();

    // «Игра» всё портит: двигает, удаляет, меняет поля.
    app.world.scene.get_mut(hero).unwrap().transform.position.x = 50.0;
    app.world.scene.get_component_mut::<Health>(hero).unwrap().hp = 1;
    app.world.scene.destroy(hat);
    app.world.scene.destroy(coin);
    app.world.flush_destroyed();
    assert_eq!(app.world.scene.len(), 1);

    app.world.restore(&snapshot).unwrap();
    assert_eq!(app.world.scene.len(), 4);
    assert!(!app.world.scene.contains(hero), "ids from before the restore are stale");
    assert!(!app.world.scene.contains(coin));

    let restored_hero = app.world.scene.find_by_name("Hero").unwrap();
    let object = app.world.scene.get(restored_hero).unwrap();
    assert_eq!(object.transform.position, Vec3::new(1.0, 2.0, 0.0));
    assert_eq!(object.get::<Health>().unwrap().hp, 100);
    let hat = app.world.scene.find_by_name("Hat").unwrap();
    assert_eq!(app.world.scene.world_position(hat), Vec3::new(1.0, 3.0, 0.0));

    // Экземпляр префаба вернулся вместе с тем, что добавили поверх префаба.
    let coin = app.world.scene.find_by_name("Coin").unwrap();
    let object = app.world.scene.get(coin).unwrap();
    assert_eq!(object.prefab(), Some("coin.prefab"));
    assert!(object.has::<Unit>() && object.has::<Health>());

    // Снимок восстановленной сцены равен исходному: Undo не «плывёт».
    assert_eq!(app.world.snapshot(), snapshot);
    // И каждый новый restore снова даёт новые id.
    app.world.restore(&snapshot).unwrap();
    assert!(!app.world.scene.contains(restored_hero));
}

#[test]
fn duplicate_copies_the_subtree_next_to_the_original() {
    let mut app = app();
    let squad = app.spawn("Squad").id();
    let hero = app
        .spawn("Hero")
        .at(1.0, 2.0)
        .with(Health { hp: 7, ..Health::default() })
        .child_of(squad)
        .id();
    app.spawn("Hat").child_of(hero);

    let copy = app.world.duplicate(hero).unwrap();
    assert_ne!(copy, hero);
    let scene = &app.world.scene;
    let object = scene.get(copy).unwrap();
    assert_eq!(object.parent(), Some(squad));
    assert_eq!(object.get::<Health>().unwrap().hp, 7);
    assert_eq!(object.children().len(), 1);
    assert_eq!(scene.get(squad).unwrap().children(), [hero, copy]);
    // Копия независима от оригинала.
    app.world.scene.get_component_mut::<Health>(copy).unwrap().hp = 1;
    assert_eq!(app.world.scene.get_component::<Health>(hero).unwrap().hp, 7);
}

#[test]
fn sibling_order_and_world_preserving_reparent() {
    let mut scene = Scene::new("Test");
    let a = scene.spawn("A");
    let b = scene.spawn("B");
    let c = scene.spawn("C");
    scene.set_sibling_index(c, 0);
    assert_eq!(scene.roots(), [c, a, b]);
    scene.set_sibling_index(c, 99);
    assert_eq!(scene.roots(), [a, b, c]);

    // Родитель сдвинут, повёрнут и увеличен; ребёнок после смены родителя остаётся на месте.
    let parent = scene
        .build("Parent")
        .at(10.0, 0.0)
        .rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
        .scale(2.0, 2.0, 1.0)
        .id();
    let child = scene.build("Child").at(3.0, 4.0).id();
    scene.set_parent_keep_world(child, Some(parent)).unwrap();
    assert!(scene.world_position(child).abs_diff_eq(Vec3::new(3.0, 4.0, 0.0), 1e-4));
    assert_ne!(scene.get(child).unwrap().transform.position, Vec3::new(3.0, 4.0, 0.0));
    scene.set_parent_keep_world(child, None).unwrap();
    let local = scene.get(child).unwrap().transform.position;
    assert!(local.abs_diff_eq(Vec3::new(3.0, 4.0, 0.0), 1e-4));

    assert!(scene.set_parent_keep_world(parent, Some(parent)).is_err());
}

#[test]
fn the_log_keeps_recent_records_for_the_console() {
    engine_core::logging::init();
    let before = engine_core::logging::records().last().map_or(0, |r| r.seq + 1);
    log::warn!("fuel is low: {}%", 7);
    log::debug!("debug chatter is not recorded");

    let fresh = engine_core::logging::records_since(before);
    let record = fresh
        .iter()
        .find(|r| r.message == "fuel is low: 7%")
        .expect("the warning is in the buffer");
    assert_eq!(record.level, engine_core::logging::Level::Warn);
    assert_eq!(record.target, "core");
    assert!(fresh.iter().all(|r| !r.message.contains("debug chatter")));
}
