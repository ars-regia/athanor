import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import cases


class CasesTest(unittest.TestCase):
    def test_the_greeter_has_the_twelve_cases_of_sh13(self):
        found = cases.surface_cases("greeter")
        self.assertEqual(len(found), 12)
        self.assertEqual(len({c.tag for c in found}), 12)
        self.assertEqual({c.scale for c in found}, {"1.0", "1.5"})
        self.assertEqual({c.variant for c in found}, {"light", "dark"})
        self.assertEqual({c.locale for c in found}, {"en_US.UTF-8", "de_DE.UTF-8", "ar_EG.UTF-8"})
        self.assertEqual({c.catalog for c in found}, {"-", "de.mo", "rtl.mo"})

    def test_tags_are_file_names(self):
        for case in cases.surface_cases("greeter"):
            self.assertRegex(case.tag, r"^greeter-(light|dark)-(1\.0|1\.5)-(en|de|rtl)$")

    def test_an_unknown_surface_is_an_error(self):
        with self.assertRaises(KeyError):
            cases.surface_cases("launcher")

    def test_the_layouts_have_the_twenty_seven_cases_of_sh13(self):
        found = cases.layout_cases()
        self.assertEqual(len(found), 27)
        self.assertEqual(len({c.tag for c in found}), 27)
        self.assertEqual(len([c for c in found if c.outputs == 1]), 21)
        self.assertEqual(len([c for c in found if c.outputs == 2]), 6)

    def test_every_layout_of_sh7_runs_at_least_once(self):
        seen = {(c.preset, c.panel, c.dock) for c in cases.layout_cases()}
        self.assertEqual(len(seen), 14)
        self.assertNotIn("visible", {c.dock for c in cases.layout_cases() if c.preset == "bar"})

    def test_portrait_cases_are_the_factory_presets_and_float_at_the_bottom(self):
        portrait = [c for c in cases.layout_cases() if c.height > c.width]
        self.assertEqual({(c.preset, c.panel, c.dock) for c in portrait},
                         {("float", "top", "visible"), ("bar", "bottom", "-"), ("minimal", "top", "none"),
                          ("float", "bottom", "visible")})
        self.assertEqual({(c.outputs, c.scale) for c in portrait}, {(1, "1.0")})

    def test_layout_tags_are_file_names(self):
        for case in cases.layout_cases():
            self.assertRegex(case.tag, r"^layout-(float|bar|minimal)-(top|bottom)(-(visible|auto-hide|none))?"
                                       r"-[12]o-(1\.0|1\.5)-(land|port)$")

    def test_the_chooser_has_the_twelve_cases_of_sh13(self):
        self.assertEqual(len(cases.surface_cases("chooser")), 12)


if __name__ == "__main__":
    unittest.main()
