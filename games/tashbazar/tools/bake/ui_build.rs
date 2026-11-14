//! Экраны игры на engine-ui: главное меню, настройки, пауза, итог полосы, HUD. Каждый
//! холст записывается префабом `assets/ui/<имя>.prefab`; в сцене это обычные экземпляры,
//! их показывает и прячет `GameManager`.

use engine::prelude::*;
use tashbazar::scripts::hud::Hud;
use tashbazar::Options;

// Палитра интерфейса: тёмный орех, кремовый текст, шафрановый акцент.
const PANEL: (f32, f32, f32, f32) = (0.085, 0.07, 0.055, 0.9);
const INK: u32 = 0xf3e6cc;
const MUTED: u32 = 0xb9a888;
const ACCENT: u32 = 0xe0a030;
const BUTTON: u32 = 0x3a2f24;
const BUTTON_HOVER: u32 = 0x54432f;
const BUTTON_PRESSED: u32 = 0x241d16;

fn panel_color() -> Color {
    Color::rgba(PANEL.0, PANEL.1, PANEL.2, PANEL.3)
}

fn style_button(scene: &mut Scene, id: GameObjectId, primary: bool) {
    if let Some(button) = scene.get_component_mut::<Button>(id) {
        if primary {
            button.normal_color = Color::hex(0xb5741a);
            button.hover_color = Color::hex(0xd18c27);
            button.pressed_color = Color::hex(0x8a5612);
        } else {
            button.normal_color = Color::hex(BUTTON);
            button.hover_color = Color::hex(BUTTON_HOVER);
            button.pressed_color = Color::hex(BUTTON_PRESSED);
        }
        button.disabled_color = Color::hex(0x221c16);
    }
}

fn button(scene: &mut Scene, parent: GameObjectId, message: &str, label: &str, primary: bool) -> GameObjectId {
    let id = scene
        .ui_button(parent, message, label)
        .name(message)
        .height(50.0)
        .font_size(22.0)
        .text_color(Color::hex(INK))
        .radius(8.0)
        .id();
    style_button(scene, id, primary);
    id
}

fn text(scene: &mut Scene, parent: GameObjectId, name: &str, s: &str, size: f32, color: u32) -> GameObjectId {
    scene.ui_text(parent, s).name(name).font_size(size).text_color(Color::hex(color)).id()
}

fn panel(scene: &mut Scene, parent: GameObjectId, name: &str, width: f32) -> GameObjectId {
    scene
        .ui_panel(parent)
        .name(name)
        .anchor(Anchor::Center)
        .width(width)
        .column(12.0)
        .padding(30.0)
        .fit()
        .color(panel_color())
        .radius(14.0)
        .border(1.0, Color::rgba(0.88, 0.63, 0.19, 0.35))
        .shadow(18.0)
        .id()
}

/// Главное меню: слева, поверх облёта карты.
fn main_menu(scene: &mut Scene) -> GameObjectId {
    let canvas = scene.ui_canvas().name("Main Menu").sort_order(10).id();
    let column = scene
        .ui_panel(canvas)
        .name("Panel")
        .anchor(Anchor::StretchLeft)
        .pos(0.0, 0.0)
        .size(440.0, 0.0)
        .column(12.0)
        .padding(44.0)
        .color(Color::rgba(0.06, 0.05, 0.04, 0.78))
        .id();
    scene.ui_node(column, "Spacer").height(70.0);
    scene
        .ui_text(column, "ТАШБАЗАР")
        .name("Title")
        .font_size(54.0)
        .bold()
        .text_color(Color::hex(ACCENT))
        .outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.6));
    text(scene, column, "Subtitle", "Пустынный городок. Движение в классическом стиле: стрейф, распрыжка, прыжок с приседом.", 17.0, MUTED);
    scene.ui_node(column, "Gap").height(24.0);
    button(scene, column, "play", "Прогулка по карте", true);
    button(scene, column, "course", "Полоса препятствий на время", false);
    button(scene, column, "gym", "Полигон: эталонные размеры", false);
    button(scene, column, "options", "Настройки", false);
    button(scene, column, "quit", "Выход", false);
    scene.ui_node(column, "Gap").height(10.0);
    text(scene, column, "BestTime", "Полоса препятствий ещё не пройдена", 17.0, INK);
    text(
        scene,
        column,
        "Controls",
        "WASD — бег  ·  пробел или колесо — прыжок  ·  Ctrl — присед  ·  Shift — шаг  ·  Esc — пауза",
        15.0,
        MUTED,
    );
    canvas
}

#[allow(clippy::too_many_arguments)]
fn slider_row(scene: &mut Scene, parent: GameObjectId, label: &str, message: &str, name: &str, min: f32, max: f32, value: f32, whole: bool) {
    let row = scene.ui_row(parent, 12.0).name(format!("{name}Row")).height(40.0).child_align(Alignment::Left).no_stretch().id();
    scene.ui_text(row, label).font_size(18.0).text_color(Color::hex(INK)).width(250.0).height(40.0).valign(VerticalAlign::Middle);
    let mut slider = scene.ui_slider(row, message, min, max, value).name(format!("{name}Slider")).width(250.0);
    if whole {
        slider = slider.whole_numbers();
    }
    let slider = slider.id();
    if let Some(s) = scene.get_component_mut::<Slider>(slider) {
        s.fill_color = Color::hex(0xc98a2a);
        s.handle_color = Color::hex(0xf3e6cc);
        s.track_color = Color::hex(0x3a2f24);
    }
    scene
        .ui_text(row, "")
        .name(format!("{name}Value"))
        .font_size(18.0)
        .text_color(Color::hex(ACCENT))
        .width(90.0)
        .height(40.0)
        .align(TextAlign::Right)
        .valign(VerticalAlign::Middle);
}

fn options_menu(scene: &mut Scene) -> GameObjectId {
    let canvas = scene.ui_canvas().name("Options").sort_order(20).hidden().id();
    let dim = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.0, 0.45)).name("Dim").stretch().id();
    let p = panel(scene, dim, "Panel", 680.0);
    scene.ui_text(p, "Настройки").name("Title").font_size(34.0).bold().text_color(Color::hex(ACCENT));
    let d = Options::default();
    slider_row(scene, p, "Чувствительность мыши", "sensitivity", "Sensitivity", Options::SENSITIVITY.0, Options::SENSITIVITY.1, d.sensitivity, false);
    slider_row(scene, p, "Угол обзора (по горизонтали на 4:3)", "fov", "Fov", Options::FOV.0, Options::FOV.1, d.fov, true);
    slider_row(scene, p, "Громкость", "volume", "Volume", 0.0, 100.0, d.volume * 100.0, true);
    for (message, name, label, on) in [
        ("autohop", "AutoHopToggle", "Автопрыжок, пока пробел зажат", d.auto_hop),
        ("showspeed", "SpeedToggle", "Показывать скорость (ед/с)", d.show_speed),
        ("invert", "InvertToggle", "Инвертировать мышь по вертикали", d.invert_y),
    ] {
        let id = scene.ui_toggle(p, message, label, on).name(name).height(36.0).font_size(18.0).text_color(Color::hex(INK)).id();
        if let Some(t) = scene.get_component_mut::<Toggle>(id) {
            t.on_color = Color::hex(0xc98a2a);
            t.style = ToggleStyle::Switch;
        }
    }
    text(scene, p, "Note", "Настройки сохраняются сразу. Чувствительность — как в классике: градусы = отсчёты мыши × чувствительность × 0.022.", 15.0, MUTED);
    button(scene, p, "back", "Назад", true);
    canvas
}

fn pause_menu(scene: &mut Scene) -> GameObjectId {
    let canvas = scene.ui_canvas().name("Pause").sort_order(30).hidden().id();
    let dim = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.0, 0.5)).name("Dim").stretch().id();
    let p = panel(scene, dim, "Panel", 420.0);
    scene.ui_text(p, "Пауза").name("Title").font_size(34.0).bold().text_color(Color::hex(ACCENT)).align(TextAlign::Center);
    button(scene, p, "resume", "Продолжить", true);
    button(scene, p, "restart", "Начать заново", false);
    button(scene, p, "options", "Настройки", false);
    button(scene, p, "menu", "В главное меню", false);
    button(scene, p, "quit", "Выход из игры", false);
    canvas
}

fn results(scene: &mut Scene) -> GameObjectId {
    let canvas = scene.ui_canvas().name("Results").sort_order(30).hidden().id();
    let dim = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.0, 0.45)).name("Dim").stretch().id();
    let p = panel(scene, dim, "Panel", 460.0);
    scene.ui_text(p, "Полоса пройдена").name("Title").font_size(32.0).bold().text_color(Color::hex(ACCENT)).align(TextAlign::Center);
    scene.ui_text(p, "0:00.00").name("ResultTime").font_size(60.0).bold().text_color(Color::hex(INK)).align(TextAlign::Center);
    scene.ui_text(p, "").name("ResultNote").font_size(19.0).text_color(Color::hex(MUTED)).align(TextAlign::Center);
    button(scene, p, "again", "Ещё раз", true);
    button(scene, p, "menu", "В главное меню", false);
    canvas
}

fn hud(scene: &mut Scene) -> GameObjectId {
    let canvas = scene.ui_canvas().name("HUD").no_navigation().hidden().id();
    // Прицел: четыре штриха и точка, с тёмной обводкой для читаемости на светлом песке.
    let cross = scene.ui_node(canvas, "Crosshair").anchor(Anchor::Center).size(40.0, 40.0).id();
    for (x, y, w, h) in [(0.0, -11.0, 2.0, 8.0), (0.0, 11.0, 2.0, 8.0), (-11.0, 0.0, 8.0, 2.0), (11.0, 0.0, 8.0, 2.0), (0.0, 0.0, 2.0, 2.0)] {
        let shadow = scene.ui_image(cross, Color::rgba(0.0, 0.0, 0.0, 0.55)).anchor(Anchor::Center).pos(x, y).size(w + 2.0, h + 2.0).id();
        if let Some(i) = scene.get_component_mut::<Image>(shadow) {
            i.raycast_target = false;
        }
        let bar = scene.ui_image(cross, Color::rgba(0.95, 1.0, 0.9, 0.95)).anchor(Anchor::Center).pos(x, y).size(w, h).id();
        if let Some(i) = scene.get_component_mut::<Image>(bar) {
            i.raycast_target = false;
        }
    }
    let shade = Color::rgba(0.0, 0.0, 0.0, 0.85);
    let speed = scene
        .ui_text(canvas, "0")
        .name("Speed")
        .anchor(Anchor::Bottom)
        .pos(0.0, -70.0)
        .size(300.0, 50.0)
        .font_size(40.0)
        .bold()
        .align(TextAlign::Center)
        .text_color(Color::hex(INK))
        .outline(2.0, shade)
        .id();
    scene
        .ui_text(canvas, "ед/с")
        .name("SpeedUnit")
        .anchor(Anchor::Bottom)
        .pos(0.0, -40.0)
        .size(300.0, 24.0)
        .font_size(15.0)
        .align(TextAlign::Center)
        .text_color(Color::hex(MUTED))
        .outline(1.5, shade);
    let position = scene
        .ui_text(canvas, "")
        .name("Position")
        .anchor(Anchor::TopLeft)
        .pos(20.0, 16.0)
        .size(700.0, 26.0)
        .font_size(17.0)
        .text_color(Color::hex(INK))
        .outline(1.5, shade)
        .no_wrap()
        .id();
    let place = scene
        .ui_text(canvas, "")
        .name("Place")
        .anchor(Anchor::TopLeft)
        .pos(20.0, 42.0)
        .size(700.0, 30.0)
        .font_size(22.0)
        .bold()
        .text_color(Color::hex(ACCENT))
        .outline(2.0, shade)
        .no_wrap()
        .id();
    let course_panel = scene
        .ui_panel(canvas)
        .name("Course")
        .anchor(Anchor::Top)
        .pos(0.0, 16.0)
        .size(760.0, 46.0)
        .color(panel_color())
        .radius(10.0)
        .hidden()
        .id();
    let course_text = scene
        .ui_text(course_panel, "")
        .name("CourseText")
        .stretch()
        .font_size(20.0)
        .align(TextAlign::Center)
        .valign(VerticalAlign::Middle)
        .text_color(Color::hex(INK))
        .no_wrap()
        .id();
    let hint = scene
        .ui_text(
            canvas,
            "WASD — бег · пробел / колесо — прыжок · Ctrl — присед · Shift — шаг · R — назад на точку · V — полёт · Esc — пауза",
        )
        .name("Hint")
        .anchor(Anchor::BottomLeft)
        .pos(20.0, -18.0)
        .size(900.0, 26.0)
        .font_size(15.0)
        .text_color(Color::hex(MUTED))
        .outline(1.5, shade)
        .no_wrap()
        .id();
    let r = |id| ObjectRef::new(scene, id);
    let component = Hud {
        speed_text: r(speed),
        position_text: r(position),
        place_text: r(place),
        course_panel: r(course_panel),
        course_text: r(course_text),
        hint_text: r(hint),
        ..Hud::default()
    };
    scene.get_mut(canvas).expect("hud").add(component);
    canvas
}

/// Построить все экраны и записать `assets/ui/*.prefab`.
pub fn bake(assets: &std::path::Path) -> Result<usize, Box<dyn std::error::Error>> {
    let mut scene = Scene::new("ui");
    let list = [
        (main_menu(&mut scene), "main_menu"),
        (options_menu(&mut scene), "options"),
        (pause_menu(&mut scene), "pause"),
        (results(&mut scene), "results"),
        (hud(&mut scene), "hud"),
    ];
    for (id, file) in list {
        let prefab = Prefab::from_object(&scene, id).ok_or("canvas is gone")?;
        let path = assets.join("ui").join(format!("{file}.prefab"));
        std::fs::create_dir_all(path.parent().ok_or("dir")?)?;
        prefab.save(&path)?;
    }
    Ok(list.len())
}
