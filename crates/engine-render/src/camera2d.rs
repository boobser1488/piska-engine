//! Помощник 2D-камеры: слежение за объектом (по тегу) и тряска.

use engine_core::prelude::*;

/// Сообщение «потряси камеру»: значение — `Value::Float(травма 0..1)`.
/// `ctx.send_message(camera, "shake", Value::Float(0.4))`.
pub const SHAKE_MESSAGE: &str = "shake";

#[derive(Clone, Debug, Default)]
pub struct CameraRuntime {
    /// Смещение от тряски, добавленное к позиции на прошлом кадре.
    last_offset: Vec2,
    time: f32,
    snapped: bool,
}

/// Гладкий шум в [-1, 1]: интерполяция случайных значений в целых точках.
fn noise(t: f32, channel: u32) -> f32 {
    let hash = |i: i32| -> f32 {
        let mut x = (i as u32).wrapping_mul(0x9E37_79B1) ^ channel.wrapping_mul(0x85EB_CA6B);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 12;
        (x & 0xFFFF) as f32 / 32767.5 - 1.0
    };
    let i = t.floor();
    let f = t - i;
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash(i as i32);
    let b = hash(i as i32 + 1);
    a + (b - a) * s
}

component_rt! {
    /// Слежение и тряска для 2D-камеры (вешается на объект с `Camera`).
    /// `target_tag` — тег объекта, за которым едет камера (пусто — не следить).
    /// Тряска по «травме»: `add_shake(0.5)` — удар, затухает сама.
    pub struct CameraController2D {
        /// Тег объекта, за которым следит камера.
        pub target_tag: String = String::new(),
        /// Смещение камеры относительно цели.
        pub offset: Vec2 = Vec2::new(0.0, 0.5),
        /// Скорость сглаживания, 1/с: больше — резче. 0 — камера прилипает к цели.
        pub smoothing: f32 = 6.0,
        /// Полуразмер «мёртвой зоны»: пока цель внутри, камера стоит.
        pub dead_zone: Vec2 = Vec2::ZERO,
        /// Ограничить центр камеры прямоугольником `bounds_min`…`bounds_max`.
        pub use_bounds: bool = false,
        pub bounds_min: Vec2 = Vec2::new(-100.0, -100.0),
        pub bounds_max: Vec2 = Vec2::new(100.0, 100.0),
        /// Текущая «травма» тряски 0..1; сдвиг пропорционален её квадрату.
        pub trauma: f32 = 0.0,
        /// Насколько быстро травма спадает, 1/с.
        pub shake_decay: f32 = 1.6,
        /// Наибольший сдвиг при травме 1, мировые единицы.
        pub shake_max_offset: Vec2 = Vec2::new(0.5, 0.5),
        /// Частота тряски, Гц.
        pub shake_frequency: f32 = 24.0,
    }
    runtime: CameraRuntime
}

impl Component for CameraController2D {
    /// Остальные хуки движок не вызывает.
    fn hooks(&self) -> Hooks {
        Hooks::LATE_UPDATE | Hooks::MESSAGE
    }

    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let target = if self.target_tag.is_empty() {
            None
        } else {
            ctx.find_by_tag(&self.target_tag)
                .map(|id| ctx.world.scene.world_position(id).truncate())
        };
        let position = ctx.transform().position;
        let resting = position.truncate() - self.rt.last_offset;
        let (base, offset) = self.step(resting, target, dt);
        ctx.transform().position = base.extend(position.z) + offset.extend(0.0);
    }

    fn on_message(&mut self, _ctx: &mut Ctx, message: &Message) {
        if message.name == SHAKE_MESSAGE {
            if let Value::Float(amount) = message.value {
                self.add_shake(amount);
            }
        }
    }
}

impl CameraController2D {
    pub fn following(tag: &str) -> Self {
        Self {
            target_tag: tag.to_string(),
            ..Self::default()
        }
    }

    pub fn with_smoothing(mut self, smoothing: f32) -> Self {
        self.smoothing = smoothing;
        self
    }

    pub fn with_offset(mut self, x: f32, y: f32) -> Self {
        self.offset = Vec2::new(x, y);
        self
    }

    /// Центр камеры не выйдет за прямоугольник (например, границы уровня за вычетом
    /// половины видимой области).
    pub fn with_bounds(mut self, min: Vec2, max: Vec2) -> Self {
        self.use_bounds = true;
        self.bounds_min = min;
        self.bounds_max = max;
        self
    }

    /// Добавить травму (0..1); общая травма не больше 1.
    pub fn add_shake(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
    }

    /// Шаг камеры: из положения покоя `resting` к цели; возвращает новое положение
    /// покоя и сдвиг тряски.
    pub fn step(&mut self, resting: Vec2, target: Option<Vec2>, dt: f32) -> (Vec2, Vec2) {
        let mut base = resting;
        if let Some(target) = target {
            let desired = target + self.offset;
            if !self.rt.snapped {
                self.rt.snapped = true;
                base = desired;
            } else {
                let mut delta = desired - base;
                for axis in 0..2 {
                    let zone = self.dead_zone[axis].max(0.0);
                    delta[axis] = if delta[axis].abs() <= zone {
                        0.0
                    } else {
                        delta[axis] - zone * delta[axis].signum()
                    };
                }
                let k = if self.smoothing > 0.0 {
                    1.0 - (-self.smoothing * dt).exp()
                } else {
                    1.0
                };
                base += delta * k;
            }
        }
        if self.use_bounds {
            base = base.clamp(self.bounds_min.min(self.bounds_max), self.bounds_min.max(self.bounds_max));
        }
        self.rt.time += dt;
        self.trauma = (self.trauma - self.shake_decay.max(0.0) * dt).max(0.0);
        let power = self.trauma * self.trauma;
        let phase = self.rt.time * self.shake_frequency;
        let offset = if power > 0.0 {
            Vec2::new(noise(phase, 1), noise(phase, 2)) * self.shake_max_offset * power
        } else {
            Vec2::ZERO
        };
        self.rt.last_offset = offset;
        (base, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_with_smoothing_and_respects_bounds() {
        let mut camera = CameraController2D::following("Player")
            .with_offset(0.0, 0.0)
            .with_bounds(Vec2::new(-1.0, -1.0), Vec2::new(5.0, 5.0));
        let target = Some(Vec2::new(10.0, 0.0));
        let (first, _) = camera.step(Vec2::ZERO, target, 1.0 / 60.0);
        assert_eq!(first.x, 5.0, "the first frame snaps to the target, then bounds clamp it");
        let mut camera = CameraController2D::following("Player").with_offset(0.0, 0.0);
        let mut position = Vec2::ZERO;
        camera.step(position, Some(Vec2::ZERO), 0.016);
        for _ in 0..60 {
            position = camera.step(position, Some(Vec2::new(4.0, 0.0)), 1.0 / 60.0).0;
        }
        assert!(position.x > 3.5 && position.x < 4.0, "{}", position.x);
    }

    #[test]
    fn shake_decays_to_zero() {
        let mut camera = CameraController2D::default();
        camera.add_shake(1.0);
        let (_, offset) = camera.step(Vec2::ZERO, None, 0.016);
        assert!(offset.length() > 0.0);
        for _ in 0..120 {
            camera.step(Vec2::ZERO, None, 0.016);
        }
        assert_eq!(camera.trauma, 0.0);
        assert_eq!(camera.step(Vec2::ZERO, None, 0.016).1, Vec2::ZERO);
    }
}
