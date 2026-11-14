use engine::prelude::*;

use crate::game::sounds;

component! {
    /// Звуки игрока: шаги на бегу (шагом и в приседе — тихо, как в классике), прыжок и
    /// приземление. Стоит на префабе игрока рядом с `FpsController`.
    pub struct PlayerSounds {
        /// Расстояние между шагами, м.
        pub stride: f32 = 1.3,
        /// Быстрее этого (ед/с) шаги слышны.
        pub audible_speed: f32 = 150.0,
        pub step_volume: f32 = 0.45,
        pub land_volume: f32 = 0.7,
        /// Пройдено с последнего шага, м (во время игры).
        pub walked: f32 = 0.0,
        /// Какой шаг следующий: левый / правый (во время игры).
        pub left: bool = true,
    }
}

impl Component for PlayerSounds {
    fn hooks(&self) -> Hooks {
        Hooks::FIXED_UPDATE
    }

    fn on_fixed_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(state) = ctx.resource::<FpsRuntime>().and_then(|r| r.state(ctx.id)).copied() else {
            return;
        };
        let Some(controller) = ctx.get::<FpsController>() else { return };
        if controller.noclip {
            return;
        }
        if state.jumped {
            sounds::play(ctx.world, "jump", 0.35, 1.0);
        }
        if state.landed && state.land_speed > 2.0 {
            let volume = (self.land_volume * (state.land_speed / 8.0)).clamp(0.2, self.land_volume);
            sounds::play(ctx.world, "land", volume, 1.0);
            self.walked = 0.0;
        }
        if !state.on_ground {
            return;
        }
        let speed = state.units_per_second();
        if speed < self.audible_speed {
            self.walked = 0.0;
            return;
        }
        self.walked += speed * fps_units() * dt;
        if self.walked >= self.stride {
            self.walked -= self.stride;
            self.left = !self.left;
            let pitch = 0.94 + ctx.random().range(0.0, 0.12);
            let clip = if self.left { "step_a" } else { "step_b" };
            sounds::play(ctx.world, clip, self.step_volume, pitch);
        }
    }
}

fn fps_units() -> f32 {
    engine::fps::UNIT
}
