#!/usr/bin/env python3
"""Генератор значка Piska Engine.

Запуск (из корня репозитория):

    python tools/gen_icon.py                       # assets/branding/*
    python tools/gen_icon.py --candidates out.png  # сравнительный лист кандидатов
    python tools/test_gen_icon.py                  # проверка: размеры, .ico, воспроизводимость

Рисует значок в высоком разрешении (суперсэмплинг 4x) и уменьшает фильтром LANCZOS.
Для малых размеров (16 / 24 / 32 px) координаты привязываются к границам пикселей, поэтому
рёбра не размываются. Результат детерминирован: два запуска дают одинаковые байты.
Нужны Python 3 и Pillow.
"""

import argparse
import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "branding"

PNG_SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
# Размеры, для которых геометрия подгоняется под пиксельную сетку.
SNAP_BELOW = 33
SS = 4  # суперсэмплинг


class Canvas:
    """Рисование в нормализованных координатах 0..1 с привязкой к пикселям."""

    def __init__(self, size, snap):
        self.size = size
        self.snap = snap
        self.big = size * SS
        self.img = Image.new("RGBA", (self.big, self.big), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.img)

    def u(self, v):
        x = v * self.big
        if self.snap:
            x = round(x / SS) * SS
        return x

    def pts(self, points):
        return [(self.u(x), self.u(y)) for x, y in points]

    def tile(self, color, margin=1 / 32, radius=0.21):
        a, b = self.u(margin), self.u(1 - margin)
        # Правый/нижний край включительно: -1 в сверхдискретных координатах.
        self.d.rounded_rectangle([a, a, b - 1, b - 1], radius=self.u(radius), fill=color)

    def rect(self, x0, y0, x1, y1, color):
        self.d.rectangle([self.u(x0), self.u(y0), self.u(x1) - 1, self.u(y1) - 1], fill=color)

    def circle(self, cx, cy, r, color):
        self.d.ellipse([self.u(cx - r), self.u(cy - r), self.u(cx + r) - 1, self.u(cy + r) - 1], fill=color)

    def poly(self, points, color):
        self.d.polygon(self.pts(points), fill=color)

    def result(self):
        return self.img.resize((self.size, self.size), Image.LANCZOS)


# ---------------------------------------------------------------- кандидаты

def design_monogram(c):
    """A: белая «P» из прямоугольника и полукруга на синей плитке, янтарная искра."""
    tile = (47, 84, 235, 255)
    c.tile(tile)
    white = (255, 255, 255, 255)
    # Ножка и чаша буквы.
    c.rect(0.29, 0.19, 0.46, 0.81, white)
    c.rect(0.46, 0.19, 0.52, 0.57, white)
    c.circle(0.52, 0.38, 0.19, white)
    # Просвет чаши.
    c.rect(0.46, 0.31, 0.53, 0.45, tile)
    c.circle(0.53, 0.38, 0.07, tile)
    # Янтарная искра у основания.
    c.rect(0.58, 0.64, 0.72, 0.78, (255, 176, 32, 255))


def design_cube(c):
    """B: изометрический куб из трёх тонов на тёмной плитке."""
    c.tile((16, 27, 46, 255))
    r, dx = 0.33, 0.33 * math.sqrt(3) / 2
    cx, cy = 0.5, 0.5
    top, ur, lr, bot, ll, ul = (cx, cy - r), (cx + dx, cy - r / 2), (cx + dx, cy + r / 2), (cx, cy + r), (cx - dx, cy + r / 2), (cx - dx, cy - r / 2)
    ctr = (cx, cy)
    c.poly([top, ur, ctr, ul], (232, 244, 255, 255))
    c.poly([ul, ctr, bot, ll], (56, 189, 248, 255))
    c.poly([ctr, ur, lr, bot], (14, 116, 190, 255))


def design_layers(c):
    """C: три стопкой лежащих ромба («слои сцены») на фиолетовой плитке."""
    c.tile((76, 29, 149, 255))
    w, h = 0.34, 0.17
    for cy, color in ((0.64, (192, 132, 252, 255)), (0.50, (251, 146, 60, 255)), (0.36, (255, 255, 255, 255))):
        c.poly([(0.5, cy - h), (0.5 + w, cy), (0.5, cy + h), (0.5 - w, cy)], color)


DESIGNS = {"A_monogram": design_monogram, "B_cube": design_cube, "C_layers": design_layers}
CHOSEN = "A_monogram"


def render(design, size):
    c = Canvas(size, snap=size < SNAP_BELOW)
    design(c)
    return c.result()


# ---------------------------------------------------------------- вывод

def png_bytes(img):
    buf = io.BytesIO()
    # Без метаданных и времени: байты воспроизводимы.
    img.save(buf, format="PNG", optimize=False, compress_level=9)
    return buf.getvalue()


def ico_bytes(images):
    """Многоразмерный .ico с PNG-вложениями (Vista и новее)."""
    entries = []
    data = b""
    offset = 6 + 16 * len(images)
    for img in images:
        png = png_bytes(img)
        w = img.width if img.width < 256 else 0
        entries.append(struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(png), offset + len(data)))
        data += png
    return struct.pack("<HHH", 0, 1, len(images)) + b"".join(entries) + data


def write_branding(design, out=OUT):
    out.mkdir(parents=True, exist_ok=True)
    cache = {s: render(design, s) for s in sorted(set(PNG_SIZES) | set(ICO_SIZES))}
    for s in PNG_SIZES:
        (out / f"icon-{s}.png").write_bytes(png_bytes(cache[s]))
    # Главный файл — 256 px: его встраивают в редактор и окна игр.
    (out / "icon.png").write_bytes(png_bytes(cache[256]))
    (out / "icon.ico").write_bytes(ico_bytes([cache[s] for s in ICO_SIZES]))
    # Фавикон для веб-экспорта.
    (out / "favicon.ico").write_bytes(ico_bytes([cache[16], cache[32], cache[48]]))
    (out / "favicon.png").write_bytes(png_bytes(cache[32]))


def write_candidates(path):
    pad = 24
    row_h = 300
    sheet = Image.new("RGBA", (pad + len(DESIGNS) * (256 + 32 + 16 + 3 * pad), 2 * row_h), (0, 0, 0, 0))
    for row, bg in enumerate(((255, 255, 255, 255), (32, 32, 36, 255))):
        ImageDraw.Draw(sheet).rectangle([0, row * row_h, sheet.width, (row + 1) * row_h], fill=bg)
        for i, design in enumerate(DESIGNS.values()):
            x = pad + i * (256 + 32 + 16 + 3 * pad)
            y = row * row_h + pad
            sheet.alpha_composite(render(design, 256), (x, y))
            sheet.alpha_composite(render(design, 32), (x + 256 + pad, y))
            sheet.alpha_composite(render(design, 16), (x + 256 + pad, y + 32 + pad))
            # Те же 16 и 32 px, увеличенные в 4 раза «как есть» (без сглаживания).
            sheet.alpha_composite(render(design, 16).resize((64, 64), Image.NEAREST), (x + 256 + pad, y + 32 + 16 + 2 * pad))
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    sheet.save(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--candidates", metavar="PNG", help="нарисовать лист кандидатов и выйти")
    parser.add_argument("--out", type=Path, default=OUT, help="каталог результата (по умолчанию assets/branding)")
    parser.add_argument("--design", default=CHOSEN, choices=sorted(DESIGNS))
    args = parser.parse_args()
    if args.candidates:
        write_candidates(args.candidates)
    else:
        write_branding(DESIGNS[args.design], args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
