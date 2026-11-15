//! Обход сцены для извлечения кадра и отсечение по пирамиде видимости.
//!
//! Зачем свой обход вместо `scene.ids()` + `is_active` + `world_matrix`:
//! * `ids()` выделяет вектор на всю сцену каждый кадр;
//! * `is_active` и `world_matrix` идут вверх по родителям для каждого объекта —
//!   в глубокой иерархии это O(объектов × глубина);
//! * здесь неактивное поддерево пропускается целиком, а мировая матрица считается
//!   один раз на объект и только если она кому-то нужна (лениво, сверху вниз).

use engine_core::prelude::*;

/// Обход активных объектов в порядке иерархии (родитель раньше детей) — тот же
/// порядок, что у `Scene::ids()`, без выделения памяти на каждый объект.
pub(crate) struct SceneWalk<'a> {
    scene: &'a Scene,
    /// Кто ещё не посещён: объект и его глубина.
    stack: Vec<(GameObjectId, u32)>,
    /// Цепочка от корня до текущего объекта; мировая матрица — когда посчитана.
    path: Vec<(&'a GameObject, Option<Mat4>)>,
    /// Сколько мировых матриц пришлось посчитать (для статистики).
    pub computed: u32,
}

impl<'a> SceneWalk<'a> {
    /// `stack` — переиспользуемый буфер (чтобы не выделять память каждый кадр).
    pub fn new(scene: &'a Scene, mut stack: Vec<(GameObjectId, u32)>) -> Self {
        stack.clear();
        stack.extend(scene.roots().iter().rev().map(|&id| (id, 0)));
        Self {
            scene,
            stack,
            path: Vec::with_capacity(16),
            computed: 0,
        }
    }

    /// Следующий объект, активный в иерархии.
    pub fn next(&mut self) -> Option<(GameObjectId, &'a GameObject)> {
        while let Some((id, depth)) = self.stack.pop() {
            let Some(object) = self.scene.get(id) else {
                continue;
            };
            // Неактивный объект выключает всё своё поддерево: детей даже не смотрим.
            if !object.active {
                continue;
            }
            // В обходе «в глубину» все предки объекта глубины d лежат в path[..d].
            self.path.truncate(depth as usize);
            self.path.push((object, None));
            self.stack
                .extend(object.children().iter().rev().map(|&child| (child, depth + 1)));
            return Some((id, object));
        }
        None
    }

    /// Мировая матрица текущего объекта (последнего из [`next`](Self::next)).
    /// Посчитанные матрицы предков переиспользуются братьями и потомками.
    pub fn world(&mut self) -> Mat4 {
        let top = self.path.len() - 1;
        if let Some(matrix) = self.path[top].1 {
            return matrix;
        }
        // Ближайший предок с готовой матрицей.
        let mut first = top;
        while first > 0 && self.path[first - 1].1.is_none() {
            first -= 1;
        }
        // У корня мировая матрица — сама локальная: без умножения на единичную.
        let mut matrix = if first == 0 {
            let root = self.path[0].0.transform.local_matrix();
            self.path[0].1 = Some(root);
            self.computed += 1;
            first = 1;
            root
        } else {
            self.path[first - 1].1.unwrap_or(Mat4::IDENTITY)
        };
        for entry in &mut self.path[first..=top] {
            matrix *= entry.0.transform.local_matrix();
            entry.1 = Some(matrix);
            self.computed += 1;
        }
        matrix
    }

    /// Вернуть буфер для следующего кадра.
    pub fn into_stack(self) -> Vec<(GameObjectId, u32)> {
        self.stack
    }
}

/// Пирамида видимости: шесть плоскостей `n·p + d ≥ 0` внутри.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Из матрицы «мир → clip» (глубина wgpu: 0..1). Плоскости z = 0 и z = w — пара
    /// «ближняя/дальняя» в любом порядке, поэтому годится и для обратной глубины сцены,
    /// и для обычной у карты теней.
    pub fn from_view_proj(m: Mat4) -> Self {
        let rows = m.transpose();
        let (r0, r1, r2, r3) = (rows.x_axis, rows.y_axis, rows.z_axis, rows.w_axis);
        let normalize = |p: Vec4| {
            let length = p.truncate().length();
            if length > 0.0 && length.is_finite() {
                p / length
            } else {
                p
            }
        };
        Self {
            planes: [
                normalize(r3 + r0),
                normalize(r3 - r0),
                normalize(r3 + r1),
                normalize(r3 - r1),
                normalize(r2),
                normalize(r3 - r2),
            ],
        }
    }

    /// Пересекает ли пирамиду прямоугольный объём `min..max` (консервативно:
    /// «да» для части объёмов у углов пирамиды — они просто рисуются).
    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        // Неконечные границы (NaN в Transform) не отсекаем: пусть видно будет ошибку.
        if !(min.is_finite() && max.is_finite()) {
            return true;
        }
        self.planes.iter().all(|plane| {
            let normal = plane.truncate();
            // Самая «внутренняя» вершина объёма вдоль нормали плоскости.
            let corner = Vec3::select(normal.cmpge(Vec3::ZERO), max, min);
            normal.dot(corner) + plane.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_matches_ids_order_and_matrices() {
        let mut scene = Scene::new("walk");
        let a = scene.build("A").at(1.0, 0.0).id();
        let b = scene.build("B").at(0.0, 2.0).child_of(a).id();
        let hidden = scene.build("Hidden").inactive().child_of(a).id();
        scene.build("UnderHidden").child_of(hidden);
        let c = scene.build("C").at3(0.0, 0.0, 3.0).child_of(b).id();
        let d = scene.build("D").id();
        let mut walk = SceneWalk::new(&scene, Vec::new());
        let mut seen = Vec::new();
        while let Some((id, _)) = walk.next() {
            seen.push((id, walk.world()));
        }
        let expected: Vec<_> = scene.ids().into_iter().filter(|id| scene.is_active(*id)).collect();
        assert_eq!(seen.iter().map(|(id, _)| *id).collect::<Vec<_>>(), expected);
        assert_eq!(expected, vec![a, b, c, d]);
        for (id, matrix) in seen {
            assert!(matrix.abs_diff_eq(scene.world_matrix(id), 1e-6));
        }
    }

    #[test]
    fn frustum_rejects_boxes_outside() {
        let view_proj = crate::depth::orthographic(-10.0, 10.0, -5.0, 5.0, -100.0, 100.0);
        let frustum = Frustum::from_view_proj(view_proj);
        assert!(frustum.intersects_aabb(Vec3::splat(-1.0), Vec3::splat(1.0)));
        assert!(frustum.intersects_aabb(Vec3::new(9.5, 0.0, 0.0), Vec3::new(11.0, 1.0, 0.0)));
        assert!(!frustum.intersects_aabb(Vec3::new(11.0, 0.0, 0.0), Vec3::new(12.0, 1.0, 0.0)));
        assert!(!frustum.intersects_aabb(Vec3::new(0.0, -7.0, 0.0), Vec3::new(1.0, -6.0, 0.0)));
        let perspective = crate::depth::perspective(1.0, 1.0, 0.1, 100.0) * Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let frustum = Frustum::from_view_proj(perspective);
        assert!(frustum.intersects_aabb(Vec3::new(-1.0, -1.0, -11.0), Vec3::new(1.0, 1.0, -9.0)));
        assert!(!frustum.intersects_aabb(Vec3::new(-1.0, -1.0, 9.0), Vec3::new(1.0, 1.0, 11.0)), "behind");
        assert!(!frustum.intersects_aabb(Vec3::new(-1.0, -1.0, -300.0), Vec3::new(1.0, 1.0, -200.0)), "too far");
    }
}
