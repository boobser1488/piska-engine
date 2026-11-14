#!/usr/bin/env python3
"""Генератор текстур и моделей «Шарового завода».

Запуск из каталога игры:  python tools/gen_assets.py
Пишет бесшовные PNG в assets/textures/ (нужны Pillow и numpy) и модели `.obj` в
assets/models/ (шестерёнка, большая шестерня-декорация, ротор вентилятора). Результат лежит
в репозитории вместе с `.meta` — скрипт нужен, чтобы перерисовать их или подправить палитру.
Сцены, префабы, материалы и звуки делает `cargo run -p marble --example bake_assets`.

Палитра: сталь (синеватая), сигнальный жёлтый и чёрный (предупреждающие полосы),
оранжевый (ящики, бамперы). Все текстуры тайлятся: движок проецирует их в мировых
координатах (MeshRenderer.tiling), поэтому стыки не должны быть видны.
"""
import os

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

SIZE = 256
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "textures")
RNG = np.random.default_rng(20240607)


def save(name, array):
    os.makedirs(OUT, exist_ok=True)
    img = Image.fromarray(np.clip(array, 0, 255).astype(np.uint8), "RGB")
    path = os.path.join(OUT, name)
    img.save(path, optimize=True)
    print("wrote textures/" + name)


def tileable_noise(scale, amount):
    """Бесшовный шум: сумма сдвигов размытых копий (размытие с заворотом)."""
    base = RNG.normal(0, 1, (SIZE, SIZE))
    big = np.tile(base, (3, 3))
    img = Image.fromarray(((big - big.min()) / (np.ptp(big)) * 255).astype(np.uint8))
    img = img.filter(ImageFilter.GaussianBlur(scale))
    arr = np.asarray(img, dtype=np.float32)[SIZE : 2 * SIZE, SIZE : 2 * SIZE]
    arr = (arr - arr.mean()) / (arr.std() + 1e-6)
    return arr * amount


def plate():
    """Стальной настил: панели со швами, заклёпки, зерно, царапины."""
    base = np.zeros((SIZE, SIZE, 3), np.float32)
    base[:] = (74, 86, 102)
    grain = tileable_noise(0.8, 5.0) + tileable_noise(6.0, 4.0)
    base += grain[..., None]
    img = Image.fromarray(np.clip(base, 0, 255).astype(np.uint8), "RGB")
    d = ImageDraw.Draw(img)
    # шов по краю тайла: тёмная канавка и светлая кромка
    for off, col in ((0, (34, 40, 50)), (1, (34, 40, 50)), (2, (112, 126, 146))):
        d.line([(off, 0), (off, SIZE)], fill=col)
        d.line([(0, off), (SIZE, off)], fill=col)
    d.line([(SIZE - 1, 0), (SIZE - 1, SIZE)], fill=(112, 126, 146))
    d.line([(0, SIZE - 1), (SIZE, SIZE - 1)], fill=(112, 126, 146))
    # шов посередине панели (светлее)
    d.line([(SIZE // 2, 0), (SIZE // 2, SIZE)], fill=(58, 68, 82))
    d.line([(0, SIZE // 2), (SIZE, SIZE // 2)], fill=(58, 68, 82))
    # заклёпки
    for cx in (16, SIZE // 2 + 16, SIZE - 16, SIZE // 2 - 16):
        for cy in (16, SIZE // 2 + 16, SIZE - 16, SIZE // 2 - 16):
            d.ellipse([cx - 4, cy - 4, cx + 4, cy + 4], fill=(40, 46, 56))
            d.ellipse([cx - 3, cy - 3, cx + 3, cy + 3], fill=(122, 136, 156))
            d.ellipse([cx - 1, cy - 2, cx + 1, cy], fill=(200, 212, 230))
    # царапины
    for _ in range(26):
        x, y = RNG.integers(14, SIZE - 14, 2)
        dx, dy = RNG.integers(-22, 22, 2)
        d.line([(x, y), (x + dx, y + dy)], fill=(96, 110, 130), width=1)
    save("plate.png", np.asarray(img, dtype=np.float32))


def hazard():
    """Предупреждающие диагональные полосы: жёлтый и почти чёрный."""
    y, x = np.mgrid[0:SIZE, 0:SIZE]
    band = ((x + y) // 32) % 2
    yellow = np.array((244, 178, 28), np.float32)
    black = np.array((30, 31, 36), np.float32)
    arr = np.where(band[..., None] == 1, yellow, black).astype(np.float32)
    arr += tileable_noise(1.0, 6.0)[..., None]
    # лёгкое затемнение у кромок полос — вид краски
    edge = np.abs(((x + y) % 32) - 16) / 16.0
    arr *= (0.86 + 0.14 * edge)[..., None]
    save("hazard.png", arr)


def brushed():
    """Шлифованный металл для бортов и колонн: длинные горизонтальные штрихи."""
    base = np.zeros((SIZE, SIZE, 3), np.float32)
    base[:] = (92, 100, 112)
    streaks = np.tile(RNG.normal(0, 1, (SIZE, 1)), (1, SIZE))
    img = Image.fromarray(((streaks - streaks.min()) / np.ptp(streaks) * 255).astype(np.uint8))
    img = img.filter(ImageFilter.GaussianBlur(0.6))
    streaks = (np.asarray(img, dtype=np.float32) - 128) / 40.0
    base += (streaks * 9.0)[..., None] + tileable_noise(1.2, 3.0)[..., None]
    save("brushed.png", base)


def crate():
    """Ящик: оранжевая окраска, тёмная рамка и диагональные распорки."""
    base = np.zeros((SIZE, SIZE, 3), np.float32)
    base[:] = (222, 118, 40)
    base += tileable_noise(1.0, 7.0)[..., None]
    img = Image.fromarray(np.clip(base, 0, 255).astype(np.uint8), "RGB")
    d = ImageDraw.Draw(img)
    frame = (92, 44, 18)
    for w, col in ((14, frame), (4, (250, 168, 90))):
        inset = 0 if w == 14 else 14
        d.rectangle([inset, inset, SIZE - 1 - inset, SIZE - 1 - inset], outline=col, width=w if w == 14 else 3)
    d.line([(14, 14), (SIZE - 14, SIZE - 14)], fill=frame, width=12)
    d.line([(SIZE - 14, 14), (14, SIZE - 14)], fill=frame, width=12)
    for cx, cy in ((24, 24), (SIZE - 24, 24), (24, SIZE - 24), (SIZE - 24, SIZE - 24)):
        d.ellipse([cx - 5, cy - 5, cx + 5, cy + 5], fill=(196, 188, 176))
        d.ellipse([cx - 2, cy - 3, cx + 1, cy], fill=(255, 255, 255))
    save("crate.png", np.asarray(img, dtype=np.float32))


MODELS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "models")


class Obj:
    """Минимальный писатель .obj: плоские грани с нормалями."""

    def __init__(self, flip=False):
        # flip — обойти каждый треугольник в обратную сторону (когда фигура описана в
        # зеркальной системе координат и иначе получилась бы вывернутой наизнанку).
        self.v, self.vn, self.f, self.flip = [], [], [], flip

    def tri(self, a, b, c):
        if self.flip:
            b, c = c, b
        a, b, c = (np.array(p, dtype=np.float64) for p in (a, b, c))
        n = np.cross(b - a, c - a)
        length = np.linalg.norm(n)
        if length < 1e-12:
            return
        n = n / length
        base = len(self.v)
        self.v += [a, b, c]
        self.vn += [n, n, n]
        self.f.append((base + 1, base + 2, base + 3))

    def quad(self, a, b, c, d):
        self.tri(a, b, c)
        self.tri(a, c, d)

    def check(self, name):
        """Модель — замкнутая поверхность, грани смотрят наружу.

        Вывернутую модель (отрицательный объём) рендер рисует «насквозь»: наружные грани
        отбрасываются как задние, видны внутренние. Незамкнутую видно насквозь с одной
        стороны."""
        volume = 0.0
        edges = {}
        key = lambda p: tuple(np.round(p, 4))
        for i, j, k in self.f:
            a, b, c = self.v[i - 1], self.v[j - 1], self.v[k - 1]
            volume += float(np.dot(a, np.cross(b, c))) / 6.0
            for p, q in ((a, b), (b, c), (c, a)):
                edges[(key(p), key(q))] = edges.get((key(p), key(q)), 0) + 1
        open_edges = sum(1 for (p, q) in edges if (q, p) not in edges)
        if volume <= 0:
            raise SystemExit("%s: faces point inward (signed volume %.4f)" % (name, volume))
        if open_edges:
            raise SystemExit("%s: the surface is not closed (%d open edges)" % (name, open_edges))

    def save(self, name):
        self.check(name)
        os.makedirs(MODELS, exist_ok=True)
        path = os.path.join(MODELS, name)
        with open(path, "w", encoding="utf-8") as out:
            out.write("# Marble Factory model, generated by tools/gen_assets.py\n")
            for v in self.v:
                out.write("v %.5f %.5f %.5f\n" % tuple(v))
            for n in self.vn:
                out.write("vn %.5f %.5f %.5f\n" % tuple(n))
            for i, j, k in self.f:
                out.write("f %d//%d %d//%d %d//%d\n" % (i, i, j, j, k, k))
        print("wrote models/" + name)


def gear_outline(teeth, r_body, r_tip):
    """Контур шестерни в плоскости XY: чередуются впадина, склон, вершина зуба, склон."""
    pts = []
    step = 2 * np.pi / teeth
    for t in range(teeth):
        a = t * step
        for frac, r in ((0.0, r_body), (0.18, r_body), (0.32, r_tip), (0.58, r_tip), (0.72, r_body)):
            ang = a + frac * step
            pts.append((r * np.cos(ang), r * np.sin(ang)))
    return pts


def gear_obj(name, teeth, r_body, r_tip, half_thick, axis):
    """Шестерня с зубьями; axis: 'z' — диск в XY (ось Z), 'y' — диск в XZ (ось Y)."""
    outline = gear_outline(teeth, r_body, r_tip)

    def place(x, y, z):
        return (x, z, -y) if axis == "y" else (x, y, z)

    obj = Obj()
    n = len(outline)
    for i in range(n):
        (x0, y0), (x1, y1) = outline[i], outline[(i + 1) % n]
        obj.quad(place(x0, y0, -half_thick), place(x1, y1, -half_thick), place(x1, y1, half_thick), place(x0, y0, half_thick))
        obj.tri(place(0, 0, half_thick), place(x0, y0, half_thick), place(x1, y1, half_thick))
        obj.tri(place(0, 0, -half_thick), place(x1, y1, -half_thick), place(x0, y0, -half_thick))
    obj.save(name)


def rotor_obj(name, blades=3):
    """Ротор вентилятора: ступица и лопасти-пластины вдоль X (ось Y), радиус 0.5."""
    # Фигура описана в плоскости XZ с осью Y вверх: обход «по возрастанию угла» здесь даёт
    # грани внутрь (раньше ротор был вывернут и просвечивал), поэтому обход переворачиваем.
    obj = Obj(flip=True)
    hub_r, hub_h, segments = 0.09, 0.1, 16
    for i in range(segments):
        a0, a1 = 2 * np.pi * i / segments, 2 * np.pi * (i + 1) / segments
        p0 = (hub_r * np.cos(a0), hub_r * np.sin(a0))
        p1 = (hub_r * np.cos(a1), hub_r * np.sin(a1))
        obj.quad((p0[0], 0, p0[1]), (p1[0], 0, p1[1]), (p1[0], hub_h, p1[1]), (p0[0], hub_h, p0[1]))
        obj.tri((0, hub_h, 0), (p0[0], hub_h, p0[1]), (p1[0], hub_h, p1[1]))
        # Дно ступицы: без него снизу ротор был виден насквозь.
        obj.tri((0, 0, 0), (p1[0], 0, p1[1]), (p0[0], 0, p0[1]))
    half_w, half_t = 0.09, 0.02
    for k in range(blades):
        ang = k * np.pi / blades
        c, s = np.cos(ang), np.sin(ang)

        def at(x, y, z):  # поворот вокруг Y
            return (x * c + z * s, y, -x * s + z * c)

        L = 0.5
        corners = [(-L, 0, -half_w), (L, 0, -half_w), (L, 0, half_w), (-L, 0, half_w)]
        top = [(x, 0.0 + 2 * half_t, z) for x, _, z in corners]
        obj.quad(*[at(*p) for p in top])
        obj.quad(*[at(*p) for p in reversed(corners)])
        for i in range(4):
            a, b = corners[i], corners[(i + 1) % 4]
            obj.quad(at(*a), at(*b), at(b[0], 2 * half_t, b[2]), at(a[0], 2 * half_t, a[2]))
    obj.save(name)


if __name__ == "__main__":
    gear_obj("gear.obj", 8, 0.34, 0.46, 0.06, "z")
    gear_obj("cog.obj", 14, 0.40, 0.5, 0.03, "y")
    rotor_obj("fan_rotor.obj")
    plate()
    hazard()
    brushed()
    crate()
