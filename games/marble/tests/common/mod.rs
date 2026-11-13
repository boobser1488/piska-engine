//! Общие помощники тестов: сборка игры без звука и окна, выбор цеха, бот-«игрок».
#![allow(dead_code)]

use engine::prelude::*;
use marble::kit::{WpKind, Waypoint};
use marble::{Action, Boot, GameState, Screen, ViewState};

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

/// Открыть все цеха, запустить `index` и дождаться конца отсчёта.
pub fn start_course(app: &mut App, index: usize) {
    state_mut(app).progress.unlocked = marble::courses::COUNT;
    if state(app).screen != Screen::Select {
        // Из меню, паузы и результатов путь в выбор цеха разный; тесты начинают с меню.
        act(app, Action::SelectScreen);
    }
    act(app, Action::Course(index));
    assert_eq!(state(app).screen, Screen::Countdown, "the course starts with a countdown");
    // Отсчёт 2.4 с.
    app.run_headless(150, DT);
    assert_eq!(state(app).screen, Screen::Playing, "the countdown ended");
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
        let route: Vec<Waypoint> = match &state(app).built {
            Some(built) => built.route.clone(),
            None => return,
        };
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
        let mut target = route[self.next.min(route.len() - 1)].pos;
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
            let d = Vec3::new(w.pos.x - pos.x, 0.0, w.pos.z - pos.z).length();
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
