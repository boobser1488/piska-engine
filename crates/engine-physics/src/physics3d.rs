//! 3D-физика поверх Rapier3D — те же идеи, что в 2D: компоненты-данные
//! ([`Rigidbody`], [`Collider`], [`CharacterController`]), ресурс [`Physics3D`] для
//! запросов и суставов, подсистема [`Physics3DSystem`], которая на каждом фиксированном
//! шаге синхронизирует сцену с симуляцией и сглаживает позы между шагами.
//!
//! Объект с `Collider` без `Rigidbody` — статическая геометрия. Объект с
//! `CharacterController` — кинематическая капсула, которую двигает контроллер
//! персонажа (ходит по ступенькам, не въезжает в стены, толкает ящики).

use std::collections::HashMap;

use engine_assets::{AssetDatabase, AssetHandle, Mesh};
use engine_core::prelude::*;
use engine_core::settings::{LayerMatrix, Physics3dSettings};
use rapier3d::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier3d::na;
use rapier3d::prelude as rp;

use crate::BodyType;

mod heightfield;
pub use heightfield::HeightfieldTile;

reflect_enum! {
    /// Форма 3D-коллайдера.
    pub enum Shape3D { Box, Sphere, Capsule, Cylinder, ConvexHull, TriMesh }
}

component! {
    /// Твёрдое тело в 3D. Скорости можно читать и менять прямо из скриптов.
    pub struct Rigidbody {
        pub body_type: BodyType = BodyType::Dynamic,
        /// Линейная скорость, м/с. После каждого шага физики здесь актуальное значение.
        pub velocity: Vec3 = Vec3::ZERO,
        /// Угловая скорость, рад/с вокруг каждой оси.
        pub angular_velocity: Vec3 = Vec3::ZERO,
        pub gravity_scale: f32 = 1.0,
        pub linear_damping: f32 = 0.0,
        pub angular_damping: f32 = 0.05,
        /// Запретить вращение (персонажи, ящики на рельсах).
        pub lock_rotation: bool = false,
        /// Непрерывное обнаружение столкновений — для быстрых мелких тел (пули, мячи).
        pub ccd: bool = false,
        /// Сглаживать позу между шагами физики.
        pub interpolate: bool = true,
    }
}
impl Component for Rigidbody {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Rigidbody {
    pub fn dynamic() -> Self {
        Self::default()
    }

    pub fn kinematic() -> Self {
        Self {
            body_type: BodyType::Kinematic,
            ..Self::default()
        }
    }

    pub fn with_velocity(mut self, velocity: Vec3) -> Self {
        self.velocity = velocity;
        self
    }
}

component! {
    /// Коллайдер. Слой столкновений берётся из `GameObject::layer`.
    pub struct Collider {
        pub shape: Shape3D = Shape3D::Box,
        /// Полный размер для `Box`; для `Capsule` и `Cylinder` — (диаметр, высота, —).
        pub size: Vec3 = Vec3::ONE,
        /// Радиус для `Sphere`.
        pub radius: f32 = 0.5,
        pub offset: Vec3 = Vec3::ZERO,
        /// Меш для `ConvexHull` (динамические тела сложной формы) и `TriMesh`
        /// (точная статическая геометрия).
        pub mesh: AssetHandle<Mesh> = AssetHandle::none(),
        /// Триггер не толкает тела, а только сообщает о входе (`on_trigger_enter`).
        pub is_trigger: bool = false,
        pub friction: f32 = 0.5,
        pub restitution: f32 = 0.0,
        /// Плотность: масса тела = плотность × объём формы.
        pub density: f32 = 1.0,
        /// Маска слоёв, с которыми сталкивается коллайдер.
        pub collides_with: u32 = u32::MAX,
    }
}
impl Component for Collider {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl Collider {
    pub fn cuboid(x: f32, y: f32, z: f32) -> Self {
        Self {
            size: Vec3::new(x, y, z),
            ..Self::default()
        }
    }

    pub fn sphere(radius: f32) -> Self {
        Self {
            shape: Shape3D::Sphere,
            radius,
            ..Self::default()
        }
    }

    pub fn capsule(radius: f32, height: f32) -> Self {
        Self {
            shape: Shape3D::Capsule,
            size: Vec3::new(radius * 2.0, height, radius * 2.0),
            ..Self::default()
        }
    }

    pub fn cylinder(radius: f32, height: f32) -> Self {
        Self {
            shape: Shape3D::Cylinder,
            size: Vec3::new(radius * 2.0, height, radius * 2.0),
            ..Self::default()
        }
    }

    /// Точная статическая геометрия по мешу (уровни, рельеф).
    pub fn trimesh(mesh: AssetHandle<Mesh>) -> Self {
        Self {
            shape: Shape3D::TriMesh,
            mesh,
            ..Self::default()
        }
    }

    /// Выпуклая оболочка меша — для динамических тел сложной формы.
    pub fn convex_hull(mesh: AssetHandle<Mesh>) -> Self {
        Self {
            shape: Shape3D::ConvexHull,
            mesh,
            ..Self::default()
        }
    }

    pub fn trigger(mut self) -> Self {
        self.is_trigger = true;
        self
    }

    pub fn with_material(mut self, friction: f32, restitution: f32) -> Self {
        self.friction = friction;
        self.restitution = restitution;
        self
    }

    pub fn with_density(mut self, density: f32) -> Self {
        self.density = density;
        self
    }
}

component! {
    /// Персонаж: капсула, которую двигает контроллер (Unity: `CharacterController`).
    /// Скрипт задаёт `velocity` (горизонталь — куда идти, `y` — прыжок), контроллер сам
    /// добавляет гравитацию, поднимается на ступеньки и останавливается у стен.
    /// Позиция объекта — у ног персонажа.
    pub struct CharacterController {
        pub height: f32 = 1.8,
        pub radius: f32 = 0.35,
        /// На уступы какой высоты персонаж поднимается сам.
        pub step_height: f32 = 0.35,
        /// Склоны круче этого угла непроходимы.
        pub max_slope_deg: f32 = 50.0,
        /// Желаемая скорость, м/с.
        pub velocity: Vec3 = Vec3::ZERO,
        pub gravity_scale: f32 = 1.0,
        /// Стоит ли персонаж на земле (обновляется каждый шаг физики).
        pub grounded: bool = false,
    }
}
impl Component for CharacterController {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

/// Результат [`Physics3D::raycast`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit3D {
    pub object: GameObjectId,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

// ----------------------------------------------------------------------
// Запросы формой (cast / overlap / penetration): контроллер от первого лица и любой
// свой персонаж, которому нужны «трассы корпуса», как в классических движках.
// ----------------------------------------------------------------------

/// Что учитывают запросы формой ([`Physics3D::cast_shape`], [`Physics3D::cast_box`],
/// [`Physics3D::overlap_box`], [`Physics3D::compute_penetration_box`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueryFilter3D {
    /// Не видеть этот объект (обычно — себя).
    pub ignore: Option<GameObjectId>,
    /// Учитывать триггеры. По умолчанию — нет: запросы ищут твёрдую геометрию.
    pub include_triggers: bool,
    /// Маска слоёв объектов, которые видит запрос (бит `1 << layer`).
    pub layer_mask: u32,
}

impl Default for QueryFilter3D {
    fn default() -> Self {
        Self { ignore: None, include_triggers: false, layer_mask: u32::MAX }
    }
}

impl QueryFilter3D {
    /// Только твёрдая геометрия, все слои.
    pub fn solid() -> Self {
        Self::default()
    }

    /// Твёрдая геометрия без объекта `id`.
    pub fn ignoring(id: GameObjectId) -> Self {
        Self { ignore: Some(id), ..Self::default() }
    }

    /// То же, но и с триггерами.
    pub fn with_triggers(mut self) -> Self {
        self.include_triggers = true;
        self
    }

    /// Только объекты слоёв из маски.
    pub fn with_layers(mut self, mask: u32) -> Self {
        self.layer_mask = mask;
        self
    }
}

/// Форма для [`Physics3D::cast_shape`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CastShape3D {
    /// Прямоугольный ящик с половинами размеров.
    Box { half_extents: Vec3 },
    Sphere { radius: f32 },
    /// Капсула вдоль локальной оси Y: радиус и половина высоты цилиндрической части.
    Capsule { radius: f32, half_height: f32 },
}

/// Результат [`Physics3D::cast_shape`] / [`Physics3D::cast_box`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeHit3D {
    pub object: GameObjectId,
    /// Доля пути `motion`, пройденная до касания: 0..=1.
    pub fraction: f32,
    /// Пройденное расстояние до касания, м.
    pub distance: f32,
    /// Точка касания на поверхности задетого коллайдера (мировые координаты).
    pub point: Vec3,
    /// Нормаль поверхности задетого коллайдера в точке касания — смотрит на форму.
    pub normal: Vec3,
    /// Форма уже касалась коллайдера или была в нём в начале пути (`fraction == 0`).
    pub started_inside: bool,
}

/// Сустав между телами, см. методы `*_joint` у [`Physics3D`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JointId(u32);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    position: Vec3,
    rotation: Quat,
}

impl Pose {
    fn differs(self, other: Pose) -> bool {
        // Повороты сравниваются покомпонентно (q и −q — один поворот), а не через угол:
        // `angle_between` берёт acos от скалярного произведения, и для двух одинаковых
        // кватернионов погрешность f32 даёт «угол» до 7e-4 рад. Из-за этого вращающееся
        // тело каждый кадр считалось передвинутым из скрипта и откатывалось к сглаженной
        // позе: на частоте выше шага физики оно замирало и дёргалось.
        let (a, b) = (self.rotation, other.rotation);
        let rotation_gap = (a - b).length_squared().min((a + b).length_squared());
        (self.position - other.position).length_squared() > 1e-8 || rotation_gap > 2.5e-9
    }

    fn lerp(self, to: Pose, t: f32) -> Pose {
        Pose {
            position: self.position.lerp(to.position, t),
            rotation: self.rotation.slerp(to.rotation, t),
        }
    }

    fn isometry(self) -> na::Isometry3<f32> {
        let r = self.rotation;
        na::Isometry3::from_parts(
            na::Translation3::new(self.position.x, self.position.y, self.position.z),
            na::UnitQuaternion::from_quaternion(na::Quaternion::new(r.w, r.x, r.y, r.z)),
        )
    }

    fn of(isometry: &na::Isometry3<f32>) -> Pose {
        let (t, r) = (isometry.translation.vector, isometry.rotation);
        Pose {
            position: Vec3::new(t.x, t.y, t.z),
            rotation: Quat::from_xyzw(r.i, r.j, r.k, r.w),
        }
    }
}

fn vector(v: Vec3) -> na::Vector3<f32> {
    na::Vector3::new(v.x, v.y, v.z)
}

fn point(v: Vec3) -> na::Point3<f32> {
    na::Point3::new(v.x, v.y, v.z)
}

fn vec3(v: &na::Vector3<f32>) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Fixed,
    Dynamic,
    Kinematic,
    Character,
}

struct Entry {
    body: rp::RigidBodyHandle,
    collider: rp::ColliderHandle,
    /// Слой объекта и маска коллайдера (у персонажа — все слои): из них и матрицы слоёв
    /// собирается фильтр столкновений, он пересобирается при смене настроек.
    layer: u32,
    collides_with: u32,
    kind: Kind,
    interpolate: bool,
    /// Позы тела после предпоследнего и последнего шага симуляции.
    previous: Pose,
    current: Pose,
    /// Поза, которую физика записала в Transform последней.
    written: Pose,
    /// Смещение центра тела от позиции объекта (у персонажа позиция — у ног).
    center_offset: Vec3,
    /// Скорости, которые физика записала в `Rigidbody` последними. Если в компоненте
    /// другое значение — его поменял скрипт, и оно уходит в симуляцию. Сравнивать с
    /// самим телом нельзя: его скорость меняют ещё и импульсы.
    synced_velocity: Vec3,
    synced_angular: Vec3,
}

enum JointSpec {
    Fixed,
    Hinge { anchor: Vec3, axis: Vec3 },
    Ball { anchor: Vec3 },
    Rope { point_a: Vec3, point_b: Vec3, slack: f32 },
}

struct PendingJoint {
    id: JointId,
    a: GameObjectId,
    b: Option<GameObjectId>,
    spec: JointSpec,
}

enum Contact {
    Collision(GameObjectId, GameObjectId),
    Trigger(GameObjectId, GameObjectId),
}

/// Масса персонажа, с которой он толкает динамические тела.
const CHARACTER_MASS: f32 = 70.0;

/// Состояние 3D-физики. Лежит в `world.resources`:
/// `ctx.resource::<Physics3D>()?.raycast(..)`.
pub struct Physics3D {
    pub gravity: Vec3,
    pipeline: rp::PhysicsPipeline,
    params: rp::IntegrationParameters,
    islands: rp::IslandManager,
    broad_phase: rp::DefaultBroadPhase,
    narrow_phase: rp::NarrowPhase,
    bodies: rp::RigidBodySet,
    colliders: rp::ColliderSet,
    impulse_joints: rp::ImpulseJointSet,
    multibody_joints: rp::MultibodyJointSet,
    ccd: rp::CCDSolver,
    query: rp::QueryPipeline,
    entries: HashMap<GameObjectId, Entry>,
    by_collider: HashMap<rp::ColliderHandle, GameObjectId>,
    /// Неподвижное тело «мир» — к нему крепятся суставы без второго тела.
    world_body: rp::RigidBodyHandle,
    next_joint: u32,
    pending_joints: Vec<PendingJoint>,
    joints: HashMap<JointId, rp::ImpulseJointHandle>,
    /// Моторы петель, заданные до того, как сустав создан.
    pending_motors: Vec<(JointId, f32, f32)>,
    /// Тела, к которым приложили силу в этом шаге (после шага силы сбрасываются).
    forced: Vec<rp::RigidBodyHandle>,
    warned_mesh: bool,
    /// Плитки карт высот (рельеф): (владелец, плитка) → коллайдер, см. heightfield.rs.
    heightfields: HashMap<(GameObjectId, i32, i32), rp::ColliderHandle>,
    /// Матрица столкновений слоёв (Project Settings → Physics).
    layer_matrix: LayerMatrix,
    /// Пороги засыпания новых тел (линейный — «нормализованный», как в Rapier).
    sleep_linear: f32,
    sleep_angular: f32,
}

impl Default for Physics3D {
    fn default() -> Self {
        let mut bodies = rp::RigidBodySet::new();
        let world_body = bodies.insert(rp::RigidBodyBuilder::fixed());
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            pipeline: rp::PhysicsPipeline::new(),
            params: rp::IntegrationParameters::default(),
            islands: rp::IslandManager::new(),
            broad_phase: rp::DefaultBroadPhase::new(),
            narrow_phase: rp::NarrowPhase::new(),
            bodies,
            colliders: rp::ColliderSet::new(),
            impulse_joints: rp::ImpulseJointSet::new(),
            multibody_joints: rp::MultibodyJointSet::new(),
            ccd: rp::CCDSolver::new(),
            query: rp::QueryPipeline::new(),
            entries: HashMap::new(),
            by_collider: HashMap::new(),
            world_body,
            next_joint: 0,
            pending_joints: Vec::new(),
            joints: HashMap::new(),
            pending_motors: Vec::new(),
            forced: Vec::new(),
            warned_mesh: false,
            heightfields: HashMap::new(),
            layer_matrix: LayerMatrix::default(),
            sleep_linear: rp::RigidBodyActivation::default_normalized_linear_threshold(),
            sleep_angular: rp::RigidBodyActivation::default_angular_threshold(),
        }
    }
}

fn world_pose(scene: &Scene, id: GameObjectId) -> (Pose, Vec3) {
    let (scale, rotation, position) = scene.world_matrix(id).to_scale_rotation_translation();
    (Pose { position, rotation }, scale.abs())
}

/// Вершины и треугольники меша в локальных координатах коллайдера.
type MeshPoints = (Vec<na::Point3<f32>>, Vec<[u32; 3]>);

impl Physics3D {
    /// Сколько объектов сцены сейчас участвует в симуляции.
    pub fn body_count(&self) -> usize {
        self.entries.len()
    }

    /// Применить раздел Physics настроек проекта: гравитация, итерации решателя, пороги
    /// засыпания, матрица слоёв. Фильтры уже созданных коллайдеров (и плиток рельефа)
    /// и пороги уже созданных тел пересобираются сразу — можно вызывать на ходу.
    /// Вызывается один раз при применении настроек: гравитация, заданная кодом игры
    /// позже, не перезаписывается.
    pub fn apply_settings(&mut self, settings: &Physics3dSettings) {
        let [x, y, z] = settings.gravity;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            self.gravity = Vec3::new(x, y, z);
        }
        self.params.num_solver_iterations =
            std::num::NonZeroUsize::new(settings.solver_iterations.clamp(1, 64) as usize).expect("at least 1");
        let finite_or = |v: f32, fallback: f32| if v.is_finite() { v.max(0.0) } else { fallback };
        self.sleep_linear = finite_or(settings.sleep_linear_threshold, self.sleep_linear);
        self.sleep_angular = finite_or(settings.sleep_angular_threshold, self.sleep_angular);
        self.layer_matrix = settings.layer_matrix.clone();
        for entry in self.entries.values() {
            let groups = self.groups_for(entry.layer, entry.collides_with);
            if let Some(collider) = self.colliders.get_mut(entry.collider) {
                collider.set_collision_groups(groups);
            }
            if let Some(body) = self.bodies.get_mut(entry.body) {
                set_sleep_thresholds(body, self.sleep_linear, self.sleep_angular);
                body.wake_up(true);
            }
        }
        let tiles: Vec<rp::ColliderHandle> = self.heightfields.values().copied().collect();
        for handle in tiles {
            let Some(layer) = self
                .colliders
                .get(handle)
                .map(|c| c.collision_groups().memberships.bits().trailing_zeros().min(31))
            else {
                continue;
            };
            let groups = self.groups_for(layer, u32::MAX);
            if let Some(collider) = self.colliders.get_mut(handle) {
                collider.set_collision_groups(groups);
            }
        }
    }

    /// Итерации решателя контактов (Project Settings → Physics).
    pub fn solver_iterations(&self) -> u32 {
        self.params.num_solver_iterations.get() as u32
    }

    /// Действующая матрица столкновений слоёв.
    pub fn layer_matrix(&self) -> &LayerMatrix {
        &self.layer_matrix
    }

    /// Пороги засыпания (линейный, угловой), с которыми создаются тела.
    pub fn sleep_thresholds(&self) -> (f32, f32) {
        (self.sleep_linear, self.sleep_angular)
    }

    /// Фильтр столкновений: член своего слоя; сталкивается с тем, что разрешают и маска
    /// коллайдера, и матрица слоёв.
    pub(crate) fn groups_for(&self, layer: u32, collides_with: u32) -> rp::InteractionGroups {
        let layer = layer.min(31);
        rp::InteractionGroups::new(
            rp::Group::from_bits_truncate(1 << layer),
            rp::Group::from_bits_truncate(collides_with & self.layer_matrix.mask(layer)),
        )
    }

    fn body(&self, id: GameObjectId) -> Option<&rp::RigidBody> {
        self.bodies.get(self.entries.get(&id)?.body)
    }

    fn body_mut(&mut self, id: GameObjectId) -> Option<&mut rp::RigidBody> {
        self.bodies.get_mut(self.entries.get(&id)?.body)
    }

    // ------------------------------------------------------------------
    // Запросы
    // ------------------------------------------------------------------

    /// Unity: `Physics.Raycast`. Триггеры игнорируются; `ignore` — обычно сам стреляющий:
    /// луч идёт от его имени и не видит слоёв, с которыми его слой не сталкивается по
    /// матрице слоёв (Project Settings → Physics 3D). Видит мир на момент последнего шага.
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        ignore: Option<GameObjectId>,
    ) -> Option<RaycastHit3D> {
        let direction = direction.try_normalize()?;
        let ray = rp::Ray::new(point(origin), vector(direction));
        let mut filter = rp::QueryFilter::default().exclude_sensors();
        if let Some(entry) = ignore.and_then(|id| self.entries.get(&id)) {
            filter = filter.exclude_rigid_body(entry.body);
            if !self.layer_matrix.is_default() {
                filter = filter.groups(self.groups_for(entry.layer, u32::MAX));
            }
        }
        let (collider, hit) =
            self.query
                .cast_ray_and_get_normal(&self.bodies, &self.colliders, &ray, max_distance, true, filter)?;
        Some(RaycastHit3D {
            object: *self.by_collider.get(&collider)?,
            point: origin + direction * hit.time_of_impact,
            normal: vec3(&hit.normal),
            distance: hit.time_of_impact,
        })
    }

    /// Unity: `Physics.OverlapSphere` — объекты, чьи коллайдеры задевают сферу.
    pub fn overlap_sphere(&self, center: Vec3, radius: f32) -> Vec<GameObjectId> {
        let mut found = Vec::new();
        let shape = rp::Ball::new(radius);
        let position = na::Isometry3::translation(center.x, center.y, center.z);
        self.query.intersections_with_shape(
            &self.bodies,
            &self.colliders,
            &position,
            &shape,
            rp::QueryFilter::default(),
            |collider| {
                if let Some(id) = self.by_collider.get(&collider) {
                    if !found.contains(id) {
                        found.push(*id);
                    }
                }
                true
            },
        );
        found
    }

    /// Фильтр Rapier из [`QueryFilter3D`].
    fn shape_query_filter(&self, filter: &QueryFilter3D) -> rp::QueryFilter<'static> {
        let mut query = rp::QueryFilter::default();
        if !filter.include_triggers {
            query = query.exclude_sensors();
        }
        // Запрос от имени объекта `ignore` видит только слои, с которыми его слой
        // сталкивается по матрице слоёв (матрица по умолчанию — всё, как раньше).
        let mut membership = rp::Group::ALL;
        let mut mask = filter.layer_mask;
        if let Some(entry) = filter.ignore.and_then(|id| self.entries.get(&id)) {
            query = query.exclude_rigid_body(entry.body);
            if !self.layer_matrix.is_default() {
                let own = self.groups_for(entry.layer, u32::MAX);
                membership = own.memberships;
                mask &= own.filter.bits();
            }
        }
        if mask != u32::MAX || membership != rp::Group::ALL {
            query = query.groups(rp::InteractionGroups::new(membership, rp::Group::from_bits_truncate(mask)));
        }
        query
    }

    /// Unity: `Physics.BoxCast` / `SphereCast` / `CapsuleCast` — протащить форму из
    /// `position` (с поворотом `rotation`) на вектор `motion` и вернуть первое касание.
    ///
    /// Коллайдеры, в которых форма уже стоит, но из которых она выходит (движение от
    /// поверхности), не останавливают её — по полу можно скользить, стоя на нём. Если
    /// же движение направлено внутрь, касание будет с `fraction == 0` и
    /// `started_inside == true`. Видит мир на момент последнего шага физики. Нулевой
    /// `motion` — `None` (для проверки «стою ли я в чём-то» есть [`Self::overlap_box`]).
    pub fn cast_shape(
        &self,
        shape: CastShape3D,
        position: Vec3,
        rotation: Quat,
        motion: Vec3,
        filter: QueryFilter3D,
    ) -> Option<ShapeHit3D> {
        let length = motion.length();
        if length.is_nan() || length <= 1e-9 || !position.is_finite() || !motion.is_finite() {
            return None;
        }
        let pose = Pose { position, rotation: rotation.normalize() }.isometry();
        let query = self.shape_query_filter(&filter);
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let velocity = vector(motion);
        let (collider, hit) = match shape {
            CastShape3D::Box { half_extents } => return self.cast_cuboid(position, rotation.normalize(), half_extents, motion, query),
            CastShape3D::Sphere { radius } => {
                let ball = rp::Ball::new(radius.abs().max(1e-4));
                self.query.cast_shape(&self.bodies, &self.colliders, &pose, &velocity, &ball, options, query)?
            }
            CastShape3D::Capsule { radius, half_height } => {
                let capsule = rp::Capsule::new_y(half_height.abs(), radius.abs().max(1e-4));
                self.query.cast_shape(&self.bodies, &self.colliders, &pose, &velocity, &capsule, options, query)?
            }
        };
        let fraction = hit.time_of_impact.clamp(0.0, 1.0);
        Some(ShapeHit3D {
            object: *self.by_collider.get(&collider)?,
            fraction,
            distance: fraction * length,
            point: Vec3::new(hit.witness1.x, hit.witness1.y, hit.witness1.z),
            normal: vec3(&hit.normal1),
            started_inside: hit.status == rapier3d::parry::query::ShapeCastStatus::PenetratingOrWithinTargetDist,
        })
    }

    /// Ящик против мира: с коллайдерами-ящиками — точный расчёт по разделяющим осям
    /// (у общего GJK относительная погрешность ~0.1 % от размеров: на полу в десятки метров
    /// это миллиметры и сантиметры, а персонажу нужна точность до долей миллиметра),
    /// с остальными формами — GJK parry.
    fn cast_cuboid(
        &self,
        position: Vec3,
        rotation: Quat,
        half_extents: Vec3,
        motion: Vec3,
        query: rp::QueryFilter,
    ) -> Option<ShapeHit3D> {
        let half = half_extents.abs().max(Vec3::splat(1e-4));
        let axes = [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z];
        let reach = Vec3::new(
            half.x * axes[0].x.abs() + half.y * axes[1].x.abs() + half.z * axes[2].x.abs(),
            half.x * axes[0].y.abs() + half.y * axes[1].y.abs() + half.z * axes[2].y.abs(),
            half.x * axes[0].z.abs() + half.y * axes[1].z.abs() + half.z * axes[2].z.abs(),
        );
        let end = position + motion;
        let swept = rp::Aabb::new(
            point(position.min(end) - reach - Vec3::splat(0.01)),
            point(position.max(end) + reach + Vec3::splat(0.01)),
        );
        let pose = Pose { position, rotation }.isometry();
        let cuboid = rp::Cuboid::new(vector(half));
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        // (доля пути, нормаль, точка, внутри с начала, коллайдер)
        let mut best: Option<(f32, Vec3, Vec3, bool, rp::ColliderHandle)> = None;
        self.query.colliders_with_aabb_intersecting_aabb(&swept, |handle| {
            let Some(collider) = self.colliders.get(*handle) else {
                return true;
            };
            if !query.test(&self.bodies, *handle, collider) {
                return true;
            }
            let other = Pose::of(collider.position());
            let found = if let Some(target) = collider.shape().as_cuboid() {
                let target_axes = [other.rotation * Vec3::X, other.rotation * Vec3::Y, other.rotation * Vec3::Z];
                sweep_boxes(position, axes, half, motion, other.position, target_axes, vec3(&target.half_extents)).map(
                    |(t, normal, inside)| {
                        let at = position + motion * t;
                        let point = at - normal * (half.x * axes[0].dot(normal).abs() + half.y * axes[1].dot(normal).abs() + half.z * axes[2].dot(normal).abs());
                        (t, normal, point, inside)
                    },
                )
            } else {
                rapier3d::parry::query::cast_shapes(
                    collider.position(),
                    &na::Vector3::zeros(),
                    collider.shape(),
                    &pose,
                    &vector(motion),
                    &cuboid,
                    options,
                )
                .ok()
                .flatten()
                .map(|hit| {
                    let normal = other.rotation * vec3(&hit.normal1);
                    let point = other.position + other.rotation * Vec3::new(hit.witness1.x, hit.witness1.y, hit.witness1.z);
                    let inside = hit.status == rapier3d::parry::query::ShapeCastStatus::PenetratingOrWithinTargetDist;
                    (hit.time_of_impact.clamp(0.0, 1.0), normal, point, inside)
                })
            };
            if let Some((t, normal, point, inside)) = found {
                if best.is_none_or(|(b, ..)| t < b) {
                    best = Some((t, normal, point, inside, *handle));
                }
            }
            true
        });
        let (fraction, normal, point, started_inside, collider) = best?;
        Some(ShapeHit3D {
            object: *self.by_collider.get(&collider)?,
            fraction,
            distance: fraction * motion.length(),
            point,
            normal,
            started_inside,
        })
    }

    /// Протащить ящик, выровненный по осям мира (центр, половины размеров), на `motion`.
    /// То же, что [`Self::cast_shape`] с `CastShape3D::Box` без поворота.
    pub fn cast_box(&self, center: Vec3, half_extents: Vec3, motion: Vec3, filter: QueryFilter3D) -> Option<ShapeHit3D> {
        self.cast_shape(CastShape3D::Box { half_extents }, center, Quat::IDENTITY, motion, filter)
    }

    /// Unity: `Physics.OverlapBox` — объекты, чьи коллайдеры пересекают ящик, выровненный
    /// по осям (касание без пересечения не считается). Триггеры — по фильтру.
    pub fn overlap_box(&self, center: Vec3, half_extents: Vec3, filter: QueryFilter3D) -> Vec<GameObjectId> {
        let mut found = Vec::new();
        if !center.is_finite() {
            return found;
        }
        let cuboid = rp::Cuboid::new(vector(half_extents.abs().max(Vec3::splat(1e-4))));
        let position = na::Isometry3::translation(center.x, center.y, center.z);
        self.query.intersections_with_shape(
            &self.bodies,
            &self.colliders,
            &position,
            &cuboid,
            self.shape_query_filter(&filter),
            |collider| {
                if let Some(id) = self.by_collider.get(&collider) {
                    if !found.contains(id) {
                        found.push(*id);
                    }
                }
                true
            },
        );
        found
    }

    /// Unity: `Physics.ComputePenetration` для ящика, выровненного по осям: на какой
    /// вектор его сдвинуть, чтобы он не пересекал твёрдую геометрию (`None` — уже не
    /// пересекает). Несколько пересечений разбираются по очереди, от самого глубокого.
    pub fn compute_penetration_box(&self, center: Vec3, half_extents: Vec3, filter: QueryFilter3D) -> Option<Vec3> {
        let cuboid = rp::Cuboid::new(vector(half_extents.abs().max(Vec3::splat(1e-4))));
        let query = self.shape_query_filter(&filter);
        let mut push = Vec3::ZERO;
        for _ in 0..6 {
            let at = center + push;
            let position = na::Isometry3::translation(at.x, at.y, at.z);
            let mut deepest: Option<(f32, Vec3)> = None;
            self.query.intersections_with_shape(&self.bodies, &self.colliders, &position, &cuboid, query, |handle| {
                let Some(collider) = self.colliders.get(handle) else {
                    return true;
                };
                if let Ok(Some(contact)) =
                    rapier3d::parry::query::contact(collider.position(), collider.shape(), &position, &cuboid, 0.0)
                {
                    let depth = -contact.dist;
                    if depth > 0.0 && deepest.is_none_or(|(d, _)| depth > d) {
                        deepest = Some((depth, vec3(&contact.normal1)));
                    }
                }
                true
            });
            match deepest {
                Some((depth, normal)) if depth > 1e-6 => push += normal * (depth + 1e-4),
                _ => break,
            }
        }
        (push != Vec3::ZERO).then_some(push)
    }

    /// Поза тела объекта такой, какой её видят запросы (после последнего шага физики):
    /// позиция и поворот. Сравнивая её между шагами, персонаж «едет» на подвижной опоре.
    pub fn collider_pose(&self, id: GameObjectId) -> Option<(Vec3, Quat)> {
        let pose = Pose::of(self.body(id)?.position());
        Some((pose.position, pose.rotation))
    }

    pub fn is_sleeping(&self, id: GameObjectId) -> bool {
        self.body(id).is_some_and(|body| body.is_sleeping())
    }

    pub fn mass(&self, id: GameObjectId) -> f32 {
        self.body(id).map_or(0.0, |body| body.mass())
    }

    // ------------------------------------------------------------------
    // Силы и импульсы
    // ------------------------------------------------------------------

    /// Толчок в центр масс: мгновенно меняет скорость на `impulse / масса`.
    pub fn apply_impulse(&mut self, id: GameObjectId, impulse: Vec3) {
        if let Some(body) = self.body_mut(id) {
            body.apply_impulse(vector(impulse), true);
        }
    }

    /// Толчок в мировой точке тела: даёт и скорость, и вращение.
    pub fn apply_impulse_at(&mut self, id: GameObjectId, impulse: Vec3, world_point: Vec3) {
        if let Some(body) = self.body_mut(id) {
            body.apply_impulse_at_point(vector(impulse), point(world_point), true);
        }
    }

    pub fn apply_torque_impulse(&mut self, id: GameObjectId, torque: Vec3) {
        if let Some(body) = self.body_mut(id) {
            body.apply_torque_impulse(vector(torque), true);
        }
    }

    /// Сила на один шаг физики (вызывайте каждый `on_fixed_update`, пока она действует).
    pub fn add_force(&mut self, id: GameObjectId, force: Vec3) {
        let Some(handle) = self.entries.get(&id).map(|entry| entry.body) else {
            return;
        };
        if let Some(body) = self.bodies.get_mut(handle) {
            body.add_force(vector(force), true);
            self.forced.push(handle);
        }
    }

    /// Взрыв: динамические тела в радиусе получают скорость до `speed` от центра
    /// (слабее к краю). Возвращает задетые объекты.
    pub fn radial_impulse(&mut self, center: Vec3, radius: f32, speed: f32) -> Vec<GameObjectId> {
        let mut hit = Vec::new();
        for id in self.overlap_sphere(center, radius) {
            let Some(body) = self.body_mut(id).filter(|body| body.is_dynamic()) else {
                continue;
            };
            let offset = vec3(body.translation()) - center;
            let distance = offset.length();
            let direction = offset.try_normalize().unwrap_or(Vec3::Y);
            let falloff = (1.0 - distance / radius.max(1e-3)).clamp(0.0, 1.0);
            let impulse = direction * speed * falloff * body.mass();
            body.apply_impulse(vector(impulse), true);
            hit.push(id);
        }
        hit
    }

    // ------------------------------------------------------------------
    // Суставы. Создаются на ближайшем шаге физики (когда тела уже существуют);
    // `b = None` — крепление к миру.
    // ------------------------------------------------------------------

    fn queue_joint(&mut self, a: GameObjectId, b: Option<GameObjectId>, spec: JointSpec) -> JointId {
        let id = JointId(self.next_joint);
        self.next_joint += 1;
        self.pending_joints.push(PendingJoint { id, a, b, spec });
        id
    }

    /// Жёстко сварить `a` с `b`, сохранив их текущее взаимное положение.
    pub fn fixed_joint(&mut self, a: GameObjectId, b: Option<GameObjectId>) -> JointId {
        self.queue_joint(a, b, JointSpec::Fixed)
    }

    /// Петля (дверь, колесо) в мировой точке `anchor` вокруг мировой оси `axis`.
    pub fn hinge_joint(&mut self, a: GameObjectId, b: Option<GameObjectId>, anchor: Vec3, axis: Vec3) -> JointId {
        self.queue_joint(a, b, JointSpec::Hinge { anchor, axis })
    }

    /// Шаровой шарнир в мировой точке.
    pub fn ball_joint(&mut self, a: GameObjectId, b: Option<GameObjectId>, anchor: Vec3) -> JointId {
        self.queue_joint(a, b, JointSpec::Ball { anchor })
    }

    /// Верёвка между мировыми точками на `a` и `b`; длина — текущее расстояние плюс `slack`.
    pub fn rope_joint(
        &mut self,
        a: GameObjectId,
        point_a: Vec3,
        b: Option<GameObjectId>,
        point_b: Vec3,
        slack: f32,
    ) -> JointId {
        self.queue_joint(a, b, JointSpec::Rope { point_a, point_b, slack })
    }

    /// Мотор петли: целевая угловая скорость (рад/с) и предел усилия.
    pub fn set_hinge_motor(&mut self, joint: JointId, velocity: f32, max_force: f32) {
        self.pending_motors.push((joint, velocity, max_force));
        self.apply_motors();
    }

    pub fn remove_joint(&mut self, joint: JointId) {
        self.pending_joints.retain(|pending| pending.id != joint);
        if let Some(handle) = self.joints.remove(&joint) {
            self.impulse_joints.remove(handle, true);
        }
    }

    fn apply_motors(&mut self) {
        let motors = std::mem::take(&mut self.pending_motors);
        for (joint, velocity, max_force) in motors {
            let Some(found) = self.joints.get(&joint).and_then(|h| self.impulse_joints.get_mut(*h)) else {
                // Сустав ещё в очереди — мотор подождёт его создания.
                if self.pending_joints.iter().any(|pending| pending.id == joint) {
                    self.pending_motors.push((joint, velocity, max_force));
                }
                continue;
            };
            // Rapier ведёт второе тело относительно первого; наше `a` — первое, знак меняется.
            found.data.set_motor_velocity(rp::JointAxis::AngX, -velocity, 1.0);
            found.data.set_motor_max_force(rp::JointAxis::AngX, max_force);
            let (a, b) = (found.body1, found.body2);
            for handle in [a, b] {
                if let Some(body) = self.bodies.get_mut(handle) {
                    body.wake_up(true);
                }
            }
        }
    }

    fn create_pending_joints(&mut self) {
        let pending = std::mem::take(&mut self.pending_joints);
        for joint in pending {
            let Some(a) = self.entries.get(&joint.a).map(|entry| entry.body) else {
                log::warn!("joint: object {:?} has no physics body (add a Collider)", joint.a);
                continue;
            };
            let b = match joint.b {
                None => self.world_body,
                Some(id) => match self.entries.get(&id) {
                    Some(entry) => entry.body,
                    None => {
                        log::warn!("joint: object {id:?} has no physics body (add a Collider)");
                        continue;
                    }
                },
            };
            let (pose_a, pose_b) = (*self.bodies[a].position(), *self.bodies[b].position());
            let local = |pose: &na::Isometry3<f32>, world: Vec3| pose.inverse_transform_point(&point(world));
            let data: rp::GenericJoint = match joint.spec {
                JointSpec::Fixed => rp::FixedJointBuilder::new()
                    .local_frame1(na::Isometry3::identity())
                    .local_frame2(pose_b.inverse() * pose_a)
                    .contacts_enabled(false)
                    .into(),
                JointSpec::Hinge { anchor, axis } => {
                    let axis = vector(axis.try_normalize().unwrap_or(Vec3::Y));
                    let axis_a = pose_a.inverse_transform_vector(&axis);
                    let axis_b = pose_b.inverse_transform_vector(&axis);
                    let mut data: rp::GenericJoint = rp::RevoluteJointBuilder::new(na::Unit::new_normalize(axis_a))
                        .local_anchor1(local(&pose_a, anchor))
                        .local_anchor2(local(&pose_b, anchor))
                        .contacts_enabled(false)
                        .into();
                    data.set_local_axis2(na::Unit::new_normalize(axis_b));
                    data
                }
                JointSpec::Ball { anchor } => rp::SphericalJointBuilder::new()
                    .local_anchor1(local(&pose_a, anchor))
                    .local_anchor2(local(&pose_b, anchor))
                    .contacts_enabled(false)
                    .into(),
                JointSpec::Rope { point_a, point_b, slack } => {
                    rp::RopeJointBuilder::new((point_a - point_b).length() + slack.max(0.0))
                        .local_anchor1(local(&pose_a, point_a))
                        .local_anchor2(local(&pose_b, point_b))
                        .into()
                }
            };
            let handle = self.impulse_joints.insert(a, b, data, true);
            self.joints.insert(joint.id, handle);
        }
        self.apply_motors();
    }

    // ------------------------------------------------------------------
    // Синхронизация со сценой
    // ------------------------------------------------------------------

    fn remove_entry(&mut self, id: GameObjectId) {
        if let Some(entry) = self.entries.remove(&id) {
            self.by_collider.remove(&entry.collider);
            // Суставы тела удаляются вместе с ним.
            self.joints.retain(|_, handle| self.impulse_joints.get(*handle).is_some_and(|j| j.body1 != entry.body && j.body2 != entry.body));
            self.bodies.remove(
                entry.body,
                &mut self.islands,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                true,
            );
        }
    }

    fn mesh_points(
        &mut self,
        collider: &Collider,
        scale: Vec3,
        assets: Option<&AssetDatabase>,
        name: &str,
    ) -> Option<MeshPoints> {
        // Меш — через базу ассетов: так он читается и из каталога, и из пака (браузер, Android).
        let loaded = (!collider.mesh.is_none())
            .then(|| assets?.load_mesh(collider.mesh.guid()).ok())
            .flatten();
        match loaded {
            Some(mesh) => Some((
                mesh.positions
                    .iter()
                    .map(|p| point(Vec3::from_array(*p) * scale))
                    .collect(),
                mesh.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect(),
            )),
            None => {
                if !self.warned_mesh {
                    self.warned_mesh = true;
                    log::warn!(
                        "`{name}`: Collider shape {:?} needs a mesh asset in its `mesh` field; using a box instead",
                        collider.shape
                    );
                }
                None
            }
        }
    }

    fn add_entry(&mut self, scene: &Scene, id: GameObjectId, assets: Option<&AssetDatabase>) {
        let Some(object) = scene.get(id) else { return };
        let (pose, scale) = world_pose(scene, id);
        let layer = object.layer;

        // Персонаж: кинематическая капсула, центр — на полвысоты выше ног.
        if let Some(character) = object.get::<CharacterController>() {
            let radius = character.radius.max(0.05);
            let half_height = (character.height * 0.5 - radius).max(0.01);
            let center_offset = Vec3::Y * (half_height + radius);
            let body_pose = Pose {
                position: pose.position + center_offset,
                rotation: Quat::IDENTITY,
            };
            let body = self
                .bodies
                .insert(rp::RigidBodyBuilder::kinematic_position_based().position(body_pose.isometry()));
            let collider = self.colliders.insert_with_parent(
                rp::ColliderBuilder::capsule_y(half_height, radius)
                    .friction(0.0)
                    .collision_groups(self.groups_for(layer, u32::MAX)),
                body,
                &mut self.bodies,
            );
            self.by_collider.insert(collider, id);
            self.entries.insert(
                id,
                Entry {
                    body,
                    collider,
                    layer,
                    collides_with: u32::MAX,
                    kind: Kind::Character,
                    interpolate: true,
                    previous: pose,
                    current: pose,
                    written: pose,
                    center_offset,
                    synced_velocity: Vec3::ZERO,
                    synced_angular: Vec3::ZERO,
                },
            );
            return;
        }

        let Some(collider) = object.get::<Collider>() else {
            return;
        };
        let rigidbody = object.get::<Rigidbody>();
        let body_type = rigidbody.map_or(BodyType::Static, |rb| rb.body_type);
        let mut body = match body_type {
            BodyType::Dynamic => rp::RigidBodyBuilder::dynamic(),
            BodyType::Kinematic => rp::RigidBodyBuilder::kinematic_position_based(),
            BodyType::Static => rp::RigidBodyBuilder::fixed(),
        }
        .position(pose.isometry());
        if let Some(rb) = rigidbody {
            body = body
                .linvel(vector(rb.velocity))
                .angvel(vector(rb.angular_velocity))
                .gravity_scale(rb.gravity_scale)
                .linear_damping(rb.linear_damping)
                .angular_damping(rb.angular_damping)
                .ccd_enabled(rb.ccd);
            if rb.lock_rotation {
                body = body.lock_rotations();
            }
        }

        let half = (collider.size * scale * 0.5).max(Vec3::splat(1e-4));
        let boxed = rp::ColliderBuilder::cuboid(half.x, half.y, half.z);
        let builder = match collider.shape {
            Shape3D::Box => boxed,
            Shape3D::Sphere => rp::ColliderBuilder::ball((collider.radius * scale.max_element()).max(1e-4)),
            Shape3D::Capsule => rp::ColliderBuilder::capsule_y((half.y - half.x).max(0.0), half.x),
            Shape3D::Cylinder => rp::ColliderBuilder::cylinder(half.y, half.x),
            Shape3D::ConvexHull => self
                .mesh_points(collider, scale, assets, &object.name)
                .and_then(|(points, _)| rp::ColliderBuilder::convex_hull(&points))
                .unwrap_or(boxed),
            Shape3D::TriMesh => match self.mesh_points(collider, scale, assets, &object.name) {
                Some((points, triangles)) => rp::ColliderBuilder::trimesh(points, triangles),
                None => boxed,
            },
        }
        .translation(vector(collider.offset * scale))
        .sensor(collider.is_trigger)
        .friction(collider.friction)
        .restitution(collider.restitution)
        .density(collider.density.max(1e-4))
        .active_events(rp::ActiveEvents::COLLISION_EVENTS)
        .collision_groups(self.groups_for(layer, collider.collides_with));

        let body_handle = self.bodies.insert(body);
        if let Some(body) = self.bodies.get_mut(body_handle) {
            set_sleep_thresholds(body, self.sleep_linear, self.sleep_angular);
        }
        let collider_handle = self
            .colliders
            .insert_with_parent(builder, body_handle, &mut self.bodies);
        self.by_collider.insert(collider_handle, id);
        self.entries.insert(
            id,
            Entry {
                body: body_handle,
                collider: collider_handle,
                layer,
                collides_with: collider.collides_with,
                kind: match body_type {
                    BodyType::Dynamic => Kind::Dynamic,
                    BodyType::Kinematic => Kind::Kinematic,
                    BodyType::Static => Kind::Fixed,
                },
                interpolate: rigidbody.is_none_or(|rb| rb.interpolate),
                previous: pose,
                current: pose,
                written: pose,
                center_offset: Vec3::ZERO,
                synced_velocity: rigidbody.map_or(Vec3::ZERO, |rb| rb.velocity),
                synced_angular: rigidbody.map_or(Vec3::ZERO, |rb| rb.angular_velocity),
            },
        );
    }

    /// Скрипт или редактор подвинул `Transform` в обход физики — тело переносится туда же.
    fn absorb_moves(&mut self, scene: &Scene) {
        for (id, entry) in &mut self.entries {
            if !scene.contains(*id) {
                continue;
            }
            let (now, _) = world_pose(scene, *id);
            if !now.differs(entry.written) {
                continue;
            }
            let Some(body) = self.bodies.get_mut(entry.body) else {
                continue;
            };
            let target = Pose {
                position: now.position + entry.center_offset,
                rotation: if entry.kind == Kind::Character { Quat::IDENTITY } else { now.rotation },
            };
            if body.is_kinematic() && entry.kind != Kind::Character {
                body.set_next_kinematic_position(target.isometry());
            } else {
                body.set_position(target.isometry(), true);
            }
            entry.previous = now;
            entry.current = now;
            entry.written = now;
        }
    }

    /// Записать позы подвижных тел в `Transform`: с `alpha` — сглаженные, без — точные.
    fn write_poses(&mut self, scene: &mut Scene, alpha: Option<f32>) {
        for (id, entry) in &mut self.entries {
            if !matches!(entry.kind, Kind::Dynamic | Kind::Character) {
                continue;
            }
            let pose = match alpha {
                Some(alpha) if entry.interpolate => entry.previous.lerp(entry.current, alpha),
                _ => entry.current,
            };
            if !pose.differs(entry.written) {
                continue;
            }
            let parent = scene
                .get(*id)
                .and_then(|o| o.parent())
                .map(|parent| scene.world_matrix(parent));
            let Some(object) = scene.get_mut(*id) else {
                continue;
            };
            // Персонажа физика только переносит: поворотом управляет скрипт.
            let keep_rotation = entry.kind == Kind::Character;
            match parent {
                None => {
                    object.transform.position = pose.position;
                    if !keep_rotation {
                        object.transform.rotation = pose.rotation;
                    }
                }
                Some(parent) => {
                    object.transform.position = parent.inverse().transform_point3(pose.position);
                    if !keep_rotation {
                        let (_, parent_rotation, _) = parent.to_scale_rotation_translation();
                        object.transform.rotation = parent_rotation.inverse() * pose.rotation;
                    }
                }
            }
            let (read_back, _) = world_pose(scene, *id);
            entry.written = read_back;
        }
    }

    /// Перед `on_fixed_update`: скрипты должны видеть точные позы, а не сглаженные.
    pub fn begin_fixed_step(&mut self, scene: &mut Scene) {
        self.absorb_moves(scene);
        self.write_poses(scene, None);
    }

    /// Раз в кадр: сглаженные позы для отрисовки и для `on_update` / `on_late_update`.
    pub fn interpolate(&mut self, scene: &mut Scene, alpha: f32) {
        self.absorb_moves(scene);
        self.write_poses(scene, Some(alpha));
    }

    fn sync_in(&mut self, scene: &Scene, assets: Option<&AssetDatabase>) {
        let stale: Vec<GameObjectId> = self
            .entries
            .keys()
            .copied()
            .filter(|id| {
                !scene.is_active(*id)
                    || !scene
                        .get(*id)
                        .is_some_and(|o| o.has::<Collider>() || o.has::<CharacterController>())
            })
            .collect();
        for id in stale {
            self.remove_entry(id);
        }

        // Один проход по активным объектам. Объекты без коллайдера и персонажа (а их в
        // сцене обычно большинство) отсеиваются до поиска в таблице и до расчёта позы:
        // раньше каждый такой объект на каждом шаге проходил `add_entry` с полной
        // мировой матрицей.
        // (Записи без коллайдера и персонажа только что убраны как устаревшие.)
        // Кандидаты — по индексу сцены (только объекты с коллайдером или персонажем),
        // в порядке иерархии: тела создаются в том же порядке, что и при обходе сцены.
        let mut ids = scene.find_with::<Collider>();
        if scene.count_with::<CharacterController>() > 0 {
            ids.extend(scene.find_with::<CharacterController>());
            scene.sort_by_hierarchy(&mut ids);
            ids.dedup();
        }
        for id in ids {
            if !scene.is_active(id) {
                continue;
            }
            let Some(object) = scene.get(id) else {
                continue;
            };
            let Some(entry) = self.entries.get_mut(&id) else {
                self.add_entry(scene, id, assets);
                continue;
            };
            if entry.kind != Kind::Dynamic {
                continue;
            }
            // Скрипт поменял скорость в компоненте → передаём в симуляцию.
            let (Some(rb), Some(body)) = (object.get::<Rigidbody>(), self.bodies.get_mut(entry.body)) else {
                continue;
            };
            if (rb.velocity - entry.synced_velocity).length_squared() > 1e-10 {
                body.set_linvel(vector(rb.velocity), true);
                entry.synced_velocity = rb.velocity;
            }
            if (rb.angular_velocity - entry.synced_angular).length_squared() > 1e-10 {
                body.set_angvel(vector(rb.angular_velocity), true);
                entry.synced_angular = rb.angular_velocity;
            }
        }
    }

    /// Подвинуть персонажей: желаемая скорость + гравитация → допустимое перемещение.
    fn move_characters(&mut self, scene: &mut Scene, dt: f32) {
        let ids: Vec<GameObjectId> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.kind == Kind::Character)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let Some(character) = scene.get_component_mut::<CharacterController>(id) else {
                continue;
            };
            let Some(entry) = self.entries.get(&id) else {
                continue;
            };
            let radius = character.radius.max(0.05);
            let half_height = (character.height * 0.5 - radius).max(0.01);
            let shape = rp::Capsule::new_y(half_height, radius);

            // Гравитация копится в вертикальной скорости, пока персонаж в воздухе.
            if character.grounded && character.velocity.y < 0.0 {
                character.velocity.y = 0.0;
            }
            character.velocity += self.gravity * character.gravity_scale * dt;

            let controller = KinematicCharacterController {
                offset: CharacterLength::Absolute(0.02),
                autostep: Some(CharacterAutostep {
                    max_height: CharacterLength::Absolute(character.step_height.max(0.0)),
                    min_width: CharacterLength::Absolute(0.2),
                    include_dynamic_bodies: false,
                }),
                max_slope_climb_angle: character.max_slope_deg.to_radians(),
                min_slope_slide_angle: character.max_slope_deg.to_radians(),
                // К земле прижимаемся, только когда не прыгаем вверх.
                snap_to_ground: (character.velocity.y <= 0.0).then_some(CharacterLength::Absolute(0.25)),
                ..KinematicCharacterController::default()
            };
            let center = entry.current.position + entry.center_offset;
            let position = na::Isometry3::translation(center.x, center.y, center.z);
            let mut filter = rp::QueryFilter::default().exclude_rigid_body(entry.body).exclude_sensors();
            // Матрица слоёв: персонаж проходит сквозь слои, с которыми его слой не
            // сталкивается. Матрица по умолчанию — фильтр как раньше (без групп).
            if !self.layer_matrix.is_default() {
                filter = filter.groups(self.groups_for(entry.layer, u32::MAX));
            }
            let mut collisions: Vec<CharacterCollision> = Vec::new();
            let desired = vector(character.velocity * dt);
            let mut movement = controller.move_shape(
                dt,
                &self.bodies,
                &self.colliders,
                &self.query,
                &shape,
                &position,
                desired,
                filter,
                |collision| collisions.push(collision),
            );
            // Rapier 0.22 считает горизонтальный шаг в любой склон «намерением подняться» и
            // въезжает на склоны круче `max_slope_deg` (рельеф, пандусы). Если шаг поднял
            // персонажа и под ногами теперь слишком круто (а не ступенька с ровным верхом),
            // убираем часть шага, направленную в крутую поверхность, и пересчитываем.
            let steepest = character.max_slope_deg.to_radians();
            let mut blocked = desired;
            for collision in &collisions {
                let n = collision.hit.normal1;
                if n.y > 1e-3 && n.y.clamp(-1.0, 1.0).acos() > steepest + 1e-3 {
                    if let Some(across) = na::Vector3::new(n.x, 0.0, n.z).try_normalize(1e-6) {
                        let into = blocked.dot(&across);
                        if into < 0.0 {
                            blocked -= across * into;
                        }
                    }
                }
            }
            if blocked != desired && movement.translation.y > 1e-4 {
                let feet = entry.current.position + vec3(&movement.translation);
                let ray = rp::Ray::new(point(feet + Vec3::Y * 0.05), vector(Vec3::NEG_Y));
                let walkable = self
                    .query
                    .cast_ray_and_get_normal(&self.bodies, &self.colliders, &ray, 0.6, true, filter)
                    .is_some_and(|(_, hit)| hit.normal.y.clamp(-1.0, 1.0).acos() <= steepest + 1e-3);
                if walkable {
                    blocked = desired; // ступенька или пологий склон — шаг верный
                }
            } else {
                blocked = desired; // не поднялся — шагу Rapier можно верить
            }
            if blocked != desired {
                collisions.clear();
                movement = controller.move_shape(
                    dt,
                    &self.bodies,
                    &self.colliders,
                    &self.query,
                    &shape,
                    &position,
                    blocked,
                    filter,
                    |collision| collisions.push(collision),
                );
            }
            // Толкаем динамические тела, в которые упёрлись.
            controller.solve_character_collision_impulses(
                dt,
                &mut self.bodies,
                &self.colliders,
                &self.query,
                &shape,
                CHARACTER_MASS,
                &collisions,
                filter,
            );
            character.grounded = movement.grounded;
            if movement.grounded && character.velocity.y < 0.0 {
                character.velocity.y = 0.0;
            }

            let feet = entry.current.position + vec3(&movement.translation);
            let body_handle = entry.body;
            let offset = entry.center_offset;
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.previous = entry.current;
                entry.current.position = feet;
            }
            if let Some(body) = self.bodies.get_mut(body_handle) {
                let center = feet + offset;
                body.set_next_kinematic_translation(vector(center));
            }
        }
    }

    fn sync_out(&mut self, scene: &mut Scene) {
        for (id, entry) in &mut self.entries {
            if entry.kind != Kind::Dynamic {
                continue;
            }
            let Some(body) = self.bodies.get(entry.body) else {
                continue;
            };
            entry.previous = entry.current;
            entry.current = Pose::of(body.position());
            entry.synced_velocity = vec3(body.linvel());
            entry.synced_angular = vec3(body.angvel());
            if let Some(rb) = scene.get_component_mut::<Rigidbody>(*id) {
                rb.velocity = entry.synced_velocity;
                rb.angular_velocity = entry.synced_angular;
            }
        }
        self.write_poses(scene, None);
    }

    /// Один фиксированный шаг. Возвращает начавшиеся контакты.
    fn step(&mut self, world: &mut World, dt: f32) -> Vec<Contact> {
        self.absorb_moves(&world.scene);
        self.sync_in(&world.scene, world.resources.get::<AssetDatabase>());
        self.create_pending_joints();
        self.move_characters(&mut world.scene, dt);

        let (collision_tx, collision_rx) = rapier3d::crossbeam::channel::unbounded();
        let (force_tx, _force_rx) = rapier3d::crossbeam::channel::unbounded();
        let events = rp::ChannelEventCollector::new(collision_tx, force_tx);

        self.params.dt = dt;
        self.pipeline.step(
            &vector(self.gravity),
            &self.params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd,
            Some(&mut self.query),
            &(),
            &events,
        );
        for handle in std::mem::take(&mut self.forced) {
            if let Some(body) = self.bodies.get_mut(handle) {
                body.reset_forces(false);
            }
        }

        self.sync_out(&mut world.scene);

        let mut contacts = Vec::new();
        while let Ok(event) = collision_rx.try_recv() {
            if let rp::CollisionEvent::Started(a, b, flags) = event {
                let (Some(&a), Some(&b)) = (self.by_collider.get(&a), self.by_collider.get(&b)) else {
                    continue;
                };
                contacts.push(if flags.contains(rp::CollisionEventFlags::SENSOR) {
                    Contact::Trigger(a, b)
                } else {
                    Contact::Collision(a, b)
                });
            }
        }
        contacts
    }
}

/// Точное время касания движущегося ящика A (центр, оси, половины; путь `motion` за
/// t ∈ [0, 1]) с неподвижным ящиком B по разделяющим осям (3 + 3 оси граней и 9 рёбер).
/// Касание — пересечение объёмов: ящики, разделённые даже микронным зазором и
/// движущиеся параллельно, не касаются. Возвращает (t, нормаль от B к A, A уже был в B).
/// Если A с самого начала в B и уходит из него (или скользит вдоль) — `None`.
fn sweep_boxes(
    a_center: Vec3,
    a_axes: [Vec3; 3],
    a_half: Vec3,
    motion: Vec3,
    b_center: Vec3,
    b_axes: [Vec3; 3],
    b_half: Vec3,
) -> Option<(f32, Vec3, bool)> {
    let delta = b_center - a_center;
    let mut enter = f32::NEG_INFINITY;
    let mut enter_normal = Vec3::ZERO;
    let mut exit = f32::INFINITY;
    let mut shallowest = f32::INFINITY;
    let mut shallow_normal = Vec3::ZERO;
    let mut axes: [Vec3; 15] = [Vec3::ZERO; 15];
    axes[..3].copy_from_slice(&b_axes);
    axes[3..6].copy_from_slice(&a_axes);
    for (i, a) in a_axes.iter().enumerate() {
        for (j, b) in b_axes.iter().enumerate() {
            axes[6 + i * 3 + j] = a.cross(*b);
        }
    }
    for (index, axis) in axes.iter().enumerate() {
        let length = axis.length();
        if length < 1e-5 {
            continue; // рёбра параллельны: ось повторяет ось грани
        }
        let l = *axis / length;
        let radius = a_half.x * a_axes[0].dot(l).abs()
            + a_half.y * a_axes[1].dot(l).abs()
            + a_half.z * a_axes[2].dot(l).abs()
            + b_half.x * b_axes[0].dot(l).abs()
            + b_half.y * b_axes[1].dot(l).abs()
            + b_half.z * b_axes[2].dot(l).abs();
        let s = delta.dot(l);
        let v = motion.dot(l);
        let depth = radius - s.abs();
        // Нормаль от B к A на этой оси.
        let toward_a = if s > 0.0 { -l } else { l };
        if depth < shallowest {
            shallowest = depth;
            shallow_normal = toward_a;
        }
        if v.abs() < 1e-9 {
            if s.abs() >= radius {
                return None; // разделены и не сближаются по этой оси
            }
            continue;
        }
        let t0 = (s - radius) / v;
        let t1 = (s + radius) / v;
        let (t_in, t_out) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
        // Оси рёбер берём, только если они явно позже граней: на стыке ящиков
        // численная ничья иначе дала бы косую нормаль.
        let bias = if index < 6 { 0.0 } else { 1e-4 };
        if t_in > enter + bias {
            enter = t_in;
            // Нормаль на входе: с какой стороны A подошёл к B.
            let s_at = s - v * t_in;
            enter_normal = if s_at > 0.0 { -l } else { l };
        }
        exit = exit.min(t_out);
        if enter > exit || enter > 1.0 || exit < 0.0 {
            return None;
        }
    }
    if enter > exit || enter > 1.0 || exit < 0.0 {
        return None;
    }
    if enter >= 0.0 {
        return Some((enter, enter_normal, false));
    }
    // С самого начала внутри: выход — по самой мелкой оси; уходим по ней — не касание.
    if shallowest <= 0.0 || motion.dot(shallow_normal) >= 0.0 {
        return None;
    }
    Some((0.0, shallow_normal, true))
}

fn set_sleep_thresholds(body: &mut rp::RigidBody, linear: f32, angular: f32) {
    let activation = body.activation_mut();
    activation.normalized_linear_threshold = linear;
    activation.angular_threshold = angular;
}

/// Применить раздел Physics (3D) из ресурса [`ProjectSettings`](engine_core::settings::ProjectSettings)
/// к ресурсу [`Physics3D`]. Регистрируется хуком настроек в `engine::app()`; редактор,
/// поменяв настройки, вызывает `world.apply_project_settings(..)` — хук применит их.
pub fn apply_project_settings_3d(world: &mut World) {
    let Some(settings) = world.resources.get::<engine_core::settings::ProjectSettings>() else {
        return;
    };
    let section = settings.physics3d.clone();
    if !world.resources.contains::<Physics3D>() {
        world.resources.insert(Physics3D::default());
    }
    if let Some(physics) = world.resources.get_mut::<Physics3D>() {
        physics.apply_settings(&section);
    }
}

/// Подсистема, шагающая [`Physics3D`] на каждом фиксированном шаге.
#[derive(Default)]
pub struct Physics3DSystem;

impl Subsystem for Physics3DSystem {
    fn name(&self) -> &'static str {
        "physics3d"
    }

    fn init(&mut self, world: &mut World) {
        if !world.resources.contains::<Physics3D>() {
            world.resources.insert(Physics3D::default());
        }
    }

    fn before_fixed_update(&mut self, world: &mut World) {
        let World { resources, scene, .. } = world;
        if let Some(physics) = resources.get_mut::<Physics3D>() {
            physics.begin_fixed_step(scene);
        }
    }

    fn interpolate(&mut self, world: &mut World, alpha: f32) {
        let World { resources, scene, .. } = world;
        if let Some(physics) = resources.get_mut::<Physics3D>() {
            physics.interpolate(scene, alpha);
        }
    }

    fn fixed_update(&mut self, world: &mut World, dt: f32) {
        // Ресурс вынимается на время шага, чтобы одновременно иметь `&mut World`.
        let Some(mut physics) = world.resources.remove::<Physics3D>() else {
            return;
        };
        let contacts = physics.step(world, dt);
        world.resources.insert(physics);

        // Колбэки — уже после возврата ресурса: в них можно делать raycast.
        for contact in contacts {
            match contact {
                Contact::Collision(a, b) => {
                    world.dispatch_hook(a, Hooks::COLLISION_ENTER, |c, ctx| c.on_collision_enter(ctx, b));
                    world.dispatch_hook(b, Hooks::COLLISION_ENTER, |c, ctx| c.on_collision_enter(ctx, a));
                }
                Contact::Trigger(a, b) => {
                    world.dispatch_hook(a, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, b));
                    world.dispatch_hook(b, Hooks::TRIGGER_ENTER, |c, ctx| c.on_trigger_enter(ctx, a));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_assets::{AssetPack, Guid};

    #[test]
    fn mesh_collider_reads_its_mesh_from_a_pack() {
        let guid = Guid(0x3E5);
        let obj = "v 0 0 0
v 1 0 0
v 0 1 0
v 0 0 1
f 1 2 3
f 1 2 4
f 1 3 4
f 2 3 4
";
        let mut builder = AssetPack::builder();
        builder.add("meshes/tetra.obj", obj.as_bytes().to_vec());
        builder.add(
            "meshes/tetra.obj.meta",
            format!("(
    guid: \"{:032x}\",
    importer: \"Mesh\",
    settings: {{}},
)
", guid.0).into_bytes(),
        );
        let database = AssetDatabase::from_pack(AssetPack::from_bytes(builder.to_bytes()).unwrap()).unwrap();
        assert!(!database.path_of(guid).unwrap().exists(), "no file on disk: bytes come from the pack");
        let mut physics = Physics3D::default();
        let collider = Collider::trimesh(AssetHandle::from_guid(guid));
        let (points, triangles) = physics
            .mesh_points(&collider, Vec3::splat(2.0), Some(&database), "Tetra")
            .expect("the mesh is loaded from the pack");
        assert!(points.len() >= 4);
        assert_eq!(triangles.len(), 4);
        assert!(points.iter().any(|p| (p.x - 2.0).abs() < 1e-6), "scale is applied");
    }
}
