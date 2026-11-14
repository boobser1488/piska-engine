//! Помощники постройки уровня из префабов-блоков: ящик по габаритам, пандус, лестница,
//! пол с вырезами, проём, навес, реквизит. Каждый вызов кладёт в сцену экземпляр префаба
//! (в редакторе это обычный объект: его можно двигать, тянуть, менять материал).

use std::collections::HashMap;
use std::error::Error;

use engine::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Прямоугольник на плане: x0..x1, z0..z1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub x1: f32,
    pub z0: f32,
    pub z1: f32,
}

pub fn rect(x0: f32, x1: f32, z0: f32, z1: f32) -> Rect {
    Rect { x0: x0.min(x1), x1: x0.max(x1), z0: z0.min(z1), z1: z0.max(z1) }
}

impl Rect {
    pub fn center(&self) -> Vec2 {
        Vec2::new((self.x0 + self.x1) * 0.5, (self.z0 + self.z1) * 0.5)
    }
    fn overlaps(&self, o: &Rect) -> bool {
        self.x0 < o.x1 - 1e-4 && o.x0 < self.x1 - 1e-4 && self.z0 < o.z1 - 1e-4 && o.z0 < self.z1 - 1e-4
    }
}

/// Что висит на стене (для резервирования места на фасаде).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallKind {
    Door,
    Window,
    Lantern,
    Awning,
}

/// Прямоугольник, занятый на плоскости стены: корень (на поверхности стены), поворот
/// «наружу», занятое вдоль стены `u` и по высоте `y` (от корня).
#[derive(Clone, Debug)]
pub struct WallItem {
    pub kind: WallKind,
    pub at: Vec3,
    pub yaw: f32,
    pub u: (f32, f32),
    pub y: (f32, f32),
}

impl WallItem {
    pub fn along(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw.to_radians()) * Vec3::X
    }
    pub fn out(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw.to_radians()) * Vec3::Z
    }
    /// На одной плоскости стены.
    pub fn same_wall(&self, o: &WallItem) -> bool {
        self.out().dot(o.out()) > 0.99 && (o.at - self.at).dot(self.out()).abs() < 0.05
    }
    /// Прямоугольники пересекаются с запасом `margin`.
    pub fn overlaps(&self, o: &WallItem, margin: f32) -> bool {
        if !self.same_wall(o) {
            return false;
        }
        let a = self.along();
        let (ca, cb) = (self.at.dot(a), o.at.dot(a));
        let (u0, u1) = (ca + self.u.0, ca + self.u.1);
        let (v0, v1) = (cb + o.u.0, cb + o.u.1);
        let (y0, y1) = (self.at.y + self.y.0, self.at.y + self.y.1);
        let (z0, z1) = (o.at.y + o.y.0, o.at.y + o.y.1);
        u0 < v1 + margin && v0 < u1 + margin && y0 < z1 + margin && z0 < y1 + margin
    }
}

/// Вид, занятое вдоль стены и по высоте от корня.
pub type Footprint = (WallKind, (f32, f32), (f32, f32));

/// Занятое место на стене по префабу (вдоль стены, по высоте от корня).
pub fn wall_footprint(prefab: &str) -> Option<Footprint> {
    Some(match prefab {
        "facade/door" => (WallKind::Door, (-0.77, 0.77), (0.0, 2.5)),
        "facade/window" => (WallKind::Window, (-0.92, 0.92), (-0.08, 1.32)),
        "facade/window_half" => (WallKind::Window, (-0.92, 0.58), (-0.08, 1.32)),
        "facade/window_shut" | "facade/window_blind" | "facade/window_ajar" => (WallKind::Window, (-0.58, 0.58), (-0.08, 1.32)),
        "props/wall_lantern" => (WallKind::Lantern, (-0.12, 0.12), (-0.2, 0.12)),
        "props/awning" => (WallKind::Awning, (-1.55, 1.55), (2.25, 2.75)),
        _ => return None,
    })
}

/// Окно, которое поставят в конце (`finish_windows`): вариант выбирается, когда известны
/// фонари, навесы, лотки и ширина улицы перед ним.
#[derive(Clone, Debug)]
pub struct PlannedWindow {
    pub at: Vec3,
    pub yaw: f32,
    pub upper: bool,
    pub seed: u32,
    pub group: Option<GameObjectId>,
}

/// Строитель сцены уровня.
pub struct Builder<'w> {
    pub world: &'w mut World,
    guids: HashMap<String, String>,
    /// Текущая группа (пустой объект-родитель) — так иерархия редактора читается по районам.
    pub group: Option<GameObjectId>,
    pub count: usize,
    /// Твёрдые ящики уже поставленного (центр, половины, поворот) — по ним реквизит
    /// встаёт на поверхность под собой.
    solids: Vec<(Vec3, Vec3, Quat)>,
    /// Реквизит, под которым пол неровный (имя, точка, разброс высот).
    pub uneven: Vec<(String, Vec3, f32)>,
    /// Занятые места на стенах (двери, окна, фонари, навесы).
    pub wall_items: Vec<WallItem>,
    /// Окна, которые ещё не поставлены.
    pub planned: Vec<PlannedWindow>,
    /// Поставленный реквизит: (вид, точка, радиус, высота верха).
    pub props: Vec<(String, Vec3, f32, f32)>,
    /// Что сделала расстановка на стенах (печатается генератором).
    pub notes: Vec<String>,
    /// Все блоки (и без коллайдера: карнизы, пояса) — чтобы окно не встало поперёк них.
    blocks: Vec<(Vec3, Vec3, Quat)>,
    /// Сколько окон какого вида поставлено.
    pub window_kinds: std::collections::BTreeMap<&'static str, usize>,
}

impl<'w> Builder<'w> {
    pub fn new(world: &'w mut World) -> Self {
        Self {
            world,
            guids: HashMap::new(),
            group: None,
            count: 0,
            solids: Vec::new(),
            uneven: Vec::new(),
            wall_items: Vec::new(),
            planned: Vec::new(),
            props: Vec::new(),
            notes: Vec::new(),
            blocks: Vec::new(),
            window_kinds: Default::default(),
        }
    }

    fn guid(&mut self, prefab: &str) -> Result<String> {
        if let Some(g) = self.guids.get(prefab) {
            return Ok(g.clone());
        }
        let path = if prefab.starts_with("ui/") { format!("{prefab}.prefab") } else { format!("prefabs/{prefab}.prefab") };
        let guid = self
            .world
            .resources
            .get::<AssetDatabase>()
            .and_then(|db| db.guid_of(&path))
            .ok_or_else(|| format!("prefab `{path}` is missing"))?
            .to_string();
        self.guids.insert(prefab.to_string(), guid.clone());
        Ok(guid)
    }

    /// Пустой объект-группа под текущей группой; становится текущей.
    pub fn group(&mut self, name: &str) -> GameObjectId {
        let builder = self.world.scene.build(name);
        let id = match self.group {
            Some(parent) => builder.child_of(parent).id(),
            None => builder.id(),
        };
        self.group = Some(id);
        id
    }

    /// Группа верхнего уровня.
    pub fn root_group(&mut self, name: &str) -> GameObjectId {
        self.group = None;
        self.group(name)
    }

    /// Экземпляр префаба в мировой позе (под текущей группой; группы стоят в начале
    /// координат без поворота, поэтому поза совпадает с мировой).
    pub fn spawn(&mut self, prefab: &str, name: &str, pos: Vec3, rot: Quat, scale: Vec3) -> Result<GameObjectId> {
        let guid = self.guid(prefab)?;
        let id = self.world.instantiate_at(&guid, pos, rot, self.group)?;
        let object = self.world.scene.get_mut(id).ok_or("instance")?;
        if !name.is_empty() {
            object.name = name.to_string();
        }
        object.transform.scale = scale;
        self.count += 1;
        self.register_solids(id);
        if prefab.starts_with("blocks/") {
            let (s, r, p) = self.world.scene.world_matrix(id).to_scale_rotation_translation();
            self.blocks.push((p, s.abs() * 0.5, r));
        }
        Ok(id)
    }

    /// Запомнить коллайдеры экземпляра (ящики и цилиндры — как ящики) для поиска опоры.
    fn register_solids(&mut self, root: GameObjectId) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(object) = self.world.scene.get(id) else { continue };
            stack.extend(object.children().iter().copied());
            let Some(collider) = object.get::<Collider>() else { continue };
            if collider.is_trigger {
                continue;
            }
            let (scale, rotation, position) = self.world.scene.world_matrix(id).to_scale_rotation_translation();
            let half = collider.size * scale.abs() * 0.5;
            let center = position + rotation * (collider.offset * scale);
            self.solids.push((center, half, rotation));
        }
    }

    /// Верх твёрдой геометрии под точкой (x, z), не выше `from_y`.
    pub fn ground_at(&self, x: f32, z: f32, from_y: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for (center, half, rotation) in &self.solids {
            // Вертикальная прямая в локальных координатах ящика.
            let inv = rotation.inverse();
            let o = inv * (Vec3::new(x, from_y, z) - *center);
            let d = inv * Vec3::NEG_Y;
            let (mut t0, mut t1) = (f32::MIN, f32::MAX);
            let mut hit = true;
            for axis in 0..3 {
                if d[axis].abs() < 1e-6 {
                    if o[axis].abs() > half[axis] {
                        hit = false;
                        break;
                    }
                } else {
                    let a = (-half[axis] - o[axis]) / d[axis];
                    let b = (half[axis] - o[axis]) / d[axis];
                    t0 = t0.max(a.min(b));
                    t1 = t1.min(a.max(b));
                }
            }
            if !hit || t0 > t1 || t1 < 0.0 {
                continue;
            }
            let top = from_y - t0.max(0.0);
            if t0 >= 0.0 && best.is_none_or(|b| top > b) {
                best = Some(top);
            }
        }
        best
    }

    /// Высота опоры под пятном радиусом `radius` вокруг (x, z): самая высокая точка;
    /// неровный пол (ступени, край) записывается в `uneven`.
    pub fn ground_under(&mut self, name: &str, x: f32, z: f32, from_y: f32, radius: f32) -> f32 {
        let mut heights = Vec::new();
        for (dx, dz) in [(0.0, 0.0), (1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            if let Some(h) = self.ground_at(x + dx * radius, z + dz * radius, from_y) {
                heights.push(h);
            }
        }
        // Опора — пол под центром; если край пятна выше (цоколь, ступень) или ниже (обрыв),
        // это записывается: такой реквизит надо передвинуть.
        let center = self.ground_at(x, z, from_y).unwrap_or(from_y - 0.5);
        let top = heights.iter().copied().fold(f32::MIN, f32::max);
        let low = heights.iter().copied().fold(f32::MAX, f32::min);
        if heights.len() < 5 || top - low > 0.01 {
            self.uneven.push((name.to_string(), Vec3::new(x, center, z), top - low));
        }
        center
    }

    /// Блок `blocks/<kind>` по габаритам (min..max).
    pub fn block(&mut self, kind: &str, name: &str, min: Vec3, max: Vec3) -> Result<GameObjectId> {
        let (lo, hi) = (min.min(max), min.max(max));
        self.spawn(&format!("blocks/{kind}"), name, (lo + hi) * 0.5, Quat::IDENTITY, hi - lo)
    }

    /// Блок на плане `r` от высоты `y0` до `y1`.
    pub fn bx(&mut self, kind: &str, name: &str, r: Rect, y0: f32, y1: f32) -> Result<GameObjectId> {
        self.block(kind, name, Vec3::new(r.x0, y0, r.z0), Vec3::new(r.x1, y1, r.z1))
    }

    /// Пол с вырезами: прямоугольник `area` минус `holes`, верх на `top`, толщина `depth`.
    /// Режется на непересекающиеся куски (одинаковые соседние клетки сливаются по рядам).
    pub fn floor(&mut self, kind: &str, name: &str, area: Rect, holes: &[Rect], top: f32, depth: f32) -> Result<usize> {
        let mut xs = vec![area.x0, area.x1];
        let mut zs = vec![area.z0, area.z1];
        for h in holes {
            for x in [h.x0, h.x1] {
                if x > area.x0 && x < area.x1 {
                    xs.push(x);
                }
            }
            for z in [h.z0, h.z1] {
                if z > area.z0 && z < area.z1 {
                    zs.push(z);
                }
            }
        }
        xs.sort_by(f32::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        zs.sort_by(f32::total_cmp);
        zs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        let mut pieces = 0;
        for zi in 0..zs.len() - 1 {
            // Слить подряд идущие свободные клетки ряда.
            let mut start: Option<f32> = None;
            for xi in 0..xs.len() - 1 {
                let cell = rect(xs[xi], xs[xi + 1], zs[zi], zs[zi + 1]);
                let free = !holes.iter().any(|h| h.overlaps(&cell));
                if free && start.is_none() {
                    start = Some(xs[xi]);
                }
                let end_of_run = !free || xi + 2 == xs.len();
                if let (true, Some(x0)) = (end_of_run, start) {
                    let x1 = if free { xs[xi + 1] } else { xs[xi] };
                    if x1 > x0 + 1e-4 {
                        self.bx(kind, name, rect(x0, x1, zs[zi], zs[zi + 1]), top - depth, top)?;
                        pieces += 1;
                    }
                    start = None;
                }
            }
        }
        Ok(pieces)
    }

    /// Пандус-плита: верхняя грань идёт от (`a`, `ya`) до (`b`, `yb`) вдоль оси Z (если
    /// `along_z`) или X, поперёк — от `c0` до `c1`. Плита толщиной `thickness` уходит вниз.
    #[allow(clippy::too_many_arguments)]
    pub fn ramp(&mut self, kind: &str, name: &str, along_z: bool, c0: f32, c1: f32, a: f32, ya: f32, b: f32, yb: f32, thickness: f32) -> Result<GameObjectId> {
        let (da, dy) = (b - a, yb - ya);
        let length = (da * da + dy * dy).sqrt();
        let width = (c1 - c0).abs();
        let mid_c = (c0 + c1) * 0.5;
        let mid_a = (a + b) * 0.5;
        let mid_y = (ya + yb) * 0.5;
        let (rotation, size, top_mid) = if along_z {
            // Локальная +Z → (0, dy, da)/L: поворот вокруг X на atan2(−dy, da).
            let mut theta = (-dy).atan2(da);
            // Нормаль (0, cos θ, sin θ) должна смотреть вверх; ящик симметричен.
            if theta.cos() < 0.0 {
                theta += std::f32::consts::PI;
            }
            (Quat::from_rotation_x(theta), Vec3::new(width, thickness, length), Vec3::new(mid_c, mid_y, mid_a))
        } else {
            // Локальная +X → (da, dy, 0)/L: поворот вокруг Z на atan2(dy, da).
            let mut theta = dy.atan2(da);
            if theta.cos() < 0.0 {
                theta += std::f32::consts::PI;
            }
            (Quat::from_rotation_z(theta), Vec3::new(length, thickness, width), Vec3::new(mid_a, mid_y, mid_c))
        };
        let normal = rotation * Vec3::Y;
        let center = top_mid - normal * (thickness * 0.5);
        self.spawn(&format!("blocks/{kind}"), name, center, rotation, size)
    }

    /// Лестница из `steps` ступеней: от `a` (высота `ya`) к `b` (высота `yb`) вдоль Z или X,
    /// поперёк — `c0..c1`. Каждая ступень — блок от `base` до своей высоты.
    #[allow(clippy::too_many_arguments)]
    pub fn stairs(&mut self, kind: &str, name: &str, along_z: bool, c0: f32, c1: f32, a: f32, ya: f32, b: f32, yb: f32, steps: usize, base: f32) -> Result<()> {
        for i in 0..steps {
            let s0 = a + (b - a) * i as f32 / steps as f32;
            let s1 = a + (b - a) * (i + 1) as f32 / steps as f32;
            let top = ya + (yb - ya) * (i + 1) as f32 / steps as f32;
            let r = if along_z { rect(c0, c1, s0, s1) } else { rect(s0, s1, c0, c1) };
            self.bx(kind, name, r, base, top)?;
        }
        Ok(())
    }

    /// Реквизит в точке (x, z), повёрнутый на `yaw_deg`: встаёт на твёрдую поверхность под
    /// собой (ищется вниз от `y + 0.5`; по пятну реквизита — самая высокая точка).
    pub fn prop(&mut self, kind: &str, x: f32, y: f32, z: f32, yaw_deg: f32) -> Result<GameObjectId> {
        let name = kind.rsplit('/').next().unwrap_or(kind).to_string();
        let radius = match kind {
            "crate_big" => 0.55,
            "crate_long" | "crate_small" | "barrel" => 0.38,
            "cart" | "well" | "bench" => 0.6,
            _ => 0.2,
        };
        let ground = self.ground_under(&name, x, z, y + 0.5, radius);
        let top = match kind {
            "market_stall" => 2.26,
            "lantern_post" => 2.7,
            "drying_line" => 2.5,
            "palm" => 5.6,
            "well" => 1.9,
            "crate_big" => 1.25,
            "cart" => 1.2,
            "barrel" | "plant_pot" | "bench" => 1.0,
            "crate_small" | "crate_long" => 0.9,
            _ => 0.6,
        };
        let reach = match kind {
            "market_stall" => 1.4,
            "cart" => 1.3,
            "drying_line" => 1.7,
            "palm" => 2.4,
            _ => radius,
        };
        self.props.push((name.clone(), Vec3::new(x, ground, z), reach, ground + top));
        self.spawn(&format!("props/{kind}"), &name, Vec3::new(x, ground, z), Quat::from_rotation_y(yaw_deg.to_radians()), Vec3::ONE)
    }

    /// Точка внутри какого-нибудь твёрдого ящика.
    pub fn inside_solid(&self, p: Vec3) -> bool {
        self.solids.iter().any(|(c, h, r)| {
            let l = r.inverse() * (p - *c);
            l.x.abs() < h.x && l.y.abs() < h.y && l.z.abs() < h.z
        })
    }

    /// Расстояние по лучу до ближайшего твёрдого ящика (не дальше `max`).
    pub fn ray_solid(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for (c, h, r) in &self.solids {
            let inv = r.inverse();
            let (o, d) = (inv * (origin - *c), inv * dir);
            let (mut t0, mut t1) = (0.0f32, max);
            let mut hit = true;
            for k in 0..3 {
                if d[k].abs() < 1e-8 {
                    if o[k].abs() > h[k] {
                        hit = false;
                        break;
                    }
                } else {
                    let (a, b) = ((-h[k] - o[k]) / d[k], (h[k] - o[k]) / d[k]);
                    t0 = t0.max(a.min(b));
                    t1 = t1.min(a.max(b));
                }
            }
            if hit && t0 <= t1 && best.is_none_or(|bst| t0 < bst) {
                best = Some(t0);
            }
        }
        best
    }

    /// Зарезервировать окно (поставится в `finish_windows`).
    pub fn plan_window(&mut self, x: f32, y: f32, z: f32, yaw_deg: f32, upper: bool, seed: u32) {
        self.planned.push(PlannedWindow { at: Vec3::new(x, y, z), yaw: yaw_deg, upper, seed, group: self.group });
    }

    fn planned_item(w: &PlannedWindow, prefab: &str) -> WallItem {
        let (kind, u, y) = wall_footprint(prefab).expect("window footprint");
        WallItem { kind, at: w.at, yaw: w.yaw, u, y }
    }

    /// Свободно ли место на стене для `item` (с запасом `margin`): не задевает поставленное
    /// и зарезервированные окна (кроме окна `skip`).
    fn wall_free(&self, item: &WallItem, margin: f32, skip: Option<usize>) -> bool {
        !self.wall_items.iter().any(|o| item.overlaps(o, margin))
            && !self
                .planned
                .iter()
                .enumerate()
                .any(|(i, w)| Some(i) != skip && item.overlaps(&Self::planned_item(w, "facade/window"), margin))
    }

    /// Стена за прямоугольником `item`: точки на 2 см в глубине — в стене; перед ним (3 см
    /// от стены, сеткой через 10 см) — ни твёрдого, ни декоративного блока (карниз, пояс,
    /// балка).
    fn on_wall(&self, item: &WallItem) -> bool {
        let (a, n) = (item.along(), item.out());
        let behind = [item.u.0 + 0.02, (item.u.0 + item.u.1) * 0.5, item.u.1 - 0.02].iter().all(|u| {
            [item.y.0 + 0.02, item.y.1 - 0.02].iter().all(|y| self.inside_solid(item.at + a * *u + Vec3::Y * *y - n * 0.02))
        });
        let in_block = |p: Vec3| {
            self.blocks.iter().any(|(c, h, r)| {
                let l = r.inverse() * (p - *c);
                l.x.abs() < h.x && l.y.abs() < h.y && l.z.abs() < h.z
            })
        };
        let steps = |lo: f32, hi: f32| {
            let n = ((hi - lo) / 0.1).ceil().max(1.0) as usize;
            (0..=n).map(move |i| lo + 0.01 + (hi - lo - 0.02) * i as f32 / n as f32)
        };
        let front = steps(item.u.0, item.u.1).all(|u| steps(item.y.0, item.y.1).all(|y| !in_block(item.at + a * u + Vec3::Y * y + n * 0.03)));
        behind && front
    }

    /// Пальма: ствол наклонён по локальной +X, крона — от −0.94 до 3.89 м по X и ±2.45 м по
    /// Z на высоте 2.4…5.6 м. Поворот выбирается из восьми так, чтобы крона не входила в
    /// стены и крыши.
    pub fn palm(&mut self, x: f32, y: f32, z: f32) -> Result<GameObjectId> {
        let crown = (Vec3::new(-0.94, 2.4, -2.45), Vec3::new(3.89, 5.6, 2.47));
        let mut best = (f32::MAX, 0.0);
        for k in 0..8 {
            let yaw = k as f32 * 45.0;
            let rot = Quat::from_rotation_y(yaw.to_radians());
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for i in 0..8 {
                let c = Vec3::new(
                    if i & 1 == 0 { crown.0.x } else { crown.1.x },
                    if i & 2 == 0 { crown.0.y } else { crown.1.y },
                    if i & 4 == 0 { crown.0.z } else { crown.1.z },
                );
                let p = Vec3::new(x, y, z) + rot * c;
                lo = lo.min(p);
                hi = hi.max(p);
            }
            let mut overlap = 0.0;
            for (center, half, _) in &self.solids {
                let (blo, bhi) = (*center - *half, *center + *half);
                let d = (hi.min(bhi) - lo.max(blo)).max(Vec3::ZERO);
                overlap += d.x * d.y * d.z;
            }
            if overlap < best.0 {
                best = (overlap, yaw);
            }
        }
        if best.0 > 0.0 {
            self.uneven.push(("palm crown".to_string(), Vec3::new(x, y, z), best.0));
        }
        self.prop("palm", x, y, z, best.1)
    }

    /// Предмет на стене (корень — на её поверхности, наружу +Z после поворота): не опускается.
    /// Занимает место на стене. Фонарь, попавший на окно, дверь или другой фонарь, сдвигается
    /// вдоль стены к ближайшему свободному простенку (до 2.5 м); если такого нет — окно под
    /// ним не ставится.
    pub fn wall_mount(&mut self, prefab: &str, x: f32, y: f32, z: f32, yaw_deg: f32) -> Result<GameObjectId> {
        let name = prefab.rsplit('/').next().unwrap_or(prefab).to_string();
        let mut at = Vec3::new(x, y, z);
        if let Some((kind, u, yy)) = wall_footprint(prefab) {
            let mut item = WallItem { kind, at, yaw: yaw_deg, u, y: yy };
            if kind == WallKind::Lantern && (!self.wall_free(&item, 0.15, None) || !self.on_wall(&item)) {
                let along = item.along();
                let mut found = None;
                'search: for step in 1..=50 {
                    for sign in [1.0, -1.0] {
                        let mut moved = item.clone();
                        moved.at = at + along * (sign * step as f32 * 0.05);
                        if self.wall_free(&moved, 0.15, None) && self.on_wall(&moved) {
                            found = Some(moved);
                            break 'search;
                        }
                    }
                }
                match found {
                    Some(moved) => {
                        self.notes.push(format!("wall lantern moved {:.2} m along the wall to a free pier: {at} -> {}", (moved.at - at).length(), moved.at));
                        at = moved.at;
                        item = moved;
                    }
                    None => {
                        let before = self.planned.len();
                        let lantern = item.clone();
                        self.planned.retain(|w| !lantern.overlaps(&Self::planned_item(w, "facade/window"), 0.15));
                        self.notes.push(format!("wall lantern at {at}: no free pier, {} window(s) dropped", before - self.planned.len()));
                    }
                }
            }
            self.wall_items.push(item);
        }
        self.spawn(prefab, &name, at, Quat::from_rotation_y(yaw_deg.to_radians()), Vec3::ONE)
    }

    /// Поставить зарезервированные окна. Вариант — по месту: перед окном лоток, фонарь на
    /// столбе или другой высокий реквизит, рядом настенный фонарь или навес — ставни
    /// закрыты; узкий переулок (до стены напротив меньше 4 м) — закрыто или заложено;
    /// иначе первый этаж в основном закрыт ставнями, второй — вперемешку.
    /// Окно, которому нет места (задевает дверь, фонарь, навес, угол стены) — не ставится;
    /// окно, смотрящее в стену ближе 0.5 м, — тоже.
    pub fn finish_windows(&mut self) -> Result<()> {
        let planned = std::mem::take(&mut self.planned);
        for (i, w) in planned.iter().enumerate() {
            let out = Quat::from_rotation_y(w.yaw.to_radians()) * Vec3::Z;
            let along = Quat::from_rotation_y(w.yaw.to_radians()) * Vec3::X;
            let center = w.at + Vec3::Y * 0.6;
            let opposite = self.ray_solid(center + out * 0.05, out, 6.0);
            if opposite.is_some_and(|d| d < 0.5) {
                self.notes.push(format!("window at {} faces a wall {:.2} m away: not placed", w.at, opposite.unwrap_or(0.0)));
                continue;
            }
            let narrow = opposite.is_some_and(|d| d < 4.0);
            // Высокий реквизит прямо перед окном.
            let blocked = self.props.iter().any(|(_, p, r, top)| {
                let rel = *p - w.at;
                let (d, u) = (rel.dot(out), rel.dot(along));
                *top > w.at.y - 0.05 && d > -0.2 && d < 1.6 + r && u.abs() < 0.95 + r
            });
            // Настенный фонарь или навес рядом на той же стене.
            let probe = WallItem { kind: WallKind::Window, at: w.at, yaw: w.yaw, u: (-1.3, 1.3), y: (-1.0, 2.0) };
            let neighbour = self.wall_items.iter().any(|o| matches!(o.kind, WallKind::Lantern | WallKind::Awning) && probe.overlaps(o, 0.0));
            let roll = (w.seed >> 3) % 100;
            let mut choice: &'static str = if blocked || neighbour {
                "facade/window_shut"
            } else if narrow {
                if roll % 3 == 1 {
                    "facade/window_blind"
                } else {
                    "facade/window_shut"
                }
            } else if !w.upper {
                match roll {
                    0..=49 => "facade/window_shut",
                    50..=69 => "facade/window_half",
                    70..=84 => "facade/window_ajar",
                    _ => "facade/window",
                }
            } else {
                match roll {
                    0..=29 => "facade/window",
                    30..=52 => "facade/window_ajar",
                    53..=72 => "facade/window_shut",
                    73..=88 => "facade/window_half",
                    _ => "facade/window_blind",
                }
            };
            let fits = |b: &Self, prefab: &str| {
                let item = Self::planned_item(w, prefab);
                !b.wall_items.iter().any(|o| item.overlaps(o, 0.08)) && b.on_wall(&item)
            };
            if !fits(self, choice) {
                // Закрытые ставни занимают меньше места, чем распахнутые.
                if fits(self, "facade/window_shut") {
                    choice = "facade/window_shut";
                } else {
                    self.notes.push(format!("window at {} has no room: not placed", w.at));
                    continue;
                }
            }
            let _ = i;
            let item = Self::planned_item(w, choice);
            self.wall_items.push(item);
            let keep = self.group;
            self.group = w.group;
            self.spawn(choice, "window", w.at, Quat::from_rotation_y(w.yaw.to_radians()), Vec3::ONE)?;
            self.group = keep;
            *self.window_kinds.entry(choice).or_default() += 1;
        }
        Ok(())
    }

    /// Ящики стопкой: `count` маленьких ящиков (0.9 м) — каждый встаёт на предыдущий.
    pub fn crate_stack(&mut self, x: f32, y: f32, z: f32, count: usize, yaw_deg: f32) -> Result<()> {
        let base = self.ground_under("crate_stack", x, z, y + 0.5, 0.38);
        for i in 0..count {
            self.prop("crate_small", x, base + 0.9 * i as f32, z, yaw_deg)?;
        }
        Ok(())
    }
}
