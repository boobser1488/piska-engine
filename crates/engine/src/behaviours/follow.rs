use engine_core::prelude::*;

component! {
    /// Плавно следует за объектом `target` (камера за игроком, питомец за героем).
    pub struct Follow {
        /// За кем следовать — перетащите объект на поле в инспекторе.
        pub target: ObjectRef = ObjectRef::NONE,
        /// Смещение от цели (камере 2D обычно нужен только z).
        pub offset: Vec3 = Vec3::ZERO,
        /// Насколько быстро догоняет: 0 — мгновенно, 5 — мягко, 1 — очень лениво.
        pub smoothing: f32 = 5.0,
    }
}

impl Component for Follow {
    // После всех `update`: цель уже сдвинулась в этом кадре — камера не отстаёт на кадр.
    fn late_update(&mut self, ctx: &mut Ctx) {
        let target = ctx.obj(self.target);
        if !ctx.exists(target) {
            return;
        }
        let goal = ctx.on(target).pos() + self.offset;
        let pos = if self.smoothing <= 0.0 {
            goal
        } else {
            // Экспоненциальное сглаживание: одинаково при любом FPS.
            goal + (ctx.pos() - goal) * (-self.smoothing * ctx.dt()).exp()
        };
        ctx.set_pos(pos);
    }
}
