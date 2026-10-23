"""Unit tests of the CI mode of scripts/devvm/acceptance/run.sh (python3 -B -m unittest
discover -s scripts/tests -t scripts/tests -p test_acceptance_ci.py).

`run.sh --ci` runs the upgrade and rollback stages of doc_update_delivery.md UD18 on the
VM of forge/test/iso/run_iso_test.sh. Only what it decides before reaching the guest is
tested here: the guest itself needs QEMU, Secure Boot firmware and a candidate image.
systemctl, gh, ssh and df are stubs on PATH; HOME is a temporary directory.
"""

import os
import pathlib
import subprocess
import tempfile
import unittest

RUN = pathlib.Path(__file__).resolve().parents[1] / "devvm" / "acceptance" / "run.sh"
DIGEST = "sha256:" + "a" * 64


class CiMode(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        self.bin = self.dir / "bin"
        self.bin.mkdir()
        self.asked = self.dir / "gh-asked"
        # The host's runner unit is active, and one of its runners is busy.
        self.stub("systemctl", "exit 0")
        self.stub("gh", f"touch {self.asked}; echo 1")
        # The guest never answers: nothing in these tests may need it.
        self.stub("ssh", "exit 255")
        self.stub("df", "echo Avail; echo 100")
        (self.dir / "home").mkdir()
        (self.dir / "out").mkdir()
        self.env = {
            key: value
            for key, value in os.environ.items()
            if key not in ("CI", "GITHUB_ACTIONS")
        }
        self.env.update(
            PATH=f"{self.bin}:{os.environ['PATH']}",
            HOME=str(self.dir / "home"),
        )
        self.ci = {
            "CI": "true",
            "ATHANOR_UPDATE_TO": DIGEST,
            "ATHANOR_UPDATE_REPOSITORY": "ghcr.io/owner/athanor-system",
            "ATHANOR_UPDATE_MIRROR": "10.0.2.2:5000/owner/athanor-system",
            "ACC_CI_OUT": str(self.dir / "out"),
            "SSH_PORT": "2222",
            "ACC_SSH_KEY": str(self.dir / "id"),
        }

    def tearDown(self):
        self.tmp.cleanup()

    def stub(self, name, body):
        path = self.bin / name
        path.write_text(f"#!/bin/sh\n{body}\n")
        path.chmod(0o755)

    def run_sh(self, *args, env=None, timeout=60):
        return subprocess.run(
            ["bash", str(RUN), *args],
            capture_output=True,
            text=True,
            env={**self.env, **(env or {})},
            timeout=timeout,
            check=False,
        )

    def test_ci_mode_refuses_outside_ci(self):
        result = self.run_sh(
            "--ci", env={k: v for k, v in self.ci.items() if k != "CI"}
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("CI", result.stderr)

    def test_ci_mode_names_a_missing_variable(self):
        for missing in self.ci:
            if missing == "CI":
                continue
            env = {k: v for k, v in self.ci.items() if k != missing}
            result = self.run_sh("--ci", env=env)
            self.assertNotEqual(result.returncode, 0, missing)
            self.assertIn(missing, result.stderr, missing)

    def test_ci_mode_rejects_a_bad_digest(self):
        result = self.run_sh("--ci", env={**self.ci, "ATHANOR_UPDATE_TO": "stable"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ATHANOR_UPDATE_TO", result.stderr)

    def test_ci_mode_rejects_a_dev_stage(self):
        result = self.run_sh("--ci", "install", env=self.ci)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown stage", result.stderr)

    def test_ci_mode_does_not_ask_github(self):
        # An answering guest with nothing to say: the seed stage stops at the digest.
        self.stub("ssh", "exit 0")
        result = self.run_sh("--ci", env=self.ci)
        self.assertFalse(
            self.asked.exists(), "run.sh --ci asked GitHub about the runners"
        )
        self.assertIn("== seed", result.stdout, result.stderr)
        self.assertNotEqual(result.returncode, 0)

    def test_dev_mode_refuses_beside_a_busy_runner(self):
        result = self.run_sh()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("busy", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
