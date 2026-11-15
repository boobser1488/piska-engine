#!/usr/bin/env python3
"""Сводка замеров `examples/bench` (строки `BENCH {json}`) в таблицы Markdown.

    python tools/bench_report.py after.txt                 # таблица одного замера
    python tools/bench_report.py after.txt before.txt      # «до / после» с разбросом

По каждому случаю берётся медиана по прогонам; разброс — (max - min) / медиана медиан
времени кадра между прогонами. Изменение меньше суммы разбросов «до» и «после» помечается
как шум (`~`), а не как выигрыш или проигрыш.
"""

import json
import sys
from collections import OrderedDict

FRAME = ["median", "p95", "p99"]
PHASES = ["tick", "scripts", "physics", "other", "end", "render", "walk", "extract", "encode", "submit", "gpu"]
COUNTS = ["draws", "shadow_draws", "tris", "meshes", "sprites", "state_changes", "upload_kb", "allocs", "peak_mb", "load_ms", "first_frame_ms", "objects"]


def load(path):
    cases = OrderedDict()
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line.startswith("BENCH "):
                continue
            row = json.loads(line[len("BENCH "):])
            cases.setdefault(row["case"], []).append(row)
    return cases


def median(values):
    values = sorted(values)
    if not values:
        return 0.0
    mid = len(values) // 2
    return values[mid] if len(values) % 2 else (values[mid - 1] + values[mid]) / 2


def summary(runs):
    out = {}
    keys = set().union(*(r.keys() for r in runs)) - {"case", "run"}
    for key in keys:
        values = [r[key] for r in runs if isinstance(r.get(key), (int, float))]
        if values:
            out[key] = median(values)
    medians = [r["median"] for r in runs]
    mid = median(medians)
    out["spread"] = (max(medians) - min(medians)) / mid if mid > 0 else 0.0
    out["runs"] = len(runs)
    return out


def fmt(value):
    if isinstance(value, float):
        if abs(value) >= 100:
            return f"{value:.0f}"
        if abs(value) >= 10:
            return f"{value:.1f}"
        return f"{value:.2f}"
    return str(value)


def table(cases):
    lines = []
    head = ["case", "runs", "frame med", "p95", "p99", "spread"] + PHASES
    lines.append("| " + " | ".join(head) + " |")
    lines.append("|" + "---|" * len(head))
    for name, runs in cases.items():
        s = summary(runs)
        row = [name, str(s["runs"])] + [fmt(s.get(k, 0.0)) for k in FRAME] + [f"{s['spread'] * 100:.0f}%"]
        row += [fmt(s[k]) if k in s else "-" for k in PHASES]
        lines.append("| " + " | ".join(row) + " |")
    lines.append("")
    head = ["case"] + COUNTS
    lines.append("| " + " | ".join(head) + " |")
    lines.append("|" + "---|" * len(head))
    for name, runs in cases.items():
        s = summary(runs)
        lines.append("| " + " | ".join([name] + [fmt(s[k]) if k in s else "-" for k in COUNTS]) + " |")
    return "\n".join(lines)


def compare(after, before):
    lines = ["| case | before ms (spread) | after ms (spread) | change | verdict | draws before -> after | allocs before -> after |", "|---|---|---|---|---|---|---|"]
    for name, runs in after.items():
        if name not in before:
            continue
        a, b = summary(runs), summary(before[name])
        change = (a["median"] - b["median"]) / b["median"] if b["median"] > 0 else 0.0
        noise = a["spread"] + b["spread"]
        verdict = "~ noise" if abs(change) <= noise else ("faster" if change < 0 else "SLOWER")
        lines.append(
            f"| {name} | {fmt(b['median'])} ({b['spread'] * 100:.0f}%) | {fmt(a['median'])} ({a['spread'] * 100:.0f}%) | "
            f"{change * 100:+.0f}% | {verdict} | {fmt(b.get('draws', 0))} -> {fmt(a.get('draws', 0))} | "
            f"{fmt(b.get('allocs', 0))} -> {fmt(a.get('allocs', 0))} |"
        )
    return "\n".join(lines)


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    after = OrderedDict()
    for path in argv[1:2]:
        after.update(load(path))
    print(table(after))
    if len(argv) > 2:
        print()
        print(compare(after, load(argv[2])))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
