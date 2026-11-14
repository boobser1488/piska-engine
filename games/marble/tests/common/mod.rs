//! Общие помощники тестов: сборка игры без звука и окна, выбор цеха, бот-«игрок».
#![allow(dead_code)]

use engine::prelude::*;
use marble::{Action, Boot, GameState, Screen, ViewState, Waypoint, WpKind};

/// Шаг тестов: как 60 кадров в секунду.
pub const DT: f32 = 1.0 / 60.0;

/// Игра с сохранениями в памяти и без звуковой карты.
pub fn game() -> App {
    game_with(SaveData::in_memory())
}

pub fn game_with(save: SaveData) -> App {
    std::env::set_var("ENGINE_AUDIO", "off");
    let mut app = marble::build_with(save, Boot::default()).expect("the game builds from its assets directory");
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app.run_headless(2, DT);
    app
}

pub fn state(app: &App) -> &GameState {
    app.world.resources.get::<GameState>().expect("GameState")
}

pub fn state_mut(app: &mut App) -> &mut GameState {
    app.world.resources.get_mut::<GameState>().expect("GameState")
}

/// Поставить действие в очередь и прокрутить кадр.
pub fn act(app: &mut App, action: Action) {
    state_mut(app).push(action);
    app.run_headless(1, DT);
}

/// Прокрутить кадры, пока игра не окажется на нужном экране (сцены меняются в конце кадра).
pub fn run_until(app: &mut App, screen: Screen, max_frames: u32) {
    for _ in 0..max_frames {
        if state(app).screen == screen && !state(app).loading {
            return;
        }
        app.run_headless(1, DT);
    }
    panic!("the game never reached {screen:?}, it is on {:?}", state(app).screen);
}

/// Открыть все цеха, запустить `index` из меню и дождаться конца отсчёта.
pub fn start_course(app: &mut App, index: usize) {
    state_mut(app).progress.unlocked = marble::COUNT;
    if state(app).screen != Screen::Menu && state(app).screen != Screen::Select {
        to_menu(app);
    }
    if state(app).screen != Screen::Select {
        act(app, Action::SelectScreen);
    }
    act(app, Action::Course(index));
    run_until(app, Screen::Countdown, 20);
    assert_eq!(state(app).course, index, "the scene of course {} is loaded", index + 1);
    // Отсчёт 2.4 с.
    app.run_headless(150, DT);
    assert_eq!(state(app).screen, Screen::Playing, "the countdown ended");
}

/// Из заезда в главное меню (через паузу).
pub fn to_menu(app: &mut App) {
    if state(app).screen == Screen::Playing || state(app).screen == Screen::Countdown {
        act(app, Action::Pause);
    }
    act(app, Action::ToMenu);
    run_until(app, Screen::Menu, 20);
}

pub fn ball_id(app: &App) -> GameObjectId {
    state(app).ball.expect("a ball exists during a course")
}

pub fn ball_pos(app: &App) -> Vec3 {
    app.world.scene.world_position(ball_id(app))
}

/// Телепортировать шар (скорости сбрасываются).
pub fn teleport_ball(app: &mut App, pos: Vec3) {
    let id = ball_id(app);
    let object = app.world.scene.get_mut(id).expect("ball");
    object.transform.position = pos;
    if let Some(body) = object.get_mut::<Rigidbody>() {
        body.velocity = Vec3::ZERO;
        body.angular_velocity = Vec3::ZERO;
    }
}

/// Сколько объектов сцены с таким именем.
pub fn count_named(app: &App, name: &str) -> usize {
    app.world.scene.ids().into_iter().filter(|id| app.world.scene.get(*id).is_some_and(|o| o.name == name)).count()
}

pub fn find(app: &App, name: &str) -> GameObjectId {
    app.world.scene.find_by_name(name).unwrap_or_else(|| panic!("no object `{name}` in the scene"))
}

/// Бот: ведёт шар по маршруту цеха левым стиком и кнопкой прыжка.
pub struct Bot {
    next: usize,
    jump_frames: u32,
    pub stuck_frames: u32,
    last: Vec3,
}

impl Bot {
    pub fn new() -> Self {
        Self { next: 0, jump_frames: 0, stuck_frames: 0, last: Vec3::ZERO }
    }

    pub fn reset_route(&mut self) {
        self.next = 0;
    }

    pub fn route_index(&self) -> usize {
        self.next
    }

    /// Выставить ввод на ближайший кадр.
    pub fn drive(&mut self, app: &mut App) {
        let pad = GamepadId(0);
        let route: Vec<Waypoint> = state(app).route.clone();
        if route.is_empty() || state(app).screen != Screen::Playing {
            app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, 0.0);
            app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickY, 0.0);
            return;
        }
        let pos = ball_pos(app);
        let yaw = app.world.resources.get::<ViewState>().map_or(0.0, |v| v.yaw);

        // После возврата на контрольную точку маршрут ищется заново: ближайшая точка впереди.
        if (pos - self.last).length() > 6.0 && self.last != Vec3::ZERO {
            self.next = self.nearest(&route, pos);
        }
        self.last = pos;

        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
        let mut jump = false;
        let mut hold = false;
        let mut target = route[self.next.min(route.len() - 1)].position();
        let mut kind = route[self.next.min(route.len() - 1)].kind;

        // Стоим на платформе: центрируемся на ней, пока рядом не окажется следующая точка.
        let movers = app.world.scene.find_all_by_tag("Mover");
        let mut riding = None;
        for mover in movers {
            let p = app.world.scene.world_position(mover);
            if flat(p - pos).length() < 3.2 && (pos.y - p.y) > 0.2 && (pos.y - p.y) < 1.6 {
                riding = Some(p);
            }
        }
        if let Some(platform) = riding {
            if flat(target - platform).length() > 3.6 {
                target = platform;
                kind = WpKind::Go;
            }
        }

        if let WpKind::WaitMover(at) = kind {
            let at = Vec3::from_array(at);
            let arrived = app
                .world
                .scene
                .find_all_by_tag("Mover")
                .into_iter()
                .any(|m| flat(app.world.scene.world_position(m) - at).length() < 1.8);
            if arrived {
                self.next += 1;
            } else {
                hold = true;
            }
        }

        let delta = flat(target - pos);
        let distance = delta.length();
        let reach = match kind {
            WpKind::Jump => 1.4,
            _ => 1.9,
        };
        if riding.is_none() && distance < reach && !matches!(kind, WpKind::WaitMover(_)) {
            if kind == WpKind::Jump {
                jump = true;
            }
            self.next = (self.next + 1).min(route.len() - 1);
        }

        let velocity = app.world.scene.get_component::<Rigidbody>(ball_id(app)).map_or(Vec3::ZERO, |b| flat(b.velocity));
        let wish = if hold {
            // Стоим: давим против скорости, пока шар не остановится.
            -velocity / velocity.length().max(1.0)
        } else if distance < 0.05 {
            Vec3::ZERO
        } else {
            delta / distance.max(1.0)
        };
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        let stick = Vec2::new(wish.dot(right), wish.dot(forward)).clamp_length_max(1.0);
        app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, stick.x);
        app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickY, stick.y);
        if jump {
            self.jump_frames = 3;
        }
        let pressed = self.jump_frames > 0;
        self.jump_frames = self.jump_frames.saturating_sub(1);
        app.world.input.set_gamepad_button(pad, GamepadButton::South, pressed);
    }

    fn nearest(&self, route: &[Waypoint], pos: Vec3) -> usize {
        let mut best = 0;
        let mut best_d = f32::MAX;
        for (i, w) in route.iter().enumerate() {
            let d = Vec3::new(w.pos[0] - pos.x, 0.0, w.pos[2] - pos.z).length();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        (best + 1).min(route.len() - 1)
    }
}

/// Играть цех ботом, пока не покажутся результаты или не кончатся кадры.
pub fn play_with_bot(app: &mut App, max_frames: u32) -> u32 {
    let mut bot = Bot::new();
    for frame in 0..max_frames {
        bot.drive(app);
        app.run_headless(1, DT);
        if state(app).screen == Screen::Results {
            return frame;
        }
    }
    max_frames
}

/// Экземпляр префаба `prefabs/<rel>.prefab` в точке мира.
pub fn spawn_prefab(app: &mut App, rel: &str, pos: Vec3) -> GameObjectId {
    app.world
        .instantiate_at(&format!("prefabs/{rel}.prefab"), pos, Quat::IDENTITY, None)
        .unwrap_or_else(|e| panic!("prefab {rel}: {e}"))
}

/// Блок-плита из префаба с размером `size` (пол для опытов).
pub fn spawn_block(app: &mut App, center: Vec3, size: Vec3) -> GameObjectId {
    let id = spawn_prefab(app, "track/plate_block", center);
    app.world.scene.get_mut(id).unwrap().transform.scale = size;
    id
}

/// Пустой мир для опытов: меню без витрины (группы объектов цеха убраны).
pub fn lab() -> App {
    let mut app = game();
    for name in ["Track", "Mechanisms", "Pickups", "Lamps", "Decor"] {
        if let Some(group) = app.world.scene.find_by_name(name) {
            app.world.scene.destroy(group);
        }
    }
    app.run_headless(2, DT);
    app
}

/// Шар-пробник: префаб шара (замороженный, без управления), тег «Ball» на месте.
pub fn probe_ball(app: &mut App, pos: Vec3) -> GameObjectId {
    spawn_prefab(app, "ball", pos)
}
