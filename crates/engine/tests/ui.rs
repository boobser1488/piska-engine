//! Игровой интерфейс: ввод → события и сообщения, сохранение в сцену, отрисовка.
//!
//! Снимки экрана можно сохранить для просмотра: `UI_TEST_SHOTS=<каталог> cargo test --test ui`.

use engine::prelude::*;
use engine::render::{CameraView, Overlay, Renderer};

const DT: f32 = 1.0 / 60.0;

/// Драйвер не любит несколько безоконных рендереров сразу: тесты с GPU идут по одному.
fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

component! {
    /// Записывает все полученные сообщения.
    pub struct Recorder {
        pub log: Vec<String> = Vec::new(),
    }
}

impl Component for Recorder {
    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        self.log.push(format!("{}={:?}", message.name, message.value));
    }
}

fn new_app() -> App {
    let mut app = engine::app();
    app.register::<Recorder>();
    app.world.input.set_screen_size(Vec2::new(1280.0, 720.0));
    app
}

fn press(app: &mut App, at: Vec2) {
    app.world.input.set_mouse_position(at);
    app.world.input.set_mouse_button(MouseButton::Left, true);
    app.tick(DT);
}

fn release(app: &mut App) {
    app.world.input.set_mouse_button(MouseButton::Left, false);
    app.tick(DT);
}

fn key(app: &mut App, key: Key) {
    app.world.input.set_key(key, true);
    app.tick(DT);
    app.world.input.set_key(key, false);
    app.tick(DT);
}

fn log(app: &App, id: GameObjectId) -> Vec<String> {
    app.world.scene.get_component::<Recorder>(id).unwrap().log.clone()
}

/// Холст с панелью-столбиком по центру экрана 1280×720: ширина 300, отступы 20,
/// промежуток 10. Дети — по 40 в высоту, поэтому их центры по Y легко посчитать.
fn menu(app: &mut App) -> (GameObjectId, GameObjectId) {
    let scene = &mut app.world.scene;
    let canvas = scene.ui_canvas().with(Recorder::default()).id();
    let panel = scene
        .ui_panel(canvas)
        .anchor(Anchor::TopLeft)
        .pos(490.0, 100.0)
        .size(300.0, 0.0)
        .column(10.0)
        .padding(20.0)
        .fit()
        .id();
    (canvas, panel)
}

/// Центр `index`-го ребёнка высотой 40 в панели из [`menu`].
fn row_center(index: usize) -> Vec2 {
    Vec2::new(640.0, 100.0 + 20.0 + index as f32 * 50.0 + 20.0)
}

#[test]
fn button_click_sends_message_to_ancestors_and_event_to_polling_code() {
    let mut app = new_app();
    let (canvas, panel) = menu(&mut app);
    let play = app.world.scene.ui_button(panel, "play", "Играть").height(40.0).id();
    app.world.scene.ui_button(panel, "quit", "Выход").height(40.0);
    app.tick(DT); // первая раскладка

    // Нажатие мимо кнопок: ничего не нажато, но курсор над панелью.
    press(&mut app, Vec2::new(500.0, 105.0));
    assert!(app.world.ui().pointer_over_ui());
    release(&mut app);
    assert!(app.world.ui().events().is_empty());

    // Нажали на кнопке, отпустили на ней же — клик.
    press(&mut app, row_center(0));
    assert!(!app.world.ui().clicked("play"), "click fires on release, not on press");
    assert_eq!(app.world.ui().hovered(), Some(play));
    release(&mut app);
    assert!(app.world.ui().clicked("play"));
    assert!(app.world.ui().clicked_id(play));
    assert!(!app.world.ui().clicked("quit"));
    assert_eq!(log(&app, canvas), ["play=Nil"]);

    // Событие живёт ровно кадр.
    app.tick(DT);
    assert!(!app.world.ui().clicked("play"));

    // Нажали на одной кнопке, отпустили на другой — не клик (как в Unity).
    press(&mut app, row_center(0));
    app.world.input.set_mouse_position(row_center(1));
    release(&mut app);
    assert!(app.world.ui().events().is_empty());

    // Недоступная кнопка не кликается, но мышь ловит.
    app.world.scene.ui_edit(play).interactable(false);
    app.tick(DT);
    press(&mut app, row_center(0));
    release(&mut app);
    assert!(app.world.ui().events().is_empty());
    assert!(app.world.ui().pointer_over_ui());

    // Вне интерфейса.
    app.world.input.set_mouse_position(Vec2::new(20.0, 700.0));
    app.tick(DT);
    assert!(!app.world.ui().pointer_over_ui());
}

#[test]
fn slider_follows_drag_and_reports_values() {
    let mut app = new_app();
    let (canvas, panel) = menu(&mut app);
    let slider = app
        .world
        .scene
        .ui_slider(panel, "volume", 0.0, 100.0, 50.0)
        .height(40.0)
        .whole_numbers()
        .id();
    app.tick(DT);
    // Дорожка: панель 490..790, отступы 20, поля ручки 10 → 520..760.
    let y = row_center(0).y;
    press(&mut app, Vec2::new(520.0, y));
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 0.0);
    assert_eq!(app.world.ui().value_changed("volume"), Some(0.0));
    // Тянем далеко вправо, за край: значение упирается в максимум.
    app.world.input.set_mouse_position(Vec2::new(1000.0, 400.0));
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 100.0);
    app.world.input.set_mouse_position(Vec2::new(640.0, y));
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 50.0);
    release(&mut app);
    assert_eq!(log(&app, canvas), ["volume=Float(0.0)", "volume=Float(100.0)", "volume=Float(50.0)"]);

    // Колесо над ползунком — шаг 1 (целые числа).
    app.world.input.add_scroll(Vec2::new(0.0, 1.0));
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 51.0);
}

#[test]
fn toggle_flips_on_click_and_reports_state() {
    let mut app = new_app();
    let (canvas, panel) = menu(&mut app);
    let toggle = app.world.scene.ui_toggle(panel, "vsync", "Вертикальная синхронизация", false).height(40.0).id();
    app.tick(DT);
    // Клик по подписи тоже переключает: подпись — часть виджета.
    press(&mut app, row_center(0) + Vec2::new(80.0, 0.0));
    release(&mut app);
    assert!(app.world.scene.get_component::<Toggle>(toggle).unwrap().is_on);
    assert_eq!(app.world.ui().toggled("vsync"), Some(true));
    press(&mut app, row_center(0));
    release(&mut app);
    assert!(!app.world.scene.get_component::<Toggle>(toggle).unwrap().is_on);
    assert_eq!(log(&app, canvas), ["vsync=Bool(true)", "vsync=Bool(false)"]);
}

#[test]
fn input_field_takes_text_backspace_and_enter() {
    let mut app = new_app();
    let (canvas, panel) = menu(&mut app);
    let field = app.world.scene.ui_input(panel, "name", "Имя").height(40.0).max_length(8).id();
    app.tick(DT);
    assert!(!app.world.ui().wants_keyboard());

    press(&mut app, row_center(0));
    release(&mut app);
    assert!(app.world.ui().wants_keyboard());
    assert_eq!(app.world.ui().focused(), Some(field));

    app.world.input.add_text("Привет");
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<InputField>(field).unwrap().text, "Привет");
    assert_eq!(app.world.ui().text_changed("name"), Some("Привет"));

    key(&mut app, Key::Backspace);
    // Длина ограничена 8 символами.
    app.world.input.add_text("!!!!!");
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<InputField>(field).unwrap().text, "Приве!!!");

    app.world.input.set_key(Key::Enter, true);
    app.tick(DT);
    assert_eq!(app.world.ui().submitted("name"), Some("Приве!!!"));
    app.world.input.set_key(Key::Enter, false);

    // Клик мимо поля заканчивает ввод.
    press(&mut app, Vec2::new(100.0, 600.0));
    release(&mut app);
    assert!(!app.world.ui().wants_keyboard());

    let messages = log(&app, canvas);
    assert_eq!(messages.first().map(String::as_str), Some("name=Str(\"Привет\")"));
    assert_eq!(messages.last().map(String::as_str), Some("name:submit=Str(\"Приве!!!\")"));
}

#[test]
fn keyboard_navigation_moves_focus_and_enter_clicks() {
    let mut app = new_app();
    let (_, panel) = menu(&mut app);
    let first = app.world.scene.ui_button(panel, "first", "Первая").height(40.0).id();
    let second = app.world.scene.ui_button(panel, "second", "Вторая").height(40.0).id();
    app.tick(DT);
    key(&mut app, Key::Down);
    assert_eq!(app.world.ui().focused(), Some(first));
    key(&mut app, Key::Tab);
    assert_eq!(app.world.ui().focused(), Some(second));
    key(&mut app, Key::Tab); // по кругу
    assert_eq!(app.world.ui().focused(), Some(first));
    key(&mut app, Key::Up);
    assert_eq!(app.world.ui().focused(), Some(second));
    app.world.input.set_key(Key::Enter, true);
    app.tick(DT);
    assert!(app.world.ui().clicked("second"));
    app.world.input.set_key(Key::Enter, false);

    // На холсте без навигации стрелки не трогают кнопки.
    let mut app = new_app();
    let hud = app.world.scene.ui_canvas().no_navigation().id();
    app.world.scene.ui_button(hud, "pause", "Пауза");
    app.tick(DT);
    key(&mut app, Key::Down);
    assert_eq!(app.world.ui().focused(), None);
}

/// Нажатие и отпускание кнопки геймпада: два кадра (нажатие, затем отпускание).
fn pad_button(app: &mut App, pad: GamepadId, button: GamepadButton) {
    app.world.input.set_gamepad_button(pad, button, true);
    app.tick(DT);
    app.world.input.set_gamepad_button(pad, button, false);
    app.tick(DT);
}

#[test]
fn gamepad_navigation_moves_focus_and_south_east_act_like_enter_and_escape() {
    let pad = GamepadId(0);
    let mut app = new_app();
    app.world.input.connect_gamepad(pad, "Test pad");
    let (_, panel) = menu(&mut app);
    let first = app.world.scene.ui_button(panel, "first", "Первая").height(40.0).id();
    let second = app.world.scene.ui_button(panel, "second", "Вторая").height(40.0).id();
    app.tick(DT);

    // Крестовина: одно нажатие — один шаг.
    pad_button(&mut app, pad, GamepadButton::DPadDown);
    assert_eq!(app.world.ui().focused(), Some(first));
    pad_button(&mut app, pad, GamepadButton::DPadDown);
    assert_eq!(app.world.ui().focused(), Some(second));
    pad_button(&mut app, pad, GamepadButton::DPadUp);
    assert_eq!(app.world.ui().focused(), Some(first));

    // Левый стик за порогом — шаг; слабый наклон (0.3 < 0.5) не двигает фокус.
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickY, -0.3);
    app.tick(DT);
    assert_eq!(app.world.ui().focused(), Some(first));
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickY, -0.9);
    app.tick(DT);
    assert_eq!(app.world.ui().focused(), Some(second));

    // Стик удерживается: первые 0.45 с фокус стоит...
    for _ in 0..20 {
        app.tick(DT);
    }
    assert_eq!(app.world.ui().focused(), Some(second));
    // ...потом повторяется: шаг по кругу на первую кнопку примерно через 0.45 с
    // (7 кадров после 20 — с запасом на округление).
    let mut frames = 0;
    while app.world.ui().focused() == Some(second) && frames < 30 {
        app.tick(DT);
        frames += 1;
    }
    assert!((6..=8).contains(&frames), "repeat after {frames} frames");
    assert_eq!(app.world.ui().focused(), Some(first));
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickY, 0.0);
    app.tick(DT);

    // South — Enter: кликает сфокусированную кнопку.
    app.world.input.set_gamepad_button(pad, GamepadButton::South, true);
    app.tick(DT);
    assert!(app.world.ui().clicked("first"));
    app.world.input.set_gamepad_button(pad, GamepadButton::South, false);
    app.tick(DT);
    assert!(!app.world.ui().clicked("first"));

    // East — Escape: снимает фокус.
    pad_button(&mut app, pad, GamepadButton::East);
    assert_eq!(app.world.ui().focused(), None);
}

#[test]
fn gamepad_dpad_changes_slider_and_south_starts_text_input() {
    let pad = GamepadId(0);
    let mut app = new_app();
    app.world.input.connect_gamepad(pad, "Test pad");
    let (_, panel) = menu(&mut app);
    let slider = app.world.scene.ui_slider(panel, "volume", 0.0, 100.0, 50.0).height(40.0).whole_numbers().id();
    let field = app.world.scene.ui_input(panel, "name", "Имя").height(40.0).id();
    app.tick(DT);

    pad_button(&mut app, pad, GamepadButton::DPadDown);
    assert_eq!(app.world.ui().focused(), Some(slider));
    pad_button(&mut app, pad, GamepadButton::DPadRight);
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 51.0);
    pad_button(&mut app, pad, GamepadButton::DPadLeft);
    pad_button(&mut app, pad, GamepadButton::DPadLeft);
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 49.0);

    // Стик вправо тоже шагает по ползунку.
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, 0.9);
    app.tick(DT);
    app.world.input.set_gamepad_axis(pad, GamepadAxis::LeftStickX, 0.0);
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<Slider>(slider).unwrap().value, 50.0);

    // К полю ввода; South начинает ввод, East заканчивает.
    pad_button(&mut app, pad, GamepadButton::DPadDown);
    assert_eq!(app.world.ui().focused(), Some(field));
    pad_button(&mut app, pad, GamepadButton::South);
    assert!(app.world.ui().wants_keyboard());
    app.world.input.add_text("Ок");
    app.tick(DT);
    assert_eq!(app.world.scene.get_component::<InputField>(field).unwrap().text, "Ок");
    pad_button(&mut app, pad, GamepadButton::East);
    assert!(!app.world.ui().wants_keyboard());
}

#[test]
fn input_field_delete_home_and_end_edit_the_text() {
    let mut app = new_app();
    let (_, panel) = menu(&mut app);
    let field = app.world.scene.ui_input(panel, "name", "Имя").height(40.0).id();
    app.tick(DT);
    press(&mut app, row_center(0));
    release(&mut app);
    app.world.input.add_text("Привет");
    app.tick(DT);
    let text = |app: &App| app.world.scene.get_component::<InputField>(field).unwrap().text.clone();
    assert_eq!(text(&app), "Привет");

    // Home — в начало; Delete удаляет символ под кареткой.
    key(&mut app, Key::Home);
    key(&mut app, Key::Delete);
    assert_eq!(text(&app), "ривет");
    app.world.input.add_text("!");
    app.tick(DT);
    assert_eq!(text(&app), "!ривет");

    // Каретка после «!» стоит на 1: Delete удаляет «р».
    key(&mut app, Key::Delete);
    assert_eq!(text(&app), "!ивет");

    // End — в конец; Delete в конце ничего не делает.
    key(&mut app, Key::End);
    key(&mut app, Key::Delete);
    assert_eq!(text(&app), "!ивет");
    app.world.input.add_text("?");
    app.tick(DT);
    assert_eq!(text(&app), "!ивет?");

    // Delete автоповторяется: держим — стирается всё.
    key(&mut app, Key::Home);
    app.world.input.set_key(Key::Delete, true);
    app.tick(DT);
    assert_eq!(text(&app), "ивет?");
    for _ in 0..60 {
        app.tick(DT);
    }
    app.world.input.set_key(Key::Delete, false);
    app.tick(DT);
    assert_eq!(text(&app), "");
}

#[test]
fn hidden_and_masked_elements_do_not_catch_the_mouse() {
    let mut app = new_app();
    let (_, panel) = menu(&mut app);
    let hidden = app.world.scene.ui_button(panel, "hidden", "Скрыта").height(40.0).hidden().id();
    app.world.scene.ui_button(panel, "shown", "Видна").height(40.0);
    // Кнопка, торчащая за маску: снаружи маски её не нажать.
    let canvas = app.world.scene.ui_canvas().sort_order(5).id();
    let mask = app.world.scene.ui_node(canvas, "Mask").anchor(Anchor::TopLeft).pos(0.0, 600.0).size(100.0, 50.0).mask().id();
    app.world.scene.ui_button(mask, "masked", "Маска").anchor(Anchor::TopLeft).size(300.0, 50.0);
    app.tick(DT);

    // Скрытая кнопка не участвует в раскладке: «Видна» встала на её место.
    press(&mut app, row_center(0));
    release(&mut app);
    assert!(app.world.ui().clicked("shown"));
    assert!(!app.world.ui().clicked("hidden"));

    press(&mut app, Vec2::new(50.0, 620.0));
    release(&mut app);
    assert!(app.world.ui().clicked("masked"));
    press(&mut app, Vec2::new(200.0, 620.0));
    release(&mut app);
    assert!(app.world.ui().events().is_empty());
    assert!(!app.world.ui().pointer_over_ui());
    let _ = hidden;
}

#[test]
fn ui_round_trips_through_ron_scenes() {
    let mut app = new_app();
    let (_, panel) = menu(&mut app);
    app.world.scene.ui_text(panel, "Заголовок").font_size(40.0).bold().align(TextAlign::Center);
    app.world.scene.ui_button(panel, "play", "Играть").primary();
    app.world.scene.ui_toggle(panel, "music", "Музыка", true).switch();
    app.world.scene.ui_slider(panel, "volume", 0.0, 10.0, 7.0).whole_numbers();
    app.world.scene.ui_progress(panel, 0.25);
    app.world.scene.ui_input(panel, "name", "Имя").value("Аня");
    let text = app.world.scene_to_string();

    let mut loaded = new_app();
    loaded.world.load_scene_str(&text, "ui.scene").unwrap();
    assert_eq!(loaded.world.scene_to_string(), text);
    let scene = &loaded.world.scene;
    let find = |name: &str| scene.find_by_name(name).unwrap();
    assert_eq!(scene.get_component::<Button>(find("play")).unwrap().normal_color, Button::primary("x").normal_color);
    assert_eq!(scene.get_component::<Toggle>(find("music")).unwrap().style, ToggleStyle::Switch);
    assert!(scene.get_component::<Slider>(find("volume")).unwrap().whole_numbers);
    assert_eq!(scene.get_component::<InputField>(find("name")).unwrap().text, "Аня");
    assert!(scene.get_component::<LayoutGroup>(find("Panel")).unwrap().fit_content);

    // Загруженный интерфейс работает: клик по кнопке.
    loaded.tick(DT);
    press(&mut loaded, row_center(1) + Vec2::new(0.0, 15.0));
    release(&mut loaded);
    assert!(loaded.world.ui().clicked("play"), "{:?}", loaded.world.ui().events());
}

// ----------------------------------------------------------------------
// Отрисовка
// ----------------------------------------------------------------------

fn save(image: &image::RgbaImage, name: &str) {
    if let Ok(dir) = std::env::var("UI_TEST_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        image.save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

fn pixel(image: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let p = image.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
    a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= tolerance)
}

#[test]
fn renders_panels_text_and_clipping_on_top_of_the_frame() {
    let mut app = new_app();
    app.spawn("Camera").with(Camera {
        clear_color: Color::BLACK,
        ..Camera::default()
    });
    let scene = &mut app.world.scene;
    let canvas = scene.ui_canvas().id();
    scene
        .ui_image(canvas, Color::rgb(1.0, 0.0, 0.0))
        .anchor(Anchor::TopLeft)
        .pos(100.0, 100.0)
        .size(200.0, 100.0);
    scene
        .ui_text(canvas, "ЖЖЖ Text")
        .anchor(Anchor::TopLeft)
        .pos(400.0, 100.0)
        .size(400.0, 100.0)
        .font_size(64.0)
        .height(100.0)
        .color(Color::WHITE);
    let mask = scene
        .ui_node(canvas, "Mask")
        .anchor(Anchor::TopLeft)
        .pos(100.0, 300.0)
        .size(100.0, 100.0)
        .mask()
        .id();
    scene
        .ui_image(mask, Color::rgb(0.0, 1.0, 0.0))
        .anchor(Anchor::TopLeft)
        .size(300.0, 100.0);
    app.tick(DT);

    // Всё одной текстурой (атлас) и без смены отсечения внутри одного холста — пара вызовов.
    let overlay = app.world.resources.get::<Overlay>().unwrap();
    assert!(overlay.batches.len() <= 3, "{} batches", overlay.batches.len());

    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().unwrap();
    let image = renderer.capture(&app.world, 1280, 720).unwrap();
    save(&image, "render_basic.png");
    assert!(close(pixel(&image, 200, 150), [255, 0, 0], 2), "{:?}", pixel(&image, 200, 150));
    assert!(close(pixel(&image, 50, 50), [0, 0, 0], 2));
    // Текст: заметное число ярких пикселей внутри его прямоугольника.
    let bright = (400..800)
        .flat_map(|x| (100..200).map(move |y| (x, y)))
        .filter(|&(x, y)| pixel(&image, x, y)[1] > 200)
        .count();
    assert!(bright > 1500, "{bright} text pixels");
    // Маска: зелёное видно внутри и отрезано снаружи.
    assert!(close(pixel(&image, 150, 350), [0, 255, 0], 2));
    assert!(close(pixel(&image, 300, 350), [0, 0, 0], 2), "{:?}", pixel(&image, 300, 350));

    // Захват другого размера: слой растягивается под цель.
    let half = renderer.capture(&app.world, 640, 360).unwrap();
    assert!(close(pixel(&half, 100, 75), [255, 0, 0], 2));
}

#[test]
fn ui_is_drawn_after_post_processing_and_only_in_game_views() {
    let mut app = new_app();
    // 3D-камера: тонмаппинг и блум включены.
    app.spawn("Camera").with(Camera::perspective(60.0));
    let canvas = app.world.scene.ui_canvas().id();
    app.world
        .scene
        .ui_image(canvas, Color::hex(0x808080))
        .anchor(Anchor::TopLeft)
        .size(200.0, 200.0);
    app.world
        .scene
        .ui_image(canvas, Color::WHITE)
        .anchor(Anchor::TopLeft)
        .pos(300.0, 0.0)
        .size(200.0, 200.0);
    app.tick(DT);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().unwrap();
    let image = renderer.capture(&app.world, 1280, 720).unwrap();
    save(&image, "render_post.png");
    // Цвет интерфейса не тонмаппится: серый 0x80 остаётся 0x80, белый — белым.
    assert!(close(pixel(&image, 100, 100), [128, 128, 128], 2), "{:?}", pixel(&image, 100, 100));
    assert!(close(pixel(&image, 400, 100), [255, 255, 255], 1));
    // И не светится блумом: рядом с белым квадратом — небо, а не ореол.
    let sky = pixel(&image, 600, 20);
    let near = pixel(&image, 505, 100);
    assert!(near.iter().zip(sky).all(|(a, b)| (*a as i32 - b as i32).abs() < 30), "{near:?} vs {sky:?}");

    // Свободный вид редактора (Scene) интерфейс не показывает, вид камеры (Game) — показывает.
    let free = CameraView::look_at(Vec3::new(0.0, 2.0, 8.0), Vec3::ZERO, 60.0, 16.0 / 9.0, Color::BLACK, false);
    assert!(!free.overlay);
    assert!(renderer.scene_view(&app.world, 16.0 / 9.0).overlay);
    use engine::render::wgpu;
    let target = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("scene view"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 720,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.format(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.render_with(&app.world, &view, 1280, 720, &free);
    let scene_view = engine::render::read_texture(renderer.device(), renderer.queue(), &target, 1280, 720).unwrap();
    assert!(!close(pixel(&scene_view, 100, 100), [128, 128, 128], 2));
}

#[test]
fn text_stays_sharp_when_the_canvas_scales() {
    // Экран вдвое больше эталона: кегль 20 становится 40 пикселями, а не растянутой картинкой.
    let mut app = new_app();
    app.world.input.set_screen_size(Vec2::new(2560.0, 1440.0));
    app.spawn("Camera").with(Camera {
        clear_color: Color::BLACK,
        ..Camera::default()
    });
    let canvas = app.world.scene.ui_canvas().id();
    app.world
        .scene
        .ui_text(canvas, "I")
        .anchor(Anchor::TopLeft)
        .size(100.0, 40.0)
        .font_size(20.0)
        .color(Color::WHITE);
    app.tick(DT);
    let _gpu = gpu_lock();
    let mut renderer = Renderer::headless().unwrap();
    let image = renderer.capture(&app.world, 2560, 1440).unwrap();
    // Вертикальная черта «I» растеризована в 40 пикселей, а не растянута из 20:
    // высота черты — около высоты прописной (≈ 0.7 кегля), а верх и низ черты резкие —
    // не больше одного полупрозрачного ряда на каждом конце.
    let mut solid_rows = 0;
    let mut soft_rows = 0;
    for y in 0..80 {
        let brightest = (0..200).map(|x| pixel(&image, x, y)[0]).max().unwrap();
        match brightest {
            0..=20 => {}
            236..=255 => solid_rows += 1,
            _ => soft_rows += 1,
        }
    }
    assert!((25..=31).contains(&solid_rows), "stem is {solid_rows} px tall");
    assert!(soft_rows <= 2, "{soft_rows} blurry rows at the stem ends");
}
