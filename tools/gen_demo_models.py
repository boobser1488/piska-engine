#!/usr/bin/env python3
"""Генератор демонстрационных моделей для окна Project (предпросмотр моделей).

Запуск из корня репозитория:  python tools/gen_demo_models.py

Пишет в assets/models/:
  torus.obj (+ torus.mtl, torus_checker.png)  — OBJ с нормалями, UV, материалом и текстурой
  vase.gltf                                   — glTF с встроенным буфером, один материал-цвет
  stairs.glb                                  — бинарный glTF, две части и два материала
и сломанный файл crates/engine/tests/data/broken.obj (для теста: он не в assets/).
У каждого файла рядом лежит `.meta` с постоянным GUID — повторный запуск даёт те же файлы.
Только стандартная библиотека Python.
"""
import base64
import json
import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODELS = ROOT / "assets" / "models"
BROKEN = ROOT / "crates" / "engine" / "tests" / "data"


def write_meta(path, guid, importer, settings=None):
    text = f'(\n    guid: "{guid}",\n    importer: "{importer}",\n'
    if settings:
        text += "    settings: {\n"
        for key, value in settings.items():
            text += f'        "{key}": {value},\n'
        text += "    },\n"
    text += ")\n"
    Path(str(path) + ".meta").write_text(text, encoding="utf-8", newline="\n")


# ---------------------------------------------------------------- геометрия

def orient(pos, nor, idx):
    """Развернуть треугольники так, чтобы обход (против часовой) совпадал с нормалями."""
    out = list(idx)
    for t in range(0, len(out), 3):
        a, b, c = out[t:t + 3]
        pa, pb, pc = pos[a], pos[b], pos[c]
        e1 = [pb[k] - pa[k] for k in range(3)]
        e2 = [pc[k] - pa[k] for k in range(3)]
        face = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        n = [nor[a][k] + nor[b][k] + nor[c][k] for k in range(3)]
        if sum(face[k] * n[k] for k in range(3)) < 0:
            out[t + 1], out[t + 2] = c, b
    return out


def torus(major=0.62, minor=0.26, rings=36, sides=18):
    """Тор в плоскости XZ: позиции, нормали, UV, индексы."""
    pos, nor, uv, idx = [], [], [], []
    for i in range(rings + 1):
        u = i / rings
        a = u * 2 * math.pi
        for j in range(sides + 1):
            v = j / sides
            b = v * 2 * math.pi
            nx, ny, nz = math.cos(b) * math.cos(a), math.sin(b), math.cos(b) * math.sin(a)
            cx, cz = major * math.cos(a), major * math.sin(a)
            pos.append((cx + minor * nx, minor * ny, cz + minor * nz))
            nor.append((nx, ny, nz))
            uv.append((u * 4, v))
    for i in range(rings):
        for j in range(sides):
            a = i * (sides + 1) + j
            b = a + sides + 1
            idx += [a, a + 1, b, a + 1, b + 1, b]
    return pos, nor, uv, orient(pos, nor, idx)


def lathe(profile, segments=40):
    """Поверхность вращения вокруг оси Y по профилю [(радиус, высота), ...]."""
    pos, nor, uv, idx = [], [], [], []
    n = len(profile)
    for i, (r, y) in enumerate(profile):
        # нормаль профиля — перпендикуляр к касательной
        r0, y0 = profile[max(i - 1, 0)]
        r1, y1 = profile[min(i + 1, n - 1)]
        tr, ty = r1 - r0, y1 - y0
        length = math.hypot(tr, ty) or 1.0
        pr, py = ty / length, -tr / length
        for k in range(segments + 1):
            a = k / segments * 2 * math.pi
            pos.append((r * math.cos(a), y, r * math.sin(a)))
            nor.append((pr * math.cos(a), py, pr * math.sin(a)))
            uv.append((k / segments, i / (n - 1)))
    for i in range(n - 1):
        for k in range(segments):
            a = i * (segments + 1) + k
            b = a + segments + 1
            idx += [a, b, a + 1, a + 1, b, b + 1]
    return pos, nor, uv, orient(pos, nor, idx)


def box(cx, cy, cz, sx, sy, sz):
    """Параллелепипед с гранями-плоскостями: 24 вершины."""
    faces = [
        ((1, 0, 0), (0, 1, 0), (0, 0, -1)),
        ((-1, 0, 0), (0, 1, 0), (0, 0, 1)),
        ((0, 1, 0), (0, 0, -1), (1, 0, 0)),
        ((0, -1, 0), (0, 0, 1), (1, 0, 0)),
        ((0, 0, 1), (0, 1, 0), (1, 0, 0)),
        ((0, 0, -1), (0, 1, 0), (-1, 0, 0)),
    ]
    half = (sx / 2, sy / 2, sz / 2)
    pos, nor, uv, idx = [], [], [], []
    for normal, up, right in faces:
        base = len(pos)
        for su, sr, tex in ((-1, -1, (0, 1)), (-1, 1, (1, 1)), (1, 1, (1, 0)), (1, -1, (0, 0))):
            p = [
                (cx, cy, cz)[a] + half[a] * (normal[a] + su * up[a] + sr * right[a])
                for a in range(3)
            ]
            pos.append(tuple(p))
            nor.append(normal)
            uv.append(tex)
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return pos, nor, uv, orient(pos, nor, idx)


def merge(parts):
    pos, nor, uv, idx = [], [], [], []
    for p, n, t, i in parts:
        base = len(pos)
        pos += p
        nor += n
        uv += t
        idx += [base + k for k in i]
    return pos, nor, uv, idx


# ---------------------------------------------------------------- OBJ

def write_obj_torus():
    pos, nor, uv, idx = torus()
    lines = ["# Piska Engine demo asset: a textured torus (generated by tools/gen_demo_models.py)",
             "mtllib torus.mtl", "o torus"]
    lines += [f"v {x:.5f} {y:.5f} {z:.5f}" for x, y, z in pos]
    lines += [f"vt {u:.5f} {v:.5f}" for u, v in uv]
    lines += [f"vn {x:.5f} {y:.5f} {z:.5f}" for x, y, z in nor]
    lines.append("usemtl checker")
    for t in range(0, len(idx), 3):
        a, b, c = (i + 1 for i in idx[t:t + 3])
        lines.append(f"f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")
    (MODELS / "torus.obj").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    (MODELS / "torus.mtl").write_text(
        "# generated by tools/gen_demo_models.py\nnewmtl checker\nKd 1.0 1.0 1.0\nmap_Kd torus_checker.png\n",
        encoding="utf-8", newline="\n")
    write_meta(MODELS / "torus.obj", "7a3c1d5e9b2f4a60b8d1e3f5a7c92b04", "Mesh")
    write_meta(MODELS / "torus.mtl", "f05d122084a7f1915ed2aac2a20d3359", "Raw")


def write_checker_png():
    size, cell = 128, 16
    orange, cream = (232, 118, 40), (246, 236, 214)
    raw = b""
    for y in range(size):
        raw += b"\x00"
        for x in range(size):
            raw += bytes(orange if (x // cell + y // cell) % 2 == 0 else cream)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
    (MODELS / "torus_checker.png").write_bytes(png)
    write_meta(MODELS / "torus_checker.png", "5d2e8a1c7f3b4960a4c8e2d6b0f19a37", "Texture",
               {"filter": 'Enum("Linear")'})


# ---------------------------------------------------------------- glTF

def pack_primitives(parts):
    """parts: [(геометрия, индекс материала)] → (бинарный буфер, bufferViews, accessors, primitives)."""
    blob = bytearray()
    views, accessors, primitives = [], [], []

    def add(data, target, kind, ctype, count, extra=None):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        accessor = {"bufferView": len(views) - 1, "componentType": ctype, "count": count, "type": kind}
        if extra:
            accessor.update(extra)
        accessors.append(accessor)
        return len(accessors) - 1

    for (pos, nor, uv, idx), material in parts:
        mins = [min(p[a] for p in pos) for a in range(3)]
        maxs = [max(p[a] for p in pos) for a in range(3)]
        a_pos = add(b"".join(struct.pack("<3f", *p) for p in pos), 34962, "VEC3", 5126, len(pos),
                    {"min": mins, "max": maxs})
        a_nor = add(b"".join(struct.pack("<3f", *n) for n in nor), 34962, "VEC3", 5126, len(nor))
        a_uv = add(b"".join(struct.pack("<2f", *t) for t in uv), 34962, "VEC2", 5126, len(uv))
        a_idx = add(b"".join(struct.pack("<I", i) for i in idx), 34963, "SCALAR", 5125, len(idx))
        primitives.append({"attributes": {"POSITION": a_pos, "NORMAL": a_nor, "TEXCOORD_0": a_uv},
                           "indices": a_idx, "material": material, "mode": 4})
    return bytes(blob), views, accessors, primitives


def gltf_document(blob, views, accessors, primitives, materials, name, uri=None):
    buffer = {"byteLength": len(blob)}
    if uri:
        buffer["uri"] = uri
    return {
        "asset": {"version": "2.0", "generator": "Piska Engine tools/gen_demo_models.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": name}],
        "meshes": [{"name": name, "primitives": primitives}],
        "materials": materials,
        "buffers": [buffer],
        "bufferViews": views,
        "accessors": accessors,
    }


def write_gltf_vase():
    profile = [(0.0, 0.0), (0.34, 0.0), (0.42, 0.06), (0.52, 0.32), (0.58, 0.62), (0.5, 0.92),
               (0.3, 1.14), (0.2, 1.3), (0.24, 1.44), (0.34, 1.52)]
    blob, views, accessors, primitives = pack_primitives([(lathe(profile), 0)])
    doc = gltf_document(
        blob, views, accessors, primitives,
        [{"name": "glazed teal",
          "pbrMetallicRoughness": {"baseColorFactor": [0.10, 0.62, 0.58, 1.0], "metallicFactor": 0.2,
                                   "roughnessFactor": 0.35}}],
        "vase", "data:application/octet-stream;base64," + base64.b64encode(blob).decode("ascii"))
    (MODELS / "vase.gltf").write_text(json.dumps(doc, indent=1), encoding="utf-8", newline="\n")
    write_meta(MODELS / "vase.gltf", "2b9f6c4d8e1a4573b0d2f4a6c8e1b305", "Mesh")


def write_glb_stairs():
    steps = [box(0.0, 0.11 + 0.22 * i, -0.32 * i, 1.2, 0.22, 0.32) for i in range(5)]
    carpet = [box(0.0, 0.225 + 0.22 * i, -0.32 * i, 0.5, 0.012, 0.32) for i in range(5)]
    blob, views, accessors, primitives = pack_primitives([(merge(steps), 0), (merge(carpet), 1)])
    doc = gltf_document(
        blob, views, accessors, primitives,
        [{"name": "stone", "pbrMetallicRoughness": {"baseColorFactor": [0.62, 0.58, 0.52, 1.0],
                                                    "metallicFactor": 0.0, "roughnessFactor": 0.9}},
         {"name": "carpet", "pbrMetallicRoughness": {"baseColorFactor": [0.70, 0.12, 0.14, 1.0],
                                                     "metallicFactor": 0.0, "roughnessFactor": 0.8}}],
        "stairs")
    json_bytes = json.dumps(doc, separators=(",", ":")).encode("utf-8")
    json_bytes += b" " * (-len(json_bytes) % 4)
    bin_bytes = blob + b"\x00" * (-len(blob) % 4)
    total = 12 + 8 + len(json_bytes) + 8 + len(bin_bytes)
    glb = (struct.pack("<III", 0x46546C67, 2, total)
           + struct.pack("<II", len(json_bytes), 0x4E4F534A) + json_bytes
           + struct.pack("<II", len(bin_bytes), 0x004E4942) + bin_bytes)
    (MODELS / "stairs.glb").write_bytes(glb)
    write_meta(MODELS / "stairs.glb", "9e4a7b1d3c5f4208a6b8d0f2e4c61a58", "Mesh")


# ---------------------------------------------------------------- сломанный файл

def write_broken():
    BROKEN.mkdir(parents=True, exist_ok=True)
    (BROKEN / "broken.obj").write_text(
        "# Deliberately broken: the face points at a vertex that does not exist.\n"
        "o broken\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 7\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    MODELS.mkdir(parents=True, exist_ok=True)
    write_checker_png()
    write_obj_torus()
    write_gltf_vase()
    write_glb_stairs()
    write_broken()
    print("models written to", MODELS)
