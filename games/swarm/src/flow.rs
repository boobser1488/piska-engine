//! Ход игры: экраны, ввод, шаг симуляции, события → отображение, сохранения.
//!
//! Всё состояние лежит в одной структуре [`Game`] (ресурс мира); компонент
//! [`Director`] на единственном объекте сцены каждый кадр вынимает её, обновляет и
//! кладёт обратно — поэтому у `Game` есть честный `&mut World` без `RefCell`.

use engine::prelude::*;

use crate::bot;
use crate::debug::Debug;
use crate::profile::*;
use crate::sim::{Checkpoint, Event, Mode, Phase, PlayerInput, Sim};
use crate::sound;
use crate::ui::{Screen, Ui};
use crate::view::View;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    None,
    Menu,
    Game,
    Boss,
}

/// Состояние экранных стиков для касаний.
#[derive(Default)]
struct TouchState {
    seen: bool,
    left: Option<(u64, Vec2)>,
    right: Option<(u64, Vec2)>,
}

/// Радиус экранного стика в пикселях (в единицах холста).
const STICK_RADIUS: f32 = 70.0;
const BASE_ORTHO: f32 = 10.4;

pub struct Game {
    pub sim: Sim,
    pub screen: Screen,
    pub opts: Options,
    pub progress: Progress,
    pub view: View,
    pub ui: Ui,
    pub track: Track,
    options_from: Screen,
    /// Оставшиеся реальные секунды замедления (смерть босса).
    pub slowmo: f32,
    /// Через сколько реальных секунд показать результаты (забег закончен).
    pub results_delay: Option<f32>,
    pub result_won: bool,
    hover: Option<GameObjectId>,
    touch: TouchState,
    mouse_aim: bool,
    ortho: f32,
    clock: f32,
    pub debug: Debug,
    /// Последнее предложение улучшений и причина (для карточек).
    reason: String,
    /// Сколько раз игра показывала карточки (для тестов).
    pub upgrade_screens: u32,
    /// Сообщение «открыт бесконечный режим» уже показано в этом забеге.
    unlock_shown: bool,
    /// Игру только что сняли с паузы в этом кадре: та же клавиша не должна поставить её снова.
    resumed_frame: bool,
}

impl Game {
    pub fn new(ctx: &mut Ctx, debug: Debug) -> Game {
        let db = ctx.resource::<AssetDatabase>().expect("assets are mounted before the scene loads").clone();
        let tex = db.handle::<Texture>("sprites/atlas.png").expect("sprites/atlas.png is in assets");
        let seed = ctx.random().next_u64();
        let scene = ctx.scene_mut();
        let view = View::new(scene, tex, seed);
        let ui = Ui::build(scene, &db);
        let (opts, progress) = {
            let save = ctx.save_data();
            (Options::load(save), Progress::load(save))
        };
        let mut game = Game {
            sim: Sim::new(seed, Mode::Campaign),
            screen: Screen::Menu,
            opts,
            progress,
            view,
            ui,
            track: Track::None,
            options_from: Screen::Menu,
            slowmo: 0.0,
            results_delay: None,
            result_won: false,
            hover: None,
            touch: TouchState::default(),
            mouse_aim: false,
            ortho: BASE_ORTHO,
            clock: 0.0,
            debug,
            reason: String::new(),
            upgrade_screens: 0,
            unlock_shown: false,
            resumed_frame: false,
        };
        game.view.show_actors(ctx.scene_mut(), true);
        if let Some(audio) = ctx.resource_mut::<Audio>() {
            sound::apply_volumes(audio, &game.opts);
        }
        game.ui.refresh_options(ctx.scene_mut(), &game.opts);
        game.go_menu(ctx);
        game.apply_debug_screen(ctx);
        game
    }

    // -----------------------------------------------------------------------
    // Переходы между экранами
    // -----------------------------------------------------------------------

    fn open(&mut self, ctx: &mut Ctx, screen: Screen) {
        self.screen = screen;
        self.ui.show(ctx.scene_mut(), screen);
        if let Some(c) = self.ui.screen_canvas(screen) {
            self.ui.fade_in(ctx, c);
        }
    }

    pub fn go_menu(&mut self, ctx: &mut Ctx) {
        self.results_delay = None;
        self.slowmo = 0.0;
        self.view.reset(ctx.scene_mut());
        self.view.show_actors(ctx.scene_mut(), true);
        let cont = load_checkpoint(ctx.save_data()).map(|c| c.next_wave);
        let best = format!(
            "Рекорд кампании: {}   ·   рекорд бесконечного режима: {}   ·   дальше всего: волна {}",
            crate::ui::grouped(self.progress.best_campaign),
            crate::ui::grouped(self.progress.best_endless),
            self.progress.max_wave
        );
        self.ui.refresh_menu(ctx.scene_mut(), cont, &best);
        // Фон меню — спокойная арена.
        self.sim = Sim::new(ctx.random().next_u64(), Mode::Campaign);
        self.sim.player.invuln = 99.0;
        self.open(ctx, Screen::Menu);
    }

    /// Начать забег (с контрольной точки или с нуля).
    pub fn start_run(&mut self, ctx: &mut Ctx, mode: Mode, checkpoint: Option<Checkpoint>) {
        let seed = ctx.random().next_u64();
        self.sim = match &checkpoint {
            Some(cp) => Sim::from_checkpoint(seed, cp),
            None => Sim::new(seed, mode),
        };
        self.view.reset(ctx.scene_mut());
        self.view.show_actors(ctx.scene_mut(), true);
        self.slowmo = 0.0;
        self.results_delay = None;
        self.unlock_shown = false;
        self.screen = Screen::Playing;
        self.ui.show(ctx.scene_mut(), Screen::Playing);
        self.open_playing(ctx);
    }

    fn open_playing(&mut self, ctx: &mut Ctx) {
        self.screen = Screen::Playing;
        self.ui.show(ctx.scene_mut(), Screen::Playing);
    }

    fn pause(&mut self, ctx: &mut Ctx) {
        if self.screen == Screen::Playing && self.results_delay.is_none() {
            self.open(ctx, Screen::Paused);
        }
    }

    fn resume(&mut self, ctx: &mut Ctx) {
        if self.screen == Screen::Paused {
            self.resumed_frame = true;
            self.open_playing(ctx);
        }
    }

    fn open_options(&mut self, ctx: &mut Ctx, from: Screen) {
        self.options_from = from;
        self.ui.refresh_options(ctx.scene_mut(), &self.opts);
        self.open(ctx, Screen::Options);
    }

    fn close_options(&mut self, ctx: &mut Ctx) {
        let save = ctx.save_data();
        self.opts.store(save);
        if let Err(e) = save.save() {
            log::warn!("settings not saved: {e}");
        }
        match self.options_from {
            Screen::Paused => self.open(ctx, Screen::Paused),
            _ => self.go_menu_keep(ctx),
        }
    }

    /// Вернуться в главное меню, не пересоздавая симуляцию-фон.
    fn go_menu_keep(&mut self, ctx: &mut Ctx) {
        let cont = load_checkpoint(ctx.save_data()).map(|c| c.next_wave);
        let best = format!(
            "Рекорд кампании: {}   ·   рекорд бесконечного режима: {}   ·   дальше всего: волна {}",
            crate::ui::grouped(self.progress.best_campaign),
            crate::ui::grouped(self.progress.best_endless),
            self.progress.max_wave
        );
        self.ui.refresh_menu(ctx.scene_mut(), cont, &best);
        self.open(ctx, Screen::Menu);
    }

    fn open_mode_select(&mut self, ctx: &mut Ctx) {
        let board = load_leaderboard(ctx.save_data());
        self.ui.refresh_mode(ctx.scene_mut(), self.progress.endless_unlocked(), &board);
        self.open(ctx, Screen::ModeSelect);
    }

    fn open_upgrade(&mut self, ctx: &mut Ctx) {
        let offer = self.sim.roll_offer();
        if offer.is_empty() {
            self.sim.skip_upgrade();
            return;
        }
        let reason = if matches!(self.sim.phase, Phase::Cleared | Phase::Intermission) {
            format!("Волна {} пройдена — выберите награду", self.sim.wave)
        } else {
            format!("Новый уровень {} — выберите улучшение", self.sim.level)
        };
        self.reason = reason.clone();
        self.ui.fill_cards(ctx.scene_mut(), &offer, &self.sim.levels, &reason);
        self.upgrade_screens += 1;
        self.open(ctx, Screen::Upgrade);
    }

    fn choose_card(&mut self, ctx: &mut Ctx, index: usize) {
        let Some(offer) = self.sim.offer.clone() else { return };
        let Some(up) = offer.get(index).copied() else { return };
        if !self.sim.choose_upgrade(up) {
            return;
        }
        if self.sim.pending_upgrades > 0 && !self.sim.is_over() {
            self.open_upgrade(ctx);
            return;
        }
        if self.sim.phase == Phase::Cleared && self.sim.pending_upgrades == 0 {
            // Контрольная точка: «Продолжить» начнёт со следующей волны.
            let cp = self.sim.checkpoint();
            let save = ctx.save_data();
            store_checkpoint(save, &cp);
            if let Err(e) = save.save() {
                log::warn!("checkpoint not saved: {e}");
            }
        }
        self.open_playing(ctx);
    }

    // -----------------------------------------------------------------------
    // Конец забега
    // -----------------------------------------------------------------------

    fn show_results(&mut self, ctx: &mut Ctx) {
        let won = self.result_won;
        let sim = &self.sim;
        let secs = sim.time as u32;
        let mut new_best = false;
        let p = &mut self.progress;
        p.runs += 1;
        match sim.mode {
            Mode::Campaign => {
                if won {
                    p.wins += 1;
                }
                if sim.score > p.best_campaign {
                    p.best_campaign = sim.score;
                    new_best = true;
                }
            }
            Mode::Endless => {
                if sim.score > p.best_endless {
                    p.best_endless = sim.score;
                    new_best = true;
                }
                p.best_endless_wave = p.best_endless_wave.max(sim.wave);
            }
        }
        let rec = RunRecord { score: sim.score, wave: sim.wave, mode: sim.mode, secs, won };
        let save = ctx.save_data();
        let place = add_record(save, rec);
        self.progress.store(save);
        clear_checkpoint(save);
        if let Err(e) = save.save() {
            log::warn!("results not saved: {e}");
        }
        self.ui.fill_results(ctx.scene_mut(), won, &self.sim, place, new_best);
        self.results_delay = None;
        self.open(ctx, Screen::Results);
    }

    // -----------------------------------------------------------------------
    // Кадр
    // -----------------------------------------------------------------------

    pub fn update(&mut self, ctx: &mut Ctx, dt: f32) {
        let real_dt = ctx.time().unscaled_delta.max(0.0);
        self.clock += real_dt;
        self.update_ortho(ctx);
        self.handle_ui(ctx);
        if self.screen == Screen::Playing {
            self.play(ctx, dt);
        }
        // Пауза результата (взрыв реактора, фанфары).
        if let Some(t) = self.results_delay.as_mut() {
            *t -= real_dt;
            if *t <= 0.0 && self.screen == Screen::Playing {
                self.show_results(ctx);
            }
        }
        if self.slowmo > 0.0 {
            self.slowmo -= real_dt;
        }
        // Отображение.
        let scene = &mut ctx.world.scene;
        self.view.sync(scene, &self.sim, dt, self.clock);
        if self.ui_hud_visible() {
            let boss = self.sim.boss().map(|b| (b.kind.info().name, (b.hp / b.max_hp).clamp(0.0, 1.0)));
            self.ui.update_hud(scene, &self.sim, boss, self.clock);
        }
        self.update_music(ctx);
        self.resumed_frame = false;
        let scale = match self.screen {
            Screen::Paused | Screen::Upgrade => 0.0,
            _ if self.slowmo > 0.0 => 0.3,
            _ => 1.0,
        };
        if (ctx.time_scale() - scale).abs() > 1e-4 {
            ctx.set_time_scale(scale);
        }
    }

    fn ui_hud_visible(&self) -> bool {
        matches!(self.screen, Screen::Playing | Screen::Paused)
    }

    fn update_ortho(&mut self, ctx: &mut Ctx) {
        let s = ctx.input().screen_size();
        let aspect = if s.y > 0.0 { s.x / s.y } else { 16.0 / 9.0 };
        let ortho = BASE_ORTHO.max(14.6 / aspect);
        if (ortho - self.ortho).abs() > 1e-3 {
            self.ortho = ortho;
        }
        let cam = self.view.cam;
        if let Some(c) = ctx.scene_mut().get_component_mut::<Camera>(cam) {
            if (c.ortho_size - ortho).abs() > 1e-3 {
                c.ortho_size = ortho;
            }
        }
    }

    fn update_music(&mut self, ctx: &mut Ctx) {
        let want = match self.screen {
            Screen::Menu | Screen::ModeSelect | Screen::Results => Track::Menu,
            Screen::Options if self.options_from == Screen::Menu => Track::Menu,
            _ => {
                if self.sim.boss().is_some() || self.sim.telegraphs.iter().any(|t| t.kind.is_boss()) {
                    Track::Boss
                } else {
                    Track::Game
                }
            }
        };
        if want != self.track {
            self.track = want;
            if let Some(audio) = ctx.resource_mut::<Audio>() {
                let name = match want {
                    Track::Menu => "music_menu",
                    Track::Game => "music_game",
                    Track::Boss => "music_boss",
                    Track::None => return,
                };
                audio.play_music(name, 1.2);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Интерфейс
    // -----------------------------------------------------------------------

    fn ui_sound(&self, ctx: &mut Ctx, name: &str) {
        if let Some(a) = ctx.resource_mut::<Audio>() {
            a.play_with(name, PlayParams::new().volume(0.7).bus(Bus::Ui));
        }
    }

    fn handle_ui(&mut self, ctx: &mut Ctx) {
        // Звуки наведения и клика.
        let hovered = ctx.ui().hovered();
        if hovered != self.hover {
            self.hover = hovered;
            if let Some(h) = hovered {
                if ctx.scene().get_component::<Button>(h).is_some() && self.screen != Screen::Playing {
                    self.ui_sound(ctx, "ui_hover");
                }
            }
        }
        let clicks: Vec<String> = ctx.ui().events().iter().filter(|e| e.kind == engine::prelude::UiEventKind::Click).map(|e| e.name.clone()).collect();
        let clicked = |name: &str| clicks.iter().any(|c| c == name);
        let cancel = ctx.input().button_down("Cancel") || ctx.input().any_gamepad_button_down(GamepadButton::Start);
        let enabled = |ctx: &Ctx, id: GameObjectId| ctx.scene().get_component::<Button>(id).is_some_and(|b| b.interactable);
        if !clicks.is_empty() {
            let blocked = clicked("endless") && !enabled(ctx, self.ui.endless_btn);
            self.ui_sound(ctx, if blocked { "ui_error" } else { "ui_click" });
        }
        match self.screen {
            Screen::Menu => {
                if clicked("continue") {
                    if let Some(cp) = load_checkpoint(ctx.save_data()) {
                        self.start_run(ctx, cp.mode, Some(cp));
                    }
                } else if clicked("play") {
                    self.open_mode_select(ctx);
                } else if clicked("options") {
                    self.open_options(ctx, Screen::Menu);
                } else if clicked("quit") {
                    ctx.quit();
                }
            }
            Screen::ModeSelect => {
                if clicked("campaign") {
                    self.start_run(ctx, Mode::Campaign, None);
                } else if clicked("endless") && self.progress.endless_unlocked() {
                    self.start_run(ctx, Mode::Endless, None);
                } else if clicked("back") || cancel {
                    self.go_menu_keep(ctx);
                }
            }
            Screen::Options => {
                if let Some(v) = ctx.ui().value_changed("music") {
                    self.opts.music = v;
                }
                if let Some(v) = ctx.ui().value_changed("sfx") {
                    self.opts.sfx = v;
                }
                if let Some(on) = ctx.ui().toggled("shake") {
                    self.opts.shake = on;
                }
                if let Some(on) = ctx.ui().toggled("numbers") {
                    self.opts.numbers = on;
                }
                if ctx.ui().value_changed("music").is_some() || ctx.ui().value_changed("sfx").is_some() {
                    if let Some(a) = ctx.resource_mut::<Audio>() {
                        sound::apply_volumes(a, &self.opts);
                    }
                    self.ui.refresh_option_labels(ctx.scene_mut(), &self.opts);
                    if ctx.ui().value_changed("sfx").is_some() {
                        self.ui_sound(ctx, "ui_click");
                    }
                }
                if clicked("options_back") || cancel {
                    self.close_options(ctx);
                }
            }
            Screen::Playing => {
                if clicked("pause") {
                    self.pause(ctx);
                }
            }
            Screen::Paused => {
                if clicked("resume") || cancel {
                    self.resume(ctx);
                } else if clicked("restart") {
                    let mode = self.sim.mode;
                    self.start_run(ctx, mode, None);
                } else if clicked("pause_options") {
                    self.open_options(ctx, Screen::Paused);
                } else if clicked("to_menu") {
                    self.go_menu(ctx);
                }
            }
            Screen::Upgrade => {
                let keys = [Key::Digit1, Key::Digit2, Key::Digit3];
                for (i, key) in keys.into_iter().enumerate() {
                    if clicked(&format!("card{i}")) || ctx.input().key_down(key) {
                        self.choose_card(ctx, i);
                        break;
                    }
                }
            }
            Screen::Results => {
                if clicked("again") {
                    let mode = self.sim.mode;
                    self.start_run(ctx, mode, None);
                } else if clicked("results_menu") {
                    self.go_menu(ctx);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Игра
    // -----------------------------------------------------------------------

    fn play(&mut self, ctx: &mut Ctx, dt: f32) {
        let pause = ctx.input().button_down("Cancel") || ctx.input().any_gamepad_button_down(GamepadButton::Start);
        if pause && self.results_delay.is_none() && !self.resumed_frame {
            self.pause(ctx);
            return;
        }
        let mut input = self.gather_input(ctx);
        if self.debug.bot {
            input = bot::bot_input(&self.sim);
        }
        if self.debug.stress {
            self.sim.fill_stress();
        }
        // Длинный кадр делим на подшаги: симуляция устойчива при шаге до 1/60.
        let dt = dt.min(0.1);
        let n = ((dt * 60.0).ceil() as u32).clamp(1, 6);
        for k in 0..n {
            let mut step_input = input;
            if k > 0 {
                step_input.dash = false;
            }
            self.sim.step(&step_input, dt / n as f32);
        }
        let events = std::mem::take(&mut self.sim.events);
        {
            let world = &mut *ctx.world;
            let audio = world.resources.get_mut::<Audio>();
            self.view.handle_events(&mut world.scene, audio, &events, &self.opts, dt);
        }
        for ev in &events {
            self.react(ctx, ev);
        }
        if self.debug.bot {
            // Автопилот сам выбирает награды, чтобы экран карточек не останавливал игру.
            while self.sim.pending_upgrades > 0 && !self.sim.is_over() {
                let offer = self.sim.roll_offer();
                match bot::bot_pick(&offer, &self.sim.levels) {
                    Some(up) => {
                        self.sim.choose_upgrade(up);
                    }
                    None => self.sim.skip_upgrade(),
                }
            }
        }
        if self.sim.upgrade_ready() && self.results_delay.is_none() {
            self.open_upgrade(ctx);
        }
    }

    /// Реакция потока на события (надписи, замедление, конец забега, прогресс).
    fn react(&mut self, ctx: &mut Ctx, ev: &Event) {
        match ev {
            Event::WaveStart { wave, boss } => {
                let unlocked = self.progress.note_wave(*wave);
                let (text, sub) = match boss {
                    Some(k) => (format!("ВОЛНА {wave}"), format!("Босс: {}", k.info().name)),
                    None => (format!("ВОЛНА {wave}"), String::new()),
                };
                let sub = if unlocked && !self.unlock_shown {
                    self.unlock_shown = true;
                    "Открыт бесконечный режим!".to_string()
                } else {
                    sub
                };
                self.ui.banner(ctx, &text, &sub, if boss.is_some() { Color::hex(0xff6a7a) } else { Color::hex(0xe6f4ff) });
                if unlocked {
                    let save = ctx.save_data();
                    self.progress.store(save);
                    let _ = save.save();
                }
            }
            Event::WaveCleared { wave } => {
                self.ui.banner(ctx, "ВОЛНА ПРОЙДЕНА", &format!("Волна {wave} позади"), Color::hex(0x7dffb0));
            }
            Event::BossAppear { kind } => {
                self.ui.banner(ctx, "ВНИМАНИЕ!", &format!("Приближается босс: {}", kind.info().name), Color::hex(0xff6a7a));
            }
            Event::BossDied { .. } => {
                self.slowmo = 1.4;
                self.ui.banner(ctx, "БОСС УНИЧТОЖЕН", "", Color::hex(0xffd23d));
            }
            Event::Victory => {
                self.result_won = true;
                self.results_delay = Some(2.6);
            }
            Event::Defeat => {
                self.result_won = false;
                self.results_delay = Some(1.9);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Ввод
    // -----------------------------------------------------------------------

    fn world_of_screen(&self, p: Vec2, screen: Vec2) -> Vec2 {
        Vec2::new((p.x - screen.x * 0.5) / screen.y * 2.0 * self.ortho, -(p.y - screen.y * 0.5) / screen.y * 2.0 * self.ortho)
    }

    /// Центр кнопки рывка на экране (пиксели) и её радиус.
    fn dash_button(&self, screen: Vec2, scale: f32) -> (Vec2, f32) {
        (Vec2::new(screen.x - 116.0 * scale, screen.y - 216.0 * scale), 52.0 * scale)
    }

    fn hud_scale(&self, ctx: &Ctx, screen: Vec2) -> f32 {
        ctx.scene().get_component::<Canvas>(self.ui.hud).map_or(1.0, |c| c.scale_for(screen))
    }

    fn gather_input(&mut self, ctx: &mut Ctx) -> PlayerInput {
        let screen = ctx.input().screen_size();
        let scale = self.hud_scale(ctx, screen);
        let input = ctx.input();
        let mut mv = Vec2::new(input.axis("Horizontal"), input.axis("Vertical")).clamp_length_max(1.0);
        let mut aim = Vec2::ZERO;
        let mut fire = false;
        let mut dash = input.button_down("Dash");

        // Касания: левая половина — движение, правая — прицел и огонь.
        let touches: Vec<Touch> = input.touches().to_vec();
        if !touches.is_empty() {
            self.touch.seen = true;
        }
        let (dash_center, dash_radius) = self.dash_button(screen, scale);
        for t in &touches {
            let assigned = self.touch.left.is_some_and(|(id, _)| id == t.id) || self.touch.right.is_some_and(|(id, _)| id == t.id);
            if t.phase == TouchPhase::Began && !assigned {
                if self.touch.seen && t.position.distance(dash_center) < dash_radius {
                    dash = true;
                } else if t.position.x < screen.x * 0.5 && self.touch.left.is_none() {
                    self.touch.left = Some((t.id, t.position));
                } else if t.position.x >= screen.x * 0.5 && self.touch.right.is_none() {
                    self.touch.right = Some((t.id, t.position));
                }
            }
        }
        let radius = STICK_RADIUS * scale;
        let mut left_view = None;
        let mut right_view = None;
        let mut touch_mv = None;
        let mut touch_aim = None;
        if let Some((id, origin)) = self.touch.left {
            match touches.iter().find(|t| t.id == id && t.phase != TouchPhase::Ended) {
                Some(t) => {
                    let d = (t.position - origin) / radius;
                    touch_mv = Some(d.clamp_length_max(1.0));
                    left_view = Some((origin, origin + d.clamp_length_max(1.0) * radius));
                }
                None => self.touch.left = None,
            }
        }
        if let Some((id, origin)) = self.touch.right {
            match touches.iter().find(|t| t.id == id && t.phase != TouchPhase::Ended) {
                Some(t) => {
                    let d = (t.position - origin) / radius;
                    touch_aim = Some(d.clamp_length_max(1.0));
                    right_view = Some((origin, origin + d.clamp_length_max(1.0) * radius));
                }
                None => self.touch.right = None,
            }
        }
        if let Some(m) = touch_mv {
            if m.length() > 0.12 {
                mv = m;
            }
        }
        if let Some(a) = touch_aim {
            if a.length() > 0.2 {
                aim = a.normalize();
                fire = true;
            }
        }
        // Правый стик геймпада и стрелки клавиатуры: прицел и огонь.
        let stick = Vec2::new(input.axis("AimX"), input.axis("AimY"));
        if stick.length() > 0.3 {
            aim = stick.normalize();
            fire = true;
            self.mouse_aim = false;
        }
        // Мышь: прицел в точку, огонь по кнопке.
        let mouse_active = input.mouse_delta().length_squared() > 0.0 || input.mouse_button_down(MouseButton::Left);
        if mouse_active && touches.is_empty() {
            self.mouse_aim = true;
        }
        if self.mouse_aim && stick.length() <= 0.3 && touches.is_empty() {
            let w = self.world_of_screen(input.mouse_position(), screen);
            let from = self.sim.player.pos;
            let d = w - from;
            if d.length_squared() > 0.01 {
                aim = d.normalize();
            }
            if input.button("Fire") && !ctx.ui().pointer_over_ui() {
                fire = true;
            }
        }
        // Гашетка геймпада работает и без правого стика.
        if input.gamepad_count() > 0 && input.button_value("Fire") > 0.3 && !self.mouse_aim {
            fire = true;
        }
        if self.touch.seen {
            let (l, r) = (left_view, right_view);
            self.ui.set_touch_ui(ctx.scene_mut(), scale, true, l, r);
        }
        if self.touch.seen && mv.length_squared() == 0.0 && left_view.is_none() {
            // нет движения
        }
        PlayerInput { mv, aim, fire, dash }
    }

    // -----------------------------------------------------------------------
    // Отладочные экраны для скриншотов
    // -----------------------------------------------------------------------

    fn apply_debug_screen(&mut self, ctx: &mut Ctx) {
        let Some(name) = self.debug.screen.clone() else { return };
        match name.as_str() {
            "menu" => {}
            "mode" => self.open_mode_select(ctx),
            "options" => self.open_options(ctx, Screen::Menu),
            other => {
                // wave<N>[_pause|_upgrade|_win|_lose]
                let (head, tail) = other.split_once('_').unwrap_or((other, ""));
                let wave: u32 = head.trim_start_matches("wave").parse().unwrap_or(1);
                self.debug_run(ctx, wave);
                match tail {
                    "pause" => self.pause(ctx),
                    "upgrade" => {
                        self.sim.pending_upgrades = 1;
                        self.open_upgrade(ctx);
                    }
                    "win" => {
                        self.result_won = true;
                        self.sim.score = self.sim.score.max(48_250);
                        self.show_results(ctx);
                    }
                    "lose" => {
                        self.result_won = false;
                        self.show_results(ctx);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Забег с готовыми улучшениями «как будто дошли до волны `wave`» и автопилотом.
    fn debug_run(&mut self, ctx: &mut Ctx, wave: u32) {
        self.start_run(ctx, Mode::Campaign, None);
        self.debug.bot = true;
        for i in 1..wave {
            self.sim.pending_upgrades = 1;
            let offer = self.sim.roll_offer();
            if let Some(up) = bot::bot_pick(&offer, &self.sim.levels) {
                self.sim.choose_upgrade(up);
            }
            if i % 2 == 0 {
                self.sim.level += 1;
            }
        }
        self.sim.start_wave(wave);
        self.sim.score = (wave as u64) * 1800;
        self.sim.time = wave as f32 * 38.0;
    }
}

component! {
    /// Менеджер игры: единственный «скрипт» сцены. Строит интерфейс и пулы объектов при
    /// старте и каждый кадр отдаёт управление [`Game`].
    pub struct Director {}
}

impl Component for Director {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let debug = crate::debug::read();
        let game = Game::new(ctx, debug);
        ctx.world.resources.insert(game);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<Game>() else { return };
        game.update(ctx, dt);
        ctx.world.resources.insert(game);
    }
}
