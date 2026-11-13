#!/usr/bin/env python3
"""Генератор графики «Роя»: атлас спрайтов, арена, фон, виньетка, иконки улучшений.

Запуск (из каталога games/swarm):  python tools/gen_assets.py

Пишет:
  assets/sprites/atlas.png      один атлас на все игровые спрайты (один батч рендера)
  assets/sprites/arena.png      пол арены
  assets/sprites/space.png      фон за ареной
  assets/ui/vignette.png        красная виньетка низкого здоровья
  assets/ui/icons/<id>.png      иконки карточек улучшений
  assets/**/*.meta              метаданные ассетов с постоянными GUID
  src/atlas.rs                  прямоугольники спрайтов в атласе (для Rust-кода)

Нужны Pillow и numpy. Результат детерминирован: повторный запуск даёт те же файлы.
"""
import hashlib
import math
import os
import random

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
GAME = os.path.dirname(HERE)
ASSETS = os.path.join(GAME, "assets")
SS = 4  # суперсэмплинг: рисуем в 4 раза крупнее и уменьшаем


def hexc(h, a=255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


def mix(c1, c2, t):
    return tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(4))


class Art:
    """Холст спрайта с центром в (0, 0), ось X вправо, Y вниз (как в картинке)."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.im = Image.new("RGBA", (w * SS, h * SS), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.im)

    def X(self, x):
        return (self.w / 2 + x) * SS

    def Y(self, y):
        return (self.h / 2 + y) * SS

    def pts(self, p):
        return [(self.X(x), self.Y(y)) for x, y in p]

    def poly(self, p, fill=None, outline=None, width=2.0):
        pp = self.pts(p)
        if fill:
            self.d.polygon(pp, fill=fill)
        if outline:
            self.d.line(pp + [pp[0], pp[1]], fill=outline, width=int(width * SS), joint="curve")

    def circle(self, cx, cy, r, fill=None, outline=None, width=2.0):
        box = [self.X(cx - r), self.Y(cy - r), self.X(cx + r), self.Y(cy + r)]
        self.d.ellipse(box, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def ellipse(self, cx, cy, rx, ry, fill=None, outline=None, width=2.0):
        box = [self.X(cx - rx), self.Y(cy - ry), self.X(cx + rx), self.Y(cy + ry)]
        self.d.ellipse(box, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def line(self, p, fill, width=2.0):
        self.d.line(self.pts(p), fill=fill, width=int(width * SS), joint="curve")

    def arc(self, cx, cy, r, a0, a1, fill, width=2.0):
        box = [self.X(cx - r), self.Y(cy - r), self.X(cx + r), self.Y(cy + r)]
        self.d.arc(box, a0, a1, fill=fill, width=int(width * SS))

    def rect(self, x0, y0, x1, y1, fill=None, outline=None, width=2.0, radius=0):
        box = [self.X(x0), self.Y(y0), self.X(x1), self.Y(y1)]
        if radius > 0:
            self.d.rounded_rectangle(box, radius=radius * SS, fill=fill, outline=outline,
                                     width=int(width * SS) if outline else 0)
        else:
            self.d.rectangle(box, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def finish(self, glow=None):
        im = self.im.resize((self.w, self.h), Image.LANCZOS)
        if glow:
            col, rad, k = glow
            a = im.split()[3].filter(ImageFilter.GaussianBlur(rad))
            a = a.point(lambda v: min(255, int(v * k)))
            g = Image.new("RGBA", im.size, col[:3] + (0,))
            g.putalpha(a)
            im = Image.alpha_composite(g, im)
        return im


def star_pts(n, r_out, r_in, rot=0.0, cx=0.0, cy=0.0):
    pts = []
    for i in range(n * 2):
        r = r_out if i % 2 == 0 else r_in
        a = rot + math.pi * i / n
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def reg_poly(n, r, rot=0.0, cx=0.0, cy=0.0):
    return [(cx + r * math.cos(rot + 2 * math.pi * i / n), cy + r * math.sin(rot + 2 * math.pi * i / n))
            for i in range(n)]


# ---------------------------------------------------------------------------
# Палитра
# ---------------------------------------------------------------------------
CYAN = hexc("#5df0ff")
CYAN_D = hexc("#123a55")
AMBER = hexc("#ffb02e")
RED = hexc("#ff5a4d")
LIME = hexc("#c8ff3d")
PURPLE = hexc("#a96bff")
GREEN = hexc("#5fd38a")
YELLOW = hexc("#ffd23d")
ORANGE = hexc("#ff8a3d")
TEAL = hexc("#3dd6c8")
PINK = hexc("#ff4da6")
WHITE = (255, 255, 255, 255)


# ---------------------------------------------------------------------------
# Спрайты атласа (смотрят вправо, +X)
# ---------------------------------------------------------------------------
def spr_player():
    a = Art(96, 96)
    # двигатели
    for s in (-1, 1):
        a.rect(-30, s * 15 - 5, -16, s * 15 + 5, fill=hexc("#ff9b3d"), radius=2)
        a.rect(-34, s * 15 - 3, -28, s * 15 + 3, fill=hexc("#ffe2b0"), radius=1)
    # корпус
    a.poly([(38, 0), (-14, -23), (-26, -12), (-19, 0), (-26, 12), (-14, 23)], fill=CYAN_D, outline=CYAN, width=2.6)
    a.poly([(30, 0), (-8, -13), (-14, 0), (-8, 13)], fill=hexc("#1d6f94"), outline=None)
    a.line([(-4, -16), (-20, -9)], fill=CYAN, width=1.6)
    a.line([(-4, 16), (-20, 9)], fill=CYAN, width=1.6)
    # ствол
    a.rect(30, -3.2, 44, 3.2, fill=hexc("#c7fbff"), radius=1.5)
    # кабина
    a.circle(6, 0, 9, fill=hexc("#aef6ff"), outline=WHITE, width=1.5)
    a.circle(7, 0, 4, fill=WHITE)
    return a.finish(glow=(CYAN, 4, 0.9))


def spr_core():
    a = Art(192, 192)
    a.circle(0, 0, 88, fill=hexc("#1b0e08"), outline=AMBER, width=4)
    a.circle(0, 0, 76, outline=hexc("#7a3a10"), width=2)
    for i in range(12):
        ang = i * math.pi / 6
        a.line([(66 * math.cos(ang), 66 * math.sin(ang)), (82 * math.cos(ang), 82 * math.sin(ang))], fill=AMBER,
               width=3)
    a.circle(0, 0, 58, fill=hexc("#2e1608"), outline=hexc("#ff7a1f"), width=3)
    a.circle(0, 0, 42, fill=hexc("#7a2c08"))
    a.circle(0, 0, 32, fill=hexc("#ff7a1f"))
    a.circle(0, 0, 22, fill=hexc("#ffc060"))
    a.circle(0, 0, 11, fill=hexc("#fff5d0"))
    return a.finish(glow=(AMBER, 6, 0.8))


def spr_core_ring():
    a = Art(192, 192)
    for i in range(12):
        a0 = i * 30 + 4
        a.arc(0, 0, 91, a0, a0 + 18, fill=CYAN, width=3)
    for i in range(6):
        a0 = i * 60 + 20
        a.arc(0, 0, 100 - 18, a0, a0 + 26, fill=hexc("#ffd9a0"), width=2)
    return a.finish(glow=(CYAN, 3, 0.7))


def spr_chaser():
    a = Art(80, 80)
    a.poly(star_pts(8, 37, 22, math.pi / 8), fill=hexc("#4a0f16"), outline=RED, width=2.6)
    a.circle(0, 0, 17, fill=hexc("#7a1a22"), outline=hexc("#ff9a8c"), width=1.5)
    a.circle(2, 0, 9, fill=hexc("#ffe0d8"))
    a.circle(4, 0, 4.5, fill=hexc("#2a0508"))
    return a.finish(glow=(RED, 3, 0.8))


def spr_swarmling():
    a = Art(48, 48)
    a.poly([(21, 0), (-14, -13), (-8, 0), (-14, 13)], fill=hexc("#33460a"), outline=LIME, width=2.2)
    a.circle(6, 0, 3.2, fill=hexc("#f4ffc0"))
    return a.finish(glow=(LIME, 2.2, 0.8))


def spr_shooter():
    a = Art(88, 88)
    a.circle(-8, -27, 8, fill=hexc("#34186a"), outline=PURPLE, width=2)
    a.circle(-8, 27, 8, fill=hexc("#34186a"), outline=PURPLE, width=2)
    a.poly(reg_poly(6, 29, 0.0), fill=hexc("#251150"), outline=PURPLE, width=2.8)
    a.rect(18, -6, 42, 6, fill=hexc("#6a3fd0"), outline=hexc("#d9c4ff"), width=1.5, radius=2)
    a.circle(0, 0, 11, fill=hexc("#3b1d80"), outline=hexc("#d9c4ff"), width=1.5)
    a.circle(1, 0, 5, fill=hexc("#f0e4ff"))
    return a.finish(glow=(PURPLE, 3, 0.8))


def spr_tank():
    a = Art(128, 128)
    a.poly(reg_poly(8, 56, math.pi / 8), fill=hexc("#0f3226"), outline=GREEN, width=3.6)
    a.poly(reg_poly(8, 42, math.pi / 8), fill=hexc("#154633"), outline=hexc("#2b8a5c"), width=2)
    for i in range(4):
        ang = i * math.pi / 2 + math.pi / 4
        cx, cy = 46 * math.cos(ang), 46 * math.sin(ang)
        a.circle(cx, cy, 6, fill=hexc("#0a2018"), outline=hexc("#9ff0c4"), width=1.6)
    a.rect(6, -6, 52, 6, fill=hexc("#1f6a4a"), outline=hexc("#9ff0c4"), width=1.8, radius=2)
    a.circle(0, 0, 22, fill=hexc("#1d5a40"), outline=hexc("#9ff0c4"), width=2.4)
    a.circle(0, 0, 9, fill=hexc("#e8fff2"))
    return a.finish(glow=(GREEN, 3.5, 0.7))


def spr_splitter():
    a = Art(96, 96)
    a.circle(-14, -8, 21, fill=hexc("#4a3a08"), outline=YELLOW, width=2.4)
    a.circle(15, 9, 23, fill=hexc("#4a3a08"), outline=YELLOW, width=2.4)
    a.circle(0, 0, 27, fill=hexc("#5a4609"), outline=YELLOW, width=2.8)
    a.arc(0, 0, 18, 200, 340, fill=hexc("#ffe88a"), width=2)
    a.arc(0, 0, 18, 20, 160, fill=hexc("#ffe88a"), width=2)
    a.circle(-6, -2, 5, fill=hexc("#fff3c0"))
    a.circle(8, 4, 5, fill=hexc("#fff3c0"))
    return a.finish(glow=(YELLOW, 3, 0.8))


def spr_splitter_mini():
    a = Art(56, 56)
    a.circle(0, 0, 19, fill=hexc("#4a3a08"), outline=hexc("#ffe88a"), width=2.2)
    a.circle(2, 0, 6, fill=hexc("#fff3c0"))
    return a.finish(glow=(YELLOW, 2.4, 0.8))


def spr_bomber():
    a = Art(88, 88)
    a.circle(0, 4, 29, fill=hexc("#3a1208"), outline=ORANGE, width=3)
    for k in (-1, 0, 1):
        a.line([(-22 + k * 14, 26), (-6 + k * 14, -18)], fill=hexc("#7a3a10"), width=4)
    a.circle(0, 4, 13, fill=hexc("#ff5a2a"))
    a.circle(0, 4, 7, fill=hexc("#fff3d0"))
    a.line([(0, -25), (8, -36)], fill=hexc("#c9a46a"), width=2.2)
    a.poly(star_pts(4, 8, 2.5, 0.3, 9, -37), fill=hexc("#ffe08a"))
    return a.finish(glow=(ORANGE, 3, 0.85))


def spr_minelayer():
    a = Art(96, 96)
    for s in (-1, 1):
        a.line([(10, s * 16), (28, s * 30), (36, s * 22)], fill=TEAL, width=3)
        a.line([(-6, s * 15), (-22, s * 30)], fill=hexc("#1f8a82"), width=2.4)
    a.ellipse(0, 0, 30, 21, fill=hexc("#0e3a38"), outline=TEAL, width=3)
    a.rect(-34, -8, -14, 8, fill=hexc("#07201f"), outline=hexc("#9ff5ec"), width=1.6, radius=2)
    a.circle(14, 0, 8, fill=hexc("#9ff5ec"))
    a.circle(16, 0, 3.5, fill=hexc("#04201e"))
    return a.finish(glow=(TEAL, 3, 0.8))


def spr_mine():
    a = Art(56, 56)
    a.poly(star_pts(8, 24, 14, 0.0), fill=hexc("#2b0f14"), outline=hexc("#ff3d5a"), width=2.2)
    a.circle(0, 0, 11, fill=hexc("#16070a"), outline=hexc("#7a2230"), width=1.6)
    a.circle(0, 0, 5, fill=hexc("#ff3d5a"))
    return a.finish(glow=(hexc("#ff3d5a"), 2.2, 0.7))


def spr_boss_hive():
    a = Art(256, 256)
    for i in range(6):
        ang = i * math.pi / 3 + math.pi / 6
        cx, cy = 96 * math.cos(ang), 96 * math.sin(ang)
        a.circle(cx, cy, 30, fill=hexc("#4a1237"), outline=PINK, width=4)
        a.circle(cx, cy, 12, fill=hexc("#ffb0d8"))
    a.circle(0, 0, 100, fill=hexc("#3a0f2a"), outline=PINK, width=6)
    a.circle(0, 0, 84, outline=hexc("#8a2a66"), width=3)
    for i in range(9):
        ang = i * 2 * math.pi / 9 + 0.2
        a.line([(40 * math.cos(ang), 40 * math.sin(ang)), (80 * math.cos(ang + 0.15), 80 * math.sin(ang + 0.15))],
               fill=hexc("#8a2a66"), width=3)
    a.circle(0, 0, 44, fill=hexc("#ffd0ea"), outline=WHITE, width=3)
    a.circle(6, 0, 22, fill=hexc("#5a0a30"))
    a.circle(12, -6, 7, fill=WHITE)
    return a.finish(glow=(PINK, 6, 0.8))


def spr_boss_ram():
    a = Art(256, 256)
    a.poly([(116, 0), (44, -76), (-70, -96), (-116, -54), (-116, 54), (-70, 96), (44, 76)],
           fill=hexc("#2a1c0a"), outline=hexc("#ff9a2e"), width=6)
    a.poly([(100, 0), (40, -60), (-60, -76), (-96, -44), (-96, 44), (-60, 76), (40, 60)],
           fill=hexc("#3a280e"), outline=hexc("#8a5a1c"), width=3)
    # рога
    a.poly([(108, -22), (118, -62), (86, -40)], fill=hexc("#c9792a"), outline=hexc("#ffd29a"), width=3)
    a.poly([(108, 22), (118, 62), (86, 40)], fill=hexc("#c9792a"), outline=hexc("#ffd29a"), width=3)
    for x in (-70, -30, 10):
        a.line([(x, -70), (x, 70)], fill=hexc("#8a5a1c"), width=3)
    a.poly([(70, -12), (40, -4), (40, 4), (70, 12)], fill=hexc("#ff5a2a"), outline=hexc("#ffd29a"), width=2)
    a.circle(-95, 0, 14, fill=hexc("#ff7a2a"), outline=hexc("#ffe0b0"), width=2)
    return a.finish(glow=(hexc("#ff9a2e"), 6, 0.7))


def spr_boss_conductor():
    a = Art(256, 256)
    a.poly(reg_poly(4, 104, 0.0), fill=hexc("#10183a"), outline=hexc("#4d7bff"), width=6)
    a.poly(reg_poly(4, 80, math.pi / 4), outline=hexc("#7aa0ff"), width=3)
    a.poly(reg_poly(4, 62, 0.0), fill=hexc("#1a2a6a"), outline=hexc("#9ab8ff"), width=3)
    for i in range(4):
        ang = i * math.pi / 2 + math.pi / 4
        cx, cy = 80 * math.cos(ang), 80 * math.sin(ang)
        a.poly(reg_poly(4, 14, 0.0, cx, cy), fill=hexc("#4d7bff"), outline=WHITE, width=2)
        a.line([(30 * math.cos(ang), 30 * math.sin(ang)), (66 * math.cos(ang), 66 * math.sin(ang))],
               fill=hexc("#7aa0ff"), width=3)
    a.circle(0, 0, 28, fill=hexc("#cfe0ff"), outline=WHITE, width=3)
    a.circle(0, 0, 11, fill=hexc("#1a2a6a"))
    return a.finish(glow=(hexc("#4d7bff"), 6, 0.8))


def spr_boss_overmind():
    a = Art(256, 256)
    a.poly(star_pts(12, 124, 100, 0.0), fill=hexc("#2a0a0a"), outline=hexc("#ff3d3d"), width=5)
    a.circle(0, 0, 92, fill=hexc("#1c0606"), outline=hexc("#ff6a5a"), width=4)
    for i in range(16):
        a.arc(0, 0, 78, i * 22.5, i * 22.5 + 12, fill=hexc("#ff9a8a"), width=4)
    a.circle(0, 0, 62, fill=hexc("#7a1010"), outline=hexc("#ff5a4d"), width=3)
    a.circle(0, 0, 44, fill=hexc("#ff3d3d"))
    a.ellipse(0, 0, 12, 40, fill=hexc("#150303"))
    a.circle(-12, -16, 7, fill=WHITE)
    return a.finish(glow=(hexc("#ff3d3d"), 7, 0.85))


def spr_pbullet():
    a = Art(40, 16)
    a.ellipse(0, 0, 17, 4.8, fill=hexc("#4adfff"))
    a.ellipse(2, 0, 12, 2.8, fill=WHITE)
    return a.finish(glow=(CYAN, 2.2, 1.0))


def spr_ebullet():
    a = Art(32, 32)
    a.circle(0, 0, 9, fill=hexc("#ff3d7a"))
    a.circle(0, 0, 5, fill=hexc("#ffd0e0"))
    a.circle(0, 0, 2, fill=WHITE)
    return a.finish(glow=(hexc("#ff3d7a"), 2.6, 1.0))


def spr_missile():
    a = Art(56, 28)
    a.poly([(-18, -5), (-26, -11), (-22, 0), (-26, 11), (-18, 5)], fill=hexc("#c97a1a"))
    a.rect(-18, -5.5, 16, 5.5, fill=hexc("#dfe8f0"), outline=hexc("#7a8a9a"), width=1.2, radius=3)
    a.poly([(16, -5.5), (25, 0), (16, 5.5)], fill=hexc("#ff4d4d"))
    a.ellipse(-27, 0, 3.5, 2.5, fill=hexc("#ffd070"))
    return a.finish(glow=(ORANGE, 2.2, 0.8))


def spr_ring():
    a = Art(128, 128)
    a.circle(0, 0, 58, outline=WHITE, width=4)
    a.circle(0, 0, 52, outline=(255, 255, 255, 90), width=2)
    return a.finish(glow=(WHITE, 2.5, 0.8))


def spr_glow():
    n = 96
    y, x = np.mgrid[0:n, 0:n].astype(np.float32)
    r = np.hypot(x - n / 2 + 0.5, y - n / 2 + 0.5) / (n / 2)
    a = np.clip(1.0 - r, 0, 1) ** 2.0
    img = np.zeros((n, n, 4), np.uint8)
    img[..., :3] = 255
    img[..., 3] = (a * 255).astype(np.uint8)
    return Image.fromarray(img, "RGBA")


def spr_bar():
    # белая полоса с мягкими краями поперёк (для линий-предупреждений)
    w, h = 32, 32
    y = np.abs(np.linspace(-1, 1, h, dtype=np.float32))
    a = np.clip(1.0 - y, 0, 1) ** 0.6
    img = np.zeros((h, w, 4), np.uint8)
    img[..., :3] = 255
    img[..., 3] = (np.tile(a[:, None], (1, w)) * 255).astype(np.uint8)
    return Image.fromarray(img, "RGBA")


def spr_telegraph():
    a = Art(112, 112)
    for i in range(8):
        a.arc(0, 0, 46, i * 45 + 4, i * 45 + 30, fill=WHITE, width=3.5)
    for i in range(4):
        ang = i * math.pi / 2 + math.pi / 4
        p0 = (30 * math.cos(ang), 30 * math.sin(ang))
        p1 = (18 * math.cos(ang), 18 * math.sin(ang))
        a.poly([(p1[0] + (p0[1] - p1[1]) * 0.35, p1[1] - (p0[0] - p1[0]) * 0.35),
                (p1[0] - (p0[1] - p1[1]) * 0.35, p1[1] + (p0[0] - p1[0]) * 0.35), p0], fill=(255, 255, 255, 200))
    a.circle(0, 0, 4, fill=WHITE)
    return a.finish(glow=(WHITE, 2.5, 0.6))


def spr_scrap():
    a = Art(32, 32)
    a.poly([(0, -12), (9, 0), (0, 12), (-9, 0)], fill=hexc("#4dffb0"), outline=WHITE, width=1.5)
    a.poly([(0, -6), (4, 0), (0, 6), (-4, 0)], fill=hexc("#c8ffe8"))
    return a.finish(glow=(hexc("#4dffb0"), 1.8, 0.9))


def spr_repair():
    a = Art(48, 48)
    a.circle(0, 0, 17, fill=hexc("#0c2a14"), outline=hexc("#5dff7a"), width=2)
    a.rect(-3.5, -11, 3.5, 11, fill=hexc("#5dff7a"), radius=1)
    a.rect(-11, -3.5, 11, 3.5, fill=hexc("#5dff7a"), radius=1)
    return a.finish(glow=(hexc("#5dff7a"), 2.4, 0.9))


def spr_shield():
    a = Art(56, 56)
    a.circle(0, 0, 15, fill=hexc("#0f4a66"), outline=WHITE, width=2.2)
    a.circle(0, 0, 8, fill=hexc("#7df0ff"))
    a.circle(-3, -3, 2.5, fill=WHITE)
    return a.finish(glow=(CYAN, 3, 1.0))


def spr_field():
    a = Art(256, 256)
    a.circle(0, 0, 122, fill=(255, 255, 255, 26), outline=(255, 255, 255, 140), width=3)
    for i in range(24):
        a.arc(0, 0, 108, i * 15 + 2, i * 15 + 8, fill=(255, 255, 255, 120), width=2)
    return a.finish()


def spr_dot():
    a = Art(16, 16)
    a.circle(0, 0, 6, fill=WHITE)
    return a.finish()


# Семисегментные цифры для чисел урона: 32x48, белые (красятся цветом спрайта).
SEGMENTS = {
    "a": [(-9, -20), (9, -20), (6, -17), (-6, -17)],
    "b": [(10, -19), (10, -2), (7, -1), (7, -16)],
    "c": [(10, 2), (10, 19), (7, 16), (7, 1)],
    "d": [(-9, 20), (9, 20), (6, 17), (-6, 17)],
    "e": [(-10, 2), (-10, 19), (-7, 16), (-7, 1)],
    "f": [(-10, -19), (-10, -2), (-7, -1), (-7, -16)],
    "g": [(-8, 0), (-5, -2.5), (5, -2.5), (8, 0), (5, 2.5), (-5, 2.5)],
}
DIGIT_SEGS = ["abcdef", "bc", "abdeg", "abcdg", "bcfg", "acdfg", "acdefg", "abc", "abcdefg", "abcdfg"]


def spr_digit(n):
    a = Art(32, 48)
    for s in DIGIT_SEGS[n]:
        a.poly(SEGMENTS[s], fill=WHITE)
    return a.finish(glow=(WHITE, 1.6, 0.5))


SPRITES = [
    # (имя, функция)
    ("player", spr_player), ("core", spr_core), ("core_ring", spr_core_ring),
    ("chaser", spr_chaser), ("swarmling", spr_swarmling), ("shooter", spr_shooter), ("tank", spr_tank),
    ("splitter", spr_splitter), ("splitter_mini", spr_splitter_mini), ("bomber", spr_bomber),
    ("minelayer", spr_minelayer), ("mine", spr_mine),
    ("boss_hive", spr_boss_hive), ("boss_ram", spr_boss_ram), ("boss_conductor", spr_boss_conductor),
    ("boss_overmind", spr_boss_overmind),
    ("pbullet", spr_pbullet), ("ebullet", spr_ebullet), ("missile", spr_missile),
    ("ring", spr_ring), ("glow", spr_glow), ("bar", spr_bar), ("telegraph", spr_telegraph),
    ("scrap", spr_scrap), ("repair", spr_repair), ("shield", spr_shield), ("field", spr_field), ("dot", spr_dot),
] + [(f"digit{n}", (lambda n=n: spr_digit(n))) for n in range(10)]


def build_atlas():
    """Складывает спрайты полками в атлас 1024 пикселя шириной; зазор 3 пикселя."""
    pad = 3
    images = [(name, fn()) for name, fn in SPRITES]
    images.sort(key=lambda it: -it[1].size[1])
    width = 1024
    x = y = pad
    shelf_h = 0
    placed = []
    for name, im in images:
        w, h = im.size
        if x + w + pad > width:
            x = pad
            y += shelf_h + pad
            shelf_h = 0
        placed.append((name, im, x, y))
        x += w + pad
        shelf_h = max(shelf_h, h)
    height = 1
    need = y + shelf_h + pad
    while height < need:
        height *= 2
    atlas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    rects = {}
    for name, im, px, py in placed:
        atlas.paste(im, (px, py))
        rects[name] = (px, py, im.size[0], im.size[1])
    return atlas, rects


# ---------------------------------------------------------------------------
# Пол арены, фон, виньетка
# ---------------------------------------------------------------------------
def smooth(edge0, edge1, x):
    t = np.clip((x - edge0) / (edge1 - edge0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def build_arena():
    n = 1024
    y, x = np.mgrid[0:n, 0:n].astype(np.float32)
    cx = cy = (n - 1) / 2
    dx, dy = x - cx, y - cy
    r = np.hypot(dx, dy)
    R = 460.0  # 9 единиц из 10 половинной ширины (1024 пикселя = 20 единиц)
    inside = 1.0 - smooth(R - 1.2, R + 1.2, r)
    base_c = np.array([16, 27, 52], np.float32)
    base_e = np.array([9, 16, 36], np.float32)
    t = np.clip(r / R, 0, 1)[..., None]
    col = base_c * (1 - t) + base_e * t
    # сетка 64 пикселя (чуть больше единицы), затухает к краю
    gx = np.abs(((x - cx) % 51.2) - 25.6)
    gy = np.abs(((y - cy) % 51.2) - 25.6)
    line = np.maximum(1.0 - smooth(0.0, 1.6, 25.6 - gx), 1.0 - smooth(0.0, 1.6, 25.6 - gy))
    col += (np.array([26, 44, 88], np.float32) - col) * (line[..., None] * 0.55 * (1 - t * 0.5))
    # концентрические кольца
    for rr, w, c, k in ((115, 1.6, (36, 70, 130), 0.9), (230, 1.6, (36, 70, 130), 0.6), (345, 1.6, (36, 70, 130), 0.45)):
        ring = 1.0 - smooth(0.0, w, np.abs(r - rr))
        col += (np.array(c, np.float32) - col) * (ring[..., None] * k)
    # площадка реактора
    pad = 1.0 - smooth(98, 100, r)
    col = col * (1 - pad[..., None] * 0.55) + np.array([42, 22, 12], np.float32) * (pad[..., None] * 0.55)
    padring = 1.0 - smooth(0, 2.2, np.abs(r - 100))
    col += (np.array([255, 150, 50], np.float32) - col) * (padring[..., None] * 0.55)
    # кромка арены: тёмная полоса и светящийся бортик
    band = smooth(R - 26, R - 2, r)
    col = col * (1 - band[..., None] * 0.55)
    stripe = ((dx + dy) % 36 < 18).astype(np.float32) * band * (r > R - 22)
    col += (np.array([60, 90, 140], np.float32) - col) * (stripe[..., None] * 0.18)
    rim = 1.0 - smooth(0, 3.6, np.abs(r - (R - 3)))
    col += (np.array([93, 240, 255], np.float32) - col) * (rim[..., None] * 0.95)
    glow = np.exp(-np.maximum(r - R, 0) / 9.0) * (r > R)
    out = np.zeros((n, n, 4), np.float32)
    out[..., :3] = col
    out[..., 3] = inside * 255
    # внешнее свечение
    out_glow = glow * 0.55
    gcol = np.array([93, 240, 255], np.float32)
    mask_out = (r > R)[..., None]
    out[..., :3] = np.where(mask_out, gcol, out[..., :3])
    out[..., 3] = np.where(r > R, out_glow * 255, out[..., 3])
    # на самой границе смешиваем аккуратно
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


def build_space():
    rng = np.random.RandomState(7)
    w, h = 1024, 576
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    r = np.hypot((x - w / 2) / (w / 2), (y - h / 2) / (h / 2))
    col = np.zeros((h, w, 3), np.float32)
    col[...] = np.array([4, 6, 14], np.float32) + (np.array([10, 14, 36], np.float32) * np.clip(1.2 - r, 0, 1)[..., None])
    # туманности: низкочастотный шум
    for seed, tint, k in ((1, (60, 20, 90), 0.5), (2, (20, 50, 100), 0.5)):
        rs = np.random.RandomState(seed)
        small = rs.rand(9, 16).astype(np.float32)
        big = np.array(Image.fromarray((small * 255).astype(np.uint8)).resize((w, h), Image.BICUBIC), np.float32) / 255
        big = (big - 0.5).clip(0, 1) * 2
        col += big[..., None] * np.array(tint, np.float32) * k * 0.35
    img = Image.fromarray(np.clip(col, 0, 255).astype(np.uint8), "RGB").convert("RGBA")
    px = img.load()
    for _ in range(420):
        sx, sy = rng.randint(0, w), rng.randint(0, h)
        b = rng.randint(80, 255)
        tint = rng.choice([0, 1, 2])
        c = (b, b, 255) if tint == 0 else ((255, b, b) if tint == 1 else (b, b, b))
        px[sx, sy] = c + (255,)
        if b > 200 and rng.rand() < 0.5:
            for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                if 0 <= sx + ox < w and 0 <= sy + oy < h:
                    px[sx + ox, sy + oy] = (b // 2, b // 2, b // 2 + 40, 255)
    return img


def build_vignette():
    n = 256
    y, x = np.mgrid[0:n, 0:n].astype(np.float32)
    r = np.hypot((x - n / 2) / (n / 2), (y - n / 2) / (n / 2))
    a = smooth(0.55, 1.25, r) ** 1.4
    img = np.zeros((n, n, 4), np.uint8)
    img[..., 0] = 255
    img[..., 1] = 20
    img[..., 2] = 30
    img[..., 3] = (a * 255).astype(np.uint8)
    return Image.fromarray(img, "RGBA")


# ---------------------------------------------------------------------------
# Иконки улучшений (96x96)
# ---------------------------------------------------------------------------
def icon_base(col):
    a = Art(96, 96)
    a.circle(0, 0, 44, fill=(14, 22, 44, 255), outline=col[:3] + (90,), width=2)
    return a


def ic_rate(col):
    a = icon_base(col)
    for i, y in enumerate((-18, 0, 18)):
        a.ellipse(-4 - i * 2, y, 22 - i * 2, 4, fill=col)
        a.line([(-34, y), (-26 - i * 2, y)], fill=col[:3] + (120,), width=2)
    return a.finish(glow=(col, 3, 0.7))


def ic_damage(col):
    a = icon_base(col)
    a.poly(star_pts(8, 34, 14, 0.2), fill=col, outline=WHITE, width=1.6)
    a.circle(0, 0, 7, fill=WHITE)
    return a.finish(glow=(col, 3, 0.7))


def ic_multishot(col):
    a = icon_base(col)
    for ang in (-28, 0, 28):
        r = math.radians(ang)
        a.line([(-24, 0), (30 * math.cos(r) - 2, 30 * math.sin(r))], fill=col, width=4)
        a.circle(32 * math.cos(r), 32 * math.sin(r), 4.5, fill=WHITE)
    a.circle(-26, 0, 6, fill=col)
    return a.finish(glow=(col, 3, 0.7))


def ic_pierce(col):
    a = icon_base(col)
    for x in (-8, 8, 24):
        a.rect(x - 3, -22, x + 3, 22, fill=(70, 90, 130, 255), outline=col, width=1.4)
    a.line([(-34, 0), (30, 0)], fill=WHITE, width=4)
    a.poly([(36, 0), (24, -9), (24, 9)], fill=WHITE)
    return a.finish(glow=(col, 3, 0.7))


def ic_ricochet(col):
    a = icon_base(col)
    a.line([(-32, -26), (-4, 24), (22, -22), (34, 6)], fill=col, width=4)
    a.poly([(38, 14), (28, 4), (40, 0)], fill=WHITE)
    a.line([(-38, 26), (38, 26)], fill=(150, 170, 210, 255), width=3)
    return a.finish(glow=(col, 3, 0.7))


def ic_explosive(col):
    a = icon_base(col)
    a.poly(star_pts(10, 36, 18, 0.1), fill=col, outline=WHITE, width=1.4)
    a.circle(0, 0, 13, fill=hexc("#fff0b0"))
    a.circle(0, 0, 6, fill=WHITE)
    return a.finish(glow=(col, 3.5, 0.8))


def ic_shield(col):
    a = icon_base(col)
    a.circle(0, 0, 12, fill=hexc("#ffb02e"))
    a.circle(0, 0, 28, outline=col[:3] + (120,), width=2)
    for i in range(3):
        ang = i * 2 * math.pi / 3 - 0.6
        a.circle(28 * math.cos(ang), 28 * math.sin(ang), 8, fill=col, outline=WHITE, width=1.5)
    return a.finish(glow=(col, 3, 0.7))


def ic_missile(col):
    a = icon_base(col)
    a.poly([(-34, 0), (-22, -12), (-18, 0), (-22, 12)], fill=hexc("#ff9a3d"))
    a.rect(-24, -8, 20, 8, fill=hexc("#dfe8f0"), outline=col, width=1.6, radius=4)
    a.poly([(20, -8), (36, 0), (20, 8)], fill=hexc("#ff4d4d"))
    a.line([(-34, -22), (-8, -22)], fill=col[:3] + (110,), width=2)
    a.line([(-34, 22), (-8, 22)], fill=col[:3] + (110,), width=2)
    return a.finish(glow=(col, 3, 0.7))


def ic_dashdmg(col):
    a = icon_base(col)
    a.poly([(-22, -8), (4, -8), (4, -22), (34, 0), (4, 22), (4, 8), (-22, 8)], fill=col, outline=WHITE, width=1.6)
    a.line([(-38, -14), (-26, -14)], fill=col[:3] + (150,), width=3)
    a.line([(-38, 14), (-26, 14)], fill=col[:3] + (150,), width=3)
    return a.finish(glow=(col, 3, 0.7))


def ic_magnet(col):
    a = icon_base(col)
    a.arc(0, 2, 22, 180, 360, fill=col, width=10)
    a.rect(-27, 2, -17, 24, fill=hexc("#ff4d4d"), radius=1)
    a.rect(17, 2, 27, 24, fill=WHITE, radius=1)
    for dx, dy in ((-30, -20), (30, -20), (0, -36)):
        a.poly([(dx, dy - 5), (dx + 5, dy), (dx, dy + 5), (dx - 5, dy)], fill=hexc("#4dffb0"))
    return a.finish(glow=(col, 3, 0.7))


def ic_hull(col):
    a = icon_base(col)
    a.poly([(0, -32), (28, -18), (28, 8), (0, 32), (-28, 8), (-28, -18)], fill=hexc("#1d3a52"), outline=col, width=2.6)
    a.rect(-4, -16, 4, 16, fill=WHITE, radius=1)
    a.rect(-16, -4, 16, 4, fill=WHITE, radius=1)
    return a.finish(glow=(col, 3, 0.7))


def ic_repair(col):
    a = icon_base(col)
    a.circle(0, 0, 22, fill=hexc("#7a2c08"), outline=hexc("#ffb02e"), width=3)
    a.circle(0, 0, 11, fill=hexc("#ffc060"))
    a.line([(-34, 30), (-8, 4)], fill=WHITE, width=6)
    a.circle(-8, 4, 8, outline=WHITE, width=3)
    a.rect(14, -34, 22, -22, fill=col)
    return a.finish(glow=(col, 3, 0.7))


def ic_slow(col):
    a = icon_base(col)
    for ang in range(0, 180, 60):
        r = math.radians(ang)
        a.line([(-30 * math.cos(r), -30 * math.sin(r)), (30 * math.cos(r), 30 * math.sin(r))], fill=col, width=4)
        for s in (-1, 1):
            tip = (s * 22 * math.cos(r), s * 22 * math.sin(r))
            for t in (-1, 1):
                rr = r + t * 0.9
                a.line([tip, (tip[0] + s * 9 * math.cos(rr), tip[1] + s * 9 * math.sin(rr))], fill=col, width=3)
    a.circle(0, 0, 6, fill=WHITE)
    return a.finish(glow=(col, 3, 0.7))


def ic_crit(col):
    a = icon_base(col)
    a.circle(0, 0, 26, outline=col, width=3.5)
    a.line([(-36, 0), (-14, 0)], fill=col, width=3.5)
    a.line([(36, 0), (14, 0)], fill=col, width=3.5)
    a.line([(0, -36), (0, -14)], fill=col, width=3.5)
    a.line([(0, 36), (0, 14)], fill=col, width=3.5)
    a.poly(star_pts(4, 14, 4, 0.0), fill=WHITE)
    return a.finish(glow=(col, 3, 0.7))


def ic_thrusters(col):
    a = icon_base(col)
    for dy in (-14, 14):
        a.poly([(-30, dy - 8), (-6, dy - 8), (4, dy), (-6, dy + 8), (-30, dy + 8)], fill=hexc("#1d3a52"), outline=col,
               width=2)
        a.poly([(-30, dy - 5), (-44, dy), (-30, dy + 5)], fill=hexc("#ff9b3d"))
    a.poly([(8, -22), (30, 0), (8, 22), (14, 0)], fill=WHITE)
    return a.finish(glow=(col, 3, 0.7))


def ic_dashcd(col):
    a = icon_base(col)
    a.circle(0, 0, 28, outline=col, width=4)
    a.line([(0, 0), (0, -17)], fill=WHITE, width=3.5)
    a.line([(0, 0), (12, 8)], fill=WHITE, width=3.5)
    a.poly([(24, -34), (38, -26), (24, -18)], fill=WHITE)
    return a.finish(glow=(col, 3, 0.7))


def ic_armor(col):
    a = icon_base(col)
    a.circle(0, 0, 30, fill=hexc("#2e1608"), outline=hexc("#ffb02e"), width=3)
    a.poly(reg_poly(6, 20, math.pi / 6), fill=hexc("#7a2c08"), outline=col, width=2.5)
    a.circle(0, 0, 8, fill=hexc("#ffc060"))
    return a.finish(glow=(col, 3, 0.7))


def ic_siphon(col):
    a = icon_base(col)
    a.poly([(0, -34), (22, 4), (22, 14), (12, 26), (-12, 26), (-22, 14), (-22, 4)], fill=col, outline=WHITE, width=1.8)
    a.rect(-3.5, 2, 3.5, 20, fill=WHITE, radius=1)
    a.rect(-12, 8, 12, 14, fill=WHITE, radius=1)
    return a.finish(glow=(col, 3, 0.7))


ICONS = [
    ("rate", ic_rate, hexc("#5df0ff")), ("damage", ic_damage, hexc("#ff6a4d")),
    ("multishot", ic_multishot, hexc("#5df0ff")), ("pierce", ic_pierce, hexc("#b48bff")),
    ("ricochet", ic_ricochet, hexc("#6aa8ff")), ("explosive", ic_explosive, hexc("#ff9a2e")),
    ("shield", ic_shield, hexc("#5df0ff")), ("missile", ic_missile, hexc("#ff9a2e")),
    ("dashdmg", ic_dashdmg, hexc("#ffd23d")), ("magnet", ic_magnet, hexc("#4dffb0")),
    ("hull", ic_hull, hexc("#5dff7a")), ("repair", ic_repair, hexc("#ffb02e")),
    ("slow", ic_slow, hexc("#7aa0ff")), ("crit", ic_crit, hexc("#ffd23d")),
    ("thrusters", ic_thrusters, hexc("#5df0ff")), ("dashcd", ic_dashcd, hexc("#ffd23d")),
    ("armor", ic_armor, hexc("#ffb02e")), ("siphon", ic_siphon, hexc("#5dff7a")),
]


# ---------------------------------------------------------------------------
# Запись файлов, .meta, atlas.rs, сцена
# ---------------------------------------------------------------------------
def guid_of(rel):
    return hashlib.md5(("swarm/" + rel).encode("utf-8")).hexdigest()


def write_meta(rel, importer, settings=None):
    path = os.path.join(ASSETS, rel + ".meta")
    lines = ["(", f'    guid: "{guid_of(rel)}",', f'    importer: "{importer}",']
    if settings:
        lines.append("    settings: {")
        for k, v in settings.items():
            lines.append(f'        "{k}": {v},')
        lines.append("    },")
    lines.append(")")
    with open(path, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(lines))


def save_png(im, rel):
    path = os.path.join(ASSETS, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    im.save(path, optimize=True)
    write_meta(rel, "Texture", {"filter": 'Enum("Linear")'})


def write_atlas_rs(rects, size):
    path = os.path.join(GAME, "src", "atlas.rs")
    out = [
        "//! Прямоугольники спрайтов в атласе `sprites/atlas.png`.",
        "//! Файл создаёт `tools/gen_assets.py` — не правьте его руками.",
        "",
        "/// Прямоугольник спрайта в пикселях атласа (от левого верхнего угла).",
        "#[derive(Clone, Copy, Debug, PartialEq)]",
        "pub struct Rect {",
        "    pub x: f32,",
        "    pub y: f32,",
        "    pub w: f32,",
        "    pub h: f32,",
        "}",
        "",
        f"pub const ATLAS_W: f32 = {size[0]}.0;",
        f"pub const ATLAS_H: f32 = {size[1]}.0;",
        "",
    ]
    for name in sorted(rects):
        x, y, w, h = rects[name]
        out.append(f"pub const {name.upper()}: Rect = Rect {{ x: {x}.0, y: {y}.0, w: {w}.0, h: {h}.0 }};")
    out.append("")
    out.append("/// Цифры 0–9 (семисегментные).")
    out.append("pub const DIGITS: [Rect; 10] = [" + ", ".join(f"DIGIT{n}" for n in range(10)) + "];")
    out.append("")
    with open(path, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(out))


def write_scene():
    atlas_guid = guid_of("sprites/atlas.png")
    space_guid = guid_of("sprites/space.png")
    arena_guid = guid_of("sprites/arena.png")
    text = f"""// Единственная сцена «Роя»: камера со свечением, звёздный фон, пол арены и менеджер игры.
// Всё остальное (спрайты врагов и снарядов, частицы, интерфейс) создаёт Director при старте.
(
    name: "Arena",
    objects: [
        (
            name: "Main Camera",
            tag: "MainCamera",
            components: [
                (kind: "Camera", fields: {{
                    "ortho_size": Float(10.4),
                    "clear_color": Color(0.012, 0.016, 0.04, 1.0),
                }}),
                (kind: "CameraController2D", fields: {{
                    "smoothing": Float(0.0),
                    "shake_decay": Float(2.4),
                    "shake_max_offset": Vec2(0.55, 0.55),
                    "shake_frequency": Float(32.0),
                }}),
                (kind: "AudioListener", fields: {{}}),
            ],
        ),
        (
            name: "Environment",
            components: [
                (kind: "Environment", fields: {{
                    "bloom": Float(0.55),
                    "vignette": Float(0.32),
                    "exposure": Float(1.0),
                }}),
            ],
        ),
        (
            name: "Space",
            components: [
                (kind: "SpriteRenderer", fields: {{
                    "texture": Asset("{space_guid}"),
                    "size": Vec2(41.0, 23.0),
                    "sorting_layer": Int(-20),
                }}),
            ],
        ),
        (
            name: "ArenaFloor",
            components: [
                (kind: "SpriteRenderer", fields: {{
                    "texture": Asset("{arena_guid}"),
                    "size": Vec2(20.0, 20.0),
                    "sorting_layer": Int(-10),
                }}),
            ],
        ),
        (
            name: "Director",
            components: [
                (kind: "Director", fields: {{}}),
            ],
        ),
    ],
)
"""
    os.makedirs(os.path.join(ASSETS, "scenes"), exist_ok=True)
    with open(os.path.join(ASSETS, "scenes", "arena.scene"), "w", newline="\n", encoding="utf-8") as f:
        f.write(text)
    write_meta("scenes/arena.scene", "Scene")


def main():
    atlas, rects = build_atlas()
    save_png(atlas, "sprites/atlas.png")
    write_atlas_rs(rects, atlas.size)
    save_png(build_arena(), "sprites/arena.png")
    save_png(build_space(), "sprites/space.png")
    save_png(build_vignette(), "ui/vignette.png")
    for name, fn, col in ICONS:
        save_png(fn(col), f"ui/icons/{name}.png")
    write_scene()
    print(f"atlas {atlas.size[0]}x{atlas.size[1]}, {len(rects)} sprites, {len(ICONS)} icons")


if __name__ == "__main__":
    main()
