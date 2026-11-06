"""scripts/ci/changes.py: the areas a change touches, as pr.yml selects its jobs."""

import importlib.util
import json
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "changes.py"
spec = importlib.util.spec_from_file_location("changes", SCRIPT)
changes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changes)

NONE = {"kernel": False, "specs": False, "shell": False, "rust_audit": False, "docs_only": False}
ALL = {area: True for area in NONE}


class ClassifyTest(unittest.TestCase):
    def test_documentation_only_selects_nothing(self):
        self.assertEqual(
            changes.classify(["docs/architecture/doc_ci.md", "README.md"]),
            {**NONE, "docs_only": True},
        )

    def test_kernel_inputs_select_the_kernel(self):
        for path in (
            "forge/specs/azoth/azoth.spec",
            "forge/specs/azoth/KERNEL.md",
            ".github/actions/kvm/action.yml",
            ".github/workflows/call-kernel.yml",
            ".github/workflows/nvidia-build.yml",
        ):
            with self.subTest(path=path):
                self.assertEqual(
                    changes.classify([path]),
                    {**NONE, "kernel": True},
                )

    def test_spec_inputs_select_the_specs_but_not_the_kernel_spec(self):
        for path in (
            "forge/specs/athanor-update/athanor-update.spec",
            "forge/config/rpmmacros",
            "forge/scripts/select_check_specs.py",
            "forge/scripts/check_idempotency.sh",
            "forge/scripts/registry_probe.sh",
            ".github/workflows/spec-build-check.yml",
        ):
            with self.subTest(path=path):
                self.assertEqual(
                    changes.classify([path]),
                    {**NONE, "specs": True},
                )
        # The builder and the retry wrapper serve the Rust audit as well.
        for path in ("forge/builder/Containerfile", "flake.lock", "forge/scripts/retry.sh"):
            with self.subTest(path=path):
                self.assertEqual(
                    changes.classify([path]),
                    {**NONE, "specs": True, "rust_audit": True},
                )
        self.assertFalse(changes.classify(["forge/specs/azoth/azoth.spec"])["specs"])

    def test_shell_inputs_select_the_visual_tests(self):
        for path in (
            "system/athanor-style/calmo/generate.py",
            "forge/test/shell/rig.sh",
            ".github/workflows/shell-surfaces.yml",
        ):
            with self.subTest(path=path):
                self.assertEqual(
                    changes.classify([path]),
                    {**NONE, "shell": True},
                )
        for path in (
            "system/athanor-sulphur-client/src/lib.rs",
            "system/athanor-sal/src/lib.rs",
            "Cargo.lock",
            "Cargo.toml",
            ".cargo/config.toml",
        ):
            with self.subTest(path=path):
                self.assertEqual(
                    changes.classify([path]),
                    {**NONE, "shell": True, "rust_audit": True},
                )

    def test_the_own_compositor_selects_the_visual_tests_and_its_package(self):
        self.assertEqual(
            changes.classify(["forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/main.rs"]),
            {**NONE, "specs": True, "shell": True, "rust_audit": True},
        )

    def test_a_crate_the_rig_does_not_draw_selects_the_audit_only(self):
        # Its unit tests run in `just check` on every change; no build area is needed for them.
        for path in ("system/athanor-bus-api/src/lib.rs", "system/athanor-portal/src/lib.rs"):
            with self.subTest(path=path):
                self.assertEqual(changes.classify([path]), {**NONE, "rust_audit": True})

    def test_rust_audit_inputs_select_the_audit(self):
        for path in (
            "fuzz/Cargo.toml",
            "fuzz/Cargo.lock",
            "system/athanor-sal/gallery/Cargo.lock",
            "fuzz/fuzz_targets/parse.rs",
            "rust-toolchain",
            "fuzz/rust-toolchain.toml",
            "clippy.toml",
            "system/athanor-sal/.clippy.toml",
            "deny.toml",
            "flake.nix",
            ".github/workflows/rust-security-audit.yml",
        ):
            with self.subTest(path=path):
                self.assertTrue(changes.classify([path])["rust_audit"])
        # A name anywhere matches the whole name, an extension the end of the name only.
        for path in ("docs/MyCargo.toml", "system/x/Cargo.toml.orig", "scripts/notes.rst", "x/myclippy.toml"):
            with self.subTest(path=path):
                self.assertFalse(changes.classify([path])["rust_audit"])

    def test_a_change_selects_every_area_it_feeds(self):
        self.assertEqual(
            changes.classify(["system/kernel-artifacts.sh", "forge/specs/athanor-bar/athanor-bar.spec"]),
            {**NONE, "kernel": True, "specs": True, "shell": True},
        )

    def test_a_prefix_is_a_directory_not_a_name_prefix(self):
        self.assertFalse(changes.classify(["forge/specs/azoth-notes.txt"])["kernel"])
        self.assertFalse(changes.classify(["forge/buildernotes"])["specs"])

    def test_the_selection_logic_itself_selects_every_area(self):
        for path in (".github/workflows/pr.yml", "scripts/ci/changes.py"):
            with self.subTest(path=path):
                self.assertEqual(
                    changes.classify([path]),
                    {**ALL, "docs_only": False},
                )

    def test_other_code_selects_no_build_and_is_not_documentation(self):
        self.assertEqual(
            changes.classify(["scripts/verify.py", "docs/operations/ci.md"]),
            NONE,
        )

    def test_no_change_is_not_documentation_only(self):
        self.assertFalse(changes.classify([])["docs_only"])


class MainTest(unittest.TestCase):
    """The command against a real repository: a rename counts on both sides."""

    def git(self, *args):
        subprocess.run(
            ["git", "-C", str(self.root), *args],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "ci@example.invalid")
        self.git("config", "user.name", "ci")
        (self.root / "forge/specs/azoth").mkdir(parents=True)
        (self.root / "forge/specs/azoth/notes.txt").write_text("kernel\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")

    def tearDown(self):
        self.tmp.cleanup()

    def test_rename_out_of_an_area_selects_it_and_the_file_is_written(self):
        (self.root / "docs").mkdir()
        self.git("mv", "forge/specs/azoth/notes.txt", "docs/notes.txt")
        self.git("commit", "-q", "-m", "move")
        out = self.root / "out" / "changes.json"
        changes.main("HEAD^1", str(out), cwd=self.root)
        written = json.loads(out.read_text())
        self.assertTrue(written["kernel"])
        self.assertFalse(written["docs_only"])
        self.assertRegex(written["base"], r"^[0-9a-f]{40}$")

    def test_a_merge_group_selects_no_build_but_still_the_audit(self):
        (self.root / "forge/specs/azoth/notes.txt").write_text("kernel 2\n")
        (self.root / "system/athanor-sal").mkdir(parents=True)
        (self.root / "system/athanor-sal/lib.rs").write_text("fn main() {}\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "change")
        out = self.root / "out" / "changes.json"
        changes.main("HEAD^1", str(out), cwd=self.root)
        on_pr = json.loads(out.read_text())
        self.assertTrue(on_pr["kernel"] and on_pr["shell"] and on_pr["rust_audit"])
        changes.main("HEAD^1", str(out), cwd=self.root, event="merge_group")
        written = json.loads(out.read_text())
        for area in changes.BUILDS:
            self.assertFalse(written[area], area)
        self.assertTrue(written["rust_audit"])
        self.assertFalse(written["docs_only"])

    def test_a_path_git_would_quote_still_selects_its_area(self):
        # Without -z, git prints such a path quoted and escaped ("forge/specs/azoth/p\303\240...").
        for name in ("p\u00e0tch.patch", "tab\there.patch", 'quote".patch'):
            with self.subTest(name=name):
                self.git("reset", "-q", "--hard", "HEAD")
                (self.root / "forge/specs/azoth" / name).write_text("patch\n")
                self.git("add", "-A")
                self.git("commit", "-q", "-m", "add")
                out = self.root / "out" / "changes.json"
                changes.main("HEAD^1", str(out), cwd=self.root)
                self.assertTrue(json.loads(out.read_text())["kernel"])


if __name__ == "__main__":
    unittest.main()
