//! Контроллер главного меню: экраны (главный, выбор уровня, настройки, управление) —
//! объекты сцены `menu.scene` из префабов `assets/ui/*.prefab`; скрипт заполняет их
//! данными (уровни из `data/levels.ron`, прогресс, настройки) и слушает кнопки.

use crate::common;
use crate::data::{format_time, GameData, Progress, LEVEL_COUNT};
use crate::sfx;
use crate::ui;
use engine::prelude::*;

/// Названия экранов (имена корневых объектов префабов).
const SCREENS: [&str; 4] = ["ScreenMain", "ScreenLevels", "ScreenOptions", "ScreenHelp"];

component! {
    /// Контроллер сцены `menu.scene`.
    pub struct MenuController {
        pub screen: String = String::from("ScreenMain"),
        pub dirty_save: bool = false,
    }
}

impl MenuController {
    fn go(&mut self, ctx: &mut Ctx, screen: &str) {
        self.screen = screen.to_string();
        for s in SCREENS {
            ui::show(ctx, s, s == screen);
        }
    }

    /// Заполнить экраны данными: подписи, звёзды, лучшее время, значения настроек.
    fn refresh(&mut self, ctx: &mut Ctx) {
        let progress = ctx.resource::<Progress>().cloned().unwrap_or_default();
        let data = ctx.resource::<GameData>().cloned();

        // главный экран: «Продолжить» есть только при прогрессе, «Играть» → «Уровни»
        let has = progress.has_progress();
        ui::show(ctx, "continue", has);
        ui::set_button_label(ctx, "play", if has { "Уровни" } else { "Играть" });
        if !has {
            ui::copy_button_style(ctx, "continue", "play");
        }
        ui::show(ctx, "quit", !cfg!(target_arch = "wasm32"));
        ui::set_text(ctx, "StarsTotal", &format!("Звёзд собрано: {} из {}", progress.total_stars(), LEVEL_COUNT * 3));

        // выбор уровня
        for i in 0..LEVEL_COUNT {
            let unlocked = i < progress.unlocked;
            let (name, blurb) = data.as_ref().map_or((String::new(), String::new()), |d| (d.level(i).name.clone(), d.level(i).blurb.clone()));
            ui::set_text(ctx, &format!("CardTitle{i}"), &format!("{}. {}", i + 1, name));
            ui::set_text(ctx, &format!("CardBlurb{i}"), if unlocked { &blurb } else { "Закрыто" });
            ui::set_interactable(ctx, &format!("level_{i}"), unlocked);
            ui::show(ctx, &format!("CardLock{i}"), !unlocked);
            ui::set_text(ctx, &format!("CardLock{i}"), &format!("Пройдите уровень {i}"));
            ui::show(ctx, &format!("CardTime{i}"), unlocked);
            let best = if progress.best_time[i] > 0.0 { format!("Лучшее время: {}", format_time(progress.best_time[i])) } else { "Ещё не пройден".to_string() };
            ui::set_text(ctx, &format!("CardTime{i}"), &best);
            for s in 0..3u8 {
                let name = format!("CardStar{i}_{s}");
                ui::show(ctx, &name, unlocked);
                let tex = common::tex(ctx, if s < progress.stars[i] { "ui_star" } else { "ui_star_empty" });
                ui::set_image_texture(ctx, &name, tex);
            }
        }

        // настройки
        ui::set_text(ctx, "MusicLabel", &format!("Музыка: {:.0}%", progress.music * 100.0));
        ui::set_text(ctx, "SfxLabel", &format!("Звуки: {:.0}%", progress.sfx * 100.0));
        ui::set_slider(ctx, "music_vol", progress.music * 100.0);
        ui::set_slider(ctx, "sfx_vol", progress.sfx * 100.0);
        ui::set_toggle(ctx, "shake", progress.shake);
    }

    fn set_volume(&mut self, ctx: &mut Ctx, music: bool, value: f32) {
        let value = value.clamp(0.0, 1.0);
        let (m, s) = {
            let p = ctx.resource_or_default::<Progress>();
            if music {
                p.music = value;
            } else {
                p.sfx = value;
            }
            (p.music, p.sfx)
        };
        if let Some(audio) = ctx.resource_mut::<Audio>() {
            sfx::apply_volumes(audio, m, s);
        }
        ui::set_text(ctx, if music { "MusicLabel" } else { "SfxLabel" }, &format!("{}: {:.0}%", if music { "Музыка" } else { "Звуки" }, value * 100.0));
        self.dirty_save = true;
    }

    /// Записать настройки на диск, если менялись.
    fn flush(&mut self, ctx: &mut Ctx) {
        if !self.dirty_save {
            return;
        }
        self.dirty_save = false;
        let progress = ctx.resource_or_default::<Progress>().clone();
        progress.store(ctx.save_data());
        if let Err(err) = ctx.save_data().save() {
            log::warn!("settings not saved: {err}");
        }
    }

    fn load_level(ctx: &mut Ctx, index: usize) {
        let scene = ctx.resource::<GameData>().map(|d| d.level(index).scene.clone());
        if let Some(scene) = scene {
            ctx.load_scene(&scene);
        }
    }
}

impl Component for MenuController {
    fn on_start(&mut self, ctx: &mut Ctx) {
        ctx.set_time_scale(1.0);
        self.refresh(ctx);
        self.go(ctx, "ScreenMain");
        sfx::music(ctx, "music_menu", 1.0);
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let clicked: Vec<String> = ctx
            .ui()
            .events()
            .iter()
            .filter(|e| matches!(e.kind, UiEventKind::Click))
            .map(|e| e.name.clone())
            .collect();
        for name in &clicked {
            sfx::play_ui(ctx, "click");
            match name.as_str() {
                "continue" => {
                    let next = ctx.resource::<Progress>().map_or(0, |p| p.next_level());
                    Self::load_level(ctx, next);
                }
                "play" => {
                    let has = ctx.resource::<Progress>().is_some_and(|p| p.has_progress());
                    if has {
                        self.go(ctx, "ScreenLevels");
                    } else {
                        Self::load_level(ctx, 0);
                    }
                }
                "options" => self.go(ctx, "ScreenOptions"),
                "help" => self.go(ctx, "ScreenHelp"),
                "quit" => ctx.quit(),
                "back_levels" | "back_options" | "back_help" => {
                    self.flush(ctx);
                    self.go(ctx, "ScreenMain");
                }
                other => {
                    if let Some(i) = other.strip_prefix("level_").and_then(|n| n.parse::<usize>().ok()) {
                        if i < LEVEL_COUNT {
                            Self::load_level(ctx, i);
                        }
                    }
                }
            }
        }

        // настройки
        if let Some(v) = ctx.ui().value_changed("music_vol") {
            self.set_volume(ctx, true, v / 100.0);
        }
        if let Some(v) = ctx.ui().value_changed("sfx_vol") {
            self.set_volume(ctx, false, v / 100.0);
            sfx::play(ctx, "jump");
        }
        if let Some(on) = ctx.ui().toggled("shake") {
            ctx.resource_or_default::<Progress>().shake = on;
            self.dirty_save = true;
        }

        // «Назад» с клавиатуры / геймпада
        if self.screen != "ScreenMain" && ctx.input().button_down("Cancel") {
            sfx::play_ui(ctx, "click");
            self.flush(ctx);
            self.go(ctx, "ScreenMain");
        }
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        self.flush(ctx);
    }
}
