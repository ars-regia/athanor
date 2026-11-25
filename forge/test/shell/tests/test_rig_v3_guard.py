"""rig.sh runs scripts/ci/check-x86-64-v3.sh before every subcommand that builds or runs workspace
code (#1508). Every subcommand of its dispatch is classified exactly once: on the guard's list, or
in SKIPPED below with its reason, so a new subcommand fails here until someone decides
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import fnmatch
import re
import unittest
from pathlib import Path

RIG = Path(__file__).resolve().parent.parent / "rig.sh"

# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS",
             "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY_OWNER")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    for name in TRUST_ENV:
        _os.environ.pop(name, None)


def tearDownModule():
    _environ.stop()

SKIPPED = {
    "build-image": "builds container images and runs no workspace code",
    "build-image-ref": "prints an image reference",
    "build-inputs": "prints a hash",
    "publish-image": "pushes the rig image",
    "publish-build-image": "pushes the build image",
    "probe-sandbox": "runs bubblewrap in the rig, no workspace binary",
    "css-parse": "GTK parse gate of generated stylesheets, no workspace binary",
    "cosmic-keys": "compares COSMIC's key files with the overlay's",
    "seal-bin": "hashes files",
    "verify-bin": "checks hashes",
}


def dispatch_labels(text):
    """The labels of the arms of rig.sh's last `case "${1:-}"`, the dispatch."""
    start = text.rindex('case "${1:-}" in\n')
    body = text[start:]
    labels = []
    for match in re.finditer(r"^([a-z0-9*][a-z0-9*| -]*)\)$", body, re.MULTILINE):
        labels.extend(label.strip() for label in match.group(1).split("|"))
    return labels


def guard(text):
    """The label patterns that skip the check, and those that run it."""
    match = re.search(
        r"case \"\$\{1:-\}\" in\s+([^)]+)\) ;;\s+([^)]+)\)\s+"
        r"bash \"\$root/scripts/ci/check-x86-64-v3\.sh\"\s+;;\s+esac",
        text,
    )
    assert match, "the guard block of rig.sh is gone or changed shape"
    return [p.strip() for p in match.group(1).split("|")], [
        p.strip() for p in match.group(2).split("|")
    ]


def matches(label, patterns):
    return any(fnmatch.fnmatchcase(label, pattern) for pattern in patterns)


class RigV3Guard(unittest.TestCase):
    def setUp(self):
        self.text = RIG.read_text(encoding="utf-8")
        self.labels = [label for label in dispatch_labels(self.text) if label != "*"]
        self.no_check, self.check = guard(self.text)

    def checked(self, label):
        return not matches(label, self.no_check) and matches(label, self.check)

    def test_the_dispatch_was_found(self):
        self.assertIn("cargo", self.labels)
        self.assertIn("surface", self.labels)
        self.assertGreater(len(self.labels), 50)

    def test_every_subcommand_is_checked_or_skipped_with_a_reason(self):
        for label in self.labels:
            if label in SKIPPED:
                self.assertFalse(
                    self.checked(label),
                    f"{label} is skipped ({SKIPPED[label]}) but the guard runs",
                )
            else:
                self.assertTrue(
                    self.checked(label),
                    f"{label} is not classified: add it to the guard of rig.sh or to SKIPPED",
                )

    def test_no_stale_skip_entry(self):
        self.assertEqual(sorted(set(SKIPPED) - set(self.labels)), [])

    def test_the_surface_recipes_are_checked(self):
        for label in (
            "surface",
            "update-goldens",
            "golden-report",
            "cosmic-comp-version",
        ):
            self.assertTrue(self.checked(label), label)


if __name__ == "__main__":
    unittest.main()
