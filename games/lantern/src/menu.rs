//! Главное меню: Играть / Продолжить, выбор уровня, настройки, управление, выход.

use crate::common::Gfx;
use crate::data::{format_time, Progress, LEVELS, LEVEL_COUNT};
use crate::sfx;
use crate::spawn;
use crate::ui::{self, GOLD};
use engine::prelude::*;

/// Названия экранов (имена корневых объектов).
const SCREENS: [&str; 4] = ["ScreenMain", "ScreenLevels", "ScreenOptions", "ScreenHelp"];

component! {
    /// Контроллер сцены `menu.scene`: строит экраны и слушает кнопки.
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

    fn build(&mut self, ctx: &mut Ctx) {
        let progress = ctx.resource::<Progress>().cloned().unwrap_or_default();
        let gfx = ctx.resource::<Gfx>().cloned().unwrap_or_default();
        let scene = ctx.scene_mut();
        let canvas = scene.ui_canvas().name("MenuCanvas").sort_order(1).id();

        // ---------- главный экран ----------
        let main = scene.ui_node(canvas, "ScreenMain").stretch().id();
        ui::picture(scene, main, gfx.tex("title_lantern"), "TitleLantern").anchor(Anchor::Top).pos(0.0, 22.0).size(210.0, 210.0);
        scene
            .ui_text(main, "Хранитель фонаря")
            .anchor(Anchor::Top)
            .pos(0.0, 226.0)
            .size(1100.0, 90.0)
            .font_size(74.0)
            .bold()
            .align(TextAlign::Center)
            .text_color(Color::hex(GOLD))
            .outline(4.0, Color::rgba(0.02, 0.03, 0.1, 0.95));
        scene
            .ui_text(main, "Маленький дух света спускается в тёмные пещеры")
            .anchor(Anchor::Top)
            .pos(0.0, 312.0)
            .size(1000.0, 34.0)
            .font_size(25.0)
            .align(TextAlign::Center)
            .text_color(Color::rgba(0.85, 0.9, 1.0, 0.9))
            .outline(3.0, Color::rgba(0.0, 0.0, 0.05, 0.9));
        let menu = scene.ui_node(main, "MainButtons").anchor(Anchor::Bottom).pos(0.0, -34.0).size(340.0, 280.0).column(10.0).id();
        if progress.has_progress() {
            scene.ui_button(menu, "continue", "Продолжить").primary().height(50.0);
            scene.ui_button(menu, "play", "Уровни").height(50.0);
        } else {
            scene.ui_button(menu, "play", "Играть").primary().height(50.0);
        }
        scene.ui_button(menu, "options", "Настройки").height(50.0);
        scene.ui_button(menu, "help", "Управление").height(50.0);
        if !cfg!(target_arch = "wasm32") {
            scene.ui_button(menu, "quit", "Выход").height(50.0);
        }
        scene
            .ui_text(main, &format!("Звёзд собрано: {} из {}", progress.total_stars(), LEVEL_COUNT * 3))
            .anchor(Anchor::BottomRight)
            .pos(-26.0, -20.0)
            .size(320.0, 28.0)
            .font_size(20.0)
            .align(TextAlign::Right)
            .text_color(Color::rgba(0.9, 0.92, 1.0, 0.8))
            .outline(3.0, Color::rgba(0.0, 0.0, 0.05, 0.9));

        // ---------- выбор уровня ----------
        let levels = scene.ui_node(canvas, "ScreenLevels").stretch().hidden().id();
        scene
            .ui_text(levels, "Выбор уровня")
            .anchor(Anchor::Top)
            .pos(0.0, 30.0)
            .size(900.0, 70.0)
            .font_size(54.0)
            .bold()
            .align(TextAlign::Center)
            .text_color(Color::hex(GOLD))
            .outline(4.0, Color::rgba(0.02, 0.03, 0.1, 0.95));
        let grid = scene.ui_node(levels, "LevelGrid").anchor(Anchor::Center).pos(0.0, 6.0).size(1110.0, 440.0).id();
        for (i, info) in LEVELS.iter().enumerate() {
            let (col, row) = ((i % 3) as f32, (i / 3) as f32);
            let unlocked = i < progress.unlocked;
            let card = scene
                .ui_button(grid, &format!("level_{i}"), "")
                .anchor(Anchor::TopLeft)
                .pos(col * 375.0, row * 225.0)
                .size(360.0, 210.0)
                .interactable(unlocked)
                .id();
            scene
                .ui_text(card, &format!("{}. {}", i + 1, info.name))
                .anchor(Anchor::Top)
                .pos(0.0, 16.0)
                .size(340.0, 34.0)
                .font_size(27.0)
                .bold()
                .align(TextAlign::Center);
            scene
                .ui_text(card, if unlocked { info.blurb } else { "Закрыто" })
                .anchor(Anchor::Top)
                .pos(0.0, 56.0)
                .size(330.0, 30.0)
                .font_size(18.0)
                .align(TextAlign::Center)
                .text_color(Color::rgba(0.78, 0.84, 1.0, 0.85));
            if !unlocked {
                scene
                    .ui_text(card, &format!("Пройдите уровень {i}"))
                    .anchor(Anchor::Center)
                    .pos(0.0, 30.0)
                    .size(330.0, 28.0)
                    .font_size(20.0)
                    .align(TextAlign::Center)
                    .text_color(Color::rgba(0.78, 0.84, 1.0, 0.7));
            }
            if unlocked {
                for s in 0..3u8 {
                    ui::picture(
                        scene,
                        card,
                        gfx.tex(if s < progress.stars[i] { "ui_star" } else { "ui_star_empty" }),
                        &format!("CardStar{i}_{s}"),
                    )
                    .anchor(Anchor::Bottom)
                    .pos(-56.0 + s as f32 * 56.0, -52.0)
                    .size(50.0, 50.0);
                }
                let best = if progress.best_time[i] > 0.0 { format!("Лучшее время: {}", format_time(progress.best_time[i])) } else { "Ещё не пройден".to_string() };
                scene
                    .ui_text(card, &best)
                    .anchor(Anchor::Bottom)
                    .pos(0.0, -14.0)
                    .size(330.0, 26.0)
                    .font_size(18.0)
                    .align(TextAlign::Center)
                    .text_color(Color::rgba(0.9, 0.93, 1.0, 0.9));
            }
        }
        scene.ui_button(levels, "back_levels", "Назад").anchor(Anchor::Bottom).pos(0.0, -22.0).size(260.0, 50.0);

        // ---------- настройки ----------
        let options = scene.ui_node(canvas, "ScreenOptions").stretch().hidden().id();
        let panel = scene.ui_panel(options).anchor(Anchor::Center).width(600.0).column(10.0).padding(30.0).fit().id();
        scene.ui_text(panel, "Настройки").font_size(44.0).bold().align(TextAlign::Center);
        scene.ui_text(panel, &format!("Музыка: {:.0}%", progress.music * 100.0)).name("MusicLabel").font_size(24.0);
        scene.ui_slider(panel, "music_vol", 0.0, 100.0, progress.music * 100.0).whole_numbers();
        scene.ui_text(panel, &format!("Звуки: {:.0}%", progress.sfx * 100.0)).name("SfxLabel").font_size(24.0);
        scene.ui_slider(panel, "sfx_vol", 0.0, 100.0, progress.sfx * 100.0).whole_numbers();
        scene.ui_toggle(panel, "shake", "Тряска камеры", progress.shake).switch();
        scene.ui_button(panel, "back_options", "Назад").primary();

        // ---------- управление ----------
        let help = scene.ui_node(canvas, "ScreenHelp").stretch().hidden().id();
        let panel = scene.ui_panel(help).anchor(Anchor::Center).width(780.0).column(10.0).padding(30.0).fit().id();
        scene.ui_text(panel, "Управление").font_size(44.0).bold().align(TextAlign::Center);
        for line in [
            "Клавиатура:  A / D или стрелки — бег,  Пробел / W / ↑ — прыжок,",
            "Shift / X — рывок (после 4-го уровня),  S / ↓ + прыжок — вниз сквозь «облачко»,  Esc / P — пауза.",
            "Геймпад:  левый стик или крестовина — бег,  A — прыжок,  X или RB — рывок,  Start — пауза.",
            "Касания:  кнопки на экране появляются после первого касания.",
            "Прыгайте на врагов сверху. Тяжеляков раздавить нельзя — перебегайте их рывком.",
            "Двойной прыжок открывается в конце 2-го уровня, рывок — в конце 4-го.",
        ] {
            scene.ui_text(panel, line).font_size(20.0).height(54.0);
        }
        scene.ui_button(panel, "back_help", "Назад").primary();
    }
}

impl Component for MenuController {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.build(ctx);
        ctx.set_time_scale(1.0);
        sfx::music(ctx, "music_menu", 1.0);
        // Парящие светлячки для настроения.
        for k in 0..16 {
            let x = -11.0 + (k as f32 * 7.31) % 22.0;
            let y = -5.0 + (k as f32 * 3.77) % 10.0;
            let _ = spawn::spawn(ctx, &format!("firefly {x:.2} {y:.2}"), false, false);
        }
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
                    ctx.load_scene(LEVELS[next].scene);
                }
                "play" => {
                    let has = ctx.resource::<Progress>().is_some_and(|p| p.has_progress());
                    if has {
                        self.go(ctx, "ScreenLevels");
                    } else {
                        ctx.load_scene(LEVELS[0].scene);
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
                            ctx.load_scene(LEVELS[i].scene);
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

impl MenuController {
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
}
