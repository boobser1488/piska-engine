//! Конструктор цехов для инструмента сборки ассетов: «черепаха» (`Track`) ведёт дорожку по
//! цеху, а `Kit` ставит в сцену **экземпляры префабов** (плиты, механизмы, шестерёнки,
//! лампы, декорации) и подгоняет их размеры и поля. Результат записывается как сцена —
//! в игре этого кода нет.
//!
//! Система координат дорожки: `yaw = 0` — вперёд по −Z; положительный поворот — налево.
//! Размеры — метры. Поверхность дорожки лежит на `y = Track::pos.y`.

use std::collections::HashMap;

use engine::prelude::*;
use marble::scripts::{
    blinker::Blinker, booster::Booster, checkpoint::Checkpoint, cog::Cog, conveyor::Conveyor, crusher::Crusher,
    fan::Fan, moving_platform::MovingPlatform, sweeper::Sweeper,
};
use marble::{Waypoint, WpKind};

/// Толщина плит дорожки.
pub const THICK: f32 = 0.6;

/// Материал плит.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mat {
    Plate,
    PlateDark,
    Hazard,
}

/// «Черепаха»: где кончился построенный участок и куда он смотрит.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub pos: Vec3,
    pub yaw: f32,
}

impl Track {
    pub fn new(pos: Vec3) -> Self {
        Self { pos, yaw: 0.0 }
    }

    /// Горизонтальное направление «вперёд».
    pub fn fwd(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    /// Направление «вправо» (по ходу движения).
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Точка относительно текущего конца: `d` вперёд, `lat` вправо, `dy` вверх.
    pub fn at(&self, d: f32, lat: f32, dy: f32) -> Vec3 {
        self.pos + self.fwd() * d + self.right() * lat + Vec3::Y * dy
    }
}

/// Построенный участок: из него берут точки для механизмов.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub start: Vec3,
    pub yaw: f32,
    /// Длина по горизонтали.
    pub len: f32,
    pub width: f32,
    /// Подъём на метр пути.
    pub slope: f32,
}

impl Seg {
    pub fn fwd(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Точка на поверхности: `d` метров от начала участка, `lat` вправо, `dy` над поверхностью.
    pub fn pt(&self, d: f32, lat: f32, dy: f32) -> Vec3 {
        self.start + self.fwd() * d + self.right() * lat + Vec3::Y * (dy + self.slope * d)
    }
}

/// Оформление участка дорожки.
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub mat: Mat,
    /// Бортики по краям.
    pub rails: bool,
}

impl Lane {
    pub const OPEN: Lane = Lane { mat: Mat::Plate, rails: false };
    pub const RAILED: Lane = Lane { mat: Mat::Plate, rails: true };
}

/// Строитель одного цеха: всё, что он создаёт, — экземпляры префабов в группах сцены.
pub struct Kit<'w> {
    pub world: &'w mut World,
    pub track: GameObjectId,
    pub mech: GameObjectId,
    pub pickups: GameObjectId,
    pub lamps: GameObjectId,
    pub decor: GameObjectId,
    /// Сколько обычных шестерёнок разложено (золотая считается отдельно).
    pub gears: u32,
    pub golden: bool,
    pub checkpoints: u32,
    pub route: Vec<Waypoint>,
    /// Границы цеха (по всем плитам) — для оформления.
    pub min: Vec3,
    pub max: Vec3,
    pub bodies: u32,
    counter: u32,
    refs: HashMap<String, String>,
}

impl<'w> Kit<'w> {
    /// Группы объектов — корни иерархии сцены: Track, Mechanisms, Pickups, Lamps, Decor.
    pub fn new(world: &'w mut World) -> Self {
        let group = |world: &mut World, name: &str| world.scene.spawn(name);
        let track = group(world, "Track");
        let mech = group(world, "Mechanisms");
        let pickups = group(world, "Pickups");
        let lamps = group(world, "Lamps");
        let decor = group(world, "Decor");
        Self {
            world,
            track,
            mech,
            pickups,
            lamps,
            decor,
            gears: 0,
            golden: false,
            checkpoints: 0,
            route: Vec::new(),
            min: Vec3::splat(f32::MAX),
            max: Vec3::splat(f32::MIN),
            bodies: 0,
            counter: 0,
            refs: HashMap::new(),
        }
    }

    // ------------------------------------------------------------------
    // Низкий уровень
    // ------------------------------------------------------------------

    /// Ссылка на префаб `prefabs/<rel>.prefab`: по GUID, чтобы файл можно было переносить.
    fn prefab_ref(&mut self, rel: &str) -> String {
        if let Some(found) = self.refs.get(rel) {
            return found.clone();
        }
        let path = format!("prefabs/{rel}.prefab");
        let guid = self
            .world
            .resources
            .get::<AssetDatabase>()
            .and_then(|db| db.guid_of(&path))
            .map(|g| g.to_string())
            .unwrap_or_else(|| panic!("prefab `{path}` is missing: run the baker from the start"));
        self.refs.insert(rel.to_string(), guid.clone());
        guid
    }

    /// Экземпляр префаба в группе; поза локальная в группе (группы — в начале координат).
    pub fn inst(&mut self, rel: &str, parent: GameObjectId, name: &str, pos: Vec3, rot: Quat) -> GameObjectId {
        let reference = self.prefab_ref(rel);
        let id = self
            .world
            .instantiate_at(&reference, pos, rot, Some(parent))
            .unwrap_or_else(|e| panic!("instantiate `{rel}`: {e}"));
        if !name.is_empty() {
            self.world.scene.get_mut(id).expect("instance").name = name.to_string();
        }
        id
    }

    fn numbered(&mut self, base: &str) -> String {
        self.counter += 1;
        format!("{base} {}", self.counter)
    }

    pub fn scale(&mut self, id: GameObjectId, scale: Vec3) {
        self.world.scene.get_mut(id).expect("object").transform.scale = scale;
    }

    /// Ребёнок по пути (`"Arm/Tip A"`).
    pub fn child(&self, id: GameObjectId, path: &str) -> GameObjectId {
        self.world.scene.find_path(id, path).unwrap_or_else(|| panic!("prefab child `{path}` is missing"))
    }

    /// Поза ребёнка (необязательные части).
    pub fn pose(&mut self, id: GameObjectId, path: &str, pos: Option<Vec3>, scale: Option<Vec3>) {
        let child = self.child(id, path);
        let t = &mut self.world.scene.get_mut(child).expect("child").transform;
        if let Some(p) = pos {
            t.position = p;
        }
        if let Some(s) = scale {
            t.scale = s;
        }
    }

    pub fn comp<T: Component>(&mut self, id: GameObjectId) -> &mut T {
        self.world.scene.get_component_mut::<T>(id).unwrap_or_else(|| panic!("component {} is missing", std::any::type_name::<T>()))
    }

    fn grow(&mut self, p: Vec3, r: f32) {
        self.min = self.min.min(p - Vec3::splat(r));
        self.max = self.max.max(p + Vec3::splat(r));
    }

    /// Точка маршрута для бота и для подсказок.
    pub fn wp(&mut self, pos: Vec3, kind: WpKind) {
        self.route.push(Waypoint { pos: pos.to_array(), kind });
    }

    // ------------------------------------------------------------------
    // Плиты и дорожка
    // ------------------------------------------------------------------

    /// Неподвижный блок-плита: экземпляр префаба нужного материала с размером `size`.
    pub fn block(&mut self, name: &str, center: Vec3, size: Vec3, rot: Quat, mat: Mat) -> GameObjectId {
        let rel = match mat {
            Mat::Plate => "track/plate_block",
            Mat::PlateDark => "track/plate_dark_block",
            Mat::Hazard => "track/hazard_block",
        };
        let track = self.track;
        let id = self.inst(rel, track, name, center, rot);
        self.scale(id, size);
        self.grow(center, size.max_element() * 0.5);
        id
    }

    /// Плита между двумя точками верхней поверхности (плоская или наклонная) + бортики.
    fn slab_between(&mut self, name: &str, a: Vec3, b: Vec3, yaw: f32, width: f32, lane: Lane) -> Seg {
        let delta = b - a;
        let hlen = Vec2::new(delta.x, delta.z).length();
        let length = delta.length();
        let pitch = delta.y.atan2(hlen.max(1e-4));
        let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
        let normal = rot * Vec3::Y;
        let center = (a + b) * 0.5 - normal * (THICK * 0.5);
        self.block(name, center, Vec3::new(width, THICK, length), rot, lane.mat);
        if lane.rails {
            let right = rot * Vec3::X;
            for (rail, side) in [("Rail L", -1.0f32), ("Rail R", 1.0)] {
                let c = (a + b) * 0.5 + right * (side * (width * 0.5 - 0.15)) + normal * 0.45;
                self.block(rail, c, Vec3::new(0.3, 0.9, length), rot, Mat::Hazard);
            }
        }
        Seg { start: a, yaw, len: hlen, width, slope: if hlen > 1e-4 { delta.y / hlen } else { 0.0 } }
    }

    /// Ровный участок длиной `len`.
    pub fn straight(&mut self, tr: &mut Track, len: f32, width: f32, lane: Lane) -> Seg {
        let end = tr.pos + tr.fwd() * len;
        let name = self.numbered("Floor");
        let seg = self.slab_between(&name, tr.pos, end, tr.yaw, width, lane);
        tr.pos = end;
        self.wp(end, WpKind::Go);
        seg
    }

    /// Наклонный участок: `len` по горизонтали, `dy` вверх (минус — вниз).
    pub fn ramp(&mut self, tr: &mut Track, len: f32, dy: f32, width: f32, lane: Lane) -> Seg {
        let end = tr.pos + tr.fwd() * len + Vec3::Y * dy;
        let name = self.numbered("Ramp");
        let seg = self.slab_between(&name, tr.pos, end, tr.yaw, width, lane);
        tr.pos = end;
        self.wp(end, WpKind::Go);
        seg
    }

    /// Щель без пола: дорожка продолжается через `len` метров (и на `dy` выше). Последняя
    /// точка маршрута перед щелью помечается прыжком.
    pub fn gap(&mut self, tr: &mut Track, len: f32, dy: f32) {
        if let Some(last) = self.route.last_mut() {
            if last.kind == WpKind::Go {
                last.kind = WpKind::Jump;
            }
        }
        tr.pos += tr.fwd() * len + Vec3::Y * dy;
        self.wp(tr.pos + tr.fwd() * 1.5, WpKind::Go);
    }

    /// Площадка-поворот `w × w` и поворот дорожки на `deg` градусов (плюс — налево).
    pub fn corner(&mut self, tr: &mut Track, w: f32, deg: f32, lane: Lane) -> Seg {
        let end = tr.pos + tr.fwd() * w;
        let old_fwd = tr.fwd();
        let old_right = tr.right();
        let name = self.numbered("Corner");
        let seg = self.slab_between(&name, tr.pos, end, tr.yaw, w, Lane { rails: false, ..lane });
        let center = tr.pos + old_fwd * (w * 0.5);
        tr.yaw += deg.to_radians();
        let new_fwd = tr.fwd();
        if lane.rails {
            // Бортики на сторонах, где нет ни входа, ни выхода.
            for dir in [old_fwd, old_right, -old_right, -old_fwd] {
                if dir.dot(-old_fwd) > 0.9 || dir.dot(new_fwd) > 0.9 {
                    continue;
                }
                let rot = Quat::from_rotation_y(f32::atan2(-dir.x, -dir.z));
                self.block("Rail", center + dir * (w * 0.5 - 0.15) + Vec3::Y * 0.45, Vec3::new(w, 0.9, 0.3), rot, Mat::Hazard);
            }
        }
        tr.pos = center + new_fwd * (w * 0.5);
        self.wp(center, WpKind::Go);
        self.wp(tr.pos, WpKind::Go);
        seg
    }

    // ------------------------------------------------------------------
    // Механизмы: экземпляры префабов с подогнанными размерами и полями
    // ------------------------------------------------------------------

    /// Ездящая платформа. `size` = (ширина, толщина, длина); `top` — центр верхней
    /// поверхности в начальной точке; `travel` — смещение конечной точки.
    #[allow(clippy::too_many_arguments)]
    pub fn platform(&mut self, top: Vec3, yaw: f32, size: Vec3, travel: Vec3, period: f32, pause: f32, phase: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("mech/moving_platform", mech, "Moving Platform", top - Vec3::Y * (size.y * 0.5), Quat::from_rotation_y(yaw));
        self.scale(id, size);
        let c = self.comp::<MovingPlatform>(id);
        c.travel = travel;
        c.period = period;
        c.pause = pause;
        c.phase = phase;
        self.grow(top + travel, size.max_element() * 0.5);
        id
    }

    /// Метла: узел в точке `floor` (на полу), `bars` перекладин длиной `length`
    /// (1 — одна палка через центр, 2 — крест).
    pub fn sweeper(&mut self, floor: Vec3, length: f32, bars: u32, speed_deg: f32, swing_deg: f32, phase_deg: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst(if bars > 1 { "mech/sweeper_cross" } else { "mech/sweeper" }, mech, "Sweeper", floor, Quat::IDENTITY);
        self.pose(id, "Arm", None, Some(Vec3::new(length, 0.55, 0.45)));
        if bars > 1 {
            self.pose(id, "Arm 2", None, Some(Vec3::new(length, 0.55, 0.45)));
        }
        let c = self.comp::<Sweeper>(id);
        c.speed_deg = speed_deg;
        c.swing_deg = swing_deg;
        c.phase_deg = phase_deg;
        self.grow(floor, length * 0.5);
        id
    }

    /// Конвейер: `top` — центр верхней поверхности, `yaw` — куда смотрит «вперёд» ленты,
    /// `speed` м/с (минус — против `yaw`).
    pub fn conveyor(&mut self, top: Vec3, yaw: f32, len: f32, width: f32, speed: f32) -> GameObjectId {
        let mech = self.mech;
        let thick = 0.4;
        let id = self.inst("mech/conveyor", mech, "Conveyor", top - Vec3::Y * (thick * 0.5), Quat::from_rotation_y(yaw));
        self.pose(id, "Belt", None, Some(Vec3::new(width, thick, len)));
        self.pose(id, "Side L", Some(Vec3::new(-(width * 0.5 + 0.15), 0.05, 0.0)), Some(Vec3::new(0.3, thick + 0.1, len)));
        self.pose(id, "Side R", Some(Vec3::new(width * 0.5 + 0.15, 0.05, 0.0)), Some(Vec3::new(0.3, thick + 0.1, len)));
        // Бегущие планки — направление движения видно сразу.
        let spacing = 1.2;
        let n = (len / spacing).ceil() as u32;
        let prefab = if speed >= 0.0 { "mech/slat" } else { "mech/slat_reverse" };
        for i in 0..n {
            let z = len * 0.5 - i as f32 * spacing;
            let slat = self.inst(prefab, id, &format!("Slat {i}"), Vec3::new(0.0, thick * 0.5 + 0.03, z), Quat::IDENTITY);
            self.scale(slat, Vec3::new(width - 0.1, 0.06, 0.28));
        }
        let c = self.comp::<Conveyor>(id);
        c.speed = speed;
        c.size = Vec3::new(width, thick, len);
        c.slat_spacing = spacing;
        self.grow(top, len.max(width) * 0.5);
        id
    }

    /// Вентилятор: `base` — центр основания, `rot` — поворот (струя идёт вдоль его «вверх»).
    pub fn fan(&mut self, base: Vec3, rot: Quat, height: f32, radius: f32, lift: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("mech/fan", mech, "Fan", base, rot);
        self.pose(id, "Grille", None, Some(Vec3::new(radius * 1.7, 0.08, radius * 1.7)));
        self.pose(id, "Ring", None, Some(Vec3::new(radius * 1.9, 0.06, radius * 1.9)));
        self.pose(id, "Rotor", None, Some(Vec3::new(radius * 1.5, 1.0, radius * 1.5)));
        let flow = self.child(id, "Flow");
        if let Some(p) = self.world.scene.get_component_mut::<ParticleSystem>(flow) {
            p.start_speed = Vec2::new(height * 0.9, height * 1.3);
            p.shape_size = Vec3::new(radius * 0.8, 0.0, 0.0);
        }
        let c = self.comp::<Fan>(id);
        c.lift = lift;
        c.height = height;
        c.radius = radius;
        self.grow(base + rot * Vec3::Y * height, radius + 1.0);
        id
    }

    /// Пресс: `head_up` — центр головы в верхнем положении, `head` — габариты головы.
    pub fn crusher(&mut self, head_up: Vec3, head: Vec3, stroke: f32, period: f32, wait_up: f32, phase: f32, yaw: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("mech/crusher", mech, "Crusher", head_up, Quat::from_rotation_y(yaw));
        let shaft_len = 9.0;
        self.pose(id, "Head", None, Some(head));
        self.pose(id, "Shaft", Some(Vec3::new(0.0, head.y * 0.5 + shaft_len * 0.5, 0.0)), Some(Vec3::new(0.7, shaft_len, 0.7)));
        self.pose(id, "Housing", Some(Vec3::new(0.0, head.y * 0.5 + shaft_len + 0.5, 0.0)), Some(Vec3::new(head.x + 0.8, 1.2, head.z + 0.8)));
        let c = self.comp::<Crusher>(id);
        c.stroke = stroke;
        c.period = period;
        c.wait_up = wait_up;
        c.phase = phase;
        self.grow(head_up, head.max_element());
        id
    }

    /// Бампер-«грибок».
    pub fn bumper(&mut self, floor: Vec3, radius: f32) -> GameObjectId {
        let mech = self.mech;
        let h = 0.9;
        let id = self.inst("mech/bumper", mech, "Bumper", floor + Vec3::Y * (h * 0.5), Quat::IDENTITY);
        self.scale(id, Vec3::new(radius * 2.0, h, radius * 2.0));
        self.grow(floor, radius + 1.0);
        id
    }

    /// Хрупкая плита; `top` — центр верхней поверхности.
    pub fn tile(&mut self, top: Vec3, yaw: f32, w: f32, l: f32) -> GameObjectId {
        let mech = self.mech;
        let size = Vec3::new(w, 0.45, l);
        let id = self.inst("mech/tile", mech, "Collapsing Tile", top - Vec3::Y * (size.y * 0.5), Quat::from_rotation_y(yaw));
        self.scale(id, size);
        self.grow(top, w.max(l));
        id
    }

    /// Ускоряющая плита: стрелки вдоль `yaw`.
    pub fn booster(&mut self, floor: Vec3, yaw: f32, w: f32, l: f32, speed: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("mech/booster", mech, "Booster", floor, Quat::from_rotation_y(yaw));
        if let Some(c) = self.world.scene.get_component_mut::<Collider>(id) {
            c.size = Vec3::new(w, 1.6, l);
        }
        self.pose(id, "Pad", None, Some(Vec3::new(w, 0.06, l)));
        self.pose(id, "Arrows", None, Some(Vec3::new(0.35, 0.02, l * 0.55)));
        self.comp::<Booster>(id).speed = speed;
        self.grow(floor, w.max(l));
        id
    }

    /// Шестерёнка-подбор; `golden` — золотая (одна на цех).
    pub fn gear(&mut self, pos: Vec3, golden: bool) -> GameObjectId {
        let pickups = self.pickups;
        let id = self.inst(if golden { "pickups/gear_golden" } else { "pickups/gear" }, pickups, if golden { "Golden Gear" } else { "Gear" }, pos, Quat::IDENTITY);
        if golden {
            self.golden = true;
        } else {
            self.gears += 1;
        }
        self.grow(pos, 1.0);
        id
    }

    /// Контрольные ворота; `floor` — центр на полу, `yaw` — куда смотрит камера после возврата.
    pub fn checkpoint(&mut self, floor: Vec3, yaw: f32, width: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("mech/checkpoint", mech, "Checkpoint", floor, Quat::from_rotation_y(yaw));
        if let Some(c) = self.world.scene.get_component_mut::<Collider>(id) {
            c.size = Vec3::new(width, 3.0, 1.0);
        }
        self.pose(id, "Post L", Some(Vec3::new(-(width * 0.5 + 0.2), 1.6, 0.0)), None);
        self.pose(id, "Post R", Some(Vec3::new(width * 0.5 + 0.2, 1.6, 0.0)), None);
        self.pose(id, "Bar", None, Some(Vec3::new(width + 0.8, 0.32, 0.32)));
        self.checkpoints += 1;
        let index = self.checkpoints as i32;
        let c = self.comp::<Checkpoint>(id);
        c.index = index;
        c.yaw_deg = yaw.to_degrees();
        id
    }

    /// Площадка отгрузки.
    pub fn finish(&mut self, floor: Vec3, yaw: f32, width: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("mech/finish_pad", mech, "Finish", floor, Quat::from_rotation_y(yaw));
        if let Some(c) = self.world.scene.get_component_mut::<Collider>(id) {
            c.size = Vec3::new(width, 4.0, width);
        }
        self.pose(id, "Pad", None, Some(Vec3::new(width * 0.9, 0.08, width * 0.9)));
        self.pose(id, "Ring", None, Some(Vec3::new(width * 0.62, 0.1, width * 0.62)));
        self.pose(id, "Pylon L", Some(Vec3::new(-width * 0.5, 1.8, 0.0)), None);
        self.pose(id, "Pylon R", Some(Vec3::new(width * 0.5, 1.8, 0.0)), None);
        self.pose(id, "Cap L", Some(Vec3::new(-width * 0.5, 3.7, 0.0)), None);
        self.pose(id, "Cap R", Some(Vec3::new(width * 0.5, 3.7, 0.0)), None);
        self.grow(floor, width);
        id
    }

    /// Стальной ящик — настоящее динамическое тело.
    pub fn crate_(&mut self, center: Vec3, size: f32) -> GameObjectId {
        let mech = self.mech;
        let id = self.inst("track/crate", mech, "Crate", center, Quat::IDENTITY);
        self.scale(id, Vec3::splat(size));
        self.bodies += 1;
        self.grow(center, size);
        id
    }

    /// Качели: доска на оси поперёк хода (стартует наклонённой), опора и упоры по концам.
    pub fn seesaw(&mut self, pivot_top: Vec3, yaw: f32, length: f32, width: f32) -> GameObjectId {
        let mech = self.mech;
        let rot = Quat::from_rotation_y(yaw);
        let fwd = rot * Vec3::NEG_Z;
        let thick = 0.35;
        let center = pivot_top - Vec3::Y * (thick * 0.5);
        // Доска стартует наклонённой: задний (входной) конец внизу — шар въезжает как на рампу.
        let tilt = 0.25f32.asin();
        let plank = self.inst("track/seesaw", mech, "Seesaw", center, rot * Quat::from_rotation_x(tilt));
        self.scale(plank, Vec3::new(width, thick, length));
        // Опора и упоры: доска наклоняется не больше чем на ~14°.
        let pivot = self.inst("track/pivot", mech, "Seesaw Pivot", center - Vec3::Y * 0.6, rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        self.scale(pivot, Vec3::new(0.6, width + 0.6, 0.6));
        let reach = length * 0.5 - 0.7;
        let stop_drop = reach * 0.25 + thick * 0.5 + 0.03;
        for (name, side) in [("Seesaw Stop A", -1.0f32), ("Seesaw Stop B", 1.0)] {
            let c = center + fwd * (side * reach) - Vec3::Y * (stop_drop + 0.3);
            self.block(name, c, Vec3::new(width, 0.6, 0.8), rot, Mat::Hazard);
        }
        self.bodies += 1;
        self.grow(center, length);
        plank
    }

    /// Лампа-мигалка (светится сильнее 1 — даёт блум).
    pub fn lamp(&mut self, pos: Vec3, color: Color, rate: f32, phase: f32) -> GameObjectId {
        let lamps = self.lamps;
        let id = self.inst("mech/lamp", lamps, "Lamp", pos, Quat::IDENTITY);
        if let Some(r) = self.world.scene.get_component_mut::<MeshRenderer>(id) {
            r.emissive = color;
        }
        let c = self.comp::<Blinker>(id);
        c.color = color;
        c.rate = rate;
        c.phase = phase;
        id
    }

    /// Столб с лампой по краю дорожки.
    pub fn lamp_post(&mut self, floor: Vec3, color: Color, phase: f32) {
        let lamps = self.lamps;
        let id = self.inst("mech/lamp_post", lamps, "Lamp Post", floor + Vec3::Y * 1.1, Quat::IDENTITY);
        let lamp = self.child(id, "Lamp");
        if let Some(r) = self.world.scene.get_component_mut::<MeshRenderer>(lamp) {
            r.emissive = color;
        }
        if let Some(b) = self.world.scene.get_component_mut::<Blinker>(lamp) {
            b.color = color;
            b.phase = phase;
        }
    }

    /// Светящаяся линия на полу (старт).
    pub fn start_line(&mut self, pos: Vec3, yaw: f32, width: f32) {
        let track = self.track;
        let id = self.inst("track/glow_strip", track, "Start Line", pos, Quat::from_rotation_y(yaw));
        self.scale(id, Vec3::new(width, 0.03, 0.35));
    }

    /// Шар-витрина для меню: стоит на старте, без тега «Ball».
    pub fn showcase_ball(&mut self, pos: Vec3) {
        let mech = self.mech;
        self.inst("showcase_ball", mech, "Showcase Ball", pos, Quat::IDENTITY);
    }

    // ------------------------------------------------------------------
    // Декорации
    // ------------------------------------------------------------------

    pub fn pillar(&mut self, pos: Vec3, width: f32, top: f32, bottom: f32) {
        let height = top - bottom;
        let decor = self.decor;
        let id = self.inst("decor/pillar", decor, "Pillar", Vec3::new(pos.x, top - height * 0.5, pos.z), Quat::IDENTITY);
        self.scale(id, Vec3::new(width, height, width));
    }

    pub fn smoke(&mut self, pos: Vec3) {
        let decor = self.decor;
        self.inst("decor/smoke", decor, "Chimney Smoke", pos, Quat::IDENTITY);
    }

    pub fn cloud_sea(&mut self, pos: Vec3) {
        let decor = self.decor;
        self.inst("decor/cloud_sea", decor, "Cloud Sea", pos, Quat::IDENTITY);
    }

    pub fn cloud(&mut self, pos: Vec3, scale: Vec3) {
        let decor = self.decor;
        let id = self.inst("decor/cloud", decor, "Cloud", pos, Quat::IDENTITY);
        self.scale(id, scale);
    }

    /// Гигантская шестерня-декорация; `yaw` — куда повёрнут диск.
    pub fn cog(&mut self, pos: Vec3, yaw: f32, size: f32, speed_deg: f32) {
        let decor = self.decor;
        let id = self.inst("decor/cog", decor, "Cog", pos, Quat::from_rotation_y(yaw) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
        self.scale(id, Vec3::splat(size));
        self.comp::<Cog>(id).speed_deg = speed_deg;
    }
}
