//! Данные рельефа без GPU: раскладка сетки, общий кэш «базовых» высот (шум или
//! карта высот), правки кистью (скульпт и раскраска) и выборка итоговых высот.
//!
//! Поверхность — сетка отсчётов с шагом `spacing` метров: отсчёт (gi, gj) лежит в
//! точке (gi · spacing, gj · spacing) от угла рельефа. Высота отсчёта = база + правка.
//! Между отсчётами — два треугольника на клетку с диагональю из (x1, z0) в (x0, z1):
//! так же режет клетки и меш (LOD 0), и `HeightField` в Rapier, поэтому
//! `height_at`, картинка и коллайдер совпадают.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use engine_assets::AssetDatabase;
use engine_core::prelude::*;
use engine_core::reflect::FieldValue;

use crate::terrain::{Terrain, TerrainSource};
use crate::terrain_noise::{Generator, NoiseParams};

/// Номер чанка (x, z).
type ChunkKey = (i32, i32);

/// Сколько слоёв можно рисовать кистью: трава, земля, скала, песок, снег.
pub const LAYER_COUNT: usize = 5;
/// Сторона плитки правок в отсчётах.
pub(crate) const TILE: i32 = 64;
/// Сколько чанков базовых высот держать в памяти на один рельеф.
const BASE_CACHE_LIMIT: usize = 4096;

/// Раскладка сетки рельефа.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainLayout {
    /// Чанков по каждой стороне.
    pub chunks: i32,
    /// Клеток на сторону чанка в LOD 0 (степень двойки, 16…128).
    pub resolution: i32,
    /// Сторона чанка, м.
    pub chunk_size: f32,
}

impl TerrainLayout {
    /// Шаг сетки отсчётов, м.
    pub fn spacing(&self) -> f32 {
        self.chunk_size / self.resolution as f32
    }

    /// Последний индекс отсчёта по стороне (отсчётов на сторону — на один больше).
    pub fn samples(&self) -> i32 {
        self.chunks * self.resolution
    }

    /// Сторона рельефа, м.
    pub fn size(&self) -> f32 {
        self.chunks as f32 * self.chunk_size
    }

    /// Сколько уровней детализации: каждый следующий вдвое реже, до 4×4 клеток на чанк.
    pub fn lod_count(&self) -> u32 {
        let mut count = 1;
        while (self.resolution >> count) >= 4 && count < 5 {
            count += 1;
        }
        count
    }
}

/// Отсчёт сетки зажать в пределы рельефа.
#[inline]
fn clamp_sample(layout: &TerrainLayout, g: i32) -> i32 {
    g.clamp(0, layout.samples())
}

/// Чанк, которому принадлежит отсчёт (последний ряд — последнему чанку).
#[inline]
fn chunk_of(layout: &TerrainLayout, g: i32) -> (i32, i32) {
    let c = (g / layout.resolution).min(layout.chunks - 1);
    (c, g - c * layout.resolution)
}

// ---------------------------------------------------------------------------
// Карта высот
// ---------------------------------------------------------------------------

/// Картинка-карта высот: значения 0..1, строка = z, столбец = x.
pub(crate) struct Heightmap {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Heightmap {
    pub fn decode(bytes: &[u8], name: &str) -> Result<Self, String> {
        // Сырые 16-битные (little-endian) квадратные карты: `.raw` / `.r16`.
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".raw") || lower.ends_with(".r16") {
            let count = bytes.len() / 2;
            let side = (count as f64).sqrt().round() as usize;
            if side < 2 || side * side != count {
                return Err(format!(
                    "`{name}` is not a square 16-bit raw heightmap ({} bytes); export it as N×N little-endian 16-bit values",
                    bytes.len()
                ));
            }
            let data = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0)
                .collect();
            return Ok(Self { width: side, height: side, data });
        }
        let image = image::load_from_memory(bytes).map_err(|e| format!("cannot decode `{name}`: {e}"))?;
        let luma = image.to_luma16();
        let (width, height) = (luma.width() as usize, luma.height() as usize);
        if width < 2 || height < 2 {
            return Err(format!("heightmap `{name}` is too small ({width}×{height}); use at least 2×2 pixels"));
        }
        let data = luma.pixels().map(|p| p.0[0] as f32 / 65535.0).collect();
        Ok(Self { width, height, data })
    }

    /// Билинейная выборка; `u`, `v` — 0..1 по x и z.
    fn sample(&self, u: f32, v: f32) -> f32 {
        let x = u.clamp(0.0, 1.0) * (self.width - 1) as f32;
        let y = v.clamp(0.0, 1.0) * (self.height - 1) as f32;
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| self.data[y * self.width + x];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        top + (bottom - top) * fy
    }
}

// ---------------------------------------------------------------------------
// Общая часть рельефа: генератор и кэш базовых высот
// ---------------------------------------------------------------------------

/// Всё, что зависит только от параметров формы. Одинаковые по форме рельефы (копии,
/// объект после Undo) делят один кэш: базовые высоты не пересчитываются.
pub(crate) struct TerrainShared {
    pub hash: u64,
    pub layout: TerrainLayout,
    pub generator: Generator,
    source: TerrainSource,
    heightmap: Option<Heightmap>,
    heightmap_height: f32,
    /// Карта высот прочитана (или её нет). `false` — ассеты были недоступны.
    complete: bool,
    base: Mutex<HashMap<ChunkKey, Arc<[f32]>>>,
}

impl TerrainShared {
    /// Базовая высота отсчёта, посчитанная заново.
    fn eval(&self, gi: i32, gj: i32) -> f32 {
        let sp = self.layout.spacing() as f64;
        let (x, z) = (gi as f64 * sp, gj as f64 * sp);
        match (&self.source, &self.heightmap) {
            (TerrainSource::Heightmap, Some(map)) => {
                let n = self.layout.samples().max(1) as f32;
                map.sample(gi as f32 / n, gj as f32 / n) * self.heightmap_height
            }
            (TerrainSource::Heightmap, None) => 0.0,
            (TerrainSource::Procedural, _) => self.generator.height(x, z),
        }
    }

    /// Базовые высоты чанка: (resolution + 1)² отсчётов, строка = z.
    pub fn base_chunk(&self, ci: i32, cj: i32) -> Arc<[f32]> {
        if let Some(found) = self.base.lock().expect("terrain cache lock").get(&(ci, cj)) {
            return found.clone();
        }
        let res = self.layout.resolution;
        let w = (res + 1) as usize;
        let mut data = Vec::with_capacity(w * w);
        for j in 0..=res {
            for i in 0..=res {
                data.push(self.eval(ci * res + i, cj * res + j));
            }
        }
        let data: Arc<[f32]> = data.into();
        let mut cache = self.base.lock().expect("terrain cache lock");
        if cache.len() >= BASE_CACHE_LIMIT {
            // Простое вытеснение: выбросить четверть. Всё пересчитывается по запросу.
            let drop: Vec<(i32, i32)> = cache.keys().copied().step_by(4).collect();
            for key in drop {
                cache.remove(&key);
            }
        }
        cache.entry((ci, cj)).or_insert(data).clone()
    }

    /// Базовая высота отсчёта: из кэша, если чанк посчитан, иначе — сразу по шуму.
    fn base_sample(&self, gi: i32, gj: i32) -> f32 {
        let layout = &self.layout;
        let (gi, gj) = (clamp_sample(layout, gi), clamp_sample(layout, gj));
        let (ci, li) = chunk_of(layout, gi);
        let (cj, lj) = chunk_of(layout, gj);
        let cached = self.base.lock().expect("terrain cache lock").get(&(ci, cj)).cloned();
        match cached {
            Some(chunk) => chunk[(lj * (layout.resolution + 1) + li) as usize],
            None => self.eval(gi, gj),
        }
    }
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// Раскладка по полям компонента (с разумными пределами).
pub(crate) fn layout_of(terrain: &Terrain) -> TerrainLayout {
    let resolution = match terrain.resolution {
        0..=23 => 16,
        24..=47 => 32,
        48..=95 => 64,
        _ => 128,
    };
    let chunk_size = if terrain.chunk_size.is_finite() { terrain.chunk_size.clamp(4.0, 1024.0) } else { 64.0 };
    let size = if terrain.size.is_finite() { terrain.size.max(chunk_size) } else { chunk_size };
    let chunks = ((size / chunk_size).round() as i32).clamp(1, 256);
    TerrainLayout { chunks, resolution, chunk_size }
}

/// Хеш всего, что влияет на базовые высоты.
fn shape_hash(terrain: &Terrain, layout: &TerrainLayout) -> u64 {
    let mut h = FNV_START;
    let mut f = |v: f32| hash_bytes(&mut h, &v.to_bits().to_le_bytes());
    f(layout.chunks as f32);
    f(layout.resolution as f32);
    f(layout.chunk_size);
    f(terrain.noise_scale);
    f(terrain.height_scale);
    f(terrain.mountains);
    f(terrain.octaves as f32);
    f(terrain.land_bias);
    f(terrain.flat_falloff);
    f(terrain.heightmap_height);
    for zone in &terrain.flat_zones {
        f(zone.x);
        f(zone.y);
        f(zone.z);
    }
    hash_bytes(&mut h, &terrain.seed.to_le_bytes());
    hash_bytes(&mut h, &[terrain.source as u8]);
    if terrain.source == TerrainSource::Heightmap {
        hash_bytes(&mut h, &terrain.heightmap.guid().0.to_le_bytes());
    }
    h
}

fn noise_params(terrain: &Terrain) -> NoiseParams {
    NoiseParams {
        seed: terrain.seed as u64,
        scale: terrain.noise_scale.max(0.01) as f64,
        height: terrain.height_scale,
        mountains: terrain.mountains.max(0.0),
        octaves: terrain.octaves.clamp(1, 10),
        land_bias: terrain.land_bias as f64,
        flat_zones: terrain.flat_zones.iter().map(|z| (z.x as f64, z.y as f64, z.z as f64)).collect(),
        flat_falloff: terrain.flat_falloff.max(0.0) as f64,
    }
}

/// Последние использованные общие части (по хешу формы), самые свежие — в конце.
static REGISTRY: Mutex<Vec<Arc<TerrainShared>>> = Mutex::new(Vec::new());
const REGISTRY_LIMIT: usize = 8;

/// Общая часть для компонента: из реестра или новая. Карта высот читается через
/// `assets`; без них (скрипт спросил высоту до первого кадра) — плоско, и при первом
/// же вызове с ассетами общая часть пересоздаётся.
pub(crate) fn shared_for(terrain: &Terrain, assets: Option<&AssetDatabase>) -> Arc<TerrainShared> {
    let layout = layout_of(terrain);
    let hash = shape_hash(terrain, &layout);
    let mut registry = REGISTRY.lock().expect("terrain registry lock");
    if let Some(index) = registry.iter().position(|s| s.hash == hash) {
        let found = registry.remove(index);
        if found.complete || assets.is_none() {
            registry.push(found.clone());
            return found;
        }
    }
    let mut heightmap = None;
    let mut complete = true;
    if terrain.source == TerrainSource::Heightmap && !terrain.heightmap.is_none() {
        match assets {
            None => complete = false,
            Some(assets) => {
                let guid = terrain.heightmap.guid();
                let name = assets.relative_path_of(guid).unwrap_or_default().to_string();
                match assets.read(guid).map_err(|e| e.to_string()).and_then(|b| Heightmap::decode(&b, &name)) {
                    Ok(map) => heightmap = Some(map),
                    Err(reason) => log::error!("Terrain heightmap: {reason}"),
                }
            }
        }
    }
    let shared = Arc::new(TerrainShared {
        hash,
        layout,
        generator: Generator::new(noise_params(terrain)),
        source: terrain.source,
        heightmap,
        heightmap_height: terrain.heightmap_height,
        complete,
        base: Mutex::new(HashMap::new()),
    });
    registry.push(shared.clone());
    if registry.len() > REGISTRY_LIMIT {
        registry.remove(0);
    }
    shared
}

// ---------------------------------------------------------------------------
// Правки кистью
// ---------------------------------------------------------------------------

/// Плитка правок TILE × TILE отсчётов.
#[derive(Clone, Debug, Default)]
pub(crate) struct EditTile {
    /// Добавка к высоте, м (пусто — высоту не трогали).
    pub delta: Vec<f32>,
    /// Нарисованные веса слоёв 0..255 (пусто — не рисовали).
    pub paint: Vec<[u8; LAYER_COUNT]>,
    /// Хеш содержимого: по нему рендер и физика узнают, что чанк пора перестроить.
    pub hash: u64,
}

impl EditTile {
    pub fn rehash(&mut self, key: (i32, i32)) {
        let mut h = FNV_START;
        hash_bytes(&mut h, &key.0.to_le_bytes());
        hash_bytes(&mut h, &key.1.to_le_bytes());
        for d in &self.delta {
            hash_bytes(&mut h, &d.to_bits().to_le_bytes());
        }
        for p in &self.paint {
            hash_bytes(&mut h, p);
        }
        self.hash = h;
    }
}

/// Все правки рельефа.
#[derive(Default)]
pub(crate) struct EditsData {
    /// Шаг сетки, на которой сделаны правки (0 — правок ещё нет).
    pub spacing: f32,
    pub tiles: BTreeMap<(i32, i32), Arc<EditTile>>,
    /// Закодированный вид для файла сцены (кэш; сбрасывается при правке).
    encoded: OnceLock<String>,
}

impl Clone for EditsData {
    fn clone(&self) -> Self {
        Self { spacing: self.spacing, tiles: self.tiles.clone(), encoded: OnceLock::new() }
    }
}

impl EditsData {
    fn tile_of(g: i32) -> (i32, i32) {
        (g.div_euclid(TILE), g.rem_euclid(TILE))
    }

    /// Правка высоты в отсчёте сетки с шагом `spacing`.
    pub fn delta(&self, gi: i32, gj: i32, spacing: f32) -> f32 {
        if self.tiles.is_empty() {
            return 0.0;
        }
        if (spacing - self.spacing).abs() <= 1e-6 * spacing.max(1.0) {
            return self.delta_exact(gi, gj);
        }
        // Шаг сетки сменили после правок: правки переносятся билинейно.
        let k = spacing / self.spacing.max(1e-6);
        let (x, z) = (gi as f32 * k, gj as f32 * k);
        let (x0, z0) = (x.floor() as i32, z.floor() as i32);
        let (fx, fz) = (x - x0 as f32, z - z0 as f32);
        let a = self.delta_exact(x0, z0);
        let b = self.delta_exact(x0 + 1, z0);
        let c = self.delta_exact(x0, z0 + 1);
        let d = self.delta_exact(x0 + 1, z0 + 1);
        let top = a + (b - a) * fx;
        let bottom = c + (d - c) * fx;
        top + (bottom - top) * fz
    }

    fn delta_exact(&self, gi: i32, gj: i32) -> f32 {
        let ((ti, li), (tj, lj)) = (Self::tile_of(gi), Self::tile_of(gj));
        match self.tiles.get(&(ti, tj)) {
            Some(tile) if !tile.delta.is_empty() => tile.delta[(lj * TILE + li) as usize],
            _ => 0.0,
        }
    }

    /// Нарисованные веса в отсчёте (ближайший отсчёт, если шаг сменили).
    pub fn paint(&self, gi: i32, gj: i32, spacing: f32) -> [u8; LAYER_COUNT] {
        if self.tiles.is_empty() {
            return [0; LAYER_COUNT];
        }
        let (gi, gj) = if (spacing - self.spacing).abs() <= 1e-6 * spacing.max(1.0) {
            (gi, gj)
        } else {
            let k = spacing / self.spacing.max(1e-6);
            ((gi as f32 * k).round() as i32, (gj as f32 * k).round() as i32)
        };
        let ((ti, li), (tj, lj)) = (Self::tile_of(gi), Self::tile_of(gj));
        match self.tiles.get(&(ti, tj)) {
            Some(tile) if !tile.paint.is_empty() => tile.paint[(lj * TILE + li) as usize],
            _ => [0; LAYER_COUNT],
        }
    }

    /// Отпечаток всех правок: меняется при любой правке.
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_START;
        for tile in self.tiles.values() {
            h = (h ^ tile.hash).wrapping_mul(0x100_0000_01b3);
        }
        h ^ self.tiles.len() as u64
    }

    /// Хеш правок в прямоугольнике отсчётов (включительно). 0 — правок там нет.
    pub fn region_hash(&self, gi0: i32, gj0: i32, gi1: i32, gj1: i32, spacing: f32) -> u64 {
        if self.tiles.is_empty() {
            return 0;
        }
        let k = spacing / self.spacing.max(1e-6);
        let to_edit = |g: i32, up: bool| {
            let v = g as f32 * k;
            (if up { v.ceil() + 1.0 } else { v.floor() - 1.0 }) as i32
        };
        let (ti0, ti1) = (to_edit(gi0, false).div_euclid(TILE), to_edit(gi1, true).div_euclid(TILE));
        let (tj0, tj1) = (to_edit(gj0, false).div_euclid(TILE), to_edit(gj1, true).div_euclid(TILE));
        let mut h = 0u64;
        for tj in tj0..=tj1 {
            for (_, tile) in self.tiles.range((ti0, tj)..=(ti1, tj)) {
                h = (h ^ tile.hash).wrapping_mul(0x100_0000_01b3).rotate_left(7);
            }
        }
        h
    }

    /// Плитка для правки (создаётся пустой). Сбрасывает закодированный вид.
    pub fn tile_mut(&mut self, key: (i32, i32)) -> &mut EditTile {
        self.encoded = OnceLock::new();
        Arc::make_mut(self.tiles.entry(key).or_default())
    }

    pub fn invalidate_encoding(&mut self) {
        self.encoded = OnceLock::new();
    }

    /// Привести правки к шагу `spacing` (перед новой правкой на сетке с другим шагом).
    pub fn rebase(&mut self, spacing: f32, layout: &TerrainLayout) {
        if self.tiles.is_empty() {
            self.spacing = spacing;
            return;
        }
        if (spacing - self.spacing).abs() <= 1e-6 * spacing.max(1.0) {
            return;
        }
        let old = self.clone();
        self.tiles.clear();
        self.spacing = spacing;
        self.encoded = OnceLock::new();
        let n = layout.samples();
        for gj in 0..=n {
            for gi in 0..=n {
                let d = old.delta(gi, gj, spacing);
                let p = old.paint(gi, gj, spacing);
                if d == 0.0 && p == [0; LAYER_COUNT] {
                    continue;
                }
                let ((ti, li), (tj, lj)) = (Self::tile_of(gi), Self::tile_of(gj));
                let tile = Arc::make_mut(self.tiles.entry((ti, tj)).or_default());
                let index = (lj * TILE + li) as usize;
                if d != 0.0 {
                    if tile.delta.is_empty() {
                        tile.delta = vec![0.0; (TILE * TILE) as usize];
                    }
                    tile.delta[index] = d;
                }
                if p != [0; LAYER_COUNT] {
                    if tile.paint.is_empty() {
                        tile.paint = vec![[0; LAYER_COUNT]; (TILE * TILE) as usize];
                    }
                    tile.paint[index] = p;
                }
            }
        }
        let keys: Vec<(i32, i32)> = self.tiles.keys().copied().collect();
        for key in keys {
            if let Some(tile) = self.tiles.get_mut(&key) {
                Arc::make_mut(tile).rehash(key);
            }
        }
    }

    // --- Кодирование для файла сцены ---

    /// Компактная строка: двоичный поток (миллиметры, нули сжаты серией) в base64.
    fn encode(&self) -> String {
        if self.tiles.is_empty() {
            return String::new();
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"PTE1");
        out.extend_from_slice(&self.spacing.to_le_bytes());
        out.extend_from_slice(&(self.tiles.len() as u32).to_le_bytes());
        for (&(ti, tj), tile) in &self.tiles {
            out.extend_from_slice(&ti.to_le_bytes());
            out.extend_from_slice(&tj.to_le_bytes());
            let flags = (!tile.delta.is_empty()) as u8 | ((!tile.paint.is_empty()) as u8) << 1;
            out.push(flags);
            if !tile.delta.is_empty() {
                let values = tile.delta.iter().map(|d| {
                    let mm = (d * 1000.0).round() as i64;
                    ((mm << 1) ^ (mm >> 63)) as u64 // zigzag
                });
                write_runs(&mut out, values);
            }
            if !tile.paint.is_empty() {
                let values = tile.paint.iter().flat_map(|p| p.iter().map(|&b| b as u64));
                write_runs(&mut out, values);
            }
        }
        base64_encode(&out)
    }

    fn decode(text: &str) -> Result<Self, String> {
        let mut data = Self::default();
        if text.trim().is_empty() {
            return Ok(data);
        }
        let bytes = base64_decode(text.trim()).ok_or("terrain edits: not base64")?;
        let mut r = Reader { bytes: &bytes, at: 0 };
        if r.take(4)? != b"PTE1" {
            return Err("terrain edits: unknown format".into());
        }
        data.spacing = f32::from_le_bytes(r.array()?);
        let count = u32::from_le_bytes(r.array()?);
        let cells = (TILE * TILE) as usize;
        for _ in 0..count {
            let ti = i32::from_le_bytes(r.array()?);
            let tj = i32::from_le_bytes(r.array()?);
            let flags = r.take(1)?[0];
            let mut tile = EditTile::default();
            if flags & 1 != 0 {
                tile.delta = read_runs(&mut r, cells)?
                    .into_iter()
                    .map(|z| {
                        let mm = ((z >> 1) as i64) ^ -((z & 1) as i64);
                        mm as f32 / 1000.0
                    })
                    .collect();
            }
            if flags & 2 != 0 {
                let flat = read_runs(&mut r, cells * LAYER_COUNT)?;
                tile.paint = flat
                    .chunks_exact(LAYER_COUNT)
                    .map(|c| std::array::from_fn(|k| c[k].min(255) as u8))
                    .collect();
            }
            tile.rehash((ti, tj));
            data.tiles.insert((ti, tj), Arc::new(tile));
        }
        Ok(data)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at + n;
        let slice = self.bytes.get(self.at..end).ok_or("terrain edits: data is cut short")?;
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let slice = self.take(N)?;
        Ok(std::array::from_fn(|i| slice[i]))
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.take(1)?[0];
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("terrain edits: bad number".into())
    }
}

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Значения подряд; серия нулей пишется как 0 и длина серии.
fn write_runs(out: &mut Vec<u8>, values: impl Iterator<Item = u64>) {
    let mut zeros = 0u64;
    for v in values {
        if v == 0 {
            zeros += 1;
            continue;
        }
        if zeros > 0 {
            write_varint(out, 0);
            write_varint(out, zeros);
            zeros = 0;
        }
        write_varint(out, v);
    }
    if zeros > 0 {
        write_varint(out, 0);
        write_varint(out, zeros);
    }
}

fn read_runs(r: &mut Reader, count: usize) -> Result<Vec<u64>, String> {
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        let v = r.varint()?;
        if v == 0 {
            let run = r.varint()? as usize;
            if values.len() + run > count {
                return Err("terrain edits: run overflows a tile".into());
            }
            values.resize(values.len() + run, 0);
        } else {
            values.push(v);
        }
    }
    Ok(values)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(BASE64[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = BASE64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Правки рельефа (скульпт и раскраска) — поле `Terrain::edits`. В файле сцены это
/// строка: плитки 64 × 64 отсчёта, высоты с точностью до миллиметра, нули сжаты.
/// Копирование дешёвое (общий указатель), правка копирует только при необходимости.
#[derive(Clone, Default)]
pub struct TerrainEdits(pub(crate) Arc<EditsData>);

impl TerrainEdits {
    /// Нет ни одной правки.
    pub fn is_empty(&self) -> bool {
        self.0.tiles.is_empty()
    }

    /// Сколько плиток 64 × 64 затронуто.
    pub fn tile_count(&self) -> usize {
        self.0.tiles.len()
    }

    pub(crate) fn data_mut(&mut self) -> &mut EditsData {
        let data = Arc::make_mut(&mut self.0);
        data.invalidate_encoding();
        data
    }

    /// Строка для файла сцены (то же, что пишет reflection).
    pub fn encode(&self) -> String {
        self.0.encoded.get_or_init(|| self.0.encode()).clone()
    }

    /// Прочитать строку из файла сцены.
    pub fn decode(text: &str) -> Result<Self, String> {
        let data = EditsData::decode(text)?;
        let _ = data.encoded.set(text.trim().to_string());
        Ok(Self(Arc::new(data)))
    }
}

impl std::fmt::Debug for TerrainEdits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TerrainEdits({} tiles)", self.0.tiles.len())
    }
}

impl FieldValue for TerrainEdits {
    const EXPECTED: &'static str = "Str (encoded terrain edits)";

    fn to_value(&self) -> Value {
        Value::Str(self.encode())
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(text) => match Self::decode(text) {
                Ok(edits) => Some(edits),
                Err(reason) => {
                    log::error!("Terrain: {reason}; the sculpting is dropped");
                    None
                }
            },
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Выборка итоговых высот
// ---------------------------------------------------------------------------

/// Итоговая поверхность рельефа (база + правки) в локальных координатах: x, z — метры
/// от угла рельефа, y — от позиции объекта. Дешёвая копия — для потоков и запросов.
#[derive(Clone)]
pub struct TerrainSampler {
    pub(crate) shared: Arc<TerrainShared>,
    pub(crate) edits: Arc<EditsData>,
}

impl TerrainSampler {
    pub fn layout(&self) -> TerrainLayout {
        self.shared.layout
    }

    /// Хеш параметров формы: другой — значит рельеф сгенерирован заново.
    pub fn shape_hash(&self) -> u64 {
        self.shared.hash
    }

    /// Высоты чанка в LOD 0: `(resolution + 1)²` отсчётов, строка = z (для коллайдера).
    pub fn chunk_heights(&self, ci: i32, cj: i32) -> Vec<f32> {
        let res = self.shared.layout.resolution;
        let mut out = Vec::new();
        self.fill(ci * res, cj * res, 1, (res + 1) as usize, (res + 1) as usize, &mut out);
        out
    }

    /// Высота отсчёта сетки (индексы зажимаются в пределы рельефа).
    pub fn height_at_sample(&self, gi: i32, gj: i32) -> f32 {
        let layout = &self.shared.layout;
        let (gi, gj) = (clamp_sample(layout, gi), clamp_sample(layout, gj));
        self.shared.base_sample(gi, gj) + self.edits.delta(gi, gj, layout.spacing())
    }

    /// Нарисованные веса слоёв в отсчёте.
    pub fn paint_at_sample(&self, gi: i32, gj: i32) -> [u8; LAYER_COUNT] {
        let layout = &self.shared.layout;
        let (gi, gj) = (clamp_sample(layout, gi), clamp_sample(layout, gj));
        self.edits.paint(gi, gj, layout.spacing())
    }

    /// Высота поверхности в точке (те же треугольники, что у меша LOD 0 и коллайдера).
    /// Вне рельефа — высота ближайшего края.
    pub fn height_local(&self, x: f32, z: f32) -> f32 {
        let layout = self.shared.layout;
        let sp = layout.spacing();
        let n = layout.samples() as f32;
        let gx = (x / sp).clamp(0.0, n);
        let gz = (z / sp).clamp(0.0, n);
        let (i0, j0) = ((gx.floor() as i32).min(layout.samples() - 1), (gz.floor() as i32).min(layout.samples() - 1));
        let (fx, fz) = (gx - i0 as f32, gz - j0 as f32);
        let h = |i: i32, j: i32| self.height_at_sample(i0 + i, j0 + j);
        if fx + fz <= 1.0 {
            let h00 = h(0, 0);
            h00 + fx * (h(1, 0) - h00) + fz * (h(0, 1) - h00)
        } else {
            let h11 = h(1, 1);
            h11 + (1.0 - fx) * (h(0, 1) - h11) + (1.0 - fz) * (h(1, 0) - h11)
        }
    }

    /// Нормаль поверхности (по центральным разностям через отсчёт).
    pub fn normal_local(&self, x: f32, z: f32) -> Vec3 {
        let e = self.shared.layout.spacing();
        let dx = self.height_local(x + e, z) - self.height_local(x - e, z);
        let dz = self.height_local(x, z + e) - self.height_local(x, z - e);
        Vec3::new(-dx, 2.0 * e, -dz).normalize_or(Vec3::Y)
    }

    /// Высоты прямоугольника отсчётов: `w × h` штук, начиная с (gi0, gj0), с шагом
    /// `step` отсчётов; строка = z. Отсчёты вне рельефа зажимаются к краю.
    pub fn fill(&self, gi0: i32, gj0: i32, step: i32, w: usize, h: usize, out: &mut Vec<f32>) {
        let layout = self.shared.layout;
        let sp = layout.spacing();
        let stride = (layout.resolution + 1) as usize;
        out.clear();
        out.reserve(w * h);
        let mut current: Option<(ChunkKey, Arc<[f32]>)> = None;
        for j in 0..h {
            let gj = clamp_sample(&layout, gj0 + j as i32 * step);
            let (cj, lj) = chunk_of(&layout, gj);
            for i in 0..w {
                let gi = clamp_sample(&layout, gi0 + i as i32 * step);
                let (ci, li) = chunk_of(&layout, gi);
                let chunk = match &current {
                    Some((key, chunk)) if *key == (ci, cj) => chunk.clone(),
                    _ => {
                        let chunk = self.shared.base_chunk(ci, cj);
                        current = Some(((ci, cj), chunk.clone()));
                        chunk
                    }
                };
                let base = chunk[lj as usize * stride + li as usize];
                out.push(base + self.edits.delta(gi, gj, sp));
            }
        }
    }

    /// Луч в локальных координатах: расстояние до поверхности. Шаг — полклетки,
    /// затем уточнение делением пополам.
    pub fn raycast_local(&self, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<f32> {
        let direction = direction.try_normalize()?;
        let size = self.shared.layout.size();
        // Отрезок луча над прямоугольником рельефа.
        let (mut t0, mut t1) = (0.0_f32, max_distance);
        for (o, d) in [(origin.x, direction.x), (origin.z, direction.z)] {
            if d.abs() < 1e-9 {
                if o < 0.0 || o > size {
                    return None;
                }
            } else {
                let (a, b) = ((0.0 - o) / d, (size - o) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t0 > t1 {
            return None;
        }
        let step = self.shared.layout.spacing() * 0.5;
        let above = |t: f32| {
            let p = origin + direction * t;
            p.y - self.height_local(p.x, p.z)
        };
        let mut previous_t = t0;
        if above(t0) < 0.0 {
            return Some(t0); // начало луча уже под землёй
        }
        let mut t = t0;
        while t < t1 {
            t = (t + step).min(t1);
            if above(t) <= 0.0 {
                let (mut lo, mut hi) = (previous_t, t);
                for _ in 0..24 {
                    let mid = (lo + hi) * 0.5;
                    if above(mid) > 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                return Some(hi);
            }
            previous_t = t;
            if t >= t1 {
                break;
            }
        }
        None
    }

    /// Хеш правок, влияющих на чанк (с запасом `margin` отсчётов вокруг) — подпись для
    /// перестройки меша и коллайдера.
    pub fn chunk_signature(&self, ci: i32, cj: i32, margin: i32) -> u64 {
        let layout = self.shared.layout;
        let res = layout.resolution;
        let base = self.shared.hash;
        let edits = self.edits.region_hash(
            ci * res - margin,
            cj * res - margin,
            (ci + 1) * res + margin,
            (cj + 1) * res + margin,
            layout.spacing(),
        );
        base ^ edits.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_round_trip_through_text() {
        let mut edits = TerrainEdits::default();
        {
            let data = edits.data_mut();
            data.spacing = 1.0;
            let tile = data.tile_mut((1, -2));
            tile.delta = vec![0.0; (TILE * TILE) as usize];
            tile.delta[5] = 1.234;
            tile.delta[4000] = -7.5;
            tile.paint = vec![[0; LAYER_COUNT]; (TILE * TILE) as usize];
            tile.paint[17] = [0, 200, 0, 55, 0];
            tile.rehash((1, -2));
        }
        let text = edits.encode();
        assert!(text.len() < 200, "sparse edits stay small: {} chars", text.len());
        let back = TerrainEdits::decode(&text).unwrap();
        let tile = &back.0.tiles[&(1, -2)];
        assert!((tile.delta[5] - 1.234).abs() < 1e-6);
        assert!((tile.delta[4000] + 7.5).abs() < 1e-6);
        assert_eq!(tile.paint[17], [0, 200, 0, 55, 0]);
        assert_eq!(back.encode(), text);
        assert!(TerrainEdits::decode("@@@").is_err());
    }

    #[test]
    fn heightmaps_decode_16_bit_png_and_raw() {
        // 16-битный PNG 3 × 2: x растёт слева направо, z — сверху вниз.
        let image = image::ImageBuffer::<image::Luma<u16>, _>::from_fn(3, 2, |x, y| image::Luma([(x * 30000 + y * 5000) as u16]));
        let mut png = Vec::new();
        image::DynamicImage::ImageLuma16(image)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let map = Heightmap::decode(&png, "island.png").unwrap();
        assert_eq!((map.width, map.height), (3, 2));
        assert!((map.sample(1.0, 0.0) - 60000.0 / 65535.0).abs() < 1e-6);
        assert!((map.sample(0.25, 1.0) - (15000.0 + 5000.0) / 65535.0).abs() < 1e-6, "bilinear between pixels");
        // Сырой квадрат 2 × 2 little-endian.
        let raw: Vec<u8> = [0u16, 65535, 32768, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let map = Heightmap::decode(&raw, "h.r16").unwrap();
        assert!((map.sample(1.0, 0.0) - 1.0).abs() < 1e-6);
        assert!(Heightmap::decode(&raw[..6], "bad.raw").is_err());
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
    }
}
