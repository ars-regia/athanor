import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import contrast
import tokens as tk

GREY = {"schema": 1, "accent": {"hue": 0, "saturation": 0},
        "variant": {v: {"color": {"fg": "#777777", "bg": "#ffffff", "veil": {"hex": "#000000", "a": 0.5}}}
                    for v in tk.VARIANTS}}


class ContrastTest(unittest.TestCase):
    def test_a_failing_text_pair_is_reported_in_every_variant(self):
        rows = contrast.check({**GREY, "pair": [{"fg": "fg", "bg": "bg", "kind": "text"}]})
        self.assertEqual(len(rows), 4)
        self.assertTrue(all(ratio < needed for *_, ratio, needed in rows))

    def test_the_same_pair_passes_as_ui(self):
        rows = contrast.check({**GREY, "pair": [{"fg": "fg", "bg": "bg", "kind": "ui"}]})
        self.assertTrue(all(ratio >= needed for *_, ratio, needed in rows))

    def test_each_expands_to_one_pair_per_background(self):
        pairs = list(contrast.expand([{"fg": "fg", "bg": ["bg", "bg"], "each": True, "kind": "ui"}]))
        self.assertEqual([p["bg"] for p in pairs], ["bg", "bg"])

    def test_a_list_background_is_composited(self):
        rows = contrast.check({**GREY, "pair": [{"fg": "fg", "bg": ["bg", "veil"], "kind": "ui"}]})
        # #777 on white veiled by 50 % black (#808080) is almost no contrast at all.
        self.assertLess(rows[0][4], 1.2)

    def test_only_restricts_a_pair_to_the_named_variants(self):
        rows = contrast.check({**GREY, "pair": [{"fg": "fg", "bg": "bg", "kind": "ui", "only": ["dark"]}]})
        self.assertEqual([r[0] for r in rows], ["dark"])

    def test_a_translucent_bottom_layer_is_an_error(self):
        with self.assertRaises(ValueError):
            contrast.check({**GREY, "pair": [{"fg": "fg", "bg": ["veil", "bg"], "kind": "ui"}]})

    def test_the_shipped_tokens_pass(self):
        failing = [r for r in contrast.check(tk.load()) if r[4] < r[5]]
        self.assertEqual(failing, [])

    def test_high_contrast_never_lowers_a_ratio(self):
        rows = {(v, fg, str(bg)): ratio for v, fg, bg, _, ratio, _ in contrast.check(tk.load())}
        for (variant, fg, bg), ratio in rows.items():
            if variant.endswith("-hc") and (variant[:-3], fg, bg) in rows:
                self.assertGreaterEqual(ratio + 1e-9, rows[(variant[:-3], fg, bg)], (variant, fg, bg))

    def test_the_presets_and_the_sweep_run_in_light_and_dark(self):
        cases = [(label, variant) for label, variant, _ in contrast.palettes(tk.load())]
        # Four variants, then 9 presets and 2 x 24 hues in each of light and dark.
        self.assertEqual(len(cases), 4 + 2 * (9 + 2 * 360 // contrast.SWEEP_STEP))
        self.assertIn(("dark purple", "dark"), cases)
        self.assertIn(("light h345 L0.67", "light"), cases)
        self.assertFalse(any(variant.endswith("-hc") for label, variant in cases[4:]))

    def test_tokens_without_an_accent_run_no_accent_case(self):
        self.assertEqual(len(contrast.palettes(GREY)), 4)

    def test_every_accent_case_passes(self):
        failing = [row for row in contrast.check(tk.load()) if row[4] < row[5]]
        self.assertEqual(failing, [])


if __name__ == "__main__":
    unittest.main()
