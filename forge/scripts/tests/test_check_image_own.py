"""Unit tests of forge/scripts/check_image_own.py: the refused RPMs of the system image against
forge/config/own-allow.txt (ADR-0141 decision 7)."""

import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check_image_own.py"
ALLOW = pathlib.Path(__file__).resolve().parents[2] / "config" / "own-allow.txt"
spec = importlib.util.spec_from_file_location("check_image_own", SCRIPT)
own = importlib.util.module_from_spec(spec)
spec.loader.exec_module(own)


class ParseAllowList(unittest.TestCase):
    def test_the_repository_list_parses(self):
        entries = own.parse_allow_list(ALLOW.read_text())
        self.assertTrue(entries)
        self.assertTrue(all(issue.startswith("#") for _, _, issue in entries))

    def test_comments_and_blank_lines_are_skipped(self):
        self.assertEqual(
            own.parse_allow_list("# x\n\nrpm cosmic-comp #1\n"),
            [("rpm", "cosmic-comp", "#1")],
        )

    def test_a_line_without_issue_fails(self):
        with self.assertRaisesRegex(ValueError, "line 1"):
            own.parse_allow_list("rpm cosmic-comp\n")

    def test_a_name_listed_twice_fails(self):
        with self.assertRaisesRegex(ValueError, "listed twice"):
            own.parse_allow_list("rpm cosmic-comp #1\nrpm cosmic-comp #2\n")

    def test_an_rpm_decision_7_does_not_refuse_fails(self):
        with self.assertRaisesRegex(ValueError, "does not refuse the RPM nautilus"):
            own.parse_allow_list("rpm nautilus #1\n")


class RpmProblems(unittest.TestCase):
    ENTRIES = [
        ("rpm", "cosmic-comp", "#1"),
        ("rpm", "gtk4-layer-shell", "#1"),
        ("crate", "athanor-bar", "#1"),
    ]

    def test_listed_rpms_and_others_pass(self):
        self.assertEqual(
            own.rpm_problems(
                {"cosmic-comp", "gtk4-layer-shell", "gtk4", "bash"}, self.ENTRIES
            ),
            [],
        )

    def test_an_unlisted_refused_rpm_fails(self):
        problems = own.rpm_problems(
            {"cosmic-comp", "gtk4-layer-shell", "pop-launcher"}, self.ENTRIES
        )
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("pop-launcher: in the image", problems[0])

    def test_a_listed_rpm_the_image_lost_fails(self):
        problems = own.rpm_problems({"cosmic-comp"}, self.ENTRIES)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("gtk4-layer-shell: listed", problems[0])

    def test_an_image_without_rpms_fails(self):
        problems = own.rpm_problems(set(), [])
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("lists no RPM", problems[0])

    def test_main_reads_names_from_stdin(self):
        with tempfile.NamedTemporaryFile("w", suffix=".txt") as f:
            f.write("rpm cosmic-comp #1\n")
            f.flush()
            ok = subprocess.run(
                [sys.executable, str(SCRIPT), f.name],
                input="cosmic-comp\nbash\n",
                capture_output=True,
                text=True,
            )
            bad = subprocess.run(
                [sys.executable, str(SCRIPT), f.name],
                input="bash\ncosmic-osd\n",
                capture_output=True,
                text=True,
            )
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertEqual(bad.returncode, 1)
        self.assertIn("cosmic-osd: in the image", bad.stderr)
        self.assertIn("cosmic-comp: listed", bad.stderr)


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
