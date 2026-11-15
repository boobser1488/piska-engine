//! Локальный свет сверх четырёх ламп: прожекторы, кластеры, тени ламп, заполняющий свет.
//!
//! Без этих компонентов меши и рельеф освещают солнце и до четырёх ближайших к камере
//! точечных источников (как раньше, шейдер того же варианта). Если в сцене есть
//! [`SpotLight`], [`LightShadows`], [`ManyLights`] или [`FillLight`], кадр рисуется
//! вариантом шейдера «с лампами» (renderer/variant.rs):
//!
//! * все лампы (точечные и прожекторы) раскладываются на CPU по кластерам — сетка
//!   16 × 9 по экрану × 16 слоёв по глубине (логарифмически); в каждой ячейке — номера
//!   до 16 ламп, чей шар задевает ячейку. Пиксель берёт свою ячейку по положению на
//!   экране и глубине и считает только её лампы. Данные — в текстурах (`textureLoad`):
//!   так работает и WebGL2, где нет storage-буферов и вычислительных шейдеров, а
//!   передавать индексы ламп из вершинного шейдера нельзя (у шейдера мешей 30 из 31
//!   компонентов интерполяции заняты);
//! * без [`ManyLights`] ламп по-прежнему не больше четырёх ближайших;
//! * тени ламп — атлас 2048² из плиток 512²: прожектор — одна плитка, точечная лампа —
//!   шесть (грани куба). Сколько ламп с тенью — пресет (`GraphicsSettings::light_shadows`).

use engine_core::prelude::*;

component! {
    /// Прожектор: точечный [`Light`](crate::Light) на этом же объекте светит конусом вдоль
    /// оси «вперёд» объекта (−Z). Дальность — `Light::range`.
    pub struct SpotLight {
        /// Полный угол конуса, градусы: за ним света нет.
        pub angle: f32 = 50.0,
        /// Полный угол яркой середины, градусы: от него к `angle` свет плавно гаснет.
        pub inner_angle: f32 = 34.0,
    }
}

impl Component for SpotLight {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Тени от точечного света или прожектора на этом объекте. Сколько ламп с тенью
    /// рисуется — пресет качества (`GraphicsSettings::light_shadows`): ближние к камере
    /// получают их первыми.
    pub struct LightShadows {
        /// Отбрасывает ли лампа тени.
        pub enabled: bool = true,
        /// Сдвиг против самозатенения, м (растёт с расстоянием до лампы).
        pub bias: f32 = 0.03,
    }
}

impl Component for LightShadows {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Много ламп: поверхность освещают все точечные лампы и прожекторы сцены (до
    /// `max_lights`, пресет может уменьшить), а не четыре ближайших к камере.
    pub struct ManyLights {
        /// Включено. `false` — как без компонента (четыре ближайшие лампы).
        pub enabled: bool = true,
        /// Больше ламп в кадре не берётся (ближние к камере — первыми; не больше 255).
        pub max_lights: i32 = 128,
    }
}

impl Component for ManyLights {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

component! {
    /// Заполняющий свет: ambient полусферой — сверху цвет неба, снизу отражённый от земли
    /// тёплый свет (сильнее, когда солнце высоко). Сила общего ambient — по-прежнему
    /// `Environment::ambient`.
    pub struct FillLight {
        /// Включено. `false` — ambient как без компонента.
        pub enabled: bool = true,
        /// Множитель силы.
        pub intensity: f32 = 1.0,
        /// Насколько верх берёт цвет неба сцены (1) вместо `sky_color` (0).
        pub sky_tint: f32 = 1.0,
        /// Цвет верха, если `sky_tint` < 1.
        pub sky_color: Color = Color::rgb(0.6, 0.7, 0.85),
        /// Цвет отражённого от земли света (снизу).
        pub ground_color: Color = Color::rgb(0.62, 0.5, 0.36),
        /// Сила отражённого света снизу.
        pub bounce: f32 = 1.0,
    }
}

impl Component for FillLight {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Больше ламп в кадре не бывает (номер лампы в ячейке — байт, 255 — «конец»).
pub(crate) const MAX_LIGHTS: usize = 255;
/// Сетка кластеров: по экрану и слоёв по глубине.
pub(crate) const CLUSTER_X: u32 = 16;
pub(crate) const CLUSTER_Y: u32 = 9;
pub(crate) const CLUSTER_Z: u32 = 16;
/// Ламп в одной ячейке, не больше (четыре `u32` по четыре байта).
const PER_CELL: usize = 16;
/// Глубины слоёв: логарифмически от `NEAR` до `FAR` метров (дальше — последний слой).
const CLUSTER_NEAR: f32 = 0.5;
const CLUSTER_FAR: f32 = 400.0;
/// Атлас теней ламп и его плитки.
pub(crate) const ATLAS_SIZE: u32 = 2048;
pub(crate) const TILE_SIZE: u32 = 512;
pub(crate) const TILES: usize = ((ATLAS_SIZE / TILE_SIZE) * (ATLAS_SIZE / TILE_SIZE)) as usize;
/// Текселей на строку данных лампы и на грань тени.
pub(crate) const PARAM_TEXELS: u32 = 4;
pub(crate) const FACE_TEXELS: u32 = 5;

/// Лампа кадра (собирается при обходе сцены).
#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalLight {
    pub position: Vec3,
    pub range: f32,
    /// Цвет × яркость.
    pub color: Vec3,
    /// Куда светит прожектор.
    pub direction: Vec3,
    /// Прожектор: cos половины внешнего и внутреннего углов.
    pub spot: Option<(f32, f32)>,
    /// Тени: сдвиг против самозатенения.
    pub shadows: Option<f32>,
    pub distance2: f32,
}

impl LocalLight {
    /// Конус прожектора по компоненту: (cos половины внешнего, cos половины внутреннего).
    pub(crate) fn spot_cone(spot: &SpotLight) -> (f32, f32) {
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let outer = finite(spot.angle, 50.0).clamp(1.0, 179.0);
        let inner = finite(spot.inner_angle, 34.0).clamp(0.0, outer - 0.5);
        ((outer * 0.5).to_radians().cos(), (inner * 0.5).to_radians().cos())
    }
}

/// Грань тени лампы: матрица «мир → плитка» и место плитки в атласе.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowFace {
    pub view_proj: Mat4,
    /// Левый верхний угол плитки в текселях атласа.
    pub origin: (u32, u32),
    /// Лампа (для отсечения заслонителей её шаром).
    pub position: Vec3,
    pub range: f32,
}

/// Что из ламп и заполняющего света нужно кадру.
#[derive(Default)]
pub(crate) struct LightFrame {
    /// По [`PARAM_TEXELS`] текселей на лампу (см. lights.wgsl).
    pub params: Vec<[f32; 4]>,
    /// Ячейки кластеров: по 16 байтов-номеров (0xFF — пусто).
    pub cells: Vec<[u32; 4]>,
    counts: Vec<u8>,
    /// По [`FACE_TEXELS`] текселей на грань тени.
    pub faces: Vec<[f32; 4]>,
    pub shadow_faces: Vec<ShadowFace>,
    pub light_count: usize,
}

/// Настройки кадра для ламп (из компонентов и пресета).
#[derive(Clone, Debug)]
pub(crate) struct LightSetup {
    /// Нужен вариант шейдера с лампами.
    pub extended: bool,
    /// Ламп в кадре, не больше.
    pub max_lights: usize,
    /// Ламп с тенью, не больше.
    pub shadow_budget: usize,
    /// Заполняющий свет сцены, если есть.
    pub fill: Option<FillLight>,
}

impl LightSetup {
    /// Что просит сцена с ограничениями пресета `max_lights` / `shadow_budget`.
    pub(crate) fn of(scene: &Scene, preset_lights: u32, preset_shadows: u32) -> Self {
        let mut many = None;
        if scene.count_with::<ManyLights>() > 0 {
            scene.visit_active_with::<ManyLights>(|_, m| {
                if many.is_none() && m.enabled {
                    many = Some(m.max_lights);
                }
            });
        }
        let mut fill = None;
        if scene.count_with::<FillLight>() > 0 {
            scene.visit_active_with::<FillLight>(|_, f| {
                if fill.is_none() && f.enabled {
                    fill = Some(f.clone());
                }
            });
        }
        let mut shadows = false;
        if scene.count_with::<LightShadows>() > 0 {
            scene.visit_active_with::<LightShadows>(|_, s| shadows |= s.enabled);
        }
        let spots = scene.count_with::<SpotLight>() > 0;
        let max_lights = match many {
            Some(n) => (n.clamp(1, MAX_LIGHTS as i32) as usize).min((preset_lights as usize).clamp(4, MAX_LIGHTS)),
            None => 4,
        };
        Self {
            extended: many.is_some() || fill.is_some() || shadows || spots,
            max_lights,
            shadow_budget: if shadows { preset_shadows as usize } else { 0 },
            fill,
        }
    }
}

/// Цвета заполняющего света (верх, низ) для шейдера: умножаются на `Environment::ambient`.
pub(crate) fn fill_colors(fill: &FillLight, sky_zenith: Vec3, sky_horizon: Vec3, sun_direction: Vec3, sun_color: Vec3) -> (Vec3, Vec3) {
    let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
    let intensity = finite(fill.intensity, 1.0).max(0.0);
    let sky = (sky_horizon + sky_zenith) * 0.5;
    let luminance = sky.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    // Верх: небо сцены почти без обесцвечивания (у ambient без компонента — наполовину серое).
    let tinted = Vec3::splat(luminance).lerp(sky, 0.8) * 1.3;
    let own = Vec3::new(fill.sky_color.r, fill.sky_color.g, fill.sky_color.b) * luminance.max(0.05) * 2.2;
    let up = own.lerp(tinted, finite(fill.sky_tint, 1.0).clamp(0.0, 1.0)) * intensity;
    // Низ: земля, освещённая солнцем (по его высоте) и небом, отражает свой цвет.
    let elevation = (-sun_direction.y).max(0.0);
    let sun_luma = sun_color.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    let lit_ground = luminance * 0.7 + sun_luma * elevation * 0.12;
    let ground = Vec3::new(fill.ground_color.r, fill.ground_color.g, fill.ground_color.b);
    let down = ground * lit_ground * finite(fill.bounce, 1.0).max(0.0) * intensity;
    (up, down)
}

impl LightFrame {
    /// Разложить лампы по кластерам вида `view_proj` и выбрать лампы с тенью.
    /// `lights` сортируются по расстоянию до камеры и обрезаются до `max_lights`.
    pub(crate) fn build(&mut self, lights: &mut Vec<LocalLight>, view_proj: Mat4, setup: &LightSetup) {
        lights.sort_by(|a, b| a.distance2.total_cmp(&b.distance2));
        lights.truncate(setup.max_lights.min(MAX_LIGHTS));
        self.light_count = lights.len();
        self.params.clear();
        self.faces.clear();
        self.shadow_faces.clear();
        let cells = (CLUSTER_X * CLUSTER_Y * CLUSTER_Z) as usize;
        self.cells.clear();
        self.cells.resize(cells, [u32::MAX; 4]);
        self.counts.clear();
        self.counts.resize(cells, 0);

        // Тени: ближние лампы с `LightShadows`, пока есть бюджет и плитки атласа.
        let mut budget = setup.shadow_budget;
        let mut tiles_used = 0usize;
        for light in lights.iter() {
            let mut shadow_slot = -1.0;
            if let Some(bias) = light.shadows {
                let need = if light.spot.is_some() { 1 } else { 6 };
                if budget > 0 && tiles_used + need <= TILES {
                    budget -= 1;
                    shadow_slot = self.shadow_faces.len() as f32;
                    for face in light_faces(light) {
                        let tile = tiles_used;
                        tiles_used += 1;
                        let per_row = ATLAS_SIZE / TILE_SIZE;
                        let origin = ((tile as u32 % per_row) * TILE_SIZE, (tile as u32 / per_row) * TILE_SIZE);
                        let m = face.to_cols_array();
                        for c in 0..4 {
                            self.faces.push([m[c * 4], m[c * 4 + 1], m[c * 4 + 2], m[c * 4 + 3]]);
                        }
                        let s = TILE_SIZE as f32 / ATLAS_SIZE as f32;
                        self.faces.push([origin.0 as f32 / ATLAS_SIZE as f32, origin.1 as f32 / ATLAS_SIZE as f32, s, s]);
                        self.shadow_faces.push(ShadowFace { view_proj: face, origin, position: light.position, range: light.range });
                    }
                    let _ = bias;
                }
            }
            let (kind, cos_outer, cos_inner, tan_half) = match light.spot {
                Some((outer, inner)) => (1.0, outer, inner, spot_fov(outer).0),
                None => (0.0, -2.0, -1.0, point_tan_half()),
            };
            self.params.push([light.position.x, light.position.y, light.position.z, light.range]);
            self.params.push([light.color.x, light.color.y, light.color.z, kind]);
            self.params.push([light.direction.x, light.direction.y, light.direction.z, cos_outer]);
            self.params.push([cos_inner, shadow_slot, tan_half, light.shadows.unwrap_or(0.0)]);
        }

        // Кластеры: прямоугольник шара лампы на экране и его диапазон глубин.
        let slice_scale = CLUSTER_Z as f32 / (CLUSTER_FAR / CLUSTER_NEAR).ln();
        for (index, light) in lights.iter().enumerate() {
            let Some((x0, x1, y0, y1, z0, z1)) = cluster_range(view_proj, light.position, light.range, slice_scale) else {
                continue;
            };
            for z in z0..=z1 {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let cell = ((z * CLUSTER_Y + y) * CLUSTER_X + x) as usize;
                        let count = self.counts[cell] as usize;
                        if count >= PER_CELL {
                            continue;
                        }
                        let word = &mut self.cells[cell][count / 4];
                        let shift = (count % 4) * 8;
                        *word = (*word & !(0xFF << shift)) | ((index as u32) << shift);
                        self.counts[cell] += 1;
                    }
                }
            }
        }
    }
}

/// Параметры кластеров для шейдера: (ширина, высота сцены в пикселях, ближняя глубина,
/// масштаб логарифма слоя).
pub(crate) fn cluster_params(width: u32, height: u32) -> [f32; 4] {
    [width as f32, height as f32, CLUSTER_NEAR, CLUSTER_Z as f32 / (CLUSTER_FAR / CLUSTER_NEAR).ln()]
}

/// Ячейки, которые задевает шар (центр, радиус): (x0, x1, y0, y1, z0, z1) или `None`,
/// если шар целиком вне кадра.
fn cluster_range(view_proj: Mat4, center: Vec3, radius: f32, slice_scale: f32) -> Option<(u32, u32, u32, u32, u32, u32)> {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    let mut w_min = f32::INFINITY;
    let mut w_max = f32::NEG_INFINITY;
    let mut behind = false;
    for i in 0..8 {
        let corner = center
            + Vec3::new(
                if i & 1 == 0 { -radius } else { radius },
                if i & 2 == 0 { -radius } else { radius },
                if i & 4 == 0 { -radius } else { radius },
            );
        let clip = view_proj * corner.extend(1.0);
        w_min = w_min.min(clip.w);
        w_max = w_max.max(clip.w);
        if clip.w <= 1e-3 {
            behind = true;
            continue;
        }
        let ndc = Vec2::new(clip.x, clip.y) / clip.w;
        min = min.min(ndc);
        max = max.max(ndc);
    }
    if w_max <= 1e-3 {
        return None;
    }
    if behind {
        // Шар пересекает плоскость камеры: по экрану — весь кадр.
        min = Vec2::splat(-1.0);
        max = Vec2::splat(1.0);
        w_min = w_min.max(1e-3);
    }
    if min.x > 1.0 || min.y > 1.0 || max.x < -1.0 || max.y < -1.0 {
        return None;
    }
    let tile = |v: f32, n: u32| (((v * 0.5 + 0.5) * n as f32).floor().max(0.0) as u32).min(n - 1);
    let x0 = tile(min.x, CLUSTER_X);
    let x1 = tile(max.x, CLUSTER_X);
    // Экран сверху вниз: y = +1 — верхняя строка.
    let y0 = tile(-max.y, CLUSTER_Y);
    let y1 = tile(-min.y, CLUSTER_Y);
    let slice = |w: f32| ((w.max(CLUSTER_NEAR) / CLUSTER_NEAR).ln() * slice_scale).floor().clamp(0.0, (CLUSTER_Z - 1) as f32) as u32;
    Some((x0, x1, y0, y1, slice(w_min), slice(w_max)))
}

/// Тангенс половины угла грани куба с запасом в 3 текселя (PCF у края грани не выходит за плитку).
fn point_tan_half() -> f32 {
    1.0 + 6.0 / TILE_SIZE as f32
}

/// Угол обзора тени прожектора (тангенс половины, полный угол в радианах).
fn spot_fov(cos_outer: f32) -> (f32, f32) {
    let half = cos_outer.clamp(-1.0, 1.0).acos().min(85f32.to_radians());
    let tan = half.tan() * (1.0 + 6.0 / TILE_SIZE as f32);
    (tan, 2.0 * tan.atan())
}

/// Матрицы граней тени лампы: шесть граней куба (+X, −X, +Y, −Y, +Z, −Z — тот же порядок
/// в lights.wgsl) или одна для прожектора. Глубина — обычная 0…1 (как у карты солнца).
fn light_faces(light: &LocalLight) -> Vec<Mat4> {
    let near = (light.range * 0.01).clamp(0.02, 0.2);
    let far = light.range.max(near * 2.0);
    match light.spot {
        Some((outer, _)) => {
            let (_, fov) = spot_fov(outer);
            let dir = light.direction.normalize_or(Vec3::NEG_Z);
            let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
            vec![Mat4::perspective_rh(fov, 1.0, near, far) * Mat4::look_to_rh(light.position, dir, up)]
        }
        None => {
            let fov = 2.0 * point_tan_half().atan();
            let projection = Mat4::perspective_rh(fov, 1.0, near, far);
            [
                (Vec3::X, Vec3::NEG_Y),
                (Vec3::NEG_X, Vec3::NEG_Y),
                (Vec3::Y, Vec3::Z),
                (Vec3::NEG_Y, Vec3::NEG_Z),
                (Vec3::Z, Vec3::NEG_Y),
                (Vec3::NEG_Z, Vec3::NEG_Y),
            ]
            .iter()
            .map(|&(dir, up)| projection * Mat4::look_to_rh(light.position, dir, up))
            .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(position: Vec3, range: f32) -> LocalLight {
        LocalLight {
            position,
            range,
            color: Vec3::ONE,
            direction: Vec3::NEG_Z,
            spot: None,
            shadows: None,
            distance2: position.length_squared(),
        }
    }

    fn view() -> Mat4 {
        crate::depth::perspective(60f32.to_radians(), 16.0 / 9.0, 0.05, 2000.0)
            * Mat4::look_at_rh(Vec3::new(0.0, 2.0, 10.0), Vec3::new(0.0, 0.0, 0.0), Vec3::Y)
    }

    #[test]
    fn a_light_lands_in_the_cells_around_its_screen_position_and_depth() {
        let mut frame = LightFrame::default();
        let setup = LightSetup { extended: true, max_lights: 255, shadow_budget: 0, fill: None };
        let mut lights = vec![light(Vec3::new(0.0, 0.0, 0.0), 1.0)];
        frame.build(&mut lights, view(), &setup);
        // Центр экрана, глубина ~10 м.
        let slice = ((10.2f32 / CLUSTER_NEAR).ln() * cluster_params(1, 1)[3]).floor() as u32;
        let cell = ((slice * CLUSTER_Y + CLUSTER_Y / 2) * CLUSTER_X + CLUSTER_X / 2) as usize;
        assert_eq!(frame.cells[cell][0] & 0xFF, 0, "the light is listed in its own cell");
        let used = frame.counts.iter().filter(|&&c| c > 0).count();
        assert!(used > 0 && used < 40, "a small light touches a few cells, not the screen: {used}");
        // Лампа за спиной камеры не попадает никуда.
        let mut behind = vec![light(Vec3::new(0.0, 2.0, 30.0), 1.0)];
        frame.build(&mut behind, view(), &setup);
        assert!(frame.counts.iter().all(|&c| c == 0));
    }

    #[test]
    fn cells_keep_at_most_sixteen_nearest_lights_and_the_setup_caps_the_count() {
        let mut frame = LightFrame::default();
        let setup = LightSetup { extended: true, max_lights: 20, shadow_budget: 0, fill: None };
        let mut lights: Vec<_> = (0..30).map(|i| light(Vec3::new(0.0, 0.0, -(i as f32) * 0.01), 50.0)).collect();
        frame.build(&mut lights, view(), &setup);
        assert_eq!(frame.light_count, 20);
        assert!(frame.counts.iter().all(|&c| c as usize <= PER_CELL));
        assert!(frame.counts.iter().any(|&c| c as usize == PER_CELL));
    }

    #[test]
    fn shadowed_lights_take_atlas_tiles_within_the_budget() {
        let mut frame = LightFrame::default();
        let setup = LightSetup { extended: true, max_lights: 255, shadow_budget: 2, fill: None };
        let mut point = light(Vec3::ZERO, 5.0);
        point.shadows = Some(0.03);
        let mut spot = light(Vec3::X, 5.0);
        spot.shadows = Some(0.03);
        spot.spot = Some(LocalLight::spot_cone(&SpotLight::default()));
        let mut third = light(Vec3::Y * 3.0, 5.0);
        third.shadows = Some(0.03);
        let mut lights = vec![point, spot, third];
        frame.build(&mut lights, view(), &setup);
        assert_eq!(frame.shadow_faces.len(), 7, "a point light takes six faces, a spot one; the third is over budget");
        assert_eq!(frame.faces.len(), 7 * FACE_TEXELS as usize);
        // Плитки не пересекаются.
        let origins: std::collections::HashSet<_> = frame.shadow_faces.iter().map(|f| f.origin).collect();
        assert_eq!(origins.len(), 7);
    }

    #[test]
    fn point_faces_cover_their_axis() {
        let l = light(Vec3::new(1.0, 2.0, 3.0), 10.0);
        let faces = light_faces(&l);
        let axes = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        for (face, axis) in faces.iter().zip(axes) {
            let clip = *face * (l.position + axis * 4.0 + axis.any_orthonormal_vector() * 3.9).extend(1.0);
            let ndc = clip.truncate() / clip.w;
            assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z), "{axis}: {ndc}");
        }
    }
}
