//! Интерфейс «Роя» на `engine-ui`: HUD, главное меню, выбор режима, настройки, пауза,
//! выбор улучшения, результаты и экранные стики для касаний.
//!
//! Экраны — отдельные холсты; показывается один за раз (`Ui::show`), поэтому навигация
//! клавиатурой и геймпадом ходит только по кнопкам текущего экрана.

use engine::prelude::*;

use crate::data::*;
use crate::profile::{Options, RunRecord};
use crate::sim::{Mode, Sim};

/// Экраны с кнопками (HUD живёт всегда, пока идёт игра).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    ModeSelect,
    Options,
    Playing,
    Paused,
    Upgrade,
    Results,
}

pub struct Card {
    pub root: GameObjectId,
    icon: GameObjectId,
    name: GameObjectId,
    level: GameObjectId,
    desc: GameObjectId,
}

pub struct Ui {
    // холсты экранов
    pub hud: GameObjectId,
    pub menu: GameObjectId,
    pub mode: GameObjectId,
    pub options: GameObjectId,
    pub pause: GameObjectId,
    pub upgrade: GameObjectId,
    pub results: GameObjectId,
    // HUD
    hp_text: GameObjectId,
    hp_bar: GameObjectId,
    core_text: GameObjectId,
    core_bar: GameObjectId,
    wave_text: GameObjectId,
    left_text: GameObjectId,
    boss_name: GameObjectId,
    boss_bar: GameObjectId,
    score_text: GameObjectId,
    mult_text: GameObjectId,
    combo_bar: GameObjectId,
    xp_bar: GameObjectId,
    level_text: GameObjectId,
    dash_bar: GameObjectId,
    dash_text: GameObjectId,
    banner: GameObjectId,
    banner_sub: GameObjectId,
    vignette: GameObjectId,
    pub pause_btn: GameObjectId,
    pub dash_btn: GameObjectId,
    touch_nodes: [GameObjectId; 4],
    // меню
    pub continue_btn: GameObjectId,
    continue_label: GameObjectId,
    menu_best: GameObjectId,
    // выбор режима
    pub endless_btn: GameObjectId,
    endless_sub: GameObjectId,
    board_text: GameObjectId,
    // настройки
    pub music_slider: GameObjectId,
    pub sfx_slider: GameObjectId,
    music_label: GameObjectId,
    sfx_label: GameObjectId,
    pub shake_toggle: GameObjectId,
    pub numbers_toggle: GameObjectId,
    // улучшения
    pub cards: [Card; 3],
    upgrade_sub: GameObjectId,
    // результаты
    res_title: GameObjectId,
    res_mode: GameObjectId,
    res_stats: GameObjectId,
    res_record: GameObjectId,
    icons: Vec<AssetHandle<Texture>>,
    pub vignette_tex: AssetHandle<Texture>,
    last_vignette: f32,
}

// ---------------------------------------------------------------------------
// Палитра
// ---------------------------------------------------------------------------

fn cyan() -> Color {
    Color::hex(0x5df0ff)
}
fn muted() -> Color {
    Color::hex(0x8fa3c7)
}
fn amber() -> Color {
    Color::hex(0xffb02e)
}
fn ink() -> Color {
    Color::rgba(0.0, 0.0, 0.02, 0.92)
}
fn panel_color() -> Color {
    Color::hex(0x0b1226).with_alpha(0.93)
}

fn style_button(scene: &mut Scene, id: GameObjectId, primary: bool) {
    if let Some(b) = scene.get_component_mut::<Button>(id) {
        if primary {
            b.normal_color = Color::hex(0x1f7fa0);
            b.hover_color = Color::hex(0x2aa6cc);
            b.pressed_color = Color::hex(0x176682);
        } else {
            b.normal_color = Color::hex(0x16233f);
            b.hover_color = Color::hex(0x24407a);
            b.pressed_color = Color::hex(0x0f1a33);
        }
        b.disabled_color = Color::hex(0x131a2b);
    }
    if let Some(i) = scene.get_component_mut::<Image>(id) {
        i.corner_radius = 12.0;
        i.border_width = 1.5;
        i.border_color = if primary { cyan().with_alpha(0.9) } else { Color::hex(0x3a5a96).with_alpha(0.9) };
    }
}

fn panel(scene: &mut Scene, parent: GameObjectId) -> GameObjectId {
    let id = scene.ui_panel(parent).id();
    if let Some(i) = scene.get_component_mut::<Image>(id) {
        i.color = panel_color();
        i.corner_radius = 18.0;
        i.border_color = Color::hex(0x2c4a86).with_alpha(0.9);
        i.border_width = 1.5;
        i.shadow_size = 36.0;
    }
    id
}

fn title(scene: &mut Scene, parent: GameObjectId, text: &str, size: f32) -> GameObjectId {
    scene.ui_text(parent, text).font_size(size).bold().align(TextAlign::Center).text_color(cyan()).outline(2.0, ink()).id()
}

fn label(scene: &mut Scene, parent: GameObjectId, text: &str, size: f32, color: Color) -> GameObjectId {
    scene.ui_text(parent, text).font_size(size).text_color(color).align(TextAlign::Center).id()
}

/// Кнопка с крупным заголовком и мелким пояснением.
#[allow(clippy::too_many_arguments)]
fn big_button(scene: &mut Scene, parent: GameObjectId, msg: &str, head: &str, sub: &str, w: f32, h: f32, primary: bool) -> (GameObjectId, GameObjectId) {
    let btn = scene.ui_button(parent, msg, head).size(w, h).id();
    style_button(scene, btn, primary);
    if let Some(l) = scene.find_child(btn, "Label") {
        if let Some(rt) = scene.get_component_mut::<RectTransform>(l) {
            rt.anchor_min = Vec2::new(0.0, 0.1);
            rt.anchor_max = Vec2::new(1.0, 0.58);
            rt.size = Vec2::ZERO;
            rt.position = Vec2::ZERO;
        }
        if let Some(t) = scene.get_component_mut::<Text>(l) {
            t.size = 28.0;
        }
    }
    let sub_id = scene
        .ui_text(btn, sub)
        .anchor(Anchor::Center)
        .size(w - 30.0, 44.0)
        .font_size(18.0)
        .text_color(muted())
        .align(TextAlign::Center)
        .id();
    if let Some(rt) = scene.get_component_mut::<RectTransform>(sub_id) {
        rt.anchor_min = Vec2::new(0.0, 0.56);
        rt.anchor_max = Vec2::new(1.0, 0.92);
        rt.size = Vec2::ZERO;
        rt.position = Vec2::ZERO;
    }
    (btn, sub_id)
}

fn canvas(scene: &mut Scene, name: &str, order: i32, navigation: bool) -> GameObjectId {
    let mut node = scene.ui_canvas().name(name).sort_order(order);
    if !navigation {
        node = node.no_navigation();
    }
    let id = node.with(CanvasGroup::default()).id();
    id
}

fn dim(scene: &mut Scene, parent: GameObjectId, alpha: f32) {
    scene.ui_image(parent, Color::rgba(0.0, 0.01, 0.04, alpha)).name("Dim").stretch();
}

pub fn set_text(scene: &mut Scene, id: GameObjectId, text: &str) {
    if let Some(t) = scene.get_component_mut::<Text>(id) {
        t.set(text);
    }
}

fn set_bar(scene: &mut Scene, id: GameObjectId, v: f32) {
    if let Some(b) = scene.get_component_mut::<ProgressBar>(id) {
        let v = v.clamp(0.0, 1.0);
        if (b.value - v).abs() > 0.0005 {
            b.value = v;
        }
    }
}

fn set_bar_color(scene: &mut Scene, id: GameObjectId, c: Color) {
    if let Some(b) = scene.get_component_mut::<ProgressBar>(id) {
        b.fill_color = c;
    }
}

fn set_text_color(scene: &mut Scene, id: GameObjectId, c: Color) {
    if let Some(t) = scene.get_component_mut::<Text>(id) {
        t.color = c;
    }
}

/// Число с пробелами между тысячами: 12 345.
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{a0}');
        }
        out.push(ch);
    }
    out
}

pub fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

// ---------------------------------------------------------------------------
// Построение
// ---------------------------------------------------------------------------

impl Ui {
    pub fn build(scene: &mut Scene, db: &AssetDatabase) -> Ui {
        let icons: Vec<AssetHandle<Texture>> = Up::ALL
            .iter()
            .map(|u| db.handle::<Texture>(&format!("ui/icons/{}.png", u.info().key)).unwrap_or_else(|_| AssetHandle::none()))
            .collect();
        let vignette_tex = db.handle::<Texture>("ui/vignette.png").unwrap_or_else(|_| AssetHandle::none());

        // ---------------- HUD ----------------
        let hud = canvas(scene, "HUD", 0, false);
        let vignette = scene.ui_image(hud, Color::rgba(1.0, 1.0, 1.0, 0.0)).name("LowHealth").stretch().id();
        if let Some(i) = scene.get_component_mut::<Image>(vignette) {
            i.texture = vignette_tex;
            i.raycast_target = false;
        }
        let left = panel(scene, hud);
        scene.ui_edit(left).name("StatusPanel").anchor(Anchor::TopLeft).pos(16.0, 14.0).size(300.0, 100.0);
        if let Some(i) = scene.get_component_mut::<Image>(left) {
            i.color = Color::hex(0x0b1226).with_alpha(0.7);
            i.corner_radius = 12.0;
            i.shadow_size = 0.0;
            i.raycast_target = false;
        }
        let hp_text = scene.ui_text(left, "ДРОН").name("HpText").anchor(Anchor::TopLeft).pos(14.0, 8.0).size(272.0, 22.0).font_size(17.0).text_color(Color::hex(0x9ff5ba)).no_wrap().id();
        let hp_bar = scene.ui_progress(left, 1.0).name("HpBar").anchor(Anchor::TopLeft).pos(14.0, 32.0).size(272.0, 12.0).color(Color::hex(0x4be07a)).id();
        let core_text = scene.ui_text(left, "РЕАКТОР").name("CoreText").anchor(Anchor::TopLeft).pos(14.0, 54.0).size(272.0, 22.0).font_size(17.0).text_color(Color::hex(0xffcf7a)).no_wrap().id();
        let core_bar = scene.ui_progress(left, 1.0).name("CoreBar").anchor(Anchor::TopLeft).pos(14.0, 78.0).size(272.0, 12.0).color(Color::hex(0xff9a2e)).id();

        let wave_text = scene.ui_text(hud, "ВОЛНА 1").name("WaveText").anchor(Anchor::Top).pos(0.0, 12.0).size(420.0, 44.0).font_size(34.0).bold().align(TextAlign::Center).outline(2.5, ink()).no_wrap().id();
        let left_text = scene.ui_text(hud, "").name("LeftText").anchor(Anchor::Top).pos(0.0, 54.0).size(420.0, 26.0).font_size(19.0).text_color(muted()).align(TextAlign::Center).outline(2.0, ink()).no_wrap().id();
        let boss_name = scene.ui_text(hud, "").name("BossName").anchor(Anchor::Top).pos(0.0, 86.0).size(520.0, 26.0).font_size(21.0).bold().text_color(Color::hex(0xff7a8a)).align(TextAlign::Center).outline(2.0, ink()).no_wrap().hidden().id();
        let boss_bar = scene.ui_progress(hud, 1.0).name("BossBar").anchor(Anchor::Top).pos(0.0, 114.0).size(520.0, 16.0).color(Color::hex(0xff3d5a)).hidden().id();

        let score_text = scene.ui_text(hud, "0").name("ScoreText").anchor(Anchor::TopRight).pos(-84.0, 10.0).size(300.0, 44.0).font_size(34.0).bold().align(TextAlign::Right).outline(2.5, ink()).no_wrap().id();
        let mult_text = scene.ui_text(hud, "×1.0").name("MultText").anchor(Anchor::TopRight).pos(-84.0, 52.0).size(300.0, 28.0).font_size(22.0).bold().text_color(amber()).align(TextAlign::Right).outline(2.0, ink()).no_wrap().id();
        let combo_bar = scene.ui_progress(hud, 0.0).name("ComboBar").anchor(Anchor::TopRight).pos(-84.0, 84.0).size(150.0, 6.0).color(amber()).id();
        let pause_btn = scene.ui_button(hud, "pause", "II").anchor(Anchor::TopRight).pos(-16.0, 14.0).size(54.0, 54.0).id();
        style_button(scene, pause_btn, false);

        let xp_bar = scene.ui_progress(hud, 0.0).name("XpBar").anchor(Anchor::StretchBottom).pos(0.0, -8.0).size(-32.0, 12.0).color(Color::hex(0x4dffb0)).id();
        let level_text = scene.ui_text(hud, "").name("LevelText").anchor(Anchor::BottomLeft).pos(18.0, -26.0).size(420.0, 24.0).font_size(18.0).text_color(Color::hex(0x9ffbd2)).outline(2.0, ink()).no_wrap().id();
        let dash_bar = scene.ui_progress(hud, 1.0).name("DashBar").anchor(Anchor::Bottom).pos(0.0, -32.0).size(220.0, 10.0).color(cyan()).id();
        let dash_text = scene.ui_text(hud, "РЫВОК").name("DashText").anchor(Anchor::Bottom).pos(0.0, -46.0).size(220.0, 20.0).font_size(15.0).text_color(cyan()).align(TextAlign::Center).outline(2.0, ink()).no_wrap().id();
        let banner = scene.ui_text(hud, "").name("Banner").anchor(Anchor::Center).pos(0.0, -230.0).size(1000.0, 90.0).font_size(64.0).bold().align(TextAlign::Center).outline(4.0, ink()).no_wrap().id();
        let banner_sub = scene.ui_text(hud, "").name("BannerSub").anchor(Anchor::Center).pos(0.0, -168.0).size(1000.0, 36.0).font_size(26.0).text_color(muted()).align(TextAlign::Center).outline(3.0, ink()).no_wrap().id();
        for id in [banner, banner_sub] {
            if let Some(t) = scene.get_component_mut::<Text>(id) {
                t.color = t.color.with_alpha(0.0);
            }
        }
        // Экранные стики и кнопка рывка (показываются только после первого касания).
        let mut touch_nodes = [GameObjectId::from_bits(0); 4];
        for (i, (size, alpha)) in [(150.0, 0.18), (64.0, 0.45), (150.0, 0.18), (64.0, 0.45)].into_iter().enumerate() {
            let id = scene.ui_image(hud, Color::rgba(0.4, 0.8, 1.0, alpha)).name("Stick").anchor(Anchor::TopLeft).size(size, size).radius(size / 2.0).hidden().id();
            if let Some(img) = scene.get_component_mut::<Image>(id) {
                img.raycast_target = false;
                img.border_width = 2.0;
                img.border_color = cyan().with_alpha(0.6);
            }
            touch_nodes[i] = id;
        }
        let dash_btn = scene.ui_image(hud, Color::rgba(0.4, 0.8, 1.0, 0.25)).name("DashButton").anchor(Anchor::BottomRight).pos(-70.0, -170.0).size(92.0, 92.0).radius(46.0).hidden().id();
        scene.ui_text(dash_btn, "РЫВОК").font_size(18.0).bold().align(TextAlign::Center).no_wrap().anchor(Anchor::Center);
        if let Some(img) = scene.get_component_mut::<Image>(dash_btn) {
            img.raycast_target = false;
            img.border_width = 2.0;
            img.border_color = cyan().with_alpha(0.7);
        }
        scene.ui_set_visible(hud, false);

        // ---------------- главное меню ----------------
        let menu = canvas(scene, "MenuScreen", 10, true);
        dim(scene, menu, 0.38);
        title_big(scene, menu);
        let col = scene.ui_column(menu, 14.0).anchor(Anchor::Center).pos(0.0, 70.0).width(380.0).fit().id();
        let continue_btn = scene.ui_button(col, "continue", "Продолжить").size(380.0, 60.0).primary().id();
        style_button(scene, continue_btn, true);
        let continue_label = scene.find_child(continue_btn, "Label").unwrap_or(continue_btn);
        let play = scene.ui_button(col, "play", "Играть").size(380.0, 60.0).id();
        style_button(scene, play, true);
        let opts = scene.ui_button(col, "options", "Настройки").size(380.0, 56.0).id();
        style_button(scene, opts, false);
        let quit = scene.ui_button(col, "quit", "Выход").size(380.0, 56.0).id();
        style_button(scene, quit, false);
        let menu_best = scene.ui_text(menu, "").name("Best").anchor(Anchor::Bottom).pos(0.0, -74.0).size(1000.0, 28.0).font_size(20.0).text_color(muted()).align(TextAlign::Center).outline(2.0, ink()).no_wrap().id();
        scene
            .ui_text(menu, "WASD — движение   ·   мышь — прицел   ·   ЛКМ — огонь   ·   Пробел — рывок   ·   Esc — пауза   ·   геймпад: левый стик, правый стик, RB")
            .name("Hint")
            .anchor(Anchor::Bottom)
            .pos(0.0, -36.0)
            .size(1240.0, 26.0)
            .font_size(17.0)
            .text_color(Color::hex(0x6d82ad))
            .align(TextAlign::Center)
            .outline(2.0, ink())
            .no_wrap();
        scene.ui_set_visible(menu, false);

        // ---------------- выбор режима ----------------
        let mode = canvas(scene, "ModeScreen", 10, true);
        dim(scene, mode, 0.55);
        title(scene, mode, "РЕЖИМ ИГРЫ", 52.0).clone_anchor(scene, Anchor::Top, 0.0, 44.0, 900.0, 70.0);
        let row = scene.ui_row(mode, 40.0).anchor(Anchor::Center).pos(0.0, 20.0).fit().no_stretch().child_align(Alignment::Top).id();
        let left_col = scene.ui_column(row, 18.0).size(440.0, 330.0).no_stretch().id();
        let (camp, _) = big_button(scene, left_col, "campaign", "Кампания", "20 волн, 4 босса — защити реактор", 440.0, 120.0, true);
        let _ = camp;
        let (endless_btn, endless_sub) = big_button(scene, left_col, "endless", "Бесконечный режим", "Откроется после 10-й волны кампании", 440.0, 120.0, false);
        let back = scene.ui_button(left_col, "back", "Назад").size(440.0, 56.0).id();
        style_button(scene, back, false);
        let board = panel(scene, row);
        scene.ui_edit(board).size(440.0, 330.0).column(6.0).padding(24.0);
        label(scene, board, "ЛУЧШИЕ ЗАБЕГИ", 24.0, cyan());
        let board_text = scene.ui_text(board, "").name("Board").font_size(21.0).align(TextAlign::Left).height(250.0).id();
        scene.ui_set_visible(mode, false);

        // ---------------- настройки ----------------
        let options = canvas(scene, "OptionsScreen", 20, true);
        dim(scene, options, 0.6);
        let p = panel(scene, options);
        scene.ui_edit(p).anchor(Anchor::Center).width(560.0).column(14.0).padding(34.0).fit();
        title(scene, p, "НАСТРОЙКИ", 44.0);
        let music_label = label(scene, p, "Музыка", 22.0, Color::hex(0xdbe6ff));
        let music_slider = scene.ui_slider(p, "music", 0.0, 1.0, 0.6).height(32.0).id();
        let sfx_label = label(scene, p, "Звуки", 22.0, Color::hex(0xdbe6ff));
        let sfx_slider = scene.ui_slider(p, "sfx", 0.0, 1.0, 0.8).height(32.0).id();
        let shake_toggle = scene.ui_toggle(p, "shake", "Тряска камеры", true).switch().height(38.0).id();
        let numbers_toggle = scene.ui_toggle(p, "numbers", "Числа урона", true).switch().height(38.0).id();
        let back = scene.ui_button(p, "options_back", "Готово").size(300.0, 56.0).primary().id();
        style_button(scene, back, true);
        scene.ui_set_visible(options, false);

        // ---------------- пауза ----------------
        let pause = canvas(scene, "PauseScreen", 20, true);
        dim(scene, pause, 0.62);
        let p = panel(scene, pause);
        scene.ui_edit(p).anchor(Anchor::Center).width(440.0).column(14.0).padding(34.0).fit();
        title(scene, p, "ПАУЗА", 50.0);
        let b = scene.ui_button(p, "resume", "Продолжить").size(360.0, 58.0).id();
        style_button(scene, b, true);
        let b = scene.ui_button(p, "restart", "Начать заново").size(360.0, 54.0).id();
        style_button(scene, b, false);
        let b = scene.ui_button(p, "pause_options", "Настройки").size(360.0, 54.0).id();
        style_button(scene, b, false);
        let b = scene.ui_button(p, "to_menu", "В главное меню").size(360.0, 54.0).id();
        style_button(scene, b, false);
        label(scene, p, "Esc / Start — вернуться в игру", 17.0, Color::hex(0x6d82ad));
        scene.ui_set_visible(pause, false);

        // ---------------- улучшения ----------------
        let upgrade = canvas(scene, "UpgradeScreen", 20, true);
        dim(scene, upgrade, 0.72);
        title(scene, upgrade, "ВЫБЕРИТЕ УЛУЧШЕНИЕ", 46.0).clone_anchor(scene, Anchor::Top, 0.0, 46.0, 1000.0, 64.0);
        let upgrade_sub = label(scene, upgrade, "", 24.0, muted());
        scene.ui_edit(upgrade_sub).anchor(Anchor::Top).pos(0.0, 108.0).size(1000.0, 32.0);
        let row = scene.ui_row(upgrade, 30.0).anchor(Anchor::Center).pos(0.0, 34.0).fit().no_stretch().id();
        let mut cards: Vec<Card> = Vec::new();
        for i in 0..3 {
            let root = scene.ui_button(row, &format!("card{i}"), "").size(310.0, 410.0).id();
            style_button(scene, root, false);
            if let Some(l) = scene.find_child(root, "Label") {
                scene.ui_set_visible(l, false);
            }
            let icon = scene.ui_image(root, Color::WHITE).anchor(Anchor::Top).pos(0.0, 26.0).size(116.0, 116.0).id();
            if let Some(img) = scene.get_component_mut::<Image>(icon) {
                img.raycast_target = false;
            }
            let name = scene.ui_text(root, "").anchor(Anchor::Top).pos(0.0, 158.0).size(280.0, 34.0).font_size(26.0).bold().text_color(cyan()).align(TextAlign::Center).no_wrap().id();
            let level = scene.ui_text(root, "").anchor(Anchor::Top).pos(0.0, 196.0).size(280.0, 24.0).font_size(18.0).text_color(amber()).align(TextAlign::Center).no_wrap().id();
            let desc = scene.ui_text(root, "").anchor(Anchor::Top).pos(0.0, 232.0).size(268.0, 120.0).font_size(21.0).text_color(Color::hex(0xdbe6ff)).align(TextAlign::Center).valign(VerticalAlign::Top).id();
            scene.ui_text(root, &format!("[{}]", i + 1)).anchor(Anchor::Bottom).pos(0.0, -14.0).size(100.0, 24.0).font_size(18.0).text_color(Color::hex(0x6d82ad)).align(TextAlign::Center);
            cards.push(Card { root, icon, name, level, desc });
        }
        scene.ui_set_visible(upgrade, false);

        // ---------------- результаты ----------------
        let results = canvas(scene, "ResultsScreen", 20, true);
        dim(scene, results, 0.7);
        let p = panel(scene, results);
        scene.ui_edit(p).anchor(Anchor::Center).width(600.0).column(10.0).padding(34.0).fit();
        let res_title = title(scene, p, "РЕЗУЛЬТАТ", 50.0);
        let res_mode = label(scene, p, "", 21.0, muted());
        let res_stats = scene.ui_text(p, "").font_size(25.0).align(TextAlign::Center).height(190.0).id();
        if let Some(t) = scene.get_component_mut::<Text>(res_stats) {
            t.line_spacing = 1.2;
        }
        let res_record = label(scene, p, "", 24.0, amber());
        let b = scene.ui_button(p, "again", "Играть снова").size(360.0, 58.0).id();
        style_button(scene, b, true);
        let b = scene.ui_button(p, "results_menu", "В главное меню").size(360.0, 54.0).id();
        style_button(scene, b, false);
        scene.ui_set_visible(results, false);

        Ui {
            hud,
            menu,
            mode,
            options,
            pause,
            upgrade,
            results,
            hp_text,
            hp_bar,
            core_text,
            core_bar,
            wave_text,
            left_text,
            boss_name,
            boss_bar,
            score_text,
            mult_text,
            combo_bar,
            xp_bar,
            level_text,
            dash_bar,
            dash_text,
            banner,
            banner_sub,
            vignette,
            pause_btn,
            dash_btn,
            touch_nodes,
            continue_btn,
            continue_label,
            menu_best,
            endless_btn,
            endless_sub,
            board_text,
            music_slider,
            sfx_slider,
            music_label,
            sfx_label,
            shake_toggle,
            numbers_toggle,
            cards: [cards.remove(0), cards.remove(0), cards.remove(0)],
            upgrade_sub,
            res_title,
            res_mode,
            res_stats,
            res_record,
            icons,
            vignette_tex,
            last_vignette: -1.0,
        }
    }

    // -----------------------------------------------------------------------
    // Показ экранов
    // -----------------------------------------------------------------------

    /// Показать нужные холсты для экрана `screen`.
    pub fn show(&self, scene: &mut Scene, screen: Screen) {
        let all = [
            (self.menu, Screen::Menu),
            (self.mode, Screen::ModeSelect),
            (self.options, Screen::Options),
            (self.pause, Screen::Paused),
            (self.upgrade, Screen::Upgrade),
            (self.results, Screen::Results),
        ];
        for (id, s) in all {
            scene.ui_set_visible(id, s == screen);
        }
        let hud = matches!(screen, Screen::Playing | Screen::Paused);
        // Из главного меню (экран «Настройки» поверх меню) HUD не нужен.
        scene.ui_set_visible(self.hud, hud && screen != Screen::Options);
    }

    /// Плавное появление холста.
    pub fn fade_in(&self, ctx: &mut Ctx, canvas: GameObjectId) {
        if let Some(g) = ctx.scene_mut().get_component_mut::<CanvasGroup>(canvas) {
            g.alpha = 0.0;
        }
        ctx.start_tween(Tween::field(canvas, "CanvasGroup.alpha", 1.0f32).duration(0.22).ease(Ease::OutQuad).unscaled());
    }

    pub fn screen_canvas(&self, screen: Screen) -> Option<GameObjectId> {
        match screen {
            Screen::Menu => Some(self.menu),
            Screen::ModeSelect => Some(self.mode),
            Screen::Options => Some(self.options),
            Screen::Paused => Some(self.pause),
            Screen::Upgrade => Some(self.upgrade),
            Screen::Results => Some(self.results),
            Screen::Playing => None,
        }
    }

    // -----------------------------------------------------------------------
    // Обновление содержимого
    // -----------------------------------------------------------------------

    pub fn refresh_menu(&self, scene: &mut Scene, continue_wave: Option<u32>, best: &str) {
        match continue_wave {
            Some(w) => {
                scene.ui_set_visible(self.continue_btn, true);
                set_text(scene, self.continue_label, &format!("Продолжить (волна {w})"));
            }
            None => scene.ui_set_visible(self.continue_btn, false),
        }
        set_text(scene, self.menu_best, best);
    }

    pub fn refresh_mode(&self, scene: &mut Scene, endless_unlocked: bool, board: &[RunRecord]) {
        if let Some(b) = scene.get_component_mut::<Button>(self.endless_btn) {
            b.interactable = endless_unlocked;
        }
        set_text(
            scene,
            self.endless_sub,
            if endless_unlocked { "Без конца: волны растут, пока цел реактор" } else { "Откроется после 10-й волны кампании" },
        );
        let mut s = String::new();
        if board.is_empty() {
            s.push_str("Пока пусто. Сыграй первый забег!");
        }
        for (i, r) in board.iter().enumerate() {
            let tag = if r.mode == Mode::Endless { "бесконечный" } else if r.won { "победа" } else { "кампания" };
            s.push_str(&format!("{}.  {:>9}   волна {:>2}  {}  {}\n", i + 1, grouped(r.score), r.wave, clock(r.secs), tag));
        }
        set_text(scene, self.board_text, s.trim_end());
    }

    pub fn refresh_options(&self, scene: &mut Scene, o: &Options) {
        if let Some(s) = scene.get_component_mut::<Slider>(self.music_slider) {
            s.value = o.music;
        }
        if let Some(s) = scene.get_component_mut::<Slider>(self.sfx_slider) {
            s.value = o.sfx;
        }
        if let Some(t) = scene.get_component_mut::<Toggle>(self.shake_toggle) {
            t.is_on = o.shake;
        }
        if let Some(t) = scene.get_component_mut::<Toggle>(self.numbers_toggle) {
            t.is_on = o.numbers;
        }
        self.refresh_option_labels(scene, o);
    }

    pub fn refresh_option_labels(&self, scene: &mut Scene, o: &Options) {
        set_text(scene, self.music_label, &format!("Музыка: {:.0}%", o.music * 100.0));
        set_text(scene, self.sfx_label, &format!("Звуки: {:.0}%", o.sfx * 100.0));
    }

    /// Заполнить три карточки улучшений.
    pub fn fill_cards(&self, scene: &mut Scene, offer: &[Up], levels: &[u8; UP_COUNT], reason: &str) {
        set_text(scene, self.upgrade_sub, reason);
        for (i, card) in self.cards.iter().enumerate() {
            match offer.get(i) {
                Some(up) => {
                    let info = up.info();
                    let lvl = levels[up.index()];
                    scene.ui_set_visible(card.root, true);
                    set_text(scene, card.name, info.name);
                    set_text(scene, card.level, &if lvl == 0 { "НОВОЕ".to_string() } else { format!("Уровень {} → {}", lvl, lvl + 1) });
                    set_text(scene, card.desc, &up.describe(lvl + 1));
                    if let Some(img) = scene.get_component_mut::<Image>(card.icon) {
                        img.texture = self.icons[up.index()];
                    }
                }
                None => scene.ui_set_visible(card.root, false),
            }
        }
    }

    pub fn fill_results(&self, scene: &mut Scene, won: bool, sim: &Sim, record: Option<usize>, new_best: bool) {
        set_text(scene, self.res_title, if won { "ПОБЕДА!" } else { "РЕАКТОР УНИЧТОЖЕН" });
        set_text_color(scene, self.res_title, if won { Color::hex(0x7dffb0) } else { Color::hex(0xff6a7a) });
        set_text(
            scene,
            self.res_mode,
            &if sim.mode == Mode::Campaign { format!("Кампания — волна {} из {}", sim.wave, CAMPAIGN_WAVES) } else { format!("Бесконечный режим — волна {}", sim.wave) },
        );
        set_text(
            scene,
            self.res_stats,
            &format!(
                "Счёт: {}\nУничтожено врагов: {}\nВремя: {}\nЛучший множитель: ×{:.1}\nУлучшений взято: {}",
                grouped(sim.score),
                sim.run.kills,
                clock(sim.time as u32),
                sim.run.best_mult.max(1.0),
                sim.run.upgrades
            ),
        );
        let rec = if new_best {
            "Новый рекорд!".to_string()
        } else if let Some(p) = record {
            format!("Место в таблице лучших: {}", p + 1)
        } else {
            String::new()
        };
        set_text(scene, self.res_record, &rec);
    }

    // -----------------------------------------------------------------------
    // HUD
    // -----------------------------------------------------------------------

    pub fn update_hud(&mut self, scene: &mut Scene, sim: &Sim, boss_name: Option<(&str, f32)>, t: f32) {
        let p = &sim.player;
        set_text(
            scene,
            self.hp_text,
            &if p.alive { format!("ДРОН   {:.0} / {:.0}", p.hp.max(0.0).ceil(), sim.stats.max_hp) } else { format!("ДРОН   восстановление {:.0}", p.respawn_t.max(0.0).ceil()) },
        );
        set_bar(scene, self.hp_bar, if p.alive { p.hp / sim.stats.max_hp } else { 0.0 });
        let hp_frac = p.hp / sim.stats.max_hp;
        set_bar_color(scene, self.hp_bar, if hp_frac < 0.3 { Color::hex(0xff4d5e) } else { Color::hex(0x4be07a) });
        set_text(scene, self.core_text, &format!("РЕАКТОР   {:.0} / {:.0}", sim.core.hp.ceil(), sim.core.max_hp));
        let core_frac = sim.core.hp / sim.core.max_hp;
        set_bar(scene, self.core_bar, core_frac);
        set_bar_color(scene, self.core_bar, if core_frac < 0.3 { Color::hex(0xff4d5e) } else { Color::hex(0xff9a2e) });
        let wave_label = if sim.mode == Mode::Campaign { format!("ВОЛНА {} / {}", sim.wave.max(1), CAMPAIGN_WAVES) } else { format!("ВОЛНА {}", sim.wave.max(1)) };
        set_text(scene, self.wave_text, &wave_label);
        set_text(
            scene,
            self.left_text,
            &match sim.phase {
                crate::sim::Phase::Intermission => format!("Следующая волна через {:.0}", sim.intermission_t.max(0.0).ceil()),
                crate::sim::Phase::Cleared => "Волна пройдена".to_string(),
                _ => format!("Врагов осталось: {}", sim.remaining()),
            },
        );
        match boss_name {
            Some((name, frac)) => {
                scene.ui_set_visible(self.boss_name, true);
                scene.ui_set_visible(self.boss_bar, true);
                set_text(scene, self.boss_name, name);
                set_bar(scene, self.boss_bar, frac);
            }
            None => {
                scene.ui_set_visible(self.boss_name, false);
                scene.ui_set_visible(self.boss_bar, false);
            }
        }
        set_text(scene, self.score_text, &grouped(sim.score));
        set_text(scene, self.mult_text, &format!("×{:.1}", sim.mult()));
        set_bar(scene, self.combo_bar, if sim.combo_t > 0.0 { (sim.combo_t / 3.0).min(1.0) } else { 0.0 });
        set_bar(scene, self.xp_bar, sim.xp / sim.xp_need);
        set_text(scene, self.level_text, &format!("Уровень {}   ·   обломки {:.0} / {:.0}", sim.level, sim.xp.floor(), sim.xp_need.ceil()));
        let dash_ready = p.dash_cd <= 0.0;
        set_bar(scene, self.dash_bar, 1.0 - (p.dash_cd / sim.stats.dash_cooldown).clamp(0.0, 1.0));
        set_text_color(scene, self.dash_text, if dash_ready { cyan() } else { muted() });
        set_text(scene, self.dash_text, if dash_ready { "РЫВОК — ПРОБЕЛ" } else { "РЫВОК" });
        // Виньетка низкого здоровья.
        let danger = if p.alive { ((0.38 - hp_frac) / 0.38).clamp(0.0, 1.0) } else { 0.0 };
        let core_danger = ((0.3 - core_frac) / 0.3).clamp(0.0, 1.0);
        let d = danger.max(core_danger * 0.8);
        let a = if d > 0.0 { d * (0.45 + 0.25 * (t * 6.0).sin()) } else { 0.0 };
        if (a - self.last_vignette).abs() > 0.004 {
            self.last_vignette = a;
            if let Some(img) = scene.get_component_mut::<Image>(self.vignette) {
                img.color = Color::rgba(1.0, 1.0, 1.0, a);
            }
        }
    }

    /// Показать крупную надпись по центру и плавно погасить.
    pub fn banner(&self, ctx: &mut Ctx, text: &str, sub: &str, color: Color) {
        set_text(ctx.scene_mut(), self.banner, text);
        set_text(ctx.scene_mut(), self.banner_sub, sub);
        set_text_color(ctx.scene_mut(), self.banner, color);
        for id in [self.banner, self.banner_sub] {
            ctx.cancel_tweens_of(id);
            if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(id) {
                t.outline_color = t.outline_color.with_alpha(0.92);
            }
        }
        let sub_color = muted();
        set_text_color(ctx.scene_mut(), self.banner_sub, sub_color);
        for (id, delay) in [(self.banner, 1.4), (self.banner_sub, 1.4)] {
            ctx.start_tween(Tween::field(id, "Text.color.a", 0.0f32).from(1.0f32).duration(0.7).delay(delay).ease(Ease::InQuad));
            ctx.start_tween(Tween::field(id, "Text.outline_color.a", 0.0f32).from(0.92f32).duration(0.7).delay(delay).ease(Ease::InQuad));
        }
        ctx.start_tween(Tween::field(self.banner, "RectTransform.position", Vec2::new(0.0, -230.0)).from(Vec2::new(0.0, -200.0)).duration(0.35).ease(Ease::OutBack));
    }

    /// Положение экранных стиков (в пикселях экрана) и видимость.
    pub fn set_touch_ui(&self, scene: &mut Scene, scale: f32, visible: bool, left: Option<(Vec2, Vec2)>, right: Option<(Vec2, Vec2)>) {
        let [lb, lk, rb, rk] = self.touch_nodes;
        scene.ui_set_visible(self.dash_btn, visible);
        let place = |scene: &mut Scene, base: GameObjectId, knob: GameObjectId, v: Option<(Vec2, Vec2)>| {
            match v {
                Some((origin, current)) => {
                    scene.ui_set_visible(base, true);
                    scene.ui_set_visible(knob, true);
                    let o = origin / scale;
                    let c = current / scale;
                    if let Some(rt) = scene.get_component_mut::<RectTransform>(base) {
                        rt.pivot = Vec2::splat(0.5);
                        rt.position = o;
                    }
                    if let Some(rt) = scene.get_component_mut::<RectTransform>(knob) {
                        rt.pivot = Vec2::splat(0.5);
                        rt.position = c;
                    }
                }
                None => {
                    scene.ui_set_visible(base, false);
                    scene.ui_set_visible(knob, false);
                }
            }
        };
        place(scene, lb, lk, left);
        place(scene, rb, rk, right);
    }
}

fn title_big(scene: &mut Scene, menu: GameObjectId) {
    let t = scene.ui_text(menu, "РОЙ").name("Title").anchor(Anchor::Top).pos(0.0, 64.0).size(800.0, 150.0).font_size(128.0).bold().align(TextAlign::Center).text_color(cyan()).outline(5.0, ink()).no_wrap().id();
    if let Some(text) = scene.get_component_mut::<Text>(t) {
        text.shadow_offset = Vec2::new(0.0, 6.0);
    }
    if let Some(rt) = scene.get_component_mut::<RectTransform>(t) {
        rt.size.y = 160.0;
    }
    scene.ui_text(menu, "оборона реактора от взбесившихся машин").name("Subtitle").anchor(Anchor::Top).pos(0.0, 218.0).size(900.0, 36.0).font_size(28.0).text_color(Color::hex(0xb9c9ee)).align(TextAlign::Center).outline(3.0, ink()).no_wrap();
}

/// Небольшой помощник: перенести только что созданный текст в нужный угол.
trait CloneAnchor {
    fn clone_anchor(self, scene: &mut Scene, a: Anchor, x: f32, y: f32, w: f32, h: f32);
}

impl CloneAnchor for GameObjectId {
    fn clone_anchor(self, scene: &mut Scene, a: Anchor, x: f32, y: f32, w: f32, h: f32) {
        scene.ui_edit(self).anchor(a).pos(x, y).size(w, h);
    }
}
