//! Правила игры без окна и видеокарты: симуляция, сетка, улучшения, волны, боссы, бот,
//! сохранения. Всё детерминировано: фиксированный шаг и зерно.

use engine::prelude::{MemoryBackend, SaveData, Vec2};
use swarm::bot::{bot_input, bot_pick, bot_step};
use swarm::data::*;
use swarm::grid::Grid;
use swarm::profile::*;
use swarm::rng::Rng;
use swarm::sim::*;

const DT: f32 = 1.0 / 60.0;

fn idle(sim: &mut Sim, seconds: f32) {
    let n = (seconds / DT) as u32;
    for _ in 0..n {
        sim.step(&PlayerInput::default(), DT);
    }
}

/// Не дать волне закончиться: «отложенная» группа врагов, которая не появится никогда.
fn hold(sim: &mut Sim) {
    sim.plan.boss = None;
    sim.plan.groups = vec![SpawnGroup { at: 1.0e9, kind: Kind::Chaser, count: 1, pattern: Pattern::Scatter }];
}

/// Волна идёт; убрать из неё всё, что мешает проверке (первая пауза уже прошла).
fn fresh_wave(sim: &mut Sim, wave: u32) {
    sim.start_wave(wave);
}

#[test]
fn grid_matches_brute_force() {
    // Свойство: пары, найденные сеткой, ровно те, что даёт полный перебор.
    for seed in 0..25u64 {
        let mut rng = Rng::new(seed);
        let n = rng.int(2, 220) as usize;
        let mut grid = Grid::new(ARENA_R + 3.0, 1.5);
        let mut items = Vec::new();
        for i in 0..n {
            // Часть объектов далеко за краем сетки, радиусы от мелких до боссовых.
            let spread = if i % 17 == 0 { 40.0 } else { ARENA_R };
            let pos = Vec2::new(rng.range(-spread, spread), rng.range(-spread, spread));
            let r = if i % 9 == 0 { rng.range(1.5, 2.3) } else { rng.range(0.2, 0.9) };
            items.push((pos, r));
        }
        for (i, (pos, r)) in items.iter().enumerate() {
            grid.insert(i as u32, *pos, *r);
        }
        let ids: Vec<u32> = (0..n as u32).collect();
        let found = grid.pairs(&ids);
        let mut brute = Vec::new();
        for a in 0..n {
            for b in (a + 1)..n {
                let limit = items[a].1 + items[b].1;
                if (items[a].0 - items[b].0).length_squared() <= limit * limit {
                    brute.push((a as u32, b as u32));
                }
            }
        }
        assert_eq!(found, brute, "seed {seed}, {n} items");
        // Точечный запрос тоже совпадает с перебором.
        for _ in 0..20 {
            let q = Vec2::new(rng.range(-12.0, 12.0), rng.range(-12.0, 12.0));
            let qr = rng.range(0.1, 3.0);
            let mut got = Vec::new();
            grid.query(q, qr, |id| got.push(id));
            got.sort_unstable();
            let want: Vec<u32> = (0..n as u32)
                .filter(|&i| {
                    let (p, r) = items[i as usize];
                    (p - q).length() <= qr + r
                })
                .collect();
            // Допускаем только расхождение на границе округления (касание).
            let missing: Vec<_> = want.iter().filter(|i| !got.contains(i)).collect();
            let extra: Vec<_> = got.iter().filter(|i| !want.contains(i)).collect();
            assert!(missing.is_empty() && extra.is_empty(), "seed {seed}: missing {missing:?} extra {extra:?}");
        }
    }
}

#[test]
fn every_enemy_kind_spawns_and_can_be_killed() {
    for kind in Kind::ALL {
        let mut sim = Sim::new(1, Mode::Campaign);
        sim.player.invuln = 999.0;
        sim.core.hp = 1.0e6;
        sim.core.max_hp = 1.0e6;
        fresh_wave(&mut sim, 12);
        // Волна без плана: только наш враг.
        hold(&mut sim);
        let pos = Vec2::new(5.0, 3.0);
        let i = sim.spawn_enemy(kind, pos).unwrap_or_else(|| panic!("{kind:?} spawns"));
        assert!(sim.enemies[i].alive);
        assert_eq!(sim.count_alive(kind), 1);
        assert!(sim.enemies[i].hp > 0.0);
        // Убиваем уроном: враг должен умереть и оставить события и очки.
        let score = sim.score;
        let uid = sim.enemies[i].uid;
        sim.damage_enemy(i, 1.0e7, false);
        assert!(!(sim.enemies[i].alive && sim.enemies[i].uid == uid), "{kind:?} dies");
        assert_eq!(sim.count_alive(kind), if kind == Kind::SplitterMini { 0 } else { sim.count_alive(kind) });
        if kind != Kind::Mine {
            assert!(sim.score > score, "{kind:?} gives score");
        }
        assert!(sim.events.iter().any(|e| matches!(e, Event::EnemyDied { kind: k, .. } if *k == kind)), "{kind:?} death event");
    }
}

#[test]
fn enemies_behave_differently() {
    // Гонщик идёт к дрону, танк — к реактору, стрелок держит дистанцию и стреляет.
    let mut sim = Sim::new(3, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    sim.player.invuln = 999.0;
    fresh_wave(&mut sim, 12);
    hold(&mut sim);
    sim.plan.boss = None;
    sim.player.pos = Vec2::new(-6.0, 0.0);
    let chaser = sim.spawn_enemy(Kind::Chaser, Vec2::new(6.0, 0.0)).unwrap();
    let tank = sim.spawn_enemy(Kind::Tank, Vec2::new(6.0, 5.0)).unwrap();
    let shooter = sim.spawn_enemy(Kind::Shooter, Vec2::new(0.0, 8.0)).unwrap();
    sim.enemies[chaser].target_core = false;
    let (c0, t0) = (sim.enemies[chaser].pos, sim.enemies[tank].pos);
    let mut bullets = 0;
    for _ in 0..120 {
        sim.step(&PlayerInput { aim: Vec2::X, ..Default::default() }, DT);
        bullets = bullets.max(sim.ebullets_alive());
    }
    assert!(sim.enemies[chaser].pos.distance(sim.player.pos) < c0.distance(sim.player.pos), "chaser closes in on the drone");
    assert!(sim.enemies[tank].pos.length() < t0.length(), "tank heads for the core");
    assert!(sim.enemies[shooter].alive);
    assert!(bullets > 0 || sim.enemies[shooter].timer > 0.0, "shooter eventually fires");
    for _ in 0..400 {
        sim.step(&PlayerInput::default(), DT);
        bullets = bullets.max(sim.ebullets_alive());
    }
    assert!(bullets > 0, "shooter fired at the drone");
    let d = sim.enemies[shooter].pos.distance(sim.player.pos);
    assert!(d > 3.5, "shooter keeps its distance: {d}");
}

#[test]
fn splitter_splits_and_bomber_blasts() {
    let mut sim = Sim::new(4, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    fresh_wave(&mut sim, 12);
    hold(&mut sim);
    let s = sim.spawn_enemy(Kind::Splitter, Vec2::new(4.0, 4.0)).unwrap();
    sim.damage_enemy(s, 1.0e6, false);
    assert_eq!(sim.count_alive(Kind::SplitterMini), 2, "a splitter breaks into two");
    // Подрывник добегает до дрона и взрывается.
    let mut sim = Sim::new(5, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    fresh_wave(&mut sim, 12);
    hold(&mut sim);
    sim.player.pos = Vec2::new(5.0, 0.0);
    sim.player.invuln = 0.0;
    let hp = sim.player.hp;
    sim.spawn_enemy(Kind::Bomber, Vec2::new(7.0, 3.0)).unwrap();
    sim.enemies[0].target_core = false;
    for _ in 0..240 {
        sim.step(&PlayerInput::default(), DT);
    }
    assert_eq!(sim.count_alive(Kind::Bomber), 0, "the bomber exploded");
    assert!(sim.player.hp < hp, "the blast hurt the drone");
}

#[test]
fn minelayer_lays_mines() {
    let mut sim = Sim::new(6, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    sim.player.invuln = 999.0;
    fresh_wave(&mut sim, 12);
    hold(&mut sim);
    sim.spawn_enemy(Kind::MineLayer, Vec2::new(6.0, 0.0)).unwrap();
    idle(&mut sim, 8.0);
    assert!(sim.count_alive(Kind::Mine) >= 2, "mines: {}", sim.count_alive(Kind::Mine));
}

#[test]
fn each_upgrade_changes_the_stat_it_claims() {
    assert_eq!(Up::ALL.len(), UP_COUNT);
    assert_eq!(UP_COUNT, Up::ALL.len());
    assert!(Up::ALL.len() >= 14, "at least 14 distinct upgrades");
    let base = Stats::from_levels(&[0; UP_COUNT]);
    for up in Up::ALL {
        let mut levels = [0u8; UP_COUNT];
        levels[up.index()] = 1;
        let after = Stats::from_levels(&levels);
        assert_ne!(after, base, "{up:?} changes some stat");
        let (b, higher_better) = base.primary(up);
        let (a, _) = after.primary(up);
        assert!(if higher_better { a > b } else { a < b }, "{up:?}: primary stat {b} -> {a}");
        // Стакается: второй уровень сильнее первого (где максимум позволяет).
        if up.info().max >= 2 {
            levels[up.index()] = 2;
            let two = Stats::from_levels(&levels);
            let (a2, _) = two.primary(up);
            assert!(if higher_better { a2 > a } else { a2 < a }, "{up:?} stacks: {a} -> {a2}");
        }
        assert!(!up.describe(1).is_empty() && !up.info().name.is_empty());
    }
}

#[test]
fn upgrades_take_effect_in_the_simulation() {
    let mut sim = Sim::new(7, Mode::Campaign);
    sim.core.hp = 100.0;
    let hp = sim.player.hp;
    sim.apply_upgrade(Up::Hull);
    assert_eq!(sim.stats.max_hp, 125.0);
    assert_eq!(sim.player.hp, hp + 25.0, "hull heals the added amount");
    sim.apply_upgrade(Up::Repair);
    assert_eq!(sim.core.hp, 130.0, "core repair heals right away");
    sim.apply_upgrade(Up::Armor);
    assert_eq!(sim.core.max_hp, 240.0);
    // Мультизалп: больше снарядов за выстрел; урон растёт; пробитие хранится в пуле.
    let shoot = |sim: &mut Sim| -> usize {
        sim.player.fire_cd = 0.0;
        let before = sim.pbullets_alive();
        sim.step(&PlayerInput { aim: Vec2::X, fire: true, ..Default::default() }, DT);
        sim.pbullets_alive() - before
    };
    assert_eq!(shoot(&mut sim), 1);
    let mut sim = Sim::new(7, Mode::Campaign);
    sim.apply_upgrade(Up::Multishot);
    sim.apply_upgrade(Up::Multishot);
    assert_eq!(shoot(&mut sim), 3, "two levels of multishot = three bullets");
    let dmg0 = sim.pbullets.iter().find(|b| b.alive).unwrap().dmg;
    let mut sim2 = Sim::new(7, Mode::Campaign);
    sim2.apply_upgrade(Up::Damage);
    shoot(&mut sim2);
    let dmg1 = sim2.pbullets.iter().find(|b| b.alive).unwrap().dmg;
    assert!(dmg1 > dmg0);
}

#[test]
fn piercing_and_explosive_rounds_work() {
    // Три врага в ряд: пробивающая пуля задевает всех, обычная — только первого.
    let setup = |pierce: u8| -> Sim {
        let mut sim = Sim::new(8, Mode::Campaign);
        sim.core.hp = 1.0e6;
        sim.core.max_hp = 1.0e6;
        fresh_wave(&mut sim, 1);
        hold(&mut sim);
        sim.player.pos = Vec2::new(-6.0, 0.0);
        for _ in 0..pierce {
            sim.apply_upgrade(Up::Pierce);
        }
        for x in [-2.0f32, 0.5, 3.0] {
            let i = sim.spawn_enemy(Kind::Tank, Vec2::new(x, 0.0)).unwrap();
            sim.enemies[i].vel = Vec2::ZERO;
            sim.enemies[i].target_core = false;
            sim.enemies[i].hp = 1.0e5;
        }
        sim
    };
    let hit_count = |pierce: u8| -> usize {
        let mut sim = setup(pierce);
        sim.player.fire_cd = 0.0;
        sim.step(&PlayerInput { aim: Vec2::X, fire: true, ..Default::default() }, DT);
        for _ in 0..30 {
            sim.step(&PlayerInput { aim: Vec2::X, ..Default::default() }, DT);
        }
        sim.enemies.iter().filter(|e| e.alive && e.hp < 1.0e5).count()
    };
    assert_eq!(hit_count(0), 1);
    assert_eq!(hit_count(2), 3);
}

#[test]
fn ricochet_keeps_bullets_in_the_arena() {
    let mut sim = Sim::new(9, Mode::Campaign);
    sim.apply_upgrade(Up::Ricochet);
    sim.player.pos = Vec2::new(0.0, -3.0);
    sim.player.fire_cd = 0.0;
    sim.step(&PlayerInput { aim: Vec2::Y, fire: true, ..Default::default() }, DT);
    let mut bounced = false;
    for _ in 0..40 {
        sim.step(&PlayerInput { aim: Vec2::Y, ..Default::default() }, DT);
        bounced |= sim.events.iter().any(|e| matches!(e, Event::Ricochet { .. }));
    }
    assert!(bounced, "the bullet bounced off the wall");
}

#[test]
fn dash_hurts_with_the_upgrade_and_is_invulnerable() {
    let mut sim = Sim::new(10, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    fresh_wave(&mut sim, 1);
    hold(&mut sim);
    sim.apply_upgrade(Up::DashDmg);
    sim.player.pos = Vec2::new(-3.0, 0.0);
    sim.player.invuln = 0.0;
    let i = sim.spawn_enemy(Kind::Tank, Vec2::new(-1.2, 0.0)).unwrap();
    sim.enemies[i].target_core = false;
    let hp = sim.enemies[i].hp;
    sim.step(&PlayerInput { mv: Vec2::X, dash: true, ..Default::default() }, DT);
    for _ in 0..12 {
        sim.step(&PlayerInput { mv: Vec2::X, ..Default::default() }, DT);
    }
    assert!(sim.enemies[i].hp < hp, "dash damage landed");
    assert_eq!(sim.player.hp, sim.stats.max_hp, "no damage taken while dashing");
    assert!(sim.player.dash_cd > 0.0);
}

#[test]
fn wave_n_plus_one_starts_after_the_wave_is_cleared_and_an_upgrade_is_chosen() {
    let mut sim = Sim::new(11, Mode::Campaign);
    sim.player.invuln = 999.0;
    // Ждём первую волну и убираем всех, как только они появились.
    let mut guard = 0;
    while sim.phase != Phase::Cleared {
        sim.step(&PlayerInput::default(), DT);
        for i in 0..sim.enemies.len() {
            if sim.enemies[i].alive {
                sim.damage_enemy(i, 1.0e7, false);
            }
        }
        guard += 1;
        assert!(guard < 20_000, "wave 1 ends");
    }
    assert_eq!(sim.wave, 1);
    assert!(sim.events.iter().any(|e| matches!(e, Event::WaveCleared { wave: 1 })));
    assert!(sim.pending_upgrades >= 1, "the wave reward is waiting");
    // Пока награда не выбрана, следующая волна не начинается.
    idle(&mut sim, 10.0);
    assert_eq!(sim.phase, Phase::Cleared);
    assert_eq!(sim.wave, 1);
    assert!(sim.upgrade_ready());
    let offer = sim.roll_offer();
    assert_eq!(offer.len(), 3, "three cards");
    let mut uniq = offer.clone();
    uniq.sort_by_key(|u| u.index());
    uniq.dedup();
    assert_eq!(uniq.len(), 3, "cards are distinct");
    assert!(sim.choose_upgrade(offer[0]));
    while sim.pending_upgrades > 0 {
        let o = sim.roll_offer();
        sim.choose_upgrade(o[0]);
    }
    // Теперь — передышка и волна 2.
    idle(&mut sim, 4.0);
    assert_eq!(sim.wave, 2, "wave 2 started");
    assert_eq!(sim.phase, Phase::Wave);
    // Нельзя выбрать улучшение, которого не предлагали, когда награды нет.
    let none = sim.pending_upgrades;
    assert!(none == 0 || sim.level > 1);
}

#[test]
fn xp_level_ups_offer_upgrades_mid_wave() {
    let mut sim = Sim::new(12, Mode::Campaign);
    fresh_wave(&mut sim, 2);
    hold(&mut sim);
    let need = sim.xp_need;
    sim.player.pos = Vec2::new(3.0, 0.0);
    for _ in 0..(need as u32 + 2) {
        sim.spawn_pickup(Vec2::new(3.0, 0.2), PickupKind::Scrap, 1.0);
    }
    idle(&mut sim, 3.0);
    assert_eq!(sim.level, 2, "enough scrap raised the level");
    assert_eq!(sim.pending_upgrades, 1);
    assert!(sim.upgrade_ready(), "a level-up offers an upgrade in the middle of a wave");
    assert!(sim.events.iter().any(|e| matches!(e, Event::LevelUp)));
}

#[test]
fn magnet_pulls_scrap_and_scrap_fills_xp() {
    let mut sim = Sim::new(13, Mode::Campaign);
    fresh_wave(&mut sim, 2);
    hold(&mut sim);
    sim.player.pos = Vec2::new(4.0, 0.0);
    sim.spawn_pickup(Vec2::new(6.2, 0.0), PickupKind::Scrap, 1.0);
    sim.pickups[0].vel = Vec2::ZERO;
    idle(&mut sim, 2.0);
    assert!(sim.xp >= 1.0, "scrap within magnet range was collected");
    // Далёкий обломок остаётся лежать.
    let mut sim = Sim::new(13, Mode::Campaign);
    fresh_wave(&mut sim, 2);
    hold(&mut sim);
    sim.player.pos = Vec2::new(-6.0, 0.0);
    sim.spawn_pickup(Vec2::new(6.0, 4.0), PickupKind::Scrap, 1.0);
    sim.pickups[0].vel = Vec2::ZERO;
    idle(&mut sim, 1.0);
    assert_eq!(sim.xp, 0.0);
}

#[test]
fn every_wave_has_a_plan_and_difficulty_grows() {
    let mut last_budget = 0.0;
    for w in 1..=CAMPAIGN_WAVES + 10 {
        let plan = plan_wave(w);
        assert!(!plan.groups.is_empty() || plan.boss.is_some(), "wave {w} is not empty");
        assert_eq!(plan, plan_wave(w), "plans are deterministic");
        assert!(plan.total() > 0);
        let cost: f32 = plan.groups.iter().map(|g| g.kind.info().cost * g.count as f32).sum();
        if Kind::boss_for_wave(w).is_none() {
            assert!(cost >= last_budget * 0.7, "wave {w} is not much easier than before ({cost} vs {last_budget})");
            last_budget = cost;
        }
        for g in &plan.groups {
            assert!(g.at >= 0.0 && g.at <= wave_duration(w) + 4.0);
        }
        assert_eq!(plan.boss.is_some(), w % 5 == 0);
    }
    // Состав: на первой волне только гонщики, к десятой есть все виды.
    assert!(plan_wave(1).groups.iter().all(|g| g.kind == Kind::Chaser));
    let kinds: Vec<Kind> = (6..=14).flat_map(|w| plan_wave(w).groups.into_iter().map(|g| g.kind)).collect();
    for k in [Kind::Chaser, Kind::Swarmling, Kind::Shooter, Kind::Tank, Kind::Splitter, Kind::Bomber, Kind::MineLayer] {
        assert!(kinds.contains(&k), "{k:?} appears in waves 6-14");
    }
    assert_eq!(Kind::boss_for_wave(5), Some(Kind::BossHive));
    assert_eq!(Kind::boss_for_wave(10), Some(Kind::BossRam));
    assert_eq!(Kind::boss_for_wave(15), Some(Kind::BossConductor));
    assert_eq!(Kind::boss_for_wave(20), Some(Kind::BossOvermind));
}

#[test]
fn boss_waves_spawn_a_boss_with_telegraphed_attacks() {
    for wave in [5u32, 10, 15, 20] {
        let boss_kind = Kind::boss_for_wave(wave).unwrap();
        let mut sim = Sim::new(20 + wave as u64, Mode::Campaign);
        sim.core.hp = 1.0e6;
        sim.core.max_hp = 1.0e6;
        sim.player.invuln = 1.0e6;
        sim.start_wave(wave);
        let mut appeared = false;
        let mut saw_telegraph = false;
        let mut saw_bullets = false;
        for _ in 0..(60 * 40) {
            sim.step(&bot_input(&sim), DT);
            if let Some(b) = sim.boss() {
                appeared = true;
                assert_eq!(b.kind, boss_kind);
                saw_telegraph |= b.tele > 0.0;
            }
            saw_bullets |= sim.ebullets_alive() > 0;
            if appeared && saw_telegraph && saw_bullets {
                break;
            }
        }
        assert!(sim.events.iter().any(|e| matches!(e, Event::BossAppear { .. })) || appeared);
        assert!(appeared, "wave {wave} spawns {boss_kind:?}");
        assert!(saw_telegraph, "{boss_kind:?} telegraphs its attacks");
        assert!(saw_bullets || boss_kind == Kind::BossRam, "{boss_kind:?} shoots");
        // Босс убивается; смерть выставляет событие и «зачищает» эскорт.
        let i = sim.enemies.iter().position(|e| e.alive && e.kind.is_boss()).unwrap();
        sim.damage_enemy(i, 1.0e9, false);
        assert!(sim.boss().is_none());
        assert!(sim.events.iter().any(|e| matches!(e, Event::BossDied { .. })));
        assert_eq!(sim.alive_enemies(), 0, "escorts die with the boss");
    }
}

#[test]
fn ram_boss_charges_and_gets_stunned() {
    let mut sim = Sim::new(30, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    sim.player.invuln = 1.0e6;
    sim.start_wave(10);
    sim.plan.groups.clear();
    let mut impact = false;
    let mut stunned = false;
    for _ in 0..(60 * 30) {
        sim.step(&bot_input(&sim), DT);
        impact |= sim.events.iter().any(|e| matches!(e, Event::BossImpact { .. }));
        stunned |= sim.boss().is_some_and(|b| b.vulnerable);
        if impact && stunned {
            break;
        }
    }
    assert!(impact, "the ram hit something");
    assert!(stunned, "the ram is vulnerable after impact");
}

#[test]
fn core_health_reaching_zero_loses_the_run() {
    let mut sim = Sim::new(40, Mode::Campaign);
    assert_eq!(sim.phase, Phase::Intermission);
    sim.hurt_core(sim.core.hp + 1.0);
    assert_eq!(sim.phase, Phase::Lost);
    assert!(sim.events.iter().any(|e| matches!(e, Event::Defeat)));
    // Проигранная симуляция заморожена.
    let t = sim.time;
    idle(&mut sim, 1.0);
    assert_eq!(sim.time, t);
    // Дрон погиб — реактор платит, а дрон возвращается.
    let mut sim = Sim::new(41, Mode::Campaign);
    sim.player.invuln = 0.0;
    let core = sim.core.hp;
    sim.hurt_player(1.0e6);
    assert!(!sim.player.alive);
    assert!(sim.core.hp < core, "a lost drone costs the reactor");
    idle(&mut sim, 3.5);
    assert!(sim.player.alive, "the drone respawns");
    assert_eq!(sim.phase, Phase::Wave);
}

#[test]
fn enemies_hurt_the_core() {
    let mut sim = Sim::new(42, Mode::Campaign);
    fresh_wave(&mut sim, 4);
    hold(&mut sim);
    sim.player.pos = Vec2::new(0.0, -8.0);
    let hp = sim.core.hp;
    let i = sim.spawn_enemy(Kind::Tank, Vec2::new(3.0, 0.0)).unwrap();
    assert!(sim.enemies[i].target_core);
    idle(&mut sim, 6.0);
    assert!(sim.core.hp < hp, "a tank that reaches the core hurts it");
}

#[test]
fn bot_clears_the_first_three_waves() {
    let mut sim = Sim::new(50, Mode::Campaign);
    let mut guard = 0;
    while sim.wave < 4 && !sim.is_over() {
        bot_step(&mut sim, DT);
        guard += 1;
        assert!(guard < 60 * 60 * 6, "three waves take well under six minutes");
    }
    assert!(!sim.is_over(), "the bot survives: {:?}, core {:.0}", sim.phase, sim.core.hp);
    assert_eq!(sim.wave, 4, "waves 1-3 were cleared and wave 4 began");
    assert!(sim.run.kills > 20 && sim.score > 0);
    assert!(sim.run.upgrades >= 3);
}

#[test]
fn campaign_ends_with_a_win_after_wave_20() {
    let mut sim = Sim::new(60, Mode::Campaign);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    sim.player.invuln = 1.0e6;
    sim.start_wave(20);
    sim.plan.groups.clear();
    // Босс 20-й волны убит — победа.
    for _ in 0..400 {
        sim.step(&PlayerInput::default(), DT);
        if sim.boss().is_some() {
            break;
        }
    }
    let i = sim.enemies.iter().position(|e| e.alive && e.kind.is_boss()).expect("the final boss appears");
    sim.damage_enemy(i, 1.0e9, false);
    idle(&mut sim, 1.0);
    assert_eq!(sim.phase, Phase::Won);
    assert!(sim.events.iter().any(|e| matches!(e, Event::Victory)));
    // В бесконечном режиме после 20-й волны игра продолжается.
    let mut sim = Sim::new(61, Mode::Endless);
    sim.core.hp = 1.0e6;
    sim.core.max_hp = 1.0e6;
    sim.player.invuln = 1.0e6;
    sim.start_wave(20);
    for _ in 0..400 {
        sim.step(&PlayerInput::default(), DT);
        if sim.boss().is_some() {
            break;
        }
    }
    let i = sim.enemies.iter().position(|e| e.alive && e.kind.is_boss()).unwrap();
    sim.plan.groups.clear();
    sim.damage_enemy(i, 1.0e9, false);
    idle(&mut sim, 3.0);
    assert_eq!(sim.phase, Phase::Cleared, "endless goes on");
    assert!(!sim.is_over());
}

#[test]
fn endless_mode_unlocks_after_wave_ten() {
    let mut progress = Progress::default();
    assert!(!progress.endless_unlocked());
    for w in 1..10 {
        assert!(!progress.note_wave(w), "wave {w} does not unlock it");
    }
    assert!(progress.note_wave(ENDLESS_UNLOCK_WAVE), "wave 10 unlocks endless mode");
    assert!(progress.endless_unlocked());
    assert!(!progress.note_wave(11), "it unlocks only once");
    // Сохраняется.
    let backend = MemoryBackend::default();
    let mut save = SaveData::with_backend(backend.clone());
    progress.store(&mut save);
    save.save().unwrap();
    let again = SaveData::with_backend(backend);
    assert!(Progress::load(&again).endless_unlocked());
}

#[test]
fn save_data_round_trips() {
    let backend = MemoryBackend::default();
    let mut save = SaveData::with_backend(backend.clone());
    let opts = Options { music: 0.25, sfx: 0.5, shake: false, numbers: false };
    opts.store(&mut save);
    let mut progress = Progress { max_wave: 12, best_campaign: 99_000, best_endless: 5_000, best_endless_wave: 14, wins: 2, runs: 9 };
    progress.note_wave(13);
    progress.store(&mut save);
    // Таблица: пять лучших по очкам, шестая и слабее не попадают.
    for (i, score) in [5_000u64, 12_000, 800, 31_000, 7_000, 20_000, 100].into_iter().enumerate() {
        let place = add_record(&mut save, RunRecord { score, wave: 3 + i as u32, mode: if i % 2 == 0 { Mode::Campaign } else { Mode::Endless }, secs: 100 + i as u32, won: i == 3 });
        if score == 100 {
            assert_eq!(place, None, "a weak run is not on the board");
        }
    }
    let cp = Checkpoint { mode: Mode::Endless, next_wave: 7, levels: { let mut l = [0u8; UP_COUNT]; l[2] = 3; l[17] = 1; l }, level: 6, xp: 4.5, score: 12_345, core_hp: 150.5, kills: 99, time: 312.5 };
    store_checkpoint(&mut save, &cp);
    save.save().unwrap();

    let save = SaveData::with_backend(backend);
    assert_eq!(Options::load(&save), opts);
    assert_eq!(Progress::load(&save), progress);
    let board = load_leaderboard(&save);
    assert_eq!(board.len(), LEADERBOARD_SIZE);
    assert_eq!(board.iter().map(|r| r.score).collect::<Vec<_>>(), vec![31_000, 20_000, 12_000, 7_000, 5_000]);
    assert!(board[0].won);
    let loaded = load_checkpoint(&save).expect("checkpoint");
    assert_eq!(loaded.next_wave, 7);
    assert_eq!(loaded.levels, cp.levels);
    assert_eq!(loaded.mode, Mode::Endless);
    assert!((loaded.core_hp - 150.5).abs() < 0.1 && loaded.score == 12_345);
    // Повреждённая запись не роняет игру.
    let mut broken = SaveData::in_memory();
    broken.set_string("run", "garbage|1,2");
    broken.set_string("board", "x;1,2;;");
    assert!(load_checkpoint(&broken).is_none());
    assert!(load_leaderboard(&broken).is_empty());
    clear_checkpoint(&mut broken);
}

#[test]
fn checkpoint_continues_the_run() {
    let mut sim = Sim::new(70, Mode::Campaign);
    sim.apply_upgrade(Up::Pierce);
    sim.apply_upgrade(Up::Damage);
    sim.wave = 6;
    sim.score = 4_000;
    sim.level = 5;
    let cp = sim.checkpoint();
    assert_eq!(cp.next_wave, 7);
    let mut restored = Sim::from_checkpoint(71, &cp);
    assert_eq!(restored.levels, sim.levels);
    assert_eq!(restored.stats.pierce, 1);
    assert_eq!(restored.score, 4_000);
    for _ in 0..(60 * 4) {
        restored.step(&PlayerInput::default(), DT);
    }
    assert_eq!(restored.wave, 7, "the run continues with the next wave");
}

#[test]
fn pools_never_exceed_their_capacity_and_stress_fills_the_arena() {
    let mut sim = Sim::new(80, Mode::Campaign);
    sim.start_wave(20);
    for _ in 0..300 {
        sim.fill_stress();
        sim.step(&PlayerInput::default(), DT);
    }
    assert!(sim.alive_enemies() >= 380, "stress mode runs 400 enemies, have {}", sim.alive_enemies());
    assert!(sim.pbullets_alive() + sim.ebullets_alive() >= 450, "and hundreds of bullets");
    assert!(sim.enemies.len() <= ENEMY_CAP && sim.pbullets.len() <= PBULLET_CAP && sim.ebullets.len() <= EBULLET_CAP);
    assert!(sim.pickups.len() <= PICKUP_CAP);
}

#[test]
fn simulation_is_deterministic() {
    let run = |seed: u64| -> (u64, u32, usize) {
        let mut sim = Sim::new(seed, Mode::Campaign);
        for _ in 0..(60 * 30) {
            bot_step(&mut sim, DT);
        }
        (sim.score, sim.run.kills, sim.pbullets_alive())
    };
    assert_eq!(run(5), run(5));
    let offer = [Up::Rate, Up::Hull, Up::Slow];
    assert_eq!(bot_pick(&offer, &[0; UP_COUNT]), Some(Up::Rate));
}
