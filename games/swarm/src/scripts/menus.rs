//! Меню и экраны: главное меню, выбор режима, настройки, пауза, карточки улучшений,
//! результаты. Компонент `Menus` на менеджере; кнопки берутся из UI-префабов `assets/ui`.

use engine::prelude::*;

use crate::bot;
use crate::profile::*;
use crate::scripts::game_manager::*;
use crate::sim::*;
use crate::sound;
use crate::ui::{Screen, Ui};

impl Game {
    pub(crate) fn open(&mut self, ctx: &mut Ctx, screen: Screen) {
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

    pub(crate) fn open_playing(&mut self, ctx: &mut Ctx) {
        self.screen = Screen::Playing;
        self.ui.show(ctx.scene_mut(), Screen::Playing);
    }

    pub(crate) fn pause(&mut self, ctx: &mut Ctx) {
        if self.screen == Screen::Playing && self.results_delay.is_none() {
            self.open(ctx, Screen::Paused);
        }
    }

    pub(crate) fn resume(&mut self, ctx: &mut Ctx) {
        if self.screen == Screen::Paused {
            self.resumed_frame = true;
            self.open_playing(ctx);
        }
    }

    pub(crate) fn open_options(&mut self, ctx: &mut Ctx, from: Screen) {
        self.options_from = from;
        self.ui.refresh_options(ctx.scene_mut(), &self.opts);
        self.open(ctx, Screen::Options);
    }

    pub(crate) fn close_options(&mut self, ctx: &mut Ctx) {
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
    pub(crate) fn go_menu_keep(&mut self, ctx: &mut Ctx) {
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

    pub(crate) fn open_mode_select(&mut self, ctx: &mut Ctx) {
        let board = load_leaderboard(ctx.save_data());
        self.ui.refresh_mode(ctx.scene_mut(), self.progress.endless_unlocked(), &board);
        self.open(ctx, Screen::ModeSelect);
    }

    pub(crate) fn open_upgrade(&mut self, ctx: &mut Ctx) {
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

    pub(crate) fn choose_card(&mut self, ctx: &mut Ctx, index: usize) {
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

    pub(crate) fn show_results(&mut self, ctx: &mut Ctx) {
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

    pub(crate) fn ui_sound(&self, ctx: &mut Ctx, name: &str) {
        if let Some(a) = ctx.resource_mut::<Audio>() {
            a.play_with(name, PlayParams::new().volume(0.7).bus(Bus::Ui));
        }
    }

    pub(crate) fn handle_ui(&mut self, ctx: &mut Ctx) {
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
}

component! {
    /// Меню и экраны. Кнопки находятся по именам в UI-префабах (`assets/ui`).
    pub struct Menus {
        /// Показывать ли кнопку «Выход». В браузере она скрыта всегда: вкладку закрыть нельзя.
        pub show_quit: bool = true,
    }
}

impl Component for Menus {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let Some(mut game) = ctx.world.resources.remove::<Game>() else { return };
        game.cfg.show_quit = self.show_quit && !cfg!(target_arch = "wasm32");
        if game.screen == Screen::Menu {
            let quit = game.ui.quit_btn;
            ctx.world.scene.ui_set_visible(quit, game.cfg.show_quit);
        }
        game.handle_ui(ctx);
        ctx.world.resources.insert(game);
    }
}
