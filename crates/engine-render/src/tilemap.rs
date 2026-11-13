//! Карта тайлов: сетка индексов поверх тайлсета (сетка одинаковых ячеек в одной
//! текстуре). Рисуется одним заходом, только видимые тайлы; твёрдые тайлы по
//! желанию превращаются в статические `Collider2D` — дети объекта, слитые в как
//! можно меньшее число прямоугольников.
//!
//! # Запись карты
//!
//! `rows` — строки сверху вниз, как на экране; в строке тайлы через пробел:
//!
//! ```text
//! rows: [
//!     ". . . . . . . .",
//!     ". . 5 . . 6 . .",
//!     "1 1 1 1 1 1 1 1",
//!     "2 2 2 2 2 2 2 2",
//! ],
//! ```
//!
//! `.` или `0` — пусто; `n` — ячейка `n − 1` тайлсета (по строкам слева направо,
//! сверху вниз; как в Tiled); `12*3` — «двенадцать раз тайл 3», для длинных
//! повторов. Начало координат объекта — левый нижний угол карты.

use std::cell::{Ref, RefCell};

use engine_assets::{AssetHandle, Texture};
use engine_core::prelude::*;

use crate::draw2d::{push_quad, SpriteVertex, UvInset, UvRect};

reflect_enum! {
    /// Какие тайлы становятся коллайдерами.
    /// `None` — никакие, `NonEmpty` — все непустые, `Listed` — только из `solid_tiles`.
    pub enum TileCollision { None, NonEmpty, Listed }
}

/// Прямоугольник из тайлов: `x`, `y` — левый верхний тайл (строка 0 — верхняя).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Разобранная карта: индексы тайлов построчно, сверху вниз.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileGrid {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<u32>,
}

impl TileGrid {
    /// Индекс тайла (0 — пусто). За пределами карты — 0.
    pub fn get(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.tiles[y * self.width + x]
        } else {
            0
        }
    }
}

/// Кэш разбора + след от последней сборки коллайдеров.
#[derive(Clone, Debug, Default)]
pub struct TilemapRuntime {
    hash: u64,
    valid: bool,
    grid: TileGrid,
    collider_signature: u64,
    /// Предупреждения о незнакомых словах в `rows` (по одному на разбор).
    problems: Vec<String>,
    /// Копия `rows`, по которой собран `grid`. Сравнение с ней — `memcmp` строк, оно
    /// в разы быстрее побайтового хеша всей карты, который раньше считался каждый кадр.
    rows: Vec<String>,
}

/// FNV-1a по строкам.
fn hash_rows(rows: &[String]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for row in rows {
        for byte in row.bytes().chain(std::iter::once(b'\n')) {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn parse_rows(rows: &[String]) -> (TileGrid, Vec<String>) {
    let mut problems = Vec::new();
    let mut parsed: Vec<Vec<u32>> = Vec::with_capacity(rows.len());
    for (line, row) in rows.iter().enumerate() {
        let mut tiles = Vec::new();
        for token in row.split_whitespace() {
            let (count, word) = match token.split_once('*') {
                Some((n, word)) => match n.parse::<usize>() {
                    Ok(n) => (n.min(1 << 16), word),
                    Err(_) => {
                        problems.push(format!("row {line}: `{token}` — repeat count must be a number, like `12*3`"));
                        (1, "0")
                    }
                },
                None => (1, token),
            };
            let tile = match word {
                "." | "-" | "_" => 0,
                _ => word.parse::<u32>().unwrap_or_else(|_| {
                    problems.push(format!("row {line}: `{word}` is not a tile number; use `.` for empty"));
                    0
                }),
            };
            tiles.extend(std::iter::repeat_n(tile, count));
        }
        parsed.push(tiles);
    }
    let width = parsed.iter().map(Vec::len).max().unwrap_or(0);
    let height = parsed.len();
    let mut tiles = Vec::with_capacity(width * height);
    for mut row in parsed {
        row.resize(width, 0);
        tiles.extend(row);
    }
    (TileGrid { width, height, tiles }, problems)
}

fn format_row(tiles: &[u32]) -> String {
    tiles
        .iter()
        .map(|t| if *t == 0 { ".".to_string() } else { t.to_string() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Слить помеченные ячейки в прямоугольники: сначала горизонтальные отрезки,
/// затем одинаковые по ширине отрезки в соседних строках склеиваются вниз.
fn merge_mask(width: usize, height: usize, mask: &[bool]) -> Vec<TileRect> {
    let mut used = vec![false; mask.len()];
    let mut rects = Vec::new();
    for y in 0..height {
        let mut x = 0;
        while x < width {
            if !mask[y * width + x] || used[y * width + x] {
                x += 1;
                continue;
            }
            let mut run = 1;
            while x + run < width && mask[y * width + x + run] && !used[y * width + x + run] {
                run += 1;
            }
            let mut rows = 1;
            'down: while y + rows < height {
                for dx in 0..run {
                    let i = (y + rows) * width + x + dx;
                    if !mask[i] || used[i] {
                        break 'down;
                    }
                }
                rows += 1;
            }
            for dy in 0..rows {
                for dx in 0..run {
                    used[(y + dy) * width + x + dx] = true;
                }
            }
            rects.push(TileRect {
                x,
                y,
                width: run,
                height: rows,
            });
            x += run;
        }
    }
    rects
}

/// Имя объектов-коллайдеров, которые тайлмап создаёт сам.
pub const TILE_COLLIDER_NAME: &str = "TileCollider";

component_rt! {
    /// Карта тайлов. Объект стоит в левом нижнем углу карты; строки `rows` идут
    /// сверху вниз, тайлы в строке — через пробел (см. документацию модуля).
    pub struct Tilemap {
        /// Картинка-тайлсет: сетка одинаковых ячеек.
        pub tileset: AssetHandle<Texture> = AssetHandle::none(),
        /// Столбцов в тайлсете.
        pub tileset_columns: u32 = 1,
        /// Строк в тайлсете.
        pub tileset_rows: u32 = 1,
        /// Размер одного тайла в мировых единицах.
        pub tile_size: Vec2 = Vec2::ONE,
        /// Карта: по строке на ряд тайлов, сверху вниз. `.` — пусто, `n` — тайл n,
        /// `12*3` — двенадцать тайлов 3 подряд.
        pub rows: Vec<String> = Vec::new(),
        pub color: Color = Color::WHITE,
        /// Слой сортировки: карты фона, земли и переднего плана кладутся в разные слои.
        pub sorting_layer: i32 = 0,
        pub order: i32 = 0,
        /// Какие тайлы твёрдые: ни одного, все непустые или перечисленные в `solid_tiles`.
        pub collision: TileCollision = TileCollision::None,
        /// Индексы твёрдых тайлов для `Listed`.
        pub solid_tiles: Vec<i32> = Vec::new(),
        /// Индексы тайлов-«облачков»: сверху держат, снизу и сбоку пропускают.
        /// Работают при любом `collision`, кроме `None`.
        pub one_way_tiles: Vec<i32> = Vec::new(),
        pub collider_friction: f32 = 0.4,
        /// Слой столкновений создаваемых коллайдеров (`GameObject::layer`).
        pub collider_layer: i32 = 0,
    }
    runtime: RefCell<TilemapRuntime>
}

impl Component for Tilemap {
    /// Остальные хуки движок не вызывает.
    fn hooks(&self) -> Hooks {
        Hooks::START | Hooks::UPDATE
    }

    fn on_start(&mut self, ctx: &mut Ctx) {
        self.sync_colliders(ctx);
    }

    fn on_update(&mut self, ctx: &mut Ctx, _dt: f32) {
        self.sync_colliders(ctx);
    }
}

impl Tilemap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Карта из строк: `Tilemap::from_rows(&["1 1 1", ". . ."])`.
    pub fn from_rows(rows: &[&str]) -> Self {
        Self {
            rows: rows.iter().map(|r| r.to_string()).collect(),
            ..Self::default()
        }
    }

    /// Тайлсет: сетка `columns × rows` ячеек в текстуре.
    pub fn with_tileset(mut self, texture: AssetHandle<Texture>, columns: u32, rows: u32) -> Self {
        self.tileset = texture;
        self.tileset_columns = columns.max(1);
        self.tileset_rows = rows.max(1);
        self
    }

    pub fn with_tile_size(mut self, width: f32, height: f32) -> Self {
        self.tile_size = Vec2::new(width, height);
        self
    }

    pub fn with_layer(mut self, sorting_layer: i32, order: i32) -> Self {
        self.sorting_layer = sorting_layer;
        self.order = order;
        self
    }

    /// Все непустые тайлы — стены, кроме `one_way` (они — «облачка»).
    pub fn with_solid_nonempty(mut self) -> Self {
        self.collision = TileCollision::NonEmpty;
        self
    }

    /// Твёрдые только перечисленные тайлы.
    pub fn with_solid_tiles(mut self, tiles: &[i32]) -> Self {
        self.collision = TileCollision::Listed;
        self.solid_tiles = tiles.to_vec();
        self
    }

    pub fn with_one_way_tiles(mut self, tiles: &[i32]) -> Self {
        self.one_way_tiles = tiles.to_vec();
        self
    }

    fn refresh(&self) {
        {
            let cache = self.rt.borrow();
            if cache.valid && cache.rows == self.rows {
                return;
            }
        }
        // Хеш нужен только подписи коллайдеров — считаем его лишь при изменении карты.
        let hash = hash_rows(&self.rows);
        let (grid, problems) = parse_rows(&self.rows);
        for problem in &problems {
            log::warn!("Tilemap: {problem}");
        }
        let mut cache = self.rt.borrow_mut();
        cache.grid = grid;
        cache.problems = problems;
        cache.hash = hash;
        cache.rows.clone_from(&self.rows);
        cache.valid = true;
    }

    /// Разобранная карта (кэшируется; пересобирается, когда меняются `rows`).
    pub fn grid(&self) -> Ref<'_, TileGrid> {
        self.refresh();
        Ref::map(self.rt.borrow(), |cache| &cache.grid)
    }

    /// Замечания к записи `rows` (незнакомые слова); пусто — всё понятно.
    pub fn problems(&self) -> Vec<String> {
        self.refresh();
        self.rt.borrow().problems.clone()
    }

    /// (ширина, высота) карты в тайлах.
    pub fn size(&self) -> (usize, usize) {
        let grid = self.grid();
        (grid.width, grid.height)
    }

    /// Индекс тайла в ячейке (`x` слева, `y` — строка сверху; 0 — пусто).
    pub fn tile(&self, x: usize, y: usize) -> u32 {
        self.grid().get(x, y)
    }

    /// Поставить тайл (0 — стереть). Карта при необходимости растёт вправо и вниз.
    pub fn set_tile(&mut self, x: usize, y: usize, tile: u32) {
        let mut grid = self.grid().clone();
        let width = grid.width.max(x + 1);
        let height = grid.height.max(y + 1);
        let mut tiles = vec![0u32; width * height];
        for row in 0..grid.height {
            for column in 0..grid.width {
                tiles[row * width + column] = grid.get(column, row);
            }
        }
        tiles[y * width + x] = tile;
        grid.tiles = tiles;
        self.rows = (0..height)
            .map(|row| format_row(&grid.tiles[row * width..(row + 1) * width]))
            .collect();
    }

    /// Ячейка под точкой `local` (в локальных координатах объекта).
    pub fn cell_at(&self, local: Vec2) -> Option<(usize, usize)> {
        let grid = self.grid();
        let fx = (local.x / self.tile_size.x).floor();
        let fy = (local.y / self.tile_size.y).floor();
        if fx < 0.0 || fy < 0.0 || fx >= grid.width as f32 || fy >= grid.height as f32 {
            return None;
        }
        Some((fx as usize, grid.height - 1 - fy as usize))
    }

    /// Центр ячейки в локальных координатах объекта.
    pub fn cell_center(&self, x: usize, y: usize) -> Vec2 {
        let height = self.grid().height;
        Vec2::new(
            (x as f32 + 0.5) * self.tile_size.x,
            (height as f32 - y as f32 - 0.5) * self.tile_size.y,
        )
    }

    fn is_one_way(&self, tile: u32) -> bool {
        self.one_way_tiles.contains(&(tile as i32))
    }

    fn is_solid(&self, tile: u32) -> bool {
        tile != 0
            && match self.collision {
                TileCollision::None => false,
                TileCollision::NonEmpty => true,
                TileCollision::Listed => self.solid_tiles.contains(&(tile as i32)),
            }
    }

    /// Прямоугольники твёрдых тайлов (кроме односторонних), слитые жадно.
    pub fn solid_rects(&self) -> Vec<TileRect> {
        let grid = self.grid();
        let mask: Vec<bool> = grid
            .tiles
            .iter()
            .map(|t| self.is_solid(*t) && !self.is_one_way(*t))
            .collect();
        merge_mask(grid.width, grid.height, &mask)
    }

    /// Прямоугольники односторонних тайлов.
    pub fn one_way_rects(&self) -> Vec<TileRect> {
        if self.collision == TileCollision::None {
            return Vec::new();
        }
        let grid = self.grid();
        let mask: Vec<bool> = grid
            .tiles
            .iter()
            .map(|t| *t != 0 && self.is_one_way(*t))
            .collect();
        merge_mask(grid.width, grid.height, &mask)
    }

    /// Центр и размер прямоугольника тайлов в локальных координатах объекта.
    pub fn rect_bounds(&self, rect: TileRect) -> (Vec2, Vec2) {
        let height = self.grid().height;
        let size = Vec2::new(rect.width as f32 * self.tile_size.x, rect.height as f32 * self.tile_size.y);
        let center = Vec2::new(
            (rect.x as f32 + rect.width as f32 * 0.5) * self.tile_size.x,
            (height as f32 - rect.y as f32 - rect.height as f32 * 0.5) * self.tile_size.y,
        );
        (center, size)
    }

    /// Всё, от чего зависит набор коллайдеров.
    fn collider_signature(&self) -> u64 {
        self.refresh();
        let mut hash = self.rt.borrow().hash ^ 0x9E37_79B9;
        let mut mix = |value: u64| {
            hash ^= value;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };
        mix(self.collision as u64 + 1);
        mix(self.tile_size.x.to_bits() as u64);
        mix(self.tile_size.y.to_bits() as u64);
        mix(self.collider_friction.to_bits() as u64);
        mix(self.collider_layer as u64);
        for tile in &self.solid_tiles {
            mix(*tile as u64 ^ 0x100);
        }
        for tile in &self.one_way_tiles {
            mix(*tile as u64 ^ 0x200);
        }
        hash | 1
    }

    /// Пересобрать коллайдеры-дети, если изменилась карта или настройки.
    fn sync_colliders(&mut self, ctx: &mut Ctx) {
        let signature = if self.collision == TileCollision::None {
            1
        } else {
            self.collider_signature()
        };
        let first_time = self.rt.borrow().collider_signature == 0;
        if self.rt.borrow().collider_signature == signature {
            return;
        }
        self.rt.borrow_mut().collider_signature = signature;

        // Старые коллайдеры (в том числе сохранённые в сцене вместе с детьми) — долой.
        let stale: Vec<GameObjectId> = ctx
            .object()
            .children()
            .iter()
            .copied()
            .filter(|id| ctx.scene().get(*id).is_some_and(|o| o.name == TILE_COLLIDER_NAME))
            .collect();
        for id in stale {
            ctx.destroy(id);
        }
        if self.collision == TileCollision::None {
            return;
        }
        if first_time {
            log::debug!("Tilemap: building colliders");
        }

        let mut specs: Vec<(Vec2, Vec2, bool)> = Vec::new();
        for rect in self.solid_rects() {
            let (center, size) = self.rect_bounds(rect);
            specs.push((center, size, false));
        }
        for rect in self.one_way_rects() {
            let (center, size) = self.rect_bounds(rect);
            specs.push((center, size, true));
        }
        for (center, size, one_way) in specs {
            let mut collider = match ctx.world.registry.create("Collider2D") {
                Ok(collider) => collider,
                Err(_) => {
                    log::error!(
                        "Tilemap: component `Collider2D` is not registered, so tile colliders were not \
                         created. Call `engine_physics::register_components(&mut app.world.registry)` \
                         (`engine::app()` does it for you)"
                    );
                    return;
                }
            };
            let fields = [
                ("size", Value::Vec2(size.x, size.y)),
                ("friction", Value::Float(self.collider_friction)),
                ("one_way", Value::Bool(one_way)),
            ];
            for (name, value) in fields {
                if let Err(error) = collider.set_field(name, &value) {
                    log::error!("Tilemap: cannot configure Collider2D: {error}");
                    return;
                }
            }
            let child = ctx.world.scene.spawn(TILE_COLLIDER_NAME);
            if ctx.world.scene.set_parent(child, Some(ctx.id)).is_err() {
                continue;
            }
            if let Some(object) = ctx.world.scene.get_mut(child) {
                object.transform.position = center.extend(0.0);
                object.layer = self.collider_layer.clamp(0, 31) as u32;
                object.add_boxed(collider);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Отрисовка
    // -----------------------------------------------------------------------

    /// Диапазон видимых ячеек `(x0, x1, y0, y1)` (включительно, строки сверху) для
    /// камеры `view_proj`. `None` — карта пуста или целиком вне кадра.
    fn visible_range(&self, grid: &TileGrid, matrix: Mat4, view_proj: Mat4) -> Option<(usize, usize, usize, usize)> {
        if grid.width == 0 || grid.height == 0 {
            return None;
        }
        let full = Some((0, grid.width - 1, 0, grid.height - 1));
        let to_local = (view_proj * matrix).inverse();
        if !to_local.is_finite() {
            return full;
        }
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for (nx, ny) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            // Луч от ближней плоскости к дальней пересекаем с плоскостью карты (z = 0).
            let near = to_local.project_point3(Vec3::new(nx, ny, 0.0));
            let far = to_local.project_point3(Vec3::new(nx, ny, 1.0));
            let dz = near.z - far.z;
            if dz.abs() < 1e-7 || !near.is_finite() || !far.is_finite() {
                return full;
            }
            let t = near.z / dz;
            if !(0.0..=1.0).contains(&t) {
                return full;
            }
            let point = near + (far - near) * t;
            min = min.min(point.truncate());
            max = max.max(point.truncate());
        }
        // Запас в тайл: поворотные спрайты и погрешности не должны обрезать край.
        let x0 = ((min.x / self.tile_size.x).floor() - 1.0).max(0.0);
        let x1 = ((max.x / self.tile_size.x).floor() + 1.0).min(grid.width as f32 - 1.0);
        let by0 = ((min.y / self.tile_size.y).floor() - 1.0).max(0.0);
        let by1 = ((max.y / self.tile_size.y).floor() + 1.0).min(grid.height as f32 - 1.0);
        if x0 > x1 || by0 > by1 {
            return None;
        }
        // Нижние строки (по Y вверх) → строки сверху.
        let top = grid.height as f32 - 1.0;
        Some((x0 as usize, x1 as usize, (top - by1) as usize, (top - by0) as usize))
    }

    /// Дописать вершины видимых тайлов. Возвращает число нарисованных тайлов.
    pub(crate) fn write_vertices(
        &self,
        matrix: Mat4,
        view_proj: Mat4,
        inset: UvInset,
        out: &mut Vec<SpriteVertex>,
    ) -> usize {
        let grid = self.grid();
        let Some((x0, x1, y0, y1)) = self.visible_range(&grid, matrix, view_proj) else {
            return 0;
        };
        let columns = self.tileset_columns.max(1);
        let rows = self.tileset_rows.max(1);
        let cells = columns * rows;
        let origin = matrix.transform_point3(Vec3::ZERO);
        let step_x = matrix.transform_vector3(Vec3::new(self.tile_size.x, 0.0, 0.0));
        let step_y = matrix.transform_vector3(Vec3::new(0.0, self.tile_size.y, 0.0));
        let color = self.color.to_array();
        let mut count = 0;
        for y in y0..=y1 {
            let bottom = (grid.height - 1 - y) as f32;
            for x in x0..=x1 {
                let tile = grid.tiles[y * grid.width + x];
                if tile == 0 {
                    continue;
                }
                let uv = UvRect::FULL.cell(columns, rows, (tile - 1) % cells, inset);
                let a = origin + step_x * x as f32 + step_y * bottom;
                push_quad(out, [a, a + step_x, a + step_x + step_y, a + step_y], uv, color);
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tokens_repeats_and_ragged_rows() {
        let map = Tilemap::from_rows(&[". 2 *", "3*1 .", "5"]);
        let grid = map.grid();
        assert_eq!((grid.width, grid.height), (4, 3));
        assert_eq!(grid.get(1, 0), 2);
        assert_eq!(grid.get(0, 1), 1);
        assert_eq!(grid.get(2, 1), 1);
        assert_eq!(grid.get(3, 1), 0);
        assert_eq!(grid.get(0, 2), 5);
        assert_eq!(grid.get(3, 2), 0);
        drop(grid);
        assert_eq!(map.problems().len(), 1, "`*` alone is not a tile");
    }

    #[test]
    fn merges_into_few_rectangles() {
        let map = Tilemap::from_rows(&[
            ". . . . .",
            "1 . . . 1",
            "1 1 1 1 1",
            "1 1 1 1 1",
        ])
        .with_solid_nonempty();
        let rects = map.solid_rects();
        // Нижний блок 5×2, плюс две башенки по краям.
        assert_eq!(rects.len(), 3, "{rects:?}");
        let covered: usize = rects.iter().map(|r| r.width * r.height).sum();
        assert_eq!(covered, 12);
    }

    #[test]
    fn set_tile_rewrites_rows_and_grows_the_map() {
        let mut map = Tilemap::from_rows(&["1 1", ". ."]);
        map.set_tile(3, 2, 7);
        assert_eq!(map.size(), (4, 3));
        assert_eq!(map.tile(3, 2), 7);
        assert_eq!(map.tile(1, 0), 1);
        map.set_tile(0, 0, 0);
        assert_eq!(map.tile(0, 0), 0);
    }

    #[test]
    fn cell_lookup_matches_bottom_left_origin() {
        let map = Tilemap::from_rows(&["1 1", "1 1", "1 1"]).with_tile_size(2.0, 2.0);
        assert_eq!(map.cell_at(Vec2::new(0.5, 0.5)), Some((0, 2)));
        assert_eq!(map.cell_at(Vec2::new(3.9, 5.9)), Some((1, 0)));
        assert_eq!(map.cell_at(Vec2::new(-0.1, 1.0)), None);
        assert_eq!(map.cell_center(0, 2), Vec2::new(1.0, 1.0));
    }
}
