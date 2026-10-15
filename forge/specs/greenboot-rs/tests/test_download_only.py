"""A download-only staged deployment arms no greenboot rollback trigger (issue #454)
(python3 -B -m unittest discover -s forge/specs/greenboot-rs/tests -v).

athanor-update stages updates with `bootc upgrade --download-only`, and a poweroff discards
such a deployment. greenboot-set-rollback-trigger.service recorded it anyway, so the next
boot was taken for a GRUB fallback and rolled back to the previous deployment. The second
patch makes the staged deployment of bootc have no ID while it is download-only. These tests
hold the spec to applying it after the first patch and to running its tests in %check.
"""

import re
import unittest
from pathlib import Path

SPEC_DIR = Path(__file__).resolve().parents[1]
SPEC = SPEC_DIR / "greenboot-rs.spec"
AUTO_REBOOT = "0001-feat-add-GREENBOOT_AUTO_REBOOT-to-roll-back-without-.patch"
DOWNLOAD_ONLY = "0002-fix-set-no-rollback-trigger-for-a-download-only-stag.patch"
DELIBERATE_ROLLBACK = "0003-fix-do-not-take-a-rollback-made-on-purpose-for-a-GRU.patch"
SECTIONS = (
    "prep build install check post preun postun files changelog package description"
)


def section(spec, name):
    """The body of the spec section NAME, up to the next section header."""
    body = spec.split(f"\n{name}\n", 1)[1]
    return re.split(
        r"(?m)^%(?:" + "|".join(SECTIONS.split()) + r")\b", body, maxsplit=1
    )[0]


def diff_lines(patch, sign):
    """The lines the diff of PATCH adds ("+") or removes ("-"), without the commit message
    before it and the signature after it."""
    body = patch.split("\ndiff --git ", 1)[1].rsplit("\n-- \n", 1)[0]
    return [
        line[1:]
        for line in body.splitlines()
        if line.startswith(sign) and not line.startswith(sign * 3)
    ]


def added_lines(patch):
    return "\n".join(diff_lines(patch, "+"))


class DownloadOnly(unittest.TestCase):
    def test_spec_applies_the_patches_in_order(self):
        spec = SPEC.read_text()
        patches = re.findall(r"(?m)^Patch(\d+):\s*(\S+)$", spec)
        self.assertEqual(
            patches,
            [("0", AUTO_REBOOT), ("1", DOWNLOAD_ONLY), ("2", DELIBERATE_ROLLBACK)],
        )
        self.assertIn("%autosetup -p1", section(spec, "%prep"))
        for name in (AUTO_REBOOT, DOWNLOAD_ONLY, DELIBERATE_ROLLBACK):
            self.assertTrue((SPEC_DIR / "SOURCES" / name).is_file(), name)

    def test_patch_changes_only_the_bootc_staged_lookup(self):
        patch = (SPEC_DIR / "SOURCES" / DOWNLOAD_ONLY).read_text()
        self.assertEqual(
            re.findall(r"(?m)^diff --git a/(\S+)", patch), ["src/lib/handler.rs"]
        )
        removed = diff_lines(patch, "-")
        # Only the tail of get_bootc_deployment_id moves into the parsing function; the
        # rpm-ostree lookup is not touched.
        self.assertFalse(
            [line for line in removed if "rpm" in line or "ostree" in line], removed
        )
        added = added_lines(patch)
        self.assertRegex(
            added, r'key == "staged"\s*&& deployment\s*\.get\("downloadOnly"\)'
        )
        self.assertIn(".unwrap_or(false)", added)

    def test_patch_tests_both_values_of_the_flag(self):
        added = added_lines((SPEC_DIR / "SOURCES" / DOWNLOAD_ONLY).read_text())
        self.assertIn(
            'assert_eq!(bootc_deployment_id(&status(Some(true)), "staged"), None);',
            added,
        )
        self.assertRegex(
            added,
            r'bootc_deployment_id\(&status\(Some\(false\)\), "staged"\)\.as_deref\(\),\s*Some\("sha256:0e59"\)',
        )

    def test_check_runs_the_tests_of_both_patches(self):
        check = section(SPEC.read_text(), "%check")
        self.assertIn("cargo test --release --locked --lib handler::tests", check)
        self.assertIn("cargo test --release --locked --bin greenboot", check)


class DeliberateRollback(unittest.TestCase):
    """The third patch (#521): a boot that starts the default deployment is no GRUB fallback,
    so `GoBack()` is not undone when the health check of the deployment left did not finish."""

    def setUp(self):
        self.patch = (SPEC_DIR / "SOURCES" / DELIBERATE_ROLLBACK).read_text()

    def test_patch_changes_the_fallback_detection_and_the_default_lookup(self):
        self.assertEqual(
            re.findall(r"(?m)^diff --git a/(\S+)", self.patch),
            ["src/lib/handler.rs", "src/main.rs"],
        )
        added = added_lines(self.patch)
        self.assertRegex(added, r"if booted_is_default\(\) == Some\(true\) \{")
        self.assertIn('.get("rollbackQueued")', added)
        # The stale trigger is cleared, as after a fallback made permanent.
        self.assertIn("unset_next_deployment_id()?;", added)

    def test_patch_tests_both_lookups(self):
        added = added_lines(self.patch)
        self.assertIn(
            "assert_eq!(bootc_booted_is_default(&queued(json!(true))), Some(false));",
            added,
        )
        self.assertRegex(
            added,
            r"rpm_ostree_booted_is_default\(&deployments\(\[false, true\]\)\),\s*Some\(false\)",
        )


if __name__ == "__main__":
    unittest.main()
