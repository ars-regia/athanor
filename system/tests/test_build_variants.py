"""system/build-variants.sh: an NVIDIA variant that fails to build leaves the run, the
default image does not (ADR-0103 D24)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "build-variants.sh"


class BuildVariants(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        self.stub = self.dir / "build-image.sh"
        self.out = self.dir / "variants.txt"
        self.summary = self.dir / "summary.md"

    def tearDown(self):
        self.tmp.cleanup()

    def run_with(self, failing=()):
        # The stub fails for the GPUs named in FAIL and records every call.
        self.stub.write_text(
            "#!/usr/bin/env bash\n"
            'echo "$*" >> "$CALLS"\n'
            "while [[ $# -gt 0 ]]; do [[ $1 == --gpu ]] && gpu=$2; shift; done\n"
            '[[ " $FAIL " != *" $gpu "* ]]\n'
        )
        env = {
            **os.environ,
            "BUILD_IMAGE": str(self.stub),
            "FAIL": " ".join(failing),
            "CALLS": str(self.dir / "calls"),
            "GITHUB_STEP_SUMMARY": str(self.summary),
        }
        return subprocess.run(
            [
                "bash",
                str(SCRIPT),
                "--system-image",
                "abc",
                "--registry",
                "r.example/o",
                "--tag",
                "412",
                "--serial",
                "7",
                "--out",
                str(self.out),
            ],
            capture_output=True,
            text=True,
            env=env,
        )

    def test_all_three_variants_built(self):
        r = self.run_with()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.out.read_text().split(),
            ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"],
        )
        self.assertNotIn("::error", r.stdout)
        self.assertEqual(
            (self.dir / "calls").read_text().splitlines(),
            [
                f"--gpu {gpu} --system-image abc --registry r.example/o --tag 412 --serial 7"
                for gpu in ("none", "nvidia", "nvidia-legacy")
            ],
        )

    def test_a_failed_legacy_build_drops_only_the_legacy_image(self):
        r = self.run_with(["nvidia-legacy"])
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.out.read_text().split(), ["athanor-system", "athanor-system-nvidia"]
        )
        self.assertIn(
            "::error title=Variant dropped::athanor-system-nvidia-legacy", r.stdout
        )
        self.assertIn("athanor-system-nvidia-legacy", self.summary.read_text())

    def test_both_nvidia_variants_may_drop(self):
        r = self.run_with(["nvidia", "nvidia-legacy"])
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.out.read_text().split(), ["athanor-system"])

    def test_a_failed_default_image_fails_the_run(self):
        r = self.run_with(["none"])
        self.assertEqual(r.returncode, 1)
        self.assertFalse(self.out.exists())


if __name__ == "__main__":
    unittest.main()
