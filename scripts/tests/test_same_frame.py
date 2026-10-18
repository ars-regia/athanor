"""same_frame.py: the share of changed pixels between two PPM captures."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "devvm"))
import same_frame as sf  # noqa: E402


def ppm(width, height, rgb):
    return b"P6\n%d %d\n255\n" % (width, height) + bytes(rgb)


class ChangedTests(unittest.TestCase):
    def test_identical_captures_do_not_change(self):
        frame = ppm(2, 1, [1, 2, 3, 4, 5, 6])
        self.assertEqual(sf.changed(frame, frame), 0)

    def test_one_pixel_of_four_is_a_quarter(self):
        before = ppm(2, 2, [0] * 12)
        after = ppm(2, 2, [0] * 9 + [0, 0, 255])
        self.assertEqual(sf.changed(before, after), 0.25)

    def test_a_byte_of_255_is_pixel_data_not_a_separator(self):
        before = ppm(1, 1, [32, 10, 255])
        self.assertEqual(sf.changed(before, before), 0)

    def test_sizes_must_match(self):
        with self.assertRaises(SystemExit):
            sf.changed(ppm(1, 1, [0, 0, 0]), ppm(2, 1, [0] * 6))

    def test_truncated_capture_is_refused(self):
        with self.assertRaises(SystemExit):
            sf.changed(ppm(2, 1, [0] * 6)[:-1], ppm(2, 1, [0] * 6))


if __name__ == "__main__":
    unittest.main()
