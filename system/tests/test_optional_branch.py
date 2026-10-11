"""system/optional-branch.sh: an optional NVIDIA branch that fails leaves the run (ADR-0103 D24)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / "system" / "optional-branch.sh"


# A build that writes part of a module, then fails: what a half-built legacy branch leaves.
HALF_BUILT = 'mkdir -p "$OUT" && echo partial > "$OUT/nvidia.ko" && exit 3'
BUILT = 'mkdir -p "$OUT" && echo whole > "$OUT/nvidia.ko"'


class OptionalBranch(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        self.out = self.dir / "out"
        self.marker = self.dir / "out.dropped"

    def run_with(self, optional, command):
        summary = self.dir / "summary.md"
        env = {**os.environ, "GITHUB_STEP_SUMMARY": str(summary), "OUT": str(self.out)}
        r = subprocess.run(["bash", str(SCRIPT), "legacy", optional, str(self.out), "--", "bash", "-c", command],
                           capture_output=True, text=True, env=env)
        return r, summary.read_text() if summary.exists() else ""

    def test_a_failed_optional_branch_is_named_and_the_step_succeeds(self):
        r, summary = self.run_with("true", "exit 3")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("::error title=Branch dropped::the legacy NVIDIA modules", r.stdout)
        self.assertIn("legacy", summary)
        self.assertTrue(self.marker.is_file())

    def test_a_half_built_optional_branch_never_reaches_the_upload(self):
        # The upload step runs only without the marker; the output is gone either way.
        r, _ = self.run_with("true", HALF_BUILT)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(self.out.exists())
        self.assertTrue(self.marker.is_file())

    def test_a_source_that_does_not_match_its_pin_stops_even_an_optional_branch(self):
        # nvidia.sh exits 4 when a pinned source (the legacy .run's sha256) does not match:
        # an integrity failure, never a build failure the run may leave behind.
        r, summary = self.run_with("true", 'mkdir -p "$OUT" && exit 4')
        self.assertEqual(r.returncode, 4)
        self.assertFalse(self.marker.exists())
        self.assertNotIn("Branch dropped", r.stdout)

    def test_a_required_branch_fails_with_its_status(self):
        r, summary = self.run_with("false", HALF_BUILT)
        self.assertEqual(r.returncode, 3)
        self.assertEqual(summary, "")
        self.assertFalse(self.marker.exists())

    def test_a_branch_that_builds_keeps_its_output_and_says_nothing(self):
        r, summary = self.run_with("true", BUILT)
        self.assertEqual((r.returncode, summary), (0, ""))
        self.assertNotIn("::error", r.stdout)
        self.assertEqual((self.out / "nvidia.ko").read_text(), "whole\n")
        self.assertFalse(self.marker.exists())

    def test_bad_arguments_are_refused(self):
        for args in (["legacy", "maybe", str(self.out), "--", "true"], ["legacy", "true", "--", "true"]):
            with self.subTest(args=args):
                self.assertEqual(subprocess.run(["bash", str(SCRIPT), *args], capture_output=True).returncode, 2)


if __name__ == "__main__":
    unittest.main()
