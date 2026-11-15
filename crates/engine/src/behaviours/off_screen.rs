use engine_core::prelude::*;

use crate::screen::ScreenCtx;

component! {
    /// Объект ушёл за край экрана (видимой области главной камеры): сообщение `message`
    /// ему самому и объекту `notify`, затем он исчезает (`destroy`). Пули, упавшие
    /// предметы, пролетевшие враги. Считается только после того, как объект побывал на
    /// экране: созданный за краем (звезда над экраном) сначала должен въехать
    /// (Godot: `VisibleOnScreenNotifier2D.screen_exited`, Unity: `OnBecameInvisible`).
    pub struct OffScreen {
        /// Имя сообщения.
        pub message: String = String::from("OffScreen"),
        /// Имя объекта, которому тоже сообщить (пусто — только себе).
        pub notify: String = String::new(),
        /// Уничтожить объект, когда ушёл.
        pub destroy: bool = true,
        /// Насколько за край (единиц мира) нужно уйти; отрицательное — сработать, ещё
        /// не дойдя до края (и «въехать» считается на столько же внутрь).
        pub margin: f32 = 0.5,
        /// Объект уже был на экране.
        pub seen: bool = false,
    }
}

impl Component for OffScreen {
    fn update(&mut self, ctx: &mut Ctx) {
        let inside = ctx.view_rect().expand(self.margin).contains(ctx.pos2());
        if inside {
            self.seen = true;
            return;
        }
        if !self.seen {
            return;
        }
        self.seen = false;
        let me = ctx.id;
        ctx.send(me, &self.message);
        if let Some(target) = ctx.find_by_name(&self.notify) {
            ctx.send(target, &self.message);
        }
        if self.destroy {
            ctx.destroy_self();
        }
    }
}
