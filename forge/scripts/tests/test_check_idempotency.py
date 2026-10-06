"""UD42: the builder hash covers the builder's inputs only
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
