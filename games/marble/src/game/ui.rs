//! Интерфейс игры живёт в ассетах: `assets/ui/*.prefab` (меню, выбор цеха, настройки, HUD,
//! пауза, результаты, затемнение) вставлены в сцены как обычные объекты. Здесь — поиск их
//! частей по именам и мелкие помощники для текстов и цветов.

use engine::prelude::*;

// Палитра интерфейса: тёмная сталь и сигнальный янтарь (те же значения, что в префабах).
pub const AMBER: u32 = 0xffb020;
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
    pub title: Option<GameObjectId>,
    pub time: Option<GameObjectId>,
    pub gears: Option<GameObjectId>,
    pub medal: Option<GameObjectId>,
}

/// Части интерфейса, к которым обращается логика. Не найденное (в этой сцене нет такого
/// холста) — `None`: в меню нет HUD, в цехе нет списка цехов.
#[derive(Clone, Debug, Default)]
pub struct UiRefs {
    pub menu: Option<GameObjectId>,
    pub select: Option<GameObjectId>,
    pub options: Option<GameObjectId>,
    pub hud: Option<GameObjectId>,
    pub pause: Option<GameObjectId>,
    pub results: Option<GameObjectId>,
    pub fade: Option<GameObjectId>,

    pub menu_play_label: Option<GameObjectId>,
    pub cards: Vec<CardIds>,

    pub opt_music_text: Option<GameObjectId>,
    pub opt_sfx_text: Option<GameObjectId>,
    pub opt_sens_text: Option<GameObjectId>,
    pub opt_music: Option<GameObjectId>,
    pub opt_sfx: Option<GameObjectId>,
    pub opt_sens: Option<GameObjectId>,

    pub hud_course: Option<GameObjectId>,
    pub hud_time: Option<GameObjectId>,
    pub hud_target: Option<GameObjectId>,
    pub hud_gears: Option<GameObjectId>,
    pub hud_golden: Option<GameObjectId>,
    pub hud_checkpoint: Option<GameObjectId>,
    pub hud_speed: Option<GameObjectId>,
    pub hud_toast: Option<GameObjectId>,
    pub hud_count: Option<GameObjectId>,
    pub hud_hint: Option<GameObjectId>,

    pub res_title: Option<GameObjectId>,
    pub res_course: Option<GameObjectId>,
    pub res_medal: Option<GameObjectId>,
    pub res_medal_text: Option<GameObjectId>,
    pub res_time: Option<GameObjectId>,
    pub res_best: Option<GameObjectId>,
    pub res_gears: Option<GameObjectId>,
    pub res_golden: Option<GameObjectId>,
    pub res_falls: Option<GameObjectId>,
    pub res_next: Option<GameObjectId>,
}

fn first_child(scene: &Scene, id: Option<GameObjectId>) -> Option<GameObjectId> {
    scene.get(id?)?.children().first().copied()
}

impl UiRefs {
    /// Найти части интерфейса в сцене по именам объектов.
    pub fn resolve(scene: &Scene) -> Self {
        let by = |name: &str| scene.find_by_name(name);
        let menu_play = by("menu_play");
        let cards = (0..crate::game::data::COUNT)
            .filter_map(|i| {
                let button = by(&format!("course_{i}"))?;
                Some(CardIds {
                    button,
                    title: first_child(scene, Some(button)),
                    time: scene.find_child(button, "Time"),
                    gears: scene.find_child(button, "Gears"),
                    medal: scene.find_child(button, "Medal"),
                })
            })
            .collect();
        Self {
            menu: by("Menu"),
            select: by("Select"),
            options: by("Options"),
            hud: by("HUD"),
            pause: by("Pause"),
            results: by("Results"),
            fade: by("Fade"),
            menu_play_label: first_child(scene, menu_play),
            cards,
            opt_music_text: by("MusicText"),
            opt_sfx_text: by("SfxText"),
            opt_sens_text: by("SensText"),
            opt_music: by("opt_music"),
            opt_sfx: by("opt_sfx"),
            opt_sens: by("opt_sens"),
            hud_course: by("HudCourse"),
            hud_time: by("HudTime"),
            hud_target: by("HudTarget"),
            hud_gears: by("HudGears"),
            hud_golden: by("HudGolden"),
            hud_checkpoint: by("HudCheckpoint"),
            hud_speed: by("HudSpeed"),
            hud_toast: by("HudToast"),
            hud_count: by("HudCount"),
            hud_hint: by("HudHint"),
            res_title: by("ResTitle"),
            res_course: by("ResCourse"),
            res_medal: by("ResMedal"),
            res_medal_text: by("ResMedalText"),
            res_time: by("ResTime"),
            res_best: by("ResBest"),
            res_gears: by("ResGears"),
            res_golden: by("ResGolden"),
            res_falls: by("ResFalls"),
            res_next: by("res_next"),
        }
    }

    /// Подпись кнопки «Дальше» в результатах (первый ребёнок кнопки).
    pub fn res_next_label(&self, scene: &Scene) -> Option<GameObjectId> {
        first_child(scene, self.res_next)
    }
}

/// Поставить текст, если он изменился.
pub fn set_text(scene: &mut Scene, id: Option<GameObjectId>, s: &str) {
    let Some(id) = id else { return };
    if let Some(label) = scene.get_component_mut::<Text>(id) {
        if label.text != s {
            label.set(s);
        }
    }
}

/// Цвет текста.
pub fn set_text_color(scene: &mut Scene, id: Option<GameObjectId>, color: u32) {
    let Some(id) = id else { return };
    if let Some(label) = scene.get_component_mut::<Text>(id) {
        label.color = Color::hex(color);
    }
}

/// Цвет закрашенного изображения.
pub fn set_image_color(scene: &mut Scene, id: Option<GameObjectId>, color: Color) {
    let Some(id) = id else { return };
    if let Some(image) = scene.get_component_mut::<Image>(id) {
        image.color = color;
    }
}

/// Показать или спрятать холст (если он есть в сцене).
pub fn set_visible(scene: &mut Scene, id: Option<GameObjectId>, visible: bool) {
    if let Some(id) = id {
        scene.ui_set_visible(id, visible);
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
