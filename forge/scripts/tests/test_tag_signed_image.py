"""tag_signed_image.sh makes hash-<hash> a registry-side copy of the signed digest and fails
when the tag does not resolve to it (python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "tag_signed_image.sh"
SIGNED = "sha256:" + "a" * 64
OTHER = "sha256:" + "b" * 64


class TagSignedImageTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        self.log = self.dir / "calls"

    def tag(self, resolves_to, digest=SIGNED):
        stub = self.dir / "skopeo"
        stub.write_text(
            '#!/usr/bin/env bash\necho "$*" >> "$STUB_LOG"\n'
            '[[ $1 == inspect ]] && echo "$STUB_DIGEST"\nexit 0\n'
        )
        stub.chmod(0o755)
        return subprocess.run(
            ["bash", SCRIPT, "ghcr.io/o/athanor-forge-bar", digest, "h1"],
            capture_output=True, text=True,
            env={**os.environ, "PATH": f"{self.dir}:{os.environ['PATH']}",
                 "STUB_LOG": str(self.log), "STUB_DIGEST": resolves_to, "RETRY_ATTEMPTS": "1"},
        )

    def test_copies_the_signed_digest_to_the_hash_tag(self):
        result = self.tag(SIGNED)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn(
            f"copy --preserve-digests docker://ghcr.io/o/athanor-forge-bar@{SIGNED} "
            "docker://ghcr.io/o/athanor-forge-bar:hash-h1",
            self.log.read_text(),
        )

    def test_a_tag_that_resolves_elsewhere_is_an_error(self):
        result = self.tag(OTHER)
        self.assertNotEqual(0, result.returncode)
        self.assertIn(OTHER, result.stderr)

    def test_a_tag_must_be_given_a_digest(self):
        self.assertNotEqual(0, self.tag(SIGNED, digest="latest").returncode)


if __name__ == "__main__":
    unittest.main()
