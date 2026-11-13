"""Генератор пиксель-арт ассетов для примера features2d (без загрузок из сети).

Запуск из корня репозитория: python tools/gen_2d_demo_assets.py
Пишет в assets/sprites: hero_sheet.png (6x3 кадра по 16x16), tileset.png (4x2 тайла
по 16x16), coin_sheet.png (4x1 кадра по 16x16) и их .meta с фильтром Nearest.
"""
import os
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sprites")


def c(hexstr):
    h = hexstr.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4)) + (255,)


def rect(img, x, y, w, h, col):
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            if 0 <= xx < img.width and 0 <= yy < img.height:
                img.putpixel((xx, yy), col)


def put(img, x, y, col):
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), col)


def meta(name, guid):
    path = os.path.join(OUT, name + ".meta")
    with open(path, "w", encoding="utf-8") as f:
        f.write('(\n    guid: "%s",\n    importer: "Texture",\n    settings: {\n        "filter": Enum("Nearest"),\n    },\n)\n' % guid)


SKIN, HAIR, SHIRT, PANTS, SHOE, EYE = c("f2c9a0"), c("4a2f1b"), c("3b82f6"), c("2f3a5f"), c("1d1d2b"), c("101018")
SCARF = c("e8453c")


def hero_frame(img, ox, oy, bob=0, legs=(0, 0), arm=0, squash=0, look=0):
    """Персонаж 10x14 в клетке 16x16 (ноги у нижней кромки, x по центру)."""
    base = oy + 15
    top = base - 13 + squash
    cx = ox + 8
    # ноги
    for i, dx in enumerate((-2, 1)):
        lift = legs[i]
        rect(img, cx + dx, base - 4 - lift, 2, 4 + lift, PANTS)
        rect(img, cx + dx, base - 1 - lift + (0 if lift == 0 else 0), 2, 1, SHOE)
    # торс
    rect(img, cx - 3, top + 6 + bob, 6, 5 - 0, SHIRT)
    rect(img, cx - 3, top + 6 + bob, 6, 1, SCARF)
    # рука
    rect(img, cx + 3, top + 7 + bob + arm, 1, 3, SKIN)
    rect(img, cx - 4, top + 7 + bob - arm, 1, 3, SKIN)
    # голова
    rect(img, cx - 3, top + bob, 6, 6, SKIN)
    rect(img, cx - 3, top + bob, 6, 2, HAIR)
    rect(img, cx - 3, top + 2 + bob, 1, 2, HAIR)
    put(img, cx + 1 + look, top + 3 + bob, EYE)
    put(img, cx - 1 + look, top + 3 + bob, EYE)


def make_hero():
    img = Image.new("RGBA", (96, 48), (0, 0, 0, 0))
    # ряд 0: idle (4 кадра), дыхание
    for i, bob in enumerate((0, 0, 1, 1)):
        hero_frame(img, i * 16, 0, bob=bob, arm=i % 2)
    # ряд 1: бег (6 кадров)
    cycle = [((0, 2), 1), ((0, 3), 0), ((0, 1), 1), ((2, 0), 1), ((3, 0), 0), ((1, 0), 1)]
    for i, (legs, arm) in enumerate(cycle):
        hero_frame(img, i * 16, 16, bob=1 if i % 3 == 1 else 0, legs=legs, arm=arm)
    # ряд 2: прыжок вверх, пик, падение
    hero_frame(img, 0, 32, legs=(1, 2), arm=-1, squash=0)
    hero_frame(img, 16, 32, legs=(2, 2), arm=-1, squash=0)
    hero_frame(img, 32, 32, legs=(0, 1), arm=1, squash=0)
    img.save(os.path.join(OUT, "hero_sheet.png"))
    meta("hero_sheet.png", "a1b2c3d4e5f60718293a4b5c6d7e8f90")


def make_tiles():
    img = Image.new("RGBA", (64, 32), (0, 0, 0, 0))
    grass, grass_d = c("5fbf4a"), c("3f8f33")
    dirt, dirt_d, dirt_l = c("8a5a3a"), c("6b4429"), c("a06c46")
    stone, stone_d, stone_l = c("7d8597"), c("5a6275"), c("9aa3b5")

    def tile(i):
        return (i % 4) * 16, (i // 4) * 16

    # 0 -> индекс 1: земля с травой сверху
    x, y = tile(0)
    rect(img, x, y, 16, 16, dirt)
    for (a, b) in [(2, 7), (9, 5), (12, 11), (5, 12), (14, 8), (3, 10)]:
        put(img, x + a, y + b, dirt_d)
    for (a, b) in [(6, 9), (11, 8), (1, 13)]:
        put(img, x + a, y + b, dirt_l)
    rect(img, x, y, 16, 4, grass)
    rect(img, x, y + 3, 16, 1, grass_d)
    for a in (1, 4, 7, 10, 13):
        put(img, x + a, y + 4, grass_d)
        put(img, x + a + 1, y + 4, grass)
    # 1 -> индекс 2: земля
    x, y = tile(1)
    rect(img, x, y, 16, 16, dirt)
    for (a, b) in [(2, 3), (9, 6), (12, 12), (5, 9), (14, 2), (3, 13), (8, 1)]:
        put(img, x + a, y + b, dirt_d)
    for (a, b) in [(6, 4), (11, 9), (1, 7), (13, 14)]:
        put(img, x + a, y + b, dirt_l)
    # 2 -> индекс 3: каменная кладка
    x, y = tile(2)
    rect(img, x, y, 16, 16, stone)
    for row in range(0, 16, 4):
        rect(img, x, y + row + 3, 16, 1, stone_d)
        off = 0 if (row // 4) % 2 == 0 else 4
        for col in range(off, 16, 8):
            rect(img, x + col, y + row, 1, 4, stone_d)
        rect(img, x, y + row, 16, 1, stone_l)
    # 3 -> индекс 4: деревянная платформа (одна сторона)
    x, y = tile(3)
    wood, wood_d, wood_l = c("b07a43"), c("7a4e27"), c("d29a5f")
    rect(img, x, y, 16, 5, wood)
    rect(img, x, y, 16, 1, wood_l)
    rect(img, x, y + 4, 16, 1, wood_d)
    for a in (0, 8, 15):
        put(img, x + a, y + 2, wood_d)
    rect(img, x + 2, y + 5, 2, 3, wood_d)
    rect(img, x + 12, y + 5, 2, 3, wood_d)
    # 4 -> индекс 5: куст (декор)
    x, y = tile(4)
    bush, bush_d, bush_l = c("3f9a45"), c("2c6e33"), c("6fd05f")
    for (cx, cy, r) in [(5, 11, 5), (10, 10, 6), (13, 13, 3)]:
        for yy in range(16):
            for xx in range(16):
                if (xx - cx) ** 2 + (yy - cy) ** 2 <= r * r and yy >= 4:
                    put(img, x + xx, y + yy, bush)
    for (a, b) in [(4, 9), (9, 8), (12, 12), (7, 12)]:
        put(img, x + a, y + b, bush_l)
    rect(img, x, y + 15, 16, 1, bush_d)
    # 5 -> индекс 6: трава и цветок
    x, y = tile(5)
    for a in (3, 6, 9, 12):
        rect(img, x + a, y + 10, 1, 6, grass_d)
        rect(img, x + a + 1, y + 11, 1, 5, grass)
    rect(img, x + 7, y + 6, 1, 6, grass_d)
    rect(img, x + 6, y + 4, 3, 3, c("ffd84a"))
    put(img, x + 7, y + 5, c("e8453c"))
    # 6 -> индекс 7: ящик
    x, y = tile(6)
    box, box_d, box_l = c("c48a4a"), c("7a4e27"), c("e0ac6c")
    rect(img, x + 1, y + 1, 14, 14, box)
    rect(img, x + 1, y + 1, 14, 1, box_l)
    for k in range(14):
        put(img, x + 1 + k, y + 1 + k, box_d)
        put(img, x + 14 - k, y + 1 + k, box_d)
    for (rx, ry, rw, rh) in [(1, 1, 14, 1), (1, 14, 14, 1), (1, 1, 1, 14), (14, 1, 1, 14)]:
        rect(img, x + rx, y + ry, rw, rh, box_d)
    # 7 -> индекс 8: факел (декор со свечением в игре)
    x, y = tile(7)
    rect(img, x + 7, y + 7, 2, 8, c("6b4429"))
    rect(img, x + 6, y + 6, 4, 2, c("8a8f9c"))
    img.save(os.path.join(OUT, "tileset.png"))
    meta("tileset.png", "b2c3d4e5f60718293a4b5c6d7e8f90a1")


def make_coin():
    img = Image.new("RGBA", (64, 16), (0, 0, 0, 0))
    gold, gold_l, gold_d = c("ffd23f"), c("fff1a0"), c("c9901a")
    for i, half in enumerate((6, 4, 1, 4)):
        x0 = i * 16 + 8
        for yy in range(2, 14):
            for xx in range(-half, half + 1):
                inside = (xx / max(half, 0.5)) ** 2 + ((yy - 8) / 6.0) ** 2 <= 1.0
                if inside:
                    put(img, x0 + xx, yy, gold)
        if half > 2:
            for yy in range(4, 9):
                put(img, x0 - half + 2, yy, gold_l)
            rect(img, x0 + half - 1, 6, 1, 5, gold_d)
            rect(img, x0 - 1, 5, 2, 6, gold_d)
    img.save(os.path.join(OUT, "coin_sheet.png"))
    meta("coin_sheet.png", "c3d4e5f60718293a4b5c6d7e8f90a1b2")


if __name__ == "__main__":
    make_hero()
    make_tiles()
    make_coin()
    print("assets written to", os.path.abspath(OUT))
