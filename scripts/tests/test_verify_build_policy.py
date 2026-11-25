"""Unit tests of the build-policy check in scripts/verify.py (#1350)
(python3 -B -m unittest discover -s scripts/tests -v)."""

import copy
import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

LINTS = copy.deepcopy(verify.BUILD_POLICY_LINTS)
FLAGS = ["-C", "force-frame-pointers=yes", "-C", "link-arg=-fuse-ld=mold"]
IN_PLACE = "Name: athanor-bar\n%build\n%set_build_flags\ncargo build --release --locked -p athanor-bar\n"
UPSTREAM = 'Name: cosmic-comp\nSource0: cosmic-comp.tar.gz\n%build\nexport RUSTFLAGS="-C opt-level=3"\n'


def tree():
    """A repository with two members, one reached only by a path dependency, and a vendored
    crate reached only by [patch]."""
    return {
        "Cargo.toml": {
            "workspace": {
                "members": ["system/a"],
                "exclude": ["fuzz"],
                "lints": copy.deepcopy(LINTS),
                "dependencies": {"b": {"path": "system/b"}},
            },
            "patch": {"crates-io": {"v": {"path": "system/a/vendor/v"}}},
        },
        "system/a/Cargo.toml": {
            "lints": {"workspace": True},
            "dependencies": {"b": {"workspace": True}, "f": {"path": "../../fuzz/f"}},
        },
        "system/b/Cargo.toml": {"lints": {"workspace": True}},
        "system/a/vendor/v/Cargo.toml": {},
        "fuzz/f/Cargo.toml": {},
        verify.GALLERY_MANIFEST: {"lints": copy.deepcopy(LINTS)},
        ".cargo/config.toml": {
            "build": {"rustflags": list(FLAGS)},
            "target": {'cfg(target_os = "linux")': {"rustflags": list(FLAGS)}},
        },
    }


def problems(files, texts=None):
    texts = (
        {
            "forge/config/rpmmacros": "%_smp_mflags -j4\n",
            "forge/specs/athanor-bar/athanor-bar.spec": IN_PLACE,
            "forge/specs/cosmic-comp/cosmic-comp.spec": UPSTREAM,
        }
        if texts is None
        else texts
    )
    return verify.build_policy_problems(files.get, texts)


class BuildPolicyTest(unittest.TestCase):
    def test_the_policy_passes(self):
        self.assertEqual(problems(tree()), [])

    def test_members_include_path_dependencies_but_not_patches_or_excluded_crates(self):
        self.assertEqual(
            verify.workspace_members(tree().get),
            ["system/a/Cargo.toml", "system/b/Cargo.toml"],
        )

    def test_a_member_reached_by_path_without_the_workspace_lints_fails(self):
        files = tree()
        del files["system/b/Cargo.toml"]["lints"]
        found = problems(files)
        self.assertEqual(len(found), 1)
        self.assertIn(
            "system/b/Cargo.toml: a workspace member without `lints.workspace = true`",
            found[0],
        )

    def test_a_missing_policy_lint_fails(self):
        files = tree()
        files["Cargo.toml"]["workspace"]["lints"]["rust"]["unsafe_code"] = "warn"
        found = problems(files)
        self.assertTrue(
            any('does not set unsafe_code = "deny"' in f for f in found), found
        )

    def test_a_gallery_without_the_roots_lints_fails(self):
        files = tree()
        del files[verify.GALLERY_MANIFEST]["lints"]["clippy"]
        found = problems(files)
        self.assertEqual(len(found), 1)
        self.assertIn(verify.GALLERY_MANIFEST, found[0])

    def test_rustflags_tables_that_differ_fail(self):
        files = tree()
        files[".cargo/config.toml"]["target"]['cfg(target_os = "linux")'][
            "rustflags"
        ] = FLAGS[:2]
        found = problems(files)
        self.assertEqual(len(found), 1)
        self.assertIn("differ from [build]'s", found[0])

    def test_a_lint_flag_in_rustflags_fails(self):
        files = tree()
        for table in (
            files[".cargo/config.toml"]["build"],
            files[".cargo/config.toml"]["target"]['cfg(target_os = "linux")'],
        ):
            table["rustflags"] = FLAGS + ["-D", "unsafe_code"]
        found = problems(files)
        self.assertEqual(len(found), 2)
        self.assertTrue(all("sets the lint flag -D" in f for f in found), found)

    def test_rpmmacros_exporting_rustflags_fails(self):
        found = problems(
            tree(),
            {
                "forge/config/rpmmacros": '%__spec_build_pre %{___build_pre}\\\n  export RUSTFLAGS="%{rustflags}"\n'
            },
        )
        self.assertEqual(len(found), 1)
        self.assertIn("forge/config/rpmmacros:2: sets RUSTFLAGS", found[0])

    def test_an_in_place_spec_setting_a_profile_fails_and_a_comment_does_not(self):
        text = "# RUSTFLAGS would replace .cargo/config.toml\n" + IN_PLACE.replace(
            "cargo build",
            "CARGO_PROFILE_RELEASE_LTO=thin CARGO_ENCODED_RUSTFLAGS= cargo build",
        )
        found = problems(tree(), {"forge/specs/athanor-bar/athanor-bar.spec": text})
        self.assertEqual(
            [f.split(": sets ")[1].split(",")[0] for f in found],
            ["CARGO_PROFILE_RELEASE_LTO", "CARGO_ENCODED_RUSTFLAGS"],
        )

    def test_an_upstream_spec_states_its_own_flags(self):
        self.assertEqual(
            problems(tree(), {"forge/specs/cosmic-comp/cosmic-comp.spec": UPSTREAM}), []
        )


if __name__ == "__main__":
    unittest.main()
