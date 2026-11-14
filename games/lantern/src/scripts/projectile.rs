//! Медленный плевок.

use crate::common;
use crate::phys::Shared;
use engine::prelude::*;

component! {
    /// Медленный плевок: летит по прямой, разбивается о стену, ранит игрока.
    pub struct Projectile {
        pub vel: Vec2 = Vec2::ZERO,
        pub life: f32 = 7.0,
        pub radius: f32 = 0.22,
    }
}

impl Component for Projectile {
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        self.life -= dt;
        let pos = ctx.world_position().truncate() + self.vel * dt;
        ctx.transform().position = pos.extend(0.0);
        ctx.transform().rotate_z(dt * 6.0);
        let hit_wall = ctx.resource::<Shared>().is_some_and(|s| s.grid.borrow().solid_at(pos));
        if hit_wall || self.life <= 0.0 {
            common::burst(ctx, pos, Color::rgb(2.4, 0.5, 1.2), 0.3);
            ctx.destroy_self();
            return;
        }
        if let Some(p) = common::player_view(ctx) {
            if p.alive && !p.dashing && (p.center - pos).length() < self.radius + 0.3 {
                if !p.invulnerable {
                    let dir = if self.vel.x >= 0.0 { 1.0 } else { -1.0 };
                    ctx.send_message(p.id, "Hurt", Value::Vec3(dir, 0.0, 0.0));
                }
                common::burst(ctx, pos, Color::rgb(2.4, 0.5, 1.2), 0.3);
                ctx.destroy_self();
            }
        }
    }
}
