//! Интерфейс уровня (HUD, пауза, результаты) и мелкие помощники для `engine-ui`.

use crate::common::Gfx;
use crate::controls::{PAD, PAD_GAP, PAD_MARGIN};
use crate::level::LevelManager;
use crate::player::MAX_HP;
use engine::prelude::*;

pub const GOLD: u32 = 0xffd75a;

pub fn find(ctx: &Ctx, name: &str) -> Option<GameObjectId> {
    ctx.find_by_name(name)
}

pub fn set_text(ctx: &mut Ctx, name: &str, text: &str) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(id) {
            t.set(text);
        }
    }
}

pub fn set_text_color(ctx: &mut Ctx, name: &str, color: Color) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(t) = ctx.scene_mut().get_component_mut::<Text>(id) {
            t.color = color;
        }
    }
}

pub fn show(ctx: &mut Ctx, name: &str, visible: bool) {
    if let Some(id) = ctx.find_by_name(name) {
        ctx.scene_mut().ui_set_visible(id, visible);
    }
}

pub fn set_image_alpha(ctx: &mut Ctx, name: &str, alpha: f32) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(i) = ctx.scene_mut().get_component_mut::<Image>(id) {
            i.color = i.color.with_alpha(alpha);
        }
    }
}

pub fn set_image_texture(ctx: &mut Ctx, name: &str, tex: AssetHandle<Texture>) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(i) = ctx.scene_mut().get_component_mut::<Image>(id) {
            i.texture = tex;
        }
    }
}

pub fn set_progress(ctx: &mut Ctx, name: &str, value: f32) {
    if let Some(id) = ctx.find_by_name(name) {
        if let Some(b) = ctx.scene_mut().get_component_mut::<ProgressBar>(id) {
            b.value = value.clamp(0.0, 1.0);
        }
    }
}

/// Картинка интерфейса из текстуры игры (мышь не ловит).
pub fn picture<'a>(scene: &'a mut Scene, parent: GameObjectId, tex: AssetHandle<Texture>, name: &str) -> UiNode<'a> {
    scene.ui_image(parent, Color::WHITE).name(name).edit::<Image>(move |i| {
        i.texture = tex;
        i.raycast_target = false;
    })
}

/// Тёмная панель-подсказка в стиле игры.
pub fn glass() -> Color {
    Color::rgba(0.02, 0.03, 0.09, 0.82)
}

/// HUD: сердца, фонарь со счётом светлячков, время, пауза, подсказки, босс, экранные кнопки.
pub fn build_hud(ctx: &mut Ctx, level: &LevelManager) {
    let gfx = ctx.resource::<Gfx>().cloned().unwrap_or_default();
    let scene = ctx.scene_mut();
    let hud = scene
        .ui_canvas()
        .name("Hud")
        .no_navigation()
        .reference_resolution(1280.0, 720.0)
        .edit::<Canvas>(|c| c.match_width_or_height = 1.0)
        .id();

    // сердца
    for i in 0..MAX_HP {
        picture(scene, hud, gfx.tex("ui_heart"), &format!("Heart{i}"))
            .anchor(Anchor::TopLeft)
            .pos(24.0 + i as f32 * 50.0, 18.0)
            .size(44.0, 44.0);
    }
    // фонарь и светлячки
    picture(scene, hud, gfx.tex("ui_lantern_dim"), "LanternDim").anchor(Anchor::TopLeft).pos(20.0, 68.0).size(60.0, 60.0);
    picture(scene, hud, gfx.tex("ui_lantern_lit"), "LanternLit").anchor(Anchor::TopLeft).pos(20.0, 68.0).size(60.0, 60.0);
    scene
        .ui_text(hud, "0 / 0")
        .name("FireflyText")
        .anchor(Anchor::TopLeft)
        .pos(88.0, 80.0)
        .size(130.0, 40.0)
        .font_size(32.0)
        .bold()
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.85));
    picture(scene, hud, gfx.tex("ui_star_empty"), "GoldenIcon").anchor(Anchor::TopLeft).pos(222.0, 82.0).size(36.0, 36.0);

    // время и название
    scene
        .ui_text(hud, "0:00")
        .name("TimerText")
        .anchor(Anchor::Top)
        .pos(0.0, 16.0)
        .size(220.0, 40.0)
        .font_size(30.0)
        .bold()
        .align(TextAlign::Center)
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.85));
    scene
        .ui_text(hud, &level.title)
        .name("LevelName")
        .anchor(Anchor::Top)
        .pos(0.0, 52.0)
        .size(420.0, 28.0)
        .font_size(19.0)
        .align(TextAlign::Center)
        .text_color(Color::rgba(0.8, 0.86, 1.0, 0.75))
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.8));

    // пауза
    scene.ui_button(hud, "pause_btn", "II").anchor(Anchor::TopRight).pos(-24.0, 18.0).size(60.0, 52.0);

    // подсказка внизу
    let banner = scene
        .ui_image(hud, glass())
        .name("Banner")
        .anchor(Anchor::Bottom)
        .pos(0.0, -190.0)
        .size(820.0, 64.0)
        .radius(14.0)
        .hidden()
        .id();
    scene
        .ui_text(banner, "")
        .name("BannerText")
        .stretch()
        .font_size(23.0)
        .align(TextAlign::Center)
        .valign(VerticalAlign::Middle)
        .inset(16.0, 4.0, 16.0, 4.0);

    // полоса здоровья босса
    let boss = scene
        .ui_image(hud, glass())
        .name("BossBar")
        .anchor(Anchor::Top)
        .pos(0.0, 96.0)
        .size(560.0, 58.0)
        .radius(12.0)
        .hidden()
        .id();
    scene
        .ui_text(boss, "Хозяин Тьмы")
        .anchor(Anchor::Top)
        .pos(0.0, 6.0)
        .size(520.0, 26.0)
        .font_size(20.0)
        .bold()
        .align(TextAlign::Center);
    scene
        .ui_progress(boss, 1.0)
        .name("BossHp")
        .anchor(Anchor::Bottom)
        .pos(0.0, -10.0)
        .size(520.0, 16.0)
        .color(Color::hex(0xe8467f));

    // заставка уровня
    let card = scene.ui_node(hud, "Card").stretch().with(CanvasGroup::default()).id();
    scene
        .ui_text(card, "")
        .name("CardNumber")
        .anchor(Anchor::Top)
        .pos(0.0, 150.0)
        .size(700.0, 32.0)
        .font_size(26.0)
        .align(TextAlign::Center)
        .text_color(Color::hex(GOLD))
        .outline(3.0, Color::rgba(0.0, 0.0, 0.0, 0.9));
    scene
        .ui_text(card, "")
        .name("CardTitle")
        .anchor(Anchor::Top)
        .pos(0.0, 186.0)
        .size(900.0, 70.0)
        .font_size(58.0)
        .bold()
        .align(TextAlign::Center)
        .outline(4.0, Color::rgba(0.0, 0.0, 0.0, 0.9));
    scene
        .ui_text(card, "")
        .name("CardSub")
        .anchor(Anchor::Top)
        .pos(0.0, 262.0)
        .size(900.0, 34.0)
        .font_size(26.0)
        .align(TextAlign::Center)
        .text_color(Color::rgba(0.85, 0.9, 1.0, 0.9))
        .outline(3.0, Color::rgba(0.0, 0.0, 0.0, 0.9));

    // экранные кнопки (показываются после первого касания)
    let touch = scene.ui_node(hud, "Touch").stretch().hidden().id();
    let pad = Vec2::splat(PAD);
    for (i, name) in ["touch_left", "touch_right"].iter().enumerate() {
        picture(scene, touch, gfx.tex(name), name)
            .anchor(Anchor::BottomLeft)
            .pos(PAD_MARGIN + i as f32 * (PAD + PAD_GAP), -PAD_MARGIN)
            .size(pad.x, pad.y);
    }
    for (i, name) in ["touch_jump", "touch_dash"].iter().enumerate() {
        picture(scene, touch, gfx.tex(name), name)
            .anchor(Anchor::BottomRight)
            .pos(-(PAD_MARGIN + i as f32 * (PAD + PAD_GAP)), -PAD_MARGIN)
            .size(pad.x, pad.y);
    }
}

/// Пауза и экран результатов (скрыты, пока не нужны).
pub fn build_overlays(ctx: &mut Ctx) {
    let scene = ctx.scene_mut();
    let canvas = scene.ui_canvas().name("Overlay").sort_order(10).id();

    // ----- пауза -----
    let dim = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.03, 0.66)).name("PauseDim").stretch().hidden().id();
    let panel = scene.ui_panel(dim).anchor(Anchor::Center).width(440.0).column(12.0).padding(30.0).fit().id();
    scene.ui_text(panel, "Пауза").font_size(44.0).bold().align(TextAlign::Center);
    scene.ui_button(panel, "resume", "Продолжить").primary();
    scene.ui_button(panel, "restart", "Начать уровень заново");
    scene.ui_button(panel, "menu", "В главное меню");
    scene
        .ui_text(panel, "A / D или стрелки — бег\nПробел, W — прыжок (в воздухе — ещё один)\nShift, X — рывок    Esc, P — пауза")
        .font_size(16.0)
        .align(TextAlign::Center)
        .text_color(Color::rgba(0.75, 0.8, 0.95, 0.8))
        .height(70.0);

    // ----- результаты -----
    let dim = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.03, 0.74)).name("ResultsDim").stretch().hidden().id();
    let panel = scene.ui_panel(dim).anchor(Anchor::Center).width(640.0).column(8.0).padding(28.0).fit().id();
    scene.ui_text(panel, "").name("ResTitle").font_size(46.0).bold().align(TextAlign::Center);
    scene.ui_text(panel, "").name("ResLevel").font_size(22.0).align(TextAlign::Center).text_color(Color::rgba(0.8, 0.86, 1.0, 0.85));
    let stars = scene.ui_node(panel, "Stars").height(84.0).id();
    for i in 0..3 {
        scene
            .ui_image(stars, Color::WHITE)
            .name(format!("ResStar{i}"))
            .anchor(Anchor::Center)
            .pos(-92.0 + i as f32 * 92.0, 0.0)
            .size(80.0, 80.0)
            .edit::<Image>(|im| im.raycast_target = false);
    }
    for name in ["ResFireflies", "ResGolden", "ResTime", "ResDeaths"] {
        scene.ui_text(panel, "").name(name).font_size(24.0).align(TextAlign::Center);
    }
    scene
        .ui_text(panel, "")
        .name("ResAbility")
        .font_size(21.0)
        .align(TextAlign::Center)
        .text_color(Color::hex(GOLD))
        .height(56.0)
        .hidden();
    scene.ui_button(panel, "next", "Дальше").primary();
    scene.ui_button(panel, "retry_cp", "Продолжить с контрольной точки").primary();
    scene.ui_button(panel, "restart", "Начать уровень заново");
    scene.ui_button(panel, "menu", "В главное меню");
}
