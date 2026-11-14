//! Общие мелочи: текстуры, поиск игрока, эффекты (частицы, тряска), ввод.

use crate::data::Progress;
use engine::prelude::*;

/// Слои сортировки: фон → декор → тайлы → предметы → враги → игрок → эффекты.
pub mod layer {
    pub const BG_FAR: i32 = -10;
    pub const BG_MID: i32 = -9;
    pub const BG_NEAR: i32 = -8;
    pub const DECO: i32 = -2;
    pub const TILES: i32 = 0;
    pub const ITEMS: i32 = 1;
    pub const ENEMIES: i32 = 2;
    pub const PLAYER: i32 = 3;
    pub const FX: i32 = 5;
}

/// Текстура из `assets/sprites/<имя>.png` по имени файла без расширения.
pub fn tex(ctx: &Ctx, name: &str) -> AssetHandle<Texture> {
    let path = format!("sprites/{name}.png");
    match ctx.resource::<AssetDatabase>().and_then(|db| db.handle::<Texture>(&path).ok()) {
        Some(handle) => handle,
        None => {
            log::error!("texture `{path}` is not in the assets");
            AssetHandle::none()
        }
    }
}

/// Игрок, как его видят остальные (враги, предметы).
#[derive(Clone, Copy, Debug)]
pub struct PlayerView {
    pub id: GameObjectId,
    /// Ступни.
    pub feet: Vec2,
    /// Центр коллайдера.
    pub center: Vec2,
    pub half: Vec2,
    pub vel: Vec2,
    pub dashing: bool,
    pub invulnerable: bool,
    pub alive: bool,
    /// Номер объекта платформы под ногами (биты) или −1.
    pub platform: i64,
}

pub fn player_view(ctx: &Ctx) -> Option<PlayerView> {
    let id = ctx.find_by_tag("Player")?;
    let p = ctx.get_on::<crate::scripts::player::Player>(id)?;
    let feet = ctx.scene().world_position(id).truncate();
    Some(PlayerView {
        id,
        feet,
        center: feet + Vec2::new(0.0, crate::scripts::player::HALF.y),
        half: crate::scripts::player::HALF,
        vel: p.vel,
        dashing: p.dash_time > 0.0,
        invulnerable: p.invuln > 0.0 || p.dash_time > 0.0,
        alive: !p.dead && p.respawn_time <= 0.0,
        platform: p.platform,
    })
}

/// Сообщение менеджеру уровня (объект `Level`).
pub fn to_level(ctx: &mut Ctx, name: &str, value: Value) {
    if let Some(level) = ctx.find_by_name("Level") {
        ctx.send_message(level, name, value);
    }
}

/// Тряска камеры (если не выключена в настройках).
pub fn shake(ctx: &mut Ctx, amount: f32) {
    if !ctx.resource::<Progress>().is_none_or(|p| p.shake) {
        return;
    }
    if let Some(cam) = ctx.find_by_tag("MainCamera") {
        if let Some(c) = ctx.scene_mut().get_component_mut::<CameraController2D>(cam) {
            c.add_shake(amount);
        }
    }
}

/// Разовый эффект из префаба `prefabs/<имя>.prefab` в точке мира: частицы удаляются сами.
/// `tune` подстраивает систему частиц экземпляра (цвет, масштаб).
fn fx(ctx: &mut Ctx, prefab: &str, at: Vec2, tune: impl FnOnce(&mut ParticleSystem)) {
    match ctx.instantiate_at(&format!("prefabs/{prefab}.prefab"), at.extend(0.0)) {
        Ok(id) => {
            if let Some(ps) = ctx.scene_mut().get_component_mut::<ParticleSystem>(id) {
                tune(ps);
            }
        }
        Err(err) => log::error!("effect `{prefab}`: {err}"),
    }
}

pub fn dust(ctx: &mut Ctx, at: Vec2, scale: f32) {
    fx(ctx, "fx_dust", at, |ps| *ps = ps.clone().scaled(scale));
}

pub fn sparkle(ctx: &mut Ctx, at: Vec2, color: Color) {
    fx(ctx, "fx_sparkle", at, |ps| {
        ps.start_color = color;
        ps.start_color_max = color.scaled(0.7);
    });
}

/// Взрыв-«искры» гибели врага.
pub fn burst(ctx: &mut Ctx, at: Vec2, color: Color, scale: f32) {
    fx(ctx, "fx_burst", at, |ps| {
        let mut scaled = ps.clone().scaled(scale);
        scaled.start_color = color;
        scaled.start_color_max = color.scaled(0.6);
        *ps = scaled;
    });
}

/// Столкнулись ли два объекта (по центрам и половинкам).
pub fn touching(a: Vec2, ha: Vec2, b: Vec2, hb: Vec2) -> bool {
    crate::phys::overlap(a, ha, b, hb)
}

/// Состояние менеджера уровня, нужное предметам: (светлячки, выход открыт, игра идёт).
pub fn level_state(ctx: &Ctx) -> Option<(i32, bool, bool)> {
    let id = ctx.find_by_name("Level")?;
    let l = ctx.get_on::<crate::scripts::level_manager::LevelManager>(id)?;
    Some((l.fireflies, l.exit_ready(), l.is_playing()))
}

/// Выпустить плевок из префаба `prefabs/projectile.prefab`.
pub fn fire_projectile(ctx: &mut Ctx, at: Vec2, vel: Vec2, scale: f32) {
    match ctx.instantiate_at("prefabs/projectile.prefab", at.extend(0.0)) {
        Ok(id) => {
            if let Some(o) = ctx.scene_mut().get_mut(id) {
                o.transform.scale = Vec3::splat(scale);
            }
            if let Some(p) = ctx.scene_mut().get_component_mut::<crate::scripts::projectile::Projectile>(id) {
                p.vel = vel;
                p.radius *= scale;
            }
        }
        Err(err) => log::error!("projectile: {err}"),
    }
}
