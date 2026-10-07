import hashlib
import json
import re
import struct
import sys
import tempfile
import unittest
import xml.dom.minidom
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import generate
import tokens as tk

# Properties GTK4 does not implement; the old stylesheet used every one of them.
WEB_ONLY = ("backdrop-filter", "!important", ":root", "var(--", "transform:", "cursor:")


class CssTest(unittest.TestCase):
    def setUp(self):
        self.tokens = tk.load()

    def test_every_variant_defines_every_colour_the_rules_use(self):
        for variant in tk.VARIANTS:
            text = generate.css(self.tokens, variant)
            defined = set(re.findall(r"@define-color (ath_\w+)", text))
            used = set(re.findall(r"@(ath_\w+)", text.split("\n\n", 1)[1]))
            self.assertEqual(used - defined, set(), variant)

    def test_no_placeholder_survives(self):
        self.assertNotIn("$", generate.css(self.tokens, "light"))

    def test_no_web_only_construct(self):
        text = generate.css(self.tokens, "dark")
        for construct in WEB_ONLY:
            self.assertNotIn(construct, text)

    def test_no_animation(self):
        text = generate.css(self.tokens, "light")
        self.assertNotRegex(text, r"\b(transition|animation)[a-z-]*\s*:")

    def test_the_accent_is_the_factory_accent(self):
        self.assertIn("@define-color ath_acc #2e44c2;", generate.css(self.tokens, "light"))
        self.assertIn("@define-color ath_acc #8898f7;", generate.css(self.tokens, "dark"))

    def test_only_the_focused_widget_draws_the_focus_ring(self):
        # GTK4 sets :focus-visible on every ancestor of the focus widget too: a bare
        # :focus-visible rule rings each container around a password entry at once.
        text = generate.css(self.tokens, "light")
        selectors = re.findall(r"^([^{}\n]*:focus-visible[^{}\n]*)\{", text, re.M)
        self.assertTrue(selectors)
        for selector in selectors:
            self.assertIn(":focus:focus-visible", selector)

    def test_translucent_colours_keep_their_alpha(self):
        self.assertRegex(generate.css(self.tokens, "light"), r"@define-color ath_d1 rgba\(\d+, \d+, \d+, 0\.5\);")


class CosmicTest(unittest.TestCase):
    def test_inputs_are_the_five_builder_keys_per_mode(self):
        inputs = json.loads(generate.cosmic_inputs(tk.load()))
        self.assertEqual(set(inputs), {"light", "dark"})
        for mode in inputs.values():
            self.assertEqual(set(mode), {"accent", "bg_color", "primary_container_bg", "neutral_tint", "text_tint"})
            self.assertTrue(all(len(v) == 3 and all(0 <= c <= 1 for c in v) for v in mode.values()))

    def test_the_window_background_is_bg1(self):
        # Not surf2: with surf2 COSMIC's window and container backgrounds came out identical.
        self.assertEqual(generate.COSMIC_INPUTS["bg_color"], "bg1")
        tokens = tk.load()
        inputs = json.loads(generate.cosmic_inputs(tokens))
        for mode, values in inputs.items():
            expected = [round(c, 6) for c in tk.colors(tokens, mode)["bg1"][:3]]
            self.assertEqual(values["bg_color"], expected, mode)

    def test_background_points_at_the_shipped_wallpaper(self):
        self.assertIn('Path("/usr/share/backgrounds/athanor/hearth-light.png")', generate.cosmic_background()["all"])


class IconsTest(unittest.TestCase):
    def test_four_well_formed_symbolic_icons(self):
        files = generate.icons()
        self.assertEqual(sorted(files), ["athanor-mark-symbolic.svg", "athanor-seal-attention-symbolic.svg",
                                         "athanor-seal-blocked-symbolic.svg", "athanor-seal-verified-symbolic.svg"])
        for text in files.values():
            xml.dom.minidom.parseString(text)

    def test_icons_are_fill_only(self):
        # GTK recolours symbolic icons by forcing `fill`; a stroked path would be filled in.
        for text in generate.icons().values():
            self.assertNotIn("stroke", text)

    def test_each_state_has_its_own_class_and_its_own_shape(self):
        files = generate.icons()
        badges = {}
        for state, css_class in (("verified", "success"), ("attention", "warning"), ("blocked", "error")):
            text = files[f"athanor-seal-{state}-symbolic.svg"]
            self.assertEqual(re.findall(r'class="(\w+)"', text), [css_class])
            badges[state] = re.search(r'class="\w+"[^>]* d="([^"]+)"', text).group(1)
        self.assertEqual(len(set(badges.values())), 3)

    def test_the_mark_alone_has_no_badge(self):
        self.assertNotIn("class=", generate.icons()["athanor-mark-symbolic.svg"])


class WallpaperTest(unittest.TestCase):
    def setUp(self):
        self.tokens = tk.load()

    def decode(self, data):
        self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
        width, height = struct.unpack(">II", data[16:24])
        idat = data[data.index(b"IDAT") + 4:data.index(b"IEND") - 8]
        raw = zlib.decompress(idat)
        stride = width * 3 + 1
        return width, height, [raw[y * stride + 1:(y + 1) * stride] for y in range(height)]

    def test_size_and_determinism(self):
        first = generate.wallpaper(self.tokens, "light", 64, 36)
        self.assertEqual(first, generate.wallpaper(self.tokens, "light", 64, 36))
        width, height, rows = self.decode(first)
        self.assertEqual((width, height, len(rows)), (64, 36, 36))

    def test_the_discs_rise_from_the_bottom_right(self):
        _, _, rows = self.decode(generate.wallpaper(self.tokens, "light", 160, 90))
        top_left, bottom_right = rows[0][0:3], rows[89][-3:]
        self.assertNotEqual(top_left, bottom_right)
        # Light variant: the corner under four discs is more saturated, so its red drops.
        self.assertLess(bottom_right[0], top_left[0])

    def test_light_and_dark_differ(self):
        self.assertNotEqual(generate.wallpaper(self.tokens, "light", 32, 18), generate.wallpaper(self.tokens, "dark", 32, 18))

    def test_block_fill_matches_the_exact_render_within_one_level(self):
        rows = list(generate.hearth_rows(self.tokens, "dark", 96, 54))
        exact = list(generate.hearth_rows(self.tokens, "dark", 96, 54, block=1))
        worst = max(abs(a - b) for fast, slow in zip(rows, exact) for a, b in zip(fast, slow))
        self.assertLessEqual(worst, 1)


class CheckTest(unittest.TestCase):
    def test_check_reports_drift_and_a_stale_cosmic_stamp(self):
        tokens = tk.load()
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            for name, text in generate.generated_files(tokens).items():
                (out / name).parent.mkdir(parents=True, exist_ok=True)
                (out / name).write_text(text, encoding="utf-8")
            (out / "cosmic").mkdir()
            (out / "cosmic" / "STAMP").write_text(hashlib.sha256(generate.cosmic_inputs(tokens).encode()).hexdigest() + "\n")
            self.assertEqual(generate.check(tokens, out), [])

            (out / "css" / "calmo-light.css").write_text("edited by hand")
            (out / "cosmic" / "STAMP").write_text("0" * 64 + "\n")
            problems = generate.check(tokens, out)
            self.assertEqual(len(problems), 2)
            self.assertTrue(problems[0].startswith("css/calmo-light.css"))
            self.assertTrue(problems[1].startswith("cosmic/"))


if __name__ == "__main__":
    unittest.main()
