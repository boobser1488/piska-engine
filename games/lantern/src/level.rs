//! Менеджер уровня: загрузка карты, появление объектов, правила (светлячки, сердца,
//! контрольные точки, победа и поражение), HUD, пауза и экран результатов.

use crate::common::{self, Gfx};
use crate::controls;
use crate::data::{format_time, Progress, RunResult, LEVELS, LEVEL_COUNT};
use crate::phys::{Grid, Shared};
use crate::player::{Player, MAX_HP};
use crate::sfx;
use crate::spawn;
use crate::ui;
use engine::prelude::*;

reflect_enum! {
    /// Что сейчас происходит на уровне.
    pub enum LevelState { Playing, Paused, Won, Lost }
}

component! {
    /// Менеджер уровня. Объект `Level` в сцене `levelN.scene`: параметры и список объектов
    /// уровня — в файле сцены (его пишет `tools/gen_levels.py`), остальное — состояние партии.
    pub struct LevelManager {
        /// Номер уровня с нуля.
        pub index: i32 = 0,
        pub title: String = String::new(),
        pub subtitle: String = String::new(),
        /// Сколько светлячков нужно для выхода.
        pub need: i32 = 5,
        /// Контрольное время для третьей звезды, секунд.
        pub par_time: f32 = 120.0,
        pub music: String = String::from("music_cave"),
        /// Объекты уровня: «вид x y ключ=значение … | текст». См. `spawn.rs`.
        pub entities: Vec<String> = Vec::new(),
        // --- состояние партии ---
        pub state: LevelState = LevelState::Playing,
        pub time: f32 = 0.0,
        pub fireflies: i32 = 0,
        pub total: i32 = 0,
        pub golden: bool = false,
        pub deaths: i32 = 0,
        pub kills: i32 = 0,
        pub checkpoint: Vec2 = Vec2::ZERO,
        pub spawn_point: Vec2 = Vec2::ZERO,
        pub boss_present: bool = false,
        pub boss_dead: bool = false,
        pub boss_hp: i32 = 9,
        pub end_timer: f32 = 0.0,
        pub banner_timer: f32 = 0.0,
        pub touch_seen: bool = false,
        pub shown_hp: i32 = -1,
        pub shown_second: i32 = -1,
        pub exit_announced: bool = false,
        pub result: String = String::new(),
    }
}

impl LevelManager {
    /// Горит ли выход: нужно число светлячков и (на уровне с боссом) победа над ним.
    pub fn exit_ready(&self) -> bool {
        self.fireflies >= self.need && (!self.boss_present || self.boss_dead)
    }

    pub fn is_playing(&self) -> bool {
        self.state == LevelState::Playing
    }

    fn info(&self) -> &'static crate::data::LevelInfo {
        &LEVELS[(self.index.max(0) as usize).min(LEVEL_COUNT - 1)]
    }

    // ------------------------------------------------------------ запуск

    fn setup_world(&mut self, ctx: &mut Ctx) {
        // Сетка столкновений из тайлмапа «Tiles».
        let grid = ctx
            .find_by_name("Tiles")
            .and_then(|id| ctx.scene().get_component::<Tilemap>(id).map(Grid::from_tilemap))
            .unwrap_or_else(|| {
                log::error!("level `{}` has no `Tiles` tilemap", self.title);
                Grid::empty(10, 10)
            });
        ctx.world.resources.insert(Shared::new(grid));
        ctx.world.resources.insert(crate::data::Session { level: self.index.max(0) as usize });

        let (double_jump, dash) = ctx.resource::<Progress>().map_or((false, false), |p| (p.has_double_jump(), p.has_dash()));
        let lines = self.entities.clone();
        for line in &lines {
            match spawn::spawn(ctx, line, double_jump, dash) {
                Ok(spawn::Spawned { kind, pos, id: _ }) => match kind.as_str() {
                    "firefly" => self.total += 1,
                    "player" => {
                        self.spawn_point = pos;
                        self.checkpoint = pos;
                    }
                    "boss" => self.boss_present = true,
                    _ => {}
                },
                Err(err) => log::error!("level object `{line}`: {err}"),
            }
        }
        self.need = self.need.min(self.total).max(0);
    }

    fn player_id(ctx: &Ctx) -> Option<GameObjectId> {
        ctx.find_by_tag("Player")
    }

    fn banner(&mut self, ctx: &mut Ctx, text: &str, seconds: f32) {
        ui::set_text(ctx, "BannerText", text);
        ui::show(ctx, "Banner", true);
        self.banner_timer = seconds;
    }

    fn hud_fireflies(&self, ctx: &mut Ctx) {
        let done = self.fireflies >= self.need;
        ui::set_text(ctx, "FireflyText", &format!("{} / {}", self.fireflies, self.need));
        ui::set_text_color(ctx, "FireflyText", if done { Color::hex(0xffd75a) } else { Color::WHITE });
        let k = if self.need > 0 { (self.fireflies as f32 / self.need as f32).clamp(0.0, 1.0) } else { 1.0 };
        ui::set_image_alpha(ctx, "LanternLit", k);
        ui::show(ctx, "GoldenIcon", true);
        let gfx = ctx.resource::<Gfx>().cloned().unwrap_or_default();
        ui::set_image_texture(ctx, "GoldenIcon", gfx.tex(if self.golden { "ui_star" } else { "ui_star_empty" }));
    }

    fn hud_hearts(&mut self, ctx: &mut Ctx, hp: i32) {
        if hp == self.shown_hp {
            return;
        }
        self.shown_hp = hp;
        let gfx = ctx.resource::<Gfx>().cloned().unwrap_or_default();
        for i in 0..MAX_HP {
            let tex = gfx.tex(if i < hp { "ui_heart" } else { "ui_heart_empty" });
            ui::set_image_texture(ctx, &format!("Heart{i}"), tex);
        }
    }

    // ------------------------------------------------------------ потоки партии

    fn set_state(&mut self, ctx: &mut Ctx, state: LevelState) {
        self.state = state;
        match state {
            LevelState::Playing => ctx.set_time_scale(1.0),
            LevelState::Paused | LevelState::Lost => ctx.set_time_scale(0.0),
            LevelState::Won => ctx.set_time_scale(0.4),
        }
    }

    fn freeze_player(ctx: &mut Ctx, on: bool) {
        if let Some(id) = Self::player_id(ctx) {
            ctx.send_message(id, "Freeze", Value::Bool(on));
        }
    }

    fn pause(&mut self, ctx: &mut Ctx, on: bool) {
        if on && self.state == LevelState::Playing {
            self.set_state(ctx, LevelState::Paused);
            ui::show(ctx, "PauseDim", true);
            sfx::play_ui(ctx, "confirm");
        } else if !on && self.state == LevelState::Paused {
            self.set_state(ctx, LevelState::Playing);
            ui::show(ctx, "PauseDim", false);
            sfx::play_ui(ctx, "click");
        }
    }

    fn win(&mut self, ctx: &mut Ctx) {
        if self.state != LevelState::Playing {
            return;
        }
        self.set_state(ctx, LevelState::Won);
        self.end_timer = 1.3;
        Self::freeze_player(ctx, true);
        sfx::play(ctx, "exit");
        common::shake(ctx, 0.3);
        // Результат записывается сразу: выйти в меню до показа экрана тоже можно.
        let index = self.index.max(0) as usize;
        let result = RunResult { time: self.time, fireflies: self.fireflies, total: self.total, golden: self.golden, deaths: self.deaths };
        let first_time = ctx.resource::<Progress>().is_none_or(|p| !p.completed.get(index).copied().unwrap_or(true));
        let progress = {
            let p = ctx.resource_or_default::<Progress>();
            p.record(index, &result, self.par_time);
            p.clone()
        };
        progress.store(ctx.save_data());
        if let Err(err) = ctx.save_data().save() {
            log::warn!("progress not saved: {err}");
        }
        self.result = match (index, first_time) {
            (1, true) => "Новая способность: двойной прыжок! Нажмите прыжок ещё раз в воздухе.".to_string(),
            (3, true) => "Новая способность: рывок! Shift / X / RB — быстрый бросок, сквозь врагов и снаряды.".to_string(),
            _ => String::new(),
        };
    }

    fn lose(&mut self, ctx: &mut Ctx) {
        if self.state != LevelState::Playing {
            return;
        }
        self.deaths += 1;
        // даём доиграть эффект гибели в замедлении; экран результатов — через секунду
        self.state = LevelState::Lost;
        ctx.set_time_scale(0.5);
        self.end_timer = 1.0;
        sfx::play(ctx, "lose");
    }

    fn show_results(&mut self, ctx: &mut Ctx) {
        let won = self.state == LevelState::Won;
        ctx.set_time_scale(0.0);
        let index = self.index.max(0) as usize;
        let last = index + 1 >= LEVEL_COUNT;
        ui::set_text(ctx, "ResTitle", if won { if last { "Тьма рассеяна!" } else { "Уровень пройден!" } } else { "Фонарь погас" });
        ui::set_text(ctx, "ResLevel", &format!("{}. {}", index + 1, self.info().name));
        ui::set_text(ctx, "ResFireflies", &format!("Светлячки: {} из {}", self.fireflies, self.total));
        ui::set_text(ctx, "ResGolden", if self.golden { "Золотой светлячок: найден" } else { "Золотой светлячок: не найден" });
        ui::set_text(ctx, "ResTime", &format!("Время: {}   (контрольное {})", format_time(self.time), format_time(self.par_time)));
        ui::set_text(ctx, "ResDeaths", &format!("Падения фонаря: {}", self.deaths));
        let stars = if won {
            RunResult { time: self.time, fireflies: self.fireflies, total: self.total, golden: self.golden, deaths: self.deaths }.stars(self.par_time)
        } else {
            0
        };
        let gfx = ctx.resource::<Gfx>().cloned().unwrap_or_default();
        for i in 0..3u8 {
            ui::set_image_texture(ctx, &format!("ResStar{i}"), gfx.tex(if i < stars { "ui_star" } else { "ui_star_empty" }));
            ui::show(ctx, &format!("ResStar{i}"), won);
        }
        ui::set_text(ctx, "ResAbility", &self.result);
        ui::show(ctx, "ResAbility", won && !self.result.is_empty());
        ui::show(ctx, "next", won && !last);
        ui::show(ctx, "retry_cp", !won);
        ui::show(ctx, "ResultsDim", true);
        if won {
            sfx::music(ctx, "music_menu", 1.0);
            sfx::play_ui(ctx, "confirm");
        }
    }

    fn leave(&mut self, ctx: &mut Ctx, scene: &str) {
        ctx.set_time_scale(1.0);
        ctx.load_scene(scene);
    }

    fn retry_from_checkpoint(&mut self, ctx: &mut Ctx) {
        ui::show(ctx, "ResultsDim", false);
        self.set_state(ctx, LevelState::Playing);
        if let Some(id) = Self::player_id(ctx) {
            let c = self.checkpoint;
            ctx.send_message(id, "Revive", Value::Vec3(c.x, c.y, 0.0));
            ctx.send_message(id, "Freeze", Value::Bool(false));
        }
        self.shown_hp = -1;
    }

    // ------------------------------------------------------------ камера

    fn drive_camera(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(cam) = ctx.find_by_tag("MainCamera") else { return };
        let Some(shared) = ctx.resource::<Shared>().cloned() else { return };
        let (w, h) = {
            let g = shared.grid.borrow();
            (g.width as f32, g.height as f32)
        };
        let screen = ctx.input().screen_size();
        let aspect = if screen.y > 0.0 { screen.x / screen.y } else { 16.0 / 9.0 };
        let ortho = ctx.scene().get_component::<Camera>(cam).map_or(7.0, |c| c.ortho_size);
        let half = Vec2::new(ortho * aspect, ortho);
        let (lo, hi) = (half, Vec2::new(w, h) - half);
        let lead = Self::player_id(ctx)
            .and_then(|id| ctx.get_on::<Player>(id).map(|p| (p.facing, p.vel.x)))
            .unwrap_or((1.0, 0.0));
        if let Some(c) = ctx.scene_mut().get_component_mut::<CameraController2D>(cam) {
            c.use_bounds = true;
            c.bounds_min = Vec2::new(lo.x, lo.y);
            c.bounds_max = Vec2::new(hi.x.max(lo.x), hi.y.max(lo.y));
            let want = lead.0 * 2.2 + lead.1 * 0.18;
            c.offset.x = exp_decay(c.offset.x, want, 2.5, dt);
        }
    }
}

impl Component for LevelManager {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.setup_world(ctx);
        ui::build_hud(ctx, self);
        ui::build_overlays(ctx);
        self.hud_fireflies(ctx);
        self.hud_hearts(ctx, MAX_HP);
        ctx.set_time_scale(1.0);
        sfx::music(ctx, &self.music.clone(), 1.2);
        // заставка с названием
        ui::set_text(ctx, "CardNumber", &format!("Уровень {}", self.index + 1));
        ui::set_text(ctx, "CardTitle", &self.title.clone());
        ui::set_text(ctx, "CardSub", &self.subtitle.clone());
        if let Some(card) = ctx.find_by_name("Card") {
            ctx.start_tween(Tween::field(card, "CanvasGroup.alpha", 0.0f32).from(1.0f32).duration(0.8).delay(2.4).unscaled());
            ctx.destroy_after(card, 3.4);
        }
        self.banner_timer = 0.0;
        self.drive_camera(ctx, 1.0);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let real_dt = ctx.time().unscaled_delta;

        // касание экрана → показать экранные кнопки
        if !self.touch_seen && controls::read_touch(ctx.input()).any {
            self.touch_seen = true;
            ui::show(ctx, "Touch", true);
        }

        // клики интерфейса: звук и действия
        let clicked: Vec<String> = ctx
            .ui()
            .events()
            .iter()
            .filter(|e| matches!(e.kind, UiEventKind::Click))
            .map(|e| e.name.clone())
            .collect();
        for name in &clicked {
            match name.as_str() {
                "pause_btn" => self.pause(ctx, true),
                "resume" => self.pause(ctx, false),
                "restart" => {
                    sfx::play_ui(ctx, "click");
                    let scene = self.info().scene;
                    self.leave(ctx, scene);
                }
                "menu" => {
                    sfx::play_ui(ctx, "click");
                    self.leave(ctx, "menu");
                }
                "next" => {
                    sfx::play_ui(ctx, "confirm");
                    let next = (self.index.max(0) as usize + 1).min(LEVEL_COUNT - 1);
                    self.leave(ctx, LEVELS[next].scene);
                }
                "retry_cp" => {
                    sfx::play_ui(ctx, "confirm");
                    self.retry_from_checkpoint(ctx);
                }
                _ => {}
            }
        }

        // пауза с клавиатуры / геймпада
        if ctx.input().button_down("Pause") {
            match self.state {
                LevelState::Playing => self.pause(ctx, true),
                LevelState::Paused => self.pause(ctx, false),
                _ => {}
            }
        }

        match self.state {
            LevelState::Playing => {
                self.time += dt;
            }
            LevelState::Won | LevelState::Lost => {
                self.end_timer -= real_dt;
                if self.end_timer <= 0.0 {
                    self.show_results(ctx);
                    self.end_timer = 1e9;
                }
            }
            LevelState::Paused => {}
        }

        // HUD
        let second = self.time as i32;
        if second != self.shown_second {
            self.shown_second = second;
            ui::set_text(ctx, "TimerText", &format_time(self.time));
        }
        if let Some(hp) = Self::player_id(ctx).and_then(|id| ctx.get_on::<Player>(id).map(|p| p.hp)) {
            self.hud_hearts(ctx, hp.max(0));
        }
        if self.banner_timer > 0.0 {
            self.banner_timer -= real_dt;
            if self.banner_timer <= 0.0 {
                ui::show(ctx, "Banner", false);
            }
        }
        if !self.exit_announced && self.exit_ready() {
            self.exit_announced = true;
            let text = if self.boss_present { "Хозяин Тьмы повержен! Выход открыт." } else { "Выход открыт! Ищите светящуюся арку." };
            self.banner(ctx, text, 4.0);
        }
    }

    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.drive_camera(ctx, dt.max(1e-4));
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        match (message.name.as_str(), &message.value) {
            ("firefly", _) => {
                self.fireflies += 1;
                self.hud_fireflies(ctx);
            }
            ("golden", _) => {
                self.golden = true;
                self.hud_fireflies(ctx);
                self.banner(ctx, "Золотой светлячок найден!", 2.5);
            }
            ("enemy_killed", _) => self.kills += 1,
            ("checkpoint", Value::Vec3(x, y, _)) => {
                self.checkpoint = Vec2::new(*x, *y);
                self.banner(ctx, "Контрольная точка", 1.6);
            }
            ("player_dead", _) => self.lose(ctx),
            ("player_hurt", _) => {}
            ("exit_reached", _) => self.win(ctx),
            ("hint", Value::Str(text)) => self.banner(ctx, text, 6.0),
            ("need_more", Value::Int(n)) => {
                let text = format!("Нужно светлячков: {} (собрано {})", n, self.fireflies);
                self.banner(ctx, &text, 2.5);
            }
            ("exit_locked", _) => {
                let text = if self.boss_present && !self.boss_dead && self.fireflies >= self.need {
                    "Сначала победите Хозяина Тьмы!".to_string()
                } else {
                    format!("Выход закрыт: нужно светлячков {} (собрано {})", self.need, self.fireflies)
                };
                self.banner(ctx, &text, 3.0);
            }
            ("boss_start", Value::Vec3(hp, x, y)) => {
                self.boss_hp = *hp as i32;
                // ворота арены закрыты: возрождаться теперь нужно внутри
                self.checkpoint = Vec2::new(*x, *y);
                ui::show(ctx, "BossBar", true);
                ui::set_progress(ctx, "BossHp", 1.0);
                self.banner(ctx, "Прыгайте на Хозяина сверху! Берегитесь тарана.", 4.0);
            }
            ("boss_hp", Value::Int(hp)) => {
                self.boss_hp = *hp as i32;
                ui::set_progress(ctx, "BossHp", *hp as f32 / crate::boss::BOSS_HP as f32);
            }
            ("boss_defeated", _) => {
                self.boss_dead = true;
                ui::show(ctx, "BossBar", false);
                sfx::music(ctx, &self.music.clone(), 1.5);
            }
            _ => {}
        }
    }
}

/// Игрок уровня: для тестов и снимков.
pub fn player_of(app: &App) -> Option<GameObjectId> {
    app.world.scene.find_by_tag("Player")
}
