use engine::prelude::*;

use crate::game::flow;
use crate::game::state::GameState;

component! {
    /// Менеджер игры: стоит на объекте «Game» каждой сцены (меню и цехов). Принимает клики
    /// интерфейса и сообщения механизмов, ведёт правила: отсчёт, время, контрольные
    /// точки, медали, сохранения. Само состояние — ресурс `GameState` (он переживает смену
    /// сцены); логика — `game::flow`.
    pub struct GameManager {
        /// Номер цеха этой сцены, 0..7; -1 — главное меню.
        pub course: i32 = -1,
        /// Куда смотрит камера в начале цеха, градусы.
        pub start_yaw_deg: f32 = 0.0,
    }
}

/// Выполнить `f` с состоянием игры, временно вынутым из ресурсов (чтобы свободно
/// пользоваться `ctx`).
fn with_state(ctx: &mut Ctx, f: impl FnOnce(&mut Ctx, &mut GameState)) {
    let Some(mut state) = ctx.world.resources.remove::<GameState>() else {
        log::error!("GameState resource is missing: build the game with marble::build()");
        return;
    };
    f(ctx, &mut state);
    ctx.world.resources.insert(state);
}

impl Component for GameManager {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let (course, yaw) = (self.course, self.start_yaw_deg);
        with_state(ctx, |ctx, st| flow::enter_scene(ctx, st, course, yaw));
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        with_state(ctx, |ctx, st| flow::update(ctx, st, dt));
    }

    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        with_state(ctx, |ctx, st| flow::handle_message(ctx, st, message));
    }
}
