use engine_core::prelude::*;

component! {
    /// Уничтожает свой объект через `seconds` секунд после появления (пули, эффекты).
    pub struct DestroyAfter {
        /// Через сколько секунд исчезнуть.
        pub seconds: f32 = 3.0,
    }
}

impl Component for DestroyAfter {
    fn start(&mut self, ctx: &mut Ctx) {
        ctx.destroy_self_after(self.seconds);
    }
}
