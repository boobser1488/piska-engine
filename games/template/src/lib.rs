//! «Звездопад» — шаблон игры на Piska Engine: ловите корзиной падающие звёзды.
//!
//! Маленькая, но полная игра: спрайты, физика 2D, префаб, сцена, интерфейс (счёт, жизни,
//! экран «Game over» с кнопкой), звук, музыка и сохранение рекорда. Большая часть игры —
//! данные, а не код; всё это видно и правится в редакторе:
//!
//! * `assets/scenes/main.scene` — камера, корзина, объект Game и интерфейс. Корзину везёт
//!   готовое поведение `Paddle2D` (клавиши, палец или мышь, край экрана), звёзды
//!   создаёт `Spawner` на объекте Game.
//! * `assets/prefabs/star.prefab` — звезда: `Collectible` (поймана → «Caught» и звук) и
//!   `OffScreen` (упала за край → «Missed»).
//! * `assets/scenarios/intro.scenario` — интро «3… 2… 1… Лови!» на языке Scenario.
//!
//! Здесь — только правила: счёт, жизни, рекорд, конец игры и рестарт. Короткие формы
//! (`ctx.ui_text`, `ctx.play`, `ctx.save`…) — в шпаргалке: `docs/api/gameplay.md` и в
//! редакторе Help → Scripting Cheat Sheet. Как сделать свою игру из шаблона —
//! `docs/api/export.md`.

/// Скрипты на Rust, созданные в редакторе (New Rust Script).
pub mod scripts;

use engine::behaviours::Spawner;
use engine::prelude::*;

// Игра: имя (окно, сохранения), стартовая сцена и компоненты на Rust. Отсюда же точки
// входа для десктопа, браузера и Android.
engine::game! {
    name: "Piska Starfall",
    scene: "scenes/main.scene",
    components: [GameManager],
}

component! {
    /// Правила: счёт, жизни, рекорд, экран «Game over».
    pub struct GameManager {
        pub score: i32 = 0,
        pub lives: i32 = 3,
        pub best: i32 = 0,
        pub game_over: bool = false,
    }
}

impl GameManager {
    fn show_score(&self, ctx: &mut Ctx) {
        ctx.ui_text("Score", format!("Score: {}", self.score));
        ctx.ui_text("Lives", format!("Lives: {}", self.lives));
        ctx.ui_text("Best", format!("Best: {}", self.best));
    }

    fn end_game(&mut self, ctx: &mut Ctx) {
        self.game_over = true;
        ctx.me_mut::<Spawner>().enabled = false;
        for star in ctx.find_all_tag("Star") {
            ctx.destroy(star);
        }
        if self.score > self.best {
            self.best = self.score;
            ctx.save("best", self.best);
        }
        ctx.ui_text("FinalScore", format!("Score: {}   Best: {}", self.score, self.best));
        ctx.ui_show("GameOverPanel");
        ctx.play("sfx:lose");
    }
}

impl Component for GameManager {
    fn start(&mut self, ctx: &mut Ctx) {
        self.best = ctx.load("best", 0);
        self.show_score(ctx);
        // Музыка не начинается заново, если она уже играет (после рестарта).
        ctx.music("music:calm");
        // Звёзды пойдут по сигналу интро ("Go"); нет интро (удалили, ошибка) — через 4 с.
        ctx.delay(4.0, |ctx| ctx.me_mut::<Spawner>().enabled = true);
    }

    fn update(&mut self, ctx: &mut Ctx) {
        // Новая игра — та же сцена заново: счёт, жизни, звёзды и интерфейс как в начале.
        if self.game_over && (ctx.clicked("restart") || ctx.key_pressed(Key::Enter)) {
            ctx.reload_scene();
        }
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        if self.game_over {
            return;
        }
        if message.is("Go") {
            ctx.me_mut::<Spawner>().enabled = true;
        } else if message.is("Caught") {
            self.score += 1;
            // С каждым очком звёзды падают чуть чаще.
            ctx.me_mut::<Spawner>().interval = (1.0 - self.score as f32 * 0.02).max(0.35);
        } else if message.is("Missed") {
            self.lives -= 1;
            ctx.play("sfx:hurt");
            if self.lives == 0 {
                self.end_game(ctx);
            }
        }
        self.show_score(ctx);
    }
}
