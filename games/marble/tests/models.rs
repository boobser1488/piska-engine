//! Модели игры — замкнутые поверхности с гранями наружу.
//!
//! Вывернутую модель (обход треугольников в обратную сторону) рендер рисует «насквозь»:
//! наружные грани отбрасываются как задние, а видны внутренние. Так выглядел ротор
//! вентилятора, пока `tools/gen_assets.py` описывал его в зеркальной системе координат.

use std::collections::HashMap;
use std::path::Path;

type Point = [i64; 3];

/// Треугольники `.obj` с координатами, округлёнными до 1e-4 (для сравнения рёбер).
fn triangles(path: &Path) -> Vec<[[f64; 3]; 3]> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("v") => {
                let v: Vec<f64> = parts.take(3).map(|p| p.parse().expect("a vertex coordinate")).collect();
                vertices.push([v[0], v[1], v[2]]);
            }
            Some("f") => {
                let corners: Vec<[f64; 3]> = parts
                    .map(|p| {
                        let index: i64 = p.split('/').next().unwrap().parse().expect("a face index");
                        let i = if index > 0 { index as usize - 1 } else { (vertices.len() as i64 + index) as usize };
                        vertices[i]
                    })
                    .collect();
                for k in 1..corners.len() - 1 {
                    out.push([corners[0], corners[k], corners[k + 1]]);
                }
            }
            _ => {}
        }
    }
    out
}

fn key(p: [f64; 3]) -> Point {
    [(p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64]
}

#[test]
fn models_are_closed_and_face_outward() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("models");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("assets/models") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("obj") {
            continue;
        }
        let triangles = triangles(&path);
        assert!(!triangles.is_empty(), "{}: no faces", path.display());
        // Ориентированный объём: положительный, когда грани обходятся против часовой
        // стрелки, если смотреть снаружи.
        let volume: f64 = triangles
            .iter()
            .map(|[a, b, c]| {
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
            })
            .sum();
        assert!(volume > 0.0, "{}: faces point inward (signed volume {volume:.4})", path.display());
        // У замкнутой поверхности каждое ребро встречается и в обратную сторону.
        let mut edges: HashMap<(Point, Point), u32> = HashMap::new();
        for [a, b, c] in &triangles {
            for (p, q) in [(a, b), (b, c), (c, a)] {
                *edges.entry((key(*p), key(*q))).or_default() += 1;
            }
        }
        let open = edges.keys().filter(|(p, q)| !edges.contains_key(&(*q, *p))).count();
        assert_eq!(open, 0, "{}: the surface is not closed ({open} open edges)", path.display());
        checked += 1;
    }
    assert!(checked >= 3, "expected the gear, cog and fan rotor models, found {checked}");
}
