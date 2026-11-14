//! Менеджер игры: ход партии — шаг симуляции, события → отображение, музыка, масштаб
//! времени, начало и конец забега.
//!
//! Всё состояние лежит в одной структуре [`Game`] (ресурс мира). Компоненты на объекте
//! `GameManager` сцены (`GameManager`, `Menus`, `TouchControls`, `Waves`, `Upgrades`, `Hud`)
//! каждый кадр вынимают её, делают своё и кладут обратно — поэтому у `Game` есть честный
//! `&mut World` без `RefCell`. Порядок компонентов задаёт `script_order` в
//! `project.settings`. Пулы объектов создаются из префабов, определения — из `assets/data`.

use engine::prelude::*;

use crate::bot;
use crate::debug::Debug;
use crate::defs::{self, Defs};
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
pub struct TouchState {
    pub seen: bool,
    pub left: Option<(u64, Vec2)>,
    pub right: Option<(u64, Vec2)>,
}

/// Настройки, которые компоненты менеджера (поля в Inspector) передают в `Game`.
#[derive(Clone, Debug)]
pub struct GameConfig {
    pub base_ortho: f32,
    pub slowmo_seconds: f32,
    pub slowmo_scale: f32,
    pub win_delay: f32,
    pub lose_delay: f32,
    pub stick_radius: f32,
    pub dash_button_radius: f32,
    pub show_quit: bool,
    pub offer_count: usize,
    pub wave_title: String,
    pub boss_line: String,
    pub clear_title: String,
    pub clear_sub: String,
    pub warning_title: String,
    pub unlock_text: String,
    pub boss_down_title: String,
    pub danger_below: f32,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            base_ortho: 10.4,
            slowmo_seconds: 1.4,
            slowmo_scale: 0.3,
            win_delay: 2.6,
            lose_delay: 1.9,
            stick_radius: 70.0,
            dash_button_radius: 52.0,
            show_quit: true,
            offer_count: 3,
            wave_title: "ВОЛНА {wave}".into(),
            boss_line: "Босс: {boss}".into(),
            clear_title: "ВОЛНА ПРОЙДЕНА".into(),
            clear_sub: "Волна {wave} позади".into(),
            warning_title: "ВНИМАНИЕ!".into(),
            unlock_text: "Открыт бесконечный режим!".into(),
            boss_down_title: "БОСС УНИЧТОЖЕН".into(),
            danger_below: 0.38,
        }
    }
}

pub struct Game {
    pub sim: Sim,
    pub screen: Screen,
    pub opts: Options,
    pub progress: Progress,
    pub view: View,
    pub ui: Ui,
    pub track: Track,
    pub cfg: GameConfig,
    /// Ввод этого кадра (его пишет `TouchControls`).
    pub input: PlayerInput,
    pub(crate) options_from: Screen,
    /// Оставшиеся реальные секунды замедления (смерть босса).
    pub slowmo: f32,
    /// Через сколько реальных секунд показать результаты (забег закончен).
    pub results_delay: Option<f32>,
    pub result_won: bool,
    pub(crate) hover: Option<GameObjectId>,
    pub touch: TouchState,
    pub(crate) mouse_aim: bool,
    pub(crate) ortho: f32,
    pub clock: f32,
    pub debug: Debug,
    /// Последнее предложение улучшений и причина (для карточек).
    pub(crate) reason: String,
    /// Сколько раз игра показывала карточки (для тестов).
    pub upgrade_screens: u32,
    /// Сообщение «открыт бесконечный режим» уже показано в этом забеге.
    pub(crate) unlock_shown: bool,
    /// Игру только что сняли с паузы в этом кадре: та же клавиша не должна поставить её снова.
    pub(crate) resumed_frame: bool,
}

impl Game {
    pub fn new(ctx: &mut Ctx, debug: Debug) -> Game {
        let db = ctx.resource::<AssetDatabase>().expect("assets are mounted before the scene loads").clone();
        match Defs::load(&db) {
            Ok(d) => defs::install(d),
            Err(e) => log::error!("{e}; using the data bundled at build time"),
        }
        let seed = ctx.random().next_u64();
        let view = View::new(ctx.world, seed);
        let ui = Ui::bind(&ctx.world.scene, &db);
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
            cfg: GameConfig::default(),
            input: PlayerInput::default(),
            options_from: Screen::Menu,
            slowmo: 0.0,
            results_delay: None,
            result_won: false,
            hover: None,
            touch: TouchState::default(),
            mouse_aim: false,
            ortho: 10.4,
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

    /// Начать забег (с контрольной точки или с нуля).
    pub fn start_run(&mut self, ctx: &mut Ctx, mode: Mode, checkpoint: Option<Checkpoint>) {
        let seed = ctx.random().next_u64();
        self.sim = match &checkpoint {
            Some(cp) => Sim::from_checkpoint(seed, cp),
            None => Sim::new(seed, mode),
        };
        self.sim.offer_count = self.cfg.offer_count;
        self.view.reset(ctx.scene_mut());
        self.view.show_actors(ctx.scene_mut(), true);
        self.slowmo = 0.0;
        self.results_delay = None;
        self.unlock_shown = false;
        self.screen = Screen::Playing;
        self.ui.show(ctx.scene_mut(), Screen::Playing);
        self.open_playing(ctx);
    }

    pub fn update(&mut self, ctx: &mut Ctx, dt: f32) {
        let real_dt = ctx.time().unscaled_delta.max(0.0);
        self.clock += real_dt;
        self.update_ortho(ctx);
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
        self.view.sync(ctx.world, &self.sim, dt, self.clock);
        self.update_music(ctx);
        self.resumed_frame = false;
        let scale = match self.screen {
            Screen::Paused | Screen::Upgrade => 0.0,
            _ if self.slowmo > 0.0 => self.cfg.slowmo_scale,
            _ => 1.0,
        };
        if (ctx.time_scale() - scale).abs() > 1e-4 {
            ctx.set_time_scale(scale);
        }
    }

    pub(crate) fn hud_visible(&self) -> bool {
        matches!(self.screen, Screen::Playing | Screen::Paused)
    }

    pub(crate) fn update_ortho(&mut self, ctx: &mut Ctx) {
        let s = ctx.input().screen_size();
        let aspect = if s.y > 0.0 { s.x / s.y } else { 16.0 / 9.0 };
        let ortho = self.cfg.base_ortho.max(14.6 / aspect);
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

    pub(crate) fn update_music(&mut self, ctx: &mut Ctx) {
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

    pub(crate) fn play(&mut self, ctx: &mut Ctx, dt: f32) {
        let pause = ctx.input().button_down("Cancel") || ctx.input().any_gamepad_button_down(GamepadButton::Start);
        if pause && self.results_delay.is_none() && !self.resumed_frame {
            self.pause(ctx);
            return;
        }
        let mut input = self.input;
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
    pub(crate) fn react(&mut self, ctx: &mut Ctx, ev: &Event) {
        match ev {
            Event::WaveStart { wave, boss } => {
                let unlocked = self.progress.note_wave(*wave);
                let c = &self.cfg;
                let text = c.wave_title.replace("{wave}", &wave.to_string());
                let sub = match boss {
                    Some(k) => c.boss_line.replace("{boss}", &k.info().name),
                    None => String::new(),
                };
                let sub = if unlocked && !self.unlock_shown {
                    self.unlock_shown = true;
                    c.unlock_text.clone()
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
                let (t, sub) = (self.cfg.clear_title.clone(), self.cfg.clear_sub.replace("{wave}", &wave.to_string()));
                self.ui.banner(ctx, &t, &sub, Color::hex(0x7dffb0));
            }
            Event::BossAppear { kind } => {
                let sub = self.view.boss_intro(*kind).replace("{boss}", &kind.info().name);
                let t = self.cfg.warning_title.clone();
                self.ui.banner(ctx, &t, &sub, Color::hex(0xff6a7a));
            }
            Event::BossDied { .. } => {
                self.slowmo = self.cfg.slowmo_seconds;
                let t = self.cfg.boss_down_title.clone();
                self.ui.banner(ctx, &t, "", Color::hex(0xffd23d));
            }
            Event::Victory => {
                self.result_won = true;
                self.results_delay = Some(self.cfg.win_delay);
            }
            Event::Defeat => {
                self.result_won = false;
                self.results_delay = Some(self.cfg.lose_delay);
            }
            _ => {}
        }
    }

    pub(crate) fn apply_debug_screen(&mut self, ctx: &mut Ctx) {
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
    pub(crate) fn debug_run(&mut self, ctx: &mut Ctx, wave: u32) {
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
    /// Менеджер игры: ход партии (шаг симуляции, события, музыка, замедление при смерти
    /// босса). Висит на объекте `GameManager` сцены вместе с остальными скриптами игры.
    pub struct GameManager {
        /// Видимая половина высоты арены (для узких экранов камера отъезжает сама).
        pub base_ortho: f32 = 10.4,
        /// Сколько реальных секунд длится замедление при смерти босса.
        pub slowmo_seconds: f32 = 1.4,
        /// Во сколько раз замедляется время.
        pub slowmo_scale: f32 = 0.3,
        /// Пауза перед экраном результатов после победы, секунд.
        pub win_delay: f32 = 2.6,
        /// То же после поражения (успеть увидеть взрыв реактора).
        pub lose_delay: f32 = 1.9,
    }
}

impl Component for GameManager {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let debug = crate::debug::read();
        let game = Game::new(ctx, debug);
        ctx.world.resources.insert(game);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<Game>() else { return };
        game.cfg.base_ortho = self.base_ortho;
        game.cfg.slowmo_seconds = self.slowmo_seconds;
        game.cfg.slowmo_scale = self.slowmo_scale;
        game.cfg.win_delay = self.win_delay;
        game.cfg.lose_delay = self.lose_delay;
        game.update(ctx, dt);
        ctx.world.resources.insert(game);
    }
}
