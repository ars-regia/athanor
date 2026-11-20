"""Unit tests of the lock-versions check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def lock(*packages):
    """The text of a Cargo.lock holding (name, version, source) packages."""
    out = ["version = 4\n"]
    for name, version, source in packages:
        out.append(f'[[package]]\nname = "{name}"\nversion = "{version}"\n')
        if source:
            out.append(f'source = "{source}"\n')
    return "\n".join(out)


def problems(root, sub):
    return verify.lock_version_problems(lock(*root), lock(*sub), "fuzz/Cargo.lock")


class LockVersionProblemsTest(unittest.TestCase):
    def test_a_version_the_root_does_not_hold_fails_with_its_command(self):
        found = problems([("cc", "1.4.4", REGISTRY)], [("cc", "1.6.0", REGISTRY)])
        self.assertEqual(len(found), 1)
        self.assertIn(
            "cargo update --manifest-path fuzz/Cargo.toml -p cc@1.6.0 --precise 1.4.4",
            found[0],
        )

    def test_the_command_names_the_compatible_root_version(self):
        root = [("thiserror", "1.0.69", REGISTRY), ("thiserror", "2.0.21", REGISTRY)]
        found = problems(root, [("thiserror", "2.0.20", REGISTRY)])
        self.assertEqual(len(found), 1)
        self.assertIn("-p thiserror@2.0.20 --precise 2.0.21", found[0])

    def test_a_zero_major_moves_only_within_its_minor(self):
        found = problems([("libc", "0.3.0", REGISTRY)], [("libc", "0.2.189", REGISTRY)])
        self.assertEqual(len(found), 1)
        self.assertNotIn("--precise", found[0])

    def test_one_of_several_root_versions_passes(self):
        root = [("thiserror", "1.0.69", REGISTRY), ("thiserror", "2.0.21", REGISTRY)]
        self.assertEqual(problems(root, [("thiserror", "1.0.69", REGISTRY)]), [])

    def test_a_crate_only_in_the_sub_lock_passes(self):
        self.assertEqual(
            problems(
                [("cc", "1.4.4", REGISTRY)], [("libfuzzer-sys", "0.4.10", REGISTRY)]
            ),
            [],
        )

    def test_a_path_crate_is_not_compared(self):
        self.assertEqual(
            problems(
                [("athanor-unit", "1.0.0", REGISTRY)], [("athanor-unit", "0.1.0", None)]
            ),
            [],
        )

    def test_a_registry_crate_the_root_patches_is_compared(self):
        found = problems(
            [("accesskit_atspi_common", "0.21.0", None)],
            [("accesskit_atspi_common", "0.21.1", REGISTRY)],
        )
        self.assertEqual(len(found), 1)
        self.assertIn("--precise 0.21.0", found[0])

    def test_the_listed_locks_exist(self):
        for path in verify.SUB_LOCKS:
            self.assertTrue((verify.ROOT / path).is_file(), path)


if __name__ == "__main__":
    unittest.main()
