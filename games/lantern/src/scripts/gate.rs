//! Запертые ворота.

use crate::common;
use crate::phys::Shared;
use crate::sfx;
use engine::prelude::*;

component! {
    /// Запертые ворота: открываются, когда собрано достаточно светлячков.
    pub struct Gate {
        pub need: i32 = 5,
        pub open: bool = false,
        pub nag: f32 = 0.0,
        /// Высота ворот в клетках.
        pub height: i32 = 3,
    }
}

impl Gate {
    fn set_solid(&self, ctx: &Ctx, solid: bool) {
        let p = ctx.world_position();
        let (x, y) = (p.x.floor() as i32, p.y.round() as i32);
        if let Some(shared) = ctx.resource::<Shared>() {
            let mut grid = shared.grid.borrow_mut();
            for dy in 0..self.height {
                grid.set_flags(x, y + dy, if solid { crate::phys::SOLID } else { 0 });
            }
        }
    }
}

impl Component for Gate {
    fn on_start(&mut self, ctx: &mut Ctx) {
        self.set_solid(ctx, true);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if self.open {
            return;
        }
        self.nag = (self.nag - dt).max(0.0);
        let Some(p) = common::player_view(ctx) else { return };
        let pos = ctx.world_position().truncate();
        let near = (p.center.x - pos.x).abs() < 2.0 && p.center.y > pos.y - 0.5 && p.center.y < pos.y + self.height as f32 + 0.5;
        if !near || !p.alive {
            return;
        }
        let fireflies = common::level_state(ctx).map_or(0, |l| l.0);
        if fireflies >= self.need {
            self.open = true;
            self.set_solid(ctx, false);
            sfx::play(ctx, "gate");
            common::shake(ctx, 0.3);
            for k in 0..3 {
                common::dust(ctx, pos + Vec2::new(0.0, 0.5 + k as f32), 1.0);
            }
            let id = ctx.id;
            ctx.start_tween(Tween::position(id, Vec3::new(pos.x, pos.y + 3.2, 0.0)).duration(0.8).ease(Ease::InOutQuad));
            ctx.start_tween(Tween::field(id, "SpriteRenderer.color.a", 0.0f32).duration(0.8).delay(0.3));
            ctx.destroy_self_after(1.2);
        } else if self.nag <= 0.0 {
            self.nag = 2.5;
            sfx::play(ctx, "error");
            common::to_level(ctx, "need_more", Value::Int(self.need as i64));
        }
    }

    fn on_destroy(&mut self, ctx: &mut Ctx) {
        if !self.open {
            self.set_solid(ctx, false);
        }
    }
}
