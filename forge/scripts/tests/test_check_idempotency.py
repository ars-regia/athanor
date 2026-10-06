"""UD42: the builder hash covers the builder's inputs only
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import pathlib
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


if __name__ == "__main__":
    unittest.main()
