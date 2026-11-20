"""Unit tests of the register counts sub-check of `verify.py docs`: the Counts section of
docs/architecture/shell-features.md must match its rows
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

ROWS = """# Shell feature register

## Bar

| Id | Feature | Sources | Athanor | Note |
|---|---|---|---|---|
| F-bar-01 | Clock | [x](https://x) | have | clock.rs:80 |
| F-bar-02 | Tray | [x](https://x) | partial | no menus |
| F-bar-03 | Weather | [x](https://x) | missing | not verified |

## Dock

| Id | Feature | Sources | Athanor | Note |
|---|---|---|---|---|
| F-dock-01 | Pins | [x](https://x) | excluded (proposed) | zero-trust |

## Counts
"""


def counts(surface, have, partial, missing, excluded, total, decided=0):
    return (
        f"\n### {surface}\n\n| Status | Entries |\n|---|---|\n| have | {have} |\n"
        f"| partial | {partial} |\n| missing | {missing} |\n"
        f"| excluded (proposed) | {excluded} |\n| excluded | {decided} |\n"
        f"| total | {total} |\n"
    )


class RegisterCountsTest(unittest.TestCase):
    def test_matching_counts_pass(self):
        text = ROWS + counts("Bar", 1, 1, 1, 0, 3) + counts("Dock", 0, 0, 0, 1, 1)
        self.assertEqual(verify.register_count_problems(text), [])

    def test_a_count_that_differs_from_the_rows_is_reported(self):
        text = ROWS + counts("Bar", 2, 0, 1, 0, 3) + counts("Dock", 0, 0, 0, 1, 1)
        found = verify.register_count_problems(text)
        self.assertEqual(len(found), 2, found)
        self.assertTrue(
            any("Bar" in p and "have" in p and "2" in p and "1" in p for p in found),
            found,
        )

    def test_a_wrong_total_is_reported(self):
        text = ROWS + counts("Bar", 1, 1, 1, 0, 4) + counts("Dock", 0, 0, 0, 1, 1)
        found = verify.register_count_problems(text)
        self.assertEqual(len(found), 1, found)
        self.assertIn("total", found[0])

    def test_a_surface_without_counts_is_reported(self):
        found = verify.register_count_problems(ROWS + counts("Bar", 1, 1, 1, 0, 3))
        self.assertEqual(len(found), 1, found)
        self.assertIn("Dock", found[0])

    def test_an_unknown_status_is_reported(self):
        text = ROWS.replace("| partial | no menus |", "| half | no menus |")
        text += counts("Bar", 1, 0, 1, 0, 3) + counts("Dock", 0, 0, 0, 1, 1)
        found = verify.register_count_problems(text)
        self.assertEqual(len(found), 1, found)
        self.assertIn("F-bar-02", found[0])

    def test_a_decided_exclusion_is_a_status_of_its_own(self):
        text = ROWS.replace("| missing | not verified |", "| excluded | decided 2026-10-04 |")
        self.assertEqual(
            verify.register_count_problems(
                text + counts("Bar", 1, 1, 0, 0, 3, decided=1) + counts("Dock", 0, 0, 0, 1, 1)
            ),
            [],
        )
        found = verify.register_count_problems(
            text + counts("Bar", 1, 1, 1, 0, 3) + counts("Dock", 0, 0, 0, 1, 1)
        )
        self.assertEqual(len(found), 2, found)
        self.assertTrue(all("Bar" in p for p in found), found)


if __name__ == "__main__":
    unittest.main()
