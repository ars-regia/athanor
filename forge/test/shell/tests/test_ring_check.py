"""Unit tests of forge/test/shell/ring_check.py, on synthetic PNGs
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import struct
import sys
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import ring_check as ring

SURFACE = (255, 255, 255)
BOX = (20, 20, 20, 10)


def png(pixels, width, height, colour_type=2):
    """8-bit RGB (2) or RGBA (6) PNG of pixels[y][x], every row filtered None."""
    raw = b"".join(b"\0" + b"".join(bytes(p) for p in row) for row in pixels)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, colour_type, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def with_ring(colour, width, offset=2, box=BOX):
    """A 60x50 surface with a ring of `width` px, `offset` px outside the box."""
    x, y, w, h = box
    rows = [[SURFACE] * 60 for _ in range(50)]
    inner, outer = offset, offset + width
    for py in range(50):
        for px in range(60):
            dx = max(x - px, 0, px - (x + w - 1))
            dy = max(y - py, 0, py - (y + h - 1))
            if inner < max(dx, dy) <= outer:
                rows[py][px] = colour
    return png(rows, 60, 50)


class ContrastTest(unittest.TestCase):
    def test_black_on_white_is_twenty_one(self):
        self.assertAlmostEqual(ring.contrast(with_ring((0, 0, 0), 2), BOX), 21.0, places=2)

    def test_a_ring_under_three_to_one_fails(self):
        # #aaaaaa on white is 2.3:1
        image = with_ring((170, 170, 170), 2)
        self.assertLess(ring.contrast(image, BOX), 3.0)
        self.assertEqual(len(ring.problems(image, BOX)), 1)

    def test_a_ring_of_three_to_one_passes(self):
        # #767676 on white is 4.54:1
        self.assertEqual(ring.problems(with_ring((118, 118, 118), 2), BOX), [])

    def test_the_ring_width_is_three_in_high_contrast(self):
        self.assertEqual(ring.ring_width(False), 2)
        self.assertEqual(ring.ring_width(True), 3)

    def test_a_three_pixel_ring_is_read_at_its_own_width(self):
        image = with_ring((0, 0, 0), 3)
        self.assertAlmostEqual(ring.contrast(image, BOX, ring.ring_width(True)), 21.0, places=2)
        # read as 2 px, the surface band would take in the third ring pixel
        self.assertLess(ring.contrast(image, BOX, 2), 21.0)

    def test_an_alpha_channel_is_ignored(self):
        rows = [[(0, 0, 0, 255)] * 4 for _ in range(4)]
        self.assertEqual(ring.read_png(png(rows, 4, 4, 6)), (4, 4, [[(0, 0, 0)] * 4 for _ in range(4)]))

    def test_a_box_without_room_for_the_ring_is_an_error(self):
        with self.assertRaises(ValueError):
            ring.contrast(with_ring((0, 0, 0), 2), (0, 0, 60, 50))


if __name__ == "__main__":
    unittest.main()
