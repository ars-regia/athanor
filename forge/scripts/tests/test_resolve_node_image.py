"""resolve_node_image.sh names the image of the brain's hash by the digest a trusted run signed,
never by :latest or by a tag (python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import json
import pathlib
import subprocess
import tempfile
import unittest

from signed_registry import DIGEST, TRUSTED, WORKFLOWS, signed_registry_env, stub_cosign, stub_skopeo

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "resolve_node_image.sh"


class ResolveNodeImageTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.state = pathlib.Path(tmp.name)
        (self.state / "bin").mkdir()
        stub_skopeo(self.state / "bin")
        stub_cosign(self.state / "bin")
        (self.state / "signer").write_text(TRUSTED)

    def resolve(self, package):
        return subprocess.run(
            ["bash", SCRIPT, "ghcr.io/owner", package], capture_output=True, text=True,
            env={**signed_registry_env(self.state / "bin", self.state), "DAG_STATE_DIR": str(self.state)},
        )

    def test_names_the_signed_digest_of_the_hash_tag(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        result = self.resolve("bar")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(f"ghcr.io/owner/athanor-forge-bar@{DIGEST}", result.stdout.strip())
        self.assertIn(f"ghcr.io/owner/athanor-forge-bar@{DIGEST}", (self.state / "cosign.log").read_text())

    def test_an_image_without_a_trusted_signature_is_not_fetched(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        for signer in (None, WORKFLOWS + "call-dag-compile.yml@refs/heads/feature"):
            with self.subTest(signer=signer):
                (self.state / "signer").unlink(missing_ok=True)
                if signer:
                    (self.state / "signer").write_text(signer)
                result = self.resolve("bar")
                self.assertNotEqual(0, result.returncode)
                self.assertEqual("", result.stdout)
                self.assertIn("is not a trusted build of its hash", result.stderr)

    def test_a_signed_image_labelled_with_another_hash_or_none_is_not_fetched(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        for label in ("cd34", "none"):
            with self.subTest(label=label):
                (self.state / "label").write_text(label)
                result = self.resolve("bar")
                self.assertNotEqual(0, result.returncode)
                self.assertEqual("", result.stdout)
                self.assertIn("not the tag's hash", result.stderr)

    def test_a_verification_that_cannot_be_answered_is_an_error(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        (self.state / "cosign_error").write_text("")
        result = self.resolve("bar")
        self.assertNotEqual(0, result.returncode)
        self.assertEqual("", result.stdout)

    def test_a_package_without_a_hash_is_an_error(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        self.assertNotEqual(0, self.resolve("dock").returncode)

    def test_a_missing_map_is_an_error(self):
        self.assertNotEqual(0, self.resolve("bar").returncode)


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
