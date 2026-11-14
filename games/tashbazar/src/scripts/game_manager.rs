use engine::prelude::*;

use crate::game::options::Options;
use crate::game::{format_time, sounds, CourseFinished};
use crate::scripts::hud::Hud;
use crate::scripts::movement_course::MovementCourse;
use crate::scripts::spawn_point::{spawns, Team};

reflect_enum! {
    /// Какой экран открыт.
    pub enum Screen { Menu, Options, Playing, Paused, Results }
}

reflect_enum! {
    /// Что делает игрок: гуляет по карте или проходит полосу препятствий на время.
    pub enum PlayMode { FreeRun, Course }
}

component! {
    /// Менеджер игры (объект «Game» в каждой сцене): главное меню, настройки, пауза,
    /// режимы «прогулка» и «полоса препятствий», захват курсора. Экраны — экземпляры
    /// `ui/*.prefab`, игрок, камера меню и полоса — объекты сцены; всё назначено полями.
    ///
    /// Клавиши в игре: Esc — пауза (курсор отпускается), R — назад на точку появления /
    /// последнюю отметку, V — полёт сквозь стены (отладка).
    pub struct GameManager {
        pub player: ObjectRef = ObjectRef::NONE,
        /// Камера облёта для меню (выключается в игре).
        pub menu_camera: ObjectRef = ObjectRef::NONE,
        pub course: ObjectRef = ObjectRef::NONE,
        pub main_menu: ObjectRef = ObjectRef::NONE,
        pub options_menu: ObjectRef = ObjectRef::NONE,
        pub pause_menu: ObjectRef = ObjectRef::NONE,
        pub results: ObjectRef = ObjectRef::NONE,
        pub hud: ObjectRef = ObjectRef::NONE,
        /// Сразу в игру, без меню (сцена-полигон).
        pub start_playing: bool = false,
        /// За какую сторону появляться в прогулке.
        pub team: Team = Team::Attackers,
        /// Сцена главного меню (кнопка «В меню» на полигоне). Пусто — меню в этой сцене.
        pub menu_scene: String = String::new(),
        /// Сцена полигона (кнопка «Полигон»).
        pub gym_scene: String = "gym".to_string(),
        /// Открытый экран (во время игры).
        pub screen: Screen = Screen::Menu,
        /// Режим (во время игры).
        pub mode: PlayMode = PlayMode::FreeRun,
    }
}

/// Состояние менеджера, которому не место в инспекторе.
#[derive(Default)]
struct Session {
    options: Options,
    /// С какого экрана открыли настройки (туда вернёт «Назад»).
    options_from: Option<Screen>,
    /// Курсор был захвачен в этой игре: его потеря (Alt+Tab) ставит паузу.
    had_capture: bool,
    /// Следующая точка появления в прогулке.
    next_spawn: usize,
}

fn find(scene: &Scene, root: Option<GameObjectId>, name: &str) -> Option<GameObjectId> {
    let root = root?;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let object = scene.get(id)?;
        if object.name == name {
            return Some(id);
        }
        stack.extend(object.children().iter().copied());
    }
    None
}

fn set_text(scene: &mut Scene, id: Option<GameObjectId>, text: &str) {
    if let Some(t) = id.and_then(|id| scene.get_component_mut::<Text>(id)) {
        t.set(text);
    }
}

impl GameManager {
    fn session<'a>(ctx: &'a mut Ctx) -> &'a mut Session {
        ctx.resource_or_default::<Session>()
    }

    fn show(&self, ctx: &mut Ctx, screen: Screen) {
        let screens = [
            (self.main_menu, Screen::Menu),
            (self.options_menu, Screen::Options),
            (self.pause_menu, Screen::Paused),
            (self.results, Screen::Results),
        ];
        for (reference, which) in screens {
            if let Some(id) = ctx.resolve(reference) {
                ctx.scene_mut().ui_set_visible(id, which == screen);
            }
        }
        let playing = matches!(screen, Screen::Playing | Screen::Paused)
            || (screen == Screen::Options && Self::session(ctx).options_from == Some(Screen::Paused))
            || screen == Screen::Results;
        if let Some(hud) = ctx.resolve(self.hud) {
            ctx.scene_mut().ui_set_visible(hud, playing);
        }
        if let Some(player) = ctx.resolve(self.player) {
            if ctx.is_active(player) != playing {
                ctx.set_active(player, playing);
            }
        }
        if let Some(camera) = ctx.resolve(self.menu_camera) {
            ctx.set_active(camera, !playing);
        }
        if let Some(player) = ctx.resolve(self.player) {
            if let Some(controller) = ctx.get_on_mut::<FpsController>(player) {
                controller.input_enabled = screen == Screen::Playing;
            }
        }
        ctx.world.input.set_cursor_captured(screen == Screen::Playing);
        ctx.set_time_scale(if matches!(screen, Screen::Paused) || (screen == Screen::Options && playing) { 0.0 } else { 1.0 });
    }

    fn go(&mut self, ctx: &mut Ctx, screen: Screen) {
        self.screen = screen;
        self.show(ctx, screen);
        if screen == Screen::Options {
            self.sync_options_ui(ctx);
        }
        if screen == Screen::Menu {
            self.refresh_menu(ctx);
        }
    }

    /// Начать игру в режиме `mode`.
    fn begin(&mut self, ctx: &mut Ctx, mode: PlayMode) {
        self.mode = mode;
        let course = ctx.resolve(self.course);
        if mode == PlayMode::Course && course.is_none() {
            log::warn!("this scene has no movement course");
            self.mode = PlayMode::FreeRun;
        }
        self.go(ctx, Screen::Playing);
        Self::session(ctx).had_capture = false;
        match self.mode {
            PlayMode::Course => {
                let Some(course) = course else { return };
                if let Some(mut c) = ctx.get_on::<MovementCourse>(course).cloned() {
                    c.arm(ctx.scene_mut());
                    if let Some(slot) = ctx.get_on_mut::<MovementCourse>(course) {
                        *slot = c;
                    }
                }
            }
            PlayMode::FreeRun => {
                self.stop_course(ctx);
                self.respawn(ctx);
            }
        }
        self.apply_options(ctx);
    }

    fn stop_course(&self, ctx: &mut Ctx) {
        if let Some(course) = ctx.resolve(self.course) {
            if let Some(c) = ctx.get_on_mut::<MovementCourse>(course) {
                c.stop();
            }
        }
    }

    /// Поставить игрока на следующую точку появления своей стороны.
    fn respawn(&self, ctx: &mut Ctx) {
        let points = spawns(ctx.scene(), self.team);
        let Some(player) = ctx.resolve(self.player) else { return };
        if points.is_empty() {
            log::warn!("no SpawnPoint for {:?} in this scene", self.team);
            return;
        }
        let index = {
            let session = Self::session(ctx);
            let i = session.next_spawn % points.len();
            session.next_spawn += 1;
            i
        };
        let (_, position, yaw) = points[index];
        if let Some(object) = ctx.scene_mut().get_mut(player) {
            object.transform.position = position + Vec3::Y * 0.01;
        }
        if let Some(controller) = ctx.get_on_mut::<FpsController>(player) {
            controller.velocity = Vec3::ZERO;
            controller.yaw_deg = yaw;
            controller.pitch_deg = 0.0;
            controller.ducked = false;
            controller.noclip = false;
        }
    }

    fn apply_options(&self, ctx: &mut Ctx) {
        let options = Self::session(ctx).options;
        if let Some(player) = ctx.resolve(self.player) {
            if let Some(controller) = ctx.get_on_mut::<FpsController>(player) {
                options.apply(controller);
            }
        }
        if let Some(hud) = ctx.resolve(self.hud) {
            if let Some(h) = ctx.get_on_mut::<Hud>(hud) {
                h.show_speed = options.show_speed;
            }
        }
        if let Some(audio) = ctx.world.resources.get_mut::<Audio>() {
            audio.set_bus_volume(Bus::Master, options.volume);
        }
    }

    /// Ползунки и флажки экрана настроек — по текущим настройкам.
    fn sync_options_ui(&self, ctx: &mut Ctx) {
        let options = Self::session(ctx).options;
        let root = ctx.resolve(self.options_menu);
        let scene = ctx.scene_mut();
        let slider = |scene: &mut Scene, name: &str, value: f32| {
            if let Some(s) = find(scene, root, name).and_then(|id| scene.get_component_mut::<Slider>(id)) {
                s.value = value;
            }
        };
        slider(scene, "SensitivitySlider", options.sensitivity);
        slider(scene, "FovSlider", options.fov);
        slider(scene, "VolumeSlider", options.volume * 100.0);
        for (name, on) in [("AutoHopToggle", options.auto_hop), ("SpeedToggle", options.show_speed), ("InvertToggle", options.invert_y)] {
            if let Some(t) = find(scene, root, name).and_then(|id| scene.get_component_mut::<Toggle>(id)) {
                t.is_on = on;
            }
        }
        self.option_labels(ctx);
    }

    fn option_labels(&self, ctx: &mut Ctx) {
        let options = Self::session(ctx).options;
        let root = ctx.resolve(self.options_menu);
        let scene = ctx.scene_mut();
        let id = find(scene, root, "SensitivityValue");
        set_text(scene, id, &format!("{:.2}", options.sensitivity));
        let id = find(scene, root, "FovValue");
        set_text(scene, id, &format!("{:.0}°", options.fov));
        let id = find(scene, root, "VolumeValue");
        set_text(scene, id, &format!("{:.0} %", options.volume * 100.0));
    }

    fn refresh_menu(&self, ctx: &mut Ctx) {
        let best = ctx
            .resolve(self.course)
            .and_then(|id| ctx.get_on::<MovementCourse>(id).cloned())
            .map(|c| c.best(ctx.save_data()))
            .unwrap_or(0.0);
        let root = ctx.resolve(self.main_menu);
        let id = find(ctx.scene(), root, "BestTime");
        let text = if best > 0.0 { format!("Лучшее время полосы: {}", format_time(best)) } else { "Полоса препятствий ещё не пройдена".to_string() };
        set_text(ctx.scene_mut(), id, &text);
    }

    fn show_results(&mut self, ctx: &mut Ctx, finished: CourseFinished) {
        self.go(ctx, Screen::Results);
        let root = ctx.resolve(self.results);
        let scene = ctx.scene_mut();
        let id = find(scene, root, "ResultTime");
        set_text(scene, id, &format_time(finished.time));
        let note = if finished.previous_best <= 0.0 {
            "Первый круг — это и есть рекорд.".to_string()
        } else if finished.time < finished.previous_best {
            format!("Новый рекорд! Было {}", format_time(finished.previous_best))
        } else {
            format!("Рекорд: {}", format_time(finished.previous_best))
        };
        let id = find(scene, root, "ResultNote");
        set_text(scene, id, &note);
    }

    fn leave_to_menu(&mut self, ctx: &mut Ctx) {
        self.stop_course(ctx);
        if self.menu_scene.is_empty() {
            self.go(ctx, Screen::Menu);
        } else {
            ctx.world.input.set_cursor_captured(false);
            ctx.set_time_scale(1.0);
            let scene = self.menu_scene.clone();
            ctx.load_scene(&scene);
        }
    }
}

impl Component for GameManager {
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::UPDATE | Hooks::MESSAGE
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        sounds::load(ctx.world);
        let options = Options::load(ctx.save_data());
        Self::session(ctx).options = options;
        Self::session(ctx).options_from = None;
        self.apply_options(ctx);
        if self.start_playing {
            self.begin(ctx, PlayMode::FreeRun);
        } else {
            self.go(ctx, Screen::Menu);
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let finished: Vec<CourseFinished> = ctx.events::<CourseFinished>().copied().collect();
        if let Some(done) = finished.last() {
            if self.screen == Screen::Playing {
                self.show_results(ctx, *done);
                return;
            }
        }
        let escape = ctx.input().key_down(Key::Escape) || ctx.input().any_gamepad_button_down(GamepadButton::Start);
        match self.screen {
            Screen::Playing => {
                let captured = ctx.input().is_cursor_captured();
                let session = Self::session(ctx);
                let lost = session.had_capture && !captured;
                session.had_capture |= captured;
                if escape || lost {
                    self.go(ctx, Screen::Paused);
                    return;
                }
                let (respawn, noclip) = (ctx.input().key_down(Key::R), ctx.input().key_down(Key::V));
                let player = ctx.resolve(self.player);
                if respawn {
                    match (self.mode, ctx.resolve(self.course)) {
                        (PlayMode::Course, Some(course)) => {
                            if let Some(c) = ctx.get_on::<MovementCourse>(course).cloned() {
                                let back = if c.armed { 0 } else { (c.next - 1).max(0) as usize };
                                c.return_player(ctx.scene_mut(), back);
                            }
                        }
                        _ => self.respawn(ctx),
                    }
                }
                if noclip {
                    if let Some(controller) = player.and_then(|p| ctx.get_on_mut::<FpsController>(p)) {
                        controller.noclip = !controller.noclip;
                        controller.velocity = Vec3::ZERO;
                    }
                }
            }
            Screen::Paused if escape => {
                sounds::click(ctx.world);
                self.go(ctx, Screen::Playing);
                Self::session(ctx).had_capture = false;
            }
            Screen::Options if escape => self.on_message(ctx, &Message { name: "back".into(), value: Value::Nil }),
            Screen::Results if escape => self.leave_to_menu(ctx),
            _ => {}
        }
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        let mut changed = false;
        match (message.name.as_str(), &message.value) {
            ("play", _) => {
                sounds::click(ctx.world);
                self.begin(ctx, PlayMode::FreeRun);
            }
            ("course" | "again", _) => {
                sounds::click(ctx.world);
                self.begin(ctx, PlayMode::Course);
            }
            ("gym", _) => {
                sounds::click(ctx.world);
                ctx.world.input.set_cursor_captured(false);
                let scene = self.gym_scene.clone();
                ctx.load_scene(&scene);
            }
            ("options", _) => {
                sounds::click(ctx.world);
                let from = self.screen;
                Self::session(ctx).options_from = Some(from);
                self.go(ctx, Screen::Options);
            }
            ("back", _) => {
                sounds::click(ctx.world);
                let back = Self::session(ctx).options_from.take().unwrap_or(Screen::Menu);
                self.go(ctx, back);
            }
            ("resume", _) => {
                sounds::click(ctx.world);
                self.go(ctx, Screen::Playing);
                Self::session(ctx).had_capture = false;
            }
            ("restart", _) => {
                sounds::click(ctx.world);
                let mode = self.mode;
                self.begin(ctx, mode);
            }
            ("menu", _) => {
                sounds::click(ctx.world);
                self.leave_to_menu(ctx);
            }
            ("quit", _) => ctx.quit(),
            ("sensitivity", Value::Float(v)) => {
                Self::session(ctx).options.sensitivity = v.clamp(Options::SENSITIVITY.0, Options::SENSITIVITY.1);
                changed = true;
            }
            ("fov", Value::Float(v)) => {
                Self::session(ctx).options.fov = v.round().clamp(Options::FOV.0, Options::FOV.1);
                changed = true;
            }
            ("volume", Value::Float(v)) => {
                Self::session(ctx).options.volume = (v / 100.0).clamp(0.0, 1.0);
                changed = true;
            }
            ("autohop", Value::Bool(on)) => {
                Self::session(ctx).options.auto_hop = *on;
                changed = true;
            }
            ("showspeed", Value::Bool(on)) => {
                Self::session(ctx).options.show_speed = *on;
                changed = true;
            }
            ("invert", Value::Bool(on)) => {
                Self::session(ctx).options.invert_y = *on;
                changed = true;
            }
            _ => {}
        }
        if changed {
            let options = Self::session(ctx).options;
            options.store(ctx.save_data());
            self.apply_options(ctx);
            self.option_labels(ctx);
        }
    }
}
