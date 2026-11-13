//! Экраны игры на engine-ui: главное меню, выбор цеха, настройки, HUD, пауза, результаты
//! и затемнение. Всё строится один раз; `game.rs` показывает и прячет холсты и обновляет тексты.

use engine::prelude::*;

use crate::courses::COUNT;

// Палитра интерфейса: тёмная сталь и сигнальный янтарь.
pub const AMBER: u32 = 0xffb020;
pub const STEEL: u32 = 0x2a3140;
pub const STEEL_HOVER: u32 = 0x3b4760;
pub const STEEL_PRESSED: u32 = 0x1b212c;
pub const INK: u32 = 0xe9eef7;
pub const MUTED: u32 = 0x9aa6bb;
pub const GOLD: u32 = 0xffc933;
pub const SILVER: u32 = 0xc9d3e3;
pub const BRONZE: u32 = 0xd98a52;
pub const NO_MEDAL: u32 = 0x4a5366;
pub const GREEN: u32 = 0x4cdf8d;
pub const RED: u32 = 0xff6a5a;

/// Карточка цеха в меню выбора.
#[derive(Clone, Copy, Debug)]
pub struct CardIds {
    pub button: GameObjectId,
    pub title: GameObjectId,
    pub time: GameObjectId,
    pub gears: GameObjectId,
    pub medal: GameObjectId,
}

/// Объекты интерфейса, к которым обращается логика.
#[derive(Clone, Debug)]
pub struct UiIds {
    pub menu: GameObjectId,
    pub select: GameObjectId,
    pub options: GameObjectId,
    pub hud: GameObjectId,
    pub pause: GameObjectId,
    pub results: GameObjectId,
    pub fade: GameObjectId,

    pub menu_play_label: GameObjectId,
    pub menu_hint: GameObjectId,
    pub cards: Vec<CardIds>,

    pub opt_music_text: GameObjectId,
    pub opt_sfx_text: GameObjectId,
    pub opt_sens_text: GameObjectId,
    pub opt_music: GameObjectId,
    pub opt_sfx: GameObjectId,
    pub opt_sens: GameObjectId,

    pub hud_course: GameObjectId,
    pub hud_time: GameObjectId,
    pub hud_target: GameObjectId,
    pub hud_gears: GameObjectId,
    pub hud_golden: GameObjectId,
    pub hud_checkpoint: GameObjectId,
    pub hud_speed: GameObjectId,
    pub hud_toast: GameObjectId,
    pub hud_count: GameObjectId,
    pub hud_hint: GameObjectId,

    pub res_title: GameObjectId,
    pub res_course: GameObjectId,
    pub res_medal: GameObjectId,
    pub res_medal_text: GameObjectId,
    pub res_time: GameObjectId,
    pub res_best: GameObjectId,
    pub res_gears: GameObjectId,
    pub res_golden: GameObjectId,
    pub res_falls: GameObjectId,
    pub res_next: GameObjectId,
}

fn button_colors(scene: &mut Scene, id: GameObjectId, primary: bool) {
    if let Some(button) = scene.get_component_mut::<Button>(id) {
        if primary {
            button.normal_color = Color::hex(0xc27a08);
            button.hover_color = Color::hex(0xe69a1e);
            button.pressed_color = Color::hex(0x9a5f05);
        } else {
            button.normal_color = Color::hex(STEEL);
            button.hover_color = Color::hex(STEEL_HOVER);
            button.pressed_color = Color::hex(STEEL_PRESSED);
        }
        button.disabled_color = Color::hex(0x1e232d);
    }
}

/// Кнопка в стиле игры.
fn button(scene: &mut Scene, parent: GameObjectId, message: &str, label: &str, primary: bool, w: f32, h: f32) -> GameObjectId {
    let id = scene.ui_button(parent, message, label).size(w, h).font_size(26.0).radius(10.0).id();
    button_colors(scene, id, primary);
    id
}

fn text(scene: &mut Scene, parent: GameObjectId, name: &str, s: &str, size: f32, color: u32) -> GameObjectId {
    scene.ui_text(parent, s).name(name).font_size(size).text_color(Color::hex(color)).id()
}

fn outlined(scene: &mut Scene, parent: GameObjectId, name: &str, s: &str, size: f32, color: u32) -> GameObjectId {
    scene
        .ui_text(parent, s)
        .name(name)
        .font_size(size)
        .bold()
        .text_color(Color::hex(color))
        .outline(2.5, Color::rgba(0.0, 0.0, 0.0, 0.85))
        .id()
}

fn panel(scene: &mut Scene, parent: GameObjectId) -> GameObjectId {
    scene
        .ui_panel(parent)
        .color(Color::rgba(0.07, 0.09, 0.13, 0.92))
        .radius(16.0)
        .border(2.0, Color::rgba(1.0, 0.69, 0.13, 0.35))
        .shadow(24.0)
        .id()
}

/// Построить все экраны. Скрытые холсты не получают ни кликов, ни фокуса.
pub fn build(scene: &mut Scene) -> UiIds {
    // ------------------------------------------------------------------
    // Главное меню
    // ------------------------------------------------------------------
    let menu = scene.ui_canvas().name("Menu").sort_order(10).id();
    scene
        .ui_text(menu, "ШАРОВОЙ ЗАВОД")
        .name("Title")
        .anchor(Anchor::TopLeft)
        .pos(72.0, 52.0)
        .size(760.0, 90.0)
        .font_size(76.0)
        .bold()
        .text_color(Color::hex(AMBER))
        .align(TextAlign::Left)
        .outline(3.0, Color::rgba(0.0, 0.0, 0.0, 0.85));
    scene
        .ui_text(menu, "Стальной шар. Заброшенный завод в небе. Доберитесь до площадки отгрузки.")
        .name("Subtitle")
        .anchor(Anchor::TopLeft)
        .pos(76.0, 168.0)
        .size(720.0, 64.0)
        .font_size(24.0)
        .text_color(Color::hex(INK))
        .align(TextAlign::Left)
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.85));
    let column = scene.ui_column(menu, 14.0).name("MenuColumn").anchor(Anchor::Left).pos(72.0, 70.0).width(380.0).fit().id();
    let play = button(scene, column, "menu_play", "Начать игру", true, 380.0, 62.0);
    let menu_play_label = scene.get(play).and_then(|o| o.children().first().copied()).unwrap_or(play);
    button(scene, column, "menu_select", "Выбор цеха", false, 380.0, 56.0);
    button(scene, column, "menu_options", "Настройки", false, 380.0, 56.0);
    button(scene, column, "menu_quit", "Выход", false, 380.0, 56.0);
    let menu_hint = scene
        .ui_text(menu, "WASD / стрелки / левый стик — катить   ·   Пробел / A — прыжок
Правая кнопка мыши, Q E или правый стик — камера   ·   R — на контрольную точку   ·   Esc / Start — пауза")
        .name("MenuHint")
        .anchor(Anchor::StretchBottom)
        .pos(0.0, -20.0)
        .size(-96.0, 58.0)
        .font_size(19.0)
        .text_color(Color::hex(INK))
        .align(TextAlign::Center)
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.9))
        .id();

    // ------------------------------------------------------------------
    // Выбор цеха
    // ------------------------------------------------------------------
    let select = scene.ui_canvas().name("Select").sort_order(11).id();
    scene.ui_image(select, Color::rgba(0.02, 0.03, 0.05, 0.78)).name("Dim").stretch();
    scene
        .ui_text(select, "Выбор цеха")
        .anchor(Anchor::Top)
        .pos(0.0, 36.0)
        .size(800.0, 70.0)
        .font_size(54.0)
        .bold()
        .align(TextAlign::Center)
        .text_color(Color::hex(AMBER));
    let grid = scene.ui_column(select, 22.0).name("Grid").anchor(Anchor::Center).pos(0.0, -4.0).width(1180.0).fit().id();
    let mut cards = Vec::new();
    for row in 0..2 {
        let line = scene.ui_row(grid, 20.0).no_stretch().height(150.0).id();
        for col in 0..4 {
            let index = row * 4 + col;
            let id = scene.ui_button(line, &format!("course_{index}"), &format!("{}. {}", index + 1, crate::courses::COURSES[index].name)).size(280.0, 150.0).font_size(23.0).radius(14.0).id();
            button_colors(scene, id, false);
            // Подпись цеха — вверху карточки.
            let title = scene.get(id).and_then(|o| o.children().first().copied()).unwrap_or(id);
            if let Some(rect) = scene.get_component_mut::<RectTransform>(title) {
                rect.anchor_min = Vec2::new(0.0, 0.0);
                rect.anchor_max = Vec2::new(1.0, 0.0);
                rect.pivot = Vec2::new(0.5, 0.0);
                rect.position = Vec2::new(0.0, 14.0);
                rect.size = Vec2::new(-24.0, 70.0);
            }
            let time = scene.ui_text(id, "").name("Time").anchor(Anchor::BottomLeft).pos(16.0, -14.0).size(150.0, 30.0).font_size(22.0).text_color(Color::hex(INK)).align(TextAlign::Left).id();
            let gears = scene.ui_text(id, "").name("Gears").anchor(Anchor::BottomRight).pos(-16.0, -14.0).size(150.0, 30.0).font_size(22.0).text_color(Color::hex(MUTED)).align(TextAlign::Right).id();
            let medal = scene.ui_image(id, Color::hex(NO_MEDAL)).name("Medal").anchor(Anchor::TopRight).pos(-14.0, 14.0).size(26.0, 26.0).radius(13.0).edit::<Image>(|i| i.raycast_target = false).id();
            cards.push(CardIds { button: id, title, time, gears, medal });
        }
    }
    let back = button(scene, select, "select_back", "Назад", false, 260.0, 56.0);
    if let Some(rect) = scene.get_component_mut::<RectTransform>(back) {
        rect.anchor_min = Vec2::new(0.5, 1.0);
        rect.anchor_max = Vec2::new(0.5, 1.0);
        rect.pivot = Vec2::new(0.5, 1.0);
        rect.position = Vec2::new(0.0, -40.0);
    }
    scene
        .ui_text(select, "Следующий цех открывается после финиша. Медаль даёт время: бронза, серебро, золото.")
        .anchor(Anchor::Bottom)
        .pos(0.0, -118.0)
        .size(1000.0, 34.0)
        .font_size(20.0)
        .align(TextAlign::Center)
        .text_color(Color::hex(MUTED));
    scene.ui_set_visible(select, false);

    // ------------------------------------------------------------------
    // Настройки
    // ------------------------------------------------------------------
    let options = scene.ui_canvas().name("Options").sort_order(12).id();
    scene.ui_image(options, Color::rgba(0.02, 0.03, 0.05, 0.78)).name("Dim").stretch();
    let box_ = panel(scene, options);
    scene.ui_edit(box_).anchor(Anchor::Center).width(720.0).column(18.0).padding(34.0).fit();
    scene.ui_text(box_, "Настройки").font_size(46.0).bold().align(TextAlign::Center).text_color(Color::hex(AMBER));
    let opt_music_text = text(scene, box_, "MusicText", "Музыка: 70%", 26.0, INK);
    let opt_music = scene.ui_slider(box_, "opt_music", 0.0, 1.0, 0.7).id();
    let opt_sfx_text = text(scene, box_, "SfxText", "Звуки: 80%", 26.0, INK);
    let opt_sfx = scene.ui_slider(box_, "opt_sfx", 0.0, 1.0, 0.8).id();
    let opt_sens_text = text(scene, box_, "SensText", "Чувствительность камеры: 100%", 26.0, INK);
    let opt_sens = scene.ui_slider(box_, "opt_sens", 0.3, 2.0, 1.0).id();
    button(scene, box_, "options_back", "Назад", true, 320.0, 58.0);
    scene.ui_set_visible(options, false);

    // ------------------------------------------------------------------
    // HUD
    // ------------------------------------------------------------------
    let hud = scene.ui_canvas().name("HUD").sort_order(5).no_navigation().id();
    let corner = scene.ui_panel(hud).anchor(Anchor::TopLeft).pos(20.0, 18.0).size(330.0, 104.0).color(Color::rgba(0.05, 0.07, 0.1, 0.62)).radius(14.0).id();
    let hud_course = text(scene, corner, "HudCourse", "", 21.0, MUTED);
    let hud_gears = text(scene, corner, "HudGears", "", 28.0, INK);
    let hud_golden = text(scene, corner, "HudGolden", "", 19.0, MUTED);
    set_rect(scene, hud_course, Anchor::TopLeft, 18.0, 10.0, 300.0, 26.0);
    set_rect(scene, hud_gears, Anchor::TopLeft, 18.0, 38.0, 300.0, 36.0);
    set_rect(scene, hud_golden, Anchor::TopLeft, 18.0, 74.0, 300.0, 24.0);
    let hud_time = outlined(scene, hud, "HudTime", "0:00.0", 56.0, INK);
    set_rect(scene, hud_time, Anchor::Top, 0.0, 14.0, 360.0, 70.0);
    let hud_target = outlined(scene, hud, "HudTarget", "", 22.0, GOLD);
    set_rect(scene, hud_target, Anchor::Top, 0.0, 84.0, 460.0, 30.0);
    let right = scene.ui_panel(hud).anchor(Anchor::TopRight).pos(-20.0, 18.0).size(330.0, 104.0).color(Color::rgba(0.05, 0.07, 0.1, 0.62)).radius(14.0).id();
    let hud_checkpoint = text(scene, right, "HudCheckpoint", "", 24.0, INK);
    set_rect(scene, hud_checkpoint, Anchor::TopRight, -18.0, 12.0, 300.0, 34.0);
    align(scene, hud_checkpoint, TextAlign::Right);
    let hud_speed = scene.ui_progress(right, 0.0).name("HudSpeed").anchor(Anchor::BottomRight).pos(-18.0, -16.0).size(294.0, 14.0).id();
    let speed_label = text(scene, right, "HudSpeedLabel", "Скорость", 18.0, MUTED);
    set_rect(scene, speed_label, Anchor::TopRight, -18.0, 46.0, 300.0, 24.0);
    align(scene, speed_label, TextAlign::Right);
    let hud_toast = outlined(scene, hud, "HudToast", "", 36.0, AMBER);
    set_rect(scene, hud_toast, Anchor::Center, 0.0, -170.0, 900.0, 50.0);
    align(scene, hud_toast, TextAlign::Center);
    let hud_count = outlined(scene, hud, "HudCount", "", 120.0, AMBER);
    set_rect(scene, hud_count, Anchor::Center, 0.0, -40.0, 800.0, 150.0);
    align(scene, hud_count, TextAlign::Center);
    let hud_hint = outlined(scene, hud, "HudHint", "WASD / стрелки — катить   ·   Пробел — прыжок   ·   ПКМ / Q E — камера   ·   R — вернуться на контрольную точку", 20.0, INK);
    set_rect(scene, hud_hint, Anchor::Bottom, 0.0, -22.0, 1200.0, 30.0);
    align(scene, hud_hint, TextAlign::Center);
    scene.ui_set_visible(hud, false);

    // ------------------------------------------------------------------
    // Пауза
    // ------------------------------------------------------------------
    let pause = scene.ui_canvas().name("Pause").sort_order(20).id();
    scene.ui_image(pause, Color::rgba(0.0, 0.0, 0.0, 0.6)).name("Dim").stretch();
    let pbox = panel(scene, pause);
    scene.ui_edit(pbox).anchor(Anchor::Center).width(460.0).column(14.0).padding(32.0).fit();
    scene.ui_text(pbox, "Пауза").font_size(52.0).bold().align(TextAlign::Center).text_color(Color::hex(AMBER));
    button(scene, pbox, "pause_resume", "Продолжить", true, 396.0, 58.0);
    button(scene, pbox, "pause_restart", "Заново", false, 396.0, 54.0);
    button(scene, pbox, "pause_select", "Выбор цеха", false, 396.0, 54.0);
    button(scene, pbox, "pause_menu", "В главное меню", false, 396.0, 54.0);
    scene.ui_set_visible(pause, false);

    // ------------------------------------------------------------------
    // Результаты
    // ------------------------------------------------------------------
    let results = scene.ui_canvas().name("Results").sort_order(20).id();
    scene.ui_image(results, Color::rgba(0.0, 0.0, 0.0, 0.55)).name("Dim").stretch();
    let rbox = panel(scene, results);
    scene.ui_edit(rbox).anchor(Anchor::Center).width(640.0).column(10.0).padding(34.0).fit();
    let res_title = scene.ui_text(rbox, "Цех пройден!").name("ResTitle").font_size(54.0).bold().align(TextAlign::Center).text_color(Color::hex(GREEN)).id();
    let res_course = text(scene, rbox, "ResCourse", "", 24.0, MUTED);
    align(scene, res_course, TextAlign::Center);
    let medal_row = scene.ui_row(rbox, 16.0).name("MedalRow").no_stretch().height(60.0).child_align(Alignment::Center).id();
    let res_medal = scene.ui_image(medal_row, Color::hex(GOLD)).name("ResMedal").size(52.0, 52.0).radius(26.0).id();
    let res_medal_text = scene.ui_text(medal_row, "Золото").name("ResMedalText").size(260.0, 52.0).font_size(36.0).bold().text_color(Color::hex(GOLD)).valign(VerticalAlign::Middle).align(TextAlign::Left).id();
    let res_time = text(scene, rbox, "ResTime", "", 30.0, INK);
    let res_best = text(scene, rbox, "ResBest", "", 24.0, MUTED);
    let res_gears = text(scene, rbox, "ResGears", "", 26.0, INK);
    let res_golden = text(scene, rbox, "ResGolden", "", 26.0, INK);
    let res_falls = text(scene, rbox, "ResFalls", "", 26.0, INK);
    for id in [res_time, res_best, res_gears, res_golden, res_falls] {
        align(scene, id, TextAlign::Center);
    }
    let buttons = scene.ui_row(rbox, 14.0).name("ResButtons").no_stretch().height(60.0).child_align(Alignment::Center).id();
    let res_next = button(scene, buttons, "res_next", "Дальше", true, 190.0, 58.0);
    button(scene, buttons, "res_retry", "Заново", false, 170.0, 58.0);
    button(scene, buttons, "res_menu", "В меню", false, 170.0, 58.0);
    scene.ui_set_visible(results, false);

    // ------------------------------------------------------------------
    // Затемнение для переходов
    // ------------------------------------------------------------------
    let fade = scene.ui_canvas().name("Fade").sort_order(50).no_navigation().id();
    scene.ui_image(fade, Color::rgba(0.0, 0.0, 0.0, 0.0)).name("FadeImage").stretch().edit::<Image>(|i| i.raycast_target = false);

    debug_assert_eq!(cards.len(), COUNT);
    UiIds {
        menu,
        select,
        options,
        hud,
        pause,
        results,
        fade,
        menu_play_label,
        menu_hint,
        cards,
        opt_music_text,
        opt_sfx_text,
        opt_sens_text,
        opt_music,
        opt_sfx,
        opt_sens,
        hud_course,
        hud_time,
        hud_target,
        hud_gears,
        hud_golden,
        hud_checkpoint,
        hud_speed,
        hud_toast,
        hud_count,
        hud_hint,
        res_title,
        res_course,
        res_medal,
        res_medal_text,
        res_time,
        res_best,
        res_gears,
        res_golden,
        res_falls,
        res_next,
    }
}

/// Выравнивание текста.
fn align(scene: &mut Scene, id: GameObjectId, align: TextAlign) {
    if let Some(label) = scene.get_component_mut::<Text>(id) {
        label.align = align;
    }
}

/// Прямоугольник объекта с якорем в одной точке.
fn set_rect(scene: &mut Scene, id: GameObjectId, anchor: Anchor, x: f32, y: f32, w: f32, h: f32) {
    if let Some(rect) = scene.get_component_mut::<RectTransform>(id) {
        *rect = RectTransform::anchored(anchor, Vec2::new(x, y), Vec2::new(w, h));
    }
}

/// Поставить текст, если он изменился.
pub fn set_text(scene: &mut Scene, id: GameObjectId, s: &str) {
    if let Some(label) = scene.get_component_mut::<Text>(id) {
        if label.text != s {
            label.set(s);
        }
    }
}

/// Цвет текста.
pub fn set_text_color(scene: &mut Scene, id: GameObjectId, color: u32) {
    if let Some(label) = scene.get_component_mut::<Text>(id) {
        label.color = Color::hex(color);
    }
}

/// Цвет закрашенного изображения.
pub fn set_image_color(scene: &mut Scene, id: GameObjectId, color: Color) {
    if let Some(image) = scene.get_component_mut::<Image>(id) {
        image.color = color;
    }
}

/// Цвет медали по номеру (0 — нет, 1 — бронза, 2 — серебро, 3 — золото).
pub fn medal_color(medal: u8) -> u32 {
    match medal {
        3 => GOLD,
        2 => SILVER,
        1 => BRONZE,
        _ => NO_MEDAL,
    }
}

/// Название медали по номеру.
pub fn medal_name(medal: u8) -> &'static str {
    match medal {
        3 => "Золото",
        2 => "Серебро",
        1 => "Бронза",
        _ => "Без медали",
    }
}

/// Время как `м:сс.д`.
pub fn fmt_time(seconds: f32) -> String {
    let s = seconds.max(0.0);
    let minutes = (s / 60.0).floor() as u32;
    format!("{}:{:04.1}", minutes, s - minutes as f32 * 60.0)
}
