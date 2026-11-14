//! Экраны главного меню: объекты, из которых печётся `assets/ui/main_menu.prefab`,
//! `level_select.prefab`, `options.prefab`, `help.prefab`.

use crate::ui_defs::{picture, GOLD};
use crate::Gfx;
use engine::prelude::*;

/// Строит все четыре экрана под временным холстом; возвращает их корни:
/// (главный, выбор уровня, настройки, управление).
pub fn screens(scene: &mut Scene, gfx: &Gfx) -> [GameObjectId; 4] {
    let canvas = scene.ui_canvas().name("MenuCanvas").sort_order(1).id();
        // ---------- главный экран ----------
        let main = scene.ui_node(canvas, "ScreenMain").stretch().id();
        picture(scene, main, gfx.tex("title_lantern"), "TitleLantern").anchor(Anchor::Top).pos(0.0, 22.0).size(210.0, 210.0);
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
        // «Продолжить» и «Уровни»: скрипт меню прячет «Продолжить», пока нет прогресса
        scene.ui_button(menu, "continue", "Продолжить").primary().height(50.0);
        scene.ui_button(menu, "play", "Уровни").height(50.0);
        scene.ui_button(menu, "options", "Настройки").height(50.0);
        scene.ui_button(menu, "help", "Управление").height(50.0);
        scene.ui_button(menu, "quit", "Выход").height(50.0);
        scene
            .ui_text(main, "Звёзд собрано: 0 из 18")
            .name("StarsTotal")
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
        for i in 0..6usize {
            let (col, row) = ((i % 3) as f32, (i / 3) as f32);
            let card = scene
                .ui_button(grid, &format!("level_{i}"), "")
                .anchor(Anchor::TopLeft)
                .pos(col * 375.0, row * 225.0)
                .size(360.0, 210.0)
                .id();
            scene
                .ui_text(card, &format!("{}. Уровень", i + 1))
                .name(format!("CardTitle{i}"))
                .anchor(Anchor::Top)
                .pos(0.0, 16.0)
                .size(340.0, 34.0)
                .font_size(27.0)
                .bold()
                .align(TextAlign::Center);
            scene
                .ui_text(card, "")
                .name(format!("CardBlurb{i}"))
                .anchor(Anchor::Top)
                .pos(0.0, 56.0)
                .size(330.0, 30.0)
                .font_size(18.0)
                .align(TextAlign::Center)
                .text_color(Color::rgba(0.78, 0.84, 1.0, 0.85));
            scene
                .ui_text(card, "")
                .name(format!("CardLock{i}"))
                .anchor(Anchor::Center)
                .pos(0.0, 30.0)
                .size(330.0, 28.0)
                .font_size(20.0)
                .align(TextAlign::Center)
                .text_color(Color::rgba(0.78, 0.84, 1.0, 0.7))
                .hidden();
            for s in 0..3u8 {
                picture(scene, card, gfx.tex("ui_star_empty"), &format!("CardStar{i}_{s}"))
                    .anchor(Anchor::Bottom)
                    .pos(-56.0 + s as f32 * 56.0, -52.0)
                    .size(50.0, 50.0);
            }
            scene
                .ui_text(card, "")
                .name(format!("CardTime{i}"))
                .anchor(Anchor::Bottom)
                .pos(0.0, -14.0)
                .size(330.0, 26.0)
                .font_size(18.0)
                .align(TextAlign::Center)
                .text_color(Color::rgba(0.9, 0.93, 1.0, 0.9));
        }
        scene.ui_button(levels, "back_levels", "Назад").anchor(Anchor::Bottom).pos(0.0, -22.0).size(260.0, 50.0);

        // ---------- настройки ----------
        let options = scene.ui_node(canvas, "ScreenOptions").stretch().hidden().id();
        let panel = scene.ui_panel(options).anchor(Anchor::Center).width(600.0).column(10.0).padding(30.0).fit().id();
        scene.ui_text(panel, "Настройки").font_size(44.0).bold().align(TextAlign::Center);
        scene.ui_text(panel, "Музыка: 60%").name("MusicLabel").font_size(24.0);
        scene.ui_slider(panel, "music_vol", 0.0, 100.0, 60.0).whole_numbers();
        scene.ui_text(panel, "Звуки: 80%").name("SfxLabel").font_size(24.0);
        scene.ui_slider(panel, "sfx_vol", 0.0, 100.0, 80.0).whole_numbers();
        scene.ui_toggle(panel, "shake", "Тряска камеры", true).switch();
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
    [main, levels, options, help]
}
