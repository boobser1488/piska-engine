//! `engine-physics` — физика в стиле Unity: 2D поверх Rapier2D (этот файл) и 3D
//! поверх Rapier3D (модуль `physics3d`: `Rigidbody`, `Collider`, `CharacterController`).
//!
//! Компоненты [`Rigidbody2D`] и [`Collider2D`] — чистые данные. Вся работа с Rapier
//! спрятана в ресурсе [`Physics2D`]; подсистема [`Physics2DSystem`] на каждом
//! фиксированном шаге синхронизирует сцену с симуляцией:
//!
//! 1. сцена → Rapier: новые/удалённые объекты, скорости и телепорты из скриптов;
//! 2. шаг симуляции;
//! 3. Rapier → сцена: позы в `Transform`, скорости в `Rigidbody2D::velocity`;
//! 4. события: `on_collision_enter` / `on_trigger_enter` у компонентов.
//!
//! Между шагами позы для отрисовки сглаживаются (`Rigidbody2D::interpolate`): физика
//! идёт 60 раз в секунду, а экран может обновляться 75 или 144 раза, и без сглаживания
//! движение дрожит. В `on_fixed_update` скрипт при этом видит точную позу тела.
//!
//! Трение пары тел — меньшее из двух (ноль у игрока не «прилипает» к шершавой стене),
//! упругость — большая из двух, как в Unity 2D.
//!
//! Объект с `Collider2D` без `Rigidbody2D` — статическая геометрия (как в Unity).
#![forbid(unsafe_code)]

// Возможности крейта: `2d` (Rapier2D: `Rigidbody2D`, `Collider2D`, `Physics2D`) и `3d`
// (Rapier3D: `Rigidbody`, `Collider`, `CharacterController`, `Physics3D`). По умолчанию
// включены обе; игра без физики (или с одной её половиной) не линкует лишний Rapier.
#[cfg(feature = "3d")]
mod physics3d;

#[cfg(feature = "3d")]
pub use physics3d::{
    apply_project_settings_3d, CharacterController, Collider, HeightfieldTile, JointId, Physics3D, Physics3DSystem,
    RaycastHit3D, Rigidbody, Shape3D,
};

#[cfg(feature = "2d")]
use std::collections::{HashMap, HashSet};

use engine_core::prelude::*;
#[cfg(feature = "2d")]
use engine_core::settings::{LayerMatrix, Physics2dSettings};
#[cfg(feature = "2d")]
use rapier2d::prelude as rp;

reflect_enum! {
    /// Тип тела. Unity: `Rigidbody2D.bodyType`.
    pub enum BodyType { Dynamic, Kinematic, Static }
}

#[cfg(feature = "2d")]
reflect_enum! {
    pub enum Shape2D { Box, Circle, Capsule }
}

#[cfg(feature = "2d")]
component! {
    /// Твёрдое тело. Скорость можно читать и менять прямо из скриптов.
    pub struct Rigidbody2D {
        pub body_type: BodyType = BodyType::Dynamic,
        /// Линейная скорость, м/с. После каждого шага физики здесь актуальное значение.
        pub velocity: Vec2 = Vec2::ZERO,
        pub gravity_scale: f32 = 1.0,
        pub linear_damping: f32 = 0.0,
        /// Запретить вращение (персонажи платформеров).
        pub lock_rotation: bool = false,
        /// Сглаживать позу между шагами физики (Unity: `interpolation = Interpolate`).
        /// Выключайте только для тел, которые не видны на экране.
        pub interpolate: bool = true,
    }
}
#[cfg(feature = "2d")]
impl Component for Rigidbody2D {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

#[cfg(feature = "2d")]
impl Rigidbody2D {
    pub fn dynamic() -> Self {
        Self::default()
    }

    pub fn kinematic() -> Self {
        Self {
            body_type: BodyType::Kinematic,
            ..Self::default()
        }
    }

    pub fn locked_rotation(mut self) -> Self {
        self.lock_rotation = true;
        self
    }
}

#[cfg(feature = "2d")]
component! {
    /// Коллайдер. Слой столкновений берётся из `GameObject::layer`.
    pub struct Collider2D {
        pub shape: Shape2D = Shape2D::Box,
        /// Полный размер для `Box`; для `Capsule` — (диаметр, полная высота).
        pub size: Vec2 = Vec2::ONE,
        /// Радиус для `Circle`.
        pub radius: f32 = 0.5,
        pub offset: Vec2 = Vec2::ZERO,
        /// Триггер не толкает тела, а только сообщает о входе (`on_trigger_enter`).
        pub is_trigger: bool = false,
        pub friction: f32 = 0.4,
        pub restitution: f32 = 0.0,
        /// Маска слоёв, с которыми сталкивается коллайдер.
        pub collides_with: u32 = u32::MAX,
        /// Односторонняя платформа: снизу и сбоку пропускает, сверху держит
        /// (Unity: `PlatformEffector2D`). «Верх» — локальная ось Y объекта.
        pub one_way: bool = false,
    }
}
#[cfg(feature = "2d")]
impl Component for Collider2D {
    /// Только данные: хуков нет.
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

#[cfg(feature = "2d")]
impl Collider2D {
    pub fn rect(width: f32, height: f32) -> Self {
        Self {
            size: Vec2::new(width, height),
            ..Self::default()
        }
    }

    pub fn circle(radius: f32) -> Self {
        Self {
            shape: Shape2D::Circle,
            radius,
            ..Self::default()
        }
    }

    pub fn trigger(mut self) -> Self {
        self.is_trigger = true;
        self
    }

    pub fn with_friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }
}

#[cfg(feature = "2d")]
/// Результат [`Physics2D::raycast`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit2D {
    pub object: GameObjectId,
    pub point: Vec2,
    pub normal: Vec2,
    pub distance: f32,
}

#[cfg(feature = "2d")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    position: Vec2,
    angle: f32,
}

#[cfg(feature = "2d")]
impl Pose {
    fn differs(self, other: Pose) -> bool {
        (self.position - other.position).length_squared() > 1e-8
            || angle_delta(other.angle, self.angle).abs() > 1e-4
    }

    fn lerp(self, to: Pose, t: f32) -> Pose {
        Pose {
            position: self.position.lerp(to.position, t),
            angle: self.angle + angle_delta(self.angle, to.angle) * t,
        }
    }
}

#[cfg(feature = "2d")]
/// Кратчайший поворот от `from` к `to`, в (−π, π].
fn angle_delta(from: f32, to: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let delta = (to - from).rem_euclid(tau);
    if delta > std::f32::consts::PI { delta - tau } else { delta }
}

#[cfg(feature = "2d")]
struct Entry {
    body: rp::RigidBodyHandle,
    collider: rp::ColliderHandle,
    /// Слой объекта и маска коллайдера на момент создания: из них и матрицы слоёв
    /// собирается фильтр столкновений (пересобирается при смене настроек).
    layer: u32,
    collides_with: u32,
    dynamic: bool,
    interpolate: bool,
    /// Позы тела после предпоследнего и последнего шага симуляции.
    previous: Pose,
    current: Pose,
    /// Поза, которую физика записала в Transform последней. Если Transform
    /// отличается — объект переместил скрипт, и тело надо телепортировать.
    written: Pose,
}

#[cfg(feature = "2d")]
/// Перехватчик контактов: реализует односторонние платформы.
struct ContactHooks<'a> {
    one_way: &'a HashSet<rp::ColliderHandle>,
}

#[cfg(feature = "2d")]
impl rp::PhysicsHooks for ContactHooks<'_> {
    fn modify_solver_contacts(&self, context: &mut rp::ContactModificationContext) {
        // Разрешённая нормаль контакта в локальных координатах первого коллайдера.
        let allowed = if self.one_way.contains(&context.collider1) {
            rp::Vector::y()
        } else if self.one_way.contains(&context.collider2) {
            -rp::Vector::y()
        } else {
            return;
        };
        context.update_as_oneway_platform(&allowed, 0.1);
    }
}

#[cfg(feature = "2d")]
enum Contact {
    Collision(GameObjectId, GameObjectId),
    Trigger(GameObjectId, GameObjectId),
}

#[cfg(feature = "2d")]
/// Состояние 2D-физики. Лежит в `world.resources`; скрипты достают его для запросов:
/// `ctx.resource::<Physics2D>()?.raycast(..)`.
pub struct Physics2D {
    pub gravity: Vec2,
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
    one_way: HashSet<rp::ColliderHandle>,
    /// Матрица столкновений слоёв (Project Settings → Physics 2D).
    layer_matrix: LayerMatrix,
    /// Пороги засыпания новых тел (линейный — «нормализованный», как в Rapier).
    sleep_linear: f32,
    sleep_angular: f32,
}

#[cfg(feature = "2d")]
impl Default for Physics2D {
    fn default() -> Self {
        Self {
            gravity: Vec2::new(0.0, -9.81),
            pipeline: rp::PhysicsPipeline::new(),
            params: rp::IntegrationParameters::default(),
            islands: rp::IslandManager::new(),
            broad_phase: rp::DefaultBroadPhase::new(),
            narrow_phase: rp::NarrowPhase::new(),
            bodies: rp::RigidBodySet::new(),
            colliders: rp::ColliderSet::new(),
            impulse_joints: rp::ImpulseJointSet::new(),
            multibody_joints: rp::MultibodyJointSet::new(),
            ccd: rp::CCDSolver::new(),
            query: rp::QueryPipeline::new(),
            entries: HashMap::new(),
            by_collider: HashMap::new(),
            one_way: HashSet::new(),
            layer_matrix: LayerMatrix::default(),
            sleep_linear: rp::RigidBodyActivation::default_normalized_linear_threshold(),
            sleep_angular: rp::RigidBodyActivation::default_angular_threshold(),
        }
    }
}

#[cfg(feature = "2d")]
fn world_pose(scene: &Scene, id: GameObjectId) -> (Vec2, f32, Vec2) {
    let (scale, rotation, position) = scene.world_matrix(id).to_scale_rotation_translation();
    let angle = rotation.to_euler(EulerRot::ZYX).0;
    (position.truncate(), angle, scale.truncate().abs())
}

#[cfg(feature = "2d")]
impl Physics2D {
    pub fn with_gravity(gravity: Vec2) -> Self {
        Self {
            gravity,
            ..Self::default()
        }
    }

    /// Сколько объектов сцены сейчас участвует в симуляции.
    pub fn body_count(&self) -> usize {
        self.entries.len()
    }

    /// Применить раздел Physics 2D настроек проекта: гравитация, итерации решателя,
    /// пороги засыпания, матрица слоёв. Фильтры уже созданных коллайдеров и пороги уже
    /// созданных тел пересобираются сразу, поэтому вызывать можно и на ходу (редактор).
    /// Вызывается один раз при применении настроек — гравитация, заданная кодом игры
    /// позже, не перезаписывается.
    pub fn apply_settings(&mut self, settings: &Physics2dSettings) {
        let [x, y] = settings.gravity;
        if x.is_finite() && y.is_finite() {
            self.gravity = Vec2::new(x, y);
        }
        self.params.num_solver_iterations =
            std::num::NonZeroUsize::new(settings.solver_iterations.clamp(1, 64) as usize).expect("at least 1");
        self.sleep_linear = finite_or(settings.sleep_linear_threshold, self.sleep_linear).max(0.0);
        self.sleep_angular = finite_or(settings.sleep_angular_threshold, self.sleep_angular).max(0.0);
        self.layer_matrix = settings.layer_matrix.clone();
        for entry in self.entries.values() {
            let groups = self.groups(entry.layer, entry.collides_with);
            if let Some(collider) = self.colliders.get_mut(entry.collider) {
                collider.set_collision_groups(groups);
            }
            if let Some(body) = self.bodies.get_mut(entry.body) {
                set_sleep_thresholds(body, self.sleep_linear, self.sleep_angular);
                body.wake_up(true);
            }
        }
    }

    /// Итерации решателя контактов (Project Settings → Physics 2D).
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
    fn groups(&self, layer: u32, collides_with: u32) -> rp::InteractionGroups {
        let layer = layer.min(31);
        rp::InteractionGroups::new(
            rp::Group::from_bits_truncate(1 << layer),
            rp::Group::from_bits_truncate(collides_with & self.layer_matrix.mask(layer)),
        )
    }

    /// Unity: `Physics2D.Raycast`. Триггеры игнорируются; `ignore` — обычно сам стреляющий.
    /// Видит мир на момент последнего шага физики.
    pub fn raycast(
        &self,
        origin: Vec2,
        direction: Vec2,
        max_distance: f32,
        ignore: Option<GameObjectId>,
    ) -> Option<RaycastHit2D> {
        let direction = direction.try_normalize()?;
        let ray = rp::Ray::new(
            rp::Point::new(origin.x, origin.y),
            rp::Vector::new(direction.x, direction.y),
        );
        let mut filter = rp::QueryFilter::default().exclude_sensors();
        if let Some(entry) = ignore.and_then(|id| self.entries.get(&id)) {
            filter = filter.exclude_rigid_body(entry.body);
        }
        let (collider, hit) = self.query.cast_ray_and_get_normal(
            &self.bodies,
            &self.colliders,
            &ray,
            max_distance,
            true,
            filter,
        )?;
        Some(RaycastHit2D {
            object: *self.by_collider.get(&collider)?,
            point: origin + direction * hit.time_of_impact,
            normal: Vec2::new(hit.normal.x, hit.normal.y),
            distance: hit.time_of_impact,
        })
    }

    fn remove_entry(&mut self, id: GameObjectId) {
        if let Some(entry) = self.entries.remove(&id) {
            self.by_collider.remove(&entry.collider);
            self.one_way.remove(&entry.collider);
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

    fn add_entry(&mut self, scene: &Scene, id: GameObjectId) {
        let Some(object) = scene.get(id) else { return };
        let Some(collider) = object.get::<Collider2D>() else {
            return;
        };
        let rigidbody = object.get::<Rigidbody2D>();
        let (position, angle, scale) = world_pose(scene, id);

        let body_type = rigidbody.map_or(BodyType::Static, |rb| rb.body_type);
        let mut body = match body_type {
            BodyType::Dynamic => rp::RigidBodyBuilder::dynamic(),
            BodyType::Kinematic => rp::RigidBodyBuilder::kinematic_position_based(),
            BodyType::Static => rp::RigidBodyBuilder::fixed(),
        }
        .translation(rp::Vector::new(position.x, position.y))
        .rotation(angle);
        if let Some(rb) = rigidbody {
            body = body
                .linvel(rp::Vector::new(rb.velocity.x, rb.velocity.y))
                .gravity_scale(rb.gravity_scale)
                .linear_damping(rb.linear_damping)
                .ccd_enabled(true);
            if rb.lock_rotation {
                body = body.lock_rotations();
            }
        }

        let builder = match collider.shape {
            Shape2D::Box => rp::ColliderBuilder::cuboid(
                (collider.size.x * scale.x * 0.5).max(1e-4),
                (collider.size.y * scale.y * 0.5).max(1e-4),
            ),
            Shape2D::Circle => {
                rp::ColliderBuilder::ball((collider.radius * scale.x.max(scale.y)).max(1e-4))
            }
            Shape2D::Capsule => {
                let radius = (collider.size.x * scale.x * 0.5).max(1e-4);
                let half_height = (collider.size.y * scale.y * 0.5 - radius).max(0.0);
                rp::ColliderBuilder::capsule_y(half_height, radius)
            }
        }
        .translation(rp::Vector::new(
            collider.offset.x * scale.x,
            collider.offset.y * scale.y,
        ))
        .sensor(collider.is_trigger)
        .friction(collider.friction)
        .restitution(collider.restitution)
        .friction_combine_rule(rp::CoefficientCombineRule::Min)
        .restitution_combine_rule(rp::CoefficientCombineRule::Max)
        .active_hooks(if collider.one_way {
            rp::ActiveHooks::MODIFY_SOLVER_CONTACTS
        } else {
            rp::ActiveHooks::empty()
        })
        .active_events(rp::ActiveEvents::COLLISION_EVENTS)
        .collision_groups(self.groups(object.layer, collider.collides_with));

        let body_handle = self.bodies.insert(body);
        if let Some(body) = self.bodies.get_mut(body_handle) {
            set_sleep_thresholds(body, self.sleep_linear, self.sleep_angular);
        }
        let collider_handle =
            self.colliders
                .insert_with_parent(builder, body_handle, &mut self.bodies);
        self.by_collider.insert(collider_handle, id);
        if collider.one_way {
            self.one_way.insert(collider_handle);
        }
        let pose = Pose { position, angle };
        self.entries.insert(
            id,
            Entry {
                body: body_handle,
                collider: collider_handle,
                layer: object.layer,
                collides_with: collider.collides_with,
                dynamic: body_type == BodyType::Dynamic,
                interpolate: rigidbody.is_none_or(|rb| rb.interpolate),
                previous: pose,
                current: pose,
                written: pose,
            },
        );
    }

    /// Скрипт или редактор подвинул `Transform` в обход физики — тело переносится туда же.
    fn absorb_moves(&mut self, scene: &Scene) {
        for (id, entry) in &mut self.entries {
            if !scene.contains(*id) {
                continue;
            }
            let (position, angle, _) = world_pose(scene, *id);
            let now = Pose { position, angle };
            if !now.differs(entry.written) {
                continue;
            }
            let Some(body) = self.bodies.get_mut(entry.body) else {
                continue;
            };
            if body.is_kinematic() {
                body.set_next_kinematic_translation(rp::Vector::new(position.x, position.y));
                body.set_next_kinematic_rotation(rp::Rotation::new(angle));
            } else {
                body.set_translation(rp::Vector::new(position.x, position.y), true);
                body.set_rotation(rp::Rotation::new(angle), true);
            }
            // Телепорт не сглаживается: иначе объект «проехал» бы через всю сцену.
            entry.previous = now;
            entry.current = now;
            entry.written = now;
        }
    }

    /// Записать позы динамических тел в `Transform`. С `alpha` — сглаженные между
    /// двумя последними шагами (для отрисовки), без него — точные (для скриптов).
    fn write_poses(&mut self, scene: &mut Scene, alpha: Option<f32>) {
        for (id, entry) in &mut self.entries {
            if !entry.dynamic {
                continue;
            }
            let pose = match alpha {
                Some(alpha) if entry.interpolate => entry.previous.lerp(entry.current, alpha),
                _ => entry.current,
            };
            if !pose.differs(entry.written) {
                continue;
            }
            // Поза тела — мировая; если у объекта есть родитель, переводим в локальную.
            let parent_matrix = scene
                .get(*id)
                .and_then(|o| o.parent())
                .map(|parent| scene.world_matrix(parent));
            let Some(object) = scene.get_mut(*id) else {
                continue;
            };
            let z = object.transform.position.z;
            match parent_matrix {
                None => {
                    object.transform.position = pose.position.extend(z);
                    object.transform.rotation = Quat::from_rotation_z(pose.angle);
                }
                Some(parent) => {
                    let local = parent.inverse().transform_point3(pose.position.extend(z));
                    let (_, parent_rotation, _) = parent.to_scale_rotation_translation();
                    object.transform.position = local;
                    object.transform.rotation =
                        parent_rotation.inverse() * Quat::from_rotation_z(pose.angle);
                }
            }
            // Запоминаем то, что реально читается обратно (после округлений матриц).
            let (position, angle, _) = world_pose(scene, *id);
            entry.written = Pose { position, angle };
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

    /// Сцена → Rapier.
    fn sync_in(&mut self, scene: &Scene) {
        let stale: Vec<GameObjectId> = self
            .entries
            .keys()
            .copied()
            .filter(|id| !scene.is_active(*id) || !scene.get(*id).is_some_and(|o| o.has::<Collider2D>()))
            .collect();
        for id in stale {
            self.remove_entry(id);
        }

        // Только объекты с коллайдером — по индексу сцены, в порядке иерархии (тела
        // создаются в том же порядке, что и раньше). Остальные объекты сцены ничего не стоят.
        scene.visit_active_with::<Collider2D>(|id, _| {
            let Some(object) = scene.get(id) else {
                return;
            };
            let Some(entry) = self.entries.get_mut(&id) else {
                self.add_entry(scene, id);
                return;
            };
            let Some(body) = self.bodies.get_mut(entry.body) else {
                return;
            };

            // Скрипт поменял скорость → передаём в симуляцию.
            if entry.dynamic {
                if let Some(rb) = object.get::<Rigidbody2D>() {
                    let current = *body.linvel();
                    if (current.x - rb.velocity.x).abs() > 1e-5
                        || (current.y - rb.velocity.y).abs() > 1e-5
                    {
                        body.set_linvel(rp::Vector::new(rb.velocity.x, rb.velocity.y), true);
                    }
                }
            }
        });
    }

    /// Rapier → сцена.
    fn sync_out(&mut self, scene: &mut Scene) {
        for (id, entry) in &mut self.entries {
            if !entry.dynamic {
                continue;
            }
            let Some(body) = self.bodies.get(entry.body) else {
                continue;
            };
            entry.previous = entry.current;
            entry.current = Pose {
                position: Vec2::new(body.translation().x, body.translation().y),
                angle: body.rotation().angle(),
            };
            let velocity = Vec2::new(body.linvel().x, body.linvel().y);
            if let Some(rb) = scene.get_component_mut::<Rigidbody2D>(*id) {
                rb.velocity = velocity;
            }
        }
        self.write_poses(scene, None);
    }

    /// Один фиксированный шаг. Возвращает начавшиеся контакты.
    fn step(&mut self, world: &mut World, dt: f32) -> Vec<Contact> {
        self.absorb_moves(&world.scene);
        self.sync_in(&world.scene);

        let (collision_tx, collision_rx) = rapier2d::crossbeam::channel::unbounded();
        let (force_tx, _force_rx) = rapier2d::crossbeam::channel::unbounded();
        let events = rp::ChannelEventCollector::new(collision_tx, force_tx);

        self.params.dt = dt;
        self.pipeline.step(
            &rp::Vector::new(self.gravity.x, self.gravity.y),
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
            &ContactHooks {
                one_way: &self.one_way,
            },
            &events,
        );

        self.sync_out(&mut world.scene);

        let mut contacts = Vec::new();
        while let Ok(event) = collision_rx.try_recv() {
            if let rp::CollisionEvent::Started(a, b, flags) = event {
                let (Some(&a), Some(&b)) = (self.by_collider.get(&a), self.by_collider.get(&b))
                else {
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

#[cfg(feature = "2d")]
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(feature = "2d")]
fn set_sleep_thresholds(body: &mut rp::RigidBody, linear: f32, angular: f32) {
    let activation = body.activation_mut();
    activation.normalized_linear_threshold = linear;
    activation.angular_threshold = angular;
}

#[cfg(feature = "2d")]
/// Применить раздел Physics 2D из ресурса [`ProjectSettings`](engine_core::settings::ProjectSettings)
/// к ресурсу [`Physics2D`]. Регистрируется хуком настроек (`World::add_settings_hook`)
/// в `engine::app()`; редактор, поменяв настройки, вызывает
/// `world.apply_project_settings(..)` — и хук применяет их к живому миру.
pub fn apply_project_settings_2d(world: &mut World) {
    let Some(settings) = world.resources.get::<engine_core::settings::ProjectSettings>() else {
        return;
    };
    let section = settings.physics2d.clone();
    if !world.resources.contains::<Physics2D>() {
        world.resources.insert(Physics2D::default());
    }
    if let Some(physics) = world.resources.get_mut::<Physics2D>() {
        physics.apply_settings(&section);
    }
}

/// Оба раздела физики (2D и 3D) — см. [`apply_project_settings_2d`] и
/// [`apply_project_settings_3d`].
pub fn apply_project_settings(world: &mut World) {
    #[cfg(feature = "2d")]
    apply_project_settings_2d(world);
    #[cfg(feature = "3d")]
    apply_project_settings_3d(world);
    #[cfg(not(any(feature = "2d", feature = "3d")))]
    let _ = world;
}

#[cfg(feature = "2d")]
/// Подсистема, шагающая [`Physics2D`] на каждом фиксированном шаге.
#[derive(Default)]
pub struct Physics2DSystem;

#[cfg(feature = "2d")]
impl Subsystem for Physics2DSystem {
    fn name(&self) -> &'static str {
        "physics2d"
    }

    fn init(&mut self, world: &mut World) {
        if !world.resources.contains::<Physics2D>() {
            world.resources.insert(Physics2D::default());
        }
    }

    fn before_fixed_update(&mut self, world: &mut World) {
        let World { resources, scene, .. } = world;
        if let Some(physics) = resources.get_mut::<Physics2D>() {
            physics.begin_fixed_step(scene);
        }
    }

    fn interpolate(&mut self, world: &mut World, alpha: f32) {
        let World { resources, scene, .. } = world;
        if let Some(physics) = resources.get_mut::<Physics2D>() {
            physics.interpolate(scene, alpha);
        }
    }

    fn fixed_update(&mut self, world: &mut World, dt: f32) {
        // Ресурс вынимается на время шага, чтобы одновременно иметь `&mut World`.
        let Some(mut physics) = world.resources.remove::<Physics2D>() else {
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

/// Зарегистрировать компоненты физики в реестре типов (для загрузки сцен).
pub fn register_components(registry: &mut TypeRegistry) {
    #[cfg(feature = "2d")]
    {
        registry.register::<Rigidbody2D>().register::<Collider2D>();
        // Категории меню Add Component в редакторе.
        registry.categorize(&["Rigidbody2D", "Collider2D"], "Physics 2D");
    }
    #[cfg(feature = "3d")]
    {
        registry
            .register::<Rigidbody>()
            .register::<Collider>()
            .register::<CharacterController>();
        registry.categorize(&["Rigidbody", "Collider", "CharacterController"], "Physics 3D");
    }
    #[cfg(not(any(feature = "2d", feature = "3d")))]
    let _ = registry;
}
