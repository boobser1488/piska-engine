//! Короткие формы физики для скриптов: `ctx.velocity()`, `ctx.set_velocity(..)`,
//! `ctx.add_force(..)`, `ctx.raycast(..)`, `ctx.grounded()`. Обёртки над компонентами
//! тел и ресурсами [`Physics2D`](crate::Physics2D) / [`Physics3D`](crate::Physics3D); длинные
//! формы (`ctx.get_mut::<Rigidbody2D>()`, `ctx.resource::<Physics3D>()`) остаются.
//!
//! Тело ищется на своём объекте в таком порядке: `Rigidbody2D`, `Rigidbody`,
//! `CharacterController`. Нет ни одного — обучающая ошибка (один раз на место вызова).

use std::panic::Location;

use engine_core::prelude::*;
use engine_core::script::{teach, AsVec3};

#[cfg(feature = "3d")]
use crate::{CharacterController, Physics3D, RaycastHit3D, Rigidbody};
#[cfg(feature = "2d")]
use crate::{Collider2D, Physics2D, RaycastHit2D, Rigidbody2D, Shape2D};

/// Физика из скрипта: `use engine::prelude::*;` уже подключает этот трейт.
pub trait PhysicsCtx {
    /// Скорость тела этого объекта (м/с; у 2D-тела `z = 0`).
    fn velocity(&mut self) -> Vec3;
    /// Скорость 2D-тела (x, y).
    fn velocity2(&mut self) -> Vec2 {
        self.velocity().truncate()
    }
    /// Задать скорость: `ctx.set_velocity(Vec2::new(run * 5.0, vy))` для 2D,
    /// `Vec3` — для 3D-тела и `CharacterController`.
    fn set_velocity(&mut self, velocity: impl AsVec3);
    /// Толкнуть тело силой на один шаг физики (вызывайте в `fixed_update`, пока сила
    /// действует). 3D-тело — сила Rapier; 2D-телу скорость меняется на `force · dt`
    /// (масса 1).
    fn add_force(&mut self, force: impl AsVec3);
    /// Стоит ли объект на земле: `CharacterController::grounded`, а у 2D-тела —
    /// короткий луч вниз из-под коллайдера.
    fn grounded(&mut self) -> bool;
    /// Луч в 3D-мире (Unity: `Physics.Raycast`); свой объект не задевает.
    #[cfg(feature = "3d")]
    fn raycast(&mut self, from: Vec3, direction: Vec3, max_distance: f32) -> Option<RaycastHit3D>;
    /// Луч в 2D-мире (Unity: `Physics2D.Raycast`); свой объект не задевает.
    #[cfg(feature = "2d")]
    fn raycast2d(&mut self, from: Vec2, direction: Vec2, max_distance: f32) -> Option<RaycastHit2D>;
    /// Объекты, чьи 3D-коллайдеры задевают сферу (Unity: `Physics.OverlapSphere`).
    #[cfg(feature = "3d")]
    fn overlap_sphere(&mut self, center: Vec3, radius: f32) -> Vec<GameObjectId>;
}

fn teach_no_body(ctx: &mut Ctx, site: &Location<'_>, call: &str) {
    let name = ctx.name().to_string();
    teach(ctx.world, site, "no-body", || {
        format!(
            "`{name}` has no physics body for ctx.{call}(). Add Rigidbody2D (2D), Rigidbody (3D) \
             or CharacterController in the Inspector; until then the call does nothing"
        )
    });
}

impl PhysicsCtx for Ctx<'_> {
    #[track_caller]
    fn velocity(&mut self) -> Vec3 {
        #[cfg(feature = "2d")]
        if let Some(body) = self.get::<Rigidbody2D>() {
            return body.velocity.extend(0.0);
        }
        #[cfg(feature = "3d")]
        {
            if let Some(body) = self.get::<Rigidbody>() {
                return body.velocity;
            }
            if let Some(character) = self.get::<CharacterController>() {
                return character.velocity;
            }
        }
        teach_no_body(self, Location::caller(), "velocity");
        Vec3::ZERO
    }

    #[track_caller]
    fn set_velocity(&mut self, velocity: impl AsVec3) {
        #[cfg(feature = "2d")]
        if let Some(body) = self.get_mut::<Rigidbody2D>() {
            body.velocity = velocity.with_z(0.0).truncate();
            return;
        }
        #[cfg(feature = "3d")]
        {
            if let Some(body) = self.get_mut::<Rigidbody>() {
                body.velocity = velocity.with_z(body.velocity.z);
                return;
            }
            if let Some(character) = self.get_mut::<CharacterController>() {
                character.velocity = velocity.with_z(character.velocity.z);
                return;
            }
        }
        let _ = velocity;
        teach_no_body(self, Location::caller(), "set_velocity");
    }

    #[track_caller]
    fn add_force(&mut self, force: impl AsVec3) {
        #[cfg(feature = "2d")]
        {
            let dt = self.dt();
            if let Some(body) = self.get_mut::<Rigidbody2D>() {
                body.velocity += force.with_z(0.0).truncate() * dt;
                return;
            }
        }
        #[cfg(feature = "3d")]
        if self.get::<Rigidbody>().is_some() {
            let id = self.id;
            if let Some(physics) = self.resource_mut::<Physics3D>() {
                physics.add_force(id, force.with_z(0.0));
            }
            return;
        }
        let _ = force;
        teach_no_body(self, Location::caller(), "add_force");
    }

    #[track_caller]
    fn grounded(&mut self) -> bool {
        #[cfg(feature = "3d")]
        if let Some(character) = self.get::<CharacterController>() {
            return character.grounded;
        }
        #[cfg(feature = "2d")]
        if self.get::<Rigidbody2D>().is_some() {
            // Низ коллайдера: от центра объекта (с учётом смещения) на полвысоты вниз.
            let (offset, half) = match self.get::<Collider2D>() {
                Some(c) => {
                    let half = match c.shape {
                        Shape2D::Circle => c.radius,
                        _ => c.size.y * 0.5,
                    };
                    (c.offset, half * self.scale().y.abs())
                }
                None => (Vec2::ZERO, 0.5),
            };
            let from = self.pos2() + offset;
            let id = self.id;
            return self
                .resource::<Physics2D>()
                .and_then(|p| p.raycast(from, Vec2::NEG_Y, half + 0.08, Some(id)))
                .is_some();
        }
        teach_no_body(self, Location::caller(), "grounded");
        false
    }

    #[cfg(feature = "3d")]
    fn raycast(&mut self, from: Vec3, direction: Vec3, max_distance: f32) -> Option<RaycastHit3D> {
        let id = self.id;
        self.resource::<Physics3D>()?.raycast(from, direction, max_distance, Some(id))
    }

    #[cfg(feature = "2d")]
    fn raycast2d(&mut self, from: Vec2, direction: Vec2, max_distance: f32) -> Option<RaycastHit2D> {
        let id = self.id;
        self.resource::<Physics2D>()?.raycast(from, direction, max_distance, Some(id))
    }

    #[cfg(feature = "3d")]
    fn overlap_sphere(&mut self, center: Vec3, radius: f32) -> Vec<GameObjectId> {
        self.resource::<Physics3D>()
            .map(|p| p.overlap_sphere(center, radius))
            .unwrap_or_default()
    }
}
