//! Свои цвета неба: необязательный компонент [`SkyColors`] на любом объекте сцены.
//!
//! Обычно цвета зенита и горизонта вычисляются по высоте солнца (день, закат, ночь). Для
//! постановочного света этого мало: лунная ночь с тенями от «луны» (направленного
//! источника высоко над горизонтом) получила бы дневное небо. `SkyColors` задаёт цвета
//! сам; от них зависят фон, рассеянный свет (ambient), отражения неба и цвет тумана.

use engine_core::prelude::*;

component! {
    /// Цвета неба вместо вычисленных по высоте солнца (линейные, можно ярче 1).
    pub struct SkyColors {
        /// Включено. `false` — небо как без компонента.
        pub enabled: bool = true,
        /// Цвет зенита.
        pub zenith: Color = Color::rgb(0.12, 0.30, 0.75),
        /// Цвет у горизонта (им же окрашен туман).
        pub horizon: Color = Color::rgb(0.55, 0.70, 0.90),
        /// Доля своих цветов: 1 — только они, 0.5 — пополам с вычисленными.
        pub blend: f32 = 1.0,
    }
}

impl Component for SkyColors {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl SkyColors {
    /// Цвета неба с учётом компонента: вычисленные `zenith` / `horizon`, смешанные со
    /// своими.
    pub fn mix(&self, zenith: Vec3, horizon: Vec3) -> (Vec3, Vec3) {
        if !self.enabled {
            return (zenith, horizon);
        }
        let blend = if self.blend.is_finite() { self.blend.clamp(0.0, 1.0) } else { 1.0 };
        let own = |c: Color, fallback: Vec3| {
            let v = Vec3::new(c.r, c.g, c.b);
            if v.is_finite() { v.max(Vec3::ZERO) } else { fallback }
        };
        (
            zenith.lerp(own(self.zenith, zenith), blend),
            horizon.lerp(own(self.horizon, horizon), blend),
        )
    }

    /// Применить первый активный `SkyColors` сцены (если он есть).
    pub(crate) fn apply(scene: &Scene, zenith: Vec3, horizon: Vec3) -> (Vec3, Vec3) {
        let mut result = (zenith, horizon);
        let mut done = false;
        if scene.count_with::<SkyColors>() > 0 {
            scene.visit_active_with::<SkyColors>(|_, sky| {
                if !done {
                    done = true;
                    result = sky.mix(zenith, horizon);
                }
            });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_mixes_and_disabled_keeps_computed() {
        let computed = (Vec3::ZERO, Vec3::ONE);
        let sky = SkyColors { zenith: Color::rgb(1.0, 0.0, 0.0), horizon: Color::rgb(0.0, 0.0, 1.0), blend: 0.5, ..Default::default() };
        let (z, h) = sky.mix(computed.0, computed.1);
        assert!((z - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-5);
        assert!((h - Vec3::new(0.5, 0.5, 1.0)).length() < 1e-5);
        assert_eq!(SkyColors { enabled: false, ..sky.clone() }.mix(computed.0, computed.1), computed);
        let broken = SkyColors { zenith: Color::rgb(f32::NAN, 0.0, 0.0), ..Default::default() };
        assert_eq!(broken.mix(computed.0, computed.1).0, computed.0);
    }
}
