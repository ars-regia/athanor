"""A package directory restored from a published tier is reused only when the tier recorded the
hash and the image digest that are verified now (issue #1176)
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import json
import pathlib
import subprocess
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parents[1]
DIGEST = "sha256:" + "a" * 64
OTHER = "sha256:" + "b" * 64
REF = f"ghcr.io/o/athanor-forge-bar@{DIGEST}"


def bash(snippet):
    return subprocess.run(
        ["bash", "-c", f'source "{SCRIPTS}/tier_cache.sh"; {snippet}'],
        capture_output=True,
        text=True,
    )


class TierCacheTest(unittest.TestCase):
    def reusable(self, recorded, hash_, ref):
        return (
            bash(f'tier_cache_reusable "{recorded}" "{hash_}" "{ref}"').returncode == 0
        )

    def test_the_entry_names_the_hash_and_the_digest(self):
        self.assertEqual(
            f"ab12 {DIGEST}", bash(f'tier_cache_entry ab12 "{REF}"').stdout.strip()
        )

    def test_the_same_hash_and_digest_is_reused(self):
        self.assertTrue(self.reusable(f"ab12 {DIGEST}", "ab12", REF))

    def test_the_same_hash_with_another_digest_is_not_reused(self):
        # An earlier unverified image put its RPMs under the label hash this run verifies.
        self.assertFalse(self.reusable(f"ab12 {OTHER}", "ab12", REF))

    def test_another_hash_is_not_reused(self):
        self.assertFalse(self.reusable(f"cd34 {DIGEST}", "ab12", REF))

    def test_a_manifest_written_without_a_digest_is_never_reused(self):
        self.assertFalse(self.reusable("ab12", "ab12", REF))

    def test_an_image_not_named_by_digest_or_without_a_hash_is_never_reused(self):
        self.assertFalse(
            self.reusable(
                f"ab12 {DIGEST}", "ab12", "ghcr.io/o/athanor-forge-bar:latest"
            )
        )
        self.assertFalse(self.reusable(f" {DIGEST}", "", REF))
        self.assertEqual("", bash('tier_cache_entry "" "x@sha256:1"').stdout.strip())

    def test_the_script_asks_the_rule_and_records_the_entry(self):
        script = (SCRIPTS / "fetch_repo_rpms.sh").read_text()
        self.assertIn("tier_cache.sh", script)
        self.assertIn(
            'tier_cache_reusable "$old_digest" "$new_digest" "$IMAGE_LOWER"', script
        )
        self.assertIn('echo "$entry" > "$TMP_DIR/digest_$img"', script)
        self.assertNotIn('[ "$old_digest" = "$new_digest" ]', script)

    def test_the_manifest_keeps_the_two_words_of_an_entry(self):
        entry = f"ab12 {DIGEST}"
        out = subprocess.run(
            ["jq", "-r", 'to_entries[] | "\\(.key) \\(.value)"'],
            input=json.dumps({"img": entry}),
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        self.assertEqual(f"img {entry}", out.strip())


# The CI run's trust environment must not reach the scripts under test: on a pull request into
# rebase/*, GITHUB_BASE_REF makes tier-digests.sh read that branch's rules over the network (#1257).
TRUST_ENV = (
    "GITHUB_BASE_REF",
    "GITHUB_REF",
    "GITHUB_REPOSITORY",
    "GITHUB_SERVER_URL",
    "TIER_TRUSTED_REFS",
    "GITHUB_ACTIONS",
    "GH_TOKEN",
    "GITHUB_TOKEN",
)
_environ_patch = None


def setUpModule():
    global _environ_patch
    import os
    from unittest import mock

    _environ_patch = mock.patch.dict(os.environ)
    _environ_patch.start()
    for name in TRUST_ENV:
        os.environ.pop(name, None)


def tearDownModule():
    _environ_patch.stop()


if __name__ == "__main__":
    unittest.main()
