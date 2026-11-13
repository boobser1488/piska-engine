//! Конструктор цехов: «черепаха» (`Track`), которая ведёт дорожку по цеху, и `Kit` — набор
//! готовых деталей (плиты, рампы, механизмы на C++, ящики, качели, шестерни…). Цех из
//! `courses.rs` — это последовательность вызовов этого набора.
//!
//! Система координат дорожки: `yaw = 0` — вперёд по −Z; положительный поворот — налево.
//! Размеры — метры. Поверхность дорожки лежит на `y = Track::pos.y`.

use engine::prelude::*;

/// Толщина плит дорожки.
pub const THICK: f32 = 0.6;

/// Материал детали уровня.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mat {
    /// Стальной настил — основной пол.
    Plate,
    /// Тёмный настил — стенки и «подвальные» части.
    PlateDark,
    /// Жёлто-чёрные предупреждающие полосы.
    Hazard,
    /// Шлифованный металл: колонны, борта, штанги.
    Brushed,
    /// Сигнальная резина (бамперы).
    Rubber,
    /// Лента конвейера.
    Belt,
    /// Ящик.
    Crate,
    /// Светящаяся деталь (лампы, стрелки); цвет — множитель HDR.
    Glow(Color),
    /// Благородный металл шестерёнки.
    Gold,
    /// Серебристая шестерёнка.
    Silver,
}

/// Чем занять шара на этой точке пути — для бота в тестах и для подсказок.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WpKind {
    /// Просто доехать.
    Go,
    /// Доехать и прыгнуть (перед щелью).
    Jump,
    /// Стоять, пока механизм с тегом `Mover` не окажется рядом с этой точкой.
    WaitMover(Vec3),
    /// Стоять, пока не пройдёт указанное число секунд игрового времени от начала цеха
    /// (окно между ударами прессов и т. п.): `(период, окно_начала, окно_конца)`.
    WaitWindow(f32, f32, f32),
}

/// Точка маршрута.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waypoint {
    pub pos: Vec3,
    pub kind: WpKind,
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

    /// Середина участка.
    pub fn mid(&self, lat: f32, dy: f32) -> Vec3 {
        self.pt(self.len * 0.5, lat, dy)
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

/// Текстуры игры (генерирует `tools/gen_assets.py`).
#[derive(Clone)]
pub struct Textures {
    pub plate: AssetHandle<Texture>,
    pub hazard: AssetHandle<Texture>,
    pub brushed: AssetHandle<Texture>,
    pub crate_: AssetHandle<Texture>,
}

impl Textures {
    /// Текстуры из базы ассетов; без базы (или без файлов) — пустые ссылки, цех будет
    /// просто однотонным.
    pub fn load(world: &World) -> Self {
        let db = world.resources.get::<AssetDatabase>();
        let get = |path: &str| -> AssetHandle<Texture> {
            match db.map(|db| db.handle::<Texture>(path)) {
                Some(Ok(handle)) => handle,
                Some(Err(err)) => {
                    log::warn!("texture `{path}` is missing: {err}");
                    AssetHandle::none()
                }
                None => AssetHandle::none(),
            }
        };
        Self {
            plate: get("textures/plate.png"),
            hazard: get("textures/hazard.png"),
            brushed: get("textures/brushed.png"),
            crate_: get("textures/crate.png"),
        }
    }
}

/// Строитель одного цеха: всё, что он создаёт, — дети корня `root`.
pub struct Kit<'w> {
    pub world: &'w mut World,
    pub root: GameObjectId,
    pub tex: Textures,
    /// Сколько обычных шестерёнок разложено (золотая считается отдельно).
    pub gears: u32,
    pub golden: bool,
    pub checkpoints: u32,
    pub route: Vec<Waypoint>,
    /// Границы цеха (по всем плитам) — для оформления и камеры.
    pub min: Vec3,
    pub max: Vec3,
    pub bodies: u32,
    counter: u32,
}

impl<'w> Kit<'w> {
    pub fn new(world: &'w mut World, root: GameObjectId) -> Self {
        let tex = Textures::load(world);
        Self {
            world,
            root,
            tex,
            gears: 0,
            golden: false,
            checkpoints: 0,
            route: Vec::new(),
            min: Vec3::splat(f32::MAX),
            max: Vec3::splat(f32::MIN),
            bodies: 0,
            counter: 0,
        }
    }

    // ------------------------------------------------------------------
    // Низкий уровень
    // ------------------------------------------------------------------

    fn next_name(&mut self, base: &str) -> String {
        self.counter += 1;
        format!("{base} {}", self.counter)
    }

    /// Пустой объект-ребёнок.
    pub fn node(&mut self, name: &str, parent: GameObjectId, pos: Vec3, rot: Quat, scale: Vec3) -> GameObjectId {
        self.world
            .scene
            .build(name)
            .at3(pos.x, pos.y, pos.z)
            .rotation(rot)
            .scale(scale.x, scale.y, scale.z)
            .child_of(parent)
            .id()
    }

    pub fn add<T: Component>(&mut self, id: GameObjectId, component: T) {
        if let Some(object) = self.world.scene.get_mut(id) {
            object.add(component);
        }
    }

    /// Повесить компонент из C++ по имени и выставить его поля. Имя или поле с опечаткой —
    /// ошибка в данных цеха: паникуем сразу с понятным текстом (тесты строят каждый цех).
    pub fn cpp(&mut self, id: GameObjectId, kind: &str, fields: &[(&str, Value)]) {
        let mut component = self
            .world
            .registry
            .create(kind)
            .unwrap_or_else(|err| panic!("C++ component `{kind}` is not registered ({err}). Did the game call engine::add_static_cpp?"));
        for (name, value) in fields {
            component
                .set_field(name, value)
                .unwrap_or_else(|err| panic!("{kind}.{name}: {err}"));
        }
        if let Some(object) = self.world.scene.get_mut(id) {
            object.add_boxed(component);
        }
    }

    fn grow(&mut self, p: Vec3, r: f32) {
        self.min = self.min.min(p - Vec3::splat(r));
        self.max = self.max.max(p + Vec3::splat(r));
    }

    /// Рендерер под материал.
    pub fn renderer(&self, shape: MeshShape, mat: Mat) -> MeshRenderer {
        let base = MeshRenderer::shape(shape);
        match mat {
            Mat::Plate => base.with_color(Color::hex(0xb8cdf2)).with_material(0.3, 0.6).with_texture(self.tex.plate, 2.0),
            Mat::PlateDark => base.with_color(Color::hex(0x7e93b8)).with_material(0.35, 0.65).with_texture(self.tex.plate, 2.0),
            Mat::Hazard => base.with_color(Color::hex(0xf2f2f2)).with_material(0.1, 0.7).with_texture(self.tex.hazard, 1.5),
            Mat::Brushed => base.with_color(Color::hex(0xd6dce6)).with_material(0.85, 0.38).with_texture(self.tex.brushed, 2.0),
            Mat::Rubber => base.with_color(Color::hex(0xff6a1a)).with_material(0.0, 0.85),
            Mat::Belt => base.with_color(Color::hex(0x20242c)).with_material(0.0, 0.9),
            Mat::Crate => base.with_color(Color::hex(0xffffff)).with_material(0.0, 0.8).with_texture(self.tex.crate_, 0.0),
            Mat::Glow(c) => base.with_color(Color::hex(0x151a22)).with_material(0.0, 0.5).with_emissive(c),
            Mat::Gold => base.with_color(Color::hex(0xffc933)).with_material(1.0, 0.25).with_emissive(Color::rgb(0.9, 0.55, 0.05)),
            Mat::Silver => base.with_color(Color::hex(0xd8e0ee)).with_material(1.0, 0.3).with_emissive(Color::rgb(0.05, 0.12, 0.22)),
        }
    }

    /// Вид без коллайдера.
    #[allow(clippy::too_many_arguments)]
    pub fn visual(&mut self, name: &str, parent: GameObjectId, pos: Vec3, rot: Quat, size: Vec3, shape: MeshShape, mat: Mat) -> GameObjectId {
        let id = self.node(name, parent, pos, rot, size);
        let renderer = self.renderer(shape, mat);
        self.add(id, renderer);
        id
    }

    /// Неподвижный ящик-блок с коллайдером (мировые позиция и поворот, дети корня).
    pub fn block(&mut self, name: &str, center: Vec3, size: Vec3, rot: Quat, mat: Mat) -> GameObjectId {
        let root = self.root;
        let id = self.visual(name, root, center, rot, size, MeshShape::Cube, mat);
        let friction = if matches!(mat, Mat::Hazard) { 0.5 } else { 0.9 };
        self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_material(friction, 0.0));
        self.grow(center, size.max_element() * 0.5);
        id
    }

    /// Блок, который двигают скрипты (кинематическое тело).
    pub fn kinematic_block(&mut self, name: &str, parent: GameObjectId, center: Vec3, rot: Quat, size: Vec3, mat: Mat) -> GameObjectId {
        let id = self.visual(name, parent, center, rot, size, MeshShape::Cube, mat);
        self.add(id, Rigidbody::kinematic());
        self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_material(0.9, 0.0));
        self.grow(center, size.max_element() * 0.5);
        id
    }

    /// Точка маршрута для бота и для подсказок.
    pub fn wp(&mut self, pos: Vec3, kind: WpKind) {
        self.route.push(Waypoint { pos, kind });
    }

    // ------------------------------------------------------------------
    // Дорожка
    // ------------------------------------------------------------------

    /// Плита между двумя точками верхней поверхности (плоская или наклонная) + бортики.
    fn slab_between(&mut self, name: &str, a: Vec3, b: Vec3, yaw: f32, width: f32, lane: Lane) -> Seg {
        let delta = b - a;
        let hlen = Vec2::new(delta.x, delta.z).length();
        let length = delta.length();
        let pitch = delta.y.atan2(hlen.max(1e-4));
        let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
        let normal = rot * Vec3::Y;
        let center = (a + b) * 0.5 - normal * (THICK * 0.5);
        let mat = lane.mat;
        self.block(name, center, Vec3::new(width, THICK, length), rot, mat);
        if lane.rails {
            let right = rot * Vec3::X;
            for side in [-1.0f32, 1.0] {
                let c = (a + b) * 0.5 + right * (side * (width * 0.5 - 0.15)) + normal * 0.45;
                self.block("Rail", c, Vec3::new(0.3, 0.9, length), rot, Mat::Hazard);
            }
        }
        Seg {
            start: a,
            yaw,
            len: hlen,
            width,
            slope: if hlen > 1e-4 { delta.y / hlen } else { 0.0 },
        }
    }

    /// Ровный участок длиной `len`.
    pub fn straight(&mut self, tr: &mut Track, len: f32, width: f32, lane: Lane) -> Seg {
        let end = tr.pos + tr.fwd() * len;
        let name = self.next_name("Floor");
        let seg = self.slab_between(&name, tr.pos, end, tr.yaw, width, lane);
        tr.pos = end;
        self.wp(end, WpKind::Go);
        seg
    }

    /// Наклонный участок: `len` по горизонтали, `dy` вверх (минус — вниз).
    pub fn ramp(&mut self, tr: &mut Track, len: f32, dy: f32, width: f32, lane: Lane) -> Seg {
        let end = tr.pos + tr.fwd() * len + Vec3::Y * dy;
        let name = self.next_name("Ramp");
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
        let name = self.next_name("Corner");
        let seg = self.slab_between(&name, tr.pos, end, tr.yaw, w, Lane { rails: false, ..lane });
        let center = tr.pos + old_fwd * (w * 0.5);
        tr.yaw += deg.to_radians();
        let new_fwd = tr.fwd();
        if lane.rails {
            // Бортики на сторонах, где нет ни входа, ни выхода.
            let sides = [old_fwd, old_right, -old_right, -old_fwd];
            for dir in sides {
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
    // Механизмы
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    /// Ездящая платформа. `size` = (ширина, толщина, длина); `top` — центр верхней поверхности
    /// в начальной точке; `travel` — смещение конечной точки.
    pub fn platform(&mut self, top: Vec3, yaw: f32, size: Vec3, travel: Vec3, period: f32, pause: f32, phase: f32) -> GameObjectId {
        let root = self.root;
        let center = top - Vec3::Y * (size.y * 0.5);
        let id = self.kinematic_block("Platform", root, center, Quat::from_rotation_y(yaw), size, Mat::Plate);
        if let Some(object) = self.world.scene.get_mut(id) {
            object.tag = "Mover".to_string();
        }
        self.cpp(
            id,
            "MovingPlatform",
            &[
                ("travel", Value::Vec3(travel.x, travel.y, travel.z)),
                ("period", Value::Float(period)),
                ("pause", Value::Float(pause)),
                ("phase", Value::Float(phase)),
            ],
        );
        // Жёлтая окантовка спереди и сзади — платформу видно издалека.
        for side in [-1.0f32, 1.0] {
            self.visual("Edge", id, Vec3::new(0.0, 0.5 + 0.02, side * 0.46), Quat::IDENTITY, Vec3::new(0.98, 0.06, 0.08), MeshShape::Cube, Mat::Glow(Color::rgb(2.4, 1.6, 0.2)));
        }
        self.grow(top + travel, size.max_element() * 0.5);
        id
    }

    /// Метла: узел в точке `floor` (на полу), `n_bars` перекладин длиной `length` каждая
    /// (1 — одна палка через центр, 2 — крест).
    pub fn sweeper(&mut self, floor: Vec3, length: f32, bars: u32, speed_deg: f32, swing_deg: f32, phase_deg: f32) -> GameObjectId {
        let root = self.root;
        let height = 0.9;
        let hub = self.node("Sweeper", root, floor, Quat::IDENTITY, Vec3::ONE);
        // Колонна в центре: статическая, чтобы не было щели под кинематикой.
        let column = self.visual("Column", hub, Vec3::new(0.0, height * 0.5 + 0.1, 0.0), Quat::IDENTITY, Vec3::new(0.9, height + 0.2, 0.9), MeshShape::Cylinder, Mat::Brushed);
        self.add(column, Collider::cylinder(0.5, 1.0));
        for k in 0..bars {
            let angle = 180.0 * k as f32 / bars as f32;
            let bar = self.kinematic_block("Arm", hub, Vec3::new(0.0, height * 0.55, 0.0), Quat::from_rotation_y(angle.to_radians()), Vec3::new(length, 0.55, 0.45), Mat::Hazard);
            self.cpp(bar, "SweeperArm", &[]);
            // Светящиеся наконечники.
            for side in [-1.0f32, 1.0] {
                self.visual("Tip", bar, Vec3::new(side * 0.5, 0.0, 0.0), Quat::IDENTITY, Vec3::new(0.06, 1.15, 1.2), MeshShape::Cube, Mat::Glow(Color::rgb(3.0, 0.5, 0.15)));
            }
        }
        self.cpp(
            hub,
            "Sweeper",
            &[
                ("speed_deg", Value::Float(speed_deg)),
                ("swing_deg", Value::Float(swing_deg)),
                ("phase_deg", Value::Float(phase_deg)),
            ],
        );
        self.grow(floor, length * 0.5);
        hub
    }

    /// Конвейер: `top` — центр верхней поверхности, `yaw` — куда смотрит «вперёд» ленты,
    /// `speed` м/с (минус — против `yaw`).
    pub fn conveyor(&mut self, top: Vec3, yaw: f32, len: f32, width: f32, speed: f32) -> GameObjectId {
        let root = self.root;
        let thick = 0.4;
        let center = top - Vec3::Y * (thick * 0.5);
        let id = self.node("Conveyor", root, center, Quat::from_rotation_y(yaw), Vec3::ONE);
        let belt = self.visual("Belt", id, Vec3::ZERO, Quat::IDENTITY, Vec3::new(width, thick, len), MeshShape::Cube, Mat::Belt);
        self.add(belt, Collider::cuboid(1.0, 1.0, 1.0).with_material(0.4, 0.0));
        // Борта по краям.
        for side in [-1.0f32, 1.0] {
            let rail = self.visual("Side", id, Vec3::new(side * (width * 0.5 + 0.15), 0.05, 0.0), Quat::IDENTITY, Vec3::new(0.3, thick + 0.1, len), MeshShape::Cube, Mat::Hazard);
            self.add(rail, Collider::cuboid(1.0, 1.0, 1.0).with_material(0.3, 0.0));
        }
        // Бегущие планки — направление движения видно сразу.
        let spacing = 1.2;
        let n = (len / spacing).ceil() as u32;
        let tint = if speed >= 0.0 { Color::rgb(2.2, 1.5, 0.2) } else { Color::rgb(2.4, 0.5, 0.25) };
        for i in 0..n {
            let z = len * 0.5 - i as f32 * spacing;
            self.visual("Slat", id, Vec3::new(0.0, thick * 0.5 + 0.03, z), Quat::IDENTITY, Vec3::new(width - 0.1, 0.06, 0.28), MeshShape::Cube, Mat::Glow(tint));
        }
        self.cpp(
            id,
            "Conveyor",
            &[
                ("speed", Value::Float(speed)),
                ("size", Value::Vec3(width, thick, len)),
                ("slat_spacing", Value::Float(spacing)),
            ],
        );
        self.grow(top, len.max(width) * 0.5);
        id
    }

    /// Вентилятор: `base` — центр основания, `rot` — поворот (струя идёт вдоль его «вверх»).
    pub fn fan(&mut self, base: Vec3, rot: Quat, height: f32, radius: f32, lift: f32) -> GameObjectId {
        let root = self.root;
        let id = self.node("Fan", root, base, rot, Vec3::ONE);
        // Решётка основания и ротор.
        self.visual("Grille", id, Vec3::new(0.0, 0.04, 0.0), Quat::IDENTITY, Vec3::new(radius * 1.7, 0.08, radius * 1.7), MeshShape::Cylinder, Mat::Brushed);
        self.visual("Ring", id, Vec3::new(0.0, 0.1, 0.0), Quat::IDENTITY, Vec3::new(radius * 1.9, 0.06, radius * 1.9), MeshShape::Cylinder, Mat::Glow(Color::rgb(0.2, 1.4, 2.6)));
        let rotor = self.node("Rotor", id, Vec3::new(0.0, 0.16, 0.0), Quat::IDENTITY, Vec3::ONE);
        for k in 0..3 {
            let a = (k as f32) * std::f32::consts::PI / 3.0;
            self.visual("Blade", rotor, Vec3::ZERO, Quat::from_rotation_y(a), Vec3::new(radius * 1.5, 0.05, 0.28), MeshShape::Cube, Mat::Silver);
        }
        // Струя воздуха: светящиеся частицы вверх.
        let flow = ParticleSystem {
            emission_rate: 38.0,
            lifetime: Vec2::new(0.5, 0.8),
            start_speed: Vec2::new(height * 0.9, height * 1.3),
            start_size: Vec2::new(0.06, 0.12),
            start_color: Color::rgba(0.55, 1.6, 2.4, 0.8),
            start_color_max: Color::rgba(0.3, 1.0, 2.0, 0.6),
            gravity: Vec3::ZERO,
            drag: 0.2,
            color_over_lifetime: vec![Color::WHITE.with_alpha(0.0), Color::WHITE, Color::WHITE.with_alpha(0.0)],
            shape: EmitterShape::Circle,
            shape_size: Vec3::new(radius * 0.8, 0.0, 0.0),
            cone_angle: 4.0,
            flat: false,
            blend: ParticleBlend::Additive,
            velocity_stretch: 0.12,
            max_particles: 120,
            seed: 11,
            ..ParticleSystem::default()
        };
        let emitter = self.node("Flow", id, Vec3::new(0.0, 0.2, 0.0), Quat::IDENTITY, Vec3::ONE);
        self.add(emitter, flow);
        self.cpp(
            id,
            "Fan",
            &[
                ("lift", Value::Float(lift)),
                ("height", Value::Float(height)),
                ("radius", Value::Float(radius)),
            ],
        );
        self.grow(base + rot * Vec3::Y * height, radius + 1.0);
        id
    }

    /// Пресс: `top_center` — центр головы в верхнем положении, `head` — габариты головы.
    pub fn crusher(&mut self, head_up: Vec3, head: Vec3, stroke: f32, period: f32, wait_up: f32, phase: f32) -> GameObjectId {
        let root = self.root;
        let id = self.node("Crusher", root, head_up, Quat::IDENTITY, Vec3::ONE);
        let head_id = self.kinematic_block("Head", id, Vec3::ZERO, Quat::IDENTITY, head, Mat::Hazard);
        // Светящаяся нижняя кромка.
        self.visual("Lip", head_id, Vec3::new(0.0, -0.52, 0.0), Quat::IDENTITY, Vec3::new(0.94, 0.04, 0.94), MeshShape::Cube, Mat::Glow(Color::rgb(3.0, 0.3, 0.1)));
        let shaft_len = 9.0;
        self.visual("Shaft", id, Vec3::new(0.0, head.y * 0.5 + shaft_len * 0.5, 0.0), Quat::IDENTITY, Vec3::new(0.7, shaft_len, 0.7), MeshShape::Cylinder, Mat::Brushed);
        // Корпус над ходом поршня.
        let top = head.y * 0.5 + shaft_len + 0.5;
        self.visual("Housing", id, Vec3::new(0.0, top, 0.0), Quat::IDENTITY, Vec3::new(head.x + 0.8, 1.2, head.z + 0.8), MeshShape::Cube, Mat::PlateDark);
        self.cpp(
            id,
            "Crusher",
            &[
                ("stroke", Value::Float(stroke)),
                ("period", Value::Float(period)),
                ("wait_up", Value::Float(wait_up)),
                ("phase", Value::Float(phase)),
            ],
        );
        self.grow(head_up, head.max_element());
        id
    }

    /// Бампер-«грибок».
    pub fn bumper(&mut self, floor: Vec3, radius: f32) -> GameObjectId {
        let root = self.root;
        let h = 0.9;
        let id = self.visual("Bumper", root, floor + Vec3::Y * (h * 0.5), Quat::IDENTITY, Vec3::new(radius * 2.0, h, radius * 2.0), MeshShape::Cylinder, Mat::Rubber);
        self.add(id, Collider::cylinder(0.5, 1.0).with_material(0.2, 0.9));
        self.visual("Cap", id, Vec3::new(0.0, 0.5, 0.0), Quat::IDENTITY, Vec3::new(0.7, 0.08, 0.7), MeshShape::Cylinder, Mat::Glow(Color::rgb(3.0, 1.2, 0.2)));
        self.cpp(id, "Bumper", &[]);
        self.grow(floor, radius + 1.0);
        id
    }

    /// Хрупкая плита; `top` — центр верхней поверхности.
    pub fn tile(&mut self, top: Vec3, yaw: f32, w: f32, l: f32) -> GameObjectId {
        let root = self.root;
        let size = Vec3::new(w, 0.45, l);
        let id = self.kinematic_block("Tile", root, top - Vec3::Y * (size.y * 0.5), Quat::from_rotation_y(yaw), size, Mat::PlateDark);
        self.visual("Frame", id, Vec3::new(0.0, 0.5 + 0.01, 0.0), Quat::IDENTITY, Vec3::new(0.96, 0.03, 0.96), MeshShape::Cube, Mat::Glow(Color::rgb(0.9, 0.55, 0.1)));
        self.cpp(id, "CollapsingTile", &[]);
        id
    }

    /// Ускоряющая плита: стрелки вдоль `yaw`.
    pub fn booster(&mut self, floor: Vec3, yaw: f32, w: f32, l: f32, speed: f32) -> GameObjectId {
        let root = self.root;
        let id = self.node("Booster", root, floor, Quat::from_rotation_y(yaw), Vec3::ONE);
        self.add(id, Collider::cuboid(w, 1.6, l).trigger());
        // collider.offset поднимает триггер над плитой.
        if let Some(c) = self.world.scene.get_mut(id).and_then(|o| o.get_mut::<Collider>()) {
            c.offset = Vec3::new(0.0, 0.8, 0.0);
        }
        self.visual("Pad", id, Vec3::new(0.0, 0.03, 0.0), Quat::IDENTITY, Vec3::new(w, 0.06, l), MeshShape::Cube, Mat::Hazard);
        let arrows = self.node("Arrows", id, Vec3::new(0.0, 0.08, 0.0), Quat::IDENTITY, Vec3::ONE);
        let shaft = self.renderer(MeshShape::Cube, Mat::Glow(Color::rgb(0.6, 0.45, 0.05)));
        self.add(arrows, shaft);
        // «Стрелка»: ствол (сам Arrows) и два крыла-ребёнка.
        if let Some(t) = self.world.scene.get_mut(arrows) {
            t.transform.scale = Vec3::new(0.35, 0.02, l * 0.55);
        }
        for side in [-1.0f32, 1.0] {
            let wing = self.node("Wing", arrows, Vec3::new(side * 1.0, 0.0, -0.32), Quat::from_rotation_y(side * 0.7), Vec3::new(1.5, 1.0, 0.3));
            let r = self.renderer(MeshShape::Cube, Mat::Glow(Color::rgb(0.6, 0.45, 0.05)));
            self.add(wing, r);
        }
        self.cpp(id, "Booster", &[("speed", Value::Float(speed))]);
        self.grow(floor, w.max(l));
        id
    }

    /// Шестерёнка-подбор; `golden` — золотая (одна на цех).
    pub fn gear(&mut self, pos: Vec3, golden: bool) -> GameObjectId {
        let root = self.root;
        let id = self.node(if golden { "Golden Gear" } else { "Gear" }, root, pos, Quat::IDENTITY, Vec3::ONE);
        self.add(id, Collider::sphere(if golden { 0.8 } else { 0.62 }).trigger());
        let k = if golden { 1.35 } else { 1.0 };
        let mat = if golden { Mat::Gold } else { Mat::Silver };
        let body = self.node("Body", id, Vec3::ZERO, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), Vec3::ONE);
        self.visual("Disc", body, Vec3::ZERO, Quat::IDENTITY, Vec3::new(0.62 * k, 0.12 * k, 0.62 * k), MeshShape::Cylinder, mat);
        self.visual("Hub", body, Vec3::ZERO, Quat::IDENTITY, Vec3::new(0.24 * k, 0.2 * k, 0.24 * k), MeshShape::Cylinder, Mat::Glow(if golden { Color::rgb(3.0, 1.8, 0.3) } else { Color::rgb(0.3, 1.2, 2.2) }));
        for t in 0..8 {
            let a = t as f32 * std::f32::consts::TAU / 8.0;
            self.visual("Tooth", body, Vec3::new(a.cos() * 0.36 * k, 0.0, a.sin() * 0.36 * k), Quat::from_rotation_y(-a), Vec3::new(0.16 * k, 0.12 * k, 0.13 * k), MeshShape::Cube, mat);
        }
        self.cpp(id, "GearPickup", &[("golden", Value::Bool(golden))]);
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
        let root = self.root;
        let id = self.node("Checkpoint", root, floor, Quat::from_rotation_y(yaw), Vec3::ONE);
        self.add(id, Collider::cuboid(width, 3.0, 1.0).trigger());
        if let Some(c) = self.world.scene.get_mut(id).and_then(|o| o.get_mut::<Collider>()) {
            c.offset = Vec3::new(0.0, 1.5, 0.0);
        }
        for side in [-1.0f32, 1.0] {
            self.visual("Post", id, Vec3::new(side * (width * 0.5 + 0.2), 1.6, 0.0), Quat::IDENTITY, Vec3::new(0.3, 3.2, 0.3), MeshShape::Cylinder, Mat::Brushed);
        }
        self.visual("Bar", id, Vec3::new(0.0, 3.2, 0.0), Quat::IDENTITY, Vec3::new(width + 0.8, 0.32, 0.32), MeshShape::Cube, Mat::Hazard);
        self.visual("Lamp", id, Vec3::new(0.0, 3.62, 0.0), Quat::IDENTITY, Vec3::splat(0.5), MeshShape::Sphere, Mat::Glow(Color::rgb(3.0, 0.4, 0.2)));
        self.checkpoints += 1;
        let index = self.checkpoints as i32;
        self.cpp(id, "Checkpoint", &[("index", Value::Int(index as i64)), ("yaw_deg", Value::Float(yaw.to_degrees()))]);
        id
    }

    /// Площадка отгрузки.
    pub fn finish(&mut self, floor: Vec3, yaw: f32, width: f32) -> GameObjectId {
        let root = self.root;
        let id = self.node("Finish", root, floor, Quat::from_rotation_y(yaw), Vec3::ONE);
        self.add(id, Collider::cuboid(width, 4.0, width).trigger());
        if let Some(c) = self.world.scene.get_mut(id).and_then(|o| o.get_mut::<Collider>()) {
            c.offset = Vec3::new(0.0, 2.0, 0.0);
        }
        self.visual("Pad", id, Vec3::new(0.0, 0.04, 0.0), Quat::IDENTITY, Vec3::new(width * 0.9, 0.08, width * 0.9), MeshShape::Cylinder, Mat::Glow(Color::rgb(0.2, 2.2, 0.9)));
        self.visual("Ring", id, Vec3::new(0.0, 0.1, 0.0), Quat::IDENTITY, Vec3::new(width * 0.62, 0.1, width * 0.62), MeshShape::Cylinder, Mat::Hazard);
        for side in [-1.0f32, 1.0] {
            self.visual("Pylon", id, Vec3::new(side * (width * 0.5), 1.8, 0.0), Quat::IDENTITY, Vec3::new(0.5, 3.6, 0.5), MeshShape::Cylinder, Mat::Brushed);
            self.visual("Cap", id, Vec3::new(side * (width * 0.5), 3.7, 0.0), Quat::IDENTITY, Vec3::splat(0.6), MeshShape::Sphere, Mat::Glow(Color::rgb(0.3, 2.0, 0.8)));
        }
        self.visual("Beacon", id, Vec3::new(0.0, 3.0, 0.0), Quat::IDENTITY, Vec3::new(0.5, 0.5, 0.5), MeshShape::Cube, Mat::Glow(Color::rgb(0.3, 2.0, 0.8)));
        self.cpp(id, "FinishPad", &[]);
        self.grow(floor, width);
        id
    }

    /// Деревянный… нет, стальной ящик — настоящее динамическое тело.
    pub fn crate_(&mut self, center: Vec3, size: f32) -> GameObjectId {
        let root = self.root;
        let id = self.visual("Crate", root, center, Quat::IDENTITY, Vec3::splat(size), MeshShape::Cube, Mat::Crate);
        self.add(id, Rigidbody { angular_damping: 0.4, linear_damping: 0.05, ..Rigidbody::dynamic() });
        self.add(id, Collider::cuboid(1.0, 1.0, 1.0).with_density(0.45).with_material(0.55, 0.0));
        self.bodies += 1;
        self.grow(center, size);
        id
    }

    /// Качели: доска на оси поперёк хода; под концами — упоры, чтобы доска не переворачивалась.
    pub fn seesaw(&mut self, pivot_top: Vec3, yaw: f32, length: f32, width: f32) -> GameObjectId {
        let root = self.root;
        let rot = Quat::from_rotation_y(yaw);
        let fwd = rot * Vec3::NEG_Z;
        let right = rot * Vec3::X;
        let thick = 0.35;
        let center = pivot_top - Vec3::Y * (thick * 0.5);
        // Доска стартует наклонённой: задний (входной) конец внизу — шар въезжает как на рампу.
        let tilt = 0.25f32.asin();
        let plank_rot = rot * Quat::from_rotation_x(tilt);
        let plank = self.visual("Seesaw", root, center, plank_rot, Vec3::new(width, thick, length), MeshShape::Cube, Mat::Plate);
        self.add(plank, Rigidbody { angular_damping: 1.6, linear_damping: 0.1, ..Rigidbody::dynamic() });
        self.add(plank, Collider::cuboid(1.0, 1.0, 1.0).with_density(0.8).with_material(0.9, 0.0));
        self.visual("Stripe", plank, Vec3::new(0.0, 0.5 + 0.01, 0.0), Quat::IDENTITY, Vec3::new(0.08, 0.03, 0.96), MeshShape::Cube, Mat::Glow(Color::rgb(2.4, 1.6, 0.2)));
        self.add(plank, SeesawRig { axis: right, anchor: center });
        // Опора и упоры: доска наклоняется не больше чем на ~14°.
        self.visual("Pivot", root, center - Vec3::Y * 0.6, rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(0.6, width + 0.6, 0.6), MeshShape::Cylinder, Mat::Brushed);
        let reach = length * 0.5 - 0.7;
        let stop_drop = reach * 0.25 + thick * 0.5 + 0.03;
        for side in [-1.0f32, 1.0] {
            let c = center + fwd * (side * reach) - Vec3::Y * (stop_drop + 0.3);
            self.block("Stop", c, Vec3::new(width, 0.6, 0.8), rot, Mat::Hazard);
        }
        self.bodies += 1;
        self.grow(center, length);
        plank
    }

    /// Лампа-мигалка (светится сильнее 1 — даёт блум).
    pub fn lamp(&mut self, pos: Vec3, color: Color, rate: f32, phase: f32) -> GameObjectId {
        let root = self.root;
        let id = self.visual("Lamp", root, pos, Quat::IDENTITY, Vec3::splat(0.4), MeshShape::Sphere, Mat::Glow(color));
        self.cpp(
            id,
            "Blinker",
            &[
                ("color", Value::Color(color.r, color.g, color.b, 1.0)),
                ("rate", Value::Float(rate)),
                ("phase", Value::Float(phase)),
            ],
        );
        id
    }

    /// Столб с лампой по краю дорожки.
    pub fn lamp_post(&mut self, floor: Vec3, color: Color, phase: f32) {
        let root = self.root;
        self.visual("Post", root, floor + Vec3::Y * 1.1, Quat::IDENTITY, Vec3::new(0.18, 2.2, 0.18), MeshShape::Cylinder, Mat::Brushed);
        self.lamp(floor + Vec3::Y * 2.35, color, 0.5, phase);
    }
}

component! {
    /// Доска качелей: вешает себя на шарнир, когда физика уже знает о теле.
    pub struct SeesawRig {
        /// Ось качания (поперёк хода).
        pub axis: Vec3 = Vec3::X,
        /// Точка оси в мире.
        pub anchor: Vec3 = Vec3::ZERO,
    }
}

impl Component for SeesawRig {
    fn on_start(&mut self, ctx: &mut Ctx) {
        let (id, axis, anchor) = (ctx.id, self.axis, self.anchor);
        if let Some(physics) = ctx.resource_mut::<Physics3D>() {
            physics.hinge_joint(id, None, anchor, axis);
        }
    }
}
