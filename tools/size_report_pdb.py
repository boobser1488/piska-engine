#!/usr/bin/env python3
"""Размер кода exe по крейтам для Windows (MSVC) — по PDB, без внешних утилит.

`cargo bloat` на MSVC не работает, а `dumpbin` / `llvm-pdbutil` не установлены, поэтому
читаем PDB сами: контейнер MSF, поток DBI, потоки символов модулей, записи
S_GPROC32 / S_LPROC32 (длина кода каждой функции). Нужна сборка с отладочной
информацией и без strip:

    CARGO_PROFILE_RELEASE_DEBUG=true CARGO_PROFILE_RELEASE_STRIP=false \\
        cargo build --release -p <game> --bin <game>
    python tools/size_report_pdb.py <target>/release/<game>.pdb [--top 40] [--functions 15]

Размеры — длины тел функций (встроенный код записан на вызывающего, как и в cargo-bloat).
Данные (`.rdata`, шрифты, шейдеры) в таблице одной строкой «(not code)» — это разница между
размером exe и суммой функций. Параметр --exe <файл> берёт размер файла оттуда.
"""
import re
import struct
import sys
from collections import defaultdict

PROC = (0x110F, 0x1110, 0x1146, 0x1147)


class Msf:
    def __init__(self, path):
        self.f = open(path, "rb")
        head = self.f.read(56)
        if not head.startswith(b"Microsoft C/C++ MSF 7.00"):
            raise SystemExit("not a PDB 7.0 file")
        self.bs, _, _, dir_bytes, _, map_addr = struct.unpack_from("<6I", head, 32)
        blocks = -(-dir_bytes // self.bs)
        self.f.seek(map_addr * self.bs)
        dir_blocks = struct.unpack("<%dI" % -(-blocks * 4 // 4), self.f.read(blocks * 4))
        directory = b"".join(self.block(b) for b in dir_blocks)[:dir_bytes]
        (count,) = struct.unpack_from("<I", directory, 0)
        sizes = struct.unpack_from("<%dI" % count, directory, 4)
        pos = 4 + 4 * count
        self.streams = []
        for size in sizes:
            if size == 0xFFFFFFFF:
                size = 0
            n = -(-size // self.bs)
            self.streams.append((size, struct.unpack_from("<%dI" % n, directory, pos)))
            pos += 4 * n

    def block(self, index):
        self.f.seek(index * self.bs)
        return self.f.read(self.bs)

    def stream(self, index):
        size, blocks = self.streams[index]
        return b"".join(self.block(b) for b in blocks)[:size]


def cstr(data, pos):
    end = data.index(b"\0", pos)
    return data[pos:end].decode("utf-8", "replace"), end + 1


def procedures(pdb):
    dbi = pdb.stream(3)
    mod_size, = struct.unpack_from("<i", dbi, 24)
    pos, end = 64, 64 + mod_size
    modules = []
    while pos < end:
        sym_stream, sym_size = struct.unpack_from("<HI", dbi, pos + 34)
        name, p = cstr(dbi, pos + 64)
        _, p = cstr(dbi, p)
        pos = (p + 3) & ~3
        modules.append((name, sym_stream, sym_size))
    for name, stream, size in modules:
        if stream == 0xFFFF or size < 8:
            continue
        data = pdb.stream(stream)[:size]
        p = 4
        while p + 4 <= len(data):
            length, kind = struct.unpack_from("<HH", data, p)
            if kind in PROC:
                code = struct.unpack_from("<I", data, p + 4 + 12)[0]
                symbol, _ = cstr(data, p + 4 + 35)
                yield symbol, code
            p += 2 + length
            if length == 0:
                break


def crate_of(symbol):
    s = symbol
    if s.startswith("<"):
        s = s.lstrip("<&*").replace("dyn ", "")
    m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)::", s)
    if m:
        return m.group(1)
    if s.startswith("_ZN") or s.startswith("_R"):
        return "(mangled)"
    return "(c/c++/glue: %s)" % (s.split("(")[0][:12] or "?")


def main():
    args = sys.argv[1:]
    if not args:
        raise SystemExit(__doc__)
    top, functions, exe = 40, 15, None
    if "--top" in args:
        top = int(args[args.index("--top") + 1])
    if "--functions" in args:
        functions = int(args[args.index("--functions") + 1])
    if "--exe" in args:
        exe = args[args.index("--exe") + 1]
    by_crate, big, total = defaultdict(int), [], 0
    for symbol, size in procedures(Msf(args[0])):
        by_crate[crate_of(symbol)] += size
        big.append((size, symbol))
        total += size
    import os
    size = os.path.getsize(exe or args[0].replace(".pdb", ".exe"))
    print("exe %d KiB, function bodies %d KiB" % (size // 1024, total // 1024))
    print("%-34s %8s %6s" % ("crate", "KiB", "%exe"))
    rows = sorted(by_crate.items(), key=lambda kv: -kv[1])
    for crate, n in rows[:top]:
        print("%-34s %8.1f %5.1f%%" % (crate[:34], n / 1024, 100.0 * n / size))
    print("%-34s %8.1f %5.1f%%" % ("(not code: data, headers)", (size - total) / 1024, 100.0 * (size - total) / size))
    print()
    for n, symbol in sorted(big, reverse=True)[:functions]:
        print("%8.1f KiB  %s" % (n / 1024, symbol[:110]))


if __name__ == "__main__":
    main()
