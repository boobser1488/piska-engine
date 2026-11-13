//! Игровой интерфейс: главное меню → настройки → HUD с паузой.
//!
//! Запуск: `cargo run --example ui_demo`. Мышь или клавиатура (Tab / стрелки / Enter),
//! Esc в игре — пауза. Для скриншота конкретного экрана:
//! `UI_DEMO_SCREEN=settings ENGINE_SCREENSHOT=ui.png cargo run --example ui_demo`
//! (`menu`, `settings`, `about`, `hud`, `pause`).

use engine::prelude::*;

/// Экраны демо — объекты с этими именами под холстами.
const SCREENS: [&str; 5] = ["MainMenu", "Settings", "About", "Hud", "Pause"];

component! {
    /// Переключает экраны по сообщениям кнопок и ведёт «игру» под HUD.
    pub struct DemoFlow {
        pub health: f32 = 0.82,
        pub score: f32 = 1250.0,
        pub shown_score: i32 = -1,
        pub volume: f32 = 70.0,
    }
}

impl DemoFlow {
    fn show(ctx: &mut Ctx, screen: &str) {
        for name in SCREENS {
            if let Some(id) = ctx.scene().find_by_name(name) {
                // HUD остаётся видимым под паузой.
                let visible = name == screen || (screen == "Pause" && name == "Hud");
                ctx.scene_mut().ui_set_visible(id, visible);
            }
        }
        // Пауза — это просто остановленное время; интерфейс работает и при нём.
        ctx.world.time.time_scale = if screen == "Hud" { 1.0 } else { 0.0 };
    }

    fn set_text(ctx: &mut Ctx, object: &str, text: &str) {
        if let Some(id) = ctx.scene().find_by_name(object) {
            if let Some(label) = ctx.scene_mut().get_component_mut::<Text>(id) {
                label.set(text);
            }
        }
    }
}

impl Component for DemoFlow {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let screen = match std::env::var("UI_DEMO_SCREEN").as_deref() {
            Ok("settings") => "Settings",
            Ok("about") => "About",
            Ok("hud") => "Hud",
            Ok("pause") => "Pause",
            _ => "MainMenu",
        };
        Self::show(ctx, screen);
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        match (message.name.as_str(), &message.value) {
            ("play" | "resume", _) => Self::show(ctx, "Hud"),
            ("settings", _) => Self::show(ctx, "Settings"),
            ("about", _) => Self::show(ctx, "About"),
            // В редакторе выходит из режима Play, в окне — закрывает игру.
            ("quit", _) => ctx.quit(),
            ("back" | "apply" | "menu", _) => Self::show(ctx, "MainMenu"),
            ("pause", _) => Self::show(ctx, "Pause"),
            ("volume", Value::Float(v)) => {
                self.volume = *v;
                Self::set_text(ctx, "VolumeValue", &format!("{v:.0}%"));
            }
            ("sensitivity", Value::Float(v)) => Self::set_text(ctx, "SensitivityValue", &format!("{v:.1}")),
            ("player_name", Value::Str(name)) => {
                let shown = if name.is_empty() { "Игрок" } else { name.as_str() };
                Self::set_text(ctx, "PlayerName", shown);
            }
            _ => {}
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let hud_visible = ctx
            .scene()
            .find_by_name("Hud")
            .and_then(|id| ctx.scene().get(id))
            .is_some_and(|o| o.active);
        let paused = ctx.world.time.time_scale == 0.0;
        if hud_visible && ctx.input().key_down(Key::Escape) && !ctx.ui().wants_keyboard() {
            Self::show(ctx, if paused { "Hud" } else { "Pause" });
        }
        if !hud_visible {
            return;
        }
        // «Игра»: здоровье колеблется, очки растут.
        let t = ctx.time().elapsed as f32;
        self.health = 0.55 + 0.4 * (t * 0.7).sin();
        self.score += dt * 37.0;
        if let Some(id) = ctx.scene().find_by_name("HealthBar") {
            if let Some(bar) = ctx.scene_mut().get_component_mut::<ProgressBar>(id) {
                bar.value = self.health;
                bar.fill_color = if self.health < 0.3 { Color::hex(0xff5d5d) } else { Color::hex(0x3ecf7a) };
            }
        }
        let score = self.score as i32;
        if score != self.shown_score {
            self.shown_score = score;
            Self::set_text(ctx, "Score", &format!("Счёт: {score}"));
            Self::set_text(ctx, "HealthValue", &format!("{:.0} / 100", self.health * 100.0));
        }
    }
}

component! {
    /// Медленно облетает сцену — живой фон под меню.
    pub struct Flyby {
        pub angle: f32 = 0.6,
    }
}

impl Component for Flyby {
    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        // Фон меню движется и на паузе: берём реальное время.
        self.angle += 0.12 * ctx.time().unscaled_delta;
        let eye = Vec3::new(self.angle.sin() * 11.0, 3.8, self.angle.cos() * 11.0);
        let transform = ctx.transform();
        transform.position = eye;
        transform.look_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
    }
}

fn build_world(app: &mut App) {
    app.spawn("Camera").with(Camera::perspective(50.0)).with(Flyby::default());
    app.spawn("Sun")
        .rotation(Quat::from_euler(EulerRot::YXZ, 0.9, -0.6, 0.0))
        .with(Light::default());
    app.spawn("Ground").scale(400.0, 1.0, 400.0).with(
        MeshRenderer::plane()
            .with_color(Color::hex(0x6f7d6a))
            .with_material(0.0, 0.9)
            .with_grid()
            .without_shadows(),
    );
    let colors = [0xff7a59, 0xffc35a, 0x5aa9ff, 0x9b7bff, 0x3ecf7a];
    for (i, color) in colors.iter().enumerate() {
        let angle = i as f32 / colors.len() as f32 * std::f32::consts::TAU;
        app.spawn("Crate")
            .at3(angle.cos() * 3.5, 0.75, angle.sin() * 3.5)
            .scale(1.5, 1.5, 1.5)
            .with(MeshRenderer::cube().with_color(Color::hex(*color)).with_material(0.0, 0.4));
    }
}

fn build_ui(scene: &mut Scene) {
    let canvas = scene.ui_canvas().with(DemoFlow::default()).id();

    // ---------- главное меню ----------
    let menu = scene.ui_panel(canvas).name("MainMenu").anchor(Anchor::Center).width(440.0)
        .column(12.0).padding(36.0).fit().id();
    scene.ui_text(menu, "STAR RAIDERS").font_size(46.0).bold().align(TextAlign::Center).height(58.0);
    scene.ui_text(menu, "Звёздные рейдеры · демо интерфейса").font_size(19.0)
        .text_color(Color::hex(0x8e9bb0)).align(TextAlign::Center).height(40.0);
    scene.ui_button(menu, "play", "Играть").primary();
    scene.ui_button(menu, "settings", "Настройки");
    scene.ui_button(menu, "about", "Об игре");
    scene.ui_button(menu, "quit", "Выход");

    // ---------- настройки ----------
    let settings = scene.ui_panel(canvas).name("Settings").anchor(Anchor::Center).width(560.0)
        .column(14.0).padding(36.0).fit().id();
    scene.ui_text(settings, "Настройки").font_size(36.0).bold().height(50.0);
    setting_slider(scene, settings, "Громкость", "volume", 0.0, 100.0, 70.0, "70%", true);
    setting_slider(scene, settings, "Чувствительность мыши", "sensitivity", 0.1, 5.0, 1.5, "1.5", false);
    scene.ui_toggle(settings, "fullscreen", "Полноэкранный режим", true).switch();
    scene.ui_toggle(settings, "vsync", "Вертикальная синхронизация", false);
    scene.ui_text(settings, "Имя игрока").font_size(18.0).text_color(Color::hex(0x8e9bb0)).height(24.0);
    scene.ui_input(settings, "player_name", "Введите имя…").max_length(24);
    scene.ui_node(settings, "Spacer").height(4.0);
    let buttons = scene.ui_row(settings, 12.0).height(52.0).expand().id();
    scene.ui_button(buttons, "back", "Назад");
    scene.ui_button(buttons, "apply", "Применить").primary();

    // ---------- об игре: перенос строк ----------
    let about = scene.ui_panel(canvas).name("About").anchor(Anchor::Center).width(560.0)
        .column(16.0).padding(36.0).fit().id();
    scene.ui_text(about, "Об игре").font_size(36.0).bold().height(50.0);
    scene.ui_text(about, "Интерфейс собран из обычных компонентов: Canvas, RectTransform, \
        LayoutGroup, Image, Text, Button. The same text engine renders Latin and Cyrillic, \
        wraps long lines and stays sharp at any canvas scale.")
        .font_size(20.0).text_color(Color::hex(0xc5cedb)).valign(VerticalAlign::Top).height(124.0);
    scene.ui_button(about, "back", "Назад");

    // ---------- HUD ----------
    let hud_canvas = scene.ui_canvas().name("HudCanvas").sort_order(-1).no_navigation().id();
    let hud = scene.ui_node(hud_canvas, "Hud").stretch().id();
    let health = scene.ui_panel(hud).name("HealthPanel").anchor(Anchor::TopLeft).pos(24.0, 24.0)
        .size(320.0, 74.0).shadow(16.0).id();
    scene.ui_text(health, "Здоровье").anchor(Anchor::TopLeft).pos(18.0, 10.0).size(160.0, 26.0)
        .font_size(17.0).text_color(Color::hex(0x8e9bb0)).height(26.0);
    scene.ui_text(health, "82 / 100").name("HealthValue").anchor(Anchor::TopRight).pos(-18.0, 10.0)
        .size(120.0, 26.0).font_size(17.0).bold().align(TextAlign::Right).height(26.0);
    scene.ui_progress(health, 0.82).name("HealthBar").anchor(Anchor::StretchBottom).pos(0.0, -16.0)
        .size(-36.0, 14.0);
    scene.ui_text(hud, "Счёт: 1250").name("Score").anchor(Anchor::Top).pos(0.0, 22.0).size(400.0, 48.0)
        .font_size(34.0).bold().align(TextAlign::Center).outline(2.0, Color::rgba(0.0, 0.0, 0.0, 0.8))
        .height(48.0);
    scene.ui_text(hud, "Игрок").name("PlayerName").anchor(Anchor::Top).pos(0.0, 70.0).size(400.0, 28.0)
        .font_size(18.0).bold().align(TextAlign::Center).outline(1.5, Color::rgba(0.0, 0.0, 0.0, 0.75)).height(28.0);
    scene.ui_button(hud, "pause", "Пауза").anchor(Anchor::TopRight).pos(-24.0, 24.0).size(132.0, 48.0);
    scene.ui_text(hud, "Esc — пауза").anchor(Anchor::Bottom).pos(0.0, -20.0).size(400.0, 30.0)
        .font_size(18.0).bold().align(TextAlign::Center).outline(1.5, Color::rgba(0.0, 0.0, 0.0, 0.75)).height(30.0);

    // ---------- пауза ----------
    let pause = scene.ui_image(canvas, Color::rgba(0.0, 0.0, 0.0, 0.55)).name("Pause").stretch().id();
    let pause_menu = scene.ui_panel(pause).anchor(Anchor::Center).width(380.0).column(12.0).padding(32.0)
        .fit().id();
    scene.ui_text(pause_menu, "Пауза").font_size(38.0).bold().align(TextAlign::Center).height(54.0);
    scene.ui_button(pause_menu, "resume", "Продолжить").primary();
    scene.ui_button(pause_menu, "menu", "В главное меню");
}

/// Строка настройки: подпись и значение над ползунком.
#[allow(clippy::too_many_arguments)]
fn setting_slider(
    scene: &mut Scene,
    parent: GameObjectId,
    label: &str,
    message: &str,
    min: f32,
    max: f32,
    value: f32,
    shown: &str,
    whole: bool,
) {
    let row = scene.ui_node(parent, label).height(26.0).id();
    scene.ui_text(row, label).stretch().font_size(19.0);
    let value_name = if message == "volume" { "VolumeValue" } else { "SensitivityValue" };
    scene.ui_text(row, shown).name(value_name).stretch().font_size(19.0).bold()
        .align(TextAlign::Right);
    let slider = scene.ui_slider(parent, message, min, max, value);
    if whole {
        slider.whole_numbers();
    }
}

fn main() -> Result<(), EngineError> {
    let mut app = engine::app();
    app.register::<DemoFlow>().register::<Flyby>();
    build_world(&mut app);
    build_ui(&mut app.world.scene);
    engine::run(app, WindowConfig::titled("UI demo"))
}
