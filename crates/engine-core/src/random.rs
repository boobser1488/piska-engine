//! Детерминированный генератор случайных чисел (xoshiro256++), без внешних зависимостей.
//!
//! Одно и то же зерно даёт одну и ту же последовательность на любой платформе —
//! на этом стоят воспроизводимые тесты, «сид дня» и повторы. Генератор лежит в
//! ресурсах мира: `ctx.random().range(0.0, 1.0)`.

use std::collections::hash_map::RandomState;
use std::f32::consts::TAU;
use std::hash::{BuildHasher, Hasher};

use crate::math::{Color, Vec2, Vec3};

/// Зерно, с которого стартует ресурс по умолчанию: игра без явного `seed` каждый раз
/// проходит одинаково (удобно в тестах). Для «настоящей» случайности: [`Random::from_entropy`].
pub const DEFAULT_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// xoshiro256++ с инициализацией через splitmix64.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Random {
    state: [u64; 4],
    /// Запасное значение гауссова генератора (метод Бокса-Мюллера даёт пару).
    spare_gaussian: Option<u32>,
}

impl Default for Random {
    fn default() -> Self {
        Self::new(DEFAULT_SEED)
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let mut random = Self {
            state: [0; 4],
            spare_gaussian: None,
        };
        random.seed(seed);
        random
    }

    /// Зерно из операционной системы (на каждый запуск своё). Работает и в вебе.
    pub fn from_entropy() -> Self {
        Self::new(RandomState::new().build_hasher().finish())
    }

    /// Перезапустить последовательность с этого зерна.
    pub fn seed(&mut self, seed: u64) {
        let mut sm = seed;
        for slot in &mut self.state {
            *slot = splitmix64(&mut sm);
        }
        self.spare_gaussian = None;
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Равномерно в `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Равномерно в `[0, 1)`.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Unity: `Random.Range(min, max)` для чисел с плавающей точкой — `[min, max)`.
    /// Границы можно перепутать местами.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.f32()
    }

    /// Unity: `Random.Range(min, max)` для целых — `max` **не** входит. При `min == max`
    /// вернёт `min`.
    pub fn int_range(&mut self, min: i32, max: i32) -> i32 {
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        let span = (hi as i64 - lo as i64) as u64;
        if span == 0 {
            return lo;
        }
        let offset = ((self.next_u64() as u128 * span as u128) >> 64) as i64;
        (lo as i64 + offset) as i32
    }

    /// Целое из `[min, max]`, обе границы входят.
    pub fn int_range_inclusive(&mut self, min: i32, max: i32) -> i32 {
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        self.int_range(lo, hi.saturating_add(1)).max(lo)
    }

    /// Выпадет ли событие с вероятностью `probability` (0..=1).
    pub fn chance(&mut self, probability: f32) -> bool {
        self.f32() < probability
    }

    pub fn coin_flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// −1 или +1.
    pub fn sign(&mut self) -> f32 {
        if self.coin_flip() {
            1.0
        } else {
            -1.0
        }
    }

    /// Случайный элемент среза; `None` для пустого.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let index = self.int_range(0, items.len() as i32) as usize;
            items.get(index)
        }
    }

    /// Индекс по весам: вероятность пропорциональна весу. `None`, если все веса нулевые.
    pub fn weighted_index(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut roll = self.f32() * total;
        for (index, weight) in weights.iter().enumerate() {
            let weight = weight.max(0.0);
            if roll < weight {
                return Some(index);
            }
            roll -= weight;
        }
        // Погрешность округления: последний ненулевой вес.
        weights.iter().rposition(|w| *w > 0.0)
    }

    /// Перетасовать на месте (Фишер — Йетс).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.int_range(0, i as i32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Unity: `Random.insideUnitCircle`.
    pub fn in_unit_circle(&mut self) -> Vec2 {
        let radius = self.f32().sqrt();
        let angle = self.f32() * TAU;
        Vec2::new(angle.cos(), angle.sin()) * radius
    }

    /// Точка на единичной окружности.
    pub fn on_unit_circle(&mut self) -> Vec2 {
        let angle = self.f32() * TAU;
        Vec2::new(angle.cos(), angle.sin())
    }

    /// Unity: `Random.onUnitSphere` — равномерно по поверхности.
    pub fn on_unit_sphere(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let angle = self.f32() * TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * angle.cos(), r * angle.sin(), z)
    }

    /// Unity: `Random.insideUnitSphere` — равномерно по объёму.
    pub fn in_unit_sphere(&mut self) -> Vec3 {
        self.on_unit_sphere() * self.f32().cbrt()
    }

    /// Нормальное распределение (метод Бокса — Мюллера).
    pub fn gaussian(&mut self, mean: f32, std_dev: f32) -> f32 {
        if let Some(bits) = self.spare_gaussian.take() {
            return mean + std_dev * f32::from_bits(bits);
        }
        let u1 = self.f32().max(f32::MIN_POSITIVE);
        let u2 = self.f32();
        let radius = (-2.0 * u1.ln()).sqrt();
        let angle = TAU * u2;
        self.spare_gaussian = Some((radius * angle.sin()).to_bits());
        mean + std_dev * radius * angle.cos()
    }

    /// Случайный насыщенный цвет: оттенок любой, `s` и `v` заданы.
    pub fn color_hsv(&mut self, saturation: f32, value: f32) -> Color {
        Color::from_hsv(self.f32(), saturation, value)
    }

    /// Случайный цвет с независимыми каналами (непрозрачный).
    pub fn color(&mut self) -> Color {
        Color::rgb(self.f32(), self.f32(), self.f32())
    }
}
