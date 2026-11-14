//! Обратная глубина (reversed-Z): ближняя плоскость камеры → глубина 1, дальняя → 0,
//! буфер глубины очищается нулём, тест — «больше». В `Depth32Float` так точность почти
//! одинакова на любом расстоянии (при обычной 0 → 1 почти вся она уходит в первые метры,
//! и вдали близкие поверхности — стекло за рамой, плитка на мостовой — мерцают).
//!
//! Матрицы камер строят [`perspective`] и [`orthographic`]. Свой `CameraView::view_proj`
//! с обычной глубиной (`Mat4::perspective_rh` и т. п.) рендерер распознаёт и переводит
//! сам ([`to_reversed_depth`]), так что старый код рисует правильно — но точность тогда
//! уже потеряна в самой матрице; стройте матрицы этими функциями.
//!
//! Карта теней солнца — отдельная цель с обычной глубиной (ортографическая проекция,
//! точность там равномерна и так).

use engine_core::prelude::*;

/// Глубина ближней плоскости в кадре (обратная глубина).
pub const NEAR_DEPTH: f32 = 1.0;
/// Глубина дальней плоскости; ею очищается буфер глубины сцены.
pub const FAR_DEPTH: f32 = 0.0;

/// Перспективная проекция правой системы (камера смотрит в −Z) с обратной глубиной:
/// `near` → 1, `far` → 0.
pub fn perspective(fov_y_radians: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let near = near.max(1e-6);
    let far = far.max(near * (1.0 + 1e-4));
    // Тот же перспективный делитель, глубина — наоборот (ближняя и дальняя поменяны).
    Mat4::perspective_rh(fov_y_radians, aspect.max(1e-6), far, near)
}

/// Ортографическая проекция правой системы с обратной глубиной: `near` → 1, `far` → 0.
pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    Mat4::orthographic_rh(left, right, bottom, top, far, near)
}

/// Перевести матрицу «мир → clip» с обычной глубиной (0 у камеры) в обратную:
/// z' = w − z, т. е. глубина d → 1 − d.
pub fn to_reversed_depth(view_proj: Mat4) -> Mat4 {
    let flip = Mat4::from_cols(
        Vec4::new(1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 1.0),
    );
    flip * view_proj
}

/// Обратная ли глубина у матрицы `view_proj` камеры, стоящей в `eye`: точка с глубиной
/// 0.75 ближе к камере, чем с глубиной 0.25. Вырожденная матрица считается обратной
/// (её не трогаем).
pub fn uses_reversed_depth(view_proj: Mat4, eye: Vec3) -> bool {
    let inverse = view_proj.inverse();
    if !inverse.is_finite() {
        return true;
    }
    let point = |z: f32| {
        let p = inverse * Vec4::new(0.0, 0.0, z, 1.0);
        p.truncate() / p.w
    };
    let (a, b) = (point(0.25), point(0.75));
    if !(a.is_finite() && b.is_finite()) {
        return true;
    }
    b.distance_squared(eye) <= a.distance_squared(eye)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(m: Mat4, z: f32) -> f32 {
        let clip = m * Vec4::new(0.0, 0.0, -z, 1.0);
        clip.z / clip.w
    }

    #[test]
    fn near_is_one_and_far_is_zero() {
        let p = perspective(1.0, 1.5, 0.04, 900.0);
        assert!((depth(p, 0.04) - 1.0).abs() < 1e-5);
        assert!(depth(p, 900.0).abs() < 1e-5);
        assert!(depth(p, 10.0) > depth(p, 100.0));
        let o = orthographic(-2.0, 2.0, -1.0, 1.0, -50.0, 50.0);
        assert!((depth(o, -50.0) - 1.0).abs() < 1e-5 && depth(o, 50.0).abs() < 1e-5);
    }

    #[test]
    fn standard_matrices_are_recognised_and_converted() {
        let eye = Vec3::new(3.0, 2.0, 5.0);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let standard = Mat4::perspective_rh(1.0, 1.5, 0.1, 500.0) * view;
        let reversed = perspective(1.0, 1.5, 0.1, 500.0) * view;
        assert!(!uses_reversed_depth(standard, eye));
        assert!(uses_reversed_depth(reversed, eye));
        let converted = to_reversed_depth(standard);
        assert!(uses_reversed_depth(converted, eye));
        let p = Vec3::new(0.3, -0.2, 0.4).extend(1.0);
        let (a, b) = (converted * p, reversed * p);
        assert!((a.z / a.w - b.z / b.w).abs() < 1e-4 && (a.x / a.w - b.x / b.w).abs() < 1e-5);
        // Ортографическая 2D-камера (стоит «перед» плоскостью z = 0).
        let flat = Mat4::orthographic_rh(-5.0, 5.0, -3.0, 3.0, -1000.0, 1000.0);
        assert!(!uses_reversed_depth(flat, Vec3::new(0.0, 0.0, 1000.0)));
        assert!(uses_reversed_depth(orthographic(-5.0, 5.0, -3.0, 3.0, -1000.0, 1000.0), Vec3::new(0.0, 0.0, 1000.0)));
    }
}
