//! Склейка 2D-заходов в батчи с перестановкой непересекающихся спрайтов.
//!
//! Порядок отрисовки задаёт сортировка (слой → порядок → глубина, при равенстве —
//! порядок иерархии). Если подряд идут спрайты с разными текстурами (A B A B …),
//! простая склейка соседей даёт по draw call'у на спрайт. Но два спрайта, которые
//! не пересекаются на экране, можно рисовать в любом порядке — картинка не меняется.
//! Поэтому заход переносится в более ранний батч со своей текстурой, если ни один
//! заход из батчей после него не пересекается с ним на экране. Через границы
//! слоя / порядка / глубины заходы не переносятся никогда.

use engine_core::prelude::*;

use crate::draw2d::{Blend, DrawRun, TexRef};

/// Прямоугольник захода в координатах экрана (NDC, −1..1); `None` — неизвестно
/// (часть за камерой) или слишком велик: такой заход «пересекается со всем».
pub(crate) type ScreenRect = Option<[f32; 4]>;

/// Прямоугольник на экране по мировым границам `min..max`.
pub(crate) fn screen_rect(view_proj: Mat4, min: Vec3, max: Vec3) -> ScreenRect {
    // Ортографическая камера (2D): проекция аффинная, прямоугольник — центр ± полуразмеры
    // через модуль матрицы, одним преобразованием вместо четырёх-восьми углов с делением.
    let affine = view_proj.x_axis.w == 0.0 && view_proj.y_axis.w == 0.0 && view_proj.z_axis.w == 0.0 && view_proj.w_axis.w == 1.0;
    if affine {
        let center = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        let c = view_proj * center.extend(1.0);
        let ex = view_proj.x_axis.x.abs() * half.x + view_proj.y_axis.x.abs() * half.y + view_proj.z_axis.x.abs() * half.z;
        let ey = view_proj.x_axis.y.abs() * half.x + view_proj.y_axis.y.abs() * half.y + view_proj.z_axis.y.abs() * half.z;
        let rect = [c.x - ex, c.y - ey, c.x + ex, c.y + ey];
        return rect.iter().all(|v| v.is_finite()).then_some(rect);
    }
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    let flat = min.z == max.z;
    for corner in 0..if flat { 4 } else { 8 } {
        let p = Vec3::new(
            if corner & 1 == 0 { min.x } else { max.x },
            if corner & 2 == 0 { min.y } else { max.y },
            if corner & 4 == 0 { min.z } else { max.z },
        );
        let clip = view_proj * p.extend(1.0);
        if clip.w <= 1e-6 || !clip.is_finite() {
            return None;
        }
        let ndc = clip.truncate().truncate() / clip.w;
        lo = lo.min(ndc);
        hi = hi.max(ndc);
    }
    Some([lo.x, lo.y, hi.x, hi.y])
}

fn overlaps(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Батч: текстура, режим смешивания и диапазон вершин (заполняется в `build`).
pub(crate) type Batch = (TexRef, Blend, std::ops::Range<u32>);

/// Заходы `a` и `b` в одной группе: тот же слой, порядок и глубина. Между группами
/// заходы не переставляются.
fn same_group(a: &DrawRun, b: &DrawRun) -> bool {
    a.sort_key == b.sort_key && a.depth.to_bits() == b.depth.to_bits()
}

/// Нужны ли прямоугольники заходов на экране: только если внутри одной группы
/// встречаются разные текстуры или режимы смешивания. Иначе каждая группа — один ключ,
/// и склейка не переставляет заходы.
pub(crate) fn needs_rects(runs: &[DrawRun]) -> bool {
    runs.windows(2)
        .any(|w| same_group(&w[0], &w[1]) && (w[0].texture, w[0].blend) != (w[1].texture, w[1].blend))
}

/// Сколько последних батчей группы просматривать в поисках своей текстуры.
const SEARCH_LIMIT: usize = 64;
/// Заход, покрывающий больше стольких ячеек сетки, проверяется отдельным списком.
const BIG_CELLS: usize = 64;

/// Рабочие буферы склейки — переиспользуются от кадра к кадру.
#[derive(Default)]
pub(crate) struct Batcher {
    /// Номер батча для каждого захода.
    assigned: Vec<u32>,
    /// Ключ каждого батча (текстура, смешивание).
    keys: Vec<(TexRef, Blend)>,
    /// Сетка ячеек экрана: голова списка заходов (индекс узла + 1, 0 — пусто) и метка группы.
    cells: Vec<(u32, u32)>,
    /// Узлы списков: (номер захода, следующий узел + 1).
    nodes: Vec<(u32, u32)>,
    /// Большие заходы текущей группы.
    big: Vec<u32>,
    /// Порядок заходов по батчам (сортировка подсчётом).
    order: Vec<u32>,
    counts: Vec<u32>,
    /// Метка группы: растёт и между кадрами, поэтому старые ячейки сетки не путаются с новыми.
    generation: u32,
}

impl Batcher {
    /// Разложить отсортированные `runs` по батчам. `rects[i]` — прямоугольник захода `i`
    /// на экране (нужен, только если в кадре больше одной текстуры). Возвращает порядок
    /// заходов для отрисовки; `batches` получает по батчу на каждую смену текстуры.
    pub fn build(&mut self, runs: &[DrawRun], rects: &[ScreenRect], batches: &mut Vec<Batch>) -> &[u32] {
        self.assigned.clear();
        self.keys.clear();
        batches.clear();
        let mixed = needs_rects(runs);

        let mut group_start = 0usize; // первый батч текущей группы
        let mut group_id = self.generation;
        let mut side = 1usize;
        for (index, run) in runs.iter().enumerate() {
            let key = (run.texture, run.blend);
            let new_group = index == 0 || !same_group(&runs[index - 1], run);
            if new_group {
                group_start = self.keys.len();
                if self.generation == u32::MAX {
                    // Метки кончились: сбрасываем сетку (раз в четыре миллиарда групп).
                    self.generation = 0;
                    self.cells.iter_mut().for_each(|cell| *cell = (0, 0));
                }
                self.generation += 1;
                group_id = self.generation;
                self.big.clear();
                if mixed {
                    // Сетка примерно по ячейке на заход группы: в ячейке мало заходов.
                    let group_len = runs[index..]
                        .iter()
                        .take_while(|r| r.sort_key == run.sort_key && r.depth.to_bits() == run.depth.to_bits())
                        .count();
                    side = ((group_len as f32).sqrt().ceil() as usize).clamp(4, 256);
                    if self.cells.len() < side * side {
                        self.cells.resize(side * side, (0, 0));
                    }
                    self.nodes.clear();
                }
            }

            // Последний батч — с той же текстурой: просто продолжаем его. Это верно и
            // на границе групп: заход всё равно рисуется после всего, что было до него
            // (так соседние группы с одной текстурой — один draw call).
            let target = if self.keys.last() == Some(&key) {
                Some(self.keys.len() - 1)
            } else if mixed {
                self.find_earlier(index, key, group_start, group_id, side, rects)
            } else {
                None
            };
            let batch = match target {
                Some(batch) => batch,
                None => {
                    self.keys.push(key);
                    self.keys.len() - 1
                }
            };
            self.assigned.push(batch as u32);
            if mixed {
                self.insert(index, group_id, side, rects);
            }
        }

        // Порядок отрисовки: по батчам, внутри батча — в исходном порядке.
        self.counts.clear();
        self.counts.resize(self.keys.len() + 1, 0);
        for &batch in &self.assigned {
            self.counts[batch as usize + 1] += 1;
        }
        for i in 1..self.counts.len() {
            self.counts[i] += self.counts[i - 1];
        }
        self.order.clear();
        self.order.resize(runs.len(), 0);
        for (index, &batch) in self.assigned.iter().enumerate() {
            let slot = &mut self.counts[batch as usize];
            self.order[*slot as usize] = index as u32;
            *slot += 1;
        }
        let mut vertex = 0u32;
        let mut current = u32::MAX;
        for &index in &self.order {
            let run = &runs[index as usize];
            let batch = self.assigned[index as usize];
            if batch != current {
                current = batch;
                batches.push((run.texture, run.blend, vertex..vertex));
            }
            vertex += run.len;
            if let Some(last) = batches.last_mut() {
                last.2.end = vertex;
            }
        }
        &self.order
    }

    /// Ячейки сетки, которые накрывает прямоугольник (`None` — слишком много).
    fn cell_range(rect: &[f32; 4], side: usize) -> Option<(usize, usize, usize, usize)> {
        let to_cell = |v: f32| (((v + 1.0) * 0.5 * side as f32).floor().max(0.0) as usize).min(side - 1);
        let (x0, y0, x1, y1) = (to_cell(rect[0]), to_cell(rect[1]), to_cell(rect[2]), to_cell(rect[3]));
        ((x1 - x0 + 1) * (y1 - y0 + 1) <= BIG_CELLS).then_some((x0, y0, x1, y1))
    }

    /// Более ранний батч группы с ключом `key`, в который можно перенести заход `index`.
    fn find_earlier(
        &self,
        index: usize,
        key: (TexRef, Blend),
        group_start: usize,
        group_id: u32,
        side: usize,
        rects: &[ScreenRect],
    ) -> Option<usize> {
        let first = group_start.max(self.keys.len().saturating_sub(SEARCH_LIMIT));
        let candidate = (first..self.keys.len()).rev().find(|&b| self.keys[b] == key)?;
        let rect = rects.get(index).copied().flatten()?;
        let blocks = |other: u32| {
            self.assigned[other as usize] as usize > candidate
                && rects[other as usize].is_none_or(|r| overlaps(&r, &rect))
        };
        if self.big.iter().any(|&other| blocks(other)) {
            return None;
        }
        let (x0, y0, x1, y1) = Self::cell_range(&rect, side)?;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (head, stamp) = self.cells[y * side + x];
                if stamp != group_id {
                    continue;
                }
                let mut node = head;
                while node != 0 {
                    let (other, next) = self.nodes[node as usize - 1];
                    if blocks(other) {
                        return None;
                    }
                    node = next;
                }
            }
        }
        Some(candidate)
    }

    /// Запомнить заход `index` в ячейках, которые он накрывает.
    fn insert(&mut self, index: usize, group_id: u32, side: usize, rects: &[ScreenRect]) {
        let range = rects.get(index).copied().flatten().and_then(|rect| Self::cell_range(&rect, side));
        let Some((x0, y0, x1, y1)) = range else {
            self.big.push(index as u32);
            return;
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let cell = &mut self.cells[y * side + x];
                let head = if cell.1 == group_id { cell.0 } else { 0 };
                self.nodes.push((index as u32, head));
                *cell = (self.nodes.len() as u32, group_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_assets::Guid;

    fn run(texture: u128, order: i32, start: u32) -> DrawRun {
        DrawRun {
            sort_key: (0, order),
            depth: 0.0,
            texture: TexRef::Asset(Guid(texture)),
            blend: Blend::Alpha,
            start,
            len: 6,
        }
    }

    fn rect(x: f32) -> ScreenRect {
        Some([x, 0.0, x + 0.05, 0.05])
    }

    #[test]
    fn disjoint_sprites_with_alternating_textures_merge() {
        let runs: Vec<_> = (0..8).map(|i| run(i as u128 % 2, 0, i * 6)).collect();
        let rects: Vec<_> = (0..8).map(|i| rect(-0.9 + i as f32 * 0.2)).collect();
        let mut batcher = Batcher::default();
        let mut batches = Vec::new();
        let order = batcher.build(&runs, &rects, &mut batches).to_vec();
        assert_eq!(batches.len(), 2);
        assert_eq!(order, vec![0, 2, 4, 6, 1, 3, 5, 7]);
        assert_eq!(batches[0].2, 0..24);
        assert_eq!(batches[1].2, 24..48);
    }

    #[test]
    fn overlapping_sprites_keep_their_order() {
        // A, B, A — все в одной точке: третий нельзя перенести в первый батч.
        let runs = vec![run(1, 0, 0), run(2, 0, 6), run(1, 0, 12)];
        let rects = vec![rect(0.0), rect(0.01), rect(0.02)];
        let mut batcher = Batcher::default();
        let mut batches = Vec::new();
        let order = batcher.build(&runs, &rects, &mut batches).to_vec();
        assert_eq!(batches.len(), 3);
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn sorting_groups_are_never_crossed() {
        let runs = vec![run(1, 0, 0), run(2, 1, 6), run(1, 2, 12)];
        let rects = vec![rect(-0.9), rect(0.0), rect(0.9)];
        let mut batcher = Batcher::default();
        let mut batches = Vec::new();
        let order = batcher.build(&runs, &rects, &mut batches).to_vec();
        assert_eq!(order, vec![0, 1, 2]);
        assert_eq!(batches.len(), 3);
    }

    #[test]
    fn neighbouring_groups_with_one_texture_share_a_batch() {
        // Как частицы в `stress`: группы 5, 8, 10; у 8 и 10 одна текстура — один батч.
        let runs = vec![run(1, 5, 0), run(2, 8, 6), run(2, 10, 12), run(2, 10, 18)];
        assert!(!needs_rects(&runs), "every group has one texture: no screen rects needed");
        let mut batcher = Batcher::default();
        let mut batches = Vec::new();
        let order = batcher.build(&runs, &[], &mut batches).to_vec();
        assert_eq!(order, vec![0, 1, 2, 3]);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].2, 6..24);
    }

    #[test]
    fn unknown_bounds_block_reordering() {
        let runs = vec![run(1, 0, 0), run(2, 0, 6), run(1, 0, 12)];
        let rects = vec![rect(-0.9), None, rect(0.9)];
        let mut batcher = Batcher::default();
        let mut batches = Vec::new();
        assert_eq!(batcher.build(&runs, &rects, &mut batches).to_vec(), vec![0, 1, 2]);
        assert_eq!(batches.len(), 3);
    }
}
