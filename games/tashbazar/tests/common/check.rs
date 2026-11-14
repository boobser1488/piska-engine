//! Проверка расстановки: всё видимое на сцене разбирается на «сборки» (экземпляр префаба
//! реквизита, фасада или отдельный блок), у каждой — куски (объекты с `MeshRenderer`) и
//! связные части мешей. Категории нарушений:
//!
//! * `FLOAT` — реквизит висит над опорой (нижние вершины выше поверхности больше чем на 1 см);
//! * `SINK` — реквизит утоплен в опору глубже 3 см;
//! * `PART` — часть сборки или меша ни с чем не соприкасается (ручка телеги в воздухе);
//! * `ORIGIN` — начало координат реквизита не у его основания;
//! * `ZFIGHT` — совпадающие сонаправленные грани разных объектов с разными материалами на виду;
//! * `INTERSECT` — реквизит входит в стену или другой реквизит глубже 5 см;
//! * `MOUNT` — настенная вещь (окно, дверь, фонарь, навес) не касается стены;
//! * `WALL` — место на стене: настенные вещи на одной стене перекрываются (фонарь на окне,
//!   фонарь на двери, навес на окне, окно на окне), окно или дверь заходит за угол стены,
//!   задевает карниз / цоколь / пояс, окно без закрытых ставен стоит за реквизитом выше
//!   своего подоконника, окно смотрит в стену ближе 0.5 м.
#![allow(dead_code)]

use std::collections::HashMap;

use engine::prelude::*;

/// Реквизит, который стоит на земле.
pub const RESTING: [&str; 15] = [
    "crate_small", "crate_big", "crate_long", "barrel", "palm", "well", "plant_pot", "jar", "sack", "cart",
    "market_stall", "bench", "lantern_post", "drying_line", "awning",
];
/// Настенное.
pub const MOUNTED: [&str; 4] = ["window", "door", "wall_lantern", "awning"];

#[derive(Clone, Debug)]
pub struct Piece {
    pub id: GameObjectId,
    pub assembly: GameObjectId,
    pub name: String,
    pub material: String,
    pub solid: bool,
    /// Связные части меша в мировых координатах: (min, max).
    pub parts: Vec<(Vec3, Vec3)>,
    pub min: Vec3,
    pub max: Vec3,
    /// Нижние вершины (в пределах 1 см от низа куска).
    pub low: Vec<Vec3>,
    /// Ящик по осям мира (куб без поворота или с поворотом на кратный 90° угол).
    pub axis_box: bool,
    /// Треугольники меша в мировых координатах.
    pub tris: Vec<[Vec3; 3]>,
}

#[derive(Clone, Debug)]
pub struct Offender {
    pub category: &'static str,
    pub name: String,
    pub at: Vec3,
    pub detail: String,
    pub id: GameObjectId,
}

fn components(mesh: &engine::assets::MeshData) -> Vec<Vec<usize>> {
    // Сварка по позиции (0.1 мм), затем объединение по треугольникам.
    let mut key: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let rep: Vec<usize> = mesh
        .positions
        .iter()
        .map(|p| {
            let k = ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64);
            let n = key.len();
            *key.entry(k).or_insert(n)
        })
        .collect();
    let mut parent: Vec<usize> = (0..key.len()).collect();
    fn find(p: &mut [usize], mut a: usize) -> usize {
        while p[a] != a {
            p[a] = p[p[a]];
            a = p[a];
        }
        a
    }
    for tri in mesh.indices.chunks_exact(3) {
        let a = find(&mut parent, rep[tri[0] as usize]);
        for i in &tri[1..] {
            let b = find(&mut parent, rep[*i as usize]);
            if a != b {
                parent[b] = a;
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, r) in rep.iter().enumerate() {
        let root = find(&mut parent, *r);
        groups.entry(root).or_default().push(i);
    }
    groups.into_values().collect()
}

fn assembly_of(scene: &Scene, id: GameObjectId) -> GameObjectId {
    // Ближайший предок-экземпляр префаба (через дочерние объекты без префаба внутри него) и
    // выше — вложенные экземпляры; группы над экземпляром не считаются.
    let has = |x: GameObjectId| scene.get(x).is_some_and(|o| o.prefab().is_some());
    let mut cur = id;
    let mut found = None;
    loop {
        if has(cur) {
            found = Some(cur);
        }
        match scene.parent_of(cur) {
            Some(p) if found.is_none() || has(p) => cur = p,
            _ => break,
        }
    }
    found.unwrap_or(id)
}

/// Собрать куски сцены.
pub fn pieces(world: &World) -> Vec<Piece> {
    let scene = &world.scene;
    let db = world.resources.get::<AssetDatabase>().expect("assets");
    let mut mesh_cache: HashMap<String, (engine::assets::MeshData, Vec<Vec<usize>>)> = HashMap::new();
    let mut out = Vec::new();
    for id in scene.find_with::<MeshRenderer>() {
        if !scene.is_active(id) {
            continue;
        }
        let object = scene.get(id).unwrap();
        // Интерфейс и камеры не в счёт.
        let renderer = object.get::<MeshRenderer>().unwrap();
        let (data, comps) = if renderer.mesh.is_none() {
            let key = format!("{:?}", renderer.shape);
            mesh_cache
                .entry(key)
                .or_insert_with(|| {
                    let m = renderer.shape.mesh();
                    let c = components(&m);
                    (m, c)
                })
                .clone()
        } else {
            let key = format!("{:?}", renderer.mesh.guid());
            match mesh_cache.get(&key) {
                Some(v) => v.clone(),
                None => {
                    let m = db.load_mesh(renderer.mesh.guid()).expect("mesh loads");
                    let c = components(&m);
                    mesh_cache.insert(key.clone(), (m.clone(), c.clone()));
                    (m, c)
                }
            }
        };
        let matrix = scene.world_matrix(id);
        let world_pts: Vec<Vec3> = data.positions.iter().map(|p| matrix.transform_point3(Vec3::from_array(*p))).collect();
        let mut parts = Vec::new();
        for comp in &comps {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for i in comp {
                lo = lo.min(world_pts[*i]);
                hi = hi.max(world_pts[*i]);
            }
            parts.push((lo, hi));
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in &world_pts {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let low: Vec<Vec3> = world_pts.iter().copied().filter(|p| p.y <= lo.y + 0.01).collect();
        let (_, rotation, _) = matrix.to_scale_rotation_translation();
        let right = rotation * Vec3::X;
        let axis_aligned = renderer.mesh.is_none()
            && renderer.shape == MeshShape::Cube
            && (rotation * Vec3::Y).y > 0.9999
            && (right.x.abs() > 0.9999 || right.z.abs() > 0.9999);
        let tris: Vec<[Vec3; 3]> = data
            .indices
            .chunks_exact(3)
            .map(|t| [world_pts[t[0] as usize], world_pts[t[1] as usize], world_pts[t[2] as usize]])
            .collect();
        out.push(Piece {
            id,
            assembly: assembly_of(scene, id),
            name: object.name.clone(),
            material: renderer.materials.first().map(|m| format!("{:?}", m.guid())).unwrap_or_default(),
            solid: object.has::<Collider>(),
            parts,
            min: lo,
            max: hi,
            low,
            axis_box: axis_aligned,
            tris,
        });
    }
    out
}

fn touch(a: (Vec3, Vec3), b: (Vec3, Vec3), e: f32) -> bool {
    a.0.x <= b.1.x + e && b.0.x <= a.1.x + e && a.0.y <= b.1.y + e && b.0.y <= a.1.y + e && a.0.z <= b.1.z + e && b.0.z <= a.1.z + e
}

fn depth(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> Vec3 {
    (a.1.min(b.1) - a.0.max(b.0)).max(Vec3::ZERO)
}

/// Самая высокая обращённая вверх поверхность чужих мешей на вертикали точки `p`, не выше
/// `p.y + above`.
fn surface_below(pieces: &[Piece], assembly: GameObjectId, p: Vec3, above: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for q in pieces {
        if q.assembly == assembly || p.x < q.min.x - 1e-3 || p.x > q.max.x + 1e-3 || p.z < q.min.z - 1e-3 || p.z > q.max.z + 1e-3 || q.min.y > p.y + above {
            continue;
        }
        for t in &q.tris {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.y.abs() < 1e-9 {
                continue;
            }
            // Только грани, смотрящие вверх (опора), и точка внутри проекции треугольника.
            if n.normalize().y < 0.3 {
                continue;
            }
            let (a, b, c) = (Vec2::new(t[0].x, t[0].z), Vec2::new(t[1].x, t[1].z), Vec2::new(t[2].x, t[2].z));
            let x = Vec2::new(p.x, p.z);
            let d = (b - a).perp_dot(c - a);
            let w1 = (x - a).perp_dot(c - a) / d;
            let w2 = (b - a).perp_dot(x - a) / d;
            let e = 1e-4;
            if w1 < -e || w2 < -e || w1 + w2 > 1.0 + e {
                continue;
            }
            let y = t[0].y + (t[1].y - t[0].y) * w1 + (t[2].y - t[0].y) * w2;
            if y <= p.y + above && best.is_none_or(|b| y > b) {
                best = Some(y);
            }
        }
    }
    best
}

pub fn assembly_name(world: &World, assembly: GameObjectId) -> String {
    world.scene.get(assembly).map(|o| o.name.clone()).unwrap_or_default()
}

/// Все нарушения сцены.
pub fn check(world: &World) -> Vec<Offender> {
    let pieces = pieces(world);
    let scene = &world.scene;
    let mut by_assembly: HashMap<GameObjectId, Vec<usize>> = HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        by_assembly.entry(p.assembly).or_default().push(i);
    }
    let mut out = Vec::new();
    let mut add = |category: &'static str, name: String, at: Vec3, detail: String, id: GameObjectId| {
        out.push(Offender { category, name, at, detail, id });
    };

    for (assembly, list) in &by_assembly {
        let name = assembly_name(world, *assembly);
        let kind = name.as_str();
        let lo = list.iter().map(|i| pieces[*i].min).fold(Vec3::splat(f32::MAX), Vec3::min);
        let hi = list.iter().map(|i| pieces[*i].max).fold(Vec3::splat(f32::MIN), Vec3::max);
        let resting = RESTING.contains(&kind);
        let mounted = MOUNTED.contains(&kind);

        // (a) опора: нижние вершины сборки лежат на поверхности.
        if resting {
            let lows: Vec<Vec3> = list.iter().flat_map(|i| pieces[*i].low.iter().copied()).filter(|p| p.y <= lo.y + 0.01).collect();
            let mut best_gap = f32::MAX;
            let mut worst_gap: f32 = 0.0;
            let mut worst_sink: f32 = 0.0;
            for p in &lows {
                match surface_below(&pieces, *assembly, *p, 0.3) {
                    Some(s) => {
                        best_gap = best_gap.min((p.y - s).max(0.0));
                        worst_gap = worst_gap.max(p.y - s);
                        worst_sink = worst_sink.max(s - p.y);
                    }
                    None => best_gap = best_gap.min(f32::INFINITY),
                }
            }
            if best_gap == f32::INFINITY || best_gap == f32::MAX {
                add("FLOAT", name.clone(), lo, "no surface below".to_string(), *assembly);
            } else if best_gap > 0.01 {
                add("FLOAT", name.clone(), lo, format!("lowest point {:.3} m above the surface", best_gap), *assembly);
            } else if worst_gap > 0.03 {
                add("FLOAT", name.clone(), lo, format!("partly hanging: part of the base {:.3} m above the surface", worst_gap), *assembly);
            }
            if worst_sink > 0.03 {
                add("SINK", name.clone(), lo, format!("{:.3} m below the surface", worst_sink), *assembly);
            }
            // (c) начало координат у основания.
            let root = scene.world_position(*assembly);
            if (root.y - lo.y).abs() > 0.01 {
                add("ORIGIN", name.clone(), root, format!("origin {:.3} m off the base", root.y - lo.y), *assembly);
            }
        }

        // (b) части: каждая связная часть касается другой части той же сборки, опоры
        // (низ сборки) или чужого куска.
        let mut nodes: Vec<(Vec3, Vec3, usize)> = Vec::new();
        for i in list {
            for part in &pieces[*i].parts {
                nodes.push((part.0, part.1, *i));
            }
        }
        let mut grounded: Vec<bool> = nodes
            .iter()
            .map(|n| {
                n.0.y <= lo.y + 0.01
                    || pieces.iter().any(|q| q.assembly != *assembly && touch((n.0, n.1), (q.min, q.max), 0.01))
            })
            .collect();
        let mut changed = true;
        while changed {
            changed = false;
            for a in 0..nodes.len() {
                if grounded[a] {
                    continue;
                }
                if (0..nodes.len()).any(|b| b != a && grounded[b] && touch((nodes[a].0, nodes[a].1), (nodes[b].0, nodes[b].1), 0.01)) {
                    grounded[a] = true;
                    changed = true;
                }
            }
        }
        for (a, n) in nodes.iter().enumerate() {
            if !grounded[a] {
                let piece = &pieces[n.2];
                add("PART", format!("{name}/{}", piece.name), (n.0 + n.1) * 0.5, "touches nothing".to_string(), piece.id);
            }
        }

        // (e) настенное касается стены (куска не из реквизита).
        if mounted {
            let wall = pieces.iter().any(|q| {
                q.assembly != *assembly
                    && !RESTING.contains(&assembly_name(world, q.assembly).as_str())
                    && !MOUNTED.contains(&assembly_name(world, q.assembly).as_str())
                    && list.iter().any(|i| touch((pieces[*i].min, pieces[*i].max), (q.min, q.max), 0.01))
            });
            if !wall {
                add("MOUNT", name.clone(), (lo + hi) * 0.5, "does not touch a wall".to_string(), *assembly);
            }
        }

        // (d) реквизит входит в стену / другой реквизит.
        if resting {
            'parts: for i in list {
                let a = &pieces[*i];
                for part in &a.parts {
                    for q in &pieces {
                        if q.assembly == *assembly || !q.solid {
                            continue;
                        }
                        let d = depth(*part, (q.min, q.max));
                        // Опора под реквизитом — не пересечение.
                        let resting_on = q.max.y <= part.0.y + 0.05;
                        if !resting_on && d.x > 0.02 && d.y > 0.02 && d.z > 0.02 {
                            let other = assembly_name(world, q.assembly);
                            add("INTERSECT", format!("{name}/{}", a.name), (part.0 + part.1) * 0.5, format!("into {other}/{} by {:.2} m", q.name, d.min_element()), a.id);
                            break 'parts;
                        }
                    }
                }
            }
        }
    }

    // (d) совпадающие грани ящиков с разными материалами.
    let boxes: Vec<&Piece> = pieces.iter().filter(|p| p.axis_box).collect();
    let mut faces: HashMap<(usize, bool, i64), Vec<usize>> = HashMap::new();
    for (i, p) in boxes.iter().enumerate() {
        for axis in 0..3 {
            for positive in [false, true] {
                let c = if positive { p.max[axis] } else { p.min[axis] };
                faces.entry((axis, positive, (c * 1000.0).round() as i64)).or_default().push(i);
            }
        }
    }
    let inside_any = |pt: Vec3, skip: [usize; 2]| {
        boxes.iter().enumerate().any(|(k, b)| {
            !skip.contains(&k) && b.solid && pt.x > b.min.x && pt.x < b.max.x && pt.y > b.min.y && pt.y < b.max.y && pt.z > b.min.z && pt.z < b.max.z
        })
    };
    wall_places(world, &pieces, &by_assembly, &mut add);

    let mut reported = 0;
    for ((axis, positive, _), list) in &faces {
        for x in 0..list.len() {
            for y in x + 1..list.len() {
                let (a, b) = (boxes[list[x]], boxes[list[y]]);
                if a.material == b.material || a.assembly == b.assembly && a.material == b.material {
                    continue;
                }
                // Перекрытие граней в плоскости.
                let mut lo = a.min.max(b.min);
                let mut hi = a.max.min(b.max);
                lo[*axis] = 0.0;
                hi[*axis] = 0.0;
                let ext = hi - lo;
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                if ext[u] <= 0.005 || ext[v] <= 0.005 {
                    continue;
                }
                // Низ, лежащий на земле, не видно ни откуда.
                if *axis == 1 && !*positive && a.min.y <= 0.05 {
                    continue;
                }
                let mut center = (lo + hi) * 0.5;
                center[*axis] = if *positive { a.max[*axis] + 0.003 } else { a.min[*axis] - 0.003 };
                if inside_any(center, [list[x], list[y]]) {
                    continue;
                }
                reported += 1;
                if reported <= 400 {
                    add("ZFIGHT", format!("{} | {}", a.name, b.name), center, format!("{:.2} x {:.2} m on axis {axis}", ext[u], ext[v]), a.id);
                }
            }
        }
    }
    out
}

/// Настенная вещь: корень, наружу, вдоль, занятое у стены (до 15 см от неё) вдоль и по
/// высоте, путь префаба.
struct WallPlace {
    assembly: GameObjectId,
    name: String,
    prefab: String,
    at: Vec3,
    out: Vec3,
    along: Vec3,
    u: (f32, f32),
    y: (f32, f32),
    /// Прямоугольники отдельных кусков (u0, u1, y0, y1).
    rects: Vec<(f32, f32, f32, f32)>,
}

/// Категория `WALL` (см. начало файла).
fn wall_places(
    world: &World,
    pieces: &[Piece],
    by_assembly: &HashMap<GameObjectId, Vec<usize>>,
    add: &mut dyn FnMut(&'static str, String, Vec3, String, GameObjectId),
) {
    let scene = &world.scene;
    let db = world.resources.get::<AssetDatabase>().expect("assets");
    let mut places: Vec<WallPlace> = Vec::new();
    for (assembly, list) in by_assembly {
        let name = assembly_name(world, *assembly);
        if !MOUNTED.contains(&name.as_str()) {
            continue;
        }
        let (_, rot, at) = scene.world_matrix(*assembly).to_scale_rotation_translation();
        let (out, along) = (rot * Vec3::Z, rot * Vec3::X);
        let (mut u, mut y) = ((f32::MAX, f32::MIN), (f32::MAX, f32::MIN));
        let mut rects = Vec::new();
        for i in list {
            let mut r = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for t in &pieces[*i].tris {
                for v in t {
                    if (*v - at).dot(out) <= 0.15 {
                        let k = (*v - at).dot(along);
                        u = (u.0.min(k), u.1.max(k));
                        y = (y.0.min(v.y), y.1.max(v.y));
                        r = (r.0.min(k), r.1.max(k), r.2.min(v.y), r.3.max(v.y));
                    }
                }
            }
            if r.0 <= r.1 {
                rects.push(r);
            }
        }
        if u.0 > u.1 {
            continue;
        }
        let prefab = scene
            .get(*assembly)
            .and_then(|o| o.prefab())
            .and_then(Guid::parse)
            .and_then(|g| db.relative_path_of(g))
            .unwrap_or_default()
            .to_string();
        places.push(WallPlace { assembly: *assembly, name, prefab, at, out, along, u, y, rects });
    }
    places.sort_by(|a, b| a.at.x.total_cmp(&b.at.x).then(a.at.z.total_cmp(&b.at.z)).then(a.at.y.total_cmp(&b.at.y)));
    let walls: Vec<&Piece> = pieces
        .iter()
        .filter(|q| {
            let n = assembly_name(world, q.assembly);
            q.solid && q.axis_box && !RESTING.contains(&n.as_str()) && !MOUNTED.contains(&n.as_str())
        })
        .collect();
    let in_wall = |p: Vec3| walls.iter().any(|q| p.x > q.min.x && p.x < q.max.x && p.y > q.min.y && p.y < q.max.y && p.z > q.min.z && p.z < q.max.z);
    let point = |w: &WallPlace, u: f32, y: f32, d: f32| {
        let flat = w.at + w.along * u + w.out * d;
        Vec3::new(flat.x, y, flat.z)
    };
    // Перекрытия на одной стене.
    for (i, a) in places.iter().enumerate() {
        for b in &places[i + 1..] {
            if a.out.dot(b.out) < 0.99 || (b.at - a.at).dot(a.out).abs() > 0.05 {
                continue;
            }
            let shift = (b.at - a.at).dot(a.along);
            let (bu0, bu1) = (b.u.0 + shift, b.u.1 + shift);
            let margin = 0.05;
            if a.u.0 < bu1 + margin && bu0 < a.u.1 + margin && a.y.0 < b.y.1 + margin && b.y.0 < a.y.1 + margin {
                add("WALL", format!("{} on {}", a.name, b.name), a.at, format!("overlap on the wall ({} / {})", a.prefab, b.prefab), a.assembly);
            }
        }
    }
    for w in &places {
        // Угол стены: за прямоугольником везде стена.
        let inset = 0.02;
        let corners = [(w.u.0 + inset, w.y.0 + inset), (w.u.1 - inset, w.y.0 + inset), (w.u.0 + inset, w.y.1 - inset), (w.u.1 - inset, w.y.1 - inset)];
        if corners.iter().any(|(u, y)| !in_wall(point(w, *u, *y, -0.02))) {
            add("WALL", w.name.clone(), w.at, "runs past the wall (corner, roof or opening)".to_string(), w.assembly);
        }
        // Карниз, цоколь, пояс, другая кладка перед каким-нибудь куском (пол и вставки
        // мостовой под дверью — не в счёт).
        let hit = pieces.iter().find(|q| {
            let n = assembly_name(world, q.assembly);
            if q.assembly == w.assembly || RESTING.contains(&n.as_str()) || MOUNTED.contains(&n.as_str()) || q.max.y <= w.y.0 + 0.06 {
                return false;
            }
            // Ящик куска в координатах стены.
            let cs = [q.min, Vec3::new(q.max.x, q.min.y, q.min.z), Vec3::new(q.min.x, q.min.y, q.max.z), Vec3::new(q.max.x, q.min.y, q.max.z)];
            let us: Vec<f32> = cs.iter().map(|c| (*c - w.at).dot(w.along)).collect();
            let ds: Vec<f32> = cs.iter().map(|c| (*c - w.at).dot(w.out)).collect();
            let (u0, u1) = (us.iter().copied().fold(f32::MAX, f32::min), us.iter().copied().fold(f32::MIN, f32::max));
            let (d0, d1) = (ds.iter().copied().fold(f32::MAX, f32::min), ds.iter().copied().fold(f32::MIN, f32::max));
            d1 > 0.005
                && d0 < 0.1
                && w.rects.iter().any(|r| u0 < r.1 - 0.01 && u1 > r.0 + 0.01 && q.min.y < r.3 - 0.01 && q.max.y > r.2 + 0.01)
        });
        if let Some(q) = hit {
            add("WALL", w.name.clone(), w.at, format!("crosses {}", q.name), w.assembly);
        }
        if w.name != "window" {
            continue;
        }
        // Размер окна: подоконник, перемычка и высота проёма — как у открытого окна (1.16 м,
        // 1.1 м, 1.2 м): закрытое не должно превращаться в маленькую табличку.
        let span = |name: &str| {
            by_assembly.get(&w.assembly).and_then(|list| {
                list.iter().map(|i| &pieces[*i]).find(|p| p.name == name).map(|p| {
                    let us = p.tris.iter().flatten().map(|v| (*v - w.at).dot(w.along));
                    let (lo, hi) = us.fold((f32::MAX, f32::MIN), |a, u| (a.0.min(u), a.1.max(u)));
                    (hi - lo, p.min.y, p.max.y)
                })
            })
        };
        match (span("Sill"), span("Lintel")) {
            (Some(sill), Some(lintel)) => {
                let opening = lintel.1 - sill.2;
                if (sill.0 - 1.16).abs() > 0.02 || (lintel.0 - 1.1).abs() > 0.02 || (opening - 1.2).abs() > 0.02 {
                    add("WALL", w.name.clone(), w.at, format!("window size {:.2} / {:.2} / {:.2} m differs from the open window (1.16 / 1.10 / 1.20)", sill.0, lintel.0, opening), w.assembly);
                }
            }
            _ => add("WALL", w.name.clone(), w.at, "window without sill or lintel".to_string(), w.assembly),
        }
        let shut = w.prefab.contains("window_shut") || w.prefab.contains("window_blind");
        // Окно смотрит в стену.
        let mid = ((w.u.0 + w.u.1) * 0.5, (w.y.0 + w.y.1) * 0.5);
        if [0.25, 0.45].iter().any(|d| in_wall(point(w, mid.0, mid.1, *d))) {
            add("WALL", w.name.clone(), w.at, "faces a wall closer than 0.5 m".to_string(), w.assembly);
        }
        // Реквизит выше подоконника прямо перед окном.
        if !shut {
            let sill = w.y.0 + 0.08;
            for (assembly, list) in by_assembly {
                let n = assembly_name(world, *assembly);
                if !RESTING.contains(&n.as_str()) || n == "awning" {
                    continue;
                }
                let lo = list.iter().map(|i| pieces[*i].min).fold(Vec3::splat(f32::MAX), Vec3::min);
                let hi = list.iter().map(|i| pieces[*i].max).fold(Vec3::splat(f32::MIN), Vec3::max);
                if hi.y <= sill {
                    continue;
                }
                let cs = [lo, Vec3::new(hi.x, lo.y, lo.z), Vec3::new(lo.x, lo.y, hi.z), Vec3::new(hi.x, lo.y, hi.z)];
                let us: Vec<f32> = cs.iter().map(|c| (*c - w.at).dot(w.along)).collect();
                let ds: Vec<f32> = cs.iter().map(|c| (*c - w.at).dot(w.out)).collect();
                let (u0, u1) = (us.iter().copied().fold(f32::MAX, f32::min), us.iter().copied().fold(f32::MIN, f32::max));
                let (d0, d1) = (ds.iter().copied().fold(f32::MAX, f32::min), ds.iter().copied().fold(f32::MIN, f32::max));
                if u0 < w.u.1 && u1 > w.u.0 && d1 > 0.0 && d0 < 1.2 {
                    add("WALL", w.name.clone(), w.at, format!("open window behind {n} ({:.2} m tall, sill at {:.2} m)", hi.y, sill), w.assembly);
                    break;
                }
            }
        }
    }
}

/// Таблица: категория, имя, место, подробности; плюс счётчики по категориям.
pub fn table(offenders: &[Offender]) -> String {
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for o in offenders {
        *counts.entry(o.category).or_default() += 1;
    }
    let mut s = format!("offenders: {} {:?}\n", offenders.len(), counts);
    for o in offenders {
        s.push_str(&format!("{:9} {:40} ({:7.2},{:6.2},{:7.2})  {}\n", o.category, o.name, o.at.x, o.at.y, o.at.z, o.detail));
    }
    s
}
