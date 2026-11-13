#!/usr/bin/env python3
"""Генератор текстур «Шарового завода».

Запуск из каталога игры:  python tools/gen_assets.py
Пишет бесшовные PNG в assets/textures/ (нужны Pillow и numpy). Результат лежит в
репозитории вместе с `.meta` — скрипт нужен, чтобы перерисовать их или подправить палитру.

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
    print("wrote", os.path.normpath(path))


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


if __name__ == "__main__":
    plate()
    hazard()
    brushed()
    crate()
