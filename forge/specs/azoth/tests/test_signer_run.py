"""Unit test of forge/specs/azoth/signer/run.sh with a stand-in podman
(python3 -B -m unittest discover -s forge/specs/azoth/tests -v): the signer runs by digest and
without network, and the keys never outlive the step."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[4]
DIGEST = "sha256:" + "a" * 64
REG = "ghcr.io/ars-regia"

# Records each call; for `run`, also whether every mounted key file is readable right then.
PODMAN = """#!/usr/bin/env bash
set -euo pipefail
echo "podman $*" >> "$PODMAN_LOG"
if [[ $1 == run ]]; then
  for arg in "$@"; do
    case $arg in /*:/run/keys/*) [[ -s ${arg%%:*} ]] && echo "key readable ${arg%%:*}" >> "$PODMAN_LOG" ;; esac
  done
fi
"""


class SignerRun(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        for path in (
            "forge/specs/azoth/signer/run.sh",
            "system/kernel-artifacts.sh",
            "forge/scripts/retry.sh",
        ):
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(REPO / path, self.root / path)
        (self.root / "kernel-artifacts").mkdir()
        (self.root / "kernel-artifacts/kernel-artifacts.env").write_text(
            f"state=modules-missing\nregistry={REG}\n"
        )
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        (bin_dir / "podman").write_text(PODMAN)
        (bin_dir / "podman").chmod(0o755)
        self.log = self.root / "podman.log"
        self.env = {
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "HOME": str(self.root),
            "PODMAN_LOG": str(self.log),
            "RETRY_ATTEMPTS": "1",
            "TMPDIR": str(self.root),
        }

    def run_script(self, stage, **env):
        return subprocess.run(
            ["bash", "forge/specs/azoth/signer/run.sh", stage],
            cwd=self.root,
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def test_without_a_committed_digest_nothing_runs(self):
        r = self.run_script("verify")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("signer/image.digest is missing", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_sign_without_both_keys_runs_nothing(self):
        (self.root / "forge/specs/azoth/signer/image.digest").write_text(DIGEST + "\n")
        r = self.run_script("sign", MODULE_SIGNING_KEY="module secret")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("SECUREBOOT_SIGNING_KEY", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_sign_runs_the_pinned_signer_offline_and_drops_the_keys(self):
        (self.root / "forge/specs/azoth/signer/image.digest").write_text(DIGEST + "\n")
        r = self.run_script(
            "sign",
            MODULE_SIGNING_KEY="module secret",
            SECUREBOOT_SIGNING_KEY="secure boot secret",
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertNotIn("secret", r.stdout + r.stderr)
        calls = self.calls()
        self.assertEqual(calls[0], f"podman pull {REG}/azoth-signer@{DIGEST}")
        runs = [c for c in calls if c.startswith("podman run ")]
        self.assertEqual(len(runs), 2)
        for run in runs:
            self.assertIn("--network=none", run)
            self.assertIn("--pull=never", run)
            self.assertIn(
                f" {REG}/azoth-signer@{DIGEST} bash forge/specs/azoth/sign-kernel.sh ",
                run,
            )
        self.assertIn(" modules --key /run/keys/module ", runs[0])
        self.assertIn(" vmlinuz --key /run/keys/secureboot ", runs[1])
        keys = [c.split()[-1] for c in calls if c.startswith("key readable ")]
        self.assertEqual(len(keys), 2)
        for key in keys:
            self.assertFalse(pathlib.Path(key).exists(), f"{key} survived the step")

    def test_a_malformed_digest_fails(self):
        (self.root / "forge/specs/azoth/signer/image.digest").write_text("latest\n")
        r = self.run_script("verify")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("not a sha256 digest", r.stderr)
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
