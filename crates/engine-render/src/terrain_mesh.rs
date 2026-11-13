//! Меш чанка рельефа (как в образце): сетка вершин уровня детализации, «юбки» по
//! краям, закрывающие щели между соседями разного LOD, запечённое затенение впадин
//! (AO) и нормали по высотам с рамкой — свет непрерывен через границы чанков.
//! Порядок вершин зависит только от LOD, поэтому индексы общие.
//!
//! Сверх образца: край, за которым сосед грубее, «пришивается» к его краю — нечётные
//! вершины края ложатся на прямую между чётными, ровно как у соседа. Шов совпадает
//! точно, юбки остаются страховкой на время, пока сосед ещё не перестроен.

use bytemuck::{Pod, Zeroable};

use crate::terrain_data::{TerrainSampler, LAYER_COUNT};

/// Вершина рельефа — 24 байта.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct TerrainVertex {
    /// Локальная позиция: x, z — от угла рельефа, y — высота.
    pub position: [f32; 3],
    /// snorm8: нормаль в xyz, разнообразие (0..1 → -1..1) в w.
    pub normal_var: [i8; 4],
    /// unorm8: x — затенение впадин (AO), y — нарисованный снег, z — насколько опустить
    /// вершину-«юбку» (в полуметрах; материал берётся по высоте края, а не юбки),
    /// w — крутизна для выбора материала (по самому крутому из односторонних уклонов:
    /// на остром гребне центральная разность даёт «плоско», и гребень покрылся бы песком).
    pub ao_snow: [u8; 4],
    /// unorm8: нарисованные трава, земля, скала, песок.
    pub paint: [u8; 4],
}

/// Радиус поиска для запечённого AO, м.
const AO_MAX_RADIUS: f32 = 16.0;
/// Рамка вокруг чанка (в шагах LOD) не больше стольких отсчётов.
const MAX_BORDER: i32 = 24;

#[inline]
fn snorm8(v: f32) -> i8 {
    (v.clamp(-1.0, 1.0) * 127.0).round() as i8
}

/// Клеток на сторону чанка на уровне `lod`.
pub(crate) fn lod_cells(resolution: i32, lod: u32) -> i32 {
    (resolution >> lod).max(1)
}

pub(crate) struct ChunkMesh {
    pub key: (i32, i32),
    pub lod: u32,
    pub signature: u64,
    pub vertices: Vec<TerrainVertex>,
    pub min_y: f32,
    pub max_y: f32,
}

/// Номер k-й вершины вдоль края `e` (0 — −z, 1 — +z, 2 — −x, 3 — +x).
fn edge_index(cells: usize, e: usize, k: usize) -> usize {
    let w = cells + 1;
    match e {
        0 => k,
        1 => cells * w + k,
        2 => k * w,
        _ => k * w + cells,
    }
}

/// Вершин в чанке уровня с `cells` клетками: сетка и четыре юбки.
pub(crate) fn vertex_count(cells: i32) -> usize {
    let w = (cells + 1) as usize;
    w * w + 4 * w
}

/// Построить меш чанка `key` на уровне `lod`. `coarser[e]` — сосед за краем `e`
/// (0 — −z, 1 — +z, 2 — −x, 3 — +x) на более грубом уровне: край пришивается к нему.
pub(crate) fn build_chunk(sampler: &TerrainSampler, key: (i32, i32), lod: u32, signature: u64, coarser: [bool; 4]) -> ChunkMesh {
    let layout = sampler.layout();
    let res = layout.resolution;
    let cells = lod_cells(res, lod);
    let step = res / cells; // шаг LOD в отсчётах LOD 0
    let sp = layout.spacing();
    let step_m = sp * step as f32;
    let (gi0, gj0) = (key.0 * res, key.1 * res);

    // Высоты с рамкой: для нормалей и AO на краю нужны соседи.
    let border = ((AO_MAX_RADIUS / step_m).ceil() as i32).clamp(1, MAX_BORDER);
    let n = (cells + 1 + 2 * border) as usize;
    let mut heights = Vec::new();
    sampler.fill(gi0 - border * step, gj0 - border * step, step, n, n, &mut heights);
    let b = border as isize;
    let hb = |i: isize, j: isize| heights[((j + b) as usize) * n + (i + b) as usize];

    let ao_at = |i: isize, j: isize, y: f32| -> u8 {
        let mut occ = 0.0f32;
        for radius in [4.0, 8.0, AO_MAX_RADIUS] {
            let r = ((radius / step_m).round() as isize).clamp(1, b);
            let dist = r as f32 * step_m;
            let mut sum = 0.0;
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
                sum += hb(i + dx * r, j + dz * r);
            }
            occ += ((sum / 8.0 - y) / dist).max(0.0);
        }
        let ao = 1.0 - (occ * 0.45).clamp(0.0, 0.75);
        (ao * 255.0 + 0.5) as u8
    };

    let w = (cells + 1) as usize;
    let mut vertices = Vec::with_capacity(vertex_count(cells));
    let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
    let two_step = 2.0 * step_m;
    let generator = &sampler.shared.generator;
    for j in 0..w {
        for i in 0..w {
            let (ii, jj) = (i as isize, j as isize);
            let y = hb(ii, jj);
            let dx = hb(ii + 1, jj) - hb(ii - 1, jj);
            let dz = hb(ii, jj + 1) - hb(ii, jj - 1);
            let len = (dx * dx + two_step * two_step + dz * dz).sqrt();
            let (gi, gj) = (gi0 + i as i32 * step, gj0 + j as i32 * step);
            let (x, z) = (gi as f32 * sp, gj as f32 * sp);
            let paint: [u8; LAYER_COUNT] = sampler.paint_at_sample(gi, gj);
            let rise = [hb(ii + 1, jj) - y, y - hb(ii - 1, jj), hb(ii, jj + 1) - y, y - hb(ii, jj - 1)]
                .into_iter()
                .fold(0.0f32, |m, d| m.max(d.abs()))
                / step_m;
            let steep = 1.0 - 1.0 / (1.0 + rise * rise).sqrt();
            vertices.push(TerrainVertex {
                position: [x, y, z],
                normal_var: [
                    snorm8(-dx / len),
                    snorm8(two_step / len),
                    snorm8(-dz / len),
                    snorm8(generator.variation(x as f64, z as f64) * 2.0 - 1.0),
                ],
                ao_snow: [ao_at(ii, jj, y), paint[4], 0, (steep * 255.0).round() as u8],
                paint: [paint[0], paint[1], paint[2], paint[3]],
            });
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    // Пришить края к грубым соседям: нечётная вершина — середина соседних чётных.
    let cells_u = cells as usize;
    for (e, &stitch) in coarser.iter().enumerate() {
        if !stitch || cells_u < 2 {
            continue;
        }
        for k in (1..cells_u).step_by(2) {
            let a = vertices[edge_index(cells_u, e, k - 1)].position[1];
            let b = vertices[edge_index(cells_u, e, k + 1)].position[1];
            vertices[edge_index(cells_u, e, k)].position[1] = (a + b) * 0.5;
        }
    }

    let skirt = (step_m * 2.0).clamp(4.0, 127.0);
    let skirt_code = (skirt * 2.0).round() as u8;
    let skirt = skirt_code as f32 * 0.5;
    for e in 0..4 {
        for k in 0..w {
            let mut v = vertices[edge_index(cells as usize, e, k)];
            v.ao_snow[2] = skirt_code;
            vertices.push(v);
        }
    }
    min_y -= skirt;

    ChunkMesh { key, lod, signature, vertices, min_y, max_y }
}

/// Индексы уровня с `cells` клетками (u16: не больше 129² + 4·129 вершин).
pub(crate) fn build_indices(cells: i32) -> Vec<u16> {
    let cells = cells as usize;
    let w = cells + 1;
    assert!(vertex_count(cells as i32) <= u16::MAX as usize, "terrain chunk is too dense for 16-bit indices");
    let mut idx = Vec::with_capacity(cells * cells * 6 + 4 * cells * 6);
    for j in 0..cells {
        for i in 0..cells {
            let a = (j * w + i) as u16;
            let b = a + 1;
            let c = a + w as u16;
            let d = c + 1;
            // Диагональ b–c: как у HeightField в Rapier (см. terrain_data.rs).
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let base = w * w;
    for e in 0..4 {
        for k in 0..cells {
            let g0 = edge_index(cells, e, k) as u16;
            let g1 = edge_index(cells, e, k + 1) as u16;
            let s0 = (base + e * w + k) as u16;
            let s1 = s0 + 1;
            // Юбка видна с обеих сторон — отсечение граней у рельефа выключено.
            idx.extend_from_slice(&[g0, s0, g1, g1, s0, s1]);
        }
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::Terrain;

    #[test]
    fn lod0_vertices_lie_on_the_sampled_surface() {
        let terrain = Terrain { size: 128.0, seed: 4, ..Terrain::default() };
        let sampler = terrain.sampler(None);
        let mesh = build_chunk(&sampler, (1, 0), 0, 0, [false; 4]);
        let cells = lod_cells(64, 0) as usize;
        assert_eq!(mesh.vertices.len(), vertex_count(64));
        for v in mesh.vertices.iter().take((cells + 1) * (cells + 1)).step_by(97) {
            let [x, y, z] = v.position;
            assert!((sampler.height_local(x, z) - y).abs() < 1e-4);
        }
        let indices = build_indices(lod_cells(64, 2));
        assert!(indices.iter().all(|&i| (i as usize) < vertex_count(lod_cells(64, 2))));
    }
}
