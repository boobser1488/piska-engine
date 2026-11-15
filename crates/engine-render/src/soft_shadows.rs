//! Каскадные мягкие тени солнца: необязательный компонент [`SoftShadows`] на любом
//! объекте сцены (Unity: URP Asset → Shadows → Cascade Count, Soft Shadows).
//!
//! Без компонента солнце рисует тени как раньше: одна карта 2048² на область вокруг
//! камеры, PCF 3×3. С компонентом область теней делится по глубине вида на каскады
//! (1–4): у ближнего — маленький участок и чёткая тень, у дальних — большие участки.
//! Каждый каскад — шар вокруг своего куска пирамиды камеры: при повороте камеры размер
//! карты не меняется, а центр привязан к текселям — края теней не дрожат. Между каскадами
//! тень смешивается, после последнего — плавно гаснет.
//!
//! Пресет качества (Project Settings → Graphics) ограничивает число каскадов и фильтр:
//! Low — без теней, Medium — 2 каскада, PCF 3×3, High — 3 каскада, диск Пуассона,
//! Ultra — 4 каскада и «контактное упрочнение» (тень у основания чёткая, дальше от
//! заслонителя — шире полутень).

use engine_core::prelude::*;

// Фильтр края тени: `Hard` — PCF 3×3, как у теней без компонента; `Soft` — 16 выборок
// по диску Пуассона, ровный мягкий край шириной `softness` текселей; `ContactHardening` —
// мягкий край, ширина которого растёт с расстоянием до заслонителя (PCSS-подобно).
reflect_enum! {
    pub enum ShadowFilter { Hard, Soft, ContactHardening }
}

component! {
    /// Каскадные мягкие тени солнца. Дальность — `Environment::shadow_distance` (и потолок
    /// пресета `GraphicsSettings::shadow_distance`).
    pub struct SoftShadows {
        /// Включено. `false` — тени как без компонента.
        pub enabled: bool = true,
        /// Сколько каскадов (1–4). Пресет может уменьшить.
        pub cascades: i32 = 3,
        /// Фильтр края; пресет может упростить.
        pub filter: ShadowFilter = ShadowFilter::ContactHardening,
        /// Ширина мягкого края в текселях карты (1…6).
        pub softness: f32 = 2.0,
        /// Доля каскада у его дальней границы, где тень смешивается со следующим (0…0.5).
        pub blend: f32 = 0.15,
        /// Распределение границ каскадов: 0 — равные доли, 1 — логарифмически (ближние
        /// мельче). Обычно 0.6–0.85.
        pub split: f32 = 0.75,
    }
}

impl Component for SoftShadows {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Сколько каскадов допускает рендер.
pub const MAX_CASCADES: usize = 4;

/// Что получилось из компонента и пресета для кадра.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CascadeSetup {
    pub count: usize,
    /// 0 — PCF 3×3, 1 — диск Пуассона, 2 — контактное упрочнение.
    pub filter: u32,
    pub softness: f32,
    pub blend: f32,
    pub split: f32,
}

impl SoftShadows {
    /// Первый активный включённый компонент сцены с ограничениями пресета: `None` — тени
    /// как без компонента (одна карта).
    pub(crate) fn setup(scene: &Scene, max_cascades: u32, max_filter: u32) -> Option<CascadeSetup> {
        let mut found = None;
        if scene.count_with::<SoftShadows>() > 0 {
            scene.visit_active_with::<SoftShadows>(|_, s| {
                if found.is_none() && s.enabled {
                    found = Some(s.clone());
                }
            });
        }
        let s = found?;
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let count = (s.cascades.clamp(1, MAX_CASCADES as i32) as u32).min(max_cascades.max(1)) as usize;
        let filter = match s.filter {
            ShadowFilter::Hard => 0,
            ShadowFilter::Soft => 1,
            ShadowFilter::ContactHardening => 2,
        }
        .min(max_filter);
        Some(CascadeSetup {
            count,
            filter,
            softness: finite(s.softness, 2.0).clamp(0.5, 8.0),
            blend: finite(s.blend, 0.15).clamp(0.0, 0.5),
            split: finite(s.split, 0.75).clamp(0.0, 1.0),
        })
    }
}

/// Дальние границы каскадов (глубина вида) на отрезке `near..far`: смесь равномерного и
/// логарифмического деления с весом `lambda` (1 — логарифмическое).
pub(crate) fn cascade_splits(near: f32, far: f32, count: usize, lambda: f32) -> [f32; MAX_CASCADES] {
    let near = near.max(0.01);
    let far = far.max(near * 1.01);
    let mut out = [far; MAX_CASCADES];
    for (i, slot) in out.iter_mut().enumerate().take(count) {
        let t = (i + 1) as f32 / count as f32;
        let log = near * (far / near).powf(t);
        let uniform = near + (far - near) * t;
        *slot = lambda * log + (1.0 - lambda) * uniform;
    }
    out[count - 1] = far;
    out
}

/// Шар, накрывающий кусок пирамиды камеры между глубинами `a` и `b`: центр на оси вида,
/// `k` — тангенс угла между осью и лучом через угол кадра. Возвращает (глубина центра,
/// радиус). Радиус зависит только от `a`, `b`, `k` — при повороте камеры не меняется.
pub(crate) fn slice_sphere(a: f32, b: f32, k: f32) -> (f32, f32) {
    let k2 = k * k;
    let center = ((a + b) * 0.5 * (1.0 + k2)).min(b);
    let radius = ((b - center).powi(2) + b * b * k2).sqrt().max(((center - a).powi(2) + a * a * k2).sqrt());
    (center, radius)
}

/// Каскад кадра: матрица «мир → карта теней» и данные для шейдера (см. `cascade_data`
/// в common.wgsl).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cascade {
    pub view_proj: Mat4,
    pub data: [f32; 4],
}

/// Каскады для перспективной камеры `view_proj` (обратная глубина), стоящей в `eye`.
/// `None` — камера ортографическая или вырожденная: тогда тени — одной картой.
pub(crate) fn fit_cascades(
    view_proj: Mat4,
    eye: Vec3,
    sun_direction: Vec3,
    setup: &CascadeSetup,
    distance: f32,
    casters: (Vec3, Vec3),
    size: u32,
) -> Option<Vec<Cascade>> {
    let ortho = view_proj.x_axis.w == 0.0 && view_proj.y_axis.w == 0.0 && view_proj.z_axis.w == 0.0;
    let inverse = view_proj.inverse();
    if ortho || !inverse.is_finite() {
        return None;
    }
    let point = |x: f32, y: f32, z: f32| {
        let p = inverse * Vec4::new(x, y, z, 1.0);
        p.truncate() / p.w
    };
    // Обратная глубина: z = 1 — ближняя плоскость, z = 0 — дальняя.
    let forward = (point(0.0, 0.0, 0.0) - point(0.0, 0.0, 1.0)).normalize_or_zero();
    if forward == Vec3::ZERO {
        return None;
    }
    let near = (point(0.0, 0.0, 1.0) - eye).dot(forward).max(0.05);
    let far_plane = (point(0.0, 0.0, 0.0) - eye).dot(forward);
    // Тангенс угла между осью вида и лучом через угол кадра.
    let corner = point(1.0, 1.0, 0.0) - eye;
    let along = corner.dot(forward).max(1e-4);
    let k = (corner - forward * along).length() / along;
    let far = distance.min(far_plane).max(near * 1.5);
    if !(k.is_finite() && far.is_finite()) {
        return None;
    }
    let splits = cascade_splits(near, far, setup.count, setup.split);
    let up = if sun_direction.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let rotation = Mat4::look_to_rh(Vec3::ZERO, sun_direction, up);
    // Отбрасывающие тень объекты: их расстояния вдоль лучей солнца.
    let mut caster_near = f32::INFINITY;
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { casters.0.x } else { casters.1.x },
            if i & 2 == 0 { casters.0.y } else { casters.1.y },
            if i & 4 == 0 { casters.0.z } else { casters.1.z },
        );
        caster_near = caster_near.min(-rotation.transform_point3(c).z);
    }
    let mut out = Vec::with_capacity(setup.count);
    for i in 0..setup.count {
        // Начало каскада — с запасом на полосу перехода из предыдущего.
        let a = if i == 0 { near } else { splits[i - 1] * (1.0 - setup.blend) };
        let b = splits[i];
        let (center_depth, radius) = slice_sphere(a, b, k);
        let center = eye + forward * center_depth;
        let texel = radius * 2.0 / size as f32;
        let mut in_light = rotation.transform_point3(center);
        in_light.x = (in_light.x / texel).round() * texel;
        in_light.y = (in_light.y / texel).round() * texel;
        let center_distance = -in_light.z;
        let z_near = (center_distance - radius).min(caster_near) - 1.0;
        let z_far = center_distance + radius + 1.0;
        let projection = Mat4::orthographic_rh(
            in_light.x - radius,
            in_light.x + radius,
            in_light.y - radius,
            in_light.y + radius,
            z_near,
            z_far,
        );
        let range = (z_far - z_near).max(1e-3);
        // Смещение глубины ≈ полтекселя в метрах (плюс 2 см), в долях глубины карты.
        let bias = (texel * 0.5 + 0.02) / range;
        out.push(Cascade { view_proj: projection * rotation, data: [b, texel * 1.6 + 0.01, bias, range] });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_grow_and_end_at_the_far_distance() {
        let s = cascade_splits(0.5, 100.0, 4, 0.75);
        assert!(s[0] < s[1] && s[1] < s[2] && s[2] < s[3]);
        assert_eq!(s[3], 100.0);
        assert!(s[0] < 10.0, "the first cascade is small: {s:?}");
        let even = cascade_splits(0.0, 90.0, 3, 0.0);
        assert!((even[0] - 30.0).abs() < 0.5 && (even[1] - 60.0).abs() < 0.5, "{even:?}");
    }

    #[test]
    fn slice_sphere_contains_all_corners() {
        for &(a, b, k) in &[(0.5, 8.0, 0.9), (8.0, 30.0, 0.9), (30.0, 100.0, 1.4), (1.0, 2.0, 0.2)] {
            let (c, r) = slice_sphere(a, b, k);
            for (depth, lateral) in [(a, a * k), (b, b * k)] {
                let d = ((depth - c) * (depth - c) + lateral * lateral).sqrt();
                assert!(d <= r + 1e-3, "corner at {depth} outside: {d} > {r}");
            }
        }
    }

    #[test]
    fn preset_caps_cascades_and_filter() {
        let mut scene = Scene::default();
        let id = scene.build("Shadows").with(SoftShadows { cascades: 4, ..Default::default() }).id();
        let full = SoftShadows::setup(&scene, 4, 2).expect("enabled");
        assert_eq!((full.count, full.filter), (4, 2));
        let medium = SoftShadows::setup(&scene, 2, 0).expect("enabled");
        assert_eq!((medium.count, medium.filter), (2, 0));
        scene.get_component_mut::<SoftShadows>(id).unwrap().enabled = false;
        assert!(SoftShadows::setup(&scene, 4, 2).is_none());
    }
}
