//! Сцены витрины: объекты расставляет детерминированный генератор, результат пишется
//! обычными файлами сцен (`assets/scenes/showcase/*.scene`) — они открываются в редакторе.
//!
//! В файл попадают только отличные от умолчаний поля компонентов, поэтому даже сцена с
//! тысячами деревьев остаётся читаемой. Камеры-закладки — объекты `Shots/<имя>` с
//! компонентом `Camera`: активна только главная (`hero`).

use std::collections::{BTreeMap, HashMap};

use engine::core::serialize::{ComponentData, NodeData, Override, SceneData};
use engine::prelude::*;

use super::geo::Rng;
use super::ground::{self, Field};
use super::kit::{self, Mat};

// ---------------------------------------------------------------------
// Пути и GUID
// ---------------------------------------------------------------------

pub const PALETTE: &str = "showcase/palette.png";
pub const ROVER_GUID: &str = "4984359c82e949f08b21b9e873f58c9f";

pub fn model_path(name: &str) -> String {
    format!("showcase/models/{name}.obj")
}

pub fn material_path(m: Mat) -> String {
    format!("showcase/materials/{}.material", m.name())
}

pub fn prefab_path(name: &str) -> String {
    format!("showcase/prefabs/{name}.prefab")
}

pub fn scene_path(name: &str) -> String {
    format!("scenes/showcase/{name}.scene")
}

/// GUID ассета витрины — хеш его пути: одинаковый при каждой генерации.
pub fn guid(path: &str) -> String {
    let fnv = |seed: u64| {
        let mut h = seed;
        for b in path.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    format!("{:016x}{:016x}", fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142))
}

// ---------------------------------------------------------------------
// Построитель узлов
// ---------------------------------------------------------------------

/// Объект сцены (обёртка над `NodeData`).
pub struct Node(pub NodeData);

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Node(NodeData { name: name.into(), ..NodeData::default() })
    }
    pub fn at(mut self, p: Vec3) -> Self {
        self.0.transform.position = [round(p.x), round(p.y), round(p.z)];
        self
    }
    /// Поворот: углы Эйлера в градусах (как в инспекторе: X — наклон, Y — курс).
    pub fn rot(mut self, x: f32, y: f32, z: f32) -> Self {
        self.0.transform.rotation = [round(x), round(y), round(z)];
        self
    }
    pub fn yaw(self, y: f32) -> Self {
        self.rot(0.0, y, 0.0)
    }
    pub fn scale(mut self, s: f32) -> Self {
        self.0.transform.scale = [round(s); 3];
        self
    }
    pub fn with(mut self, kind: &str, fields: Vec<(&str, Value)>) -> Self {
        let fields: BTreeMap<String, Value> = fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        self.0.components.push(ComponentData { kind: kind.to_string(), fields });
        self
    }
    pub fn child(mut self, node: Node) -> Self {
        self.0.children.push(node.0);
        self
    }
    pub fn children(mut self, nodes: impl IntoIterator<Item = Node>) -> Self {
        self.0.children.extend(nodes.into_iter().map(|n| n.0));
        self
    }
    /// Переопределить поле компонента префаба у этого экземпляра.
    pub fn set(mut self, component: &str, field: &str, value: Value) -> Self {
        self.0.overrides.push(Override { path: String::new(), component: component.to_string(), field: field.to_string(), value });
        self
    }
    pub fn inactive(mut self) -> Self {
        self.0.active = false;
        self
    }
}

fn round(v: f32) -> f32 {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

pub fn f(v: f32) -> Value {
    Value::Float(round(v))
}
pub fn rgb(r: f32, g: f32, b: f32) -> Value {
    Value::Color(round(r), round(g), round(b), 1.0)
}
pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Value {
    Value::Color(round(r), round(g), round(b), round(a))
}
pub fn v2(a: f32, b: f32) -> Value {
    Value::Vec2(round(a), round(b))
}
pub fn v3(a: f32, b: f32, c: f32) -> Value {
    Value::Vec3(round(a), round(b), round(c))
}
pub fn e(name: &str) -> Value {
    Value::Enum(name.to_string())
}

/// Углы камеры / света (наклон, курс в градусах), чтобы «вперёд» (−Z) смотрел по `dir`.
pub fn aim(dir: Vec3) -> (f32, f32) {
    let d = dir.normalize();
    (d.y.clamp(-1.0, 1.0).asin().to_degrees(), (-d.x).atan2(-d.z).to_degrees())
}

/// Курс, при котором «лицо» модели (−Z) смотрит из `from` на `to` (по горизонтали).
pub fn face(from: Vec3, to: Vec3) -> f32 {
    aim(Vec3::new(to.x - from.x, 0.0, to.z - from.z)).1
}

/// Модели набора: какие у каждой части (по ним — список материалов).
pub struct Kit {
    parts: HashMap<&'static str, Vec<Mat>>,
}

impl Kit {
    pub fn new() -> Self {
        let parts = kit::all()
            .into_iter()
            .chain(ground::all())
            .map(|m| (m.name, m.parts()))
            .collect();
        Self { parts }
    }

    /// `MeshRenderer` модели набора с общими материалами по частям.
    pub fn mesh(&self, name: &str) -> Vec<(&'static str, Value)> {
        let parts = self.parts.get(name).unwrap_or_else(|| panic!("no kit model `{name}`"));
        vec![
            ("mesh", Value::Asset(guid(&model_path(name)))),
            ("materials", Value::List(parts.iter().map(|m| Value::Asset(guid(&material_path(*m)))).collect())),
        ]
    }

    /// Объект с моделью набора.
    pub fn object(&self, label: impl Into<String>, model: &str, p: Vec3, yaw: f32, scale: f32) -> Node {
        assert!(self.parts.contains_key(model), "no kit model `{model}`");
        let mut node = Node::new(label).at(p).yaw(yaw).scale(scale);
        node.0.prefab = Some(prefab_path(model));
        node
    }

    /// То же с оттенком (переопределение поля `tint` префаба; пачки не разбиваются).
    pub fn tinted(&self, label: impl Into<String>, model: &str, p: Vec3, yaw: f32, scale: f32, tint: Color) -> Node {
        self.object(label, model, p, yaw, scale).set("MeshRenderer", "tint", rgb(tint.r, tint.g, tint.b))
    }

    /// Префаб модели: объект с `MeshRenderer` и общими материалами.
    pub fn prefab(&self, model: &str) -> NodeData {
        Node::new(model).with("MeshRenderer", self.mesh(model)).0
    }
}

/// Камера-закладка.
pub fn shot(name: &str, eye: Vec3, target: Vec3, fov: f32, sky: bool, clear: Color) -> Node {
    let (pitch, yaw) = aim(target - eye);
    let mut fields = vec![
        ("projection", e("Perspective")),
        ("fov_deg", f(fov)),
        ("near", f(0.2)),
        ("far", f(4000.0)),
        ("background", e(if sky { "Sky" } else { "SolidColor" })),
    ];
    if !sky {
        fields.push(("clear_color", rgb(clear.r, clear.g, clear.b)));
    }
    let node = Node::new(name).at(eye).rot(pitch, yaw, 0.0).with("Camera", fields);
    if name == "hero" { node } else { node.inactive() }
}

/// Направленный свет («солнце», «луна»), летящий вдоль `dir`.
pub fn sun(name: &str, dir: Vec3, color: Color, intensity: f32, shadows: bool) -> Node {
    let (pitch, yaw) = aim(dir);
    Node::new(name).rot(pitch, yaw, 0.0).with(
        "Light",
        vec![
            ("color", rgb(color.r, color.g, color.b)),
            ("intensity", f(intensity)),
            ("shadows", Value::Bool(shadows)),
        ],
    )
}

pub fn point_light(name: &str, p: Vec3, color: Color, intensity: f32, range: f32) -> Node {
    Node::new(name).at(p).with(
        "Light",
        vec![
            ("kind", e("Point")),
            ("color", rgb(color.r, color.g, color.b)),
            ("intensity", f(intensity)),
            ("range", f(range)),
        ],
    )
}

/// Разбросать точки с минимальным расстоянием `spacing` по условию `accept(x, z, rng)`.
pub fn scatter(
    rng: &mut Rng,
    count: usize,
    spacing: f32,
    area: (Vec2, Vec2),
    taken: &mut Vec<Vec2>,
    mut accept: impl FnMut(f32, f32, &mut Rng) -> bool,
) -> Vec<Vec2> {
    let cell = spacing.max(0.1);
    let mut grid: HashMap<(i32, i32), Vec<Vec2>> = HashMap::new();
    let key = |p: Vec2| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32);
    for p in taken.iter() {
        grid.entry(key(*p)).or_default().push(*p);
    }
    let mut out = Vec::new();
    let mut attempts = 0;
    while out.len() < count && attempts < count * 40 {
        attempts += 1;
        let p = Vec2::new(rng.range(area.0.x, area.1.x), rng.range(area.0.y, area.1.y));
        let (kx, kz) = key(p);
        let near = (-1..=1).any(|dx| {
            (-1..=1).any(|dz| grid.get(&(kx + dx, kz + dz)).is_some_and(|v| v.iter().any(|q| q.distance(p) < spacing)))
        });
        if near || !accept(p.x, p.y, rng) {
            continue;
        }
        grid.entry((kx, kz)).or_default().push(p);
        out.push(p);
    }
    taken.extend(out.iter().copied());
    out
}

fn on(field: &Field, x: f32, z: f32) -> Vec3 {
    Vec3::new(x, field.height(x, z), z)
}

fn scene(name: &str, objects: Vec<Node>) -> SceneData {
    SceneData { name: name.to_string(), objects: objects.into_iter().map(|n| n.0).collect() }
}

/// Все сцены витрины: (имя файла, данные).
pub fn all() -> Vec<(&'static str, SceneData)> {
    let kit = Kit::new();
    vec![
        ("island", island(&kit)),
        ("clearing", clearing(&kit)),
        ("workshop", workshop(&kit)),
        ("valley", valley(&kit)),
    ]
}

// ---------------------------------------------------------------------
// Компоненты витрины (tour.rs) в файле сцены
// ---------------------------------------------------------------------

fn spin(axis: Vec3, speed: f32) -> (&'static str, Vec<(&'static str, Value)>) {
    ("Spin", vec![("axis", v3(axis.x, axis.y, axis.z)), ("speed", f(speed))])
}

fn smoke(seed: i64) -> Vec<(&'static str, Value)> {
    vec![
        ("emission_rate", f(5.0)),
        ("max_particles", Value::Int(80)),
        ("lifetime", v2(5.0, 7.5)),
        ("start_speed", v2(0.5, 0.9)),
        ("start_size", v2(0.5, 0.8)),
        ("start_rotation", v2(0.0, 360.0)),
        ("rotation_speed", v2(-15.0, 15.0)),
        ("start_color", rgba(0.82, 0.78, 0.76, 0.45)),
        ("start_color_max", rgba(0.66, 0.63, 0.62, 0.4)),
        ("gravity", v3(0.35, 0.25, 0.15)),
        ("drag", f(0.4)),
        ("color_over_lifetime", Value::List(vec![rgba(1.0, 1.0, 1.0, 0.0), rgba(1.0, 1.0, 1.0, 1.0), rgba(1.0, 1.0, 1.0, 0.0)])),
        ("size_over_lifetime", Value::List(vec![f(0.6), f(2.2), f(3.4)])),
        ("shape", e("Cone")),
        ("shape_size", v3(0.15, 0.15, 0.15)),
        ("cone_angle", f(10.0)),
        ("seed", Value::Int(seed)),
    ]
}

// ---------------------------------------------------------------------
// 1. Остров на закате
// ---------------------------------------------------------------------

/// Главная камера острова: с юго-востока, пирс — ведущая линия слева внизу.
const HERO_EYE: Vec3 = Vec3::new(16.0, 11.0, 64.0);
/// Камера на площади деревни.
const VILLAGE_EYE: Vec3 = Vec3::new(-3.5, 5.2, 19.5);

fn island(kit: &Kit) -> SceneData {
    let field = ground::island_field();
    let h = |x: f32, z: f32| field.height(x, z);
    let mut rng = Rng::new(2024);
    // Места камер-закладок свободны от деревьев.
    let mut taken: Vec<Vec2> = Vec::new();
    for eye in [HERO_EYE, VILLAGE_EYE] {
        for k in 0..12 {
            let a = k as f32 * 0.5236;
            taken.push(Vec2::new(eye.x + a.cos() * 3.0, eye.z + a.sin() * 3.0));
        }
    }
    let reserve = |taken: &mut Vec<Vec2>, p: Vec3, r: f32| {
        // Занятое место: кольцо точек, чтобы разброс не залез под постройку.
        taken.push(Vec2::new(p.x, p.z));
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            taken.push(Vec2::new(p.x + a.cos() * r * 0.6, p.z + a.sin() * r * 0.6));
        }
    };

    let sun_dir = Vec3::new(0.9, -0.22, -0.38).normalize();
    let lighting = Node::new("Lighting")
        .with(
            "Environment",
            vec![
                ("ambient", f(0.26)),
                ("fog_density", f(0.004)),
                ("exposure", f(1.05)),
                ("bloom", f(0.1)),
                ("vignette", f(0.28)),
                ("shadow_distance", f(110.0)),
            ],
        )
        .with("Fog", vec![("start", f(55.0)), ("height", f(0.0)), ("height_falloff", f(0.03))])
        .with("SkyColors", vec![("zenith", rgb(0.1, 0.18, 0.42)), ("horizon", rgb(0.95, 0.52, 0.3))])
        .with("SoftShadows", vec![])
        .with("Sky", vec![("clouds", f(0.3)), ("cloud_color", rgb(1.0, 0.86, 0.78))])
        .with("AmbientOcclusion", vec![])
        .with("PostQuality", vec![])
        .with(
            "ColorGrading",
            vec![
                ("temperature", f(0.12)),
                ("contrast", f(1.08)),
                ("saturation", f(1.08)),
                ("shadows", rgb(0.35, 0.12, 0.45)),
            ],
        )
        .child(sun("Sun", sun_dir, Color::rgb(1.0, 0.68, 0.42), 4.2, true));

    let mut world = vec![
        lighting,
        kit.object("Island", "island", Vec3::ZERO, 0.0, 1.0),
        Node::new("Sea").with(
            "Water",
            vec![("deep_color", rgb(0.015, 0.08, 0.1)), ("opacity", f(0.8)), ("waves", f(0.7))],
        ),
    ];

    // --- деревня вокруг колодца ---
    let (vx, vz, _) = ground::VILLAGE;
    let square = on(&field, vx, vz);
    let mut village = Node::new("Village");
    village = village.child(kit.object("Well", "well", square, 20.0, 1.0));
    reserve(&mut taken, square, 2.5);
    let houses = [
        ("Cottage", "cottage", -9.0, -3.5),
        ("Cottage (2)", "cottage", 5.5, -2.0),
        ("Tall House", "cottage_tall", -13.0, 6.5),
        ("Cottage (3)", "cottage", 6.0, 8.0),
    ];
    for (name, model, dx, dz) in houses {
        let p = on(&field, vx + dx, vz + dz);
        let yaw = face(p, square) + rng.range(-8.0, 8.0);
        village = village.child(kit.object(name, model, p, yaw, 1.0));
        reserve(&mut taken, p, 7.0);
        // Дым из трубы.
        let chimney = if model == "cottage" { Vec3::new(1.4, 5.3, 0.7) } else { Vec3::new(-1.3, 8.8, 1.0) };
        let top = p + Quat::from_rotation_y(yaw.to_radians()) * chimney;
        village = village.child(
            Node::new(format!("{name} Smoke")).at(top).with("ParticleSystem", smoke(rng.index(1000) as i64 + 1)),
        );
    }
    let stall = on(&field, vx + 1.5, vz + 4.5);
    village = village.child(kit.object("Market Stall", "market_stall", stall, face(stall, Vec3::new(-30.0, 0.0, 48.0)), 1.0));
    reserve(&mut taken, stall, 3.0);
    for (i, (dx, dz)) in [(3.5, 6.5), (-1.0, 7.0)].into_iter().enumerate() {
        let p = on(&field, vx + dx, vz + dz);
        village = village.child(kit.object(format!("Crate {i}"), "crate", p, rng.range(0.0, 90.0), 0.9));
    }
    let p = on(&field, vx + 4.6, vz + 5.6);
    village = village.child(kit.object("Barrel", "barrel", p, 0.0, 1.0));
    let p = on(&field, vx - 0.3, vz + 6.2);
    village = village.child(kit.object("Sack", "sack", p, 30.0, 1.0));
    let cart = on(&field, vx - 4.5, vz + 4.0);
    village = village.child(kit.object("Cart", "cart", cart, 115.0, 1.0));
    reserve(&mut taken, cart, 2.5);
    for (i, (dx, dz)) in [(-3.0, -1.5), (3.0, 2.5), (-5.5, 9.5)].into_iter().enumerate() {
        let p = on(&field, vx + dx, vz + dz);
        village = village.child(kit.object(format!("Lantern {i}"), "lantern_post", p, rng.range(0.0, 360.0), 1.0));
        reserve(&mut taken, p, 1.0);
    }
    // Огород за забором.
    for i in 0..6 {
        let p = on(&field, vx + 9.0 + i as f32 * 2.0, vz + 2.5);
        village = village.child(kit.object(format!("Fence {i}"), "fence", p, 0.0, 1.0));
        reserve(&mut taken, p, 1.5);
    }
    for i in 0..3 {
        let p = on(&field, vx + 20.0, vz + 1.5 - i as f32 * 2.0);
        village = village.child(kit.object(format!("Fence {}", i + 6), "fence", p - Vec3::X, 90.0, 1.0));
    }
    let sign = on(&field, vx - 6.0, vz + 12.5);
    village = village.child(kit.object("Signpost", "signpost", sign, 30.0, 1.0));
    let bench = on(&field, vx + 1.5, vz - 3.0);
    village = village.child(kit.object("Bench", "bench", bench, face(bench, square) + 180.0, 1.0));
    world.push(village);

    // --- мельница на холме ---
    let (hx, hz) = ground::HILL;
    let mill = on(&field, hx, hz);
    let camera_eye = HERO_EYE;
    let mill_yaw = face(mill, camera_eye) - 25.0;
    let hub = mill + Quat::from_rotation_y(mill_yaw.to_radians()) * Vec3::new(0.0, kit::WINDMILL_HUB, kit::WINDMILL_HUB_Z);
    let (kind, fields) = spin(Vec3::Z, 22.0);
    world.push(
        Node::new("Windmill")
            .child(kit.object("Tower", "windmill", mill, mill_yaw, 1.0))
            .child(kit.object("Sails", "windmill_sails", hub, mill_yaw, 1.0).with(kind, fields)),
    );
    reserve(&mut taken, mill, 4.0);

    // --- маяк на восточном обрыве ---
    let light = on(&field, 30.0, 2.0);
    world.push(kit.object("Lighthouse", "lighthouse", light, 0.0, 1.0));
    reserve(&mut taken, light, 4.0);

    // --- пирс и лодка на южном берегу ---
    let mut pier_start = on(&field, -9.0, 20.0);
    for step in 0..200 {
        let z = 20.0 + step as f32 * 0.5;
        pier_start = on(&field, -9.0, z);
        if pier_start.y < 0.7 {
            break;
        }
    }
    let pier_p = Vec3::new(pier_start.x, 0.0, pier_start.z - 1.0);
    world.push(kit.object("Pier", "pier", pier_p, 180.0, 1.0));
    reserve(&mut taken, pier_p, 3.0);
    let boat = pier_p + Vec3::new(2.4, -0.18, 6.0);
    world.push(
        kit.object("Boat", "boat", boat, 12.0, 1.0)
            .with("Bob", vec![("height", f(0.08)), ("roll", f(2.5)), ("speed", f(0.9))]),
    );

    // --- растительность ---
    let mut nature = Node::new("Nature");
    let area = (Vec2::splat(-50.0), Vec2::splat(50.0));
    let slope = |x: f32, z: f32| field.normal(x, z).y;
    // Пальмы у воды.
    for (i, p) in scatter(&mut rng, 16, 5.0, area, &mut taken, |x, z, _| {
        let y = h(x, z);
        (0.35..1.6).contains(&y) && slope(x, z) > 0.85
    })
    .into_iter()
    .enumerate()
    {
        // Пальма клонится к морю (модель клонится к +X).
        let out = p.normalize_or_zero();
        let lean = (-out.y).atan2(out.x).to_degrees() + rng.range(-25.0, 25.0);
        nature = nature.child(kit.object(format!("Palm {i}"), "palm", on(&field, p.x, p.y), lean, rng.range(0.85, 1.25)));
    }
    // Ели на холме и обрыве.
    for (i, p) in scatter(&mut rng, 40, 3.2, area, &mut taken, |x, z, _| h(x, z) > 5.0 && slope(x, z) > 0.7) .into_iter().enumerate() {
        let model = if i % 3 == 0 { "pine_small" } else { "pine" };
        nature = nature.child(kit.tinted(format!("Pine {i}"), model, on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(0.8, 1.3), Color::rgb(rng.range(0.85, 1.05), 1.0, rng.range(0.85, 1.0))));
    }
    // Лиственные вокруг деревни.
    let trees = ["tree_round", "tree_autumn", "birch", "tree_round"];
    for (i, p) in scatter(&mut rng, 38, 4.5, area, &mut taken, |x, z, _| {
        let y = h(x, z);
        (1.6..5.5).contains(&y) && slope(x, z) > 0.8
    })
    .into_iter()
    .enumerate()
    {
        let model = trees[i % trees.len()];
        nature = nature.child(kit.object(format!("Tree {i}"), model, on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(0.8, 1.2)));
    }
    // Камни: на берегу и обрывах.
    for (i, p) in scatter(&mut rng, 40, 2.5, area, &mut taken, |x, z, _| {
        let y = h(x, z);
        (y > -0.6 && y < 0.6) || slope(x, z) < 0.8
    })
    .into_iter()
    .enumerate()
    {
        let model = if i % 4 == 0 { "rock_large" } else { "rock_small" };
        let s = if i % 4 == 0 { rng.range(0.5, 0.9) } else { rng.range(0.7, 1.6) };
        nature = nature.child(kit.object(format!("Rock {i}"), model, on(&field, p.x, p.y) - Vec3::Y * 0.15, rng.range(0.0, 360.0), s));
    }
    // Кусты, цветы, трава.
    let grassy = |x: f32, z: f32| {
        let y = h(x, z);
        y > 1.0 && slope(x, z) > 0.85
    };
    for (i, p) in scatter(&mut rng, 40, 2.2, area, &mut taken, |x, z, _| grassy(x, z)).into_iter().enumerate() {
        let model = if i % 3 == 0 { "bush_berry" } else { "bush" };
        nature = nature.child(kit.object(format!("Bush {i}"), model, on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(0.7, 1.2)));
    }
    for (i, p) in scatter(&mut rng, 90, 1.2, area, &mut taken, |x, z, _| grassy(x, z)).into_iter().enumerate() {
        nature = nature.child(kit.object(format!("Flowers {i}"), "flowers", on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(0.8, 1.3)));
    }
    for (i, p) in scatter(&mut rng, 450, 0.9, area, &mut taken, |x, z, _| grassy(x, z)).into_iter().enumerate() {
        nature = nature.child(kit.tinted(format!("Grass {i}"), "grass_tuft", on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(0.9, 1.8), Color::rgb(rng.range(0.9, 1.1), rng.range(0.9, 1.05), 0.9)));
    }
    world.push(nature);

    // --- небо: облака и чайки ---
    let mut sky = Node::new("Sky");
    for i in 0..9 {
        // Облака — за островом, низко над горизонтом (камера смотрит к −Z).
        let p = Vec3::new(rng.range(-160.0, 160.0), rng.range(22.0, 40.0), rng.range(-220.0, -90.0));
        let s = rng.range(2.0, 3.6);
        sky = sky.child(kit.object(format!("Cloud {i}"), "cloud", p, rng.range(0.0, 360.0), s));
    }
    for i in 0..3 {
        let (kind, fields) = spin(Vec3::Y, 14.0 + i as f32 * 5.0);
        let center = pier_p + Vec3::new(rng.range(-4.0, 4.0), 9.0 + i as f32 * 2.5, 4.0);
        let radius = 5.0 + i as f32 * 2.5;
        sky = sky.child(
            Node::new(format!("Gull Orbit {i}"))
                .at(center)
                .yaw(i as f32 * 120.0)
                .with(kind, fields)
                .child(kit.object("Gull", "gull", Vec3::new(radius, 0.0, 0.0), 0.0, 1.6).rot(0.0, 0.0, -18.0)),
        );
    }
    world.push(sky);
    fix_clouds(&mut world);

    // --- камеры ---
    let shots = Node::new("Shots").children([
        shot("hero", camera_eye, Vec3::new(2.5, 3.5, 5.0), 40.0, true, Color::BLACK),
        shot("village", VILLAGE_EYE, square + Vec3::new(1.0, 2.6, -2.0), 48.0, true, Color::BLACK),
        shot("windmill", mill + Vec3::new(-14.0, 2.5, 13.0), mill + Vec3::new(0.0, 6.0, 0.0), 45.0, true, Color::BLACK),
        shot("lighthouse", light + Vec3::new(-22.0, 2.0, 30.0), light + Vec3::new(0.0, 6.0, 0.0), 40.0, true, Color::BLACK),
        shot("aerial", Vec3::new(55.0, 60.0, 70.0), Vec3::new(0.0, 0.0, 0.0), 45.0, true, Color::BLACK),
    ]);
    world.push(shots);
    scene("Showcase: Island at Golden Hour", world)
}

/// У облаков нет теней (иначе на острове пятна от невидимых в кадре облаков).
fn fix_clouds(world: &mut [Node]) {
    for node in world.iter_mut().filter(|n| n.0.name == "Sky") {
        for child in node.0.children.iter_mut().filter(|c| c.name.starts_with("Cloud")) {
            child.overrides.push(Override {
                path: String::new(),
                component: "MeshRenderer".to_string(),
                field: "cast_shadows".to_string(),
                value: Value::Bool(false),
            });
        }
    }
}

// ---------------------------------------------------------------------
// 2. Лесная поляна ночью
// ---------------------------------------------------------------------

fn clearing(kit: &Kit) -> SceneData {
    let field = ground::clearing_field();
    let h = |x: f32, z: f32| field.height(x, z);
    let mut rng = Rng::new(77);
    let mut taken: Vec<Vec2> = vec![Vec2::ZERO];
    let fire = on(&field, 0.0, 0.0);

    let moon_dir = -ground::MOON.normalize();
    let lighting = Node::new("Lighting")
        .with(
            "Environment",
            vec![
                ("ambient", f(1.0)),
                ("fog_density", f(0.03)),
                ("exposure", f(1.15)),
                ("bloom", f(0.16)),
                ("vignette", f(0.38)),
                ("shadow_distance", f(45.0)),
            ],
        )
        .with("Fog", vec![("start", f(5.0)), ("height", f(0.4)), ("height_falloff", f(0.22))])
        .with("SkyColors", vec![("zenith", rgb(0.006, 0.012, 0.04)), ("horizon", rgb(0.03, 0.05, 0.1))])
        .with("SoftShadows", vec![])
        .with("AmbientOcclusion", vec![])
        .with("PostQuality", vec![])
        .with(
            "ColorGrading",
            vec![
                ("temperature", f(-0.05)),
                ("contrast", f(1.12)),
                ("saturation", f(1.05)),
                ("shadows", rgb(0.1, 0.22, 0.5)),
            ],
        )
        .child(sun("Moon", moon_dir, Color::rgb(0.55, 0.66, 1.0), 0.5, true));

    let mut camp = Node::new("Camp")
        .child(kit.object("Campfire", "campfire", fire, 0.0, 1.0))
        .child(
            point_light("Fire Light", fire + Vec3::Y * 1.0, Color::rgb(1.0, 0.52, 0.2), 16.0, 14.0)
                .with("Flicker", vec![("amount", f(0.22)), ("speed", f(9.0))]),
        )
        .child(Node::new("Embers").at(fire + Vec3::Y * 0.4).with(
            "ParticleSystem",
            vec![
                ("emission_rate", f(14.0)),
                ("lifetime", v2(1.2, 2.6)),
                ("start_speed", v2(0.6, 1.6)),
                ("start_size", v2(0.035, 0.07)),
                ("start_color", rgb(4.0, 1.4, 0.3)),
                ("start_color_max", rgb(3.0, 0.7, 0.12)),
                ("gravity", v3(0.1, 0.5, 0.0)),
                ("drag", f(0.8)),
                ("color_over_lifetime", Value::List(vec![rgba(1.0, 1.0, 1.0, 1.0), rgba(1.0, 0.8, 0.6, 0.8), rgba(1.0, 0.4, 0.2, 0.0)])),
                ("shape", e("Cone")),
                ("shape_size", v3(0.3, 0.3, 0.3)),
                ("cone_angle", f(20.0)),
                ("blend", e("Additive")),
                ("particle_shape", e("Circle")),
                ("seed", Value::Int(5)),
            ],
        ))
        .child(Node::new("Fire Smoke").at(fire + Vec3::Y * 1.3).with("ParticleSystem", {
            let mut s = smoke(9);
            s.retain(|(k, _)| *k != "start_color" && *k != "start_color_max");
            s.push(("start_color", rgba(0.12, 0.11, 0.11, 0.35)));
            s.push(("start_color_max", rgba(0.08, 0.08, 0.09, 0.3)));
            s
        }));
    // Брёвна-скамейки вокруг огня.
    for (i, a) in [200.0f32, 290.0, 20.0].into_iter().enumerate() {
        let r = a.to_radians();
        let p = on(&field, r.cos() * 2.7, r.sin() * 2.7);
        camp = camp.child(kit.object(format!("Seat Log {i}"), "log", p, -a + 90.0 + rng.range(-10.0, 10.0), 0.9));
    }
    let tent = on(&field, -5.2, -2.6);
    camp = camp.child(kit.object("Tent", "tent", tent, face(tent, fire), 1.0));
    let lamp = on(&field, -3.4, -4.6);
    camp = camp
        .child(kit.object("Lantern", "lantern_post", lamp, 150.0, 0.9))
        .child(point_light("Lantern Light", lamp + Quat::from_rotation_y(150f32.to_radians()) * Vec3::new(0.54, 2.0, 0.0), Color::rgb(1.0, 0.7, 0.35), 3.0, 6.0));
    let wood = on(&field, -7.5, 0.6);
    camp = camp.child(kit.object("Firewood", "firewood", wood, 70.0, 1.0));
    let fox = on(&field, -1.5, 2.3);
    camp = camp.child(kit.object("Fox", "fox", fox, face(fox, fire) + 15.0, 1.5));
    let stump = on(&field, 4.6, -3.2);
    camp = camp
        .child(kit.object("Stump", "stump", stump, 30.0, 1.0))
        .child(kit.object("Owl", "owl", stump + Vec3::Y * 0.56, face(stump, fire) + 25.0, 1.1));
    camp = camp.child(kit.object("Sack", "sack", on(&field, -4.0, 0.6), 40.0, 1.0));
    camp = camp.child(kit.object("Signpost", "signpost", on(&field, 2.2, -9.0), 200.0, 1.0));
    for p in [tent, lamp, wood, fox, stump] {
        taken.push(Vec2::new(p.x, p.z));
    }
    let mut world = vec![
        lighting,
        kit.object("Ground", "clearing", Vec3::ZERO, 0.0, 1.0),
        kit.object("Night Sky", "night_sky", Vec3::ZERO, 0.0, 1.0).set("MeshRenderer", "cast_shadows", Value::Bool(false)),
        camp,
    ];

    // Светлячки над поляной.
    world.push(Node::new("Fireflies").at(fire + Vec3::Y * 1.8).with(
        "ParticleSystem",
        vec![
            ("emission_rate", f(7.0)),
            ("max_particles", Value::Int(70)),
            ("lifetime", v2(5.0, 9.0)),
            ("start_speed", v2(0.05, 0.25)),
            ("start_size", v2(0.05, 0.09)),
            ("start_color", rgb(2.2, 3.0, 0.8)),
            ("start_color_max", rgb(1.6, 2.6, 0.5)),
            ("gravity", v3(0.0, 0.03, 0.0)),
            ("color_over_lifetime", Value::List(vec![rgba(1.0, 1.0, 1.0, 0.0), rgba(1.0, 1.0, 1.0, 1.0), rgba(1.0, 1.0, 1.0, 0.0)])),
            ("shape", e("Sphere")),
            ("shape_size", v3(9.0, 9.0, 9.0)),
            ("blend", e("Additive")),
            ("particle_shape", e("Soft")),
            ("seed", Value::Int(17)),
        ],
    ));

    // --- лес ---
    let mut forest = Node::new("Forest");
    let area = (Vec2::splat(-36.0), Vec2::splat(36.0));
    for (i, p) in scatter(&mut rng, 230, 2.6, area, &mut taken, |x, z, rng| {
        let r = (x * x + z * z).sqrt();
        r > 11.0 + rng.range(0.0, 5.0) && r < 36.0 && !(x.abs() < 2.5 && z < -8.0)
    })
    .into_iter()
    .enumerate()
    {
        let model = match i % 7 {
            0 | 3 => "pine_small",
            _ => "pine",
        };
        let s = rng.range(0.9, 1.6);
        nature_push(&mut forest, kit, format!("Tree {i}"), model, on(&field, p.x, p.y), rng.range(0.0, 360.0), s, &mut rng);
    }
    for (i, p) in scatter(&mut rng, 26, 2.0, area, &mut taken, |x, z, _| {
        let r = (x * x + z * z).sqrt();
        r > 5.0 && r < 20.0
    })
    .into_iter()
    .enumerate()
    {
        let model = ["rock_small", "bush", "rock_large", "bush_berry"][i % 4];
        let s = if model == "rock_large" { 0.5 } else { rng.range(0.7, 1.2) };
        forest = forest.child(kit.object(format!("Undergrowth {i}"), model, on(&field, p.x, p.y) - Vec3::Y * 0.1, rng.range(0.0, 360.0), s));
    }
    for (i, p) in scatter(&mut rng, 14, 1.5, area, &mut taken, |x, z, _| {
        let r = (x * x + z * z).sqrt();
        r > 4.5 && r < 14.0
    })
    .into_iter()
    .enumerate()
    {
        let model = ["glow_shrooms", "mushroom", "mushroom_cluster"][i % 3];
        forest = forest.child(kit.object(format!("Mushrooms {i}"), model, on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(1.0, 1.6)));
    }
    for (i, p) in scatter(&mut rng, 380, 0.8, area, &mut taken, |x, z, _| {
        let r = (x * x + z * z).sqrt();
        r > 3.4 && r < 26.0 && h(x, z) < 2.0
    })
    .into_iter()
    .enumerate()
    {
        forest = forest.child(kit.tinted(format!("Grass {i}"), "grass_tuft", on(&field, p.x, p.y), rng.range(0.0, 360.0), rng.range(1.0, 1.8), Color::rgb(0.75, rng.range(0.8, 1.0), 0.8)));
    }
    for (i, p) in scatter(&mut rng, 30, 1.2, area, &mut taken, |x, z, _| {
        let r = (x * x + z * z).sqrt();
        r > 4.0 && r < 16.0
    })
    .into_iter()
    .enumerate()
    {
        forest = forest.child(kit.object(format!("Flowers {i}"), "flowers", on(&field, p.x, p.y), rng.range(0.0, 360.0), 1.0));
    }
    world.push(forest);

    let shots = Node::new("Shots").children([
        shot("hero", fire + Vec3::new(6.5, 2.2, 7.5), fire + Vec3::new(-1.0, 0.9, -1.0), 45.0, true, Color::BLACK),
        shot("fox", on(&field, 1.8, 5.4) + Vec3::Y * 1.0, fire.lerp(fox, 0.55) + Vec3::Y * 0.6, 40.0, true, Color::BLACK),
        shot("tent", fire + Vec3::new(4.0, 2.0, -6.0), tent + Vec3::new(0.0, 0.9, 0.0), 45.0, true, Color::BLACK),
        shot("owl", stump + Vec3::new(-2.6, 1.15, 1.2), stump + Vec3::new(0.0, 0.9, 0.0), 38.0, true, Color::BLACK),
        shot("wide", Vec3::new(16.0, 14.0, 20.0), fire + Vec3::new(-1.0, 0.0, -2.0), 45.0, true, Color::BLACK),
    ]);
    world.push(shots);
    scene("Showcase: Forest Clearing at Night", world)
}

fn nature_push(parent: &mut Node, kit: &Kit, name: String, model: &str, p: Vec3, yaw: f32, s: f32, rng: &mut Rng) {
    let tint = Color::rgb(rng.range(0.85, 1.05), rng.range(0.9, 1.05), rng.range(0.85, 1.05));
    let node = std::mem::replace(parent, Node::new(""));
    *parent = node.child(kit.tinted(name, model, p, yaw, s, tint));
}

// ---------------------------------------------------------------------
// 3. Мастерская-диорама
// ---------------------------------------------------------------------

fn workshop(kit: &Kit) -> SceneData {
    let hs = ground::DIORAMA * 0.5;
    let background = Color::rgb(0.045, 0.05, 0.065);
    let sun_dir = Vec3::new(0.3, -0.45, 0.84).normalize();
    let lighting = Node::new("Lighting")
        .with(
            "Environment",
            vec![
                ("ambient", f(0.45)),
                ("fog_density", f(0.0)),
                ("exposure", f(1.0)),
                ("bloom", f(0.12)),
                ("vignette", f(0.3)),
                ("shadow_distance", f(25.0)),
            ],
        )
        .with("Fog", vec![("enabled", Value::Bool(false))])
        .with("SkyColors", vec![("zenith", rgb(0.22, 0.27, 0.38)), ("horizon", rgb(0.34, 0.33, 0.36))])
        .with("SoftShadows", vec![])
        .with("AmbientOcclusion", vec![])
        .with("Outline", vec![])
        .with("PostQuality", vec![])
        .with(
            "ColorGrading",
            vec![
                ("temperature", f(0.05)),
                ("contrast", f(1.1)),
                ("saturation", f(1.06)),
                ("shadows", rgb(0.05, 0.25, 0.32)),
            ],
        )
        .child(sun("Window Sun", sun_dir, Color::rgb(1.0, 0.82, 0.6), 4.2, true));

    let mut room = Node::new("Workshop").child(kit.object("Diorama", "diorama", Vec3::ZERO, 0.0, 1.0));
    let bench = Vec3::new(-2.0, 0.0, -hs + 0.65);
    room = room
        .child(kit.object("Workbench", "workbench", bench, 180.0, 1.0))
        .child(kit.object("Desk Lamp", "desk_lamp", bench + Vec3::new(-0.75, 0.98, 0.15), 200.0, 1.0))
        .child(point_light("Lamp Light", bench + Vec3::new(-0.5, 1.6, 0.3), Color::rgb(1.0, 0.72, 0.4), 4.0, 5.0))
        .child(kit.object("Jar", "jar", bench + Vec3::new(0.35, 0.98, 0.2), 0.0, 1.0))
        .child(kit.object("Gear (bench)", "gear", bench + Vec3::new(0.0, 0.98, 0.25), 10.0, 0.6))
        .child(kit.object("Shelf", "shelf", Vec3::new(-hs + 0.3, 0.0, 0.6), -90.0, 1.0))
        .child(kit.object("Toolbox", "toolbox", Vec3::new(-1.0, 0.0, -2.6), 20.0, 1.2))
        .child(kit.object("Gear", "gear", Vec3::new(0.0, 0.0, 1.9), 0.0, 1.2))
        .child(kit.object("Gear (small)", "gear", Vec3::new(-0.9, 0.0, 1.1), 17.0, 0.7))
        .child(kit.object("Gear (leaning)", "gear", Vec3::new(-3.3, 0.55, -1.6), 0.0, 0.9).rot(0.0, 0.0, 80.0))
        .child(kit.object("Crystal", "crystal", Vec3::new(-3.2, 0.0, 2.9), 30.0, 1.5))
        .child(point_light("Crystal Light", Vec3::new(-3.0, 1.0, 2.8), Color::rgb(0.35, 0.85, 1.0), 3.5, 5.0))
        .child(kit.object("Crate", "crate", Vec3::new(3.0, 0.0, -3.0), 8.0, 1.0))
        .child(kit.object("Crate (2)", "crate", Vec3::new(3.2, 0.0, -1.9), -12.0, 0.9))
        .child(kit.object("Crate (top)", "crate", Vec3::new(3.05, 0.9, -2.95), 30.0, 0.8))
        .child(kit.object("Terminal", "terminal", Vec3::new(3.2, 0.81, -1.9), -40.0, 1.0))
        .child(kit.object("Barrel", "barrel", Vec3::new(2.4, 0.0, -1.2), 0.0, 1.0))
        .child(kit.object("Plant", "plant_pot", Vec3::new(1.7, 1.3, -hs + 0.07), 0.0, 0.9))
        .child(kit.object("Sack", "sack", Vec3::new(-3.2, 0.0, -2.6), 20.0, 1.0))
        .child(kit.object("Robot", "robot", Vec3::new(1.6, 0.0, 0.5), face(Vec3::new(1.6, 0.0, 0.5), Vec3::new(6.0, 0.0, 8.0)), 1.25))
        .child(
            Node::new("Rover")
                .at(Vec3::new(-2.4, 0.0, 1.9))
                .yaw(-120.0)
                .scale(0.9)
                .with(
                    "MeshRenderer",
                    vec![("mesh", Value::Asset(ROVER_GUID.to_string())), ("color", rgb(1.0, 1.0, 1.0)), ("roughness", f(0.55))],
                ),
        );
    // Пылинки в луче из окна.
    room = room.child(Node::new("Dust").at(Vec3::new(0.0, 1.6, -1.0)).with(
        "ParticleSystem",
        vec![
            ("emission_rate", f(10.0)),
            ("max_particles", Value::Int(90)),
            ("lifetime", v2(5.0, 9.0)),
            ("start_speed", v2(0.02, 0.08)),
            ("start_size", v2(0.02, 0.035)),
            ("start_color", rgba(1.6, 1.4, 1.1, 0.7)),
            ("start_color_max", rgba(1.2, 1.1, 0.9, 0.5)),
            ("gravity", v3(0.0, -0.01, 0.0)),
            ("color_over_lifetime", Value::List(vec![rgba(1.0, 1.0, 1.0, 0.0), rgba(1.0, 1.0, 1.0, 1.0), rgba(1.0, 1.0, 1.0, 0.0)])),
            ("shape", e("Box")),
            ("shape_size", v3(2.6, 2.2, 3.0)),
            ("blend", e("Additive")),
            ("particle_shape", e("Soft")),
            ("seed", Value::Int(3)),
        ],
    ));
    let center = Vec3::new(0.0, 0.9, 0.0);
    let shots = Node::new("Shots").children([
        shot("hero", Vec3::new(11.5, 8.6, 12.5), center + Vec3::new(-0.3, -0.6, -0.3), 33.0, false, background),
        shot("robot", Vec3::new(4.6, 2.1, 5.0), Vec3::new(1.4, 1.2, 0.5), 40.0, false, background),
        shot("bench", Vec3::new(0.4, 1.9, -0.6), bench + Vec3::new(-0.3, 1.1, 0.0), 45.0, false, background),
        shot("top", Vec3::new(9.0, 13.0, 6.0), center, 34.0, false, background),
    ]);
    scene("Showcase: Workshop Diorama", vec![lighting, room, shots])
}

// ---------------------------------------------------------------------
// 4. Долина днём: рельеф 2 км и тысячи деревьев
// ---------------------------------------------------------------------

/// Угол рельефа долины.
pub const VALLEY_CORNER: f32 = -1024.0;

pub fn valley_terrain() -> Terrain {
    Terrain {
        seed: 99,
        size: 2048.0,
        view_distance: 1800.0,
        grass_color: Color::rgb(0.95, 1.0, 0.72),
        dirt_color: Color::rgb(1.0, 0.92, 0.82),
        ..Terrain::default()
    }
}

fn valley(kit: &Kit) -> SceneData {
    let terrain = valley_terrain();
    let sampler = terrain.sampler(None);
    let h = |x: f32, z: f32| sampler.height_local(x - VALLEY_CORNER, z - VALLEY_CORNER);
    let n = |x: f32, z: f32| sampler.normal_local(x - VALLEY_CORNER, z - VALLEY_CORNER);
    let mut rng = Rng::new(4242);
    let eye = Vec3::new(-260.0, h(-260.0, 420.0) + 40.0, 420.0);
    let target = Vec3::new(120.0, 60.0, -60.0);

    let sun_dir = Vec3::new(0.65, -0.6, 0.05).normalize();
    let lighting = Node::new("Lighting")
        .with(
            "Environment",
            vec![
                ("ambient", f(0.45)),
                ("fog_density", f(0.0006)),
                ("bloom", f(0.05)),
                ("vignette", f(0.2)),
                ("shadow_distance", f(260.0)),
            ],
        )
        .with("Fog", vec![("start", f(120.0)), ("height_falloff", f(0.004))])
        .with("SoftShadows", vec![])
        .with("Sky", vec![("clouds", f(0.35))])
        .with("AmbientOcclusion", vec![])
        .with("PostQuality", vec![])
        .with("ColorGrading", vec![("contrast", f(1.05)), ("saturation", f(1.12)), ("temperature", f(0.04))])
        .child(sun("Sun", sun_dir, Color::rgb(1.0, 0.94, 0.85), 2.8, true));

    let terrain_fields = vec![
        ("seed", Value::Int(99)),
        ("size", f(2048.0)),
        ("view_distance", f(1800.0)),
        ("grass_color", rgb(0.95, 1.0, 0.72)),
        ("dirt_color", rgb(1.0, 0.92, 0.82)),
    ];
    let mut world = vec![
        lighting,
        Node::new("Terrain").at(Vec3::new(VALLEY_CORNER, 0.0, VALLEY_CORNER)).with("Terrain", terrain_fields),
        Node::new("Sea").with("Water", vec![]),
    ];

    // Леса: густые пятна по шуму, на суше с пологим склоном, в клине перед камерой.
    let forward = Vec2::new(target.x - eye.x, target.z - eye.z).normalize();
    let mut taken = Vec::new();
    let models = ["pine", "pine", "pine_small", "tree_round", "birch", "pine", "tree_round", "pine", "pine_small", "tree_round", "pine", "birch", "pine", "tree_autumn"];
    let mut forest = Node::new("Forest");
    let area = (Vec2::new(-700.0, -500.0), Vec2::new(500.0, 700.0));
    let trees = scatter(&mut rng, 2400, 5.0, area, &mut taken, |x, z, rng| {
        let to = Vec2::new(x - eye.x, z - eye.z);
        let dist = to.length();
        if dist < 25.0 || dist > 900.0 || to.normalize().dot(forward) < 0.55 {
            return false;
        }
        let y = h(x, z);
        if !(2.5..150.0).contains(&y) || n(x, z).y < 0.82 {
            return false;
        }
        let clump = ground::fbm(x * 0.012, z * 0.012, 3, 77);
        clump > -0.05 + rng.range(-0.15, 0.15)
    });
    for (i, p) in trees.into_iter().enumerate() {
        let model = models[i % models.len()];
        let s = rng.range(1.4, 2.4);
        let tint = Color::rgb(rng.range(0.85, 1.05), rng.range(0.9, 1.05), rng.range(0.8, 1.0));
        forest = forest.child(kit.tinted(format!("Tree {i}"), model, Vec3::new(p.x, h(p.x, p.y) - 0.3, p.y), rng.range(0.0, 360.0), s, tint));
    }
    world.push(forest);
    let mut rocks = Node::new("Rocks");
    for (i, p) in scatter(&mut rng, 160, 8.0, area, &mut taken, |x, z, _| {
        let to = Vec2::new(x - eye.x, z - eye.z);
        to.length() < 400.0 && to.normalize().dot(forward) > 0.5 && h(x, z) > 2.0 && n(x, z).y < 0.9
    })
    .into_iter()
    .enumerate()
    {
        rocks = rocks.child(kit.object(format!("Boulder {i}"), "rock_large", Vec3::new(p.x, h(p.x, p.y) - 0.5, p.y), rng.range(0.0, 360.0), rng.range(1.2, 3.0)));
    }
    world.push(rocks);
    let mut sky = Node::new("Sky");
    for i in 0..14 {
        let along = rng.range(200.0, 1400.0);
        let side = rng.range(-700.0, 700.0);
        let p = Vec2::new(eye.x, eye.z) + forward * along + Vec2::new(-forward.y, forward.x) * side;
        let node = kit.object(format!("Cloud {i}"), "cloud", Vec3::new(p.x, rng.range(260.0, 420.0), p.y), rng.range(0.0, 360.0), rng.range(12.0, 20.0));
        sky = sky.child(node);
    }
    world.push(sky);
    fix_clouds(&mut world);

    let forest_eye = {
        let (x, z) = (eye.x + 60.0, eye.z - 80.0);
        Vec3::new(x, h(x, z) + 9.0, z)
    };
    let shots = Node::new("Shots").children([
        shot("hero", eye, target, 55.0, true, Color::BLACK),
        shot("forest", forest_eye, forest_eye + Vec3::new(0.62, -0.08, -0.78), 60.0, true, Color::BLACK),
        shot("high", eye + Vec3::new(-120.0, 220.0, 160.0), target + Vec3::new(0.0, 0.0, -200.0), 50.0, true, Color::BLACK),
    ]);
    world.push(shots);
    scene("Showcase: Valley", world)
}

// ---------------------------------------------------------------------
// Компактная запись сцены: экземпляр префаба — одна строка
// ---------------------------------------------------------------------

fn num(v: f32) -> String {
    format!("{v:?}")
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Nil => "Nil".to_string(),
        Value::Bool(b) => format!("Bool({b})"),
        Value::Int(i) => format!("Int({i})"),
        Value::Float(x) => format!("Float({})", num(*x)),
        Value::Str(s) => format!("Str({s:?})"),
        Value::Vec2(a, b) => format!("Vec2({}, {})", num(*a), num(*b)),
        Value::Vec3(a, b, c) => format!("Vec3({}, {}, {})", num(*a), num(*b), num(*c)),
        Value::Quat(a, b, c, d) => format!("Quat({}, {}, {}, {})", num(*a), num(*b), num(*c), num(*d)),
        Value::Color(a, b, c, d) => format!("Color({}, {}, {}, {})", num(*a), num(*b), num(*c), num(*d)),
        Value::Enum(s) => format!("Enum({s:?})"),
        Value::Asset(s) => format!("Asset({s:?})"),
        Value::List(items) => format!("List([{}])", items.iter().map(value_text).collect::<Vec<_>>().join(", ")),
        // Прочие (ссылки на объекты и всё, что появится позже) — как их пишет сам RON.
        #[allow(unreachable_patterns)]
        other => format!("{other:?}"),
    }
}

fn triple(v: [f32; 3]) -> String {
    format!("({}, {}, {})", num(v[0]), num(v[1]), num(v[2]))
}

fn component_text(c: &ComponentData) -> String {
    let fields: Vec<String> = c.fields.iter().map(|(k, v)| format!("{k:?}: {}", value_text(v))).collect();
    if fields.is_empty() {
        format!("(kind: {:?})", c.kind)
    } else {
        format!("(kind: {:?}, fields: {{{}}})", c.kind, fields.join(", "))
    }
}

fn write_node(out: &mut String, n: &NodeData, depth: usize) {
    let pad = "    ".repeat(depth);
    let mut head = vec![format!("name: {:?}", n.name)];
    if !n.tag.is_empty() {
        head.push(format!("tag: {:?}", n.tag));
    }
    if n.layer != 0 {
        head.push(format!("layer: {}", n.layer));
    }
    if !n.active {
        head.push("active: false".to_string());
    }
    if let Some(prefab) = &n.prefab {
        head.push(format!("prefab: Some({prefab:?})"));
    }
    let t = &n.transform;
    let mut tf = Vec::new();
    if t.position != [0.0; 3] {
        tf.push(format!("position: {}", triple(t.position)));
    }
    if t.rotation != [0.0; 3] {
        tf.push(format!("rotation: {}", triple(t.rotation)));
    }
    if t.scale != [1.0; 3] {
        tf.push(format!("scale: {}", triple(t.scale)));
    }
    if !tf.is_empty() {
        head.push(format!("transform: ({})", tf.join(", ")));
    }
    if !n.overrides.is_empty() {
        let o: Vec<String> = n
            .overrides
            .iter()
            .map(|o| {
                let path = if o.path.is_empty() { String::new() } else { format!("path: {:?}, ", o.path) };
                format!("({path}component: {:?}, field: {:?}, value: {})", o.component, o.field, value_text(&o.value))
            })
            .collect();
        head.push(format!("overrides: [{}]", o.join(", ")));
    }
    out.push_str(&pad);
    out.push('(');
    out.push_str(&head.join(", "));
    if !n.components.is_empty() {
        out.push_str(", components: [\n");
        for c in &n.components {
            out.push_str(&format!("{pad}    {},\n", component_text(c)));
        }
        out.push_str(&pad);
        out.push(']');
    }
    if !n.children.is_empty() {
        out.push_str(", children: [\n");
        for c in &n.children {
            write_node(out, c, depth + 1);
        }
        out.push_str(&pad);
        out.push(']');
    }
    out.push_str("),\n");
}

/// Текст файла сцены (RON, тот же формат, что пишет редактор, но компактнее).
pub fn scene_text(data: &SceneData) -> String {
    let mut out = format!("(\n    name: {:?},\n    objects: [\n", data.name);
    for n in &data.objects {
        write_node(&mut out, n, 2);
    }
    out.push_str("    ],\n)\n");
    out
}

/// Текст префаба (один `NodeData`).
pub fn prefab_text(node: &NodeData) -> String {
    let mut out = String::new();
    write_node(&mut out, node, 0);
    out.trim_end().trim_end_matches(',').to_string() + "\n"
}
