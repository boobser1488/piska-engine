//! Равномерная сетка (пространственный хеш) для проверок «круг против круга».
//!
//! Сетка не хранит объекты — только индексы в связных списках по ячейкам; пересобирается
//! каждый кадр без выделения памяти. Запрос возвращает **ровно** те объекты, чьи круги
//! пересекаются с кругом запроса (точная проверка внутри), поэтому результат совпадает с
//! полным перебором — это проверяет тест `grid_matches_brute_force`.

use engine::prelude::Vec2;

#[derive(Clone, Debug)]
pub struct Grid {
    min: Vec2,
    inv_cell: f32,
    w: i32,
    h: i32,
    head: Vec<i32>,
    next: Vec<i32>,
    pos: Vec<Vec2>,
    rad: Vec<f32>,
    max_r: f32,
    count: usize,
}

impl Grid {
    /// Сетка на квадрат `[-half, half]²` с ячейкой `cell`. Объекты за краем попадают
    /// в крайние ячейки (запросы остаются точными).
    pub fn new(half: f32, cell: f32) -> Self {
        let n = ((2.0 * half / cell).ceil() as i32).max(1);
        Self {
            min: Vec2::splat(-half),
            inv_cell: 1.0 / cell,
            w: n,
            h: n,
            head: vec![-1; (n * n) as usize],
            next: Vec::new(),
            pos: Vec::new(),
            rad: Vec::new(),
            max_r: 0.0,
            count: 0,
        }
    }

    /// Забыть все объекты (перед пересборкой).
    pub fn clear(&mut self) {
        self.head.fill(-1);
        self.max_r = 0.0;
        self.count = 0;
    }

    /// Сколько объектов вставлено с последнего `clear`.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn cell_of(&self, p: Vec2) -> (i32, i32) {
        let c = (p - self.min) * self.inv_cell;
        ((c.x.floor() as i32).clamp(0, self.w - 1), (c.y.floor() as i32).clamp(0, self.h - 1))
    }

    /// Добавить объект `id` (индекс слота у владельца) с центром `pos` и радиусом `r`.
    pub fn insert(&mut self, id: u32, pos: Vec2, r: f32) {
        let i = id as usize;
        if i >= self.next.len() {
            self.next.resize(i + 1, -1);
            self.pos.resize(i + 1, Vec2::ZERO);
            self.rad.resize(i + 1, 0.0);
        }
        let (cx, cy) = self.cell_of(pos);
        let cell = (cy * self.w + cx) as usize;
        self.next[i] = self.head[cell];
        self.head[cell] = id as i32;
        self.pos[i] = pos;
        self.rad[i] = r;
        self.max_r = self.max_r.max(r);
        self.count += 1;
    }

    /// Позвать `f(id)` для каждого вставленного объекта, чей круг пересекает круг
    /// `(pos, r)` (касание считается пересечением).
    pub fn query(&self, pos: Vec2, r: f32, mut f: impl FnMut(u32)) {
        let reach = r + self.max_r;
        let (x0, y0) = self.cell_of(pos - Vec2::splat(reach));
        let (x1, y1) = self.cell_of(pos + Vec2::splat(reach));
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let mut id = self.head[(cy * self.w + cx) as usize];
                while id >= 0 {
                    let i = id as usize;
                    let limit = r + self.rad[i];
                    if (self.pos[i] - pos).length_squared() <= limit * limit {
                        f(id as u32);
                    }
                    id = self.next[i];
                }
            }
        }
    }

    /// Все пересекающиеся пары `(a, b)` с `a < b` среди вставленных объектов (для тестов).
    pub fn pairs(&self, ids: &[u32]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for &a in ids {
            let (pa, ra) = (self.pos[a as usize], self.rad[a as usize]);
            self.query(pa, ra, |b| {
                if b > a {
                    out.push((a, b));
                }
            });
        }
        out.sort_unstable();
        out
    }
}
