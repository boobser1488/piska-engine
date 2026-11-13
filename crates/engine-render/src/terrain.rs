//! Рельеф (`Terrain`) и вода (`Water`) — как в проекте-образце: процедурный рельеф из
//! слоёв шума (материки, холмы, хребты, площадки), разбитый на чанки с уровнями
//! детализации и подгрузкой вокруг камеры, текстуры по высоте и уклону с
//! раскраской кистью поверх. Высоты можно взять и из карты высот (16-битный PNG или
//! `.raw`). Скульпт и раскраска хранятся правками поверх базы (поле `edits`).
//!
//! Объект с `Terrain`: позиция — угол рельефа (x, z) и нулевая высота (y), рельеф
//! уходит в +x и +z на `size` метров. Поворот и масштаб объекта не применяются
//! (как в Unity).
//!
//! Запросы для скриптов: [`terrain_height`], [`terrain_normal`], [`terrain_raycast`].
//! Коллайдер для 3D-физики создаётся сам (подсистема фасада `engine`).

use engine_assets::{AssetDatabase, AssetHandle, Texture};
use engine_core::prelude::*;

use crate::terrain_data::{layout_of, shared_for, TerrainLayout, TerrainSampler, LAYER_COUNT, TILE};
pub use crate::terrain_data::TerrainEdits;

reflect_enum! {
    /// Откуда берутся высоты рельефа.
    pub enum TerrainSource { Procedural, Heightmap }
}

component! {
    /// Рельеф: чанки с уровнями детализации, подгрузка вокруг камеры, слои текстур по
    /// высоте и уклону, кисти скульпта и раскраски. Позиция объекта — угол рельефа.
    pub struct Terrain {
        /// Procedural — шум (как в образце), Heightmap — картинка `heightmap`.
        pub source: TerrainSource = TerrainSource::Procedural,
        /// Сид шума: один сид — один и тот же рельеф.
        pub seed: i64 = 1337,
        /// Сторона рельефа, м (округляется до целого числа чанков).
        pub size: f32 = 1024.0,
        /// Сторона чанка, м.
        pub chunk_size: f32 = 64.0,
        /// Клеток на сторону чанка в ближнем LOD (16, 32, 64 или 128).
        pub resolution: u32 = 64,
        /// Масштаб форм: 2 — горы и холмы вдвое шире.
        pub noise_scale: f32 = 1.0,
        /// Множитель всех высот.
        pub height_scale: f32 = 1.0,
        /// Множитель высоты горных хребтов (0 — без гор).
        pub mountains: f32 = 1.0,
        /// Октавы холмов: больше — больше мелких деталей.
        pub octaves: u32 = 5,
        /// Суша против моря: больше — больше суши (в образце 0.18).
        pub land_bias: f32 = 0.18,
        /// Выровненные площадки (x, z, радиус) в метрах от угла рельефа.
        pub flat_zones: Vec<Vec3> = Vec::new(),
        /// Ширина перехода от площадки к природному рельефу, м.
        pub flat_falloff: f32 = 40.0,
        /// Карта высот (16-битный PNG, обычный PNG или квадратный `.raw`/`.r16`):
        /// белое — `heightmap_height`, чёрное — 0. Верхняя строка — край z = 0.
        pub heightmap: AssetHandle<Texture> = AssetHandle::none(),
        /// Высота белого цвета карты высот, м.
        pub heightmap_height: f32 = 200.0,
        /// Дальность прорисовки, м.
        pub view_distance: f32 = 1500.0,
        /// Множитель дальностей смены LOD: больше — детальнее вдали, но больше треугольников.
        pub lod_distance: f32 = 1.0,
        /// Рельеф отбрасывает тени от солнца.
        pub cast_shadows: bool = true,
        /// Уровень моря для текстур: ниже — песок и мокрое дно.
        pub sea_level: f32 = 0.0,
        /// Высота, с которой начинается снег, м.
        pub snow_height: f32 = 200.0,
        /// Пустыня: 0 — трава, 1 — песок и тёплые скалы без снега.
        pub desert: f32 = 0.0,
        /// Масштаб текстур слоёв: 2 — узор вдвое крупнее.
        pub texture_scale: f32 = 1.0,
        /// Свои текстуры слоёв (пусто — встроенные).
        pub grass_texture: AssetHandle<Texture> = AssetHandle::none(),
        pub dirt_texture: AssetHandle<Texture> = AssetHandle::none(),
        pub rock_texture: AssetHandle<Texture> = AssetHandle::none(),
        pub sand_texture: AssetHandle<Texture> = AssetHandle::none(),
        pub snow_texture: AssetHandle<Texture> = AssetHandle::none(),
        /// Оттенки слоёв (умножаются на текстуру).
        pub grass_color: Color = Color::WHITE,
        pub dirt_color: Color = Color::WHITE,
        pub rock_color: Color = Color::WHITE,
        pub sand_color: Color = Color::WHITE,
        pub snow_color: Color = Color::WHITE,
        /// Скульпт и раскраска кистью поверх базовых высот.
        pub edits: TerrainEdits = TerrainEdits::default(),
    }
}

impl Component for Terrain {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Имена полей, от которых зависит форма рельефа (их меняет «Regenerate» в редакторе).
pub const TERRAIN_SHAPE_FIELDS: &[&str] = &[
    "source",
    "seed",
    "size",
    "chunk_size",
    "resolution",
    "noise_scale",
    "height_scale",
    "mountains",
    "octaves",
    "land_bias",
    "flat_zones",
    "flat_falloff",
    "heightmap",
    "heightmap_height",
];

/// Названия слоёв в порядке их номеров (для раскраски и инспектора).
pub const TERRAIN_LAYERS: [&str; LAYER_COUNT] = ["Grass", "Dirt", "Rock", "Sand", "Snow"];

/// Инструмент кисти рельефа.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushTool {
    /// Поднять (с отрицательной силой — опустить).
    Raise,
    Lower,
    /// Сгладить: высота тянется к среднему соседей.
    Smooth,
    /// Выровнять к высоте `target_height`.
    Flatten,
    /// Рисовать слой `layer` (`None` — стереть раскраску, вернуть автоматические слои).
    Paint,
}

/// Кисть рельефа. Сила — доля от «полного» действия за секунду.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainBrush {
    pub tool: BrushTool,
    /// Радиус, м.
    pub radius: f32,
    /// 0..1.
    pub strength: f32,
    /// Мягкость края: 0 — жёсткий край, 1 — спадает от центра.
    pub falloff: f32,
    /// Высота для `Flatten` (локальная, от позиции объекта).
    pub target_height: f32,
    /// Слой для `Paint` (номер в [`TERRAIN_LAYERS`]); `None` — стереть раскраску.
    pub layer: Option<usize>,
}

impl Default for TerrainBrush {
    fn default() -> Self {
        Self { tool: BrushTool::Raise, radius: 12.0, strength: 0.5, falloff: 0.6, target_height: 0.0, layer: Some(0) }
    }
}

impl TerrainBrush {
    /// Вес кисти на расстоянии `distance` от центра.
    pub fn weight(&self, distance: f32) -> f32 {
        let d = distance / self.radius.max(1e-3);
        if d >= 1.0 {
            return 0.0;
        }
        let inner = 1.0 - self.falloff.clamp(0.0, 1.0);
        if d <= inner {
            return 1.0;
        }
        let t = ((d - inner) / (1.0 - inner).max(1e-4)).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}

impl Terrain {
    /// Раскладка сетки: число чанков, клетки, шаг.
    pub fn layout(&self) -> TerrainLayout {
        layout_of(self)
    }

    /// Сторона рельефа в метрах после округления до чанков.
    pub fn extent(&self) -> f32 {
        self.layout().size()
    }

    /// Поверхность для запросов. `assets` нужны только для карты высот.
    pub fn sampler(&self, assets: Option<&AssetDatabase>) -> TerrainSampler {
        TerrainSampler { shared: shared_for(self, assets), edits: self.edits.0.clone() }
    }

    /// Высота в локальной точке (x, z от угла рельефа), относительно позиции объекта.
    /// Для карты высот нужен хотя бы один кадр с ассетами (см. [`terrain_height`]).
    pub fn height_local(&self, x: f32, z: f32) -> f32 {
        self.sampler(None).height_local(x, z)
    }

    /// Сбросить весь скульпт и раскраску.
    pub fn clear_edits(&mut self) {
        self.edits = TerrainEdits::default();
    }

    /// Задать высоты отсчётов в прямоугольнике `min..=max` (локальные x, z):
    /// `f(точка, текущая высота) → новая высота`. Unity: `TerrainData.SetHeights`.
    /// Изменение хранится правкой поверх базы (как скульпт кистью).
    pub fn modify_heights(
        &mut self,
        min: Vec2,
        max: Vec2,
        assets: Option<&AssetDatabase>,
        mut f: impl FnMut(Vec2, f32) -> f32,
    ) -> bool {
        let layout = self.layout();
        let sp = layout.spacing();
        let n = layout.samples();
        let gi0 = ((min.x / sp).ceil().max(0.0) as i32).min(n + 1);
        let gj0 = ((min.y / sp).ceil().max(0.0) as i32).min(n + 1);
        let gi1 = ((max.x / sp).floor() as i32).min(n);
        let gj1 = ((max.y / sp).floor() as i32).min(n);
        if gi0 > gi1 || gj0 > gj1 {
            return false;
        }
        let w = (gi1 - gi0 + 1) as usize;
        let mut heights = Vec::new();
        let sampler = self.sampler(assets);
        sampler.fill(gi0, gj0, 1, w, (gj1 - gj0 + 1) as usize, &mut heights);
        drop(sampler);
        let data = self.edits.data_mut();
        data.rebase(sp, &layout);
        let mut touched = Vec::new();
        for gj in gj0..=gj1 {
            for gi in gi0..=gi1 {
                let current = heights[(gj - gj0) as usize * w + (gi - gi0) as usize];
                let wanted = f(Vec2::new(gi as f32 * sp, gj as f32 * sp), current);
                if !wanted.is_finite() || wanted == current {
                    continue;
                }
                let key = (gi.div_euclid(TILE), gj.div_euclid(TILE));
                let tile = data.tile_mut(key);
                if tile.delta.is_empty() {
                    tile.delta = vec![0.0; (TILE * TILE) as usize];
                }
                tile.delta[(gj.rem_euclid(TILE) * TILE + gi.rem_euclid(TILE)) as usize] += wanted - current;
                if !touched.contains(&key) {
                    touched.push(key);
                }
            }
        }
        for key in &touched {
            if let Some(tile) = data.tiles.get_mut(key) {
                std::sync::Arc::make_mut(tile).rehash(*key);
            }
        }
        !touched.is_empty()
    }

    /// Применить кисть с центром в локальной точке (x, z) на время `dt` секунд.
    /// Возвращает, изменилось ли что-нибудь. Меняются только отсчёты внутри радиуса.
    pub fn apply_brush(&mut self, brush: &TerrainBrush, x: f32, z: f32, dt: f32, assets: Option<&AssetDatabase>) -> bool {
        let layout = self.layout();
        let sp = layout.spacing();
        let n = layout.samples();
        let r = brush.radius.max(sp * 0.5);
        let gi0 = ((x - r) / sp).floor().max(0.0) as i32;
        let gj0 = ((z - r) / sp).floor().max(0.0) as i32;
        let gi1 = (((x + r) / sp).ceil() as i32).min(n);
        let gj1 = (((z + r) / sp).ceil() as i32).min(n);
        if gi0 > gi1 || gj0 > gj1 || dt <= 0.0 {
            return false;
        }
        // Текущие высоты области с рамкой в один отсчёт (для сглаживания).
        let sampler = self.sampler(assets);
        let (w, h) = ((gi1 - gi0 + 3) as usize, (gj1 - gj0 + 3) as usize);
        let mut heights = Vec::new();
        sampler.fill(gi0 - 1, gj0 - 1, 1, w, h, &mut heights);
        drop(sampler); // иначе правка ниже скопирует общие данные
        let at = |i: i32, j: i32| heights[((j - gj0 + 1) as usize) * w + (i - gi0 + 1) as usize];

        let strength = brush.strength.clamp(0.0, 1.0);
        let pull = (strength * dt * 8.0).min(1.0);
        let data = self.edits.data_mut();
        data.rebase(sp, &layout);
        let mut changed = false;
        let mut touched = Vec::new();
        for gj in gj0..=gj1 {
            for gi in gi0..=gi1 {
                let distance = Vec2::new(gi as f32 * sp - x, gj as f32 * sp - z).length();
                let weight = brush.weight(distance);
                if weight <= 0.0 {
                    continue;
                }
                let key = (gi.div_euclid(TILE), gj.div_euclid(TILE));
                let index = (gj.rem_euclid(TILE) * TILE + gi.rem_euclid(TILE)) as usize;
                let current = at(gi, gj);
                let change = match brush.tool {
                    BrushTool::Raise => strength * dt * 10.0 * weight,
                    BrushTool::Lower => -strength * dt * 10.0 * weight,
                    BrushTool::Smooth => {
                        let average = (at(gi - 1, gj) + at(gi + 1, gj) + at(gi, gj - 1) + at(gi, gj + 1)
                            + at(gi - 1, gj - 1) + at(gi + 1, gj - 1) + at(gi - 1, gj + 1) + at(gi + 1, gj + 1))
                            / 8.0;
                        (average - current) * pull * weight
                    }
                    BrushTool::Flatten => (brush.target_height - current) * pull * weight,
                    BrushTool::Paint => 0.0,
                };
                let tile = data.tile_mut(key);
                if brush.tool == BrushTool::Paint {
                    if tile.paint.is_empty() {
                        tile.paint = vec![[0; LAYER_COUNT]; (TILE * TILE) as usize];
                    }
                    let amount = (strength * dt * 6.0 * weight).min(1.0);
                    let cell = &mut tile.paint[index];
                    let before = *cell;
                    let mut weights = cell.map(|v| v as f32 / 255.0);
                    for (k, value) in weights.iter_mut().enumerate() {
                        let goal = if brush.layer == Some(k) { 1.0 } else { 0.0 };
                        *value += (goal - *value) * amount;
                    }
                    *cell = weights.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8);
                    // Последние доли процента не исчезают от округления: добиваем.
                    if *cell == before && amount > 0.0 {
                        for (k, value) in cell.iter_mut().enumerate() {
                            if brush.layer == Some(k) {
                                *value = value.saturating_add(1);
                            } else {
                                *value = value.saturating_sub(1);
                            }
                        }
                    }
                    changed |= *cell != before;
                } else if change != 0.0 {
                    if tile.delta.is_empty() {
                        tile.delta = vec![0.0; (TILE * TILE) as usize];
                    }
                    tile.delta[index] += change;
                    changed = true;
                }
                if touched.last() != Some(&key) && !touched.contains(&key) {
                    touched.push(key);
                }
            }
        }
        for key in touched {
            if let Some(tile) = data.tiles.get_mut(&key) {
                std::sync::Arc::make_mut(tile).rehash(key);
            }
        }
        changed
    }
}

component! {
    /// Вода: плоскость на высоте объекта с волнами (по нормали), отражением неба и
    /// бликом солнца — как море в образце. `size` 0 — бесконечное море вокруг камеры.
    pub struct Water {
        /// Сторона квадрата воды с центром в позиции объекта, м. 0 — до горизонта.
        pub size: f32 = 0.0,
        /// Цвет глубины.
        pub deep_color: Color = Color::rgb(0.006, 0.05, 0.065),
        /// Непрозрачность при взгляде сверху (у горизонта вода всегда непрозрачна).
        pub opacity: f32 = 0.72,
        /// Высота волн (только освещение): 0 — зеркало.
        pub waves: f32 = 1.0,
        pub wave_speed: f32 = 1.0,
    }
}

impl Component for Water {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Попадание луча в рельеф.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainHit {
    pub object: GameObjectId,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

/// Позиция угла рельефа в мире.
fn origin_of(scene: &Scene, id: GameObjectId) -> Vec3 {
    scene.world_matrix(id).w_axis.truncate()
}

/// Рельеф под точкой (x, z) мира: объект, его угол и поверхность.
pub fn terrain_at(world: &World, x: f32, z: f32) -> Option<(GameObjectId, Vec3, TerrainSampler)> {
    let assets = world.resources.get::<AssetDatabase>();
    let mut found = None;
    world.scene.visit_active_with::<Terrain>(|id, terrain| {
        if found.is_some() {
            return;
        }
        let origin = origin_of(&world.scene, id);
        let size = terrain.extent();
        let (lx, lz) = (x - origin.x, z - origin.z);
        if (0.0..=size).contains(&lx) && (0.0..=size).contains(&lz) {
            found = Some((id, origin, terrain.sampler(assets)));
        }
    });
    found
}

/// Unity: `Terrain.SampleHeight` — высота поверхности рельефа под точкой (x, z) мира
/// (мировая y). `None` — под точкой нет активного рельефа.
pub fn terrain_height(world: &World, x: f32, z: f32) -> Option<f32> {
    let (_, origin, sampler) = terrain_at(world, x, z)?;
    Some(origin.y + sampler.height_local(x - origin.x, z - origin.z))
}

/// Нормаль поверхности рельефа под точкой (x, z) мира.
pub fn terrain_normal(world: &World, x: f32, z: f32) -> Option<Vec3> {
    let (_, origin, sampler) = terrain_at(world, x, z)?;
    Some(sampler.normal_local(x - origin.x, z - origin.z))
}

/// Луч по всем активным рельефам сцены (без физики — работает и в редакторе).
pub fn terrain_raycast(world: &World, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<TerrainHit> {
    let direction = direction.try_normalize()?;
    let assets = world.resources.get::<AssetDatabase>();
    let mut best: Option<TerrainHit> = None;
    world.scene.visit_active_with::<Terrain>(|id, terrain| {
        let corner = origin_of(&world.scene, id);
        let sampler = terrain.sampler(assets);
        if let Some(t) = sampler.raycast_local(origin - corner, direction, max_distance) {
            if best.is_none_or(|b| t < b.distance) {
                let point = origin + direction * t;
                best = Some(TerrainHit {
                    object: id,
                    point,
                    normal: sampler.normal_local(point.x - corner.x, point.z - corner.z),
                    distance: t,
                });
            }
        }
    });
    best
}

/// Поставить объект на рельеф: его мировая позиция опускается (или поднимается) на
/// поверхность под ним плюс `offset`. `false` — под объектом нет рельефа.
pub fn snap_to_terrain(world: &mut World, id: GameObjectId, offset: f32) -> bool {
    let Some(object) = world.scene.get(id) else { return false };
    let parent = object.parent();
    let position = world.scene.world_matrix(id).w_axis.truncate();
    let Some(ground) = terrain_height(world, position.x, position.z) else {
        return false;
    };
    let target = Vec3::new(position.x, ground + offset, position.z);
    let local = match parent {
        Some(parent) => world.scene.world_matrix(parent).inverse().transform_point3(target),
        None => target,
    };
    if let Some(object) = world.scene.get_mut(id) {
        object.transform.position = local;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(seed: i64) -> Terrain {
        Terrain { seed, size: 256.0, ..Terrain::default() }
    }

    #[test]
    fn same_seed_same_heights_other_seed_differs() {
        let (a, b, c) = (small(5), small(5), small(6));
        let mut differs = 0;
        for k in 0..200 {
            let (x, z) = ((k * 37 % 256) as f32 + 0.3, (k * 91 % 256) as f32 + 0.7);
            assert_eq!(a.height_local(x, z).to_bits(), b.height_local(x, z).to_bits());
            if (a.height_local(x, z) - c.height_local(x, z)).abs() > 0.01 {
                differs += 1;
            }
        }
        assert!(differs > 150, "another seed gives another landscape ({differs}/200 differ)");
    }

    #[test]
    fn brush_changes_heights_only_inside_and_survives_text() {
        let mut terrain = small(9);
        let before: Vec<f32> = (0..64).map(|k| terrain.height_local(k as f32 * 4.0, 100.0)).collect();
        let brush = TerrainBrush { tool: BrushTool::Raise, radius: 10.0, strength: 1.0, falloff: 0.5, ..TerrainBrush::default() };
        assert!(terrain.apply_brush(&brush, 128.0, 100.0, 0.5, None));
        for (k, old) in before.iter().enumerate() {
            let x = k as f32 * 4.0;
            let now = terrain.height_local(x, 100.0);
            if (x - 128.0).abs() > 11.0 {
                assert_eq!(now.to_bits(), old.to_bits(), "outside the brush at x = {x}");
            }
        }
        let centre = terrain.height_local(128.0, 100.0);
        assert!((centre - before[32]).abs() > 4.0, "raised by ~5 m: {} → {centre}", before[32]);

        // Сохранение через reflection и загрузка обратно.
        let value = terrain.get_field("edits").unwrap();
        let mut loaded = small(9);
        loaded.set_field("edits", &value).unwrap();
        assert!((loaded.height_local(128.0, 100.0) - centre).abs() < 2e-3);
    }

    #[test]
    fn flatten_and_paint() {
        let mut terrain = small(3);
        let brush = TerrainBrush { tool: BrushTool::Flatten, radius: 8.0, strength: 1.0, falloff: 0.0, target_height: 42.0, layer: None };
        for _ in 0..30 {
            terrain.apply_brush(&brush, 50.0, 50.0, 0.1, None);
        }
        assert!((terrain.height_local(50.0, 50.0) - 42.0).abs() < 0.05);
        let paint = TerrainBrush { tool: BrushTool::Paint, layer: Some(2), ..brush };
        for _ in 0..20 {
            terrain.apply_brush(&paint, 50.0, 50.0, 0.1, None);
        }
        let sampler = terrain.sampler(None);
        let sp = terrain.layout().spacing();
        let p = sampler.paint_at_sample((50.0 / sp) as i32, (50.0 / sp) as i32);
        assert!(p[2] > 240 && p[0] < 10, "{p:?}");
    }

    #[test]
    fn raycast_finds_the_surface() {
        let terrain = small(11);
        let sampler = terrain.sampler(None);
        let ground = sampler.height_local(100.0, 120.0);
        let t = sampler
            .raycast_local(Vec3::new(100.0, ground + 50.0, 120.0), Vec3::NEG_Y, 500.0)
            .expect("a ray straight down hits");
        assert!((t - 50.0).abs() < 0.01, "{t}");
        assert!(sampler.raycast_local(Vec3::new(-50.0, 0.0, -50.0), Vec3::NEG_Y, 500.0).is_none());
    }
}
