//! «Звездопад» — шаблон игры на Piska Engine: ловите корзиной падающие звёзды.
//!
//! Маленькая, но полная игра: спрайты из ассетов, физика 2D (тело корзины, триггеры
//! звёзд), префаб, сцена из файла, интерфейс (счёт, жизни, экран «Game over» с
//! кнопкой), звук (клип `.wav` из ассетов и синтезатор) и сохранение рекорда.
//! Одна функция [`build`] собирает игру; `engine::game_main!` делает из неё
//! программу для десктопа, браузера и Android. Как сделать свою игру из шаблона —
//! `docs/api/export.md`.
//!
//! Интро «3… 2… 1… Лови!» и подсказка после первой пойманной звезды написаны на языке
//! Scenario: `assets/scenarios/intro.scenario` (компонент `ScenarioRunner` на объекте
//! Game). Его можно править прямо во время игры — он перезапустится сам.
//!
//! Управление: A/D или ←/→, либо зажать мышь / палец там, куда ехать.

/// Скрипты на Rust, созданные в редакторе (New Rust Script).
pub mod scripts;

use engine::prelude::*;

/// Имя игры: каталог сохранений на десктопе, ключ `localStorage` в браузере.
pub const GAME_NAME: &str = "Piska Starfall";
/// Половина видимой высоты мира (поле `ortho_size` камеры в `scenes/main.scene`).
const ORTHO_SIZE: f32 = 6.0;
/// Ниже этой высоты звезда считается упущенной.
const MISS_HEIGHT: f32 = -5.6;
const LIVES: i32 = 3;

/// Половина видимой ширины мира для текущего размера экрана.
fn half_width(ctx: &Ctx) -> f32 {
    let screen = ctx.input().screen_size();
    let aspect = if screen.y > 0.0 { screen.x / screen.y } else { 16.0 / 9.0 };
    ORTHO_SIZE * aspect
}

component! {
    /// Корзина игрока: едет влево-вправо по оси `Horizontal` или к указателю (мышь, палец).
    pub struct Basket {
        pub speed: f32 = 9.0,
    }
}

impl Component for Basket {
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        let half = half_width(ctx);
        let x = ctx.transform().position.x;
        let mut run = ctx.input().axis("Horizontal");

        // Указатель: палец или зажатая левая кнопка (но не по интерфейсу).
        let screen = ctx.input().screen_size();
        let pointer = match ctx.input().touches().first() {
            Some(touch) => Some(touch.position),
            None if ctx.input().mouse_button(MouseButton::Left) && !ctx.ui().pointer_over_ui() => {
                Some(ctx.input().mouse_position())
            }
            None => None,
        };
        if let (Some(pointer), true) = (pointer, screen.x > 0.0) {
            let target = (pointer.x / screen.x - 0.5) * 2.0 * half;
            run = ((target - x) * 2.0).clamp(-1.0, 1.0);
        }

        let limit = (half - 0.8).max(0.0);
        if (x <= -limit && run < 0.0) || (x >= limit && run > 0.0) {
            run = 0.0;
        }
        if let Some(body) = ctx.get_mut::<Rigidbody2D>() {
            body.velocity.x = run * self.speed;
        }
        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            if run != 0.0 {
                sprite.flip_x = run < 0.0;
            }
        }
    }
}

component! {
    /// Звезда (префаб `prefabs/star.prefab`): поймана корзиной — очко, упала — минус жизнь.
    pub struct Star {}
}

impl Star {
    fn report(ctx: &mut Ctx, what: &str) {
        if let Some(game) = ctx.find_by_name("Game") {
            ctx.send_message(game, what, Value::Nil);
        }
        ctx.destroy_self();
    }
}

impl Component for Star {
    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if ctx.scene().get(other).is_some_and(|o| o.tag == "Player") {
            Self::report(ctx, "Caught");
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if ctx.world_position().y < MISS_HEIGHT {
            Self::report(ctx, "Missed");
        }
    }
}

component! {
    /// Правила: появление звёзд, счёт, жизни, рекорд, экран «Game over».
    pub struct GameManager {
        /// Секунд между звёздами в начале; с каждым очком чуть меньше.
        pub spawn_interval: f32 = 1.0,
        pub score: i32 = 0,
        pub best: i32 = 0,
        pub lives: i32 = 3,
        pub game_over: bool = false,
        /// Сколько звёзд появилось с начала игры.
        pub spawned: i32 = 0,
        pub spawn_timer: f32 = 0.0,
        /// Ждать сообщения `Go` от интро-сценария перед первой звездой.
        pub wait_for_intro: bool = true,
        /// Если интро так и не прислало `Go` (сценарий удалён или с ошибкой), игра
        /// начинается сама через столько секунд.
        pub intro_timeout: f32 = 4.0,
    }
}

impl GameManager {
    fn build_ui(ctx: &mut Ctx) {
        let scene = ctx.scene_mut();
        let canvas = scene.ui_canvas().name("HUD").id();
        let bar = scene
            .ui_row(canvas, 32.0)
            .anchor(Anchor::TopLeft)
            .pos(24.0, 16.0)
            .fit()
            .no_navigation()
            .id();
        scene.ui_text(bar, "Score: 0").name("Score").font_size(30.0).bold().outline(2.0, Color::BLACK);
        scene.ui_text(bar, "Lives: 3").name("Lives").font_size(30.0).outline(2.0, Color::BLACK);
        scene.ui_text(bar, "Best: 0").name("Best").font_size(30.0).outline(2.0, Color::BLACK);

        let panel = scene
            .ui_panel(canvas)
            .name("GameOverPanel")
            .anchor(Anchor::Center)
            .width(440.0)
            .column(16.0)
            .padding(32.0)
            .fit()
            .id();
        scene.ui_text(panel, "Game over").font_size(44.0).bold().align(TextAlign::Center);
        scene.ui_text(panel, "").name("FinalScore").font_size(26.0).align(TextAlign::Center);
        scene.ui_button(panel, "restart", "Play again").primary();
        scene.ui_set_visible(panel, false);

        // Крупная надпись по центру: её ведёт интро-сценарий (`ui.text("Hint", …)`).
        scene
            .ui_text(canvas, "")
            .name("Hint")
            .anchor(Anchor::Center)
            .pos(0.0, -120.0)
            .size(900.0, 80.0)
            .font_size(48.0)
            .bold()
            .align(TextAlign::Center)
            .outline(3.0, Color::BLACK);
    }

    fn set_text(ctx: &mut Ctx, name: &str, text: &str) {
        if let Some(id) = ctx.find_by_name(name) {
            if let Some(label) = ctx.scene_mut().get_component_mut::<Text>(id) {
                label.set(text);
            }
        }
    }

    fn refresh_hud(&self, ctx: &mut Ctx) {
        Self::set_text(ctx, "Score", &format!("Score: {}", self.score));
        Self::set_text(ctx, "Lives", &format!("Lives: {}", self.lives.max(0)));
        Self::set_text(ctx, "Best", &format!("Best: {}", self.best));
    }

    fn play(ctx: &mut Ctx, clip: &str) {
        if let Some(audio) = ctx.resource_mut::<Audio>() {
            audio.play(clip);
        }
    }

    fn spawn_star(&mut self, ctx: &mut Ctx) {
        let half = (half_width(ctx) - 1.0).max(0.5);
        let x = ctx.random().range(-half, half);
        match ctx.instantiate_at("prefabs/star.prefab", Vec3::new(x, ORTHO_SIZE + 0.8, 0.0)) {
            Ok(_) => self.spawned += 1,
            Err(err) => log::error!("cannot spawn a star: {err}"),
        }
    }

    fn end_game(&mut self, ctx: &mut Ctx) {
        self.game_over = true;
        for star in ctx.find_all_by_tag("Star") {
            ctx.destroy(star);
        }
        if self.score > self.best {
            self.best = self.score;
            let save = ctx.save_data();
            save.set_int("best", self.best as i64);
            if let Err(err) = save.save() {
                log::warn!("best score not saved: {err}");
            }
        }
        Self::set_text(ctx, "FinalScore", &format!("Score: {}   Best: {}", self.score, self.best));
        if let Some(panel) = ctx.find_by_name("GameOverPanel") {
            ctx.scene_mut().ui_set_visible(panel, true);
        }
        Self::play(ctx, "lose");
        self.refresh_hud(ctx);
    }

    fn restart(&mut self, ctx: &mut Ctx) {
        self.game_over = false;
        self.score = 0;
        self.lives = LIVES;
        self.spawn_timer = 0.0;
        if let Some(panel) = ctx.find_by_name("GameOverPanel") {
            ctx.scene_mut().ui_set_visible(panel, false);
        }
        Self::play(ctx, "start");
        self.refresh_hud(ctx);
    }
}

impl Component for GameManager {
    fn on_start(&mut self, ctx: &mut Ctx) {
        Self::build_ui(ctx);
        self.best = ctx.save_data().int("best", 0) as i32;
        self.lives = LIVES;
        self.refresh_hud(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if self.game_over {
            if ctx.ui().clicked("restart") || ctx.input().key_down(Key::Enter) {
                self.restart(ctx);
            }
            return;
        }
        if self.wait_for_intro {
            self.intro_timeout -= dt;
            if self.intro_timeout > 0.0 {
                return;
            }
            self.wait_for_intro = false;
        }
        self.spawn_timer += dt;
        let interval = (self.spawn_interval - self.score as f32 * 0.02).max(0.35);
        if self.spawn_timer >= interval {
            self.spawn_timer -= interval;
            self.spawn_star(ctx);
        }
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        if self.game_over {
            return;
        }
        match message.name.as_str() {
            // Интро досчитало: звёзды пошли.
            "Go" => {
                self.wait_for_intro = false;
                return;
            }
            "Caught" => {
                self.score += 1;
                Self::play(ctx, "catch");
            }
            "Missed" => {
                self.lives -= 1;
                Self::play(ctx, "miss");
                if self.lives <= 0 {
                    return self.end_game(ctx);
                }
            }
            _ => return,
        }
        self.refresh_hud(ctx);
    }
}

/// Зарегистрировать компоненты игры (сцена и префаб ссылаются на них по имени).
pub fn register(app: &mut App) {
    scripts::register(app);
    app.register::<Basket>().register::<Star>().register::<GameManager>();
}

/// Собрать игру с заданным хранилищем сохранений (тесты передают `SaveData::in_memory()`).
pub fn build_with(save: SaveData) -> Result<App, EngineError> {
    let mut app = engine::app();
    register(&mut app);
    app.world.resources.insert(save);
    engine::mount_assets(&mut app, engine::assets_dir!())?;

    // Звук из ассетов читается через базу — так он работает и из пака (браузер, APK).
    let database = app
        .world
        .resources
        .get::<AssetDatabase>()
        .expect("mount_assets inserted the database")
        .clone();
    let catch_bytes = database.read_path("sounds/catch.wav")?;
    let catch = AudioClip::from_bytes(&catch_bytes).map_err(|e| engine::assets::AssetError::Import {
        path: "sounds/catch.wav".to_string(),
        reason: e.to_string(),
    })?;
    if let Some(audio) = app.world.resources.get_mut::<Audio>() {
        audio.add_clip("catch", catch);
        audio.add_clip("miss", synth::sfx::hurt());
        audio.add_clip("lose", synth::sfx::lose());
        audio.add_clip("start", synth::sfx::powerup());
        audio.add_clip("tick", synth::sfx::coin());
        audio.add_clip("theme", synth::music::calm());
        audio.play_music("theme", 1.0);
    }

    engine::load_scene(&mut app, "scenes/main.scene")?;
    Ok(app)
}

/// Собрать игру: сохранения — там, где они живут на этой платформе.
pub fn build() -> Result<App, EngineError> {
    build_with(engine::save_data(GAME_NAME))
}

engine::game_main!(build, WindowConfig::titled("Piska Starfall").with_size(1280, 720));
