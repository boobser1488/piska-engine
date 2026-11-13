#!/usr/bin/env python3
"""Генератор демонстрационных материалов (docs/api/materials.md).

Запуск из корня репозитория:  python tools/gen_sample_materials.py

Пишет в assets/materials/ текстуры 256×256 (все бесшовные) и файлы `.material`:
  painted_metal  — красная краска с царапинами до металла (metallic-roughness карта)
  rough_stone    — булыжник: цвет, карта нормалей из рельефа, ambient occlusion
  emissive_lamp  — тёмный корпус со светящимися полосами (карта свечения, блум)
  glass          — полупрозрачное голубое стекло (Blend)
  foliage        — листья с вырезом по альфе (Cutout, двусторонний)
  checker        — шахматка с тайлингом UV
У каждого файла рядом `.meta` с постоянным GUID — повторный запуск даёт те же файлы.
Только стандартная библиотека Python.
"""
import math
import random
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "materials"
SIZE = 256


def write_meta(path, guid, importer, settings=None):
    text = f'(\n    guid: "{guid}",\n    importer: "{importer}",\n'
    if settings:
        text += "    settings: {\n"
        for key, value in settings.items():
            text += f'        "{key}": {value},\n'
        text += "    },\n"
    text += ")\n"
    Path(str(path) + ".meta").write_text(text, encoding="utf-8", newline="\n")


def write_png(path, pixels):
    """pixels: список строк, строка — список (r, g, b, a) 0..255."""
    height, width = len(pixels), len(pixels[0])
    raw = bytearray()
    for row in pixels:
        raw.append(0)
        for r, g, b, a in row:
            raw += bytes((clamp8(r), clamp8(g), clamp8(b), clamp8(a)))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def clamp8(v):
    return max(0, min(255, int(round(v))))


def image(fn):
    return [[fn(x, y) for x in range(SIZE)] for y in range(SIZE)]


# ---------------------------------------------------------------- шум и ячейки

def value_noise(seed, cells):
    """Бесшовный value noise: сетка cells×cells, билинейно со сглаживанием."""
    rng = random.Random(seed)
    grid = [[rng.random() for _ in range(cells)] for _ in range(cells)]

    def sample(x, y):
        fx, fy = x / SIZE * cells, y / SIZE * cells
        x0, y0 = int(fx) % cells, int(fy) % cells
        x1, y1 = (x0 + 1) % cells, (y0 + 1) % cells
        tx, ty = fx - int(fx), fy - int(fy)
        tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
        top = grid[y0][x0] * (1 - tx) + grid[y0][x1] * tx
        bottom = grid[y1][x0] * (1 - tx) + grid[y1][x1] * tx
        return top * (1 - ty) + bottom * ty

    return sample


def fbm(seed):
    layers = [value_noise(seed + i, 4 << i) for i in range(4)]
    return lambda x, y: sum(layer(x, y) * 0.5 ** i for i, layer in enumerate(layers)) / 1.875


def cells(seed, count):
    """Бесшовные ячейки Вороного: (расстояние до края ячейки в пикселях, номер ячейки)."""
    rng = random.Random(seed)
    step = SIZE / count
    points = [((i + 0.2 + rng.random() * 0.6) * step, (j + 0.2 + rng.random() * 0.6) * step)
              for j in range(count) for i in range(count)]

    def at(x, y):
        best = []
        for index, (px, py) in enumerate(points):
            dx = (x - px + SIZE / 2) % SIZE - SIZE / 2
            dy = (y - py + SIZE / 2) % SIZE - SIZE / 2
            best.append((dx * dx + dy * dy, index))
        best.sort()
        (d1, i1), (d2, _) = best[0], best[1]
        return (math.sqrt(d2) - math.sqrt(d1)) * 0.5, i1

    return at


# ---------------------------------------------------------------- текстуры

def checker():
    return image(lambda x, y: (225, 225, 225, 255) if ((x // 32) + (y // 32)) % 2 == 0 else (64, 68, 78, 255))


def stone():
    cell = cells(7, 5)
    grain = fbm(11)
    table = {}
    for y in range(SIZE):
        for x in range(SIZE):
            table[(x, y)] = cell(x, y)
    tint_rng = random.Random(3)
    tints = [(0.85 + tint_rng.random() * 0.3, 0.8 + tint_rng.random() * 0.25) for _ in range(25)]

    def height(x, y):
        edge, _ = table[(x % SIZE, y % SIZE)]
        dome = min(edge / 9.0, 1.0)
        return math.sqrt(dome) * 0.85 + grain(x % SIZE, y % SIZE) * 0.15

    heights = [[height(x, y) for x in range(SIZE)] for y in range(SIZE)]

    def albedo(x, y):
        edge, index = table[(x, y)]
        light, warm = tints[index % len(tints)]
        g = grain(x, y)
        mortar = edge < 2.5
        base = 70 if mortar else (128 + 70 * (g - 0.5)) * light
        return (base * warm * 1.05, base * warm, base * 0.92, 255)

    def normal(x, y):
        h = lambda i, j: heights[j % SIZE][i % SIZE]
        dx = (h(x + 1, y) - h(x - 1, y)) * 0.5
        dy = (h(x, y + 1) - h(x, y - 1)) * 0.5
        k = 6.0
        # OpenGL-соглашение: зелёный — «вверх» по картинке (против роста y).
        nx, ny, nz = -dx * k, dy * k, 1.0
        length = math.sqrt(nx * nx + ny * ny + nz * nz)
        return ((nx / length * 0.5 + 0.5) * 255, (ny / length * 0.5 + 0.5) * 255, (nz / length * 0.5 + 0.5) * 255, 255)

    def occlusion(x, y):
        edge, _ = table[(x, y)]
        ao = 0.35 + 0.65 * min(edge / 6.0, 1.0)
        return (ao * 255, ao * 255, ao * 255, 255)

    return image(albedo), image(normal), image(occlusion)


def painted_metal():
    rng = random.Random(5)
    scratched = [[False] * SIZE for _ in range(SIZE)]
    for _ in range(46):
        x, y = rng.random() * SIZE, rng.random() * SIZE
        angle = rng.random() * math.pi
        length = 10 + rng.random() * 50
        width = 0.6 + rng.random() * 1.6
        for t in range(int(length * 2)):
            px = x + math.cos(angle) * t * 0.5
            py = y + math.sin(angle) * t * 0.5
            r = int(math.ceil(width))
            for oy in range(-r, r + 1):
                for ox in range(-r, r + 1):
                    if ox * ox + oy * oy <= width * width:
                        scratched[int(py + oy) % SIZE][int(px + ox) % SIZE] = True
    grain = fbm(21)

    def albedo(x, y):
        if scratched[y][x]:
            g = 165 + 40 * grain(x, y)
            return (g, g, g * 1.04, 255)
        v = 0.9 + 0.2 * grain(x, y)
        return (196 * v, 38 * v, 30 * v, 255)

    def metal_rough(x, y):
        # glTF: зелёный — шероховатость, синий — металличность.
        if scratched[y][x]:
            return (0, 0.28 * 255, 255, 255)
        return (0, (0.42 + 0.12 * grain(x, y)) * 255, 0, 255)

    return image(albedo), image(metal_rough)


def leaves():
    rng = random.Random(9)
    pixels = [[(40, 90, 30, 0) for _ in range(SIZE)] for _ in range(SIZE)]
    for _ in range(26):
        cx, cy = rng.random() * SIZE, rng.random() * SIZE
        angle = rng.random() * math.tau
        length, width = 30 + rng.random() * 22, 11 + rng.random() * 6
        shade = 0.75 + rng.random() * 0.45
        ca, sa = math.cos(angle), math.sin(angle)
        r = int(length) + 2
        for oy in range(-r, r + 1):
            for ox in range(-r, r + 1):
                u = (ox * ca + oy * sa) / length
                v = (-ox * sa + oy * ca) / width
                # Лист — заострённый эллипс с прожилкой посередине.
                if u * u + v * v * (1.0 + 0.8 * abs(u)) <= 1.0:
                    vein = 0.75 if abs(v) < 0.08 else 1.0
                    light = shade * vein * (0.85 + 0.25 * (1 - abs(v)))
                    pixels[int(cy + oy) % SIZE][int(cx + ox) % SIZE] = (58 * light, 128 * light, 40 * light, 255)
    return pixels


def lamp_mask():
    def at(x, y):
        band = 0.5 + 0.5 * math.cos((y / SIZE) * math.tau * 4)
        glow = 255 if band > 0.55 else 30 * band
        return (glow, glow, glow, 255)

    return image(at)


# ---------------------------------------------------------------- материалы

TEXTURES = {
    "checker.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405162",
    "stone_albedo.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405163",
    "stone_normal.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405164",
    "stone_ao.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405165",
    "paint_albedo.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405166",
    "paint_metal_rough.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405167",
    "leaves.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405168",
    "lamp_mask.png": "6d3f1a2b8c4e4f5a9b0c1d2e3f405169",
}

MATERIALS = {
    "painted_metal": ("7a1e0c3d5b2f4e6a8c9d0e1f2a3b4c01", {
        "base_color_texture": "paint_albedo.png",
        "metallic": 1.0,
        "roughness": 1.0,
        "metallic_roughness_texture": "paint_metal_rough.png",
    }),
    "rough_stone": ("7a1e0c3d5b2f4e6a8c9d0e1f2a3b4c02", {
        "base_color_texture": "stone_albedo.png",
        "roughness": 0.95,
        "normal_texture": "stone_normal.png",
        "normal_strength": 1.0,
        "occlusion_texture": "stone_ao.png",
        "uv_scale": (2.0, 1.0),
    }),
    "emissive_lamp": ("7a1e0c3d5b2f4e6a8c9d0e1f2a3b4c03", {
        "base_color": (0.08, 0.07, 0.06, 1.0),
        "roughness": 0.4,
        "emissive": (1.0, 0.55, 0.2),
        "emissive_texture": "lamp_mask.png",
        "emissive_intensity": 9.0,
        "cast_shadows": False,
    }),
    "glass": ("7a1e0c3d5b2f4e6a8c9d0e1f2a3b4c04", {
        "base_color": (0.55, 0.78, 1.0, 0.3),
        "roughness": 0.05,
        "alpha_mode": "Blend",
    }),
    "foliage": ("7a1e0c3d5b2f4e6a8c9d0e1f2a3b4c05", {
        "base_color_texture": "leaves.png",
        "roughness": 0.8,
        "alpha_mode": "Cutout",
        "alpha_cutoff": 0.5,
        "double_sided": True,
    }),
    "checker": ("7a1e0c3d5b2f4e6a8c9d0e1f2a3b4c06", {
        "base_color_texture": "checker.png",
        "roughness": 0.5,
        "uv_scale": (2.0, 2.0),
    }),
}


def ron_value(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        return f"{value:.3f}".rstrip("0").rstrip(".") if value % 1 else f"{value:.1f}"
    if isinstance(value, tuple):
        return "(" + ", ".join(ron_value(v) for v in value) + ")"
    if isinstance(value, str) and value.endswith(".png"):
        return f'"{TEXTURES[value]}"'
    return str(value)  # перечисление: Blend, Cutout


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    stone_albedo, stone_normal, stone_ao = stone()
    paint_albedo, paint_mr = painted_metal()
    images = {
        "checker.png": checker(),
        "stone_albedo.png": stone_albedo,
        "stone_normal.png": stone_normal,
        "stone_ao.png": stone_ao,
        "paint_albedo.png": paint_albedo,
        "paint_metal_rough.png": paint_mr,
        "leaves.png": leaves(),
        "lamp_mask.png": lamp_mask(),
    }
    for name, pixels in images.items():
        path = OUT / name
        write_png(path, pixels)
        write_meta(path, TEXTURES[name], "Texture", {"filter": 'Enum("Linear")'})
    for name, (guid, fields) in MATERIALS.items():
        path = OUT / f"{name}.material"
        lines = ["// Material (Piska Engine). Textures are referenced by GUID.",
                 f"// Generated by tools/gen_sample_materials.py ({name}).", "("]
        lines += [f"    {key}: {ron_value(value)}," for key, value in fields.items()]
        lines.append(")")
        path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        write_meta(path, guid, "Material")
    print(f"wrote {len(images)} textures and {len(MATERIALS)} materials to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
