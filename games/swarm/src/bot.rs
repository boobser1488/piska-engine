//! Бот: играет в симуляцию напрямую (тесты, подбор баланса, автопилот для скриншотов).
//!
//! Целится в ближайшего врага с упреждением, держится на кольце вокруг реактора,
//! отталкивается от врагов и пуль, подбирает обломки, делает рывок при опасности.

use engine::prelude::Vec2;

use crate::data::*;
use crate::sim::{Phase, PlayerInput, Sim};

/// Управление бота на этот кадр.
pub fn bot_input(sim: &Sim) -> PlayerInput {
    let p = &sim.player;
    if !p.alive {
        return PlayerInput::default();
    }
    // Цель: ближайший враг (мины и далёкие — реже), босс в приоритете.
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    let mut danger = Vec2::ZERO;
    let mut close = 0.0f32;
    for e in sim.enemies.iter().filter(|e| e.alive) {
        let d = e.pos.distance(p.pos);
        let mut score = d;
        if e.kind == Kind::Mine {
            score += 6.0;
        }
        if e.kind.is_boss() {
            score -= 4.0;
        }
        if best.is_none_or(|(s, _, _)| score < s) {
            best = Some((score, e.pos, e.vel));
        }
        let reach = e.r + 2.8;
        if d < reach && e.kind != Kind::Mine || (e.kind == Kind::Mine && d < 2.2) {
            let w = ((reach - d) / reach).max(0.0) * if e.kind.is_boss() { 1.6 } else { 1.0 };
            danger += (p.pos - e.pos).normalize_or_zero() * w;
            close += w;
        }
    }
    for b in sim.ebullets.iter().filter(|b| b.alive) {
        let d = b.pos.distance(p.pos);
        if d < 3.0 {
            // Уклонение вбок от траектории пули.
            let dir = b.vel.normalize_or_zero();
            let rel = p.pos - b.pos;
            let side = rel - dir * rel.dot(dir);
            danger += side.normalize_or_zero() * ((3.0 - d) / 3.0) * 0.9;
            close += 0.2;
        }
    }
    let mut mv = danger * 1.6;
    // К кольцу вокруг реактора.
    let r = p.pos.length();
    let ring = 4.6;
    mv += p.pos.normalize_or_zero() * (ring - r) * 0.12;
    if r > arena_r() - 1.6 {
        mv -= p.pos.normalize_or_zero() * 1.5;
    }
    // К ближайшим обломкам.
    if let Some(pk) = sim
        .pickups
        .iter()
        .filter(|k| k.alive)
        .min_by(|a, b| a.pos.distance_squared(p.pos).total_cmp(&b.pos.distance_squared(p.pos)))
    {
        let d = pk.pos.distance(p.pos);
        if d < 7.0 && close < 0.6 {
            mv += (pk.pos - p.pos).normalize_or_zero() * 0.7;
        }
    }
    // Если врагов нет, ждём в центре кольца.
    let mv = mv.clamp_length_max(1.0);
    let mut input = PlayerInput { mv, aim: p.aim, fire: false, dash: false };
    if let Some((_, pos, vel)) = best {
        let dist = pos.distance(p.pos);
        let t = dist / 22.0;
        let lead = pos + vel * t.min(0.5);
        let aim = (lead - p.pos).normalize_or_zero();
        if aim != Vec2::ZERO {
            input.aim = aim;
        }
        input.fire = dist < 15.0;
    }
    if close > 2.2 && p.dash_cd <= 0.0 && sim.phase != Phase::Intermission {
        input.dash = true;
        if mv.length_squared() < 0.04 {
            input.mv = (-danger).normalize_or_zero();
        }
    }
    input
}

/// Какое улучшение берёт бот: приоритеты по порядку, иначе первое из предложенного.
pub fn bot_pick(offer: &[Up], levels: &[u8; UP_COUNT]) -> Option<Up> {
    const PRIORITY: [Up; 18] = [
        Up::Damage,
        Up::Rate,
        Up::Multishot,
        Up::Pierce,
        Up::Explosive,
        Up::Hull,
        Up::Shield,
        Up::Repair,
        Up::Missile,
        Up::Crit,
        Up::Armor,
        Up::Slow,
        Up::Magnet,
        Up::Thrusters,
        Up::Ricochet,
        Up::DashCd,
        Up::DashDmg,
        Up::Siphon,
    ];
    let _ = levels;
    PRIORITY.iter().copied().find(|u| offer.contains(u)).or_else(|| offer.first().copied())
}

/// Один шаг бота: управление + автоматический выбор улучшений.
pub fn bot_step(sim: &mut Sim, dt: f32) {
    while sim.pending_upgrades > 0 && !sim.is_over() {
        let offer = sim.roll_offer();
        match bot_pick(&offer, &sim.levels) {
            Some(up) => {
                sim.choose_upgrade(up);
            }
            None => sim.skip_upgrade(),
        }
    }
    let input = bot_input(sim);
    sim.step(&input, dt);
}
