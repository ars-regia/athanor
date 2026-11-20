"""UD42: the builder hash covers the builder's inputs only
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

from signed_registry import (BUILDER_TRUSTED, DIGEST, TRUSTED, WORKFLOWS, signed_registry_env, stub_cosign,
                             stub_skopeo)

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check_idempotency.sh"


class BuilderHashTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.forge = root / "forge"
        (self.forge / "builder").mkdir(parents=True)
        (self.forge / "config").mkdir()
        (self.forge / "builder/verify_compilers.sh").write_text("true\n")
        (self.forge / "config/packages.json").write_text('{"custom_packages": []}\n')
        (root / "flake.nix").write_text("{}\n")
        (root / "flake.lock").write_text("{}\n")
        self.root = root

    def builder_hash(self):
        out = subprocess.run(
            ["bash", SCRIPT, "--package", "builder", "--hash-only"],
            cwd=self.forge, capture_output=True, text=True, check=True,
        ).stdout
        return out.strip().removeprefix("CONTENT_HASH=")

    def test_packages_json_is_not_a_builder_input(self):
        before = self.builder_hash()
        (self.forge / "config/packages.json").write_text('{"custom_packages": ["x"]}\n')
        self.assertEqual(before, self.builder_hash())

    def test_flake_lock_is_a_builder_input(self):
        before = self.builder_hash()
        (self.root / "flake.lock").write_text('{"nodes": {}}\n')
        self.assertNotEqual(before, self.builder_hash())


class PackageHashTest(unittest.TestCase):
    """A package builds from the Cargo path dependencies of its crates, so its hash covers them."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.forge = root / "forge"
        (self.forge / "config").mkdir(parents=True)
        for rel, deps in (
            ("system/shared", ""),
            ("system/unrelated", ""),
            ("forge/specs/athanor-dock/dock-1.0.0", 'shared = { path = "../../../../system/shared" }\n'),
        ):
            crate = root / rel
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                f'[package]\nname = "{crate.name}"\nversion = "1.0.0"\n\n[dependencies]\n{deps}'
            )
            (crate / "lib.rs").write_text("// v1\n")
        (self.forge / "specs/athanor-dock/dock.spec").write_text("Name: athanor-dock\n")
        self.root = root

    def dock_hash(self, locale=None):
        env = {**os.environ, "LC_ALL": locale} if locale else None
        out = subprocess.run(
            ["bash", SCRIPT, "--package", "dock", "--hash-only"],
            cwd=self.forge, capture_output=True, text=True, check=True, env=env,
        ).stdout
        return out.strip().removeprefix("CONTENT_HASH=")

    def test_a_path_dependency_change_changes_the_hash(self):
        before = self.dock_hash()
        (self.root / "system/shared/lib.rs").write_text("// v2\n")
        self.assertNotEqual(before, self.dock_hash())

    def test_an_unrelated_crate_change_keeps_the_hash(self):
        before = self.dock_hash()
        (self.root / "system/unrelated/lib.rs").write_text("// v2\n")
        self.assertEqual(before, self.dock_hash())

    def test_workspace_lock_build_scripts_and_declared_inputs_are_inputs(self):
        (self.forge / "specs/athanor-dock/dock.spec").write_text(
            "# repo-input: system/unrelated\nName: athanor-dock\n%build\ncargo build\n"
        )
        (self.root / "Cargo.lock").write_text("v1\n")
        (self.forge / "scripts").mkdir()
        (self.forge / "scripts/build_spec.sh").write_text("v1\n")
        before = self.dock_hash()
        for path in ("Cargo.lock", "forge/scripts/build_spec.sh", "system/unrelated/lib.rs"):
            original = (self.root / path).read_text()
            (self.root / path).write_text("changed\n")
            self.assertNotEqual(before, self.dock_hash(), path)
            (self.root / path).write_text(original)
            self.assertEqual(before, self.dock_hash(), path)

    def test_only_the_packages_own_packages_json_entry_is_an_input(self):
        packages = self.forge / "config/packages.json"
        packages.write_text(json.dumps(
            {"custom_packages": ["dock"], "custom_tier1": ["dock"], "upstream_cli": ["bat"]}
        ))
        before = self.dock_hash()
        packages.write_text(json.dumps(
            {"custom_packages": ["dock"], "custom_tier1": ["dock"], "upstream_cli": ["bat", "fd"]}
        ))
        self.assertEqual(before, self.dock_hash())
        packages.write_text(json.dumps(
            {"custom_packages": ["dock"], "custom_tier2": ["dock"], "upstream_cli": ["bat"]}
        ))
        self.assertNotEqual(before, self.dock_hash())

    def test_the_hash_does_not_depend_on_the_locale(self):
        spec = self.forge / "specs/athanor-dock"
        (spec / "a-b.txt").write_text("1\n")
        (spec / "aa.txt").write_text("2\n")
        self.assertEqual(self.dock_hash("C"), self.dock_hash("en_US.UTF-8"))

    def test_a_failing_dependency_scan_fails_the_hash(self):
        crate = self.root / "forge/specs/athanor-dock/dock-1.0.0"
        (crate / "Cargo.toml").write_text(
            '[package]\nname = "dock"\nversion = "1.0.0"\n\n[dependencies]\n'
            'gone = { path = "../../../../system/gone" }\n'
        )
        result = subprocess.run(
            ["bash", SCRIPT, "--package", "dock", "--hash-only"],
            cwd=self.forge, capture_output=True, text=True,
        )
        self.assertNotEqual(0, result.returncode)

    def test_the_hash_does_not_depend_on_the_checkout_location(self):
        before = self.dock_hash()
        moved = self.root.parent / (self.root.name + "-moved")
        self.addCleanup(lambda: shutil.rmtree(moved, ignore_errors=True))
        shutil.copytree(self.root, moved)
        self.forge = moved / "forge"
        self.assertEqual(before, self.dock_hash())


class PackageCacheHitTest(unittest.TestCase):
    """A package's hash tag is a cache hit only when a trusted run of call-dag-compile.yml
    signed the digest it names (issue #1176); otherwise the package is rebuilt."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.forge = root / "forge"
        (self.forge / "config").mkdir(parents=True)
        (self.forge / "specs/athanor-dock").mkdir(parents=True)
        (self.forge / "specs/athanor-dock/dock.spec").write_text("Name: athanor-dock\n")
        (root / "bin").mkdir()
        stub_skopeo(root / "bin")
        stub_cosign(root / "bin")
        self.root = root

    def check(self, signer):
        if signer:
            (self.root / "signer").write_text(signer)
        result = subprocess.run(
            ["bash", SCRIPT, "--package", "dock", "--registry", "ghcr.io", "--owner", "owner",
             "--image-name", "athanor-forge-dock"],
            cwd=self.forge, capture_output=True, text=True,
            env=signed_registry_env(self.root / "bin", self.root),
        )
        return result

    def test_a_signed_image_is_a_hit(self):
        result = self.check(TRUSTED)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("CACHE_HIT=true", result.stdout.splitlines())

    def test_an_unsigned_or_foreign_image_is_a_miss_and_an_error(self):
        for signer in (None, WORKFLOWS + "release.yml@refs/heads/iso-v0"):
            with self.subTest(signer=signer):
                result = self.check(signer)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn("CACHE_HIT=false", result.stdout.splitlines())
                self.assertIn("no trusted run of call-dag-compile.yml signed", result.stderr)

    def test_a_signed_image_labelled_with_another_hash_or_none_is_a_miss_and_an_error(self):
        for label in ("f" * 64, "none"):
            with self.subTest(label=label):
                (self.root / "label").write_text(label)
                result = self.check(TRUSTED)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn("CACHE_HIT=false", result.stdout.splitlines())
                self.assertIn("not the tag's hash", result.stderr)

    def test_a_verification_that_cannot_be_answered_fails_the_check(self):
        (self.root / "cosign_error").write_text("")
        result = self.check(TRUSTED)
        self.assertNotEqual(0, result.returncode)
        self.assertNotIn("CACHE_HIT=", result.stdout)


class BuilderCacheHitTest(BuilderHashTest):
    """The builder's tag is a hit only when a trusted run of call-build-builder.yml signed the
    digest it names, and the hit names that digest (issue #1205); otherwise it is rebuilt."""

    def setUp(self):
        super().setUp()
        (self.root / "bin").mkdir()
        stub_skopeo(self.root / "bin")
        stub_cosign(self.root / "bin")

    def check(self, signer):
        if signer:
            (self.root / "signer").write_text(signer)
        return subprocess.run(
            ["bash", SCRIPT, "--package", "builder", "--registry", "ghcr.io", "--owner", "owner",
             "--image-name", "athanor-builder"],
            cwd=self.forge, capture_output=True, text=True,
            env=signed_registry_env(self.root / "bin", self.root),
        )

    def test_a_signed_builder_is_a_hit_by_its_digest(self):
        result = self.check(BUILDER_TRUSTED)
        self.assertEqual(0, result.returncode, result.stderr)
        lines = result.stdout.splitlines()
        self.assertIn("CACHE_HIT=true", lines)
        self.assertIn(f"IMAGE=ghcr.io/owner/athanor-builder@{DIGEST}", lines)
        self.assertIn("call-build-builder\\.yml", (self.root / "cosign.log").read_text())

    def test_an_unsigned_or_foreign_builder_is_a_miss_and_an_error(self):
        for signer in (None, TRUSTED, WORKFLOWS + "call-build-builder.yml@refs/heads/feature"):
            with self.subTest(signer=signer):
                (self.root / "signer").unlink(missing_ok=True)
                result = self.check(signer)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn("CACHE_HIT=false", result.stdout.splitlines())
                self.assertNotIn("IMAGE=", result.stdout)
                self.assertIn("no trusted run of call-build-builder.yml signed", result.stderr)

    def test_a_signed_builder_labelled_with_another_hash_is_a_miss(self):
        # A signature binds a digest, not a tag: a trusted builder of other inputs copied with
        # its signature under this hash is not this checkout's builder.
        for label in ("none", "f" * 64):
            with self.subTest(label=label):
                (self.root / "label").write_text(label)
                result = self.check(BUILDER_TRUSTED)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn("CACHE_HIT=false", result.stdout.splitlines())
                self.assertNotIn("IMAGE=", result.stdout)
                self.assertIn("not the tag's hash", result.stderr)

    def test_an_absent_builder_is_a_miss(self):
        (self.root / "status").write_text("1")
        (self.root / "answer").write_text("manifest unknown")
        result = self.check(BUILDER_TRUSTED)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("CACHE_HIT=false", result.stdout.splitlines())

    def test_a_verification_that_cannot_be_answered_fails_the_check(self):
        (self.root / "cosign_error").write_text("")
        result = self.check(BUILDER_TRUSTED)
        self.assertNotEqual(0, result.returncode)
        self.assertNotIn("CACHE_HIT=", result.stdout)


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS",
             "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY_OWNER")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
