//! Частицы: CPU-симуляция в `on_update`, отрисовка — billboard-квадами через общий
//! батчинг спрайтов (один заход на систему, без аллокаций на частицу).

use std::sync::atomic::{AtomicU64, Ordering};

use engine_assets::{AssetHandle, Texture};
use engine_core::prelude::*;

use crate::draw2d::{push_quad, CameraBasis, SpriteVertex, UvInset, UvRect};

reflect_enum! {
    /// Откуда вылетают частицы и куда летят.
    /// `Point` — из точки, `Circle` — из диска радиуса `shape_size.x` наружу,
    /// `Sphere` — из шара (в `flat`-режиме — из диска) наружу, `Cone` — веером вокруг
    /// оси +Y объекта (полуугол `cone_angle`), `Box` — из прямоугольника `shape_size`.
    pub enum EmitterShape { Point, Circle, Sphere, Cone, Box }
}

reflect_enum! {
    /// В каких координатах живут частицы: `World` — остаются на месте, когда
    /// эмиттер уезжает (дым, искры); `Local` — едут вместе с эмиттером (ореол, щит).
    pub enum SimulationSpace { World, Local }
}

reflect_enum! {
    /// `Alpha` — обычное смешивание; `Additive` — свет складывается (огонь, искры,
    /// свечение; в HDR-сцене с блумом такие частицы светятся).
    pub enum ParticleBlend { Alpha, Additive }
}

reflect_enum! {
    /// Встроенная форма частицы, если не назначена текстура.
    pub enum ParticleShape { Soft, Circle, Square }
}

/// Одна живая частица (только для чтения: `ParticleSystem::particles`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// В мировых или локальных координатах — по `SimulationSpace` системы.
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    /// Радианы.
    pub rotation: f32,
    pub rotation_speed: f32,
    pub color: Color,
    /// Начальный кадр листа (случайный), если лист не привязан к времени жизни.
    pub frame: u32,
}

#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    /// splitmix64.
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// [0, 1)
    fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Равномерно между `range.x` и `range.y`.
    fn range(&mut self, range: Vec2) -> f32 {
        range.x + (range.y - range.x) * self.f32()
    }
}

/// Рабочее состояние системы.
#[derive(Clone, Debug, Default)]
pub struct ParticleRuntime {
    particles: Vec<Particle>,
    rng: Option<Rng>,
    /// Секунд от начала текущего цикла эмиссии.
    time: f32,
    rate_accumulator: f32,
    burst_done: Vec<bool>,
    pending_burst: u32,
    /// Эмиссия закончена (не зацикленная система проработала `duration`).
    done: bool,
}

/// Счётчик для систем без заданного `seed`: каждая получает свой поток случайных чисел.
static SEED_COUNTER: AtomicU64 = AtomicU64::new(1);

component_rt! {
    /// Система частиц (CPU). Работает и в 2D, и в 3D (частицы — billboard'ы, всегда
    /// лицом к камере). Диапазоны — `Vec2(min, max)`; значение берётся случайно между ними.
    /// Готовые настройки: `ParticleSystem::explosion()`, `sparks()`, `smoke()`, `dust()`,
    /// `pickup_sparkle()`, `fire()`.
    pub struct ParticleSystem {
        /// Испускать ли частицы. `false` — новые не рождаются, живые доживают.
        pub playing: bool = true,
        /// Длительность цикла эмиссии, секунд.
        pub duration: f32 = 5.0,
        /// Повторять цикл бесконечно. `false` — один проход (взрыв, вспышка).
        pub looping: bool = true,
        /// Когда эмиссия закончилась и частиц не осталось, удалить объект.
        pub destroy_when_done: bool = false,
        /// Частиц в секунду.
        pub emission_rate: f32 = 10.0,
        /// Залпы: `Vec2(время от начала цикла, число частиц)`.
        pub bursts: Vec<Vec2> = Vec::new(),
        /// Потолок живых частиц; лишние не рождаются.
        pub max_particles: i32 = 1000,
        /// Время жизни, секунд (min, max).
        pub lifetime: Vec2 = Vec2::new(0.8, 1.2),
        /// Начальная скорость, м/с (min, max).
        pub start_speed: Vec2 = Vec2::new(1.0, 2.0),
        /// Начальный размер в мировых единицах (min, max).
        pub start_size: Vec2 = Vec2::new(0.15, 0.25),
        /// Начальный поворот, градусы (min, max).
        pub start_rotation: Vec2 = Vec2::ZERO,
        /// Скорость вращения, градусы/с (min, max).
        pub rotation_speed: Vec2 = Vec2::ZERO,
        /// Начальный цвет: каждая частица берёт случайную точку между двумя цветами.
        /// Значения больше 1 светятся (блум).
        pub start_color: Color = Color::WHITE,
        pub start_color_max: Color = Color::WHITE,
        /// Ускорение в мировых координатах, м/с² (гравитация: (0, -9.81, 0)).
        pub gravity: Vec3 = Vec3::ZERO,
        /// Сопротивление воздуха: скорость затухает как 1 / (1 + drag · t).
        pub drag: f32 = 0.0,
        /// Множитель цвета по времени жизни: ключи равномерно от рождения до смерти
        /// (2–3 ключа — градиент). Пусто — без изменений.
        pub color_over_lifetime: Vec<Color> = vec![Color::WHITE, Color::WHITE.with_alpha(0.0)],
        /// Множитель размера по времени жизни: ключи равномерно. Пусто — без изменений.
        pub size_over_lifetime: Vec<f32> = Vec::new(),
        pub shape: EmitterShape = EmitterShape::Cone,
        /// Размер формы: радиус в `x` для Circle/Sphere/Cone, габариты для Box.
        pub shape_size: Vec3 = Vec3::new(0.1, 0.1, 0.1),
        /// Полуугол веера для Point / Cone / Box, градусы (180 — во все стороны).
        pub cone_angle: f32 = 25.0,
        /// Только плоскость XY: для 2D-сцен частицы не уходят по Z.
        pub flat: bool = false,
        pub space: SimulationSpace = SimulationSpace::World,
        pub blend: ParticleBlend = ParticleBlend::Alpha,
        /// Текстура частицы. Без неё — встроенная форма `particle_shape`.
        pub texture: AssetHandle<Texture> = AssetHandle::none(),
        pub particle_shape: ParticleShape = ParticleShape::Soft,
        /// Лист спрайтов текстуры: столбцов.
        pub sheet_columns: u32 = 1,
        pub sheet_rows: u32 = 1,
        /// Листать кадры от рождения до смерти (иначе у каждой частицы случайный кадр).
        pub sheet_over_lifetime: bool = false,
        /// Вытягивание вдоль скорости: добавка к длине = скорость × значение
        /// (искры-штрихи). 0 — обычный квадрат.
        pub velocity_stretch: f32 = 0.0,
        pub sorting_layer: i32 = 0,
        pub order: i32 = 0,
        /// Зерно случайных чисел: с одинаковым зерном система ведёт себя одинаково
        /// (тесты, реплеи). 0 — каждый раз по-разному.
        pub seed: i32 = 0,
    }
    runtime: ParticleRuntime
}

impl Component for ParticleSystem {
    /// Остальные хуки движок не вызывает.
    fn hooks(&self) -> Hooks {
        Hooks::UPDATE
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        let matrix = ctx.world.scene.world_matrix(ctx.id);
        if self.simulate(dt, matrix) {
            ctx.destroy_self();
        }
    }
}

// ---------------------------------------------------------------------------
// Симуляция
// ---------------------------------------------------------------------------

fn due_bursts(bursts: &[Vec2], done: &mut [bool], limit: f32) -> u32 {
    let mut count = 0;
    for (burst, done) in bursts.iter().zip(done.iter_mut()) {
        if !*done && burst.x <= limit {
            *done = true;
            count += burst.y.max(0.0).round() as u32;
        }
    }
    count
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Значение кривой с ключами, равномерно распределёнными на 0..1.
fn sample_keys<T: Copy>(keys: &[T], t: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    match keys {
        [] => None,
        [only] => Some(*only),
        _ => {
            let scaled = t.clamp(0.0, 1.0) * (keys.len() - 1) as f32;
            let index = (scaled as usize).min(keys.len() - 2);
            Some(lerp(keys[index], keys[index + 1], scaled - index as f32))
        }
    }
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Живые частицы по порядку рождения (самые старые первыми).
    pub fn particles(&self) -> &[Particle] {
        &self.rt.particles
    }

    pub fn alive_count(&self) -> usize {
        self.rt.particles.len()
    }

    /// Идёт ли ещё эмиссия (не зацикленная система после `duration` — нет).
    pub fn is_emitting(&self) -> bool {
        self.playing && !self.rt.done
    }

    /// Эмиссия закончена и частиц не осталось.
    pub fn is_finished(&self) -> bool {
        !self.looping && self.rt.done && self.rt.particles.is_empty()
    }

    /// Выпустить `count` частиц сразу (в ближайшем обновлении), независимо от `playing`.
    pub fn burst(&mut self, count: u32) {
        self.rt.pending_burst += count;
    }

    /// Начать сначала: убрать частицы, обнулить цикл, включить эмиссию.
    pub fn restart(&mut self) {
        self.rt = ParticleRuntime::default();
        self.playing = true;
    }

    /// Прекратить эмиссию; живые частицы доживают.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Убрать все живые частицы.
    pub fn clear(&mut self) {
        self.rt.particles.clear();
    }

    fn rng(&mut self) -> &mut Rng {
        let seed = self.seed;
        self.rt.rng.get_or_insert_with(|| {
            Rng::new(if seed != 0 {
                seed as u32 as u64
            } else {
                SEED_COUNTER.fetch_add(1, Ordering::Relaxed) ^ 0xABCD_1234
            })
        })
    }

    /// Шаг симуляции на `dt` секунд. `emitter` — мировая матрица объекта. Возвращает
    /// `true`, если объект пора удалить (`destroy_when_done`).
    pub fn simulate(&mut self, dt: f32, emitter: Mat4) -> bool {
        // Длинный кадр (пауза отладчика, загрузка) не должен взорвать симуляцию.
        let dt = dt.clamp(0.0, 0.1);
        let (_, rotation, _) = emitter.to_scale_rotation_translation();

        // 1. Движение живых частиц.
        let drag = 1.0 / (1.0 + self.drag.max(0.0) * dt);
        let gravity = match self.space {
            SimulationSpace::World => self.gravity,
            SimulationSpace::Local => rotation.inverse() * self.gravity,
        };
        self.rt.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.lifetime {
                return false;
            }
            p.velocity = (p.velocity + gravity * dt) * drag;
            p.position += p.velocity * dt;
            p.rotation += p.rotation_speed * dt;
            true
        });

        // 2. Сколько родится.
        let duration = self.duration.max(0.01);
        let mut burst_count = std::mem::take(&mut self.rt.pending_burst);
        let mut rate_dt = 0.0;
        if self.playing && !self.rt.done {
            if self.rt.burst_done.len() != self.bursts.len() {
                self.rt.burst_done = vec![false; self.bursts.len()];
            }
            let mut t1 = self.rt.time + dt;
            rate_dt = dt;
            if t1 >= duration {
                burst_count += due_bursts(&self.bursts, &mut self.rt.burst_done, duration);
                if self.looping {
                    t1 %= duration;
                    self.rt.burst_done.fill(false);
                } else {
                    rate_dt = (duration - self.rt.time).max(0.0);
                    t1 = duration;
                    self.rt.done = true;
                }
            }
            burst_count += due_bursts(&self.bursts, &mut self.rt.burst_done, t1);
            self.rt.time = t1;
        }
        self.rt.rate_accumulator += self.emission_rate.max(0.0) * rate_dt;
        let rate_count = self.rt.rate_accumulator.floor();
        self.rt.rate_accumulator -= rate_count;
        let rate_count = rate_count as u32;

        // 3. Рождение.
        let room = (self.max_particles.max(0) as usize).saturating_sub(self.rt.particles.len());
        let total = (burst_count as usize + rate_count as usize).min(room);
        self.rt.particles.reserve(total);
        for k in 0..total {
            // Частицы кадра рождаются «в разное время» кадра — поток без ступенек.
            let lead = if k < burst_count as usize {
                0.0
            } else {
                dt * (1.0 - (k - burst_count as usize) as f32 / rate_count.max(1) as f32)
            };
            self.spawn_one(emitter, rotation, lead);
        }

        self.destroy_when_done && !self.looping && self.rt.done && self.rt.particles.is_empty()
    }

    fn spawn_one(&mut self, emitter: Mat4, rotation: Quat, lead: f32) {
        let flat = self.flat;
        let shape = self.shape;
        let size = self.shape_size;
        let angle = self.cone_angle.clamp(0.0, 180.0).to_radians();
        let speed_range = self.start_speed;
        let lifetime_range = self.lifetime;
        let size_range = self.start_size;
        let rotation_range = self.start_rotation;
        let spin_range = self.rotation_speed;
        let (color_a, color_b) = (self.start_color, self.start_color_max);
        let sheet = (self.sheet_columns.max(1) * self.sheet_rows.max(1)).max(1);
        let local_space = self.space == SimulationSpace::Local;
        let rng = self.rng();

        // Направление в конусе вокруг +Y.
        let cone = |rng: &mut Rng| -> Vec3 {
            if flat {
                let theta = (rng.f32() * 2.0 - 1.0) * angle.min(std::f32::consts::PI);
                Vec3::new(theta.sin(), theta.cos(), 0.0)
            } else {
                let cos_phi = 1.0 + (angle.cos() - 1.0) * rng.f32();
                let sin_phi = (1.0 - cos_phi * cos_phi).max(0.0).sqrt();
                let psi = rng.f32() * std::f32::consts::TAU;
                Vec3::new(sin_phi * psi.cos(), cos_phi, sin_phi * psi.sin())
            }
        };
        let disc = |rng: &mut Rng, radius: f32| -> Vec3 {
            let r = radius * rng.f32().sqrt();
            let a = rng.f32() * std::f32::consts::TAU;
            Vec3::new(r * a.cos(), r * a.sin(), 0.0)
        };

        let (position, direction) = match shape {
            EmitterShape::Point => (Vec3::ZERO, cone(rng)),
            EmitterShape::Circle => {
                let p = disc(rng, size.x);
                (p, p.try_normalize().unwrap_or_else(|| cone(rng)))
            }
            EmitterShape::Sphere => {
                if flat {
                    let p = disc(rng, size.x);
                    (p, p.try_normalize().unwrap_or_else(|| cone(rng)))
                } else {
                    let cos_t = rng.f32() * 2.0 - 1.0;
                    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
                    let a = rng.f32() * std::f32::consts::TAU;
                    let unit = Vec3::new(sin_t * a.cos(), cos_t, sin_t * a.sin());
                    (unit * size.x * rng.f32().cbrt(), unit)
                }
            }
            EmitterShape::Cone => {
                let p = if flat {
                    Vec3::new((rng.f32() * 2.0 - 1.0) * size.x, 0.0, 0.0)
                } else {
                    let p = disc(rng, size.x);
                    Vec3::new(p.x, 0.0, p.y)
                };
                (p, cone(rng))
            }
            EmitterShape::Box => {
                let p = Vec3::new(
                    (rng.f32() - 0.5) * size.x,
                    (rng.f32() - 0.5) * size.y,
                    if flat { 0.0 } else { (rng.f32() - 0.5) * size.z },
                );
                (p, cone(rng))
            }
        };

        let speed = rng.range(speed_range);
        let (mut position, mut velocity) = if local_space {
            (position, direction * speed)
        } else {
            (emitter.transform_point3(position), rotation * direction * speed)
        };
        position += velocity * lead;
        if flat {
            position.z = if local_space { position.z } else { emitter.w_axis.z };
            velocity.z = 0.0;
        }
        let mix = rng.f32();
        let particle = Particle {
            position,
            velocity,
            age: lead,
            lifetime: rng.range(lifetime_range).max(0.01),
            size: rng.range(size_range).max(0.0),
            rotation: rng.range(rotation_range).to_radians(),
            rotation_speed: rng.range(spin_range).to_radians(),
            color: lerp_color(color_a, color_b, mix),
            frame: (rng.f32() * sheet as f32) as u32 % sheet,
        };
        self.rt.particles.push(particle);
    }

    // -----------------------------------------------------------------------
    // Отрисовка
    // -----------------------------------------------------------------------

    /// Дописать в `out` по шесть вершин на частицу. `matrix` — мировая матрица
    /// эмиттера (нужна для `Local`).
    pub(crate) fn write_vertices(&self, matrix: Mat4, basis: CameraBasis, inset: UvInset, out: &mut Vec<SpriteVertex>) {
        let local = self.space == SimulationSpace::Local;
        let (_, rotation, _) = matrix.to_scale_rotation_translation();
        let sheet = self.sheet_columns.max(1) * self.sheet_rows.max(1);
        let sheeted = sheet > 1;
        out.reserve(self.rt.particles.len() * 6);
        for p in &self.rt.particles {
            let t = (p.age / p.lifetime).clamp(0.0, 1.0);
            let mut color = p.color;
            if let Some(m) = sample_keys(&self.color_over_lifetime, t, lerp_color) {
                color = Color::rgba(color.r * m.r, color.g * m.g, color.b * m.b, color.a * m.a);
            }
            if color.a <= 0.0 {
                continue;
            }
            let mut size = p.size;
            if let Some(m) = sample_keys(&self.size_over_lifetime, t, |a, b, t| a + (b - a) * t) {
                size *= m;
            }
            if size <= 0.0 {
                continue;
            }
            let (center, velocity) = if local {
                (matrix.transform_point3(p.position), rotation * p.velocity)
            } else {
                (p.position, p.velocity)
            };

            // Оси квада на экране: поворот или вытягивание вдоль скорости.
            let (axis_x, axis_y) = {
                let along = Vec2::new(velocity.dot(basis.right), velocity.dot(basis.up));
                let speed = along.length();
                if self.velocity_stretch > 0.0 && speed > 1e-3 {
                    let d = along / speed;
                    let length = size + speed * self.velocity_stretch;
                    let forward = basis.right * d.x + basis.up * d.y;
                    let side = basis.right * -d.y + basis.up * d.x;
                    (forward * (length * 0.5), side * (size * 0.5))
                } else {
                    let (sin, cos) = p.rotation.sin_cos();
                    (
                        (basis.right * cos + basis.up * sin) * (size * 0.5),
                        (basis.up * cos - basis.right * sin) * (size * 0.5),
                    )
                }
            };
            let uv = if sheeted {
                let index = if self.sheet_over_lifetime {
                    ((t * sheet as f32) as u32).min(sheet - 1)
                } else {
                    p.frame
                };
                UvRect::FULL.cell(self.sheet_columns, self.sheet_rows, index, inset)
            } else {
                UvRect::FULL
            };
            push_quad(
                out,
                [
                    center - axis_x - axis_y,
                    center + axis_x - axis_y,
                    center + axis_x + axis_y,
                    center - axis_x + axis_y,
                ],
                uv,
                color.to_array(),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Готовые эффекты
// ---------------------------------------------------------------------------

impl ParticleSystem {
    /// Перевести эффект в 3D: частицы летят во все стороны, а не в плоскости XY.
    pub fn in_3d(mut self) -> Self {
        self.flat = false;
        self
    }

    pub fn with_texture(mut self, texture: AssetHandle<Texture>) -> Self {
        self.texture = texture;
        self
    }

    pub fn with_seed(mut self, seed: i32) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_layer(mut self, sorting_layer: i32, order: i32) -> Self {
        self.sorting_layer = sorting_layer;
        self.order = order;
        self
    }

    /// Масштаб эффекта: размеры, скорости и гравитация.
    pub fn scaled(mut self, factor: f32) -> Self {
        self.start_size *= factor;
        self.start_speed *= factor;
        self.gravity *= factor;
        self.shape_size *= factor;
        self
    }

    /// Взрыв: яркая вспышка и разлетающиеся искры, один раз; объект удаляется сам.
    pub fn explosion() -> Self {
        Self {
            looping: false,
            duration: 0.1,
            destroy_when_done: true,
            emission_rate: 0.0,
            bursts: vec![Vec2::new(0.0, 48.0)],
            lifetime: Vec2::new(0.35, 0.8),
            start_speed: Vec2::new(2.0, 7.0),
            start_size: Vec2::new(0.18, 0.5),
            start_color: Color::rgb(2.2, 1.0, 0.25),
            start_color_max: Color::rgb(2.4, 0.45, 0.1),
            drag: 2.5,
            color_over_lifetime: vec![
                Color::WHITE,
                Color::rgba(1.0, 0.5, 0.3, 0.8),
                Color::rgba(0.3, 0.1, 0.1, 0.0),
            ],
            size_over_lifetime: vec![1.0, 1.4, 0.4],
            shape: EmitterShape::Circle,
            shape_size: Vec3::new(0.15, 0.15, 0.15),
            flat: true,
            blend: ParticleBlend::Additive,
            order: 10,
            ..Self::default()
        }
    }

    /// Фонтан искр-штрихов с гравитацией; зацикленный (для сварки, удара о металл).
    pub fn sparks() -> Self {
        Self {
            emission_rate: 60.0,
            lifetime: Vec2::new(0.3, 0.7),
            start_speed: Vec2::new(3.0, 6.0),
            start_size: Vec2::new(0.04, 0.07),
            start_color: Color::rgb(4.0, 2.4, 0.8),
            start_color_max: Color::rgb(4.0, 1.2, 0.3),
            gravity: Vec3::new(0.0, -9.0, 0.0),
            color_over_lifetime: vec![Color::WHITE, Color::WHITE, Color::WHITE.with_alpha(0.0)],
            shape: EmitterShape::Cone,
            shape_size: Vec3::splat(0.02),
            cone_angle: 35.0,
            flat: true,
            blend: ParticleBlend::Additive,
            velocity_stretch: 0.06,
            order: 10,
            ..Self::default()
        }
    }

    /// Поднимающийся дым: крупные тусклые частицы растут и тают.
    pub fn smoke() -> Self {
        Self {
            emission_rate: 14.0,
            lifetime: Vec2::new(1.6, 2.6),
            start_speed: Vec2::new(0.3, 0.7),
            start_size: Vec2::new(0.35, 0.6),
            start_rotation: Vec2::new(0.0, 360.0),
            rotation_speed: Vec2::new(-25.0, 25.0),
            start_color: Color::rgba(0.55, 0.55, 0.58, 0.55),
            start_color_max: Color::rgba(0.3, 0.3, 0.33, 0.55),
            gravity: Vec3::new(0.0, 0.35, 0.0),
            drag: 0.6,
            color_over_lifetime: vec![
                Color::WHITE.with_alpha(0.0),
                Color::WHITE,
                Color::WHITE.with_alpha(0.0),
            ],
            size_over_lifetime: vec![0.6, 1.8],
            shape: EmitterShape::Cone,
            shape_size: Vec3::splat(0.1),
            cone_angle: 18.0,
            flat: true,
            order: 5,
            ..Self::default()
        }
    }

    /// Пыль: короткий выброс в стороны — для приземления и шагов; удаляется сам.
    pub fn dust() -> Self {
        Self {
            looping: false,
            duration: 0.1,
            destroy_when_done: true,
            emission_rate: 0.0,
            bursts: vec![Vec2::new(0.0, 8.0)],
            lifetime: Vec2::new(0.3, 0.55),
            start_speed: Vec2::new(0.6, 1.6),
            start_size: Vec2::new(0.14, 0.26),
            start_color: Color::rgba(0.78, 0.7, 0.58, 0.6),
            start_color_max: Color::rgba(0.62, 0.55, 0.46, 0.6),
            gravity: Vec3::new(0.0, 0.4, 0.0),
            drag: 3.0,
            color_over_lifetime: vec![Color::WHITE, Color::WHITE.with_alpha(0.0)],
            size_over_lifetime: vec![0.7, 1.5],
            shape: EmitterShape::Box,
            shape_size: Vec3::new(0.5, 0.05, 0.0),
            cone_angle: 80.0,
            flat: true,
            order: 4,
            ..Self::default()
        }
    }

    /// Блеск подбираемого предмета: звёздочки, разлетающиеся и тающие; удаляется сам.
    pub fn pickup_sparkle() -> Self {
        Self {
            looping: false,
            duration: 0.1,
            destroy_when_done: true,
            emission_rate: 0.0,
            bursts: vec![Vec2::new(0.0, 14.0)],
            lifetime: Vec2::new(0.4, 0.8),
            start_speed: Vec2::new(0.8, 2.4),
            start_size: Vec2::new(0.08, 0.16),
            start_color: Color::rgb(3.0, 2.6, 0.9),
            start_color_max: Color::rgb(2.4, 2.8, 3.0),
            gravity: Vec3::new(0.0, 1.2, 0.0),
            drag: 2.0,
            color_over_lifetime: vec![Color::WHITE, Color::WHITE, Color::WHITE.with_alpha(0.0)],
            size_over_lifetime: vec![0.4, 1.0, 0.0],
            shape: EmitterShape::Circle,
            shape_size: Vec3::splat(0.2),
            flat: true,
            blend: ParticleBlend::Additive,
            order: 11,
            ..Self::default()
        }
    }

    /// Пламя: яркое ядро, оранжевые языки вверх; зацикленное.
    pub fn fire() -> Self {
        Self {
            emission_rate: 45.0,
            lifetime: Vec2::new(0.4, 0.8),
            start_speed: Vec2::new(0.6, 1.4),
            start_size: Vec2::new(0.25, 0.4),
            start_color: Color::rgb(1.5, 0.6, 0.12),
            start_color_max: Color::rgb(1.7, 0.3, 0.06),
            gravity: Vec3::new(0.0, 1.5, 0.0),
            color_over_lifetime: vec![
                Color::WHITE.with_alpha(0.0),
                Color::rgba(1.0, 1.0, 1.0, 0.8),
                Color::rgba(0.4, 0.1, 0.05, 0.0),
            ],
            size_over_lifetime: vec![0.5, 1.0, 0.2],
            shape: EmitterShape::Cone,
            shape_size: Vec3::splat(0.12),
            cone_angle: 15.0,
            flat: true,
            blend: ParticleBlend::Additive,
            order: 8,
            ..Self::default()
        }
    }
}
