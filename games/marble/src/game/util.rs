//! Мелочи, общие для скриптов механизмов.

use engine::prelude::*;

/// Послать сообщение менеджеру игры («Game»): `"checkpoint"`, `"gear"`, `"finish"`…
pub fn tell_game(ctx: &mut Ctx, name: &str, value: Value) {
    if let Some(game) = ctx.world.scene.find_by_name("Game") {
        ctx.send_message(game, name, value);
    }
}

/// Тряска камеры (0..1) — сообщение менеджеру.
pub fn shake(ctx: &mut Ctx, amount: f32) {
    tell_game(ctx, "shake", Value::Float(amount));
}

/// Шар игрока помечен тегом «Ball».
pub fn is_ball(scene: &Scene, id: GameObjectId) -> bool {
    scene.get(id).is_some_and(|o| o.tag == "Ball")
}

/// Есть ли у объекта тело (Rigidbody): ленты и вентиляторы действуют только на такие.
pub fn has_body(scene: &Scene, id: GameObjectId) -> bool {
    scene.get(id).is_some_and(|o| o.has::<Rigidbody>())
}

/// Положение точки `p` в осях объекта `id`: x — вправо, y — вверх, z — вперёд (−Z объекта).
pub fn to_local_axes(scene: &Scene, id: GameObjectId, p: Vec3) -> Vec3 {
    let rotation = scene.world_rotation(id);
    let rel = p - scene.world_position(id);
    Vec3::new(rel.dot(rotation * Vec3::X), rel.dot(rotation * Vec3::Y), rel.dot(rotation * Vec3::NEG_Z))
}

/// Первый шар среди тел, которые задевает сфера.
pub fn find_ball_near(ctx: &Ctx, center: Vec3, radius: f32) -> Option<GameObjectId> {
    let physics = ctx.resource::<Physics3D>()?;
    physics.overlap_sphere(center, radius).into_iter().find(|id| is_ball(ctx.scene(), *id))
}

/// Звук в точке мира (клипы регистрирует `game::sounds`).
pub fn play_at(ctx: &mut Ctx, clip: &str, at: Vec3, volume: f32) {
    if let Some(audio) = ctx.resource_mut::<Audio>() {
        audio.play_with(clip, PlayParams::new().at(at).volume(volume));
    }
}

/// Скорость тела из компонента `Rigidbody`.
pub fn velocity(scene: &Scene, id: GameObjectId) -> Vec3 {
    scene.get_component::<Rigidbody>(id).map_or(Vec3::ZERO, |body| body.velocity)
}

/// Задать свечение (`MeshRenderer.emissive`) объекта.
pub fn set_emissive(scene: &mut Scene, id: GameObjectId, color: Color) {
    if let Some(renderer) = scene.get_component_mut::<MeshRenderer>(id) {
        renderer.emissive = color;
    }
}

/// Разовые частицы из префаба `prefabs/fx/<name>.prefab` в точке мира (объект удаляет
/// себя сам, когда частицы кончились).
pub fn effect(world: &mut World, name: &str, at: Vec3) {
    let path = format!("prefabs/fx/{name}.prefab");
    if let Err(err) = world.instantiate_at(&path, at, Quat::IDENTITY, None) {
        log::warn!("effect `{name}`: {err}");
    }
}
