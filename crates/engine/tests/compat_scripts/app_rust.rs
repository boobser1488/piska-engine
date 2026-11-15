//! Rust — скрипт на Rust. Он вешается на объект так же, как Light или MeshRenderer:
//! в инспекторе — Add Component → Rust.
//!
//! Код вкомпилирован в программу: после правки нажмите Build (Ctrl+B) — редактор
//! пересоберёт игру и перезапустится с новым кодом, сцена и выделение останутся.

use engine::prelude::*;

component! {
    /// Rust: опишите здесь, что делает скрипт, — это подсказка в инспекторе.
    pub struct Rust {
        /// Скорость вращения, градусов в секунду. Поля из `component!` видны в
        /// инспекторе и сохраняются в сцену.
        pub speed: f32 = 90.0,
    }
}

impl Component for Rust {
    /// Один раз, перед первым кадром объекта: найти другие объекты, прочитать настройки.
    fn on_start(&mut self, _ctx: &mut Ctx) {}

    /// Каждый кадр. Пример: вращать объект вокруг вертикальной оси.
    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        ctx.transform().rotate_y(self.speed.to_radians() * dt);
    }

    // Другие хуки (переопределяйте только нужные): on_fixed_update (шаг физики),
    // on_late_update (камеры), on_message, on_collision_enter, on_trigger_enter,
    // on_destroy. Ввод, время, таймеры, твины, сцены, сохранения, случайность —
    // docs/api/gameplay.md.
}
