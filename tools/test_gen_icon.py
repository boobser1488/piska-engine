#!/usr/bin/env python3
"""Проверка генератора значка: `python tools/test_gen_icon.py`."""

import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_icon  # noqa: E402


class GenIconTest(unittest.TestCase):
    def test_two_runs_give_identical_bytes_and_match_the_checked_in_files(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            gen_icon.write_branding(gen_icon.DESIGNS[gen_icon.CHOSEN], Path(a))
            gen_icon.write_branding(gen_icon.DESIGNS[gen_icon.CHOSEN], Path(b))
            names = sorted(p.name for p in Path(a).iterdir())
            self.assertEqual(names, sorted(p.name for p in Path(b).iterdir()))
            for name in names:
                first = (Path(a) / name).read_bytes()
                self.assertEqual(first, (Path(b) / name).read_bytes(), name)
                committed = gen_icon.OUT / name
                self.assertTrue(committed.exists(), f"{name} is not in assets/branding: run tools/gen_icon.py")
                self.assertEqual(first, committed.read_bytes(), f"{name} is stale: run tools/gen_icon.py")

    def test_every_png_size_exists(self):
        from PIL import Image
        for size in gen_icon.PNG_SIZES:
            image = Image.open(gen_icon.OUT / f"icon-{size}.png")
            self.assertEqual(image.size, (size, size))
            self.assertEqual(image.mode, "RGBA")

    def test_ico_directory_lists_the_expected_sizes(self):
        data = (gen_icon.OUT / "icon.ico").read_bytes()
        reserved, kind, count = struct.unpack_from("<HHH", data)
        self.assertEqual((reserved, kind), (0, 1))
        sizes = [struct.unpack_from("<B", data, 6 + 16 * i)[0] or 256 for i in range(count)]
        self.assertEqual(sizes, gen_icon.ICO_SIZES)


if __name__ == "__main__":
    unittest.main()
