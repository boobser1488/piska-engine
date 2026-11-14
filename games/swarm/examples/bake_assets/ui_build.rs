//! Сборка интерфейса «Роя» из кода — только для запекателя: он сохраняет каждый экран как
//! префаб `assets/ui/*.prefab`. В игре этого кода нет: экраны лежат в `arena.scene`
//! экземплярами префабов, а `ui::Ui::bind` находит их по именам.
#![allow(unused_variables, unused_mut, dead_code)]

use engine::prelude::*;
use swarm::data::Up;

struct Card {
    root: GameObjectId,
    icon: GameObjectId,
    name: GameObjectId,
    level: GameObjectId,
    desc: GameObjectId,
}

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
        .name("Sub")
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


pub fn build_all(scene: &mut Scene, db: &AssetDatabase) -> Vec<(&'static str, GameObjectId)> {
    let icons: Vec<AssetHandle<Texture>> = Up::ALL
        .iter()
        .map(|u| db.handle::<Texture>(u.info().icon).unwrap_or_else(|_| AssetHandle::none()))
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
        let id = scene.ui_image(hud, Color::rgba(0.4, 0.8, 1.0, alpha)).name(&format!("Stick{i}")).anchor(Anchor::TopLeft).size(size, size).radius(size / 2.0).hidden().id();
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
    scene.ui_edit(music_label).name("MusicLabel");
    let music_slider = scene.ui_slider(p, "music", 0.0, 1.0, 0.6).height(32.0).id();
    let sfx_label = label(scene, p, "Звуки", 22.0, Color::hex(0xdbe6ff));
    scene.ui_edit(sfx_label).name("SfxLabel");
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
    scene.ui_edit(upgrade_sub).name("UpgradeSub").anchor(Anchor::Top).pos(0.0, 108.0).size(1000.0, 32.0);
    let row = scene.ui_row(upgrade, 30.0).anchor(Anchor::Center).pos(0.0, 34.0).fit().no_stretch().id();
    let mut cards: Vec<Card> = Vec::new();
    for i in 0..3 {
        let root = scene.ui_button(row, &format!("card{i}"), "").size(310.0, 410.0).id();
        style_button(scene, root, false);
        if let Some(l) = scene.find_child(root, "Label") {
            scene.ui_set_visible(l, false);
        }
        let icon = scene.ui_image(root, Color::WHITE).name("Icon").anchor(Anchor::Top).pos(0.0, 26.0).size(116.0, 116.0).id();
        if let Some(img) = scene.get_component_mut::<Image>(icon) {
            img.raycast_target = false;
        }
        let name = scene.ui_text(root, "").name("Name").anchor(Anchor::Top).pos(0.0, 158.0).size(280.0, 34.0).font_size(26.0).bold().text_color(cyan()).align(TextAlign::Center).no_wrap().id();
        let level = scene.ui_text(root, "").name("Level").anchor(Anchor::Top).pos(0.0, 196.0).size(280.0, 24.0).font_size(18.0).text_color(amber()).align(TextAlign::Center).no_wrap().id();
        let desc = scene.ui_text(root, "").name("Desc").anchor(Anchor::Top).pos(0.0, 232.0).size(268.0, 120.0).font_size(21.0).text_color(Color::hex(0xdbe6ff)).align(TextAlign::Center).valign(VerticalAlign::Top).id();
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
    scene.ui_edit(res_title).name("ResTitle");
    let res_mode = label(scene, p, "", 21.0, muted());
    scene.ui_edit(res_mode).name("ResMode");
    let res_stats = scene.ui_text(p, "").name("ResStats").font_size(25.0).align(TextAlign::Center).height(190.0).id();
    if let Some(t) = scene.get_component_mut::<Text>(res_stats) {
        t.line_spacing = 1.2;
    }
    let res_record = label(scene, p, "", 24.0, amber());
    scene.ui_edit(res_record).name("ResRecord");
    let b = scene.ui_button(p, "again", "Играть снова").size(360.0, 58.0).id();
    style_button(scene, b, true);
    let b = scene.ui_button(p, "results_menu", "В главное меню").size(360.0, 54.0).id();
    style_button(scene, b, false);
    scene.ui_set_visible(results, false);

    vec![("hud", hud), ("main_menu", menu), ("mode_select", mode), ("options", options), ("pause", pause), ("upgrade_cards", upgrade), ("results", results)]
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
