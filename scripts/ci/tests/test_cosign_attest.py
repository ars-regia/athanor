"""scripts/ci/cosign-attest-check.sh: system/sign-images.sh --attestations against the real
cosign and skopeo and a throwaway registry (#705). It needs cosign, skopeo, registry and python3
on PATH: scripts/ci/install-tools.sh installs cosign and registry at the pinned version, and
fails here, not skips, where they are missing."""

import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
CHECK = ROOT / "scripts/ci/cosign-attest-check.sh"
INSTALL = ROOT / "scripts/ci/install-tools.sh"
WORKFLOWS = ROOT / ".github/workflows"
INSTALLER = "sigstore/cosign-installer@"


def run_check(**env):
    return subprocess.run(
        ["bash", str(CHECK)],
        env={**os.environ, **env},
        capture_output=True,
        text=True,
        timeout=300,
    )


class PinnedVersion(unittest.TestCase):
    def test_workflows_install_the_cosign_the_check_runs(self):
        pinned = re.search(r"^COSIGN_INSTALLER=([0-9a-f]{40})$", INSTALL.read_text(), re.M)
        self.assertIsNotNone(pinned)
        used = set()
        for workflow in WORKFLOWS.glob("*.yml"):
            used |= set(re.findall(rf"{INSTALLER}(\S+)", workflow.read_text()))
        self.assertEqual(used, {pinned.group(1)})

    def test_every_download_has_a_digest(self):
        text = INSTALL.read_text()
        for name in ("COSIGN", "REGISTRY"):
            self.assertRegex(text, rf"(?m)^{name}_SHA256=[0-9a-f]{{64}}$")


class RealCosign(unittest.TestCase):
    def test_sign_images_attests_and_the_attestation_verifies(self):
        run = run_check()
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("attested:", run.stdout)
        self.assertIn("attestation verified:", run.stdout)

    def test_the_check_fails_without_the_flag_705_needed(self):
        flag = " --use-signing-config=false"
        with tempfile.TemporaryDirectory() as tmp:
            tree = pathlib.Path(tmp)
            # sign-images.sh finds its helpers from its own location: link them all but itself.
            for name in ("forge", "scripts"):
                (tree / name).symlink_to(ROOT / name)
            (tree / "system").mkdir()
            for path in (ROOT / "system").iterdir():
                if path.name != "sign-images.sh":
                    (tree / "system" / path.name).symlink_to(path)
            text = (ROOT / "system/sign-images.sh").read_text()
            self.assertIn(flag, text)
            copy = tree / "system/sign-images.sh"
            copy.write_text(text.replace(flag, ""))
            run = run_check(SIGN_IMAGES=str(copy))
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("--use-signing-config", run.stdout + run.stderr)


if __name__ == "__main__":
    unittest.main()
