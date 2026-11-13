//! Transform — обязательная часть каждого GameObject'а.

use crate::math::{EulerRot, Mat4, Quat, Vec3};

/// Локальные (относительно родителя) позиция, поворот и масштаб.
/// Мировые значения даёт сцена: [`Scene::world_matrix`](crate::scene::Scene::world_matrix).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_xy(x: f32, y: f32) -> Self {
        Self::from_xyz(x, y, 0.0)
    }

    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: Vec3::new(x, y, z),
            ..Self::IDENTITY
        }
    }

    pub fn local_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    pub fn translate(&mut self, delta: Vec3) {
        self.position += delta;
    }

    /// Поворот вокруг Z (радианы) — основной поворот в 2D.
    pub fn rotate_z(&mut self, angle: f32) {
        self.rotation = Quat::from_rotation_z(angle) * self.rotation;
    }

    /// Поворот вокруг Y (радианы) — «рыскание» в 3D.
    pub fn rotate_y(&mut self, angle: f32) {
        self.rotation = Quat::from_rotation_y(angle) * self.rotation;
    }

    /// Угол поворота вокруг Z в радианах (2D). Для чисто плоских поворотов
    /// обратен [`set_angle_z`](Self::set_angle_z).
    pub fn angle_z(&self) -> f32 {
        self.rotation.to_euler(EulerRot::ZYX).0
    }

    /// Выставить поворот вокруг Z (радианы), сбросив остальные оси.
    pub fn set_angle_z(&mut self, angle: f32) {
        self.rotation = Quat::from_rotation_z(angle);
    }

    /// Углы Эйлера в градусах (X, Y, Z) — то, что показывает инспектор.
    pub fn euler_deg(&self) -> Vec3 {
        let (y, x, z) = self.rotation.to_euler(EulerRot::YXZ);
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }

    pub fn set_euler_deg(&mut self, euler: Vec3) {
        self.rotation = Quat::from_euler(
            EulerRot::YXZ,
            euler.y.to_radians(),
            euler.x.to_radians(),
            euler.z.to_radians(),
        );
    }

    /// Направление «вперёд» (−Z, правосторонняя система координат).
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// Развернуть «вперёд» на точку `target`.
    pub fn look_at(&mut self, target: Vec3, up: Vec3) {
        let dir = target - self.position;
        if dir.length_squared() > 1e-12 {
            self.rotation = Quat::from_mat4(&Mat4::look_to_rh(Vec3::ZERO, dir, up)).inverse();
        }
    }
}
