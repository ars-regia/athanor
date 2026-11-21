"""The builder image is published labelled with its content hash and signed, and run and
promoted only by a digest a trusted run of call-build-builder.yml signed (issue #1205):
publish_builder.sh, resolve_builder_image.sh and promote_builder_latest.sh
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import pathlib
import subprocess
import tempfile
import unittest

from signed_registry import (
    BUILDER_TRUSTED,
    DIGEST,
    TRUSTED,
    WORKFLOWS,
    signed_registry_env,
    stub_cosign,
    stub_skopeo,
)

SCRIPTS = pathlib.Path(__file__).resolve().parents[1]
UNTRUSTED = (None, TRUSTED, WORKFLOWS + "call-build-builder.yml@refs/heads/feature")
CONTENT_HASH = "ab" * 32

# The build tools publish_builder.sh runs, each logging its arguments to <tool>.log: nix names an
# archive, podman loads it and writes DIGEST as the pushed digest, buildah names its container.
TOOL_STUBS = {
    "nix": 'echo "$ANSWER_DIR/builder.tar"\n',
    "podman": """case $1 in
load) echo "Loaded image: localhost/athanor-builder:nix" ;;
login) cat > /dev/null ;;
push) [[ $2 == --digestfile ]] && echo "%s" > "$3" ;;
esac
"""
    % DIGEST,
    "buildah": """case $1 in
from) echo ctr-1 ;;
commit) echo image-1 ;;
rm) echo ctr-1 ;;
esac
""",
    "sleep": "",
}


class BuilderTrustTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.state = pathlib.Path(tmp.name)
        (self.state / "bin").mkdir()
        stub_skopeo(self.state / "bin")
        stub_cosign(self.state / "bin")

    def run_script(self, script, *args, signer=BUILDER_TRUSTED, env=None):
        (self.state / "signer").unlink(missing_ok=True)
        if signer:
            (self.state / "signer").write_text(signer)
        (self.state / "skopeo.log").unlink(missing_ok=True)
        return subprocess.run(
            ["bash", SCRIPTS / script, *args],
            capture_output=True,
            text=True,
            env={**signed_registry_env(self.state / "bin", self.state), **(env or {})},
        )

    def log(self, tool):
        log = self.state / f"{tool}.log"
        return log.read_text() if log.exists() else ""

    def publish(self, signer=BUILDER_TRUSTED):
        for tool, body in TOOL_STUBS.items():
            stub = self.state / "bin" / tool
            stub.write_text(
                f'#!/usr/bin/env bash\nprintf \'%s\\n\' "$*" >> "$ANSWER_DIR/{tool}.log"\n{body}'
            )
            stub.chmod(0o755)
        return self.run_script(
            "publish_builder.sh",
            "ghcr.io/owner/athanor-builder",
            CONTENT_HASH,
            signer=signer,
            env={"GITHUB_TOKEN": "token", "GITHUB_ACTOR": "actor"},
        )

    def test_publish_labels_the_image_with_its_hash_and_prints_only_the_signed_digest(
        self,
    ):
        result = self.publish()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(f"ghcr.io/owner/athanor-builder@{DIGEST}\n", result.stdout)
        buildah = self.log("buildah").splitlines()
        self.assertIn(
            f"config --label tier.content.sha256={CONTENT_HASH} ctr-1", buildah
        )
        self.assertLess(
            buildah.index(f"config --label tier.content.sha256={CONTENT_HASH} ctr-1"),
            buildah.index(
                f"commit --omit-timestamp ctr-1 ghcr.io/owner/athanor-builder:{CONTENT_HASH}"
            ),
        )
        self.assertIn(
            f"ghcr.io/owner/athanor-builder:{CONTENT_HASH}",
            self.log("podman").splitlines()[-1],
        )
        self.assertIn(
            f"sign --yes ghcr.io/owner/athanor-builder@{DIGEST}", self.log("cosign")
        )

    def test_publish_fails_when_the_pushed_digest_does_not_verify_as_signed(self):
        result = self.publish(signer=TRUSTED)
        self.assertNotEqual(0, result.returncode)
        self.assertEqual("", result.stdout)
        self.assertIn(
            "does not verify as signed by a trusted run of call-build-builder.yml",
            result.stderr,
        )

    def test_resolve_names_the_signed_digest_of_the_tag(self):
        result = self.run_script("resolve_builder_image.sh", "ghcr.io/owner", "latest")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(f"ghcr.io/owner/athanor-builder@{DIGEST}", result.stdout.strip())

    def test_resolve_binds_latest_by_its_signature_alone(self):
        # :latest names no hash, so no label is asked of it; promote_builder_latest.sh moves it
        # only to a digest the label check passed.
        (self.state / "label").write_text("none")
        result = self.run_script("resolve_builder_image.sh", "ghcr.io/owner", "latest")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertNotIn("--config", self.log("skopeo"))

    def test_resolve_refuses_a_builder_no_trusted_run_signed(self):
        for signer in UNTRUSTED:
            with self.subTest(signer=signer):
                result = self.run_script("resolve_builder_image.sh", "ghcr.io/owner", "latest", signer=signer)
                self.assertNotEqual(0, result.returncode)
                self.assertEqual("", result.stdout)
                self.assertIn("is not a builder a trusted run of call-build-builder.yml signed", result.stderr)

    def test_resolve_fails_when_the_tag_is_absent_or_the_check_cannot_answer(self):
        (self.state / "cosign_error").write_text("")
        result = self.run_script("resolve_builder_image.sh", "ghcr.io/owner", "latest")
        self.assertNotEqual(0, result.returncode)
        self.assertEqual("", result.stdout)
        (self.state / "cosign_error").unlink()
        (self.state / "status").write_text("1")
        (self.state / "answer").write_text("manifest unknown")
        result = self.run_script("resolve_builder_image.sh", "ghcr.io/owner", "latest")
        self.assertNotEqual(0, result.returncode)
        self.assertIn("is not published", result.stderr)

    def promote(
        self,
        image=f"ghcr.io/owner/athanor-builder@{DIGEST}",
        ref="refs/heads/iso-v0",
        **kw,
    ):
        return self.run_script("promote_builder_latest.sh", image, ref, "iso-v0", **kw)

    def test_latest_moves_to_the_verified_digest(self):
        result = self.promote()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn(
            f"copy --all --preserve-digests docker://ghcr.io/owner/athanor-builder@{DIGEST} "
            "docker://ghcr.io/owner/athanor-builder:latest",
            self.log("skopeo"),
        )

    def test_latest_never_moves_to_a_builder_no_trusted_run_signed(self):
        for signer in UNTRUSTED:
            with self.subTest(signer=signer):
                result = self.promote(signer=signer)
                self.assertNotEqual(0, result.returncode)
                self.assertNotIn("copy", self.log("skopeo"))
                self.assertIn(":latest is not moved", result.stderr)

    def test_latest_takes_only_a_digest_and_only_on_the_default_branch(self):
        result = self.promote(image="ghcr.io/owner/athanor-builder:" + "a" * 64)
        self.assertEqual(2, result.returncode)
        self.assertNotIn("copy", self.log("skopeo"))
        result = self.promote(ref="refs/heads/feature")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertNotIn("copy", self.log("skopeo"))


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
