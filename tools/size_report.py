#!/usr/bin/env python3
"""Размер функций wasm-модуля по крейтам (замена `cargo bloat`, которая не читает wasm).

Нужна сборка с секцией имён: не вырезанная (`CARGO_PROFILE_WEB_RELEASE_STRIP=false`) и
не прогнанная через `remove_name_section`, то есть сам `.wasm` из каталога `target`,
а не `game_bg.wasm` из экспорта. Использование:

    python tools/size_report.py game.wasm [--top 40] [--functions 15]

Печатает таблицу «крейт → КиБ» (по телам функций секции кода) и самые большие функции.
Крейт берётся из первого сегмента пути legacy-манглинга; для `<T as Trait>::f` —
из первого сегмента внутри угловых скобок, то есть обобщённый код записывается на
тот крейт, чей тип в нём указан (так же считает cargo-bloat).
"""
import re
import sys
from collections import defaultdict


def leb(data, pos):
    result = shift = 0
    while True:
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, pos
        shift += 7


def sections(data):
    if data[:4] != b"\0asm":
        raise SystemExit("not a wasm file")
    pos = 8
    while pos < len(data):
        sid = data[pos]
        size, pos = leb(data, pos + 1)
        yield sid, pos, pos + size
        pos += size


def read_name(data, pos):
    n, pos = leb(data, pos)
    return data[pos:pos + n].decode("utf-8", "replace"), pos + n


def parse(path):
    data = open(path, "rb").read()
    imports = 0
    bodies = []  # (размер тела) по порядку определённых функций
    names = {}
    for sid, start, end in sections(data):
        if sid == 2:  # import
            count, pos = leb(data, start)
            for _ in range(count):
                _, pos = read_name(data, pos)
                _, pos = read_name(data, pos)
                kind = data[pos]
                pos += 1
                if kind == 0:
                    _, pos = leb(data, pos)
                    imports += 1
                elif kind == 1:
                    pos += 1
                    flag = data[pos]
                    pos += 1
                    _, pos = leb(data, pos)
                    if flag & 1:
                        _, pos = leb(data, pos)
                elif kind == 2:
                    flag = data[pos]
                    pos += 1
                    _, pos = leb(data, pos)
                    if flag & 1:
                        _, pos = leb(data, pos)
                elif kind == 3:
                    pos += 2
                else:
                    raise SystemExit("unsupported import kind %d" % kind)
        elif sid == 10:  # code
            count, pos = leb(data, start)
            for _ in range(count):
                size, pos = leb(data, pos)
                bodies.append(size)
                pos += size
        elif sid == 0:
            name, pos = read_name(data, start)
            if name == "name":
                while pos < end:
                    sub = data[pos]
                    size, pos = leb(data, pos + 1)
                    if sub == 1:  # имена функций
                        count, p = leb(data, pos)
                        for _ in range(count):
                            index, p = leb(data, p)
                            fname, p = read_name(data, p)
                            names[index] = fname
                    pos += size
    return len(data), imports, bodies, names


LEGACY = re.compile(r"^_ZN(.*)E$")


def demangle(symbol):
    """Грубая расшифровка legacy-манглинга Rust: список сегментов пути."""
    m = LEGACY.match(symbol)
    if not m:
        return None
    rest, parts = m.group(1), []
    while rest:
        m2 = re.match(r"(\d+)", rest)
        if not m2:
            break
        n = int(m2.group(1))
        parts.append(rest[m2.end():m2.end() + n])
        rest = rest[m2.end() + n:]
    parts = [p for p in parts if not re.fullmatch(r"h[0-9a-f]{16}", p)]
    return [p.replace("..", "::") for p in parts] or None


def crate_of(symbol):
    parts = demangle(symbol)
    if not parts:
        if symbol.startswith("_R"):
            # v0: `Cs<хэш>_<длина><имя крейта>` — берём первый такой сегмент.
            m = re.search(r"C[sa]?[0-9A-Za-z]*_(\d+)", symbol)
            if m:
                n = int(m.group(1))
                return symbol[m.end():m.end() + n]
            m = re.search(r"C(\d+)", symbol)
            if m:
                n = int(m.group(1))
                return symbol[m.end():m.end() + n]
            return "?"
        return "(no name: %s)" % ("glue" if not symbol else "foreign")
    first = parts[0]
    if first.startswith("_$LT$") or first.startswith("$LT$"):
        inner = first.replace("$LT$", "<").replace("$GT$", ">").replace("$u20$", " ")
        inner = inner.replace("$RF$", "&").replace("$BP$", "*").replace("$u5b$", "[").replace("$u5d$", "]")
        inner = inner.replace("$C$", ",").replace("$LP$", "(").replace("$RP$", ")")
        for token in re.findall(r"[A-Za-z_][A-Za-z0-9_]*(?=::)", inner):
            return token
        return "(generic)"
    return first.split("::")[0]


def pretty(symbol):
    parts = demangle(symbol)
    return "::".join(parts) if parts else symbol


def main():
    args = sys.argv[1:]
    if not args:
        raise SystemExit(__doc__)
    path, top, functions = args[0], 40, 15
    if "--top" in args:
        top = int(args[args.index("--top") + 1])
    if "--functions" in args:
        functions = int(args[args.index("--functions") + 1])
    total, imports, bodies, names = parse(path)
    if not names:
        raise SystemExit("no name section: build without strip and use the raw .wasm from target/")
    by_crate = defaultdict(int)
    big = []
    for i, size in enumerate(bodies):
        symbol = names.get(imports + i, "")
        by_crate[crate_of(symbol)] += size
        big.append((size, symbol))
    code = sum(bodies)
    print("file %d KiB, code section %d KiB, %d functions" % (total // 1024, code // 1024, len(bodies)))
    print("%-34s %8s %6s" % ("crate", "KiB", "%code"))
    for crate, size in sorted(by_crate.items(), key=lambda kv: -kv[1])[:top]:
        print("%-34s %8.1f %5.1f%%" % (crate[:34], size / 1024, 100.0 * size / code))
    print()
    for size, symbol in sorted(big, reverse=True)[:functions]:
        print("%8.1f KiB  %s" % (size / 1024, pretty(symbol)[:110]))


if __name__ == "__main__":
    main()
