"""Unit tests of the direction check in scripts/verify.py: no crate but athanor-i18n calls
`Catalog::is_rtl` (python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

PROGRAM = "forge/specs/athanor-bar/athanor-bar-1.0.0/src/i18n.rs"


class DirectionProblemsTest(unittest.TestCase):
    def test_a_direct_call_is_reported_with_its_line(self):
        for call in ("catalog().is_rtl()", "catalog.is_rtl()", "catalog_ref().is_rtl()",
                     "Catalog::is_rtl(&c)"):
            problems = verify.direction_problems(PROGRAM, f"fn a() {{}}\nfn b() -> bool {{ {call} }}\n")
            self.assertEqual(len(problems), 1, call)
            self.assertTrue(problems[0].startswith(f"{PROGRAM}:2 "), problems[0])

    def test_the_shared_function_and_comments_pass(self):
        text = "// catalog.is_rtl() is the catalog's rule\nathanor_i18n::is_rtl(DOMAIN)\n"
        self.assertEqual(verify.direction_problems(PROGRAM, text), [])

    def test_athanor_i18n_may_call_it(self):
        self.assertEqual(
            verify.direction_problems("system/athanor-i18n/src/lib.rs", "catalog.is_rtl()\n"), [])

    def test_the_repository_passes(self):
        self.assertEqual(verify.check_i18n_direction().problems, [])


if __name__ == "__main__":
    unittest.main()
