use engine_core::prelude::*;

use crate::screen::ScreenCtx;

component! {
    /// Создаёт префаб каждые `interval` секунд в своей позиции (± `spread` по X) или в
    /// случайном месте по ширине экрана (`across_view`: звёзды, капли, астероиды сверху).
    /// Скрипт может ставить его на паузу (`enabled`) и менять `interval` на ходу —
    /// например, чаще с каждым очком.
    pub struct Spawner {
        /// Создавать (выключено — пауза: часы стоят).
        pub enabled: bool = true,
        /// Путь к префабу в ассетах, например `prefabs/coin.prefab`.
        pub prefab: String = String::new(),
        /// Секунд между появлениями (можно менять во время игры).
        pub interval: f32 = 2.0,
        /// Сколько всего создать (0 — без конца).
        pub count: i32 = 0,
        /// Случайный разброс по X в обе стороны.
        pub spread: f32 = 0.0,
        /// X — случайный по всей ширине видимой области главной камеры (вместо `spread`);
        /// Y — свой.
        pub across_view: bool = false,
        /// Отступ от боковых краёв экрана для `across_view`, единиц мира.
        pub view_margin: f32 = 1.0,
        /// Сколько уже создано.
        pub spawned: i32 = 0,
        /// Часы до следующего появления.
        pub timer: Timer = Timer::repeating(2.0),
    }
}

impl Component for Spawner {
    fn start(&mut self, ctx: &mut Ctx) {
        self.timer = Timer::repeating(self.interval.max(0.01));
        if self.prefab.is_empty() {
            ctx.warn("Spawner: the `prefab` field is empty — set a prefab path in the Inspector");
        }
    }

    fn update(&mut self, ctx: &mut Ctx) {
        if !self.enabled || self.prefab.is_empty() || (self.count > 0 && self.spawned >= self.count) {
            return;
        }
        // `interval` могли поменять на ходу: часы подхватывают новое значение.
        let interval = self.interval.max(0.01);
        if self.timer.duration() != interval {
            self.timer.set_duration(interval);
        }
        for _ in 0..self.timer.tick(ctx.dt()).times_finished() {
            let mut at = ctx.pos();
            if self.across_view {
                let area = ctx.view_rect();
                let half = (area.width * 0.5 - self.view_margin).max(0.0);
                at.x = area.center().x + ctx.rand_range(-half, half);
            } else {
                at.x += ctx.rand_range(-self.spread, self.spread);
            }
            ctx.spawn_at(&self.prefab, at);
            self.spawned += 1;
        }
    }
}
