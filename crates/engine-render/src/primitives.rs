//! Встроенные меши: куб, сфера, плоскость, цилиндр. Все вписаны в единичный куб
//! (от −0.5 до 0.5), обход граней — против часовой стрелки, если смотреть снаружи.

use engine_assets::MeshData;
use engine_core::prelude::*;

reflect_enum! {
    /// Встроенная форма для [`MeshRenderer`](crate::MeshRenderer).
    pub enum MeshShape { Cube, Sphere, Plane, Cylinder }
}

impl MeshShape {
    /// Данные меша этой формы.
    pub fn mesh(self) -> MeshData {
        match self {
            MeshShape::Cube => cube(),
            MeshShape::Sphere => sphere(32, 16),
            MeshShape::Plane => plane(),
            MeshShape::Cylinder => cylinder(32),
        }
    }

    /// Границы в локальных координатах (для выбора щелчком и наведения камеры).
    pub fn bounds(self) -> (Vec3, Vec3) {
        match self {
            MeshShape::Plane => (Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 0.0, 0.5)),
            _ => (Vec3::splat(-0.5), Vec3::splat(0.5)),
        }
    }
}

fn push_quad(mesh: &mut MeshData, corners: [Vec3; 4], normal: Vec3) {
    let base = mesh.positions.len() as u32;
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for (corner, uv) in corners.iter().zip(uvs) {
        mesh.positions.push(corner.to_array());
        mesh.normals.push(normal.to_array());
        mesh.uvs.push(uv);
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn cube() -> MeshData {
    let mut mesh = MeshData::default();
    // (нормаль, «вправо», «вверх»): вправо × вверх = нормаль.
    let faces = [
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    ];
    for (normal, right, up) in faces {
        let center = normal * 0.5;
        let (r, u) = (right * 0.5, up * 0.5);
        push_quad(
            &mut mesh,
            [center - r - u, center + r - u, center + r + u, center - r + u],
            normal,
        );
    }
    mesh
}

fn plane() -> MeshData {
    let mut mesh = MeshData::default();
    let (r, u) = (Vec3::X * 0.5, Vec3::NEG_Z * 0.5);
    push_quad(&mut mesh, [-r - u, r - u, r + u, -r + u], Vec3::Y);
    mesh
}

fn sphere(segments: u32, rings: u32) -> MeshData {
    let mut mesh = MeshData::default();
    for ring in 0..=rings {
        let theta = std::f32::consts::PI * ring as f32 / rings as f32;
        for segment in 0..=segments {
            let phi = std::f32::consts::TAU * segment as f32 / segments as f32;
            let normal = Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
            mesh.positions.push((normal * 0.5).to_array());
            mesh.normals.push(normal.to_array());
            mesh.uvs
                .push([1.0 - segment as f32 / segments as f32, ring as f32 / rings as f32]);
        }
    }
    let row = segments + 1;
    for ring in 0..rings {
        for segment in 0..segments {
            let a = ring * row + segment;
            let b = a + row;
            mesh.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    mesh
}

fn cylinder(segments: u32) -> MeshData {
    let mut mesh = MeshData::default();
    let point = |segment: u32, y: f32| {
        let phi = std::f32::consts::TAU * segment as f32 / segments as f32;
        Vec3::new(phi.cos() * 0.5, y, phi.sin() * 0.5)
    };

    // Боковая поверхность: верхнее кольцо, затем нижнее.
    for (y, v) in [(0.5, 0.0), (-0.5, 1.0)] {
        for segment in 0..=segments {
            let position = point(segment, y);
            mesh.positions.push(position.to_array());
            mesh.normals
                .push(Vec3::new(position.x, 0.0, position.z).normalize().to_array());
            mesh.uvs.push([1.0 - segment as f32 / segments as f32, v]);
        }
    }
    let row = segments + 1;
    for segment in 0..segments {
        let a = segment;
        let b = a + row;
        mesh.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
    }

    // Крышки: центр и кольцо со своей нормалью.
    for (y, normal) in [(0.5, Vec3::Y), (-0.5, Vec3::NEG_Y)] {
        let center = mesh.positions.len() as u32;
        mesh.positions.push([0.0, y, 0.0]);
        mesh.normals.push(normal.to_array());
        mesh.uvs.push([0.5, 0.5]);
        for segment in 0..=segments {
            let position = point(segment, y);
            mesh.positions.push(position.to_array());
            mesh.normals.push(normal.to_array());
            mesh.uvs.push([position.x + 0.5, 0.5 - position.z]);
        }
        for segment in 0..segments {
            let (current, next) = (center + 1 + segment, center + 2 + segment);
            if y > 0.0 {
                mesh.indices.extend([center, next, current]);
            } else {
                mesh.indices.extend([center, current, next]);
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Каждая грань смотрит наружу: её геометрическая нормаль сонаправлена с
    /// нормалями вершин. Иначе отсечение задних граней «съело» бы меш.
    #[test]
    fn primitives_are_closed_consistent_and_face_outward() {
        for shape in [MeshShape::Cube, MeshShape::Sphere, MeshShape::Plane, MeshShape::Cylinder] {
            let mesh = shape.mesh();
            assert_eq!(mesh.normals.len(), mesh.vertex_count(), "{shape:?}");
            assert_eq!(mesh.uvs.len(), mesh.vertex_count(), "{shape:?}");
            assert!(mesh.triangle_count() > 0);
            for triangle in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|i| i as usize);
                let (pa, pb, pc) = (
                    Vec3::from_array(mesh.positions[a]),
                    Vec3::from_array(mesh.positions[b]),
                    Vec3::from_array(mesh.positions[c]),
                );
                let face = (pb - pa).cross(pc - pa);
                if face.length() < 1e-7 {
                    continue; // вырожденные треугольники на полюсах сферы
                }
                let vertex_normal = Vec3::from_array(mesh.normals[a])
                    + Vec3::from_array(mesh.normals[b])
                    + Vec3::from_array(mesh.normals[c]);
                assert!(
                    face.dot(vertex_normal) > 0.0,
                    "{shape:?}: triangle {triangle:?} faces inward"
                );
            }
            let (min, max) = mesh.bounds();
            let (expected_min, expected_max) = shape.bounds();
            assert!(min.abs_diff_eq(expected_min, 1e-5) && max.abs_diff_eq(expected_max, 1e-5), "{shape:?}");
        }
    }
}
