"""publish_iso.sh refuses a build without an ISO and a system image whose digest moved since the
push (python3 -B -m unittest discover -s scripts/ci/tests)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[3] / "forge" / "scripts" / "publish_iso.sh"
PUSHED = "sha256:" + "a" * 64
MOVED = "sha256:" + "b" * 64


class PublishIsoTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        (self.dir / "artifacts").mkdir()
        (self.dir / "output").mkdir()
        (self.dir / "artifacts" / "image-digests.txt").write_text(f"r/athanor-system tag {PUSHED}\n")
        (self.dir / "artifacts" / "iso-image.digest").write_text("sha256:" + "c" * 64)
        self.bin = self.dir / "bin"
        self.bin.mkdir()
        # sudo runs its arguments, so the stubbed podman answers "image inspect".
        stubs = {
            "podman": '[[ $1 == image ]] && echo "$STUB_SYSTEM_DIGEST"; exit 0',
            "sudo": 'exec "$@"',
        }
        for name, body in stubs.items():
            stub = self.bin / name
            stub.write_text(f"#!/usr/bin/env bash\n{body}\n")
            stub.chmod(0o755)

    def publish(self, system_digest=PUSHED):
        return subprocess.run(
            ["bash", SCRIPT], cwd=self.dir, stdin=subprocess.DEVNULL, capture_output=True, text=True,
            env={**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}", "STUB_SYSTEM_DIGEST": system_digest,
                 "IMAGE_REGISTRY": "r", "RUN_ID": "1", "GITHUB_TOKEN": "t", "GITHUB_ACTOR": "a",
                 "GITHUB_SHA": "s", "GITHUB_SERVER_URL": "u", "GITHUB_REPOSITORY": "o/r",
                 "GITHUB_STEP_SUMMARY": str(self.dir / "summary"), "RETRY_ATTEMPTS": "1"},
        )

    def test_no_iso_is_an_error(self):
        result = self.publish()
        self.assertEqual(1, result.returncode)
        self.assertIn("no .iso", result.stderr)

    def test_a_moved_system_image_is_an_error(self):
        (self.dir / "output" / "a.iso").write_text("iso")
        result = self.publish(MOVED)
        self.assertEqual(1, result.returncode)
        self.assertIn("is not the pushed one", result.stderr)

    def test_the_digests_are_recorded(self):
        (self.dir / "output" / "a.iso").write_text("iso")
        result = self.publish()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(f"r/athanor-iso sha256:{'c' * 64}\nr/athanor-system {PUSHED}\n",
                         (self.dir / "artifacts" / "iso-digest.txt").read_text())


if __name__ == "__main__":
    unittest.main()
