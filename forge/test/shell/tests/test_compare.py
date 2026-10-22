import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import compare


class CompareTest(unittest.TestCase):
    def test_fx_prints_the_count_plain_or_in_exponent_form(self):
        self.assertEqual(compare.parse_count("0"), 0)
        self.assertEqual(compare.parse_count("188\n"), 188)
        self.assertEqual(compare.parse_count("2.0736e+06"), 2073600)

    def test_garbage_is_an_error_not_a_pass(self):
        with self.assertRaises(ValueError):
            compare.parse_count("magick: unable to open image")

    def test_the_tolerance_is_sixty_four_pixels(self):
        self.assertEqual(compare.TOLERANCE, 64)
        self.assertTrue(compare.verdict(64))
        self.assertFalse(compare.verdict(65))

    def test_a_toolkit_dock_case_fails_without_its_running_indicator(self):
        # The indicator is 2 px across a 32 px button at scale 1.0: 64 pixels, which the
        # default tolerance would let through.
        missing_indicator = 2 * 32
        dock = Path("/repo/forge/test/shell/golden/dock")
        toolkit = compare.tolerance(dock, "toolkit-dock-light-1.0-en")
        self.assertFalse(compare.verdict(missing_indicator, toolkit))
        self.assertTrue(compare.verdict(0, toolkit))

    def test_the_gtk_dock_and_other_surfaces_keep_the_default_tolerance(self):
        dock = Path("/repo/forge/test/shell/golden/dock")
        self.assertEqual(compare.tolerance(dock, "dock-light-1.0-en"), compare.TOLERANCE)
        self.assertEqual(compare.tolerance(Path("golden/bar"), "toolkit-bar"), compare.TOLERANCE)


if __name__ == "__main__":
    unittest.main()
