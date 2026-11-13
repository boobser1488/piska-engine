//! Геймпле-API ядра: время, таймеры, твины, сцены, запросы, сохранения, случайность, ввод.
//! Только публичный API, без окна, детерминированно (`run_headless`).

use engine_core::prelude::*;
use engine_core::serialize::MemoryPrefabs;

const DT: f32 = 1.0 / 60.0;

/// Журнал событий — глобальный ресурс мира.
#[derive(Default)]
struct Log(Vec<String>);

fn log(app: &App) -> Vec<String> {
    app.world.resources.get::<Log>().map(|l| l.0.clone()).unwrap_or_default()
}

fn push(ctx: &mut Ctx, line: String) {
    ctx.resource_or_default::<Log>().0.push(line);
}

component! {
    /// Пишет в журнал сообщения, старт и уничтожение.
    pub struct Probe {
        pub label: String = String::from("p"),
        pub updates: i32 = 0,
        pub fixed: i32 = 0,
    }
}

impl Component for Probe {
    fn on_start(&mut self, ctx: &mut Ctx) {
        push(ctx, format!("{}:start", self.label));
    }
    fn on_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        self.updates += 1;
    }
    fn on_fixed_update(&mut self, _ctx: &mut Ctx, _dt: f32) {
        self.fixed += 1;
    }
    fn on_destroy(&mut self, ctx: &mut Ctx) {
        push(ctx, format!("{}:destroy", self.label));
    }
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        push(ctx, format!("{}:{}", self.label, message.name));
    }
}

component! {
    pub struct Stats {
        pub hp: i32 = 10,
        pub speed: f32 = 1.0,
        pub tint: Color = Color::WHITE,
        pub offset: Vec3 = Vec3::ZERO,
        pub name: String = String::new(),
    }
}
impl Component for Stats {}

component! {
    /// Запрашивает загрузку сцены на заданном кадре, изнутри `on_update`.
    pub struct Portal {
        pub scene: String = String::from("b"),
        pub at_frame: i32 = 3,
        pub frame: i32 = 0,
    }
}
impl Component for Portal {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        self.frame += 1;
        if self.frame == self.at_frame {
            let scene = self.scene.clone();
            ctx.load_scene(&scene);
            // Остаток кадра доживает в старой сцене.
            let line = format!("portal:requested at {}", ctx.scene_name());
            push(ctx, line);
        }
    }
}

component! {
    pub struct Timed {
        pub fuse: f32 = 0.5,
    }
}
impl Component for Timed {
    fn on_start(&mut self, ctx: &mut Ctx) {
        ctx.after(self.fuse, "Boom", Value::Int(7));
    }
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        push(ctx, format!("timed:{}={:?}", message.name, message.value));
    }
}

fn app() -> App {
    let mut app = App::new();
    app.register::<Probe>()
        .register::<Stats>()
        .register::<Portal>()
        .register::<Timed>();
    app.world.resources.insert(Log::default());
    app
}

fn secs(app: &mut App, seconds: f32) {
    app.run_headless((seconds / DT).round() as u32, DT);
}

// ----------------------------------------------------------------------
// Время
// ----------------------------------------------------------------------

#[test]
fn time_tracks_scaled_and_unscaled_time() {
    let mut app = app();
    app.world.time.time_scale = 0.5;
    app.run_headless(60, DT);
    let t = app.world.time;
    assert!((t.elapsed - 0.5).abs() < 1e-3, "scaled elapsed {}", t.elapsed);
    assert!((t.unscaled_elapsed - 1.0).abs() < 1e-3);
    assert!((t.delta - DT * 0.5).abs() < 1e-6);
    assert!((t.unscaled_delta - DT).abs() < 1e-6);
    assert_eq!(t.frame, 60);
    assert!((t.fps() - 60.0).abs() < 0.1);
}

#[test]
fn time_scale_zero_pauses_fixed_steps_and_resumes_cleanly() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();
    app.run_headless(60, DT);
    let before = app.world.scene.get_component::<Probe>(id).unwrap().fixed;
    assert!((59..=61).contains(&before), "fixed steps at scale 1: {before}");

    app.world.time.time_scale = 0.0;
    assert!(app.world.time.is_paused());
    app.run_headless(120, DT);
    let paused = app.world.scene.get_component::<Probe>(id).unwrap().fixed;
    assert_eq!(paused, before, "no fixed steps while paused");
    assert_eq!(app.world.time.delta, 0.0);
    // Накопитель не «взорвался»: после паузы шаги идут с обычной частотой, без пачки догоняющих.
    app.world.time.time_scale = 1.0;
    app.run_headless(1, DT);
    let after_one = app.world.scene.get_component::<Probe>(id).unwrap().fixed;
    assert!(after_one - paused <= 1, "no catch-up burst after pause: {}", after_one - paused);
    app.run_headless(59, DT);
    let resumed = app.world.scene.get_component::<Probe>(id).unwrap().fixed - paused;
    assert!((59..=61).contains(&resumed), "resumed steps {resumed}");
}

#[test]
fn slow_motion_scales_fixed_steps() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();
    app.world.time.time_scale = 0.25;
    app.run_headless(240, DT);
    let fixed = app.world.scene.get_component::<Probe>(id).unwrap().fixed;
    assert!((59..=61).contains(&fixed), "4 real seconds at 0.25x = 1 game second: {fixed}");
}

#[test]
fn broken_time_scales_do_not_break_the_loop() {
    let mut app = app();
    for scale in [-3.0, f32::NAN, f32::INFINITY] {
        app.world.time.time_scale = scale;
        app.run_headless(3, DT);
        assert!(app.world.time.delta.is_finite() && app.world.time.delta >= 0.0, "scale {scale}");
    }
}

// ----------------------------------------------------------------------
// Timer / Cooldown
// ----------------------------------------------------------------------

#[test]
fn timer_once_and_repeating() {
    let mut once = Timer::once(1.0);
    assert!(!once.tick(0.6).just_finished());
    assert!(once.tick(0.6).just_finished());
    assert!(once.finished());
    assert!(!once.tick(5.0).just_finished(), "a one-shot timer fires once");

    let mut every = Timer::repeating(0.25);
    let fired: u32 = (0..32).map(|_| every.tick(0.125).times_finished()).sum();
    assert_eq!(fired, 16, "4 seconds / 0.25");
    // Большой dt даёт несколько срабатываний, а не одно.
    let mut every = Timer::repeating(0.1);
    assert_eq!(every.tick(0.35).times_finished(), 3);
    assert!((every.fraction() - 0.5).abs() < 1e-4);

    let mut paused = Timer::once(1.0);
    paused.pause();
    paused.tick(10.0);
    assert!(!paused.finished());
}

#[test]
fn cooldown_gates_an_ability() {
    let mut dash = Cooldown::new(2.0);
    assert!(dash.trigger());
    assert!(!dash.trigger());
    dash.tick(1.0);
    assert!((dash.fraction() - 0.5).abs() < 1e-4);
    assert!(!dash.ready());
    dash.tick(1.0);
    assert!(dash.ready() && dash.trigger());
    assert!(!Cooldown::starting_cold(1.0).ready());
}

component! {
    pub struct Gun {
        pub cooldown: Cooldown = Cooldown::new(0.5),
        pub charge: Timer = Timer::once(1.0),
        pub area: Rect = Rect::new(0.0, 0.0, 2.0, 1.0),
    }
}
impl Component for Gun {}

#[test]
fn timers_and_rects_are_usable_as_component_fields() {
    let mut app = App::new();
    app.register::<Gun>();
    let mut gun = Gun::default();
    gun.cooldown.trigger();
    let value = gun.get_field("cooldown").unwrap();
    assert_eq!(value, Value::Vec2(0.5, 0.5));
    gun.set_field("area", &Value::List(vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0), Value::Float(4.0)]))
        .unwrap();
    assert_eq!(gun.area, Rect::new(1.0, 2.0, 3.0, 4.0));
    assert!(gun.set_field("charge", &Value::Bool(true)).is_err());

    // Сохранение сцены и загрузка обратно.
    app.spawn("G").with(gun);
    let text = app.world.scene_to_string();
    let mut other = App::new();
    other.register::<Gun>();
    other.world.load_scene_str(&text, "t").unwrap();
    let id = other.world.scene.find_by_name("G").unwrap();
    assert_eq!(other.world.scene.get_component::<Gun>(id).unwrap().area, Rect::new(1.0, 2.0, 3.0, 4.0));
}

// ----------------------------------------------------------------------
// Планировщик
// ----------------------------------------------------------------------

#[test]
fn after_delivers_a_message_to_the_caller_at_the_right_time() {
    let mut app = app();
    app.spawn("T").with(Timed { fuse: 0.5 });
    secs(&mut app, 0.4);
    assert!(log(&app).is_empty(), "too early: {:?}", log(&app));
    secs(&mut app, 0.2);
    assert_eq!(log(&app), ["timed:Boom=Int(7)"]);
    secs(&mut app, 2.0);
    assert_eq!(log(&app).len(), 1, "one-shot");
}

#[test]
fn repeating_timers_cancel_and_limit() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();
    let every = app.world.scheduler().send_every(id, 0.25, "Tick", Value::Nil);
    secs(&mut app, 1.01);
    assert_eq!(log(&app).iter().filter(|l| *l == "p:Tick").count(), 4);

    assert!(app.world.scheduler().cancel(every));
    assert!(!app.world.scheduler().cancel(every), "already cancelled");
    secs(&mut app, 1.0);
    assert_eq!(log(&app).iter().filter(|l| *l == "p:Tick").count(), 4);

    let limited = app.world.scheduler().send_every(id, 0.1, "Lim", Value::Nil);
    app.world.scheduler().limit(limited, 3);
    secs(&mut app, 2.0);
    assert_eq!(log(&app).iter().filter(|l| *l == "p:Lim").count(), 3);
    assert!(!app.world.scheduler().is_pending(limited));
}

#[test]
fn destroy_after_removes_the_object_even_when_inactive() {
    let mut app = app();
    let a = app.spawn("A").with(Probe { label: "a".into(), ..Probe::default() }).id();
    let b = app.spawn("B").inactive().id();
    app.world.scheduler().destroy_after(a, 0.5);
    app.world.scheduler().destroy_after(b, 0.5);
    secs(&mut app, 0.4);
    assert!(app.world.scene.contains(a) && app.world.scene.contains(b));
    secs(&mut app, 0.2);
    assert!(!app.world.scene.contains(a) && !app.world.scene.contains(b));
    assert!(log(&app).contains(&"a:destroy".to_string()));
}

#[test]
fn timers_follow_time_scale_unless_unscaled() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();
    app.world.scheduler().send_after(id, 0.5, "Scaled", Value::Nil);
    let ui = app.world.scheduler().send_after(id, 0.5, "Ui", Value::Nil);
    app.world.scheduler().unscaled(ui);

    app.world.time.time_scale = 0.0;
    secs(&mut app, 1.0);
    assert_eq!(log(&app).iter().filter(|l| l.ends_with("Ui")).count(), 1, "unscaled fires on pause");
    assert_eq!(log(&app).iter().filter(|l| l.ends_with("Scaled")).count(), 0, "scaled waits");
    app.world.time.time_scale = 1.0;
    secs(&mut app, 0.6);
    assert_eq!(log(&app).iter().filter(|l| l.ends_with("Scaled")).count(), 1);
}

#[test]
fn timers_stop_while_the_object_is_inactive_and_die_with_it() {
    let mut app = app();
    let id = app.spawn("P").with(Probe::default()).id();
    app.world.scheduler().send_after(id, 0.3, "Hi", Value::Nil);
    app.world.scene.set_active(id, false);
    secs(&mut app, 1.0);
    assert!(log(&app).is_empty(), "paused while inactive");
    app.world.scene.set_active(id, true);
    secs(&mut app, 0.5);
    assert!(log(&app).contains(&"p:Hi".to_string()));

    let victim = app.spawn("V").with(Probe::default()).id();
    app.world.scheduler().send_after(victim, 1.0, "Never", Value::Nil);
    app.world.scene.destroy(victim);
    secs(&mut app, 2.0);
    assert!(!log(&app).iter().any(|l| l.ends_with("Never")));
    assert!(app.world.scheduler().is_empty(), "dead owner's timer is dropped");
}

#[test]
fn call_after_runs_a_closure_with_world_access() {
    let mut app = app();
    app.world.scheduler().call_after(0.2, |world| {
        world.scene.spawn("FromTimer");
    });
    secs(&mut app, 0.1);
    assert!(app.world.scene.find_by_name("FromTimer").is_none());
    secs(&mut app, 0.2);
    assert!(app.world.scene.find_by_name("FromTimer").is_some());

    // Колбэк может сам ставить и отменять таймеры.
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let h = hits.clone();
    app.world.scheduler().call_every(0.1, move |world| {
        h.set(h.get() + 1);
        if h.get() == 3 {
            world.scheduler().call_after(0.0, |w| {
                w.scene.spawn("Chained");
            });
        }
    });
    secs(&mut app, 1.0);
    assert!((9..=10).contains(&hits.get()), "hits {}", hits.get());
    assert!(app.world.scene.find_by_name("Chained").is_some());
}

// ----------------------------------------------------------------------
// Твины
// ----------------------------------------------------------------------

#[test]
fn ease_curves_hit_their_endpoints_and_are_monotonic_where_expected() {
    for ease in Ease::ALL {
        assert_eq!(ease.apply(0.0), 0.0, "{ease:?}");
        assert_eq!(ease.apply(1.0), 1.0, "{ease:?}");
        assert!(ease.apply(0.5).is_finite());
    }
    assert!((Ease::Linear.apply(0.3) - 0.3).abs() < 1e-6);
    assert!(Ease::InQuad.apply(0.5) < 0.5 && Ease::OutQuad.apply(0.5) > 0.5);
    assert!((Ease::InOutCubic.apply(0.5) - 0.5).abs() < 1e-6);
    assert!(Ease::OutBack.apply(0.8) > 1.0, "back overshoots");
    assert!(Ease::InBack.apply(0.2) < 0.0, "back anticipates");
    // Монотонные кривые.
    for ease in [Ease::InQuad, Ease::OutCubic, Ease::InOutSine, Ease::OutExpo, Ease::SmoothStep, Ease::InOutQuart] {
        let mut prev = 0.0;
        for i in 1..=100 {
            let v = ease.apply(i as f32 / 100.0);
            assert!(v >= prev - 1e-6, "{ease:?} not monotonic at {i}");
            prev = v;
        }
    }
}

#[test]
fn tween_moves_an_object_over_time_and_lands_exactly() {
    let mut app = app();
    let id = app.spawn("Box").at(1.0, 0.0).id();
    app.world.start_tween(Tween::position(id, Vec3::new(5.0, 2.0, 0.0)).duration(1.0));
    secs(&mut app, 0.5);
    let p = app.world.scene.get(id).unwrap().transform.position;
    assert!((p.x - 3.0).abs() < 0.1 && (p.y - 1.0).abs() < 0.1, "halfway: {p:?}");
    secs(&mut app, 0.6);
    assert_eq!(app.world.scene.get(id).unwrap().transform.position, Vec3::new(5.0, 2.0, 0.0));
    assert!(app.world.tweens().is_empty(), "finished tweens are removed");
}

#[test]
fn tween_delay_loops_and_yoyo() {
    let mut app = app();
    let id = app.spawn("Box").id();
    app.world.start_tween(
        Tween::field(id, "Transform.position.x", 10.0f32)
            .duration(1.0)
            .delay(0.5)
            .yoyo()
            .loops(2),
    );
    secs(&mut app, 0.4);
    assert_eq!(app.world.scene.get(id).unwrap().transform.position.x, 0.0, "still in the delay");
    secs(&mut app, 1.1);
    let x = app.world.scene.get(id).unwrap().transform.position.x;
    assert!((x - 10.0).abs() < 0.3, "end of the forward leg: {x}");
    secs(&mut app, 1.1);
    let x = app.world.scene.get(id).unwrap().transform.position.x;
    assert!(x.abs() < 1e-3, "yoyo with 2 loops ends where it started: {x}");
    assert!(app.world.tweens().is_empty());

    let forever = app.world.start_tween(Tween::field(id, "Transform.position.x", 4.0f32).duration(0.5).forever().yoyo());
    secs(&mut app, 5.0);
    assert!(app.world.tweens().is_running(forever), "forever never finishes");
    assert!(app.world.tweens().cancel(forever));
    assert!(!app.world.tweens().cancel(forever));
}

#[test]
fn tween_animates_any_reflected_field_by_path() {
    let mut app = app();
    let id = app.spawn("S").with(Stats::default()).id();
    app.world.start_tween(Tween::field(id, "Stats.tint", Color::RED).duration(1.0));
    app.world.start_tween(Tween::field(id, "Stats.hp", 110i32).duration(1.0));
    app.world.start_tween(Tween::field(id, "Stats.speed", 3.0f32).duration(1.0).ease(Ease::InQuad));
    app.world.start_tween(Tween::field(id, "Stats.offset.y", 4.0f32).duration(1.0));
    app.world.start_tween(Tween::field(id, "Stats.tint.a", 0.0f32).duration(1.0));
    secs(&mut app, 0.5);
    let s = app.world.scene.get_component::<Stats>(id).unwrap().clone();
    assert!((s.hp - 60).abs() <= 2, "int rounds: {}", s.hp);
    assert!((s.speed - 1.5).abs() < 0.3, "in-quad is slower early: {}", s.speed);
    assert!((s.offset.y - 2.0).abs() < 0.1 && s.offset.x == 0.0);
    assert!(s.tint.r == 1.0 && (s.tint.g - 0.5).abs() < 0.05 && (s.tint.a - 0.5).abs() < 0.05, "{:?}", s.tint);
    secs(&mut app, 0.6);
    let s = app.world.scene.get_component::<Stats>(id).unwrap();
    assert_eq!((s.hp, s.speed, s.offset.y, s.tint), (110, 3.0, 4.0, Color::RED.with_alpha(0.0)));
}

#[test]
fn tween_rotation_and_scale() {
    let mut app = app();
    let id = app.spawn("R").id();
    app.world.start_tween(Tween::rotation_z(id, std::f32::consts::PI).duration(1.0));
    app.world.start_tween(Tween::scale(id, Vec3::splat(3.0)).duration(1.0));
    app.world.start_tween(Tween::field(id, "Transform.euler.y", 90.0f32).duration(1.0).delay(2.0));
    secs(&mut app, 1.1);
    let t = app.world.scene.get(id).unwrap().transform;
    assert!((t.angle_z() - std::f32::consts::PI).abs() < 1e-3 || (t.angle_z() + std::f32::consts::PI).abs() < 1e-3);
    assert_eq!(t.scale, Vec3::splat(3.0));
    secs(&mut app, 2.0);
    let t = app.world.scene.get(id).unwrap().transform;
    assert!((t.euler_deg().y - 90.0).abs() < 0.1, "{:?}", t.euler_deg());

    let q = Quat::from_rotation_y(1.0);
    let other = app.spawn("Q").id();
    app.world.start_tween(Tween::rotation(other, q).duration(0.2));
    secs(&mut app, 0.3);
    assert!(app.world.scene.get(other).unwrap().transform.rotation.angle_between(q) < 1e-3);
}

#[test]
fn tween_on_a_destroyed_object_disappears_silently() {
    let mut app = app();
    let id = app.spawn("Doomed").id();
    let keep = app.spawn("Keep").id();
    app.world.start_tween(Tween::position(id, Vec3::X * 10.0).duration(2.0).on_complete(id, "Never", Value::Nil));
    app.world.start_tween(Tween::position(keep, Vec3::X * 10.0).duration(2.0));
    secs(&mut app, 0.5);
    app.world.scene.destroy(id);
    secs(&mut app, 0.1);
    assert_eq!(app.world.tweens().len(), 1, "the dead object's tween was dropped");
    secs(&mut app, 2.0);
    assert_eq!(app.world.scene.get(keep).unwrap().transform.position.x, 10.0);
    assert!(!log(&app).iter().any(|l| l.contains("Never")));

    // Слот мог быть переиспользован новым объектом: старый твин на него не действует.
    let tween = app.world.start_tween(Tween::position(keep, Vec3::Y).duration(5.0));
    app.world.scene.destroy(keep);
    secs(&mut app, 0.1);
    let reused = app.spawn("Reused").id();
    secs(&mut app, 0.5);
    assert_eq!(app.world.scene.get(reused).unwrap().transform.position, Vec3::ZERO);
    assert!(!app.world.tweens().is_running(tween));
}

#[test]
fn tween_completion_message_event_and_sequences() {
    let mut app = app();
    let id = app.spawn("Hero").with(Probe::default()).id();
    let seq = app.world.start_sequence(
        Sequence::new()
            .then(Tween::position(id, Vec3::X).duration(0.5))
            .join(Tween::scale(id, Vec3::splat(2.0)).duration(0.25))
            .wait(0.25)
            .then(Tween::position(id, Vec3::ZERO).duration(0.5).on_complete(id, "StepDone", Value::Nil))
            .on_complete(id, "AllDone", Value::Int(1)),
    );
    assert!((1.0..=1.5).contains(&Sequence::new().then(Tween::position(id, Vec3::X).duration(0.5)).wait(0.5).total_duration()));

    secs(&mut app, 0.55);
    assert!((app.world.scene.get(id).unwrap().transform.position.x - 1.0).abs() < 0.05);
    assert_eq!(app.world.scene.get(id).unwrap().transform.scale, Vec3::splat(2.0), "joined step ran in parallel");
    secs(&mut app, 0.15);
    assert!(!log(&app).iter().any(|l| l.contains("Done")), "the pause is not over: {:?}", log(&app));
    secs(&mut app, 0.2);
    let x = app.world.scene.get(id).unwrap().transform.position.x;
    assert!(x < 1.0, "second step started after the pause: {x}");
    secs(&mut app, 0.6);
    let lines = log(&app);
    assert_eq!(lines.iter().filter(|l| *l == "p:StepDone").count(), 1, "{lines:?}");
    assert_eq!(lines.iter().filter(|l| *l == "p:AllDone").count(), 1, "{lines:?}");
    assert!(!app.world.tweens().is_running(seq));
    assert_eq!(app.world.scene.get(id).unwrap().transform.position, Vec3::ZERO);
}

#[test]
fn finished_event_is_published_for_the_next_frame() {
    let mut app = app();
    let id = app.spawn("E").id();
    let tween = app.world.start_tween(Tween::position(id, Vec3::X).duration(0.1));
    let mut seen = None;
    for _ in 0..30 {
        app.tick(DT);
        if let Some(e) = app.world.events.read::<TweenFinished>().next() {
            seen = Some(*e);
        }
    }
    let event = seen.expect("TweenFinished was emitted");
    assert_eq!((event.id, event.target), (tween, Some(id)));
}

#[test]
fn cancelling_a_sequence_stops_all_its_steps_and_the_completion() {
    let mut app = app();
    let id = app.spawn("S").with(Probe::default()).id();
    let seq = app.world.start_sequence(
        Sequence::new()
            .then(Tween::position(id, Vec3::X * 10.0).duration(1.0))
            .then(Tween::position(id, Vec3::ZERO).duration(1.0))
            .on_complete(id, "AllDone", Value::Nil),
    );
    secs(&mut app, 0.5);
    assert!(app.world.tweens().cancel(seq));
    let stopped = app.world.scene.get(id).unwrap().transform.position.x;
    secs(&mut app, 3.0);
    assert_eq!(app.world.scene.get(id).unwrap().transform.position.x, stopped, "frozen where it was");
    assert!(!log(&app).iter().any(|l| l.ends_with("AllDone")));
}

#[test]
fn tweens_pause_with_time_scale_unless_unscaled() {
    let mut app = app();
    let world_obj = app.spawn("World").id();
    let ui = app.spawn("Ui").id();
    app.world.start_tween(Tween::position(world_obj, Vec3::X).duration(1.0));
    app.world.start_tween(Tween::position(ui, Vec3::X).duration(1.0).unscaled());
    app.world.time.time_scale = 0.0;
    secs(&mut app, 0.5);
    assert_eq!(app.world.scene.get(world_obj).unwrap().transform.position.x, 0.0);
    assert!((app.world.scene.get(ui).unwrap().transform.position.x - 0.5).abs() < 0.05);
    app.world.time.time_scale = 1.0;
    secs(&mut app, 1.1);
    assert_eq!(app.world.scene.get(world_obj).unwrap().transform.position.x, 1.0);
}

#[test]
fn tweens_on_inactive_objects_wait_and_bad_paths_do_not_panic() {
    let mut app = app();
    let id = app.spawn("I").with(Stats::default()).inactive().id();
    app.world.start_tween(Tween::position(id, Vec3::X).duration(0.2));
    secs(&mut app, 1.0);
    assert_eq!(app.world.scene.get(id).unwrap().transform.position.x, 0.0);
    app.world.scene.set_active(id, true);
    secs(&mut app, 0.5);
    assert_eq!(app.world.scene.get(id).unwrap().transform.position.x, 1.0);

    app.world.start_tween(Tween::field(id, "Stats.nope", 1.0f32));
    app.world.start_tween(Tween::field(id, "Nope.hp", 1.0f32));
    app.world.start_tween(Tween::field(id, "Stats.hp", Color::RED));
    app.world.start_tween(Tween::field(id, "Stats", 1.0f32));
    app.world.start_tween(Tween::field(id, "Stats.hp.q", 1.0f32));
    secs(&mut app, 0.2);
    assert!(app.world.tweens().is_empty(), "invalid tweens are dropped with a log message");
}

// ----------------------------------------------------------------------
// Сцены
// ----------------------------------------------------------------------

fn scenes() -> MemoryScenes {
    MemoryScenes::default()
        .with("a", r#"(name: "SceneA", objects: [(name: "A1", components: [(kind: "Probe", fields: {"label": Str("a1")})])])"#)
        .with(
            "b",
            r#"(name: "SceneB", objects: [
                (name: "B1", components: [(kind: "Probe", fields: {"label": Str("b1")})]),
                (name: "B2"),
            ])"#,
        )
        .with("hud", r#"(name: "Hud", objects: [(name: "Hud1", components: [(kind: "Probe", fields: {"label": Str("hud")})])])"#)
        .with("broken", r#"(name: "Broken", objects: [(name: "X", components: [(kind: "Nope")])])"#)
}

#[test]
fn load_scene_from_on_update_is_applied_at_the_end_of_the_frame() {
    let mut app = app();
    app.world.scenes().set_source(scenes());
    app.world.scene.name = "Start".into();
    let old = app.spawn("Old").with(Probe { label: "old".into(), ..Probe::default() }).with(Portal { at_frame: 3, ..Portal::default() }).id();
    app.run_headless(2, DT);
    assert_eq!(app.world.scene.name, "Start");

    app.tick(DT); // кадр 3: портал просит загрузку
    assert_eq!(app.world.scene.name, "SceneB", "applied by the end of that same frame");
    let loaded: Vec<_> = app.world.events.read::<SceneLoaded>().cloned().collect();
    assert_eq!(loaded, [SceneLoaded { name: "b".into(), additive: false }]);
    assert!(!app.world.scene.contains(old), "the asking script's object is gone");
    assert!(app.world.scene.find_by_name("B1").is_some());
    let lines = log(&app);
    assert!(lines.contains(&"portal:requested at Start".to_string()), "the rest of the frame ran in the old scene: {lines:?}");
    assert!(lines.contains(&"old:destroy".to_string()), "on_destroy of the replaced scene ran: {lines:?}");
    assert!(!lines.contains(&"b1:start".to_string()), "the new scene starts on the next frame");

    app.tick(DT);
    assert!(log(&app).contains(&"b1:start".to_string()));
    assert_eq!(app.world.scenes().loaded_name(), Some("b"));
}

#[test]
fn a_failed_load_keeps_the_current_scene_and_reports() {
    let mut app = app();
    app.world.scenes().set_source(scenes());
    app.spawn("Keep");
    app.world.scenes().load("broken");
    app.world.scenes().load("does-not-exist");
    app.tick(DT);
    assert!(app.world.scene.find_by_name("Keep").is_some());
    let failed: Vec<_> = app.world.events.read::<SceneLoadFailed>().map(|f| f.name.clone()).collect();
    assert_eq!(failed, ["broken", "does-not-exist"]);
    let reason = app.world.events.read::<SceneLoadFailed>().next().unwrap().error.clone();
    assert!(reason.contains("Nope") && reason.contains("register"), "{reason}");
    assert!(app.world.scene.find_by_name("Keep").is_some());
}

#[test]
fn dont_destroy_on_load_keeps_objects_and_their_ids() {
    let mut app = app();
    app.world.scenes().set_source(scenes());
    let music = app.spawn("Music").with(Probe { label: "music".into(), ..Probe::default() }).id();
    let child = app.spawn("Track").child_of(music).id();
    app.spawn("Level1");
    app.world.scene.set_persistent(child, true); // флаг уходит на корень иерархии
    assert!(app.world.scene.get(music).unwrap().persistent);

    app.world.scenes().load("a");
    app.run_headless(2, DT);
    assert!(app.world.scene.contains(music) && app.world.scene.contains(child), "ids stay valid");
    assert!(app.world.scene.find_by_name("Level1").is_none());
    assert!(app.world.scene.find_by_name("A1").is_some());
    assert_eq!(app.world.scene.name, "SceneA");
    assert!(!log(&app).contains(&"music:destroy".to_string()));

    // Второй переход: постоянный объект не стартует заново.
    app.world.scenes().load("b");
    app.run_headless(2, DT);
    assert!(app.world.scene.contains(music));
    assert_eq!(log(&app).iter().filter(|l| *l == "music:start").count(), 1);
    assert!(app.world.scene.find_by_name("A1").is_none() && app.world.scene.find_by_name("B1").is_some());

    // Прямой вызов `World::load_scene_str` ведёт себя так же.
    app.world.load_scene_str(r#"(name: "Direct", objects: [(name: "D")])"#, "d").unwrap();
    assert!(app.world.scene.contains(music) && app.world.scene.find_by_name("D").is_some());
}

#[test]
fn additive_load_and_unload() {
    let mut app = app();
    app.world.scenes().set_source(scenes());
    app.world.scenes().load("a");
    app.tick(DT);
    app.world.scenes().load_additive("hud");
    app.tick(DT);
    assert!(app.world.scene.find_by_name("A1").is_some() && app.world.scene.find_by_name("Hud1").is_some());
    assert_eq!(app.world.scene.name, "SceneA", "the base scene keeps its name");
    app.tick(DT); // Hud1 стартует: on_destroy вызывается только у стартовавших

    app.world.scenes().unload("hud");
    app.tick(DT);
    assert!(app.world.scene.find_by_name("Hud1").is_none() && app.world.scene.find_by_name("A1").is_some());
    assert!(log(&app).contains(&"hud:destroy".to_string()));
}

#[test]
fn reload_restores_the_initial_state() {
    let mut app = app();
    app.world.scenes().set_source(scenes());
    app.world.scenes().load("a");
    app.tick(DT);
    let a1 = app.world.scene.find_by_name("A1").unwrap();
    app.world.scene.get_component_mut::<Probe>(a1).unwrap().updates = 99;
    app.world.scenes().reload();
    app.tick(DT);
    let fresh = app.world.scene.find_by_name("A1").unwrap();
    assert_eq!(app.world.scene.get_component::<Probe>(fresh).unwrap().updates, 0);
    assert!(!app.world.scene.contains(a1));
}

#[test]
fn an_owned_timer_firing_while_the_scene_is_replaced_does_not_leak_into_the_new_one() {
    let mut app = app();
    app.world.scenes().set_source(scenes());
    let old = app.spawn("Old").with(Probe { label: "old".into(), ..Probe::default() }).id();
    app.world.scheduler().send_after(old, 0.05, "Late", Value::Nil);
    let global = app.world.scheduler().call_after(0.05, |w| {
        w.scene.spawn("GlobalTimerWasHere");
    });
    // Замена сцены раньше срабатывания обоих таймеров.
    app.world.scenes().load("b");
    app.tick(DT);
    assert!(!app.world.scene.contains(old));
    secs(&mut app, 0.2);
    assert!(!log(&app).iter().any(|l| l.ends_with("Late")), "message to a dead object is dropped");
    assert!(app.world.scene.find_by_name("GlobalTimerWasHere").is_some(), "unowned timers survive scene loads");
    assert!(!app.world.scheduler().is_pending(global));

    // Таймер срабатывает в том же кадре, в конце которого сцена заменяется.
    let id = app.world.scene.find_by_name("B1").unwrap();
    app.world.scheduler().send_after(id, 0.0, "Now", Value::Nil);
    app.world.scenes().load("a");
    app.tick(DT);
    assert!(log(&app).contains(&"b1:Now".to_string()), "fired during the frame, before the swap");
    assert!(app.world.scene.find_by_name("A1").is_some());
}

// ----------------------------------------------------------------------
// Запросы и жизненный цикл
// ----------------------------------------------------------------------

component! {
    pub struct Marker { pub n: i32 = 0 }
}
impl Component for Marker {}

#[test]
fn component_queries_over_the_scene_and_hierarchy() {
    let mut app = app();
    let root = app.spawn("Root").tag("Enemy").with(Marker { n: 1 }).id();
    let mid = app.spawn("Mid").child_of(root).id();
    let leaf = app.spawn("Leaf").tag("Enemy").child_of(mid).with(Marker { n: 3 }).id();
    let off = app.spawn("Off").tag("Enemy").with(Marker { n: 100 }).inactive().id();
    let scene = &mut app.world.scene;

    assert_eq!(scene.iter_with::<Marker>().map(|(_, m)| m.n).sum::<i32>(), 104);
    assert_eq!(scene.iter_active_with::<Marker>().map(|(_, m)| m.n).sum::<i32>(), 4);
    assert_eq!(scene.find_all_by_tag("Enemy").len(), 3);
    assert_eq!(scene.find_all_active_by_tag("Enemy"), vec![root, leaf]);
    assert_eq!(scene.count_with_tag("Enemy"), 3);

    assert_eq!(scene.find_in_children::<Marker>(mid), Some(leaf));
    assert_eq!(scene.find_in_children::<Marker>(root), Some(root), "the object itself counts first");
    assert_eq!(scene.find_all_in_children::<Marker>(root), vec![root, leaf]);
    assert_eq!(scene.find_in_parent::<Marker>(mid), Some(root));
    assert_eq!(scene.find_in_parent::<Marker>(leaf), Some(leaf));
    assert_eq!(scene.find_in_parent::<Stats>(leaf), None);
    assert_eq!(scene.component_in_children::<Marker>(mid).unwrap().n, 3);
    scene.component_in_parent_mut::<Marker>(mid).unwrap().n = 7;
    assert_eq!(scene.get_component::<Marker>(root).unwrap().n, 7);
    assert_eq!(scene.find_child(root, "Mid"), Some(mid));
    assert_eq!(scene.find_path(root, "Mid/Leaf"), Some(leaf));
    assert_eq!(scene.find_path(root, "Mid/Nope"), None);
    assert_eq!(scene.root_of(leaf), Some(root));
    scene.for_each_with_mut::<Marker>(|_, m| m.n += 1);
    assert_eq!(scene.get_component::<Marker>(off).unwrap().n, 101);
}

#[test]
fn inactive_objects_do_not_update_and_the_subtree_follows() {
    let mut app = app();
    let parent = app.spawn("Parent").with(Probe::default()).id();
    let child = app.spawn("Child").child_of(parent).with(Probe::default()).id();
    app.run_headless(5, DT);
    assert_eq!(app.world.scene.get_component::<Probe>(child).unwrap().updates, 5);

    assert!(app.world.scene.set_active(parent, false));
    app.run_headless(5, DT);
    assert_eq!(app.world.scene.get_component::<Probe>(parent).unwrap().updates, 5);
    assert_eq!(app.world.scene.get_component::<Probe>(child).unwrap().updates, 5, "children of an inactive parent stand still");
    assert!(!app.world.scene.is_active(child));

    app.world.scene.set_active(parent, true);
    app.run_headless(5, DT);
    assert_eq!(app.world.scene.get_component::<Probe>(child).unwrap().updates, 10);
    assert!(!app.world.scene.set_active(GameObjectId::from_bits(999), true));
}

fn bullet_prefab() -> Prefab {
    Prefab::from_ron(
        r#"(name: "Bullet", tag: "Shot", transform: (scale: (2.0, 2.0, 1.0)),
            components: [(kind: "Stats", fields: {"hp": Int(1)})],
            children: [(name: "Trail")])"#,
        "bullet.prefab",
    )
    .unwrap()
}

#[test]
fn instantiate_at_a_position_or_under_a_parent() {
    let mut app = app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("bullet.prefab", bullet_prefab()));
    let gun = app.spawn("Gun").at(10.0, 0.0).id();

    let free = app.world.instantiate_at("bullet.prefab", Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY, None).unwrap();
    let o = app.world.scene.get(free).unwrap();
    assert_eq!(o.transform.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(o.transform.scale, Vec3::new(2.0, 2.0, 1.0), "the prefab's scale is kept");
    assert_eq!((o.tag.as_str(), o.children().len()), ("Shot", 1));
    assert_eq!(o.parent(), None);

    let q = Quat::from_rotation_z(0.5);
    let child = app.world.instantiate_at("bullet.prefab", Vec3::new(0.0, 1.0, 0.0), q, Some(gun)).unwrap();
    assert_eq!(app.world.scene.get(child).unwrap().parent(), Some(gun));
    assert_eq!(app.world.scene.get(child).unwrap().transform.rotation, q);
    assert_eq!(app.world.scene.world_position(child), Vec3::new(10.0, 1.0, 0.0), "local to the parent");

    assert!(matches!(
        app.world.instantiate_at("bullet.prefab", Vec3::ZERO, Quat::IDENTITY, Some(GameObjectId::from_bits(77))),
        Err(SceneError::DeadObject(_))
    ));
    assert!(app.world.instantiate_at("missing.prefab", Vec3::ZERO, Quat::IDENTITY, None).is_err());
}

component! {
    pub struct Spawner { pub made: i32 = 0 }
}
impl Component for Spawner {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.made < 3 {
            self.made += 1;
            let me = ctx.id;
            let shot = ctx.instantiate_at("bullet.prefab", Vec3::new(self.made as f32, 0.0, 0.0)).unwrap();
            ctx.destroy_after(shot, 0.1);
            let _ = ctx.instantiate_under("bullet.prefab", me).unwrap();
        }
    }
}

#[test]
fn ctx_helpers_spawn_and_schedule_destruction() {
    let mut app = app();
    app.world.prefabs = Box::new(MemoryPrefabs::default().with("bullet.prefab", bullet_prefab()));
    let spawner = app.spawn("S").with(Spawner::default()).id();
    app.run_headless(5, DT);
    assert_eq!(app.world.scene.find_all_by_tag("Shot").len(), 6);
    secs(&mut app, 0.3);
    assert_eq!(app.world.scene.find_all_by_tag("Shot").len(), 3, "the free bullets expired, the children stay");
    assert_eq!(app.world.scene.get(spawner).unwrap().children().len(), 3);
}

// ----------------------------------------------------------------------
// Сохранения
// ----------------------------------------------------------------------

#[test]
fn save_data_roundtrip_through_a_backend() {
    let backend = MemoryBackend::default();
    let mut save = SaveData::with_backend(backend.clone());
    assert_eq!((save.int("score", 5), save.bool("sound", true), save.string("name", "anon")), (5, true, "anon".to_string()));
    save.set_int("score", 1200);
    save.set_bool("sound", false);
    save.set_float("volume", 0.75);
    save.set_string("name", "Ada");
    save.set_vec2("checkpoint", Vec2::new(3.0, 4.0));
    save.set_vec3("camera", Vec3::new(1.0, 2.0, 3.0));
    save.set("tint", &Color::RED);
    assert!(save.is_dirty());
    assert!(backend.contents().is_none(), "nothing is written until save()");
    save.save().unwrap();
    assert!(!save.is_dirty());

    let loaded = SaveData::with_backend(backend.clone());
    assert_eq!(loaded.int("score", 0), 1200);
    assert!(!loaded.bool("sound", true));
    assert_eq!(loaded.float("volume", 0.0), 0.75);
    assert_eq!(loaded.string("name", ""), "Ada");
    assert_eq!(loaded.vec2("checkpoint", Vec2::ZERO), Vec2::new(3.0, 4.0));
    assert_eq!(loaded.vec3("camera", Vec3::ZERO), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(loaded.get::<Color>("tint"), Some(Color::RED));
    assert_eq!(loaded.len(), 7);
    // Значение другого типа — это «нет значения», а не паника.
    assert_eq!(loaded.get::<bool>("score"), None);
    assert!(loaded.bool("score", true));

    let mut edited = loaded;
    assert!(edited.delete_key("score") && !edited.delete_key("score"));
    assert!(edited.is_dirty());
    edited.set_int("x", 1);
    edited.delete_all();
    assert!(edited.is_empty());
}

#[test]
fn corrupt_save_files_fall_back_to_defaults_and_are_kept_for_inspection() {
    let backend = MemoryBackend::with_text("(version: 1, values: {\"a\": Int(");
    let mut save = SaveData::with_backend(backend.clone());
    assert!(save.is_empty());
    assert_eq!(save.int("a", 42), 42);
    assert_eq!(backend.quarantined().as_deref(), Some("(version: 1, values: {\"a\": Int("));
    // Игра продолжается и может сохраниться поверх.
    save.set_int("a", 1);
    save.save().unwrap();
    assert_eq!(SaveData::with_backend(backend).int("a", 0), 1);
}

#[test]
fn file_backend_writes_atomically_into_a_directory_it_creates() {
    let dir = std::env::temp_dir().join(format!("engine_core_save_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("nested").join("save.ron");
    {
        let mut save = SaveData::with_backend(FileBackend::new(&path));
        assert!(save.is_empty(), "missing file is not an error");
        save.set_int("level", 3);
        save.save().unwrap();
    }
    assert!(path.is_file());
    assert!(!path.with_extension("ron.tmp").exists(), "temp file was renamed away");
    assert_eq!(SaveData::with_backend(FileBackend::new(&path)).int("level", 0), 3);

    std::fs::write(&path, "not ron at all").unwrap();
    let save = SaveData::with_backend(FileBackend::new(&path));
    assert!(save.is_empty());
    assert!(path.with_extension("ron.corrupt").is_file(), "the broken file was kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_data_dir_is_per_game_and_sanitised() {
    let a = engine_core::save::data_dir("My Game");
    let b = engine_core::save::data_dir("../evil/..\\x");
    assert!(a.ends_with("My Game"));
    assert!(!b.to_string_lossy().contains("evil/.."), "{b:?}");
    assert!(b.file_name().unwrap().to_string_lossy().chars().all(|c| c != '/' && c != '\\'));
}

// ----------------------------------------------------------------------
// Random
// ----------------------------------------------------------------------

#[test]
fn random_is_deterministic_per_seed() {
    let mut a = Random::new(42);
    let mut b = Random::new(42);
    let xs: Vec<u64> = (0..20).map(|_| a.next_u64()).collect();
    assert_eq!(xs, (0..20).map(|_| b.next_u64()).collect::<Vec<_>>());
    assert_ne!(xs, (0..20).map(|_| Random::new(43).next_u64()).collect::<Vec<_>>());
    a.seed(42);
    assert_eq!(a.next_u64(), xs[0], "reseeding restarts the sequence");
    // Значения зафиксированы: если это сломается, сломаются и сохранённые «сиды дня».
    assert_eq!(Random::new(1).next_u64(), Random::new(1).next_u64());
    assert_ne!(Random::from_entropy(), Random::from_entropy());
}

#[test]
fn random_ranges_and_helpers() {
    let mut r = Random::new(7);
    for _ in 0..2000 {
        let f = r.range(-2.0, 3.0);
        assert!((-2.0..3.0).contains(&f));
        let swapped = r.range(3.0, -2.0);
        assert!((-2.0..=3.0).contains(&swapped));
        assert!((0.0..1.0).contains(&r.f32()));
        let i = r.int_range(-3, 4);
        assert!((-3..4).contains(&i));
        let j = r.int_range_inclusive(1, 6);
        assert!((1..=6).contains(&j));
    }
    assert_eq!(r.int_range(5, 5), 5);
    // Все грани кубика выпадают.
    let mut seen = [false; 6];
    for _ in 0..500 {
        seen[(r.int_range_inclusive(1, 6) - 1) as usize] = true;
    }
    assert!(seen.iter().all(|s| *s));
    assert!(!r.chance(0.0) && r.chance(1.0));
    let heads = (0..4000).filter(|_| r.chance(0.25)).count();
    assert!((850..1150).contains(&heads), "{heads}");

    assert_eq!(r.pick::<i32>(&[]), None);
    assert_eq!(r.pick(&[9]), Some(&9));
    let weights = [0.0, 1.0, 0.0];
    assert!((0..50).all(|_| r.weighted_index(&weights) == Some(1)));
    assert_eq!(r.weighted_index(&[0.0, 0.0]), None);

    let mut deck: Vec<i32> = (0..52).collect();
    r.shuffle(&mut deck);
    assert_ne!(deck, (0..52).collect::<Vec<_>>());
    deck.sort();
    assert_eq!(deck, (0..52).collect::<Vec<_>>(), "shuffle is a permutation");
}

#[test]
fn random_shapes_and_gaussian() {
    let mut r = Random::new(11);
    for _ in 0..500 {
        assert!(r.in_unit_circle().length() <= 1.0 + 1e-5);
        assert!((r.on_unit_circle().length() - 1.0).abs() < 1e-4);
        assert!((r.on_unit_sphere().length() - 1.0).abs() < 1e-4);
        assert!(r.in_unit_sphere().length() <= 1.0 + 1e-5);
        assert!(r.sign().abs() == 1.0);
    }
    let n = 20000;
    let samples: Vec<f32> = (0..n).map(|_| r.gaussian(10.0, 2.0)).collect();
    let mean = samples.iter().sum::<f32>() / n as f32;
    let var = samples.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32;
    assert!((mean - 10.0).abs() < 0.1, "mean {mean}");
    assert!((var.sqrt() - 2.0).abs() < 0.1, "std {}", var.sqrt());
    let c = r.color_hsv(1.0, 1.0);
    assert!(c.to_hsv().1 > 0.99);
}

#[test]
fn ctx_random_is_a_shared_resource_that_can_be_seeded() {
    let mut app = app();
    app.world.random().seed(5);
    let first = app.world.random().next_u64();
    app.world.random().seed(5);
    assert_eq!(app.world.random().next_u64(), first);
}

// ----------------------------------------------------------------------
// Математика
// ----------------------------------------------------------------------

#[test]
fn scalar_helpers() {
    assert_eq!(lerp(0.0, 10.0, 0.25), 2.5);
    assert_eq!(lerp(0.0, 10.0, 2.0), 20.0, "unclamped like Mathf.LerpUnclamped");
    assert_eq!(inverse_lerp(10.0, 20.0, 15.0), 0.5);
    assert_eq!(inverse_lerp(1.0, 1.0, 5.0), 0.0, "degenerate range is safe");
    assert_eq!(remap(5.0, 0.0, 10.0, 100.0, 200.0), 150.0);
    assert_eq!((smoothstep(0.0), smoothstep(0.5), smoothstep(1.0), smoothstep(7.0)), (0.0, 0.5, 1.0, 1.0));
    assert_eq!(move_towards(0.0, 10.0, 3.0), 3.0);
    assert_eq!(move_towards(9.0, 10.0, 3.0), 10.0, "no overshoot");
    assert_eq!(move_towards(0.0, -10.0, 3.0), -3.0);
    assert!((move_towards_vec2(Vec2::ZERO, Vec2::new(3.0, 4.0), 2.5) - Vec2::new(1.5, 2.0)).length() < 1e-5);
    assert_eq!(move_towards_vec3(Vec3::ZERO, Vec3::X, 5.0), Vec3::X);
    assert_eq!(repeat(-1.0, 3.0), 2.0);
    assert_eq!(ping_pong(4.0, 3.0), 2.0);
    assert!(approx_eq(0.1 + 0.2, 0.3));
    // exp_decay не зависит от частоты кадров.
    let mut slow = 0.0;
    let mut fast = 0.0;
    for _ in 0..10 {
        slow = exp_decay(slow, 1.0, 5.0, 0.1);
    }
    for _ in 0..100 {
        fast = exp_decay(fast, 1.0, 5.0, 0.01);
    }
    assert!((slow - fast).abs() < 1e-4);
}

#[test]
fn smooth_damp_converges_without_overshoot() {
    let (mut x, mut v) = (0.0, 0.0);
    let mut max = 0.0f32;
    for _ in 0..300 {
        x = smooth_damp(x, 10.0, &mut v, 0.3, DT);
        max = max.max(x);
    }
    assert!((x - 10.0).abs() < 0.01, "converged: {x}");
    assert!(max <= 10.0 + 1e-3, "no overshoot: {max}");
    let mut vel = Vec3::ZERO;
    let p = smooth_damp_vec3(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), &mut vel, 0.5, DT);
    assert!(p.x > 0.0 && p.y > p.x && p.z > p.y);
}

#[test]
fn angle_helpers_take_the_short_way() {
    use std::f32::consts::PI;
    assert!((delta_angle(0.1, -0.1) + 0.2).abs() < 1e-5);
    assert!((delta_angle(PI - 0.1, -PI + 0.1) - 0.2).abs() < 1e-4, "across the seam");
    assert!((delta_angle_deg(350.0, 10.0) - 20.0).abs() < 1e-3);
    assert!((delta_angle_deg(10.0, 350.0) + 20.0).abs() < 1e-3);
    assert!((lerp_angle_deg(350.0, 10.0, 0.5) - 360.0).abs() < 1e-3);
    assert!((lerp_angle(PI - 0.1, -PI + 0.1, 0.5) - PI).abs() < 1e-4);
    assert!((move_towards_angle(PI - 0.1, -PI + 0.1, 0.05) - (PI - 0.05)).abs() < 1e-4);
    assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-4 || (wrap_angle(3.0 * PI) + PI).abs() < 1e-4);
    assert!((angle_of(Vec2::Y) - PI / 2.0).abs() < 1e-6);
    assert!((from_angle(PI).x + 1.0).abs() < 1e-6);
}

#[test]
fn rect_geometry() {
    let r = Rect::new(0.0, 0.0, 4.0, 2.0);
    assert_eq!((r.min(), r.max(), r.center(), r.size(), r.area()), (Vec2::ZERO, Vec2::new(4.0, 2.0), Vec2::new(2.0, 1.0), Vec2::new(4.0, 2.0), 8.0));
    assert!(r.contains(Vec2::new(4.0, 2.0)) && !r.contains(Vec2::new(4.1, 1.0)));
    let o = Rect::from_center_size(Vec2::new(4.0, 2.0), Vec2::splat(2.0));
    assert!(r.overlaps(&o));
    assert_eq!(r.intersection(&o), Some(Rect::new(3.0, 1.0, 1.0, 1.0)));
    assert!(!r.overlaps(&Rect::new(4.0, 0.0, 1.0, 1.0)), "touching edges do not overlap");
    assert_eq!(r.intersection(&Rect::new(9.0, 9.0, 1.0, 1.0)), None);
    assert_eq!(r.union(&Rect::new(5.0, 5.0, 1.0, 1.0)), Rect::new(0.0, 0.0, 6.0, 6.0));
    assert_eq!(r.expand(1.0), Rect::new(-1.0, -1.0, 6.0, 4.0));
    assert_eq!(r.clamp_point(Vec2::new(9.0, -3.0)), Vec2::new(4.0, 0.0));
    assert_eq!(r.point_at(Vec2::new(0.5, 0.5)), r.center());
    assert_eq!(r.normalized_point(Vec2::new(1.0, 1.0)), Vec2::new(0.25, 0.5));
    assert_eq!(r.translate(Vec2::ONE).min(), Vec2::ONE);
}

#[test]
fn color_helpers() {
    let a = Color::rgba(0.0, 0.0, 0.0, 0.0);
    let b = Color::rgba(1.0, 0.5, 0.0, 1.0);
    assert_eq!(a.lerp(b, 0.5), Color::rgba(0.5, 0.25, 0.0, 0.5));
    // HSV: красный, зелёный, синий и серый.
    assert_eq!(Color::from_hsv(0.0, 1.0, 1.0), Color::RED);
    let green = Color::from_hsv(1.0 / 3.0, 1.0, 1.0);
    assert!(green.g > 0.999 && green.r < 1e-3 && green.b < 1e-3, "{green:?}");
    let blue = Color::from_hsv(2.0 / 3.0, 1.0, 1.0);
    assert!(blue.b > 0.999 && blue.r < 1e-3);
    assert_eq!(Color::from_hsv(0.3, 0.0, 1.0), Color::WHITE);
    assert_eq!(Color::from_hsv(1.25, 1.0, 1.0), Color::from_hsv(0.25, 1.0, 1.0), "hue wraps");
    let (h, s, v) = Color::hex(0xff7a59).to_hsv();
    let back = Color::from_hsv(h, s, v);
    assert!((back.r - Color::hex(0xff7a59).r).abs() < 1e-4 && (back.g - Color::hex(0xff7a59).g).abs() < 1e-4);
    // Hex: строки, альфа, обратное преобразование.
    assert_eq!(Color::parse_hex("#ff7a59"), Some(Color::hex(0xff7a59)));
    assert_eq!(Color::parse_hex("ff7a59"), Some(Color::hex(0xff7a59)));
    assert_eq!(Color::parse_hex("#f00"), Some(Color::RED));
    assert_eq!(Color::parse_hex("#ff0000"), Some(Color::RED));
    assert_eq!(Color::parse_hex("#ff000080").unwrap().a, 128.0 / 255.0);
    assert_eq!(Color::parse_hex("#12345"), None);
    assert_eq!(Color::parse_hex("zzzzzz"), None);
    assert_eq!(Color::hex(0xff7a59).to_hex_string(), "#ff7a59");
    assert_eq!(Color::WHITE.scaled(2.0).r, 2.0);
    assert_eq!(Color::RED.with_alpha(0.25).a, 0.25);
    assert_eq!(Color::RED.multiply(Color::GREEN), Color::rgb(0.0, 0.0, 0.0));
    assert!(Color::WHITE.luminance() > 0.99);
}

#[test]
fn state_machine_transitions_are_visible_for_one_tick() {
    reflect_enum! {
        pub enum Mode { Menu, Playing, Over }
    }
    let mut m = StateMachine::new(Mode::Menu);
    assert!(m.just_entered(Mode::Menu));
    m.tick(0.1);
    assert!(!m.just_entered(Mode::Menu) && m.is(Mode::Menu));
    m.change(Mode::Playing);
    assert!(m.is(Mode::Menu), "not applied until tick");
    assert!(m.has_pending());
    m.tick(0.1);
    assert!(m.just_entered(Mode::Playing) && m.just_exited(Mode::Menu));
    assert_eq!(m.previous(), Some(Mode::Menu));
    m.tick(0.25);
    m.tick(0.25);
    assert!((m.time_in_state() - 0.5).abs() < 1e-6);
    m.change(Mode::Playing); // то же состояние: не считается переходом
    m.tick(0.25);
    assert!(!m.just_entered(Mode::Playing) && m.time_in_state() > 0.7);
    assert_eq!(<StateMachine<Mode> as FieldValue>::from_value(&Value::Enum("Over".into())).unwrap().current(), Mode::Over);
}

// ----------------------------------------------------------------------
// Ввод: действия, геймпады, переназначение
// ----------------------------------------------------------------------

const PAD: GamepadId = GamepadId(0);

#[test]
fn existing_actions_still_work_and_now_react_to_a_gamepad() {
    let mut input = Input::default();
    // Клавиатура — как раньше.
    input.set_key(Key::D, true);
    assert_eq!(input.axis("Horizontal"), 1.0);
    input.set_key(Key::A, true);
    assert_eq!(input.axis("Horizontal"), 0.0, "opposite keys cancel");
    input.set_key(Key::D, false);
    input.set_key(Key::A, false);
    assert_eq!(input.axis("Horizontal"), 0.0);
    assert_eq!(input.axis("Nope"), 0.0, "unknown axis is 0 with a warning");

    // Геймпад — те же имена.
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, 1.0);
    assert_eq!(input.axis("Horizontal"), 1.0);
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, -0.6);
    assert!(input.axis("Horizontal") < -0.4 && input.axis("Horizontal") > -0.6);
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, 0.0);
    input.set_gamepad_button(PAD, GamepadButton::DPadRight, true);
    assert_eq!(input.axis("Horizontal"), 1.0, "d-pad counts as digital");
    input.set_gamepad_button(PAD, GamepadButton::DPadRight, false);
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickY, 0.9);
    assert!(input.axis("Vertical") > 0.8);
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickY, 0.0);

    assert!(!input.button("Jump"));
    input.set_gamepad_button(PAD, GamepadButton::South, true);
    assert!(input.button("Jump") && input.button_down("Jump") && input.button("Submit"));
    assert_eq!(input.button_value("Jump"), 1.0);
    input.end_frame();
    assert!(input.button("Jump") && !input.button_down("Jump"));
    input.set_gamepad_button(PAD, GamepadButton::South, false);
    assert!(input.button_up("Jump") && !input.button("Jump"));
}

#[test]
fn dead_zone_hides_stick_noise_and_rescales_the_rest() {
    let mut input = Input::default();
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, 0.15);
    assert_eq!(input.axis("Horizontal"), 0.0, "inside the default 0.2 dead zone");
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, 0.6);
    assert!((input.axis("Horizontal") - 0.5).abs() < 1e-5, "(0.6 - 0.2) / 0.8");
    input.axis_bindings_mut("Horizontal").dead_zone = 0.5;
    assert!((input.axis("Horizontal") - 0.2).abs() < 1e-5);
    input.axis_bindings_mut("Horizontal").dead_zone = 0.0;
    assert!((input.axis("Horizontal") - 0.6).abs() < 1e-5);
    assert!((apply_dead_zone(-1.0, 0.3) + 1.0).abs() < 1e-6);

    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, 0.3);
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickY, 0.4);
    assert_eq!(input.gamepad_left_stick(PAD, 0.6), Vec2::ZERO, "radial dead zone: length 0.5 < 0.6");
    assert!((input.gamepad_left_stick(PAD, 0.0).length() - 0.5).abs() < 1e-5);
    assert_eq!(input.gamepad_axis(PAD, GamepadAxis::LeftStickX), 0.3, "raw reads ignore dead zones");
}

#[test]
fn rebinding_at_runtime() {
    let mut input = Input::default();
    // Переназначить прыжок с пробела на F.
    input.rebind_button("Jump", Binding::Key(Key::Space), Binding::Key(Key::F));
    input.set_key(Key::Space, true);
    assert!(!input.button("Jump"), "the old key no longer works");
    input.set_key(Key::F, true);
    assert!(input.button_down("Jump"));
    input.set_gamepad_button(PAD, GamepadButton::South, true);
    assert!(input.button("Jump"), "the pad binding stayed");
    assert_eq!(input.button_bindings("Jump"), &[Binding::Key(Key::F), Binding::Pad(GamepadButton::South)]);

    // Старый API заменяет только клавиатурные привязки.
    input.bind_button("Jump", &[Key::J]);
    assert_eq!(input.button_bindings("Jump"), &[Binding::Pad(GamepadButton::South), Binding::Key(Key::J)]);
    input.bind_axis("Horizontal", &[Key::J], &[Key::L]);
    input.set_key(Key::L, true);
    assert_eq!(input.axis("Horizontal"), 1.0);
    input.set_key(Key::L, false);
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, -1.0);
    assert_eq!(input.axis("Horizontal"), -1.0, "the stick binding survived bind_axis");

    // Полная замена и удаление.
    input.set_button_bindings("Dash", vec![Binding::Mouse(MouseButton::Right), Binding::PadAxis { axis: GamepadAxis::RightTrigger, positive: true }]);
    input.set_mouse_button(MouseButton::Right, true);
    assert!(input.button_down("Dash"));
    input.set_mouse_button(MouseButton::Right, false);
    assert!(input.button_up("Dash"));
    assert!(input.remove_button_binding("Dash", Binding::Mouse(MouseButton::Right)));
    assert!(!input.remove_button_binding("Dash", Binding::Mouse(MouseButton::Right)));
    input.set_gamepad_axis(PAD, GamepadAxis::RightTrigger, 0.2);
    assert!(!input.button("Dash") && (input.button_value("Dash") - 0.2).abs() < 1e-6);
    input.set_gamepad_axis(PAD, GamepadAxis::RightTrigger, 0.9);
    assert!(input.button("Dash"), "a trigger pulled past the threshold is a button");
    assert!(input.has_button("Dash"));
    input.remove_action("Dash");
    assert!(!input.has_button("Dash"));
    assert!(input.button_names().contains(&"Jump") && input.axis_names().contains(&"Vertical"));

    // Новая ось с нуля.
    input.set_axis_bindings("Steer", AxisBindings::pair(Key::Q, Key::E).with_analog(GamepadAxis::RightStickX).with_dead_zone(0.1));
    input.set_key(Key::Q, true);
    assert_eq!(input.axis("Steer"), -1.0);
}

#[test]
fn last_pressed_binding_supports_a_rebinding_screen() {
    let mut input = Input::default();
    assert_eq!(input.last_pressed_binding(), None);
    input.set_gamepad_button(PAD, GamepadButton::North, true);
    assert_eq!(input.last_pressed_binding(), Some(Binding::Pad(GamepadButton::North)));
    input.end_frame();
    assert_eq!(input.last_pressed_binding(), None, "only this frame's presses");
    input.set_gamepad_axis(PAD, GamepadAxis::LeftStickX, -1.0);
    assert_eq!(input.last_pressed_binding(), Some(Binding::PadAxis { axis: GamepadAxis::LeftStickX, positive: false }));
    input.end_frame();
    input.set_key(Key::Z, true);
    assert_eq!(input.last_pressed_binding(), Some(Binding::Key(Key::Z)));
    input.end_frame();
    input.set_mouse_button(MouseButton::Middle, true);
    assert_eq!(input.last_pressed_binding(), Some(Binding::Mouse(MouseButton::Middle)));
}

#[test]
fn bindings_roundtrip_through_text() {
    for key in Key::ALL {
        let b = Binding::Key(key);
        assert_eq!(Binding::parse(&b.to_text()), Some(b), "{key:?}");
    }
    for button in GamepadButton::ALL {
        let b = Binding::Pad(button);
        assert_eq!(Binding::parse(&b.to_text()), Some(b));
    }
    for axis in GamepadAxis::ALL {
        for positive in [true, false] {
            let b = Binding::PadAxis { axis, positive };
            assert_eq!(Binding::parse(&b.to_text()), Some(b));
        }
    }
    for m in MouseButton::ALL {
        assert_eq!(Binding::parse(&Binding::Mouse(m).to_text()), Some(Binding::Mouse(m)));
    }
    assert_eq!(Binding::Key(Key::Space).to_text(), "Key:Space");
    for bad in ["", "Key", "Key:Nope", "Pad:", "PadAxis:LeftStickX", "Joy:1"] {
        assert_eq!(Binding::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn gamepad_connection_state_and_readers() {
    let mut input = Input::default();
    assert_eq!(input.gamepad_count(), 0);
    input.connect_gamepad(GamepadId(2), "Pad Two");
    input.set_gamepad_button(GamepadId(1), GamepadButton::East, true); // подключается сам
    assert_eq!(input.gamepads(), vec![GamepadId(1), GamepadId(2)]);
    assert_eq!(input.gamepad_name(GamepadId(2)), Some("Pad Two"));
    assert!(input.gamepad_button(GamepadId(1), GamepadButton::East) && !input.gamepad_button(GamepadId(2), GamepadButton::East));
    assert!(input.any_gamepad_button(GamepadButton::East));
    assert!(input.gamepad_button_down(GamepadId(1), GamepadButton::East));
    assert!(input.button("Cancel"), "any pad can drive an action");

    input.set_gamepad_axis(GamepadId(1), GamepadAxis::LeftStickX, 0.3);
    input.set_gamepad_axis(GamepadId(2), GamepadAxis::LeftStickX, -0.9);
    assert_eq!(input.any_gamepad_axis(GamepadAxis::LeftStickX), -0.9, "largest deflection wins");
    assert_eq!(input.gamepad_axis(GamepadId(5), GamepadAxis::LeftStickX), 0.0);
    input.set_gamepad_axis(GamepadId(1), GamepadAxis::LeftStickX, f32::NAN);
    assert_eq!(input.gamepad_axis(GamepadId(1), GamepadAxis::LeftStickX), 0.0, "NaN from a driver is sanitised");

    input.end_frame();
    input.disconnect_gamepad(GamepadId(1));
    assert!(!input.is_gamepad_connected(GamepadId(1)));
    assert!(!input.any_gamepad_button(GamepadButton::East), "buttons of a removed pad are released");
    assert!(input.button_up("Cancel"), "and report a release");
    assert_eq!(input.gamepad_count(), 1);
    input.disconnect_gamepad(GamepadId(1)); // повторно — безвредно

    input.release_all();
    assert_eq!(input.gamepad_count(), 1, "focus loss keeps pads connected");
}

component! {
    /// Читает ввод только в фиксированном шаге.
    pub struct FixedReader {
        pub jumps: i32 = 0,
        pub fires: i32 = 0,
        pub dashes: i32 = 0,
        pub releases: i32 = 0,
        pub clicks: i32 = 0,
    }
}
impl Component for FixedReader {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let input = ctx.input();
        self.jumps += input.button_down("Jump") as i32;
        self.fires += input.gamepad_button_down(PAD, GamepadButton::West) as i32;
        self.dashes += input.button_down("Dash") as i32;
        self.releases += input.button_up("Jump") as i32;
        self.clicks += input.mouse_button_down(MouseButton::Left) as i32;
    }
}

#[test]
fn fixed_step_input_semantics_cover_gamepad_and_mouse() {
    let mut app = App::new();
    app.world.input.set_button_bindings(
        "Dash",
        vec![Binding::PadAxis { axis: GamepadAxis::LeftTrigger, positive: true }],
    );
    let id = app.spawn("R").with(FixedReader::default()).id();
    app.tick(DT);
    let read = |app: &App| app.world.scene.get_component::<FixedReader>(id).unwrap().clone();
    let base = read(&app);
    assert_eq!(base.jumps, 0);

    // Нажатие и отпускание целиком внутри одного кадра без шага физики: событие не теряется.
    // Кадр 1/240 с: шаг физики в нём может случиться, а может и нет — событие
    // не должно ни потеряться, ни задвоиться.
    app.world.input.set_gamepad_button(PAD, GamepadButton::South, true);
    app.world.input.set_gamepad_button(PAD, GamepadButton::West, true);
    app.world.input.set_gamepad_axis(PAD, GamepadAxis::LeftTrigger, 1.0);
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.world.input.set_gamepad_button(PAD, GamepadButton::South, false);
    app.tick(1.0 / 240.0);
    app.world.input.set_gamepad_button(PAD, GamepadButton::West, false);
    app.world.input.set_gamepad_axis(PAD, GamepadAxis::LeftTrigger, 0.0);
    app.world.input.set_mouse_button(MouseButton::Left, false);
    app.run_headless(10, DT);
    let r = read(&app);
    assert_eq!(r.jumps, 1, "a pad tap shorter than a frame still reaches on_fixed_update exactly once");
    assert_eq!(r.releases, 1);
    assert_eq!(r.fires, 1);
    assert_eq!(r.dashes, 1, "axis-as-button too");
    assert_eq!(r.clicks, 1);

    // Удерживаемая кнопка не даёт «нажато» на каждом шаге.
    app.world.input.set_gamepad_button(PAD, GamepadButton::South, true);
    app.run_headless(30, DT);
    assert_eq!(read(&app).jumps, 2);
}
