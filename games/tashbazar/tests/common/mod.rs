//! Общее для тестов: игра из ассетов, старт режима, бот, который ведёт игрока по маршруту
//! из `data/routes.ron` командами контроллера (как игрок мышью и клавишами).
#![allow(dead_code)]

pub mod check;

use std::cell::RefCell;
use std::rc::Rc;

use engine::fps::{FpsRuntime, MoveState, UserCmd};
use engine::prelude::*;
use tashbazar::scripts::game_manager::{GameManager, Screen};
use tashbazar::{Route, RouteTable, Step};

/// Шаг: классические 64 в секунду (как в `project.settings`).
pub const TICK: f32 = 1.0 / 64.0;

/// Каталог ассетов игры; `TASHBAZAR_CHECK_ASSETS=<каталог>` — проверить другую копию.
pub fn assets() -> &'static str {
    match std::env::var("TASHBAZAR_CHECK_ASSETS") {
        Ok(dir) => Box::leak(dir.into_boxed_str()),
        Err(_) => concat!(env!("CARGO_MANIFEST_DIR"), "/assets"),
    }
}

pub fn game() -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    tashbazar::build_in(SaveData::in_memory(), assets(), tashbazar::MAP_SCENE).expect("the game builds from its assets")
}

pub fn routes(app: &App) -> RouteTable {
    app.world.resources.get::<AssetDatabase>().unwrap().load_data::<RouteTable>("data/routes.ron").expect("data/routes.ron")
}

/// Нажать кнопку меню (`play`, `course`) и вернуть игрока.
pub fn press(app: &mut App, message: &str) -> GameObjectId {
    app.run_headless(3, TICK);
    let game = app.world.scene.find_by_name("Game").expect("Game");
    app.world.send_message(game, message, Value::Nil);
    app.run_headless(3, TICK);
    assert_eq!(app.world.scene.get_component::<GameManager>(game).unwrap().screen, Screen::Playing);
    app.world.scene.find_by_name("Player").expect("Player")
}

pub fn state(app: &App, player: GameObjectId) -> MoveState {
    *app.world.resources.get::<FpsRuntime>().and_then(|r| r.state(player)).expect("the controller runs")
}

/// Отчёт бота.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Сколько точек маршрута пройдено.
    pub reached: usize,
    /// Шаг, на котором взята последняя точка.
    pub finished_at: Option<u64>,
    /// Шаг старта.
    pub started_at: Option<u64>,
    /// Где и когда бот застрял (не сдвинулся на 0.3 м за секунду).
    pub stuck: Option<(u64, Vec3, usize)>,
    pub max_speed_ups: f32,
    pub jumps: usize,
}

fn yaw_towards(dx: f32, dz: f32) -> f32 {
    (-dx).atan2(-dz).to_degrees()
}

/// Повести игрока по маршруту. Отчёт пишется в `Rc<RefCell<Report>>`.
pub fn drive(app: &mut App, player: GameObjectId, route: &Route) -> Rc<RefCell<Report>> {
    let report: Rc<RefCell<Report>> = Rc::default();
    let out = report.clone();
    let points = route.points.clone();
    let mut index = 1usize; // точка 0 — старт
    let mut in_air_action: Option<Step> = None;
    let mut last_check = (0u64, Vec3::ZERO);
    let mut alternate = false;
    app.world.resources.get_or_insert_with(FpsRuntime::default).set_driver(player, move |tick, s| {
        let mut r = out.borrow_mut();
        if r.started_at.is_none() {
            r.started_at = Some(tick);
            last_check = (tick, s.origin);
        }
        r.max_speed_ups = r.max_speed_ups.max(s.units_per_second());
        if index >= points.len() {
            return UserCmd { yaw_deg: 0.0, ..UserCmd::default() };
        }
        // Застрял?
        if tick - last_check.0 >= 64 {
            let moved = Vec2::new(s.origin.x - last_check.1.x, s.origin.z - last_check.1.z).length();
            if moved < 0.3 && r.stuck.is_none() {
                r.stuck = Some((tick, s.origin, index));
            }
            last_check = (tick, s.origin);
        }
        let target = points[index].position();
        let to = Vec2::new(target.x - s.origin.x, target.z - s.origin.z);
        let distance = to.length();
        // Точка взята: рядом или проскочили её вдоль отрезка.
        let prev = points[index - 1].position();
        let segment = Vec2::new(target.x - prev.x, target.z - prev.z);
        let passed = segment.dot(-to) > 0.0 && distance < 1.2;
        let radius = match points[index].step {
            Step::Run => 0.6,
            _ => 0.2,
        };
        let mut cmd = UserCmd { forward: 1.0, ..UserCmd::default() };
        let mut jump_now = false;
        if (distance < radius || passed) && s.on_ground || (points[index].step == Step::Run && (distance < radius || passed)) {
            match points[index].step {
                Step::Jump | Step::CrouchJump | Step::StrafeJump => {
                    jump_now = true;
                    in_air_action = Some(points[index].step);
                    r.jumps += 1;
                }
                _ => {}
            }
            index += 1;
            r.reached = index;
            if index >= points.len() {
                r.finished_at = Some(tick);
                return UserCmd::default();
            }
        }
        let target = points[index].position();
        let to = Vec2::new(target.x - s.origin.x, target.z - s.origin.z);
        cmd.yaw_deg = yaw_towards(to.x, to.y);
        if jump_now {
            cmd.jump = true;
            return cmd;
        }
        if s.on_ground {
            in_air_action = None;
        }
        match in_air_action {
            Some(Step::CrouchJump) => cmd.duck = true,
            Some(Step::StrafeJump) => {
                // Стрейф: желаемое направление поперёк скорости, в сторону цели; совпали
                // по курсу — чередуем стороны (зигзаг набирает скорость и держит линию).
                let v = Vec2::new(s.velocity.x, s.velocity.z).normalize_or_zero();
                let t = to.normalize_or_zero();
                let left = Vec2::new(-v.y, v.x);
                let right = Vec2::new(v.y, -v.x);
                let aligned = v.dot(t) > 0.9994;
                alternate = !alternate;
                let wish = if aligned {
                    if alternate { left } else { right }
                } else if left.dot(t) > right.dot(t) {
                    left
                } else {
                    right
                };
                // right(yaw) = (cos y, −sin y) = wish.
                cmd.yaw_deg = (-wish.y).atan2(wish.x).to_degrees();
                cmd.forward = 0.0;
                cmd.side = 1.0;
            }
            _ => {}
        }
        cmd
    });
    report
}

/// Поставить игрока в начало маршрута, лицом ко второй точке.
pub fn place_at_start(app: &mut App, player: GameObjectId, route: &Route) {
    let start = route.points[0].position();
    let next = route.points[1].position();
    app.world.scene.get_mut(player).unwrap().transform.position = start + Vec3::Y * 0.01;
    let c = app.world.scene.get_component_mut::<FpsController>(player).unwrap();
    c.velocity = Vec3::ZERO;
    c.yaw_deg = yaw_towards(next.x - start.x, next.z - start.z);
    app.run_headless(8, TICK);
}

/// Прогнать до конца маршрута или до `limit` секунд.
pub fn run_until_done(app: &mut App, report: &Rc<RefCell<Report>>, limit: f32) {
    let frames = (limit / TICK) as usize;
    for _ in 0..frames {
        app.tick(TICK);
        if report.borrow().finished_at.is_some() {
            break;
        }
    }
}
