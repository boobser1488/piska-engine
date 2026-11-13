//! След за движущимся объектом: лента, сужающаяся и тающая к хвосту.

use engine_core::prelude::*;

use crate::draw2d::{push_quad, CameraBasis, SpriteVertex, UvRect};
use crate::particles::ParticleBlend;

#[derive(Clone, Copy, Debug)]
struct TrailPoint {
    position: Vec3,
    age: f32,
}

#[derive(Clone, Debug, Default)]
pub struct TrailRuntime {
    /// От старых к новым.
    points: Vec<TrailPoint>,
    /// Текущая позиция объекта — «голова» ленты.
    head: Option<Vec3>,
}

component_rt! {
    /// Рисует ленту за объектом: точки пути запоминаются, живут `lifetime` секунд,
    /// ширина и цвет плавно меняются от головы к хвосту.
    pub struct TrailRenderer {
        /// Сколько секунд живёт точка следа.
        pub lifetime: f32 = 0.5,
        /// Ширина у головы, мировые единицы.
        pub start_width: f32 = 0.2,
        /// Ширина у хвоста.
        pub end_width: f32 = 0.0,
        pub start_color: Color = Color::WHITE,
        pub end_color: Color = Color::WHITE.with_alpha(0.0),
        /// Новая точка добавляется, когда объект ушёл от последней дальше этого расстояния.
        pub min_vertex_distance: f32 = 0.05,
        /// Потолок числа точек (старейшие отбрасываются).
        pub max_points: i32 = 128,
        /// Добавлять новые точки. `false` — след догорает и исчезает.
        pub emitting: bool = true,
        pub blend: ParticleBlend = ParticleBlend::Alpha,
        pub sorting_layer: i32 = 0,
        pub order: i32 = 0,
    }
    runtime: TrailRuntime
}

impl Component for TrailRenderer {
    /// Остальные хуки движок не вызывает.
    fn hooks(&self) -> Hooks {
        Hooks::LATE_UPDATE
    }

    fn on_late_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let position = ctx.world_position();
        self.track(position, dt);
    }
}

impl TrailRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Свечущий след: сложение цветов, яркая голова.
    pub fn glowing(color: Color, width: f32, lifetime: f32) -> Self {
        Self {
            lifetime,
            start_width: width,
            start_color: color,
            end_color: color.with_alpha(0.0),
            blend: ParticleBlend::Additive,
            ..Self::default()
        }
    }

    pub fn with_layer(mut self, sorting_layer: i32, order: i32) -> Self {
        self.sorting_layer = sorting_layer;
        self.order = order;
        self
    }

    /// Убрать все точки.
    pub fn clear(&mut self) {
        self.rt.points.clear();
        self.rt.head = None;
    }

    pub fn point_count(&self) -> usize {
        self.rt.points.len()
    }

    /// Обновить след: объект сейчас в `position`, прошло `dt`. Зовётся из `on_late_update`.
    pub fn track(&mut self, position: Vec3, dt: f32) {
        let lifetime = self.lifetime.max(1e-3);
        for point in &mut self.rt.points {
            point.age += dt;
        }
        self.rt.points.retain(|p| p.age < lifetime);
        let far_enough = self
            .rt
            .points
            .last()
            .is_none_or(|last| last.position.distance(position) >= self.min_vertex_distance.max(1e-4));
        if self.emitting && far_enough {
            self.rt.points.push(TrailPoint { position, age: 0.0 });
            let limit = self.max_points.max(2) as usize;
            if self.rt.points.len() > limit {
                let extra = self.rt.points.len() - limit;
                self.rt.points.drain(..extra);
            }
        }
        self.rt.head = Some(position);
        if !self.emitting && self.rt.points.is_empty() {
            self.rt.head = None;
        }
    }

    pub(crate) fn write_vertices(&self, basis: CameraBasis, out: &mut Vec<SpriteVertex>) {
        let points = &self.rt.points;
        // Голова — живая позиция объекта, нулевого возраста; при `emitting = false` её нет.
        let head = self.rt.head.filter(|_| self.emitting);
        let count = points.len() + usize::from(head.is_some());
        if count < 2 {
            return;
        }
        let at = |i: usize| -> (Vec3, f32) {
            match points.get(i) {
                Some(p) => (p.position, p.age),
                None => (head.unwrap_or(Vec3::ZERO), 0.0),
            }
        };
        let forward = basis.right.cross(basis.up).normalize_or_zero();
        let lifetime = self.lifetime.max(1e-3);
        // Пара точек по краям ленты в узле i.
        let edge = |i: usize| -> (Vec3, Vec3, [f32; 4]) {
            let (position, age) = at(i);
            let previous = at(i.saturating_sub(1)).0;
            let next = at((i + 1).min(count - 1)).0;
            let direction = (next - previous).normalize_or_zero();
            let mut side = direction.cross(forward).normalize_or_zero();
            if side == Vec3::ZERO {
                side = basis.up;
            }
            let t = (age / lifetime).clamp(0.0, 1.0);
            let width = self.start_width + (self.end_width - self.start_width) * t;
            let (a, b) = (self.start_color, self.end_color);
            let color = [
                a.r + (b.r - a.r) * t,
                a.g + (b.g - a.g) * t,
                a.b + (b.b - a.b) * t,
                a.a + (b.a - a.a) * t,
            ];
            (position + side * (width * 0.5), position - side * (width * 0.5), color)
        };
        out.reserve((count - 1) * 6);
        let mut previous = edge(0);
        for i in 1..count {
            let current = edge(i);
            // Цвет квада — по новому концу сегмента; плавности хватает при частых точках.
            push_quad(
                out,
                [previous.1, current.1, current.0, previous.0],
                UvRect::FULL,
                current.2,
            );
            previous = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_points_for_lifetime_and_spacing() {
        let mut trail = TrailRenderer {
            lifetime: 0.5,
            min_vertex_distance: 0.1,
            ..TrailRenderer::default()
        };
        for i in 0..20 {
            trail.track(Vec3::new(i as f32 * 0.05, 0.0, 0.0), 0.05);
        }
        // Шаг 0.05 < 0.1: точка каждые два-три кадра; живут 0.5 с = 10 кадров.
        let alive = trail.point_count();
        assert!((3..=6).contains(&alive), "{alive}");
        trail.emitting = false;
        for _ in 0..12 {
            trail.track(Vec3::new(5.0, 0.0, 0.0), 0.05);
        }
        assert_eq!(trail.point_count(), 0, "the trail burns out after the object stops emitting");
    }
}
