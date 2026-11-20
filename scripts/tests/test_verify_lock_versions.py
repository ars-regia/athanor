"""Unit tests of the lock-versions check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
SMITHAY = "git+https://github.com/Smithay/smithay?rev={0}#{0}"


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

    def test_a_git_crate_at_another_revision_fails(self):
        root = [("smithay", "0.7.0", SMITHAY.format("aaa"))]
        self.assertEqual(
            problems(root, [("smithay", "0.7.0", SMITHAY.format("aaa"))]), []
        )
        found = problems(root, [("smithay", "0.7.0", SMITHAY.format("bbb"))])
        self.assertEqual(len(found), 1)
        self.assertNotIn("--precise", found[0])


class SemverCompatibleTest(unittest.TestCase):
    def test_the_first_non_zero_component_decides(self):
        self.assertTrue(verify.semver_compatible("1.2.3", "1.9.0"))
        self.assertFalse(verify.semver_compatible("1.2.3", "2.0.0"))
        self.assertTrue(verify.semver_compatible("0.2.1", "0.2.9"))
        self.assertFalse(verify.semver_compatible("0.0.3", "0.0.4"))

    def test_pre_release_and_build_metadata_are_set_aside(self):
        self.assertTrue(verify.semver_compatible("0.0.3-rc.1", "0.0.3"))
        self.assertTrue(verify.semver_compatible("0.0.3+build.5", "0.0.3"))


class SubLocksTest(unittest.TestCase):
    WORKSPACE = '[package]\nname = "x"\n\n[workspace]\n\n[dependencies]\n'

    def subs(self, dependency):
        return verify.sub_locks(
            ["Cargo.lock", "a/Cargo.lock", "a/vendor/v/Cargo.lock"],
            {
                "Cargo.toml": self.WORKSPACE + dependency,
                "a/Cargo.toml": self.WORKSPACE + dependency,
                "a/vendor/v/Cargo.toml": self.WORKSPACE + dependency,
            },
        )

    def test_a_workspace_that_builds_a_crate_outside_its_directory_is_compared(self):
        self.assertEqual(self.subs('b = { path = "../b" }\n'), ["a/Cargo.lock"])

    def test_a_patch_outside_its_directory_counts(self):
        self.assertEqual(
            self.subs('\n[patch.crates-io]\nc = { path = "../vendor/c" }\n'),
            ["a/Cargo.lock"],
        )

    def test_a_workspace_of_crates_inside_its_directory_is_its_own(self):
        self.assertEqual(self.subs('b = { path = "b" }\n'), [])

    def test_a_crate_without_a_workspace_table_is_not_a_root(self):
        member = '[package]\nname = "x"\n\n[dependencies]\nb = { path = "../b" }\n'
        self.assertEqual(
            verify.sub_locks(["a/Cargo.lock"], {"a/Cargo.toml": member}), []
        )


class CheckTest(unittest.TestCase):
    def test_the_repository_compares_the_fuzz_and_gallery_locks_and_passes(self):
        result = verify.check_lock_versions()
        self.assertTrue(result.ok, result.problems)
        compared = []
        real = verify.lock_version_problems

        def spy(root, sub, path):
            compared.append(path)
            return real(root, sub, path)

        with mock.patch.object(verify, "lock_version_problems", spy):
            verify.check_lock_versions()
        self.assertEqual(
            compared, ["fuzz/Cargo.lock", "system/athanor-sal/gallery/Cargo.lock"]
        )

    def test_a_drifted_lock_fails_the_check(self):
        with mock.patch.object(
            verify, "lock_version_problems", lambda root, sub, path: [f"{path}: drift"]
        ):
            result = verify.check_lock_versions()
        self.assertEqual(
            result.problems,
            ["fuzz/Cargo.lock: drift", "system/athanor-sal/gallery/Cargo.lock: drift"],
        )

    def test_no_workspace_to_compare_fails_the_check(self):
        with mock.patch.object(verify, "sub_locks", lambda locks, manifests: []):
            self.assertFalse(verify.check_lock_versions().ok)

    def test_an_unreadable_lock_fails_the_check(self):
        with mock.patch.object(verify, "read", side_effect=OSError("gone")):
            result = verify.check_lock_versions()
        self.assertFalse(result.ok)
        self.assertIn("gone", result.problems[-1])


if __name__ == "__main__":
    unittest.main()
