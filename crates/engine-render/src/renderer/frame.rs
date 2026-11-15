//! Данные кадра, которые извлечение готовит для проходов: экземпляры мешей,
//! их пачки для инстансинга и мелкие кэши, убирающие поиск в хеш-таблицах из
//! внутренних циклов.

use std::any::{Any, TypeId};
use std::ops::Range;

use engine_core::math::Mat3;
use engine_core::prelude::*;

use super::MeshKey;
use crate::components::{Environment, Light, MeshRenderer, SpriteRenderer};
use crate::particles::ParticleSystem;
use crate::tilemap::Tilemap;
use crate::trail::TrailRenderer;

/// Параметры одного экземпляра меша — второй вершинный буфер с шагом «экземпляр»
/// (см. `Instance` в common.wgsl). Вместо uniform-буфера с динамическим смещением на
/// каждый объект: одинаковые меши рисуются одним draw call'ом, а данные всех объектов
/// уходят на GPU одним сплошным `write_buffer`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct MeshInstance {
    model: [f32; 16],
    /// Столбцы обратной транспонированной 3×3 (выровнены до vec4).
    normal: [f32; 12],
    base_color: [f32; 4],
    emissive: [f32; 4],
    params: [f32; 4],
}

/// Атрибуты экземпляра для конвейера мешей (позиции 3..=12).
pub(super) const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
    7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
    10 => Float32x4, 11 => Float32x4, 12 => Float32x4
];

/// Только матрица модели — для прохода теней (тот же буфер и шаг).
pub(super) const INSTANCE_MODEL_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
];

/// Часть «весь меш» (у меша нет частей или все рисуются одним материалом).
pub(super) const WHOLE_MESH: u32 = u32::MAX;

/// Меш (или его часть со своим материалом), найденный при извлечении.
pub(super) struct MeshItem {
    pub key: MeshKey,
    /// Номер меша на GPU — ключ сортировки (дешевле, чем хешировать `MeshKey`).
    pub mesh_id: u32,
    /// Номер части меша или [`WHOLE_MESH`].
    pub part: u32,
    /// Группа привязок материала (см. material.rs).
    pub bind: u32,
    /// Конвейер основного прохода и теней (`PASS_*`, `SHADOW_*` в material.rs).
    pub pass: u8,
    pub shadow: u8,
    pub matrix: Mat4,
    pub base_color: [f32; 4],
    pub emissive: [f32; 4],
    pub params: [f32; 4],
    pub casts_shadows: bool,
    /// Попадает в пирамиду камеры.
    pub visible: bool,
    pub world_bounds: (Vec3, Vec3),
}

impl MeshItem {
    /// Экземпляр для основного прохода (с матрицей нормалей).
    pub fn instance(&self) -> MeshInstance {
        let normal = Mat3::from_mat4(self.matrix).inverse().transpose();
        let column = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let mut normal_columns = [0.0; 12];
        normal_columns[0..4].copy_from_slice(&column(normal.x_axis));
        normal_columns[4..8].copy_from_slice(&column(normal.y_axis));
        normal_columns[8..12].copy_from_slice(&column(normal.z_axis));
        MeshInstance {
            model: self.matrix.to_cols_array(),
            normal: normal_columns,
            base_color: self.base_color,
            emissive: self.emissive,
            params: self.params,
        }
    }

    /// Экземпляр для прохода теней: матрица модели и (для выреза по альфе) базовый цвет.
    pub fn shadow_instance(&self) -> MeshInstance {
        MeshInstance {
            model: self.matrix.to_cols_array(),
            normal: [0.0; 12],
            base_color: self.base_color,
            emissive: [0.0; 4],
            params: [0.0; 4],
        }
    }
}

/// Пачка экземпляров одной части меша с одной группой материала — один draw call.
pub(super) struct MeshBatch {
    /// Любой элемент пачки: по нему берутся меш и текстура.
    pub first: u32,
    pub instances: Range<u32>,
}

/// Буферы мешевой части кадра: переиспользуются, чтобы не выделять память каждый кадр.
#[derive(Default)]
pub(super) struct MeshScratch {
    pub items: Vec<MeshItem>,
    pub order: Vec<u32>,
    pub instances: Vec<MeshInstance>,
    pub batches: Vec<MeshBatch>,
    pub shadow_batches: Vec<MeshBatch>,
    /// Пачки теней каждого каскада (`SoftShadows`); без каскадов не используются.
    pub cascade_batches: Vec<Vec<MeshBatch>>,
    /// Пачки теней каждой грани теней ламп (`LightShadows`, lights.rs).
    pub face_batches: Vec<Vec<MeshBatch>>,
    /// Полупрозрачные — от дальних к ближним, рисуются после непрозрачных.
    pub blend: Vec<u32>,
    pub blend_batches: Vec<MeshBatch>,
}

/// Склеить отсортированные элементы `order` в пачки (соседние с одинаковым ключом
/// `same`) и дописать их экземпляры в `instances`.
pub(super) fn build_batches(
    items: &[MeshItem],
    order: &[u32],
    instances: &mut Vec<MeshInstance>,
    batches: &mut Vec<MeshBatch>,
    shadow: bool,
) {
    for &index in order {
        let item = &items[index as usize];
        let instance = instances.len() as u32;
        let same = |first: &MeshItem| {
            first.mesh_id == item.mesh_id
                && first.part == item.part
                && if shadow {
                    first.shadow == item.shadow && (item.shadow != super::material::SHADOW_CUTOUT || first.bind == item.bind)
                } else {
                    first.pass == item.pass && first.bind == item.bind
                }
        };
        match batches.last_mut() {
            Some(batch) if batch.instances.end == instance && same(&items[batch.first as usize]) => {
                batch.instances.end = instance + 1;
            }
            _ => batches.push(MeshBatch { first: index, instances: instance..instance + 1 }),
        }
        instances.push(if shadow { item.shadow_instance() } else { item.instance() });
    }
}

/// Мировые границы по локальным: центр переносится матрицей, полуразмеры — модулем
/// её 3×3. Тот же прямоугольник, что у восьми преобразованных углов, но втрое дешевле.
pub(super) fn aabb_transform(matrix: Mat4, (min, max): (Vec3, Vec3)) -> (Vec3, Vec3) {
    let center = matrix.transform_point3((min + max) * 0.5);
    let half = (max - min) * 0.5;
    let extent = Vec3::new(
        matrix.x_axis.x.abs() * half.x + matrix.y_axis.x.abs() * half.y + matrix.z_axis.x.abs() * half.z,
        matrix.x_axis.y.abs() * half.x + matrix.y_axis.y.abs() * half.y + matrix.z_axis.y.abs() * half.z,
        matrix.x_axis.z.abs() * half.x + matrix.y_axis.z.abs() * half.y + matrix.z_axis.z.abs() * half.z,
    );
    (center - extent, center + extent)
}

/// Несколько последних пар «ключ → значение». В кадре обычно горстка текстур и мешей,
/// и линейный просмотр восьми ячеек быстрее хеширования 128-битного GUID.
pub(super) struct RecentCache<K, V> {
    slots: [Option<(K, V)>; 8],
    next: usize,
}

impl<K, V> Default for RecentCache<K, V> {
    fn default() -> Self {
        Self {
            slots: Default::default(),
            next: 0,
        }
    }
}

impl<K: PartialEq + Copy, V: Copy> RecentCache<K, V> {
    pub fn get(&self, key: &K) -> Option<V> {
        self.slots.iter().flatten().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    pub fn put(&mut self, key: K, value: V) {
        self.slots[self.next] = Some((key, value));
        self.next = (self.next + 1) % self.slots.len();
    }
}

/// Как у `GameObject::get`: берётся первый компонент каждого типа.
fn first<'a, T: 'static>(slot: &mut Option<&'a T>, any: &'a dyn Any) {
    if slot.is_none() {
        *slot = any.downcast_ref::<T>();
    }
}

/// Компоненты объекта, которые интересны рендеру. Собираются одним проходом по
/// списку компонентов (один виртуальный вызов `type_id` на компонент) вместо шести
/// отдельных `get::<T>()`, каждый из которых заново перебирает весь список.
#[derive(Default)]
pub(super) struct RenderParts<'a> {
    pub light: Option<&'a Light>,
    pub sprite: Option<&'a SpriteRenderer>,
    pub particles: Option<&'a ParticleSystem>,
    pub tilemap: Option<&'a Tilemap>,
    pub trail: Option<&'a TrailRenderer>,
    pub mesh: Option<&'a MeshRenderer>,
    pub environment: Option<&'a Environment>,
}

impl<'a> RenderParts<'a> {
    pub fn of(object: &'a GameObject) -> Self {
        let mut parts = Self::default();
        for component in object.components() {
            let any: &dyn Any = component.as_any();
            let id = any.type_id();
            if id == TypeId::of::<SpriteRenderer>() {
                first(&mut parts.sprite, any);
            } else if id == TypeId::of::<MeshRenderer>() {
                first(&mut parts.mesh, any);
            } else if id == TypeId::of::<Light>() {
                first(&mut parts.light, any);
            } else if id == TypeId::of::<ParticleSystem>() {
                first(&mut parts.particles, any);
            } else if id == TypeId::of::<Tilemap>() {
                first(&mut parts.tilemap, any);
            } else if id == TypeId::of::<TrailRenderer>() {
                first(&mut parts.trail, any);
            } else if id == TypeId::of::<Environment>() {
                first(&mut parts.environment, any);
            }
        }
        parts
    }

    pub fn is_empty(&self) -> bool {
        self.light.is_none()
            && self.sprite.is_none()
            && self.particles.is_none()
            && self.tilemap.is_none()
            && self.trail.is_none()
            && self.mesh.is_none()
            && self.environment.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_transform_matches_corner_transform() {
        let matrix = Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 0.5, 3.0),
            Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.2),
            Vec3::new(5.0, -1.0, 2.0),
        );
        let local = (Vec3::new(-0.5, -1.0, -0.25), Vec3::new(0.5, 2.0, 0.75));
        let (a_min, a_max) = aabb_transform(matrix, local);
        let (b_min, b_max) = crate::renderer::transform_bounds(matrix, local);
        assert!(a_min.abs_diff_eq(b_min, 1e-4) && a_max.abs_diff_eq(b_max, 1e-4));
    }

    #[test]
    fn recent_cache_remembers_last_entries() {
        let mut cache = RecentCache::<u32, u32>::default();
        for i in 0..10 {
            cache.put(i, i * 10);
        }
        assert_eq!(cache.get(&9), Some(90));
        assert_eq!(cache.get(&2), Some(20));
        assert_eq!(cache.get(&1), None, "evicted");
    }
}
