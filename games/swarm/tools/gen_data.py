#!/usr/bin/env python3
"""Таблицы баланса «Роя» -> assets/data/*.ron (враги, боссы, улучшения, баланс).

Таблицу волн (waves.ron) пишет Rust-запекатель `cargo run -p swarm --example bake_assets`:
для волн 1-20 он записывает то, что даёт генератор волн из этих таблиц; бесконечный режим
продолжает генератор по параметрам из balance.ron. После запуска таблицы можно править руками.
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(os.path.dirname(HERE), "assets", "data")
os.makedirs(OUT, exist_ok=True)

# id, name, prefab, hp, speed, radius, contact, score, scrap, cost, faces, color, unlock, weight, group, core_chance, core_from
E = [
    ("Chaser", "Гонщик", "enemies/chaser", 30, 2.7, 0.42, 10, 10, (1, 1), 1.0, False, (2.2, 0.55, 0.45), 1, 5.0, (3, 6), 0.2, 3),
    ("Swarmling", "Рой-мальок", "enemies/swarmling", 11, 4.6, 0.26, 6, 5, (0, 1), 0.38, True, (1.6, 2.2, 0.5), 2, 3.0, (6, 12), 0.0, 1),
    ("Shooter", "Стрелок", "enemies/shooter", 42, 1.9, 0.45, 8, 25, (2, 2), 2.2, True, (1.6, 0.9, 2.4), 3, 2.2, (1, 3), 0.0, 1),
    ("Tank", "Танк", "enemies/tank", 240, 1.1, 0.9, 14, 60, (5, 7), 5.5, True, (0.8, 2.2, 1.3), 4, 1.0, (1, 2), 1.0, 1),
    ("Splitter", "Делитель", "enemies/splitter", 75, 2.0, 0.62, 10, 20, (1, 2), 2.6, False, (2.2, 1.8, 0.5), 6, 1.6, (1, 3), 0.3, 1),
    ("SplitterMini", "Осколок", "enemies/splitter_mini", 22, 3.3, 0.32, 6, 6, (0, 1), 0.0, False, (2.2, 1.8, 0.5), 99, 0.0, (1, 1), 0.0, 1),
    ("Bomber", "Подрывник", "enemies/bomber", 26, 3.5, 0.42, 0, 30, (2, 2), 2.2, False, (2.4, 1.4, 0.5), 7, 1.6, (2, 4), 0.4, 1),
    ("MineLayer", "Минёр", "enemies/minelayer", 70, 2.0, 0.5, 8, 35, (3, 3), 4.0, True, (0.6, 2.0, 1.8), 9, 0.6, (1, 1), 0.0, 1),
    ("Mine", "Мина", "enemies/mine", 16, 0.0, 0.34, 0, 0, (0, 0), 0.0, False, (2.4, 0.5, 0.7), 99, 0.0, (1, 1), 0.0, 1),
    ("BossHive", "Матка", "bosses/hive", 1700, 1.1, 1.9, 18, 1500, (28, 28), 0.0, False, (2.4, 0.7, 1.5), 99, 0.0, (1, 1), 0.0, 1),
    ("BossRam", "Таран", "bosses/ram", 3600, 1.9, 1.8, 30, 3000, (36, 36), 0.0, True, (2.4, 1.6, 0.6), 99, 0.0, (1, 1), 0.0, 1),
    ("BossConductor", "Дирижёр", "bosses/conductor", 5800, 1.4, 1.7, 22, 4500, (44, 44), 0.0, False, (0.9, 1.4, 2.6), 99, 0.0, (1, 1), 0.0, 1),
    ("BossOvermind", "Сверхразум", "bosses/overmind", 10500, 1.5, 2.1, 30, 9000, (60, 60), 0.0, False, (2.6, 0.6, 0.6), 99, 0.0, (1, 1), 0.0, 1),
]
SCRIPTS = {
    "BossHive": ["Idle(1.4)", "Ring(n: 16, speed: 5.0, count: 2)", "Idle(1.2)", "Summon(kind: Swarmling, count: 8)", "Idle(1.0)",
                 "Aimed(volleys: 3, per: 3)", "Idle(1.2)", "Summon(kind: Chaser, count: 4)", "Ring(n: 20, speed: 5.5, count: 2)"],
    "BossRam": ["Idle(1.2)", "Charge", "Idle(0.8)", "Charge", "Idle(0.6)", "Mines(count: 4)", "Aimed(volleys: 2, per: 3)", "Idle(1.0)", "Charge"],
    "BossConductor": ["Idle(1.0)", "Spiral(secs: 4.0, arms: 3)", "Idle(0.8)", "Blink", "Summon(kind: Shooter, count: 3)", "Idle(0.8)",
                      "Spiral(secs: 4.0, arms: 2)", "Blink", "Ring(n: 20, speed: 5.5, count: 2)"],
    "BossOvermind": ["Idle(1.0)", "Ring(n: 24, speed: 5.5, count: 3)", "Aimed(volleys: 4, per: 5)", "Summon(kind: Bomber, count: 4)", "Charge",
                     "Spiral(secs: 5.0, arms: 4)", "Blink", "Summon(kind: Chaser, count: 6)", "Ring(n: 28, speed: 6.0, count: 3)", "Idle(0.6)"],
}


def f(x):
    return repr(float(x))


def enemies():
    out = ["// Враги и боссы: характеристики, префабы, правила появления, сценарии боссов.",
           "// Читается игрой через базу ассетов (работает и из пака). Править можно в редакторе.", "["]
    for (i, name, prefab, hp, sp, r, ct, sc, scrap, cost, faces, col, unlock, w, grp, cc, ccf) in E:
        out.append("    (")
        out.append(f"        id: {i},")
        out.append(f'        name: "{name}",')
        out.append(f'        prefab: "prefabs/{prefab}.prefab",')
        out.append(f"        hp: {f(hp)}, speed: {f(sp)}, radius: {f(r)}, contact: {f(ct)}, score: {sc},")
        out.append(f"        scrap: ({scrap[0]}, {scrap[1]}), cost: {f(cost)}, faces_heading: {str(faces).lower()},")
        out.append(f"        color: ({f(col[0])}, {f(col[1])}, {f(col[2])}),")
        out.append(f"        unlock_wave: {unlock}, weight: {f(w)}, group: ({grp[0]}, {grp[1]}),")
        out.append(f"        core_chance: {f(cc)}, core_chance_from_wave: {ccf},")
        if i in SCRIPTS:
            out.append("        script: [")
            for s in SCRIPTS[i]:
                out.append(f"            {s},")
            out.append("        ],")
        out.append("    ),")
    out.append("]")
    return "\n".join(out) + "\n"


# id, name, icon, max, weight, params, description ({0}.. — числа считает код по params)
U = [
    ("Rate", "Скорострельность", "rate", 6, 1.2, [0.22], "Темп стрельбы +{0}% (всего +{1}%)"),
    ("Damage", "Мощность залпа", "damage", 8, 1.2, [0.3], "Урон снарядов +{0}% (всего +{1}%)"),
    ("Multishot", "Мультизалп", "multishot", 4, 1.0, [], "Выстрел делится веером на {0} {1}"),
    ("Pierce", "Бронебойные", "pierce", 4, 0.9, [], "Снаряд проходит насквозь, целей: {0}"),
    ("Ricochet", "Рикошет", "ricochet", 3, 0.8, [], "Отскоков снаряда от стены: {0}"),
    ("Explosive", "Разрывные заряды", "explosive", 4, 0.9, [0.9, 0.3], "Взрыв при попадании, радиус {0}"),
    ("Shield", "Орбитальные щиты", "shield", 5, 0.9, [14.0, 4.0], "Щитов-шаров вокруг дрона: {0}"),
    ("Missile", "Самонаводящиеся ракеты", "missile", 5, 0.9, [4.2, 0.5, 1.6], "Ракеты каждые {0} с, за залп — {1}"),
    ("DashDmg", "Таранный рывок", "dashdmg", 4, 0.8, [45.0], "Рывок наносит {0} урона всем на пути"),
    ("Magnet", "Магнит обломков", "magnet", 4, 0.9, [1.1], "Радиус притяжения обломков: {0}"),
    ("Hull", "Усиленный корпус", "hull", 5, 1.0, [25.0], "Прочность дрона +25 (всего {0})"),
    ("Repair", "Ремонт реактора", "repair", 6, 1.0, [0.35, 30.0], "Реактор сразу получает +30 прочности и чинится на {0} в секунду"),
    ("Slow", "Замедляющее поле", "slow", 4, 0.8, [0.2, 0.1, 0.7, 3.0, 0.4], "Враги рядом замедлены на {0}%"),
    ("Crit", "Критический удар", "crit", 5, 0.9, [0.07, 0.5], "Шанс критического удара {0}% (урон ×2)"),
    ("Thrusters", "Форсаж двигателей", "thrusters", 4, 0.8, [0.1], "Скорость дрона +10% (всего +{0}%)"),
    ("DashCd", "Ускоритель рывка", "dashcd", 3, 0.7, [0.85], "Перезарядка рывка: {0} с"),
    ("Armor", "Броня реактора", "armor", 4, 0.9, [40.0, 0.92], "Прочность реактора +40, урон по нему меньше на 8% (всего ×{0})"),
    ("Siphon", "Нано-ремонтники", "siphon", 4, 0.7, [0.03], "Шанс {0}% выбить ремкомплект из врага"),
]


def upgrades():
    out = ["// Каталог улучшений: название, иконка, предел уровней, вес в выпадении, числа и описание.",
           "// В описании {0}, {1} — числа, которые код считает по `params` для следующего уровня.", "["]
    for (i, name, icon, mx, w, params, desc) in U:
        ps = ", ".join(f(p) for p in params)
        out.append(f'    (id: {i}, name: "{name}", icon: "ui/icons/{icon}.png", max: {mx}, weight: {f(w)}, params: [{ps}],')
        out.append(f'        desc: "{desc}"),')
    out.append("]")
    return "\n".join(out) + "\n"


BALANCE = """// Общие числа игры: размеры арены, базовые характеристики дрона и реактора.
(
    arena_radius: 9.0,
    core_radius: 1.15,
    player_radius: 0.42,
    player_hp: 100.0,
    core_hp: 200.0,
    fire_interval: 0.26,
    damage: 10.0,
    move_speed: 6.4,
    dash_cooldown: 2.4,
    dash_speed: 24.0,
    dash_time: 0.17,
    magnet_radius: 2.6,
    repair_drop: 0.015,
    drone_loss_core_damage: 25.0,
    respawn_time: 3.0,
    xp_base: 12.0,
    xp_per_level: 5.0,
    xp_per_level_sq: 0.6,
    combo_time: 3.0,
    combo_step: 0.1,
    combo_cap: 8.0,
    // Генератор волн (после 20-й волны и для запекания таблицы).
    generator: (
        budget_base: 6.0,
        budget_per_wave: 3.2,
        budget_per_wave_sq: 0.28,
        boss_budget_mult: 0.4,
        duration_base: 16.0,
        duration_per_wave: 1.1,
        duration_max: 44.0,
        hp_per_wave: 0.07,
        hp_endless_extra: 0.12,
        speed_per_wave: 0.012,
        speed_cap: 1.4,
        boss_hp_per_wave: 0.04,
        boss_every: 5,
    ),
)
"""

for name, text in (("enemies.ron", enemies()), ("upgrades.ron", upgrades()), ("balance.ron", BALANCE)):
    with open(os.path.join(OUT, name), "w", newline="\n", encoding="utf-8") as fh:
        fh.write(text)
print("data written to", OUT)
