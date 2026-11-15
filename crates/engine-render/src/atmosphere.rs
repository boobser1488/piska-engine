//! Небо и время суток: необязательные компоненты [`Sky`] и [`TimeOfDay`].
//!
//! Без `Sky` небо рисуется как раньше: градиент по высоте солнца и маленький диск.
//! С ним — диск солнца заданного размера с ореолом, простое аналитическое рассеяние
//! (днём — голубой зенит и светлый горизонт, на закате — тёплая полоса на стороне солнца,
//! ночью — тёмно-синее небо), стилизованные облака (два слоя шума, плывут по ветру), луна
//! напротив солнца и звёзды. Рассеяние рисуется только на фоне неба (туман и ambient
//! его не получают) и следует за яркостью солнца.
//!
//! `TimeOfDay` на объекте с направленным светом вращает его по часам (восход — закат —
//! ночь) и меняет цвет и яркость: тёплый низкий свет утром и вечером, слабый голубой
//! «лунный» ночью. Пресеты: [`TimeOfDay::sunrise`], [`noon`](TimeOfDay::noon),
//! [`sunset`](TimeOfDay::sunset), [`night`](TimeOfDay::night).

use engine_core::prelude::*;

use crate::components::Light;

component! {
    /// Небо: солнце с ореолом, рассеяние, облака, луна и звёзды.
    pub struct Sky {
        /// Включено. `false` — небо как без компонента.
        pub enabled: bool = true,
        /// Угловой радиус диска солнца, градусы.
        pub sun_size: f32 = 0.7,
        /// Сила ореола вокруг солнца (0 — без него).
        pub sun_glow: f32 = 1.0,
        /// Сила рассеяния: светлый горизонт, тёплая полоса заката на стороне солнца.
        pub scattering: f32 = 1.0,
        /// Покрытие облаками: 0 — ясно, 1 — сплошь.
        pub clouds: f32 = 0.35,
        /// Скорость облаков, м/с на высоте слоя.
        pub cloud_speed: f32 = 6.0,
        /// Размер облаков: больше — крупнее.
        pub cloud_scale: f32 = 1.0,
        /// Цвет освещённых облаков.
        pub cloud_color: Color = Color::rgb(1.0, 0.98, 0.95),
        /// Яркость звёзд ночью (0 — без звёзд).
        pub stars: f32 = 1.0,
        /// Луна напротив солнца, когда оно под горизонтом.
        pub moon: bool = true,
    }
}

impl Component for Sky {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Параметры неба для шейдера (три `vec4` в `Globals`, см. common.wgsl).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SkyParams {
    /// x — включено, y — cos углового радиуса солнца, z — ореол, w — рассеяние.
    pub sun: [f32; 4],
    /// x — покрытие облаками, yz — сдвиг облаков (ветер × время), w — масштаб.
    pub clouds: [f32; 4],
    /// rgb — цвет облаков, w — звёзды.
    pub cloud_color: [f32; 4],
    /// x — луна, y — время (с) для мерцания звёзд, z — занято рендером (дешёвый рельеф
    /// на пресете Low), w — запас.
    pub night: [f32; 4],
}

impl Sky {
    /// Первый активный включённый `Sky` сцены → параметры шейдера (нули — выключено).
    pub(crate) fn shader_params(scene: &Scene, time: f64, clouds_allowed: bool) -> SkyParams {
        let mut found = None;
        if scene.count_with::<Sky>() > 0 {
            scene.visit_active_with::<Sky>(|_, s| {
                if found.is_none() && s.enabled {
                    found = Some(s.clone());
                }
            });
        }
        let Some(s) = found else {
            return SkyParams::default();
        };
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let size = finite(s.sun_size, 0.7).clamp(0.05, 10.0).to_radians();
        let scale = finite(s.cloud_scale, 1.0).clamp(0.05, 20.0);
        let speed = finite(s.cloud_speed, 6.0).clamp(-500.0, 500.0);
        // Ветер — вдоль +X и чуть вбок; сдвиг по модулю большого периода, чтобы f32 не терял точность.
        let travel = ((time * speed as f64) % 100_000.0) as f32;
        let coverage = if clouds_allowed { finite(s.clouds, 0.35).clamp(0.0, 1.0) } else { 0.0 };
        let c = s.cloud_color;
        SkyParams {
            sun: [1.0, size.cos(), finite(s.sun_glow, 1.0).clamp(0.0, 10.0), finite(s.scattering, 1.0).clamp(0.0, 4.0)],
            clouds: [coverage, travel, travel * 0.35, scale],
            cloud_color: [finite(c.r, 1.0).max(0.0), finite(c.g, 1.0).max(0.0), finite(c.b, 1.0).max(0.0), finite(s.stars, 1.0).clamp(0.0, 10.0)],
            night: [if s.moon { 1.0 } else { 0.0 }, (time % 3600.0) as f32, 0.0, 0.0],
        }
    }
}

component! {
    /// Время суток: вращает направленный свет этого объекта по часам и меняет его цвет
    /// и яркость. Положите на объект с `Light` (солнце).
    pub struct TimeOfDay {
        /// Час дня, 0…24 (6 — восход, 12 — полдень, 18 — закат).
        pub hour: f32 = 12.0,
        /// Скорость: игровых часов в секунду (0 — время стоит).
        pub speed: f32 = 0.0,
        /// Направление восхода по горизонту (градусы от +X к −Z).
        pub heading: f32 = 30.0,
        /// Наибольшая высота солнца в полдень, градусы.
        pub max_elevation: f32 = 60.0,
        /// Яркость солнца в полдень.
        pub intensity: f32 = 2.4,
        /// Яркость «луны» (свет ночью).
        pub night_intensity: f32 = 0.18,
    }
}

impl Component for TimeOfDay {
    fn hooks(&self) -> Hooks {
        Hooks::START.union(Hooks::UPDATE)
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        self.apply(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if self.speed != 0.0 && self.speed.is_finite() {
            self.hour = (self.hour + self.speed * dt).rem_euclid(24.0);
        }
        self.apply(ctx);
    }
}

impl TimeOfDay {
    /// Раннее утро: низкое тёплое солнце.
    pub fn sunrise() -> Self {
        Self { hour: 6.6, ..Self::default() }
    }

    /// Полдень.
    pub fn noon() -> Self {
        Self { hour: 12.0, ..Self::default() }
    }

    /// Закат.
    pub fn sunset() -> Self {
        Self { hour: 17.6, ..Self::default() }
    }

    /// Ночь: луна высоко, свет слабый и холодный.
    pub fn night() -> Self {
        Self { hour: 0.5, ..Self::default() }
    }

    /// Высота солнца (радианы, отрицательная — под горизонтом) и направление на солнце.
    pub fn sun(&self) -> (f32, Vec3) {
        let hour = if self.hour.is_finite() { self.hour.rem_euclid(24.0) } else { 12.0 };
        // Суточный круг: в 6 ч солнце на горизонте на востоке, в 12 — выше всего, в 18 — на западе.
        let angle = (hour - 6.0) / 12.0 * std::f32::consts::PI;
        let max = self.max_elevation.clamp(1.0, 90.0).to_radians();
        let elevation = angle.sin() * max;
        let azimuth = self.heading.to_radians() + angle;
        let horizontal = Vec3::new(azimuth.cos(), 0.0, -azimuth.sin());
        let to_sun = (horizontal * elevation.cos() + Vec3::Y * elevation.sin()).normalize_or(Vec3::Y);
        (elevation, to_sun)
    }

    /// Свет по часам: направление лучей (куда летит свет), цвет и яркость. Ночью свет
    /// идёт от «луны» — напротив солнца.
    pub fn light(&self) -> (Vec3, Color, f32) {
        let (elevation, to_sun) = self.sun();
        let day = smoothstep(-0.05, 0.12, elevation);
        let low = 1.0 - smoothstep(0.05, 0.5, elevation);
        let warm = Vec3::new(1.0, 0.62, 0.36);
        let white = Vec3::new(1.0, 0.95, 0.88);
        let moon = Vec3::new(0.55, 0.65, 1.0);
        let sun_color = white.lerp(warm, low);
        let color = moon.lerp(sun_color, day);
        let intensity = finite_or(self.night_intensity, 0.18).max(0.0) * (1.0 - day)
            + finite_or(self.intensity, 2.4).max(0.0) * day * (0.35 + 0.65 * smoothstep(0.0, 0.35, elevation));
        // Ночью светит луна напротив солнца (над горизонтом, когда солнце под ним).
        let source = if day > 0.0 || elevation >= 0.0 { to_sun } else { Vec3::new(-to_sun.x, -to_sun.y, -to_sun.z) };
        let source = if source.y < 0.05 { Vec3::new(source.x, 0.05, source.z).normalize() } else { source };
        (-source, Color::rgb(color.x, color.y, color.z), intensity)
    }

    fn apply(&self, ctx: &mut Ctx) {
        let (direction, color, intensity) = self.light();
        // Ось «вперёд» объекта (−Z) — направление лучей.
        let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, direction);
        ctx.transform().rotation = rotation;
        if let Some(light) = ctx.get_mut::<Light>() {
            light.color = color;
            light.intensity = intensity;
        }
    }
}

fn finite_or(v: f32, d: f32) -> f32 {
    if v.is_finite() { v } else { d }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_rises_peaks_and_sets() {
        let at = |hour: f32| TimeOfDay { hour, ..Default::default() }.sun().0;
        assert!(at(6.0).abs() < 1e-3);
        assert!((at(12.0) - 60f32.to_radians()).abs() < 1e-3);
        assert!(at(18.0).abs() < 1e-3);
        assert!(at(0.0) < -0.9);
    }

    #[test]
    fn light_is_warm_low_white_high_and_dim_blue_at_night() {
        let (dir, noon, i_noon) = TimeOfDay::noon().light();
        assert!(dir.y < -0.8, "noon light points down: {dir:?}");
        let (_, sunset, i_sunset) = TimeOfDay::sunset().light();
        let (dir_night, night, i_night) = TimeOfDay::night().light();
        assert!(i_noon > i_sunset && i_sunset > i_night, "{i_noon} {i_sunset} {i_night}");
        assert!(sunset.r / sunset.b > noon.r / noon.b, "sunset is warmer");
        assert!(night.b > night.r, "night is blue");
        assert!(dir_night.y < 0.0, "the moon is above the horizon");
    }

    #[test]
    fn sky_params_are_zero_without_the_component() {
        let scene = Scene::default();
        assert_eq!(Sky::shader_params(&scene, 1.0, true), SkyParams::default());
        let mut scene = Scene::default();
        scene.build("Sky").with(Sky { clouds: 0.5, ..Default::default() });
        let p = Sky::shader_params(&scene, 10.0, true);
        assert_eq!(p.sun[0], 1.0);
        assert_eq!(p.clouds[0], 0.5);
        assert_eq!(Sky::shader_params(&scene, 10.0, false).clouds[0], 0.0, "the preset can switch clouds off");
    }
}
