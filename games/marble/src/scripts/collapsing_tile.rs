use engine::prelude::*;

use crate::game::util;

reflect_enum! {
    /// Состояние хрупкой плиты.
    pub enum TileState { Idle, Warn, Fall, Gone }
}

component! {
    /// Хрупкая плита: пока на ней стоит шар, она трескается (дрожит), потом проваливается,
    /// а через несколько секунд возвращается на место. Кинематическое тело с коллайдером.
    /// Шар ищем сами (`overlap_sphere` над плитой): событие касания кинематики ненадёжно,
    /// а такой опрос работает и для медленно подкатившегося шара.
    pub struct CollapsingTile {
        /// Сколько плита держится под шаром, прежде чем провалиться, с.
        pub delay: f32 = 0.7,
        /// Через сколько вернуться, с.
        pub respawn: f32 = 5.0,
        /// Амплитуда дрожи, м.
        pub shake: f32 = 0.04,
        pub state: TileState = TileState::Idle,
        pub timer: f32 = 0.0,
        /// Насколько уже упала.
        pub drop: f32 = 0.0,
        pub fall_speed: f32 = 0.0,
        /// Исходное положение (запоминается при старте).
        pub origin: Vec3 = Vec3::ZERO,
    }
}

impl CollapsingTile {
    fn ball_on_top(&self, ctx: &Ctx) -> bool {
        let half = ctx.object().transform.scale * 0.5;
        let center = ctx.world_position();
        let Some(ball) = util::find_ball_near(ctx, center + Vec3::Y * (half.y + 0.5), half.x + half.z + 1.2) else {
            return false;
        };
        let at = ctx.scene().world_position(ball);
        let d = util::to_local_axes(ctx.scene(), ctx.id, at);
        d.x.abs() <= half.x + 0.25 && d.z.abs() <= half.z + 0.25 && d.y > half.y && d.y < half.y + 1.3
    }
}

impl Component for CollapsingTile {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.origin = ctx.transform().position;
    }

    fn on_fixed_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        if self.state == TileState::Idle && self.ball_on_top(ctx) {
            self.state = TileState::Warn;
            self.timer = 0.0;
            let at = ctx.world_position();
            util::play_at(ctx, "crack", at, 0.9);
        }
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        match self.state {
            TileState::Idle => {}
            TileState::Warn => {
                self.timer += dt;
                let k = self.shake * (0.4 + 0.6 * self.timer / self.delay.max(0.05));
                let random = ctx.random();
                let jitter = Vec3::new(random.range(-k, k), 0.0, random.range(-k, k));
                ctx.transform().position = self.origin + jitter;
                if self.timer >= self.delay {
                    self.state = TileState::Fall;
                    self.timer = 0.0;
                    self.drop = 0.0;
                    self.fall_speed = 0.0;
                    let at = ctx.world_position();
                    util::play_at(ctx, "crack", at, 1.0);
                }
            }
            TileState::Fall => {
                self.fall_speed += 22.0 * dt;
                self.drop += self.fall_speed * dt;
                ctx.transform().position = self.origin - Vec3::Y * self.drop;
                if self.drop > 60.0 {
                    self.state = TileState::Gone;
                    self.timer = 0.0;
                }
            }
            TileState::Gone => {
                self.timer += dt;
                if self.timer >= self.respawn {
                    self.state = TileState::Idle;
                    ctx.transform().position = self.origin;
                }
            }
        }
    }
}
