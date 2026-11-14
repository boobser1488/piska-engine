#!/usr/bin/env python3
"""Генератор карт «Хранителя фонаря»: шесть уровней и меню -> `assets/scenes/*.scene`.

Запуск: `python games/lantern/tools/gen_levels.py` (после `gen_assets.py`: нужны GUID картинок).
Уровни описаны здесь вручную, кусок за куском (земля, ямы, облачка, шипы, враги, светлячки);
построитель следит, чтобы основной маршрут был проходим: ширина ям и высота ступеней не
больше пределов, заданных для уровня (с учётом открытых способностей). Рядом с каждым
уровнем пишется текстовый эскиз в `tools/preview/levelN.txt`.

Символы карты: `#` скала, `=` «облачко», `^` `v` шипы вверх и вниз, `>` `<` шипы на стене.
Объекты — строки `вид x y ключ=значение … | текст` (см. `src/spawn.rs`): x — середина клетки,
y — низ клетки (для парящих — середина).
"""
import math
import os
import random

from gen_assets import guid_for

HERE = os.path.dirname(os.path.abspath(__file__))
SCENES = os.path.join(HERE, "..", "assets", "scenes")
PREVIEW = os.path.join(HERE, "preview")
H = 30
# Стабильные номера (`id`) узлов сцены, на которые ссылаются поля-ссылки скриптов.
DECO_ID = 100
TILES_ID = 101
INFO = {}
BLURBS = ["Первые огоньки", "Пружины и подъёмы", "Двойной прыжок в деле", "Платформы и шипы", "Рывок сквозь тьму", "Хозяин пещер"]


def lin(c):
    """sRGB 0..1 -> линейное."""
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


BOOST = (1.0, 1.0, 1.0)
BOOSTS = [(0.8, 1.15, 1.05), (1.35, 0.8, 1.4), (0.8, 1.25, 1.5), (1.6, 1.0, 0.55), (1.05, 1.3, 1.6), (1.5, 0.7, 0.95)]


def set_boost(i):
    global BOOST
    BOOST = BOOSTS[i]


def col(r, g, b, a=1.0, hdr=1.0):
    r, g, b = r * BOOST[0], g * BOOST[1], b * BOOST[2]
    return "Color(%.4f, %.4f, %.4f, %.3f)" % (lin(r) * hdr, lin(g) * hdr, lin(b) * hdr, a)


def hexcol(h, a=1.0):
    return col(((h >> 16) & 255) / 255, ((h >> 8) & 255) / 255, (h & 255) / 255, a)


def ron_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


class Level:
    def __init__(self, index, width, title, subtitle, need, par, music, gap, rise, theme, ceil=H - 4):
        self.index, self.w, self.title, self.subtitle = index, width, title, subtitle
        self.need, self.par, self.music = need, par, music
        self.gap_limit, self.rise_limit = gap, rise
        self.theme = theme
        self.g = [["." for _ in range(width)] for _ in range(H)]  # g[y][x], y снизу
        self.ents = []
        self.ff = 0
        self.prev = None  # (x1, top) предыдущего звена маршрута
        self.rng = random.Random(1000 + index)
        self.base_ceil = ceil
        self.keep = []  # диапазоны x, где потолок не трогаем (арена)
        # рамка: стены и потолок
        self.fill(0, 0, 1, H - 1)
        self.fill(width - 2, 0, width - 1, H - 1)
        self.fill(0, ceil, width - 1, H - 1)

    # --- карта ---
    def put(self, x, y, ch):
        assert 0 <= x < self.w and 0 <= y < H, (x, y, ch)
        self.g[y][x] = ch

    def fill(self, x0, y0, x1, y1, ch="#"):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.put(x, y, ch)

    def at(self, x, y):
        if x < 0 or x >= self.w:
            return "#"
        if y >= H:
            return "#"
        if y < 0:
            return "."
        return self.g[y][x]

    def solid(self, x, y):
        return self.at(x, y) == "#"

    def seg(self, x0, x1, top, link=None):
        """Звено основного маршрута: земля x0..x1, верхняя грань на высоте `top`."""
        self.fill(x0, 0, x1, top - 1)
        if self.prev and link is None:
            px1, ptop = self.prev
            gap = x0 - px1 - 1
            rise = top - ptop
            assert gap <= self.gap_limit, "L%d: gap %d before x=%d too wide" % (self.index + 1, gap, x0)
            assert rise <= self.rise_limit, "L%d: rise %d at x=%d too high" % (self.index + 1, rise, x0)
        self.prev = (x1, top)

    def block(self, x0, y0, x1, y1):
        self.fill(x0, y0, x1, y1)

    def cloud(self, x0, x1, y):
        self.fill(x0, y, x1, y, "=")

    def spikes(self, x0, x1, y, ch="^"):
        self.fill(x0, y, x1, y, ch)

    # --- объекты ---
    def e(self, kind, x, y, text=None, **kw):
        s = "%s %.2f %.2f" % (kind, x, y)
        for k, v in kw.items():
            s += " %s=%s" % (k, ("%g" % v) if isinstance(v, (int, float)) else v)
        if text:
            s += " | " + text
        self.ents.append(s)

    def player(self, x, y):
        self.e("player", x + 0.5, y)

    def ground_ent(self, kind, x, y, **kw):
        assert self.supported(x, y), "L%d: %s at (%d,%d) floats" % (self.index + 1, kind, x, y)
        assert not self.solid(x, y), "L%d: %s at (%d,%d) is inside a wall" % (self.index + 1, kind, x, y)
        self.e(kind, x + 0.5, y, **kw)

    def supported(self, x, y):
        return self.at(x, y - 1) in "#="

    def crawler(self, x, y, **kw): self.ground_ent("crawler", x, y, **kw)
    def brute(self, x, y, **kw): self.ground_ent("brute", x, y, **kw)
    def spitter(self, x, y, **kw): self.ground_ent("spitter", x, y, **kw)
    def checkpoint(self, x, y): self.ground_ent("checkpoint", x, y)
    def spring(self, x, y, power=20): self.ground_ent("spring", x, y, power=power)
    def exit(self, x, y): self.ground_ent("exit", x, y)

    def gate(self, x, y, need):
        assert self.supported(x, y)
        self.e("gate", x + 0.5, y, need=need)

    def bat(self, x, y, **kw):
        self.e("bat", x + 0.5, y + 0.5, **kw)

    def firefly(self, x, y):
        assert not self.solid(int(x), int(y)), "L%d: firefly in a wall at (%s,%s)" % (self.index + 1, x, y)
        self.e("firefly", x + 0.5, y + 0.5)
        self.ff += 1

    def ffs(self, x0, x1, y, n=None):
        n = n or (x1 - x0 + 1)
        for i in range(n):
            self.firefly(x0 + (x1 - x0) * i / max(1, n - 1), y)

    def arc(self, x0, x1, y0, h, n):
        """Светлячки дугой (над ямой)."""
        for i in range(n):
            t = i / max(1, n - 1)
            self.firefly(x0 + (x1 - x0) * t, y0 + h * 4 * t * (1 - t))

    def golden(self, x, y):
        self.e("golden", x + 0.5, y + 0.5)

    def mplat(self, x, y, dx=0, dy=0, speed=1.0, phase=0.0):
        """Движущаяся платформа: x — середина, y — верхняя грань."""
        self.e("mplat", x, y, dx=dx, dy=dy, speed=speed, phase=phase)

    def cplat(self, x, y):
        self.e("cplat", x, y)

    def hint(self, x, y, text, w=3, h=4):
        self.e("hint", x + 0.5, y + 0.5, text=text, w=w, h=h)

    # --- вывод ---
    def tile_rows(self):
        """Номера тайлов (сверху вниз) для слоёв Tiles и Deco."""
        solid = self.solid
        tiles = [[0] * self.w for _ in range(H)]
        deco = [[0] * self.w for _ in range(H)]
        occupied = set()
        for s in self.ents:
            parts = s.split()
            occupied.add((int(float(parts[1])), int(float(parts[2]))))
        for y in range(H):
            for x in range(self.w):
                ch = self.g[y][x]
                row = H - 1 - y
                if ch == "#":
                    mask = 0
                    if not solid(x, y + 1) and y + 1 < H: mask |= 1
                    if not solid(x + 1, y) and x + 1 < self.w: mask |= 2
                    if not solid(x, y - 1) and y - 1 >= 0: mask |= 4
                    if not solid(x - 1, y) and x - 1 >= 0: mask |= 8
                    if mask == 0:
                        tiles[row][x] = self.rng.choice([1, 1, 1, 22, 23])
                    else:
                        tiles[row][x] = 1 + mask
                elif ch == "=":
                    tiles[row][x] = 17
                elif ch == "^":
                    tiles[row][x] = 18
                elif ch == "v":
                    tiles[row][x] = 19
                elif ch == ">":
                    tiles[row][x] = 20
                elif ch == "<":
                    tiles[row][x] = 21
        # декор: на земле и под потолком
        weights = self.theme["deco"]
        for y in range(1, H - 1):
            for x in range(2, self.w - 2):
                if self.g[y][x] != "." or (x, y) in occupied:
                    continue
                below, above = self.g[y - 1][x], self.g[y + 1][x]
                if below == "#" and self.rng.random() < self.theme["deco_ground"]:
                    deco[H - 1 - y][x] = self.rng.choices([24, 25, 26, 28, 31, 32], weights)[0]
                elif above == "#" and self.rng.random() < self.theme["deco_ceiling"]:
                    deco[H - 1 - y][x] = self.rng.choice([27, 27, 27, 29])
        return tiles, deco

    def keep_ceiling(self, x0, x1):
        self.keep.append((x0, x1))

    def dress(self):
        """Оформление пещеры: потолок опускается над пустыми участками и колышется."""
        used = [0] * self.w
        for y in range(H):
            for x in range(self.w):
                if self.g[y][x] != "." and y < self.base_ceil:
                    used[x] = max(used[x], y)
        for s in self.ents:
            p = s.split()
            x, y = int(float(p[1])), int(float(p[2]))
            extra = 0
            for kv in p[3:]:
                if kv.startswith("amp="):
                    extra = int(float(kv[4:])) + 1
                if kv.startswith("dy="):
                    extra = max(extra, int(float(kv[3:])))
            top = y + extra + (2 if p[0] in ("exit", "gate", "boss") else 0)
            for dx in range(-1, 2):
                if 0 <= x + dx < self.w:
                    used[x + dx] = max(used[x + dx], top)
        # плавная огибающая: макс. по окну, затем низкочастотный шум
        win = 9
        env = [max(used[max(0, x - win): x + win + 1]) for x in range(self.w)]
        ph = self.rng.random() * 6.28
        ceil = []
        for x in range(self.w):
            wob = 2.2 * math.sin(x / 7.0 + ph) + 1.6 * math.sin(x / 3.1 + ph * 2) + self.rng.random() * 0.8
            c = int(round(env[x] + 9 + wob))
            ceil.append(max(12, min(self.base_ceil, c)))
        for _ in range(6):  # ступеньки не круче трёх клеток
            for x in range(1, self.w):
                ceil[x] = min(ceil[x], ceil[x - 1] + 3)
            for x in range(self.w - 2, -1, -1):
                ceil[x] = min(ceil[x], ceil[x + 1] + 3)
        for x in range(2, self.w - 2):
            if any(a <= x <= b for a, b in self.keep):
                continue
            for y in range(ceil[x], self.base_ceil):
                if self.g[y][x] == ".":
                    self.g[y][x] = "#"

    # --- объекты сцены: экземпляры префабов ---
    @staticmethod
    def parse_ent(line):
        head, _, text = line.partition("|")
        toks = head.split()
        kw = dict(t.split("=", 1) for t in toks[3:])
        return toks[0], float(toks[1]), float(toks[2]), {k: float(v) for k, v in kw.items()}, text.strip()

    def object_nodes(self):
        def f(v):
            return "Float(%s)" % repr(float(v))

        def ov(comp, field, value):
            return '(component: "%s", field: "%s", value: %s)' % (comp, field, value)

        groups = {"Player": [], "Enemies": [], "Fireflies": [], "Mechanisms": [], "Checkpoints": [], "Exit": []}
        for line in self.ents:
            kind, x, y, kw, text = self.parse_ent(line)
            if kind == "hint":
                continue
            o = []
            group, prefab, name = "Mechanisms", None, kind.capitalize()
            if kind == "player":
                group, prefab, name = "Player", "player", "Player"
            elif kind in ("crawler", "brute"):
                group, prefab, name = "Enemies", kind, kind.capitalize()
                comp = kind.capitalize()
                for k in ("dir", "speed", "range"):
                    if k in kw:
                        o.append(ov(comp, k, f(kw[k])))
            elif kind == "bat":
                group, prefab, name = "Enemies", "bat", "Bat"
                for k, field in (("range", "range"), ("amp", "amp"), ("speed", "speed"), ("phase", "t")):
                    if k in kw:
                        o.append(ov("Bat", field, f(kw[k])))
            elif kind == "spitter":
                group, prefab, name = "Enemies", "spitter", "Spitter"
                if "period" in kw:
                    o.append(ov("Spitter", "period", f(kw["period"])))
                if "delay" in kw:
                    o.append(ov("Spitter", "timer", f(kw["delay"])))
            elif kind == "boss":
                group, prefab, name = "Enemies", "boss", "Boss"
                o.append(ov("Boss", "arena_x", "Vec2(%r, %r)" % (kw["x0"], kw["x1"])))
                o.append(ov("Boss", "floor_y", f(kw["floor"])))
                o.append(ov("Boss", "ceiling_y", f(kw["ceil"])))
                o.append(ov("Boss", "door", "Vec3(%r, %r, 0.0)" % (kw["doorx"], kw["doory0"])))
                o.append(ov("Boss", "door_y1", f(kw["doory1"])))
                # Ссылка на тайлмап уровня (узел Tiles, id TILES_ID): дверь закладывается в нём.
                o.append(ov("Boss", "tiles", "Object(%d)" % TILES_ID))
            elif kind in ("firefly", "golden"):
                group = "Fireflies"
                prefab = "golden_firefly" if kind == "golden" else "firefly"
                name = "GoldenFirefly" if kind == "golden" else "Firefly"
            elif kind == "checkpoint":
                group, prefab, name = "Checkpoints", "checkpoint", "Checkpoint"
            elif kind == "spring":
                prefab, name = "spring", "Spring"
                if "power" in kw:
                    o.append(ov("Spring", "power", f(kw["power"])))
            elif kind == "mplat":
                prefab, name = "moving_platform", "MovingPlatform"
                o.append(ov("MovingPlatform", "from", "Vec2(%r, %r)" % (x, y)))
                o.append(ov("MovingPlatform", "to", "Vec2(%r, %r)" % (x + kw.get("dx", 0.0), y + kw.get("dy", 0.0))))
                for k, field in (("speed", "speed"), ("phase", "t")):
                    if k in kw:
                        o.append(ov("MovingPlatform", field, f(kw[k])))
            elif kind == "cplat":
                prefab, name = "crumble_platform", "CrumblePlatform"
            elif kind == "gate":
                prefab, name = "gate", "Gate"
                o.append(ov("Gate", "need", "Int(%d)" % int(kw["need"])))
            elif kind == "exit":
                group, prefab, name = "Exit", "exit", "Exit"
            else:
                raise ValueError("unknown object kind " + kind)
            node = '            (name: "%s", prefab: Some("prefabs/%s.prefab"), transform: (position: (%r, %r, 0.0))' % (name, prefab, x, y)
            if o:
                node += ", overrides: [%s]" % ", ".join(o)
            node += "),"
            groups[group].append(node)
        out = []
        for g in ("Player", "Enemies", "Fireflies", "Checkpoints", "Mechanisms", "Exit"):
            if not groups[g]:
                continue
            if g in ("Player", "Exit"):
                # одиночные объекты — прямо в корне сцены
                out.extend(n.replace("            (", "        (", 1) for n in groups[g])
            else:
                out.append('        (name: "%s", children: [\n%s\n        ]),' % (g, "\n".join(groups[g])))
        return out

    # --- сценарий уровня: заставка и подсказки ---
    def write_scenario(self):
        hints = []
        for line in self.ents:
            kind, x, y, kw, text = self.parse_ent(line)
            if kind == "hint":
                hints.append((x, y, kw.get("w", 2.0), kw.get("h", 2.0), text))
        hints.sort()
        lines = [
            "-- Уровень %d «%s»: заставка с названием и подсказки." % (self.index + 1, self.title),
            "-- Сгенерировано tools/gen_levels.py (hints у уровня); править можно и руками — сценарий",
            "-- перезагружается сам при сохранении.",
            "wait_frame(2)   -- менеджер уровня успел прочитать таблицу data/levels.ron",
            'ui.text("CardNumber", "Уровень " .. (self.get("LevelManager.index") + 1))',
            'ui.text("CardTitle", self.get("LevelManager.title"))',
            'ui.text("CardSub", self.get("LevelManager.subtitle"))',
            'ui.show("Card")',
            "",
            "-- Подсказка: зона вокруг точки (x, y) шириной w и высотой h; срабатывает один раз.",
            "wait_until(() => find_tag(\"Player\") != nil)",
            "const player = find_tag(\"Player\")",
            "function hint(x, y, w, h, text) {",
            "    start(() => {",
            "        wait_until(() => abs(player.world_position.x - x) < w / 2 + 0.3 and abs(player.world_position.y + 0.5 - y) < h / 2 + 0.5)",
            "        self.send(\"hint\", text)",
            "    })",
            "}",
        ]
        for x, y, w, h, text in hints:
            lines.append('hint(%r, %r, %r, %r, "%s")' % (x, y, w, h, text.replace('"', "'")))
        lines += [
            "",
            "wait(2.4)",
            'find("Card").tween("CanvasGroup.alpha", 0, 0.8)',
            "wait(0.8)",
            'ui.hide("Card")',
            "",
        ]
        d = os.path.join(HERE, "..", "assets", "scenarios")
        os.makedirs(d, exist_ok=True)
        name = "level%d.scenario" % (self.index + 1)
        with open(os.path.join(d, name), "w", encoding="utf-8", newline="\n") as fh:
            fh.write("\n".join(lines))
        with open(os.path.join(d, name + ".meta"), "w", newline="\n") as fh:
            fh.write('(\n    guid: "%s",\n    importer: "Scenario",\n)' % guid_for("scenarios/" + name))

    def write(self):
        self.dress()
        tiles, deco = self.tile_rows()
        th = self.theme

        def rows(t):
            out = []
            for r in t:
                toks, i = [], 0
                while i < len(r):
                    j = i
                    while j < len(r) and r[j] == r[i]:
                        j += 1
                    v = "." if r[i] == 0 else str(r[i])
                    toks.append("%d*%s" % (j - i, v) if j - i > 1 else v)
                    i = j
                out.append('            Str("%s"),' % " ".join(toks))
            return "\n".join(out)

        tileset = guid_for("sprites/tiles.png")
        start = [s for s in self.ents if s.startswith("player")][0].split()
        cx, cy = float(start[1]), float(start[2]) + 2.0
        parts = []
        parts.append("""        (
            name: "Main Camera",
            tag: "MainCamera",
            transform: (position: (%.2f, %.2f, 0.0)),
            components: [
                (kind: "Camera", fields: { "ortho_size": Float(7.0), "clear_color": %s }),
                (kind: "CameraController2D", fields: {
                    "target_tag": Str("Player"), "offset": Vec2(0.0, 1.6), "smoothing": Float(7.0),
                    "use_bounds": Bool(true), "shake_max_offset": Vec2(0.5, 0.4),
                }),
                (kind: "Environment", fields: {
                    "ambient": Float(1.0), "fog_density": Float(0.0), "exposure": Float(%.2f),
                    "bloom": Float(%.2f), "vignette": Float(0.32),
                }),
            ],
        ),""" % (cx, cy, th["clear"], th["exposure"], th["bloom"]))
        for layer, name, factor, ylayer, tint, sl in (
            (0, "bg_far", 0.06, 0.08, th["bg_far"], -10),
            (1, "bg_mid", 0.22, 0.16, th["bg_mid"], -9),
            (2, "bg_near", 0.45, 0.22, th["bg_near"], -8),
        ):
            for slot in range(3):
                parts.append("""        (
            name: "%s_%d",
            components: [
                (kind: "SpriteRenderer", fields: {
                    "texture": Asset("%s"), "size": Vec2(32.0, 18.0), "sorting_layer": Int(%d), "color": %s,
                }),
                (kind: "Parallax", fields: {
                    "factor": Float(%.2f), "period": Float(32.0), "slot": Int(%d), "y_follow": Float(%.2f), "mid_y": Float(%.1f),
                }),
            ],
        ),""" % (name, slot, guid_for("sprites/%s.png" % name), sl, tint, factor, slot, ylayer, H / 2))
        for name, t, layer, tint, uid in (("Deco", deco, -2, th["deco_tint"], DECO_ID), ("Tiles", tiles, 0, th["tiles"], TILES_ID)):
            parts.append("""        (
            name: "%s",
            id: %d,
            components: [
                (kind: "Tilemap", fields: {
                    "tileset": Asset("%s"), "tileset_columns": Int(8), "tileset_rows": Int(4),
                    "tile_size": Vec2(1.0, 1.0), "sorting_layer": Int(%d), "color": %s,
                    "rows": List([
%s
                    ]),
                }),
            ],
        ),""" % (name, uid, tileset, layer, tint, rows(t)))
        parts.extend(self.object_nodes())
        parts.append("""        (
            name: "Level",
            components: [
                (kind: "LevelManager", fields: { "index": Int(%d), "tiles": Object(%d) }),
                (kind: "ScenarioRunner", fields: { "path": Str("scenarios/level%d.scenario") }),
            ],
        ),""" % (self.index, TILES_ID, self.index + 1))
        for name, prefab in (("Hud", "ui/hud.prefab"), ("PauseOverlay", "ui/pause.prefab"), ("ResultsOverlay", "ui/results.prefab")):
            parts.append('        (name: "%s", prefab: Some("%s")),' % (name, prefab))
        self.write_scenario()
        INFO[self.index] = self
        text = "// Уровень %d: %s. Сгенерировано tools/gen_levels.py — правьте генератор, а не этот файл.\n(\n    name: \"Level%d\",\n    objects: [\n%s\n    ],\n)\n" % (
            self.index + 1, self.title, self.index + 1, "\n".join(parts))
        os.makedirs(SCENES, exist_ok=True)
        path = os.path.join(SCENES, "level%d.scene" % (self.index + 1))
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        meta = os.path.join(SCENES, "level%d.scene.meta" % (self.index + 1))
        with open(meta, "w", newline="\n") as f:
            f.write('(\n    guid: "%s",\n    importer: "Scene",\n)' % guid_for("scenes/level%d.scene" % (self.index + 1)))
        # эскиз
        os.makedirs(PREVIEW, exist_ok=True)
        grid = [list(r) for r in self.g]
        for s in self.ents:
            p = s.split()
            if p[0] in ("hint",):
                continue
            x, y = int(float(p[1])), int(float(p[2]))
            sym = {"player": "P", "firefly": "o", "golden": "G", "crawler": "c", "brute": "B", "bat": "b", "spitter": "s",
                   "checkpoint": "C", "spring": "S", "gate": "|", "exit": "E", "boss": "@", "mplat": "m", "cplat": "x"}.get(p[0], "?")
            if 0 <= y < H and 0 <= x < self.w:
                grid[y][x] = sym
        with open(os.path.join(PREVIEW, "level%d.txt" % (self.index + 1)), "w", encoding="utf-8") as f:
            for y in range(H - 1, -1, -1):
                f.write("".join(grid[y]) + "\n")
        total = self.ff
        print("level %d: %dx%d, %d fireflies (need %d), %d objects" % (self.index + 1, self.w, H, total, self.need, len(self.ents)))
        assert total >= self.need + 2, "not enough fireflies"


def theme(clear, tiles, far, mid, near, deco_tint, exposure=1.0, bloom=0.35, deco=(2, 1, 2, 1, 1, 1), dg=0.22, dc=0.2):
    return {"clear": clear, "tiles": tiles, "bg_far": far, "bg_mid": mid, "bg_near": near, "deco_tint": deco_tint,
            "exposure": exposure, "bloom": bloom, "deco": deco, "deco_ground": dg, "deco_ceiling": dc}


# ---------------------------------------------------------------- уровень 1
def level1():
    set_boost(0)
    L = Level(0, 128, "Тихая пещера", "Первые огоньки", need=12, par=150, music="music_cave", gap=4, rise=2, ceil=21,
              theme=theme(col(0.03, 0.05, 0.12), col(0.8, 1.0, 1.1), col(0.7, 0.95, 1.2), col(0.8, 1.0, 1.15),
                          col(0.8, 1.0, 1.15, 0.7), col(0.9, 1.0, 1.0)))
    L.seg(2, 16, 5)
    L.player(4, 5)
    L.hint(6, 5, "A / D или стрелки — бег, Пробел — прыжок. Держите прыжок — выше.", w=4)
    L.ffs(9, 13, 6, 5)
    L.seg(17, 27, 6)                       # ступенька в 1 клетку
    L.ffs(19, 25, 7, 4)
    L.seg(32, 52, 5)                       # первая яма (4 клетки)
    L.arc(28, 31, 7, 2.0, 4)
    L.hint(33, 5, "Прыгайте на врагов сверху!", w=4)
    L.crawler(40, 5)
    L.ffs(35, 38, 6, 3)
    L.firefly(42, 8)
    L.cloud(51, 52, 5)
    L.seg(53, 60, 7, link="cloud")         # подъём в 2 клетки через «облачко»
    L.spikes(56, 57, 7)
    L.hint(54, 7, "Шипы колются: перепрыгните их.", w=3)
    L.firefly(54, 8); L.firefly(58.5, 10); L.firefly(60, 8)
    L.seg(65, 84, 5)                       # яма в 4 клетки и спуск
    L.arc(61, 64, 9, 1.5, 3)
    L.checkpoint(67, 5)
    L.crawler(75, 5)
    L.bat(81, 9, range=3, amp=1.0)
    L.ffs(70, 73, 6, 3)
    # лесенка к золотому светлячку
    L.cloud(70, 72, 6); L.cloud(74, 76, 8); L.cloud(70, 72, 10); L.cloud(66, 68, 12)
    L.golden(67, 13)
    L.firefly(75, 10); L.firefly(71, 12)
    L.ffs(78, 83, 6, 3)
    L.seg(89, 104, 5)                      # яма 4
    L.arc(85, 88, 8, 1.5, 3)
    L.spring(92, 5, power=17)
    L.block(95, 5, 100, 7)                 # уступ над пружиной
    L.ffs(95, 100, 8, 4)
    L.bat(100, 10, range=3, amp=1.2, phase=1.5)
    L.crawler(90, 5, dir=1, range=1)
    L.seg(109, 125, 5)
    L.arc(105, 108, 7, 1.5, 3)
    L.hint(110, 5, "Соберите светлячков — арка выхода загорится.", w=3)
    L.ffs(112, 118, 6, 4)
    L.exit(121, 5)
    L.write()


# ---------------------------------------------------------------- уровень 2
def level2():
    set_boost(1)
    L = Level(1, 136, "Грибной грот", "Пружины, облачка и хрупкие мосты", need=14, par=190, music="music_cave", gap=4, rise=2, ceil=22,
              theme=theme(col(0.07, 0.03, 0.12), col(1.15, 0.85, 1.15), col(1.1, 0.8, 1.25), col(1.15, 0.85, 1.2),
                          col(1.15, 0.85, 1.2, 0.7), col(1.1, 0.9, 1.1), deco=(1, 1, 4, 1, 3, 1), dg=0.3))
    L.seg(2, 20, 5)
    L.player(4, 5)
    L.hint(6, 5, "Пружины подбрасывают высоко. Облачка пропускают снизу; S / ↓ + прыжок — вниз.", w=4)
    L.ffs(9, 18, 6, 5)
    L.seg(24, 40, 6)
    L.arc(21, 23, 7, 1.5, 3)
    L.crawler(30, 6)
    L.crawler(36, 6, dir=1)
    L.spikes(33, 34, 6)
    L.ffs(26, 29, 7, 3)
    L.spring(39, 6, power=19)
    L.cloud(41, 43, 9)
    L.seg(44, 57, 12, link="spring")
    L.firefly(41, 12); L.firefly(42.5, 11)
    L.ffs(46, 55, 13, 4)
    L.bat(50, 16, range=3, amp=1.0)
    L.cloud(53, 55, 13); L.cloud(48, 50, 15)
    L.golden(49, 17)
    L.seg(62, 73, 8)
    L.arc(58, 61, 12, 1.0, 3)
    L.checkpoint(63, 8)
    L.spitter(70, 8)
    L.ffs(65, 68, 9, 3)
    # хрупкий мост над ямой x=74..88
    for cx in (76, 81.5, 87.0):
        L.cplat(cx, 8)
        L.firefly(cx - 0.5, 10);
    L.seg(90, 105, 8, link="crumble")
    L.bat(96, 12, range=4, amp=1.2)
    L.crawler(100, 8)
    L.ffs(92, 98, 9, 4)
    L.mplat(107, 8, dx=4, speed=0.9)       # платформа через яму x=106..113
    L.firefly(110, 10)
    L.seg(114, 133, 8, link="mplat")
    L.checkpoint(115, 8)
    L.spikes(118, 119, 8)
    L.ffs(116, 122, 9, 3)
    L.gate(123, 8, need=10)
    L.hint(120, 8, "Ворота откроются, когда соберёте нужное число светлячков.", w=3)
    L.ffs(125, 130, 9, 3)
    L.exit(129, 8)
    L.write()


# ---------------------------------------------------------------- уровень 3
def level3():
    set_boost(2)
    L = Level(2, 150, "Кристальные залы", "Двойной прыжок", need=16, par=240, music="music_deep", gap=7, rise=4,
              theme=theme(col(0.02, 0.07, 0.13), col(0.85, 1.1, 1.25), col(0.7, 1.05, 1.35), col(0.8, 1.1, 1.3),
                          col(0.8, 1.1, 1.3, 0.7), col(0.9, 1.1, 1.3), exposure=1.0, deco=(1, 5, 1, 2, 2, 1), dg=0.28))
    L.seg(2, 18, 5)
    L.player(4, 5)
    L.hint(6, 5, "Двойной прыжок: нажмите прыжок ещё раз в воздухе.", w=4)
    L.ffs(9, 16, 6, 4)
    L.seg(19, 34, 9)                       # стена в 4 клетки: нужен двойной прыжок
    L.firefly(18, 8); L.firefly(18, 10)
    L.checkpoint(22, 9)
    L.spitter(31, 9)
    L.ffs(24, 29, 10, 3)
    L.seg(42, 52, 9)                       # яма 7
    L.arc(35, 41, 11, 3.0, 5)
    L.crawler(46, 9); L.crawler(50, 9, dir=1)
    L.bat(47, 13, range=3, amp=1.0)
    # подъём «облачками» (по 3 клетки)
    L.cloud(54, 56, 11); L.cloud(58, 60, 14); L.cloud(58, 60, 17)
    L.firefly(55, 13); L.firefly(59, 16); L.firefly(59, 19)
    L.seg(62, 78, 19, link="clouds")
    L.checkpoint(63, 19)
    L.ffs(64, 70, 20, 3)
    L.gate(72, 19, need=8)
    L.spitter(76, 19)
    L.mplat(80.5, 17, dx=4, speed=0.9)     # яма 79..86
    L.seg(87, 104, 14, link="mplat")
    L.firefly(82, 19)
    L.brute(96, 14, range=5)
    L.ffs(89, 94, 15, 3); L.ffs(98, 103, 15, 3)
    L.spikes(91, 92, 14)
    # золотой светлячок высоко над уступом
    L.cloud(99, 101, 17); L.cloud(102, 104, 20); L.cloud(98, 100, 23)
    L.golden(99, 24)
    L.seg(109, 128, 14)
    L.arc(105, 108, 16, 2.0, 3)
    L.checkpoint(110, 14)
    L.crawler(114, 14)
    L.bat(116, 18, range=4, amp=1.5)
    L.spitter(120, 14)
    L.ffs(112, 118, 15, 3)
    L.spikes(123, 125, 14)
    L.firefly(124, 18)
    L.seg(133, 147, 14)                    # яма 4
    L.arc(129, 132, 17, 1.5, 3)
    L.ffs(134, 136, 15, 3)
    L.gate(138, 14, need=14)
    L.exit(143, 14)
    L.write()


# ---------------------------------------------------------------- уровень 4
def level4():
    set_boost(3)
    L = Level(3, 150, "Ржавые шахты", "Платформы, шипы и тяжеляки", need=18, par=280, music="music_deep", gap=7, rise=4,
              theme=theme(col(0.09, 0.05, 0.03), col(1.3, 0.95, 0.75), col(1.4, 0.9, 0.7), col(1.35, 0.95, 0.75),
                          col(1.35, 0.95, 0.75, 0.7), col(1.25, 1.0, 0.8), exposure=1.0, deco=(1, 1, 2, 2, 1, 4), dg=0.18))
    L.seg(2, 16, 6)
    L.player(4, 6)
    L.ffs(8, 14, 7, 4)
    L.mplat(19.5, 6, dx=7, speed=0.8)      # яма 17..27, платформа идёт через
    L.spikes(18, 27, 0)
    L.ffs(20, 26, 9, 4)
    L.seg(28, 46, 6, link="mplat")
    L.checkpoint(29, 6)
    L.brute(36, 6, range=5)
    L.crawler(42, 6)
    L.ffs(31, 34, 7, 3); L.ffs(39, 44, 7, 3)
    L.spitter(45, 6)
    L.cplat(50, 6)                         # яма 47..52 с хрупкой платформой
    L.firefly(50, 8)
    L.seg(53, 62, 6, link="crumble")
    L.bat(58, 10, range=3, amp=1.2)
    L.ffs(55, 60, 7, 3)
    L.cloud(63, 65, 7)
    L.seg(66, 80, 10)
    L.checkpoint(67, 10)
    L.spitter(70, 10, delay=0.5); L.spitter(78, 10, delay=1.7)
    L.spikes(73, 74, 10)
    L.ffs(68, 79, 11, 5)
    L.mplat(83, 10, dy=8, speed=0.7)       # вертикальная платформа, яма 81..85
    L.seg(86, 101, 18, link="mplat")
    L.crawler(90, 18); L.crawler(97, 18, dir=1)
    L.bat(94, 22, range=4, amp=1.2)
    L.ffs(88, 99, 19, 5)
    L.cloud(92, 94, 20); L.cloud(95, 97, 22)
    L.golden(96, 23)
    L.seg(102, 118, 8)                     # спуск
    L.ffs(104, 108, 12, 3)
    L.spikes(110, 111, 8)
    L.brute(114, 8, range=3)
    L.checkpoint(103, 8)
    L.firefly(113, 11)
    L.cplat(123, 8); L.cplat(128.5, 8)    # яма 119..131
    L.firefly(123, 10); L.firefly(128.5, 10)
    L.seg(132, 147, 8, link="crumble")
    L.checkpoint(133, 8)
    L.spitter(136, 8)
    L.ffs(134, 138, 9, 3)
    L.gate(140, 8, need=14)
    L.ffs(141, 143, 9, 3)
    L.exit(144, 8)
    L.write()


# ---------------------------------------------------------------- уровень 5
def level5():
    set_boost(4)
    L = Level(4, 150, "Ледяной провал", "Рывок сквозь тьму", need=20, par=300, music="music_deep", gap=8, rise=4,
              theme=theme(col(0.04, 0.08, 0.15), col(1.0, 1.2, 1.4), col(0.85, 1.2, 1.5), col(0.95, 1.25, 1.5),
                          col(0.95, 1.25, 1.5, 0.7), col(1.0, 1.2, 1.45), exposure=1.05, bloom=0.4, deco=(2, 5, 0, 2, 1, 0), dg=0.22))
    L.seg(2, 14, 6)
    L.player(4, 6)
    L.hint(6, 6, "Рывок: Shift / X / RB. Он проносит сквозь врагов и снаряды.", w=4)
    L.ffs(8, 13, 7, 4)
    L.seg(23, 34, 6)                       # яма 8: прыжок + двойной + рывок
    L.arc(15, 22, 8, 3.0, 5)
    L.spikes(15, 22, 0)
    L.checkpoint(24, 6)
    # туннель высотой 2 клетки с тяжеляком: пробежать можно только рывком
    L.seg(35, 60, 6)
    L.block(37, 8, 59, 25)                 # потолок туннеля (клетки y=6,7 свободны)
    L.brute(48, 6, range=9)
    L.ffs(38, 44, 7, 4); L.ffs(52, 58, 7, 4)
    L.hint(32, 6, "Тяжеляка не раздавить и не перепрыгнуть в узком туннеле — используйте рывок.", w=3)
    L.seg(61, 72, 6)
    L.crawler(66, 6)
    L.ffs(62, 70, 7, 3)
    L.seg(81, 96, 8)                       # яма 8
    L.arc(73, 80, 10, 3.0, 5)
    L.spikes(73, 80, 0)
    L.checkpoint(82, 8)
    L.spitter(90, 8, delay=0.6); L.spitter(95, 8, delay=1.8)
    L.crawler(86, 8)
    L.ffs(84, 94, 9, 5)
    # золото: «облачная» лестница вверх
    L.cloud(86, 88, 10); L.cloud(90, 92, 13); L.cloud(94, 96, 16)
    L.golden(95, 17)
    # ледяная лестница: хрупкие платформы вверх
    L.cplat(101, 9); L.cplat(107, 11); L.cplat(113, 13); L.cplat(119, 15)
    L.firefly(101, 11); L.firefly(107, 13); L.firefly(113, 15); L.firefly(119, 17)
    L.bat(104, 14, range=3, amp=1.0); L.bat(116, 18, range=3, amp=1.0, phase=1.0)
    L.seg(123, 146, 16, link="crumble")
    L.checkpoint(124, 16)
    L.spitter(131, 16)
    L.brute(135, 16, range=3)
    L.ffs(126, 133, 17, 4)
    L.gate(139, 16, need=16)
    L.exit(143, 16)
    L.write()


# ---------------------------------------------------------------- уровень 6
def level6():
    set_boost(5)
    L = Level(5, 160, "Сердце тьмы", "Хозяин пещер", need=18, par=360, music="music_deep", gap=8, rise=4,
              theme=theme(col(0.08, 0.02, 0.09), col(1.25, 0.8, 1.05), col(1.4, 0.7, 1.0), col(1.35, 0.75, 1.05),
                          col(1.35, 0.75, 1.05, 0.7), col(1.25, 0.85, 1.1), exposure=1.0, bloom=0.4, deco=(1, 3, 2, 2, 1, 3), dg=0.2))
    L.seg(2, 12, 6)
    L.player(4, 6)
    L.ffs(7, 11, 7, 3)
    L.seg(19, 30, 6)                       # яма 6: двойной прыжок
    L.arc(13, 18, 8, 2.5, 4)
    L.spitter(28, 6)
    L.checkpoint(20, 6)
    L.ffs(22, 27, 7, 3)
    L.mplat(33, 6, dx=5, speed=0.9)        # яма 31..40
    L.spikes(31, 40, 0)
    L.firefly(36, 9)
    L.seg(41, 52, 6, link="mplat")
    L.crawler(45, 6); L.brute(49, 6, range=3)
    L.ffs(43, 51, 7, 3)
    L.cloud(53, 55, 7); L.cloud(57, 59, 9)
    L.seg(60, 74, 12, link="clouds")
    L.checkpoint(61, 12)
    L.spitter(66, 12, delay=0.4); L.spitter(72, 12, delay=1.9)
    L.bat(69, 16, range=4, amp=1.2)
    L.ffs(62, 73, 13, 5)
    L.cplat(79, 12); L.cplat(85, 14); L.cplat(91, 12)   # хрупкая лестница, яма 75..95
    L.firefly(79, 14); L.firefly(85, 16); L.firefly(91, 14)
    L.seg(96, 108, 12, link="crumble")
    L.checkpoint(97, 12)
    # туннель с тяжеляком (нужен рывок)
    L.block(99, 14, 107, 25)
    L.brute(103, 12, range=3)
    L.ffs(100, 106, 13, 4)
    L.cloud(96, 98, 14); L.cloud(96, 98, 17)
    L.golden(97, 18)
    # к арене
    L.seg(112, 124, 6)                     # спуск, яма 3
    L.arc(109, 111, 14, 1.0, 3)
    L.checkpoint(113, 6)
    L.ffs(115, 120, 7, 4)
    L.gate(122, 6, need=14)
    # --- арена: пол y=6, x=125..157, потолок y>=22, дверь x=125 (клетки 6..9) ---
    L.fill(125, 0, 157, 5)
    L.fill(125, 10, 125, 21)
    L.fill(125, 22, 157, H - 1)
    L.cloud(134, 138, 8); L.cloud(146, 150, 8)
    L.cloud(140, 144, 11)
    L.firefly(136, 10); L.firefly(148, 10); L.firefly(142, 13)
    L.e("boss", 142.0, 10.0, x0=126, x1=156, floor=6, ceil=22, doorx=125, doory0=6, doory1=9)
    L.exit(155, 6)
    L.keep_ceiling(125, 157)
    L.write()


# ---------------------------------------------------------------- меню
def menu_scene():
    th = col(0.03, 0.05, 0.12)
    parts = ["""        (
            name: "Main Camera",
            tag: "MainCamera",
            transform: (position: (0.0, 0.0, 0.0)),
            components: [
                (kind: "Camera", fields: { "ortho_size": Float(7.0), "clear_color": %s }),
                (kind: "Environment", fields: { "ambient": Float(1.0), "fog_density": Float(0.0), "bloom": Float(0.4), "vignette": Float(0.4) }),
            ],
        ),""" % th]
    for name, factor, tint, sl, drift in (("bg_far", 0.0, col(0.8, 1.0, 1.2), -10, 0.4), ("bg_mid", 0.0, col(0.9, 1.1, 1.3), -9, 1.2), ("bg_near", 0.0, col(0.9, 1.1, 1.3, 0.7), -8, 2.4)):
        for slot in range(3):
            parts.append("""        (
            name: "%s_%d",
            components: [
                (kind: "SpriteRenderer", fields: { "texture": Asset("%s"), "size": Vec2(32.0, 18.0), "sorting_layer": Int(%d), "color": %s }),
                (kind: "Parallax", fields: { "factor": Float(0.0), "period": Float(32.0), "slot": Int(%d), "y_follow": Float(0.0), "mid_y": Float(0.0), "drift": Float(%.2f) }),
            ],
        ),""" % (name, slot, guid_for("sprites/%s.png" % name), sl, tint, slot, drift))
    flies = []
    for k in range(16):
        x = -11.0 + (k * 7.31) % 22.0
        y = -5.0 + (k * 3.77) % 10.0
        flies.append('            (name: "Firefly", prefab: Some("prefabs/firefly.prefab"), transform: (position: (%.2f, %.2f, 0.0))),' % (x, y))
    parts.append('        (name: "Fireflies", children: [\n%s\n        ]),' % "\n".join(flies))
    parts.append("""        (
            name: "Menu",
            components: [
                (kind: "MenuController", fields: {}),
            ],
        ),""")
    screens = ", ".join('(name: "%s", prefab: Some("ui/%s.prefab"))' % (n, f) for n, f in (
        ("ScreenMain", "main_menu"), ("ScreenLevels", "level_select"), ("ScreenOptions", "options"), ("ScreenHelp", "help")))
    parts.append('        (name: "MenuCanvas", prefab: Some("ui/menu_canvas.prefab"), children: [%s]),' % screens)
    os.makedirs(SCENES, exist_ok=True)
    text = "// Главное меню. Сгенерировано tools/gen_levels.py.\n(\n    name: \"Menu\",\n    objects: [\n%s\n    ],\n)\n" % "\n".join(parts)
    with open(os.path.join(SCENES, "menu.scene"), "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    with open(os.path.join(SCENES, "menu.scene.meta"), "w", newline="\n") as f:
        f.write('(\n    guid: "%s",\n    importer: "Scene",\n)' % guid_for("scenes/menu.scene"))
    print("menu scene written")


def write_data():
    d = os.path.join(HERE, "..", "assets", "data")
    os.makedirs(d, exist_ok=True)
    rows = []
    for i in range(6):
        L = INFO[i]
        rows.append('    (scene: "level%d", name: %s, subtitle: %s, blurb: %s, need: %d, par_time: %.1f, music: %s),' % (
            i + 1, ron_str(L.title), ron_str(L.subtitle), ron_str(BLURBS[i]), L.need, L.par, ron_str(L.music)))
    with open(os.path.join(d, "levels.ron"), "w", encoding="utf-8", newline="\n") as f:
        f.write("// Таблица уровней: имя сцены, названия, сколько светлячков нужно для выхода, контрольное\n"
                "// время (третья звезда) и музыка. Читается при старте уровня и в меню.\n[\n" + "\n".join(rows) + "\n]\n")
    with open(os.path.join(d, "rules.ron"), "w", encoding="utf-8", newline="\n") as f:
        f.write("// Правила звёзд: 1 — дошли до выхода; 2 — не меньше star2_fireflies_percent %% светлячков\n"
                "// и не больше star2_max_deaths поражений; 3 — ещё золотой светлячок и время не хуже контрольного.\n"
                "(star2_fireflies_percent: 60, star2_max_deaths: 2)\n")


def write_settings():
    """assets/project.settings: карта ввода, слои, окно, физика, звук, порядок скриптов."""
    text = """// Настройки проекта (Edit -> Project Settings). Подробно: docs/api/settings.md.
(
    // Порядок скриптов: платформы и враги двигаются в on_update, игрок — в on_late_update.
    script_order: { "LevelManager": -50 },
    player: (title: "Хранитель фонаря", width: 1280, height: 720, default_scene: "scenes/menu.scene"),
    physics2d: (gravity: (0.0, -9.81)),
    tags_and_layers: (tags: ["Player", "Enemy", "Pickup", "MainCamera"]),
    input: (actions: [
        (name: "Horizontal", kind: Axis, negative: ["Key:A", "Key:Left", "Pad:DPadLeft"], positive: ["Key:D", "Key:Right", "Pad:DPadRight"], analog: ["LeftStickX"], dead_zone: 0.2),
        (name: "Vertical", kind: Axis, negative: ["Key:S", "Key:Down", "Pad:DPadDown"], positive: ["Key:W", "Key:Up", "Pad:DPadUp"], analog: ["LeftStickY"], dead_zone: 0.2),
        (name: "Jump", kind: Button, bindings: ["Key:Space", "Key:W", "Key:Up", "Pad:South"]),
        (name: "Dash", kind: Button, bindings: ["Key:LShift", "Key:RShift", "Key:X", "Pad:West", "Pad:RightBumper", "Pad:RightTrigger"]),
        (name: "Pause", kind: Button, bindings: ["Key:Escape", "Key:P", "Pad:Start"]),
        (name: "Fire1", kind: Button, bindings: ["Key:LCtrl", "Pad:West"]),
        (name: "Submit", kind: Button, bindings: ["Key:Enter", "Pad:South"]),
        (name: "Cancel", kind: Button, bindings: ["Key:Escape", "Pad:East"]),
    ]),
    audio: (master_volume: 1.0, music_volume: 0.6),
)
"""
    with open(os.path.join(HERE, "..", "assets", "project.settings"), "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


if __name__ == "__main__":
    menu_scene()
    for build in (level1, level2, level3, level4, level5, level6):
        build()
    write_data()
    write_settings()
