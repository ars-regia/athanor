"""verify_image.sh refuses a reference it cannot bind to a hash (issue #1176)
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import pathlib
import subprocess
import tempfile
import unittest

from signed_registry import TRUSTED, signed_registry_env, stub_cosign, stub_skopeo

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify_image.sh"


class VerifyImageTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.state = pathlib.Path(tmp.name)
        (self.state / "bin").mkdir()
        stub_skopeo(self.state / "bin")
        stub_cosign(self.state / "bin")
        (self.state / "signer").write_text(TRUSTED)

    def verify(self, ref):
        return subprocess.run(
            ["bash", SCRIPT, ref, "call-dag-compile.yml"],
            capture_output=True,
            text=True,
            env=signed_registry_env(self.state / "bin", self.state),
        )

    def test_an_empty_hash_or_tag_is_a_usage_error_never_signed(self):
        # The stub labels an image with the hash of the tag asked; "none" leaves it unlabelled,
        # so an empty hash would equal the empty label if it were accepted.
        (self.state / "label").write_text("none")
        for ref in (
            "ghcr.io/o/athanor-forge-bar:hash-",
            "ghcr.io/o/athanor-forge-bar:",
        ):
            with self.subTest(ref=ref):
                result = self.verify(ref)
                self.assertEqual(2, result.returncode, result.stdout)
                self.assertNotIn("signed", result.stdout)
                self.assertIn("usage", result.stderr)

    def test_a_hash_tag_with_its_label_is_signed(self):
        result = self.verify("ghcr.io/o/athanor-forge-bar:hash-ab12")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue(result.stdout.startswith("signed "), result.stdout)


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


if __name__ == "__main__":
    unittest.main()
