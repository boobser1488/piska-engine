//! Короткие формы скриптов: каждая ведёт себя как длинная, обучающие ошибки называют
//! объект, недостающую вещь и подсказку — и печатаются один раз.

use engine_core::prelude::*;

const DT: f32 = 0.1;

fn errors(app: &App) -> Vec<String> {
    app.world
        .resources
        .get::<ScriptErrors>()
        .map(|e| e.messages().to_vec())
        .unwrap_or_default()
}

/// Выполнить `f` как «хук» объекта `id`.
fn with_ctx<R>(app: &mut App, id: GameObjectId, f: impl FnOnce(&mut Ctx) -> R) -> R {
    f(&mut Ctx { world: &mut app.world, id })
}

// ----------------------------------------------------------------------
// Короткие имена хуков
// ----------------------------------------------------------------------

component! {
    /// Пишет, какие короткие хуки вызваны и какой `dt` видел `ctx.dt()`.
    pub struct Short {
        pub log: Vec<String> = Vec::new(),
    }
}
impl Component for Short {
    fn start(&mut self, ctx: &mut Ctx) {
        self.log.push(format!("start {}", ctx.dt()));
    }
    fn update(&mut self, ctx: &mut Ctx) {
        self.log.push(format!("update {}", ctx.dt()));
    }
    fn fixed_update(&mut self, ctx: &mut Ctx) {
        self.log.push(format!("fixed {}", ctx.dt()));
    }
    fn late_update(&mut self, ctx: &mut Ctx) {
        self.log.push(format!("late {}", ctx.dt()));
    }
    fn on_collision(&mut self, _ctx: &mut Ctx, _other: GameObjectId) {
        self.log.push("collision".into());
    }
    fn on_trigger(&mut self, _ctx: &mut Ctx, _other: GameObjectId) {
        self.log.push("trigger".into());
    }
    fn on_click(&mut self, _ctx: &mut Ctx) {
        self.log.push("click".into());
    }
}

component! {
    /// То же длинными хуками: `dt` из параметра.
    pub struct Long {
        pub log: Vec<String> = Vec::new(),
    }
}
impl Component for Long {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.log.push(format!("start {}", ctx.time().delta));
    }
    fn on_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.log.push(format!("update {dt}"));
    }
    fn on_fixed_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.log.push(format!("fixed {dt}"));
    }
    fn on_late_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.log.push(format!("late {dt}"));
    }
    fn on_collision_enter(&mut self, _ctx: &mut Ctx, _other: GameObjectId) {
        self.log.push("collision".into());
    }
    fn on_trigger_enter(&mut self, _ctx: &mut Ctx, _other: GameObjectId) {
        self.log.push("trigger".into());
    }
}

component! {
    /// Переопределены и длинный, и короткий хук: работает длинный, как в старом коде.
    pub struct Both {
        pub long: u32 = 0,
        pub short: u32 = 0,
    }
}
impl Component for Both {
    fn on_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        self.long += 1;
    }
    fn update(&mut self, _ctx: &mut Ctx) {
        self.short += 1;
    }
}

#[test]
fn short_hooks_are_called_exactly_like_the_long_ones() {
    let mut app = App::new();
    app.world.time.fixed_delta = 0.05;
    let short = app.spawn("Short").with(Short::default()).id();
    let long = app.spawn("Long").with(Long::default()).id();
    app.run_headless(2, DT);
    for id in [short, long] {
        app.world.dispatch_hook(id, Hooks::COLLISION_ENTER, |c, ctx| c.on_collision_enter(ctx, id));
        app.world.dispatch_hook(id, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, id));
    }
    let short_log = &app.world.scene.get_component::<Short>(short).unwrap().log;
    let long_log = &app.world.scene.get_component::<Long>(long).unwrap().log;
    assert_eq!(short_log, long_log, "same order, same dt in every phase");
    assert!(short_log.contains(&"fixed 0.05".to_string()), "ctx.dt() is the physics step in fixed_update: {short_log:?}");
    assert!(short_log.contains(&format!("update {DT}")));
    assert!(short_log.contains(&format!("late {DT}")));
}

#[test]
fn a_long_hook_wins_over_a_short_one_on_the_same_type() {
    let mut app = App::new();
    let id = app.spawn("Both").with(Both::default()).id();
    app.run_headless(3, DT);
    let both = app.world.scene.get_component::<Both>(id).unwrap();
    assert_eq!((both.long, both.short), (3, 0));
}

#[test]
fn on_click_has_its_own_hook_bit() {
    let mut app = App::new();
    let id = app.spawn("Button").with(Short::default()).id();
    app.tick(DT);
    app.world.dispatch_hook(id, Hooks::CLICK, |c, ctx| c.on_click(ctx));
    assert_eq!(app.world.scene.get_component::<Short>(id).unwrap().log.last().unwrap(), "click");
    assert_eq!(format!("{:?}", Hooks::CLICK | Hooks::UPDATE), "Hooks(UPDATE | CLICK)");
}

// ----------------------------------------------------------------------
// Transform
// ----------------------------------------------------------------------

#[test]
fn transform_short_forms_match_the_transform_component() {
    let mut app = App::new();
    let parent = app.spawn("Parent").at(10.0, 0.0).id();
    let id = app.spawn("Child").at(1.0, 2.0).child_of(parent).id();
    with_ctx(&mut app, id, |ctx| {
        assert_eq!(ctx.pos(), ctx.world_position());
        assert_eq!(ctx.pos2(), Vec2::new(11.0, 2.0));
        // set_pos — мировая точка, z сохраняется у Vec2.
        ctx.set_pos(Vec2::new(12.0, 5.0));
        assert_eq!(ctx.transform().position, Vec3::new(2.0, 5.0, 0.0));
        ctx.move_by(Vec2::new(1.0, -1.0));
        assert_eq!(ctx.pos(), Vec3::new(13.0, 4.0, 0.0));
        ctx.set_angle(90.0);
        assert!((ctx.angle() - 90.0).abs() < 1e-3);
        assert!((ctx.transform().angle_z() - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        ctx.rotate_by(-45.0);
        assert!((ctx.angle() - 45.0).abs() < 1e-3);
        ctx.set_scale(Vec2::new(2.0, 3.0));
        assert_eq!(ctx.scale(), Vec3::new(2.0, 3.0, 1.0));
    });
}

#[test]
fn look_at_turns_forward_towards_the_target() {
    let mut app = App::new();
    let id = app.spawn("Eye").at3(0.0, 0.0, 0.0).id();
    with_ctx(&mut app, id, |ctx| {
        ctx.look_at(Vec3::new(5.0, 0.0, 0.0));
        assert!((ctx.forward() - Vec3::X).length() < 1e-4, "{:?}", ctx.forward());
        assert!((ctx.up() - Vec3::Y).length() < 1e-4);
        let long = ctx.transform().forward();
        assert!((long - ctx.forward()).length() < 1e-5, "same as Transform::forward for a root object");
        ctx.rotate_by(Vec3::new(0.0, 90.0, 0.0));
        assert!((ctx.forward() - Vec3::NEG_Z).length() < 1e-4, "{:?}", ctx.forward());
    });
}

#[test]
fn move_to_is_a_position_tween() {
    let mut app = App::new();
    let id = app.spawn("Box").id();
    with_ctx(&mut app, id, |ctx| ctx.move_to(Vec2::new(4.0, 0.0), 1.0));
    app.run_headless(12, DT);
    assert!((app.world.scene.world_position(id).x - 4.0).abs() < 1e-4);
}

// ----------------------------------------------------------------------
// Ввод
// ----------------------------------------------------------------------

#[test]
fn input_short_forms_read_the_same_state() {
    let mut app = App::new();
    let id = app.spawn("Player").id();
    app.world.input.set_key(Key::Space, true);
    app.world.input.set_key(Key::D, true);
    app.world.input.set_key(Key::W, true);
    with_ctx(&mut app, id, |ctx| {
        assert!(ctx.key(Key::Space) && ctx.key_pressed(Key::Space) && !ctx.key_released(Key::Space));
        assert!(ctx.pressed("Jump") && ctx.held("Jump") && !ctx.released("Jump"));
        assert_eq!(ctx.axis("Horizontal"), ctx.input().axis("Horizontal"));
        let v = ctx.axis2("Horizontal", "Vertical");
        assert!((v.length() - 1.0).abs() < 1e-5, "diagonal is normalized: {v:?}");
    });
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

component! {
    /// Читает действие с опечаткой каждый кадр.
    pub struct Typo {}
}
impl Component for Typo {
    fn update(&mut self, ctx: &mut Ctx) {
        let _ = ctx.axis("Horizntal");
        let _ = ctx.pressed("Horizontal");
    }
}

#[test]
fn an_unknown_action_teaches_once_with_a_suggestion() {
    let mut app = App::new();
    app.spawn("Hero").with(Typo::default());
    app.run_headless(30, DT);
    let errors = errors(&app);
    assert_eq!(errors.len(), 2, "one per call site, not per frame: {errors:#?}");
    assert!(errors[0].starts_with("Hero:"), "{}", errors[0]);
    assert!(errors[0].contains("`Horizntal`") && errors[0].contains("Did you mean `Horizontal`?"), "{}", errors[0]);
    assert!(errors[0].contains("script_api.rs"), "the site is named: {}", errors[0]);
    assert!(errors[1].contains("is an axis") && errors[1].contains("ctx.axis"), "{}", errors[1]);
}

// ----------------------------------------------------------------------
// Компоненты: me / me_mut / with
// ----------------------------------------------------------------------

component! {
    pub struct Health { pub hp: i32 = 3 }
}
impl Component for Health {}

component! {
    pub struct Health2 { pub hp: i32 = 1 }
}
impl Component for Health2 {}

component! {
    /// Ищет `Healht`-подобную ошибку: компонента `Health2` на объекте нет.
    pub struct NeedsHealth2 { pub reads: i32 = 0 }
}
impl Component for NeedsHealth2 {
    fn update(&mut self, ctx: &mut Ctx) {
        ctx.me_mut::<Health2>().hp -= 1; // пишет в заглушку
        self.reads += ctx.me::<Health2>().hp; // заглушка свежая: 1
    }
}

component! {
    /// Ошибка новичка: просит у ctx самого себя.
    pub struct AsksForItself { pub n: i32 = 0 }
}
impl Component for AsksForItself {
    fn update(&mut self, ctx: &mut Ctx) {
        ctx.me_mut::<AsksForItself>().n += 1;
    }
}

#[test]
fn me_mut_is_get_mut_without_option() {
    let mut app = App::new();
    let id = app.spawn("Hero").with(Health::default()).id();
    with_ctx(&mut app, id, |ctx| {
        ctx.me_mut::<Health>().hp -= 1;
        assert_eq!(ctx.get::<Health>().unwrap().hp, 2);
        assert_eq!(ctx.me::<Health>().hp, 2);
        assert_eq!(ctx.with::<Health, _>(|h| h.hp * 10), Some(20));
        assert!(ctx.has::<Health>() && !ctx.has::<Health2>());
    });
    assert!(errors(&app).is_empty());
}

#[test]
fn a_missing_component_teaches_once_and_changes_nothing() {
    let mut app = App::new();
    app.register::<Health>().register::<Health2>();
    let id = app
        .spawn("Hero")
        .with(Health::default())
        .with(NeedsHealth2::default())
        .id();
    app.run_headless(20, DT);
    let errors = errors(&app);
    // Два места вызова (`me_mut` и `me`) — два сообщения за 20 кадров, а не сорок.
    assert_eq!(errors.len(), 2, "logged once per call site, not every frame: {errors:#?}");
    assert_ne!(errors[0], errors[1], "each names its own line");
    let e = &errors[0];
    assert!(e.contains("`Hero` has no `Health2` component"), "{e}");
    assert!(e.contains("It has: Transform, Health"), "lists what is there: {e}");
    assert!(e.contains("Did you mean `Health`?"), "{e}");
    assert!(e.contains("Add Component → Health2"), "{e}");
    assert!(e.contains("script_api.rs"), "{e}");
    let script = app.world.scene.get_component::<NeedsHealth2>(id).unwrap();
    assert_eq!(script.reads, 20, "the stand-in is reset to its default every call");
    assert_eq!(app.world.scene.get_component::<Health>(id).unwrap().hp, 3, "the real Health is untouched");
}

#[test]
fn asking_ctx_for_the_running_script_points_to_self() {
    let mut app = App::new();
    app.spawn("Hero").with(AsksForItself::default());
    app.run_headless(3, DT);
    let errors = errors(&app);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("use `self` instead"), "{}", errors[0]);
}

// ----------------------------------------------------------------------
// Другие объекты, сообщения, жизнь объектов
// ----------------------------------------------------------------------

component! {
    /// Принимает сообщения.
    pub struct Inbox { pub got: Vec<String> = Vec::new(), pub damage: i32 = 0 }
}
impl Component for Inbox {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        if message.is("Damage") {
            self.damage += message.get::<i32>().unwrap_or(0);
        }
        self.got.push(message.name.clone());
    }
}

#[test]
fn find_send_and_on_work_like_the_long_forms() {
    let mut app = App::new();
    let me = app.spawn("Player").tag("Player").id();
    let game = app.spawn("Game").with(Inbox::default()).id();
    app.tick(DT); // сообщения получают стартовавшие компоненты
    with_ctx(&mut app, me, |ctx| {
        let found = ctx.find("Game");
        assert_eq!(Some(found), ctx.find_by_name("Game"));
        assert_eq!(ctx.find_tag("Player"), me);
        assert_eq!(ctx.first_with::<Inbox>(), game);
        assert_eq!(ctx.all_with::<Inbox>(), ctx.find_with::<Inbox>());
        ctx.send(found, "Caught");
        ctx.send_value(found, "Damage", 2);
        assert!(ctx.has_tag(me, "Player") && !ctx.has_tag(found, "Player"));
        ctx.on(found).set_pos(Vec2::new(3.0, 4.0));
        ctx.on(found).me_mut::<Inbox>().got.push("direct".into());
        assert_eq!(ctx.name_of(found), "Game");
    });
    app.world.flush_messages();
    let inbox = app.world.scene.get_component::<Inbox>(game).unwrap();
    assert_eq!(inbox.got, ["direct", "Caught", "Damage"]);
    assert_eq!(inbox.damage, 2);
    assert_eq!(app.world.scene.world_position(game), Vec3::new(3.0, 4.0, 0.0));
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn a_wrong_name_teaches_and_none_is_harmless() {
    let mut app = App::new();
    app.spawn("GameManager");
    let me = app.spawn("Coin").id();
    with_ctx(&mut app, me, |ctx| {
        for _ in 0..5 {
            let game = ctx.find("GameManger");
            assert_eq!(game, GameObjectId::NONE);
            // Всё с NONE ничего не делает и не паникует.
            ctx.send(game, "Caught");
            ctx.on(game).set_pos(Vec2::ONE);
            ctx.on(game).move_by(Vec2::ONE);
            ctx.on(game).destroy_self();
            assert!(!ctx.exists(game));
            assert_eq!(ctx.name_of(game), "<none>");
        }
    });
    let errors = errors(&app);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(errors[0].starts_with("Coin: ctx.find(\"GameManger\")"), "{}", errors[0]);
    assert!(errors[0].contains("Did you mean `GameManager`?"), "{}", errors[0]);
}

#[test]
fn spawn_returns_the_new_object_or_teaches() {
    let mut app = App::new();
    app.world.prefabs = Box::new(engine_core::serialize::MemoryPrefabs::default());
    let me = app.spawn("Spawner").at(2.0, 1.0).id();
    let id = with_ctx(&mut app, me, |ctx| ctx.spawn("prefabs/missing.prefab"));
    assert_eq!(id, GameObjectId::NONE);
    let errors = errors(&app);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("ctx.spawn(\"prefabs/missing.prefab\") failed"), "{}", errors[0]);
}

#[test]
fn set_active_self_and_obj_refs() {
    let mut app = App::new();
    let target = app.spawn("Door").id();
    let me = app.spawn("Switch").id();
    with_ctx(&mut app, me, |ctx| {
        let field = ctx.object_ref(target);
        assert_eq!(ctx.obj(field), target);
        ctx.on(target).set_active_self(false);
        assert!(!ctx.is_active(target));
        assert_eq!(ctx.obj(ObjectRef::NONE), GameObjectId::NONE);
    });
    let errors = errors(&app);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("not assigned") && errors[0].contains("Inspector"), "{}", errors[0]);
}

// ----------------------------------------------------------------------
// Таймеры, сохранения, случайность
// ----------------------------------------------------------------------

component! {
    pub struct Counter { pub n: i32 = 0 }
}
impl Component for Counter {}

#[test]
fn delay_and_repeat_run_closures_on_the_object() {
    let mut app = App::new();
    let id = app.spawn("Clock").with(Counter::default()).id();
    with_ctx(&mut app, id, |ctx| {
        ctx.repeat(0.25, |ctx| ctx.me_mut::<Counter>().n += 1);
        ctx.delay(0.55, |ctx| ctx.set_pos(Vec2::new(9.0, 0.0)));
    });
    app.run_headless(10, DT); // 1 секунда
    assert_eq!(app.world.scene.get_component::<Counter>(id).unwrap().n, 4);
    assert_eq!(app.world.scene.world_position(id).x, 9.0);
    // Таймеры умирают вместе с объектом.
    app.world.scene.destroy(id);
    app.run_headless(10, DT);
    assert!(errors(&app).is_empty());
}

#[test]
fn save_and_load_round_trip_through_save_data() {
    let mut app = App::new();
    app.world.resources.insert(SaveData::in_memory());
    let id = app.spawn("Game").id();
    with_ctx(&mut app, id, |ctx| {
        assert_eq!(ctx.load("best", 0), 0);
        ctx.save("best", 42);
        ctx.save("name", String::from("Ann"));
        assert_eq!(ctx.load("best", 0), 42);
        assert_eq!(ctx.save_data().int("best", 0), 42, "the same store as the long form");
        assert_eq!(ctx.load("name", String::new()), "Ann");
        assert!(!ctx.save_data().is_dirty(), "ctx.save writes to the backend at once");
    });
}

#[test]
fn random_short_forms_stay_in_range() {
    let mut app = App::new();
    let id = app.spawn("Dice").id();
    with_ctx(&mut app, id, |ctx| {
        let mut seen = [false; 6];
        for _ in 0..500 {
            let r = ctx.rand();
            assert!((0.0..1.0).contains(&r));
            let x = ctx.rand_range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&x));
            seen[(ctx.rand_int(1, 6) - 1) as usize] = true;
        }
        assert!(seen.iter().all(|s| *s), "1..=6 inclusive: {seen:?}");
    });
}

// ----------------------------------------------------------------------
// Стоимость вызова хука
// ----------------------------------------------------------------------

component! {
    pub struct LongForm { pub sum: f32 = 0.0 }
}
impl Component for LongForm {
    fn on_update(&mut self, _ctx: &mut Ctx, dt: f32) {
        self.sum += dt;
    }
}

component! {
    pub struct ShortForm { pub sum: f32 = 0.0 }
}
impl Component for ShortForm {
    fn update(&mut self, ctx: &mut Ctx) {
        self.sum += ctx.dt();
    }
}

fn ns_per_hook<T: Component + Default>() -> f64 {
    const OBJECTS: usize = 4000;
    const FRAMES: u32 = 200;
    let mut app = App::new();
    for i in 0..OBJECTS {
        let id = app.world.scene.spawn(format!("o{i}"));
        app.world.scene.get_mut(id).unwrap().add(T::default());
    }
    app.world.start_pending();
    app.world.update(0.02);
    let mut best = f64::MAX;
    for _ in 0..3 {
        let start = std::time::Instant::now();
        for _ in 0..FRAMES {
            app.world.update(0.02);
        }
        best = best.min(start.elapsed().as_nanos() as f64 / (FRAMES as f64 * OBJECTS as f64));
    }
    best
}

#[test]
fn short_hooks_cost_the_same_as_long_ones() {
    let long = ns_per_hook::<LongForm>();
    let short = ns_per_hook::<ShortForm>();
    println!("dispatch: on_update {long:.1} ns, update {short:.1} ns per call");
    assert!(short < long * 1.5 + 20.0, "short {short:.1} ns vs long {long:.1} ns");
}

// ----------------------------------------------------------------------
// Шпаргалка и документация не расходятся
// ----------------------------------------------------------------------

#[test]
fn the_cheat_sheet_in_the_docs_is_generated_from_the_same_table() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/gameplay.md");
    let Ok(doc) = std::fs::read_to_string(&path) else {
        eprintln!("no docs/api/gameplay.md next to the crate: skipped");
        return;
    };
    let doc = doc.replace("\r\n", "\n");
    let table = engine_core::script::cheat_sheet_markdown();
    assert!(
        doc.contains(&table),
        "docs/api/gameplay.md has a stale cheat sheet; replace the table between the cheat-sheet markers with:\n{table}"
    );
    for entry in engine_core::script::CHEAT_SHEET {
        assert!(!entry.short.is_empty() && !entry.example.is_empty() && !entry.wraps.is_empty(), "{entry:?}");
    }
}
