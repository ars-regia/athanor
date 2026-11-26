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
        calls = {
            "catalog": "fn a() {}\nfn b() -> bool { catalog().is_rtl() }\n",
            "a local": "fn a() {}\nlet c = Catalog::load(D)?; c.is_rtl()\n",
            "a chain": "fn a() {}\nCatalog::load(D).unwrap_or_else(f).is_rtl()\n",
            "a closure": "fn a() {}\nCatalog::load(D).map(|c| c.is_rtl())\n",
            "a path": "fn a() {}\nCatalog::load(D).map(Catalog::is_rtl)\n",
            "a wrapped call": "fn a() {}\ncatalog\n    .is_rtl()\n",
            "a line that begins with it": "fn a() {}\n    .is_rtl()\n",
            "after a url": 'fn a() {}\nlet u = "https://x"; catalog.is_rtl()\n',
        }
        for name, text in calls.items():
            problems = verify.direction_problems(PROGRAM, text)
            self.assertEqual(len(problems), 1, name)
            line = 3 if name == "a wrapped call" else 2  # a wrapped call is named at its `.is_rtl(`
            self.assertTrue(problems[0].startswith(f"{PROGRAM}:{line} "), (name, problems[0]))

    def test_the_shared_function_and_comments_pass(self):
        text = "// catalog.is_rtl() is the catalog's rule\nathanor_i18n::is_rtl(DOMAIN)\n"
        self.assertEqual(verify.direction_problems(PROGRAM, text), [])
        url = 'let u = "https://x"; athanor_i18n::is_rtl(DOMAIN)\n'
        self.assertEqual(verify.direction_problems(PROGRAM, url), [])

    def test_athanor_i18n_may_call_it(self):
        self.assertEqual(
            verify.direction_problems("system/athanor-i18n/src/lib.rs", "catalog.is_rtl()\n"), [])

    def test_the_repository_passes(self):
        self.assertEqual(verify.check_i18n_direction().problems, [])


if __name__ == "__main__":
    unittest.main()
