//! Общие мелочи: текстуры, поиск игрока, эффекты (частицы, тряска), ввод.

use crate::data::Progress;
use engine::prelude::*;
use std::collections::HashMap;

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

/// Все картинки игры по имени файла без расширения (`"player"` → `sprites/player.png`).
pub const TEXTURES: &[&str] = &[
    "player", "crawler", "bat", "spitter", "brute", "boss", "firefly", "checkpoint", "gate", "exit", "spring",
    "platform", "crumble", "projectile", "rock", "glow", "tiles", "bg_far", "bg_mid", "bg_near", "ui_heart",
    "ui_heart_empty", "ui_star", "ui_star_empty", "ui_lantern_dim", "ui_lantern_lit", "ui_firefly",
    "title_lantern", "touch_left", "touch_right", "touch_jump", "touch_dash", "ui_dark",
];

/// Дескрипторы текстур: заполняются при старте игры из базы ассетов.
#[derive(Default, Clone)]
pub struct Gfx {
    map: HashMap<&'static str, AssetHandle<Texture>>,
}

impl Gfx {
    pub fn load(db: &AssetDatabase) -> Result<Gfx, engine::assets::AssetError> {
        let mut map = HashMap::new();
        for name in TEXTURES {
            map.insert(*name, db.handle::<Texture>(&format!("sprites/{name}.png"))?);
        }
        Ok(Gfx { map })
    }

    /// Текстура по имени; неизвестное имя — программистская ошибка, а не повод падать.
    pub fn tex(&self, name: &str) -> AssetHandle<Texture> {
        self.map.get(name).cloned().unwrap_or_else(|| {
            log::error!("unknown texture `{name}`: add it to common::TEXTURES");
            AssetHandle::none()
        })
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
    let p = ctx.get_on::<crate::player::Player>(id)?;
    let feet = ctx.scene().world_position(id).truncate();
    Some(PlayerView {
        id,
        feet,
        center: feet + Vec2::new(0.0, crate::player::HALF.y),
        half: crate::player::HALF,
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

/// Разовый эффект частиц в точке мира.
pub fn spawn_fx(ctx: &mut Ctx, at: Vec2, fx: ParticleSystem) {
    ctx.world.scene.build("Fx").at(at.x, at.y).with(fx).id();
}

pub fn dust(ctx: &mut Ctx, at: Vec2, scale: f32) {
    let mut p = ParticleSystem::dust().scaled(scale);
    p.sorting_layer = layer::FX;
    spawn_fx(ctx, at, p);
}

pub fn sparkle(ctx: &mut Ctx, at: Vec2, color: Color) {
    let mut p = ParticleSystem::pickup_sparkle();
    p.start_color = color;
    p.start_color_max = color.scaled(0.7);
    p.sorting_layer = layer::FX;
    spawn_fx(ctx, at, p);
}

/// Взрыв-«искры» гибели врага.
pub fn burst(ctx: &mut Ctx, at: Vec2, color: Color, scale: f32) {
    let mut p = ParticleSystem::explosion().scaled(scale);
    p.start_color = color;
    p.start_color_max = color.scaled(0.6);
    p.sorting_layer = layer::FX;
    spawn_fx(ctx, at, p);
}

/// Столкнулись ли два объекта (по центрам и половинкам).
pub fn touching(a: Vec2, ha: Vec2, b: Vec2, hb: Vec2) -> bool {
    crate::phys::overlap(a, ha, b, hb)
}
