#!/usr/bin/env python3
"""Генератор картинок «Хранителя фонаря».

Запуск из любого места: `python games/lantern/tools/gen_assets.py`.
Пишет PNG и `.meta` в `games/lantern/assets/sprites/` (нужен Pillow 12 и numpy).
Всё рисуется в 4-кратном разрешении и уменьшается: гладкие силуэты без «лесенки».
GUID у `.meta` стабильные (из имени файла), чтобы сцены не ломались при перегенерации.
"""
import hashlib
import math
import os
import random

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets", "sprites")
SS = 4  # суперсэмплинг
T = 48  # пикселей на тайл

# Палитра: ночная пещера, тёплый свет фонаря.
INK = (12, 14, 34)
NAVY = (22, 28, 62)
INDIGO = (38, 48, 98)
SLATE = (74, 88, 140)
MIST = (150, 168, 214)
TEAL = (60, 214, 190)
TEAL_D = (28, 120, 120)
GOLD = (255, 207, 90)
GOLD_H = (255, 246, 205)
AMBER = (255, 150, 52)
MAGENTA = (232, 70, 130)
MAGENTA_D = (120, 30, 80)
VIOLET = (110, 70, 190)
BONE = (230, 224, 210)


def guid_for(name):
    return hashlib.md5(("lantern/" + name).encode()).hexdigest()


def meta(path, nearest=False):
    name = os.path.relpath(path, os.path.join(OUT, "..")).replace("\\", "/")
    filt = "Nearest" if nearest else "Linear"
    text = '(\n    guid: "%s",\n    importer: "Texture",\n    settings: {\n        "filter": Enum("%s"),\n    },\n)' % (guid_for(name), filt)
    with open(path + ".meta", "w", newline="\n") as f:
        f.write(text)


def save(img, name, nearest=False):
    path = os.path.join(OUT, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path)
    meta(path, nearest)
    print("wrote", name, img.size)


class Cv:
    """Холст в суперсэмплинге: координаты — в итоговых пикселях."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.img = Image.new("RGBA", (w * SS, h * SS), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.img)

    def _p(self, pts):
        return [(x * SS, y * SS) for x, y in pts]

    def ell(self, cx, cy, rx, ry, fill, outline=None, width=0):
        box = [(cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS]
        self.d.ellipse(box, fill=fill, outline=outline, width=int(width * SS))

    def rrect(self, x0, y0, x1, y1, r, fill, outline=None, width=0):
        self.d.rounded_rectangle([x0 * SS, y0 * SS, x1 * SS, y1 * SS], radius=r * SS, fill=fill,
                                 outline=outline, width=int(width * SS))

    def poly(self, pts, fill, outline=None):
        self.d.polygon(self._p(pts), fill=fill, outline=outline)

    def line(self, pts, fill, width=1):
        self.d.line(self._p(pts), fill=fill, width=int(width * SS), joint="curve")

    def done(self):
        return self.img.resize((self.w, self.h), Image.LANCZOS)


def tint(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c[:3]) + ((c[3],) if len(c) > 3 else ())


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def sheet(frames, cols):
    fw, fh = frames[0].size
    rows = (len(frames) + cols - 1) // cols
    img = Image.new("RGBA", (fw * cols, fh * rows), (0, 0, 0, 0))
    for i, f in enumerate(frames):
        img.paste(f, ((i % cols) * fw, (i // cols) * fh))
    return img


def glow_disc(size, color, power=2.2, peak=1.0):
    """Мягкий радиальный блик (альфа спадает от центра)."""
    yy, xx = np.mgrid[0:size, 0:size].astype(np.float32)
    r = np.sqrt((xx - size / 2 + 0.5) ** 2 + (yy - size / 2 + 0.5) ** 2) / (size / 2)
    a = np.clip(1 - r, 0, 1) ** power * peak
    arr = np.zeros((size, size, 4), np.uint8)
    arr[..., 0], arr[..., 1], arr[..., 2] = color
    arr[..., 3] = (a * 255).astype(np.uint8)
    return Image.fromarray(arr, "RGBA")


# ---------------------------------------------------------------- игрок
def outlined(img, width=5, color=(8, 10, 28, 255)):
    """Тёмный контур вокруг силуэта: герой читается на любом фоне."""
    a = img.getchannel("A").point(lambda v: 255 if v > 40 else 0)
    grown = a.filter(ImageFilter.MaxFilter(width))
    base = Image.new("RGBA", img.size, color)
    base.putalpha(grown)
    return Image.alpha_composite(base, img)


def lantern_frame(bob=0.0, flame=1.0, legs="stand", lean=0.0, scarf=0.0, eyes="open",
                  squash=(1.0, 1.0), hurt=False, stretch_x=1.0, streak=False, step=0.0):
    """Дух-фонарь 48×48, смотрит вправо. Опора — середина нижней кромки."""
    c = Cv(T, T)
    cx, base = 24 + lean, 46
    sx, sy = squash
    top = base - 36 * sy + bob
    # шарф (бирюзовый), развевается назад
    sc = TEAL if not hurt else MAGENTA
    sy0 = top + 17 * sy
    c.poly([(cx - 8, sy0), (cx - 8 - 10 - scarf * 6, sy0 + 2 - scarf * 7), (cx - 8 - 14 - scarf * 7, sy0 + 7 - scarf * 6),
            (cx - 9, sy0 + 7)], sc)
    c.poly([(cx - 8, sy0 + 3), (cx - 8 - 8 - scarf * 5, sy0 + 9 - scarf * 4), (cx - 8 - 11 - scarf * 5, sy0 + 12 - scarf * 4),
            (cx - 9, sy0 + 9)], tint(sc, 0.7))
    # ножки
    leg = INK
    foot_y = base - 1
    def boot(x, y, lift=0):
        c.rrect(x - 3.4, y - 6 - lift, x + 3.4, y + 1 - lift, 2.5, leg)
    if legs == "stand":
        boot(cx - 5, foot_y); boot(cx + 5, foot_y)
    elif legs == "run":
        boot(cx - 5 + step * 6, foot_y - max(0.0, -step) * 4)
        boot(cx + 5 - step * 6, foot_y - max(0.0, step) * 4)
    elif legs == "tuck":
        boot(cx - 5, foot_y, 5); boot(cx + 5, foot_y, 4)
    elif legs == "fall":
        boot(cx - 8, foot_y, 2); boot(cx + 8, foot_y, 1)
    # тело-фонарь: стекло
    w, h = 13 * sx, 15 * sy
    gcx, gcy = cx, top + 20 * sy
    glass_o = (255, 130, 60) if not hurt else (255, 90, 90)
    c.rrect(gcx - w - 1.6, gcy - h - 1.6, gcx + w + 1.6, gcy + h + 1.6, 9, (28, 34, 66))
    c.rrect(gcx - w, gcy - h, gcx + w, gcy + h, 7.5, glass_o)
    c.rrect(gcx - w + 2.2, gcy - h + 2.2, gcx + w - 2.2, gcy + h - 2.2, 6, GOLD)
    c.ell(gcx, gcy + 1, (w - 4) * flame, (h - 4) * flame, GOLD_H)
    c.ell(gcx - 7, gcy - 7, 2.0, 3.2, (255, 255, 235, 200))
    # переплёт рамы
    c.line([(gcx - w, gcy), (gcx + w, gcy)], (28, 34, 66, 140), 1)
    # глаза
    ex = gcx + 4 + lean * 0.2
    if eyes == "open":
        for dx in (-3.6, 4.6):
            c.ell(ex + dx - 2, gcy + 1, 2.0, 3.2, INK)
            c.ell(ex + dx - 2.6, gcy - 0.2, 0.7, 0.9, (255, 255, 255))
    elif eyes == "hurt":
        for dx in (-3.6, 4.6):
            c.line([(ex + dx - 4, gcy - 2), (ex + dx, gcy + 3)], INK, 1.4)
            c.line([(ex + dx - 4, gcy + 3), (ex + dx, gcy - 2)], INK, 1.4)
    # крышка и ручка
    cap_y = gcy - h - 1.6
    c.poly([(gcx - w - 3, cap_y + 3), (gcx - w + 3, cap_y - 6), (gcx + w - 3, cap_y - 6), (gcx + w + 3, cap_y + 3)], (52, 58, 96))
    c.poly([(gcx - w + 3, cap_y - 6), (gcx + w - 3, cap_y - 6), (gcx + w - 5, cap_y - 4), (gcx - w + 5, cap_y - 4)], (96, 108, 160))
    c.ell(gcx, cap_y - 9, 4.2, 4.2, None, outline=(52, 58, 96), width=2)
    # дно
    c.rrect(gcx - w - 1, gcy + h - 1, gcx + w + 1, gcy + h + 4, 2, (52, 58, 96))
    img = c.done()
    if stretch_x != 1.0:
        nw = int(T * stretch_x)
        img = img.resize((nw, T), Image.LANCZOS)
        out = Image.new("RGBA", (T, T), (0, 0, 0, 0))
        out.paste(img, ((T - nw) // 2, 0))
        img = out
    if streak:
        s = Cv(T, T)
        for i, a in enumerate((60, 40, 24)):
            s.rrect(0, 14 + i * 7, 20 - i * 3, 18 + i * 7, 2, (255, 214, 120, a))
        img = Image.alpha_composite(s.done(), img)
    return outlined(img)


def make_player():
    fr = []
    for i in range(4):  # idle: покачивание, мерцание
        fr.append(lantern_frame(bob=[0, -1, -1.6, -0.6][i], flame=[1.0, 0.95, 1.05, 0.98][i], scarf=0.15 * i % 0.4))
    fr += [None] * 4
    for i in range(6):  # бег
        ph = i / 6 * 2 * math.pi
        fr.append(lantern_frame(bob=-abs(math.sin(ph)) * 2.2, legs="run", step=math.sin(ph), lean=1.5, scarf=0.55))
    fr += [None] * 2
    fr += [lantern_frame(legs="tuck", squash=(0.92, 1.08), scarf=0.9, bob=-1),
           lantern_frame(legs="fall", squash=(0.95, 1.04), scarf=1.0),
           lantern_frame(legs="fall", eyes="hurt", hurt=True, lean=-1.5, scarf=0.3),
           lantern_frame(legs="tuck", stretch_x=1.12, lean=2, streak=True, scarf=1.0)]
    blank = Image.new("RGBA", (T, T), (0, 0, 0, 0))
    fr = [f if f is not None else blank for f in fr]
    while len(fr) % 8:
        fr.append(blank)
    save(sheet(fr, 8), "player.png")


# ---------------------------------------------------------------- враги
def crawler_frame(i):
    c = Cv(T, T)
    ph = i / 4 * 2 * math.pi
    base = 44
    # лапки
    for k in range(4):
        x = 12 + k * 7.5
        off = math.sin(ph + k * 1.6) * 2.2
        c.line([(x, base - 7), (x + off - 1.5, base)], INK, 2.4)
    # панцирь
    c.ell(24, base - 14, 19, 13.5, (60, 36, 96))
    c.ell(24, base - 16, 17, 11, (92, 56, 140))
    c.ell(21, base - 21, 9, 4.5, (150, 110, 205, 150))
    for k, x in enumerate((15, 22, 29)):
        c.ell(x, base - 16 + (k % 2) * 2, 2.6, 2.6, MAGENTA)
        c.ell(x, base - 16 + (k % 2) * 2, 1.2, 1.2, (255, 190, 220))
    # голова и глаза
    c.ell(39, base - 9, 7, 6, (60, 36, 96))
    c.ell(41, base - 11, 2.8, 3, (255, 240, 120))
    c.ell(42, base - 11, 1.3, 1.8, INK)
    c.line([(37, base - 15), (44, base - 14)], INK, 1.8)
    return c.done()


def bat_frame(i):
    c = Cv(T, T)
    # 0 крылья вверх, 1 середина, 2 вниз, 3 середина
    a = [-1.0, -0.2, 0.9, -0.2][i]
    cx, cy = 24, 26
    for sgn in (-1, 1):
        tip = (cx + sgn * 20, cy + a * 12 - 4)
        mid = (cx + sgn * 14, cy + a * 4 - 10)
        c.poly([(cx + sgn * 3, cy - 3), mid, tip, (cx + sgn * 14, cy + 4 + a * 3), (cx + sgn * 8, cy + 6)], (80, 40, 120))
        c.line([(cx + sgn * 3, cy - 3), tip], (140, 90, 190), 1.5)
        c.line([(cx + sgn * 3, cy), (cx + sgn * 14, cy + 4 + a * 3)], (140, 90, 190), 1.2)
    c.ell(cx, cy + 1, 8, 9, (62, 34, 98))
    c.poly([(cx - 7, cy - 6), (cx - 5, cy - 15), (cx - 1, cy - 8)], (62, 34, 98))
    c.poly([(cx + 7, cy - 6), (cx + 5, cy - 15), (cx + 1, cy - 8)], (62, 34, 98))
    c.ell(cx - 3.2, cy - 1, 2.4, 2.8, (255, 70, 90))
    c.ell(cx + 3.2, cy - 1, 2.4, 2.8, (255, 70, 90))
    c.ell(cx - 3.2, cy - 1, 1, 1.2, INK)
    c.ell(cx + 3.2, cy - 1, 1, 1.2, INK)
    c.poly([(cx - 2, cy + 5), (cx - 1, cy + 8), (cx, cy + 5)], BONE)
    c.poly([(cx, cy + 5), (cx + 1, cy + 8), (cx + 2, cy + 5)], BONE)
    return c.done()


def spitter_frame(i):
    """Плеватель: луковица на стебле. 0-1 покой, 2 замах (рот открыт), 3 выстрел."""
    c = Cv(T, T)
    sway = [0, 1.5, 0, -1][i]
    base = 47
    # стебель
    c.line([(24, base), (24 + sway * 0.4, base - 14), (24 + sway, base - 24)], (40, 120, 90), 5)
    c.poly([(24, base - 8), (12, base - 12), (22, base - 4)], (40, 150, 100))
    c.poly([(24, base - 10), (37, base - 15), (26, base - 5)], (40, 150, 100))
    hx, hy = 24 + sway, base - 28
    squash = [1, 1.04, 1.12, 0.9][i]
    c.ell(hx, hy, 14 * (2 - squash) * 0.5 + 6, 12 * squash, (46, 168, 120))
    c.ell(hx, hy - 3, 12, 8, (90, 214, 150, 140))
    mouth = [3, 3.5, 8, 5][i]
    c.ell(hx + 6, hy + 2, 7, mouth, (60, 14, 52))
    if i in (2, 3):
        c.ell(hx + 8, hy + 2, 3 + (i == 2) * 2, 3 + (i == 2) * 2, (255, 120, 190))
    c.ell(hx - 3, hy - 5, 2.6, 3, (255, 240, 130))
    c.ell(hx - 2.2, hy - 5, 1.1, 1.6, INK)
    for k in range(5):  # шипики-зубки
        a = -0.6 + k * 0.3
        c.poly([(hx + math.cos(a) * 13, hy - 8 - 2 + math.sin(a) * 8 - 2), (hx + math.cos(a) * 17, hy - 12 + math.sin(a) * 8),
                (hx + math.cos(a + 0.2) * 13, hy - 8 + math.sin(a + 0.2) * 8)], (30, 100, 80))
    return c.done()


def brute_frame(i):
    """Тяжеляк 72×72: каменный «жук» с шипами на спине, не растаптывается."""
    W = 72
    c = Cv(W, W)
    ph = i / 4 * 2 * math.pi
    base = 70
    for k, x in enumerate((20, 31, 43, 54)):
        off = math.sin(ph + k * math.pi) * 3
        c.rrect(x - 4, base - 12, x + 4, base + off * 0.0, 3, (46, 40, 72))
        c.ell(x + off, base - 1, 5.5, 2.4, (30, 26, 52))
    c.ell(34, base - 28, 29, 24, (88, 78, 120))
    c.ell(34, base - 32, 27, 19, (118, 106, 156))
    c.ell(28, base - 40, 14, 6, (170, 160, 210, 120))
    # шипы
    for k in range(6):
        x = 14 + k * 8
        h = 14 + (k % 2) * 5
        c.poly([(x - 4, base - 36 - (k in (0, 5)) * -3), (x + 1, base - 36 - h - 12), (x + 6, base - 36 - (k in (0, 5)) * -3)], (210, 200, 232))
        c.poly([(x - 1, base - 36), (x + 1, base - 36 - h - 12), (x + 3, base - 36)], (255, 255, 255, 120))
    # голова
    c.ell(60, base - 22, 11, 10, (88, 78, 120))
    c.ell(64, base - 25, 3.6, 3.6, (255, 90, 70))
    c.ell(65, base - 25, 1.6, 2.2, INK)
    c.line([(58, base - 31), (68, base - 28)], INK, 2.4)
    c.poly([(62, base - 16), (64, base - 11), (66, base - 16)], BONE)
    # трещины-огонь
    c.line([(18, base - 28), (26, base - 22), (24, base - 14)], (255, 150, 60), 1.6)
    return c.done()


def boss_frame(i):
    """Хозяин Тьмы 192×192. Кадры 0-3 — взмахи, 4 — замах (красный глаз), 5 — ранен, 6-7 — гибель."""
    W = 192
    c = Cv(W, W)
    cx, cy = 96, 100
    flap = [-0.9, -0.2, 0.7, -0.2, -0.6, 0.2, 0.9, 0.9][i]
    body = (46, 24, 82) if i != 5 else (140, 80, 150)
    body_l = (84, 48, 140) if i != 5 else (210, 150, 220)
    for sgn in (-1, 1):
        tip = (cx + sgn * 88, cy + flap * 50 - 26)
        c.poly([(cx + sgn * 20, cy - 12), (cx + sgn * 56, cy + flap * 18 - 52), tip,
                (cx + sgn * 70, cy + flap * 24 + 12), (cx + sgn * 44, cy + 24), (cx + sgn * 24, cy + 18)], (70, 36, 118) if i != 5 else (170, 110, 190))
        for k in range(4):
            fx = cx + sgn * (36 + k * 15)
            c.line([(cx + sgn * 20, cy), (fx + sgn * 6, cy + flap * (16 + k * 6) - 12 + k * 6)], body_l, 2.4)
    c.ell(cx, cy, 44, 48, body)
    c.ell(cx, cy - 6, 40, 40, body_l)
    c.ell(cx - 10, cy - 26, 20, 12, (200, 170, 255, 70))
    # рога
    for sgn in (-1, 1):
        c.poly([(cx + sgn * 20, cy - 40), (cx + sgn * 42, cy - 74), (cx + sgn * 36, cy - 34)], (222, 214, 240))
    # глаз
    red = i in (4, 6, 7)
    ec = (255, 60, 70) if red else (255, 210, 90)
    c.ell(cx, cy - 2, 25, 22, (20, 10, 30))
    c.ell(cx, cy - 2, 21, 18, ec)
    c.ell(cx, cy - 2, 9, 15, INK)
    c.ell(cx - 7, cy - 9, 4, 4, (255, 255, 255, 200))
    # пасть
    c.poly([(cx - 22, cy + 26), (cx - 16, cy + 36), (cx - 10, cy + 26), (cx - 4, cy + 36), (cx + 2, cy + 26),
            (cx + 8, cy + 36), (cx + 14, cy + 26), (cx + 20, cy + 36), (cx + 24, cy + 26)], BONE)
    if i >= 6:
        c.line([(cx - 20, cy - 20), (cx + 6, cy + 10)], (255, 255, 255), 3)
    return c.done()


# ---------------------------------------------------------------- предметы
def make_firefly():
    fr = []
    for i in range(4):
        c = Cv(32, 32)
        a = [0.9, 0.2, -0.6, 0.2][i]
        c.ell(16, 16, 5, 5, (255, 255, 255, 0))
        for sgn in (-1, 1):
            c.ell(16 + sgn * 6, 13 + a * 3, 5.5, 3.2 * (0.6 + abs(a)), (210, 240, 255, 190))
        c.ell(16, 18, 3.4, 5, (255, 244, 170))
        c.ell(16, 21, 3.0, 3.4, (255, 255, 255))
        img = c.done()
        g = glow_disc(32, (255, 235, 140), 1.7, 0.75)
        fr.append(Image.alpha_composite(g, img))
    save(sheet(fr, 4), "firefly.png")


def make_checkpoint():
    fr = []
    for lit in (0, 1):
        c = Cv(48, 72)
        c.rrect(21, 18, 27, 70, 2, (60, 66, 108))
        c.rrect(14, 66, 34, 71, 2, (60, 66, 108))
        c.poly([(12, 22), (36, 22), (32, 8), (16, 8)], (60, 66, 108))
        c.rrect(16, 20, 32, 42, 4, (30, 34, 62))
        if lit:
            c.rrect(17, 21, 31, 41, 3, AMBER)
            c.ell(24, 33, 5.5, 7, GOLD_H)
        else:
            c.rrect(17, 21, 31, 41, 3, (46, 52, 90))
        c.ell(24, 6, 4, 4, None, outline=(60, 66, 108), width=2)
        img = c.done()
        if lit:
            img = Image.alpha_composite(glow_disc(72, (255, 190, 90), 2.0, 0.5).crop((12, 0, 60, 72)).resize((48, 72)), img)
        fr.append(img)
    save(sheet(fr, 2), "checkpoint.png")


def make_gate():
    c = Cv(48, 144)
    c.rrect(2, 0, 46, 10, 3, (64, 70, 112))
    c.rrect(2, 134, 46, 144, 3, (64, 70, 112))
    for k in range(5):
        x = 6 + k * 9
        c.rrect(x - 2.5, 8, x + 2.5, 136, 2, (118, 128, 176))
        c.rrect(x - 1.2, 8, x + 0.2, 136, 1, (190, 200, 235))
    c.line([(2, 40), (46, 40)], (64, 70, 112), 3)
    c.line([(2, 100), (46, 100)], (64, 70, 112), 3)
    # замочная руна
    c.ell(24, 72, 11, 11, (30, 34, 62))
    c.ell(24, 72, 8.5, 8.5, GOLD)
    c.ell(24, 72, 4, 4, GOLD_H)
    save(c.done(), "gate.png")


def make_exit():
    fr = []
    for i in range(5):
        c = Cv(96, 96)
        on = i > 0
        c.rrect(8, 4, 88, 96, 36, (64, 70, 112))
        c.rrect(15, 12, 81, 96, 30, (22, 26, 56))
        if on:
            ph = (i - 1) / 4 * 2 * math.pi
            for k in range(3):
                r = 26 - k * 7
                c.ell(48 + math.sin(ph + k) * 3, 60 + math.cos(ph + k) * 3, r * 0.82, r * 1.4,
                      (255, 200 - k * 20, 90 + k * 30, 150 + k * 30))
            c.ell(48, 60, 8, 16, GOLD_H)
        else:
            c.line([(34, 50), (62, 50)], (60, 66, 108), 3)
            c.line([(34, 66), (62, 66)], (60, 66, 108), 3)
            c.ell(48, 58, 6, 6, (60, 66, 108))
        # руны на арке
        for k in range(5):
            a = math.pi * (0.2 + 0.6 * k / 4)
            x, y = 48 - math.cos(a) * 34, 30 - math.sin(a) * 22
            c.ell(x, y + 14, 2.6, 2.6, GOLD if on else SLATE)
        img = c.done()
        if on:
            img = Image.alpha_composite(glow_disc(96, (255, 190, 90), 2.2, 0.45), img)
        fr.append(img)
    save(sheet(fr, 5), "exit.png")


def make_spring():
    fr = []
    for ext in (0, 1):
        c = Cv(48, 48)
        h = 14 + ext * 10
        base = 46
        c.rrect(8, base - 5, 40, base, 3, (64, 70, 112))
        # пружина
        pts = []
        for k in range(9):
            pts.append((24 + (11 if k % 2 else -11), base - 5 - k * h / 8))
        c.line([(24, base - 5)] + pts, TEAL, 3.2)
        c.rrect(6, base - 7 - h - 4, 42, base - 7 - h + 2, 3, MAGENTA)
        c.rrect(8, base - 7 - h - 3, 40, base - 7 - h - 1, 2, (255, 160, 200))
        fr.append(c.done())
    save(sheet(fr, 2), "spring.png")


def make_platforms():
    c = Cv(144, 24)
    c.rrect(0, 4, 144, 22, 7, (46, 52, 92))
    c.rrect(2, 2, 142, 14, 6, (96, 110, 166))
    c.rrect(6, 3, 138, 7, 2, (170, 186, 230))
    for x in (14, 72, 130):
        c.ell(x, 17, 3, 3, GOLD)
    # лампочки по бокам
    save(c.done(), "platform.png")
    fr = []
    for k in range(3):
        c = Cv(144, 24)
        c.rrect(0, 4, 144, 22, 7, (88, 62, 70))
        c.rrect(2, 2, 142, 14, 6, (170, 128, 120))
        c.rrect(6, 3, 138, 7, 2, (226, 190, 170))
        rnd = random.Random(7)
        for j in range(k * 3 + 1):
            x = rnd.randint(10, 134)
            c.line([(x, 2), (x + rnd.randint(-8, 8), 12), (x + rnd.randint(-10, 10), 22)], (40, 20, 30), 1.6)
        fr.append(c.done())
    img = Image.new("RGBA", (144, 72))
    for i, f in enumerate(fr):
        img.paste(f, (0, i * 24))
    save(img, "crumble.png")


def make_projectile():
    c = Cv(24, 24)
    c.ell(12, 12, 9, 9, (255, 90, 160, 120))
    c.ell(12, 12, 6.5, 6.5, MAGENTA)
    c.ell(12, 12, 3.5, 3.5, (255, 220, 240))
    save(Image.alpha_composite(glow_disc(24, (255, 100, 170), 1.5, 0.5), c.done()), "projectile.png")


def make_rock():
    c = Cv(48, 96)
    c.poly([(4, 0), (44, 0), (36, 40), (24, 94), (12, 40)], (110, 96, 150))
    c.poly([(4, 0), (20, 0), (24, 94), (12, 40)], (150, 136, 196))
    c.poly([(24, 0), (44, 0), (36, 40), (24, 94)], (74, 64, 108))
    save(c.done(), "rock.png")


def make_glow():
    save(glow_disc(128, (255, 255, 255), 2.0, 1.0), "glow.png")


# ---------------------------------------------------------------- тайлсет
def noise(w, h, seed, amp):
    rng = np.random.RandomState(seed)
    return (rng.rand(h, w) - 0.5) * amp


def rock_tile(mask, variant=0, base=(40, 50, 100)):
    """Плитка скалы: mask — открытые стороны (1 N, 2 E, 4 S, 8 W)."""
    rng = np.random.RandomState(mask * 7 + variant * 131 + 3)
    arr = np.zeros((T, T, 3), np.float32)
    yy, xx = np.mgrid[0:T, 0:T].astype(np.float32)
    grad = 1.0 + 0.12 * np.sin(xx / 7.0 + variant) * np.cos(yy / 9.0 + mask)
    for ch in range(3):
        arr[..., ch] = base[ch] * grad
    arr += noise(T, T, mask * 17 + variant, 10)[..., None]
    # крупные «камни»
    for _ in range(4):
        x, y, r = rng.randint(0, T), rng.randint(0, T), rng.randint(5, 11)
        d = np.sqrt((xx - x) ** 2 + (yy - y) ** 2)
        arr[d < r] *= 1.07
        arr[(d >= r) & (d < r + 1.3)] *= 0.82
    img = Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "RGB").convert("RGBA")
    c = Cv(T, T)
    rim = (96, 114, 180)
    # боковые кромки
    if mask & 2:
        c.rrect(T - 5, 0, T, T, 0, (24, 28, 60)); c.rrect(T - 8, 0, T - 5, T, 0, rim)
    if mask & 8:
        c.rrect(0, 0, 5, T, 0, (24, 28, 60)); c.rrect(5, 0, 8, T, 0, rim)
    if mask & 4:
        c.rrect(0, T - 7, T, T, 0, (18, 22, 50)); c.rrect(0, T - 10, T, T - 7, 0, (70, 82, 140))
    if mask & 1:
        # мох: волнистая бирюзовая полоса сверху
        pts = [(0, 0)]
        for x in range(0, T + 1, 4):
            pts.append((x, 7 + 3 * math.sin(x / 5.0 + mask) + (x * 7 % 3)))
        pts.append((T, 0))
        c.poly(pts, TEAL_D)
        pts2 = [(0, 0)]
        for x in range(0, T + 1, 4):
            pts2.append((x, 4 + 2 * math.sin(x / 5.0 + mask + 1)))
        pts2.append((T, 0))
        c.poly(pts2, TEAL)
        c.rrect(0, 0, T, 1.6, 0, (190, 255, 240))
    over = c.done()
    # углы соседних открытых сторон слегка скругляем затемнением
    return Image.alpha_composite(img, over)


def spike_tile(kind):
    c = Cv(T, T)
    # kind: up, down, left, right
    n = 4
    for k in range(n):
        x0 = k * T / n
        pts = [(x0 + 1.5, T), (x0 + T / n / 2, T - 24), (x0 + T / n - 1.5, T)]
        c.poly(pts, (210, 218, 245))
        c.poly([(x0 + 1.5, T), (x0 + T / n / 2, T - 24), (x0 + T / n / 2, T)], (250, 252, 255))
        c.poly([(x0 + T / n / 2, T - 24), (x0 + T / n - 1.5, T), (x0 + T / n / 2, T)], (140, 150, 195))
        c.ell(x0 + T / n / 2, T - 22, 1.4, 1.4, MAGENTA)
    c.rrect(0, T - 5, T, T, 1, (46, 52, 92))
    img = c.done()
    if kind == "down":
        img = img.transpose(Image.FLIP_TOP_BOTTOM)
    elif kind == "left":
        img = img.rotate(-90)  # шипы вверх -> влево
    elif kind == "right":
        img = img.rotate(90)
    return img


def deco_tile(kind):
    c = Cv(T, T)
    if kind == "moss":
        for k in range(7):
            x = 4 + k * 6.5
            c.line([(x, T), (x + math.sin(k) * 3, T - 8 - (k % 3) * 5)], (44, 170, 140), 2.4)
        c.ell(24, T - 3, 20, 5, (36, 130, 110))
    elif kind == "crystal":
        for x, h, col in ((15, 30, (120, 230, 255)), (26, 40, (90, 190, 255)), (36, 24, (160, 240, 255))):
            c.poly([(x - 5, T), (x - 3, T - h + 8), (x, T - h), (x + 3, T - h + 8), (x + 5, T)], col)
            c.poly([(x - 1, T), (x, T - h), (x + 3, T - h + 8), (x + 5, T)], tint(col, 0.7))
    elif kind == "shroom":
        c.rrect(20, T - 18, 28, T, 3, (200, 190, 220))
        c.ell(24, T - 20, 15, 11, (210, 60, 140))
        c.rrect(8, T - 18, 40, T - 12, 0, (0, 0, 0, 0))
        for x, y in ((17, T - 24), (25, T - 28), (31, T - 21)):
            c.ell(x, y, 2.4, 2.4, (255, 220, 240))
    elif kind == "stalactite":
        for x, h in ((12, 26), (24, 38), (36, 20)):
            c.poly([(x - 6, 0), (x + 6, 0), (x, h)], (70, 82, 134))
            c.poly([(x - 6, 0), (x, 0), (x, h)], (100, 116, 176))
    elif kind == "stalagmite":
        for x, h in ((12, 22), (26, 34), (38, 18)):
            c.poly([(x - 6, T), (x + 6, T), (x, T - h)], (70, 82, 134))
            c.poly([(x - 6, T), (x, T), (x, T - h)], (100, 116, 176))
    elif kind == "vines":
        for x in (10, 22, 34, 42):
            pts = [(x, 0)]
            for y in range(6, 40, 6):
                pts.append((x + math.sin(y * 0.4 + x) * 3, y))
            c.line(pts, (38, 150, 112), 2.2)
            c.ell(pts[-1][0], pts[-1][1], 2.8, 2.8, TEAL)
    elif kind == "lamp":
        c.line([(24, 0), (24, 14)], (60, 66, 108), 2)
        c.rrect(16, 14, 32, 34, 4, (60, 66, 108))
        c.rrect(18, 17, 30, 32, 3, AMBER)
        c.ell(24, 25, 4, 6, GOLD_H)
    elif kind == "glowshroom":
        c.rrect(22, T - 12, 26, T, 2, (170, 220, 220))
        c.ell(24, T - 14, 9, 7, (90, 240, 220))
        c.ell(24, T - 15, 5, 3.5, (210, 255, 250))
        c.ell(36, T - 8, 6, 4.5, (70, 200, 190))
        c.rrect(35, T - 6, 37, T, 1, (170, 220, 220))
    elif kind == "bones":
        c.line([(8, T - 4), (38, T - 6)], BONE, 3.2)
        c.ell(8, T - 4, 3.6, 3.6, BONE)
        c.ell(40, T - 6, 3.6, 3.6, BONE)
        c.ell(24, T - 12, 8, 7, BONE)
        c.ell(21, T - 12, 2, 2.4, INK)
        c.ell(27, T - 12, 2, 2.4, INK)
    img = c.done()
    if kind in ("moss", "crystal", "shroom", "glowshroom"):
        pass
    return img


def make_tileset():
    cells = []
    for m in range(16):
        cells.append(rock_tile(m))
    # 16 — «облачко» (односторонняя платформа)
    c = Cv(T, T)
    c.rrect(0, 0, T, 14, 5, (46, 52, 92))
    c.rrect(0, 0, T, 9, 5, (110, 126, 190))
    c.rrect(2, 1, T - 2, 4, 2, (190, 206, 245))
    for x in (8, 24, 40):
        c.ell(x, 11, 2, 2, GOLD)
    cells.append(c.done())
    cells += [spike_tile("up"), spike_tile("down"), spike_tile("left"), spike_tile("right")]
    cells += [rock_tile(0, 1), rock_tile(0, 2)]
    for kind in ("moss", "crystal", "shroom", "stalactite", "stalagmite", "vines", "lamp", "glowshroom", "bones"):
        cells.append(deco_tile(kind))
    assert len(cells) == 32, len(cells)
    save(sheet(cells, 8), "tiles.png", nearest=True)


# ---------------------------------------------------------------- фон
def wrap_circle(c, x, y, r, W, col):
    for dx in (-W, 0, W):
        c.ell(x + dx, y, r, r, col)


def make_bg(name, seed, base_top, base_bot, kind):
    """Бесшовные по горизонтали слои пещеры. Размер 768×432."""
    W, H = 768, 432
    rnd = random.Random(seed)
    if kind == "far":
        yy = np.linspace(0, 1, H, dtype=np.float32)[:, None]
        arr = np.zeros((H, W, 4), np.float32)
        for ch in range(3):
            arr[..., ch] = base_top[ch] * (1 - yy) + base_bot[ch] * yy
        arr[..., 3] = 255
        img = Image.fromarray(arr.astype(np.uint8), "RGBA")
        # далёкие огоньки
        c = Cv(W, H)
        for _ in range(60):
            x, y = rnd.randint(0, W), rnd.randint(0, H)
            r = rnd.uniform(1.2, 3.4)
            col = rnd.choice([(150, 190, 255, 110), (255, 220, 150, 90), (140, 255, 230, 100)])
            wrap_circle(c, x, y, r, W, col)
        # слабые сталактиты далеко
        for k in range(12):
            x = k * W / 12 + rnd.randint(-20, 20)
            h = rnd.randint(60, 150)
            wcol = (tint(base_top, 0.7) + (200,))
            for dx in (-W, 0, W):
                c.poly([(x - 28 + dx, 0), (x + 28 + dx, 0), (x + dx, h)], wcol)
        over = c.done().filter(ImageFilter.GaussianBlur(1.2))
        img = Image.alpha_composite(img, over)
    elif kind == "mid":
        img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        c = Cv(W, H)
        col = tint(base_top, 0.85) + (255,)
        for k in range(9):
            x = k * W / 9 + rnd.randint(-18, 18)
            h = rnd.randint(110, 230)
            w = rnd.randint(26, 52)
            for dx in (-W, 0, W):
                c.poly([(x - w + dx, 0), (x + w + dx, 0), (x + w * 0.2 + dx, h * 0.7), (x + dx, h)], col)
        for k in range(8):
            x = k * W / 8 + rnd.randint(-20, 20)
            h = rnd.randint(70, 170)
            w = rnd.randint(30, 60)
            for dx in (-W, 0, W):
                c.poly([(x - w + dx, H), (x + w + dx, H), (x + w * 0.25 + dx, H - h), (x - w * 0.2 + dx, H - h * 0.9)],
                       tint(base_top, 0.8) + (255,))
        img = c.done()
        img = img.filter(ImageFilter.GaussianBlur(0.8))
    else:  # near
        c = Cv(W, H)
        col = tint(base_top, 0.55) + (255,)
        for k in range(5):
            x = k * W / 5 + rnd.randint(-30, 30)
            w = rnd.randint(34, 60)
            for dx in (-W, 0, W):
                c.poly([(x - w * 1.4 + dx, H), (x - w + dx, H * 0.45), (x + w * 0.3 + dx, H * 0.3), (x + w + dx, H * 0.5),
                        (x + w * 1.5 + dx, H)], col)
        for k in range(7):
            x = k * W / 7 + rnd.randint(-25, 25)
            h = rnd.randint(40, 110)
            for dx in (-W, 0, W):
                c.poly([(x - 30 + dx, 0), (x + 30 + dx, 0), (x + dx, h)], col)
        img = c.done()
    save(img, name)


# ---------------------------------------------------------------- интерфейс
def make_ui():
    # сердце
    for name, fill, edge in (("ui_heart.png", (255, 84, 110), (140, 20, 60)), ("ui_heart_empty.png", (54, 58, 96), (28, 30, 56))):
        c = Cv(48, 48)
        c.ell(15, 17, 11, 11, edge); c.ell(33, 17, 11, 11, edge)
        c.poly([(3, 21), (45, 21), (24, 45)], edge)
        c.ell(15, 17, 9, 9, fill); c.ell(33, 17, 9, 9, fill)
        c.poly([(5, 21), (43, 21), (24, 42)], fill)
        if "empty" not in name:
            c.ell(12, 13, 3.4, 2.4, (255, 220, 230))
        save(c.done(), name)
    # звезда
    for name, fill, edge in (("ui_star.png", (255, 214, 80), (150, 90, 20)), ("ui_star_empty.png", (50, 56, 92), (26, 30, 56))):
        c = Cv(64, 64)
        pts = []
        for k in range(10):
            a = -math.pi / 2 + k * math.pi / 5
            r = 28 if k % 2 == 0 else 12
            pts.append((32 + math.cos(a) * r, 34 + math.sin(a) * r))
        c.poly(pts, edge)
        pts = []
        for k in range(10):
            a = -math.pi / 2 + k * math.pi / 5
            r = 24 if k % 2 == 0 else 10
            pts.append((32 + math.cos(a) * r, 34 + math.sin(a) * r))
        c.poly(pts, fill)
        save(c.done(), name)
    # фонарь HUD (тёмный и горящий)
    for name, lit in (("ui_lantern_dim.png", False), ("ui_lantern_lit.png", True)):
        c = Cv(64, 64)
        c.rrect(18, 14, 46, 52, 8, (60, 66, 108))
        c.rrect(21, 17, 43, 49, 6, AMBER if lit else (40, 44, 78))
        if lit:
            c.ell(32, 34, 8, 12, GOLD_H)
        c.poly([(14, 16), (50, 16), (42, 6), (22, 6)], (96, 108, 160))
        c.ell(32, 5, 5, 5, None, outline=(96, 108, 160), width=2)
        c.rrect(16, 50, 48, 58, 3, (96, 108, 160))
        save(c.done(), name)
    # светлячок HUD
    save(glow_disc(48, (255, 235, 140), 1.5, 0.9), "ui_firefly.png")
    # большая лампа для заставки
    c = Cv(256, 256)
    c.rrect(66, 62, 190, 214, 30, (40, 46, 84))
    c.rrect(76, 72, 180, 204, 24, AMBER)
    c.rrect(84, 80, 172, 196, 20, GOLD)
    c.ell(128, 140, 36, 48, GOLD_H)
    c.ell(110, 98, 9, 14, (255, 252, 225, 200))
    c.poly([(50, 70), (206, 70), (170, 28), (86, 28)], (84, 94, 146))
    c.ell(128, 22, 18, 18, None, outline=(84, 94, 146), width=7)
    c.rrect(56, 208, 200, 236, 10, (84, 94, 146))
    c.ell(112, 138, 6, 11, INK); c.ell(144, 138, 6, 11, INK)
    c.line([(114, 160), (128, 170), (142, 160)], INK, 3.2)
    big = c.done()
    big = Image.alpha_composite(glow_disc(256, (255, 190, 90), 2.2, 0.6), big)
    save(big, "title_lantern.png")
    # сенсорные кнопки
    for name, glyph in (("touch_left.png", "L"), ("touch_right.png", "R"), ("touch_jump.png", "J"), ("touch_dash.png", "D")):
        c = Cv(96, 96)
        c.ell(48, 48, 44, 44, (255, 255, 255, 70))
        c.ell(48, 48, 40, 40, (20, 24, 50, 170))
        w = (255, 255, 255, 230)
        if glyph == "L":
            c.poly([(62, 26), (28, 48), (62, 70)], w)
        elif glyph == "R":
            c.poly([(34, 26), (68, 48), (34, 70)], w)
        elif glyph == "J":
            c.poly([(48, 22), (74, 54), (22, 54)], w)
            c.rrect(40, 52, 56, 74, 2, w)
        else:
            c.poly([(40, 26), (72, 48), (40, 70)], w)
            c.poly([(18, 32), (44, 48), (18, 64)], (255, 255, 255, 140))
        save(c.done(), name)
    # пятно виньетки-затемнения для экранов
    arr = np.zeros((64, 64, 4), np.uint8)
    arr[..., 0] = 6; arr[..., 1] = 8; arr[..., 2] = 22; arr[..., 3] = 255
    save(Image.fromarray(arr, "RGBA"), "ui_dark.png")


def main():
    os.makedirs(OUT, exist_ok=True)
    make_player()
    save(sheet([crawler_frame(i) for i in range(4)], 4), "crawler.png")
    save(sheet([bat_frame(i) for i in range(4)], 4), "bat.png")
    save(sheet([spitter_frame(i) for i in range(4)], 4), "spitter.png")
    save(sheet([brute_frame(i) for i in range(4)], 4), "brute.png")
    save(sheet([boss_frame(i) for i in range(8)], 4), "boss.png")
    make_firefly()
    make_checkpoint()
    make_gate()
    make_exit()
    make_spring()
    make_platforms()
    make_projectile()
    make_rock()
    make_glow()
    make_tileset()
    make_bg("bg_far.png", 11, (14, 20, 52), (8, 10, 30), "far")
    make_bg("bg_mid.png", 12, (30, 40, 84), (30, 40, 84), "mid")
    make_bg("bg_near.png", 13, (30, 40, 84), (30, 40, 84), "near")
    make_ui()


if __name__ == "__main__":
    main()
