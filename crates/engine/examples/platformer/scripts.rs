//! Игровые скрипты платформера. Подключаются и примером, и интеграционным тестом.

use engine::prelude::*;

component! {
    /// Управление игроком: бег по оси `Horizontal`, прыжок по кнопке `Jump`.
    pub struct PlayerController {
        pub speed: f32 = 6.0,
        pub jump_speed: f32 = 11.0,
        pub coins: i32 = 0,
        /// Ниже этой высоты игрок считается упавшим и возвращается на старт.
        pub fall_limit: f32 = -30.0,
        /// Откуда игрок начал уровень (запоминается при старте).
        pub spawn_point: Vec3 = Vec3::ZERO,
    }
}

impl PlayerController {
    /// Стоим ли на чём-то твёрдом: два коротких луча вниз от краёв коллайдера.
    fn is_grounded(ctx: &mut Ctx) -> bool {
        // Летим вверх — значит, не стоим (в том числе пока пролетаем сквозь
        // одностороннюю платформу снизу).
        if ctx.get::<Rigidbody2D>().is_some_and(|body| body.velocity.y > 0.5) {
            return false;
        }
        let position = ctx.world_position().truncate();
        let half = ctx.get::<Collider2D>().map_or(Vec2::splat(0.5), |c| c.size * 0.5);
        let Some(physics) = ctx.resource::<Physics2D>() else {
            return false;
        };
        [-0.9, 0.9].into_iter().any(|side| {
            let origin = position + Vec2::new(half.x * side, 0.0);
            physics
                .raycast(origin, Vec2::NEG_Y, half.y + 0.08, Some(ctx.id))
                .is_some()
        })
    }
}

impl Component for PlayerController {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.spawn_point = ctx.transform().position;
    }

    // Весь ввод читается прямо здесь: в `on_fixed_update` «нажато» означает
    // «нажато с прошлого шага физики», поэтому нажатие нельзя пропустить.
    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if ctx.transform().position.y < self.fall_limit {
            ctx.transform().position = self.spawn_point;
            if let Some(body) = ctx.get_mut::<Rigidbody2D>() {
                body.velocity = Vec2::ZERO;
            }
            return;
        }

        let run = ctx.input().axis("Horizontal") * self.speed;
        let jump = ctx.input().button_down("Jump") && Self::is_grounded(ctx);

        if let Some(sprite) = ctx.get_mut::<SpriteRenderer>() {
            if run != 0.0 {
                sprite.flip_x = run < 0.0;
            }
        }
        let Some(body) = ctx.get_mut::<Rigidbody2D>() else {
            return;
        };
        body.velocity.x = run;
        if jump {
            body.velocity.y = self.jump_speed;
        }
    }

    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        if let ("AddCoins", Value::Int(amount)) = (message.name.as_str(), &message.value) {
            self.coins += *amount as i32;
            log::info!("coins: {}", self.coins);
        }
    }
}

component! {
    /// Монета: при входе игрока в триггер начисляет очки и исчезает.
    pub struct Coin {
        pub value: i32 = 1,
    }
}

impl Component for Coin {
    fn on_trigger_enter(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if ctx.scene().get(other).is_some_and(|o| o.tag == "Player") {
            ctx.send_message(other, "AddCoins", Value::Int(self.value as i64));
            ctx.destroy_self();
        }
    }
}

component! {
    /// Плавно ведёт объект (камеру) за целью с заданным тегом.
    pub struct CameraFollow {
        pub target_tag: String = String::from("Player"),
        /// Чем больше, тем жёстче камера привязана к цели.
        pub smoothing: f32 = 6.0,
        pub offset: Vec2 = Vec2::new(0.0, 1.5),
    }
}

impl Component for CameraFollow {
    // late_update: цель в этом кадре уже сдвинулась, камера не отстаёт на кадр.
    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(target) = ctx.find_by_tag(&self.target_tag) else {
            return;
        };
        let goal = ctx.scene().world_position(target).truncate() + self.offset;
        // Экспоненциальное сглаживание, не зависящее от частоты кадров.
        let blend = 1.0 - (-self.smoothing * dt).exp();
        let transform = ctx.transform();
        let position = transform.position.truncate().lerp(goal, blend);
        transform.position = position.extend(transform.position.z);
    }
}

/// Зарегистрировать скрипты, чтобы сцена могла ссылаться на них по имени.
pub fn register(app: &mut App) {
    app.register::<PlayerController>()
        .register::<Coin>()
        .register::<CameraFollow>();
}
