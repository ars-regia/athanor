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


MANIFEST = '[workspace]\nresolver = "2"\nmembers = ["system/a"]\n'
LOCK = (
    "version = 4\n\n"
    '[[package]]\nname = "a"\nversion = "0.1.0"\ndependencies = ["libc"]\n\n'
    '[[package]]\nname = "libc"\nversion = "0.2.0"\n'
    'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
)
NEW_MEMBER = '\n[[package]]\nname = "b"\nversion = "0.1.0"\ndependencies = ["libc"]\n'


class OnlyAddsMembersTest(unittest.TestCase):
    def test_a_new_member_only(self):
        added = MANIFEST.replace('["system/a"]', '["system/a", "system/b"]')
        self.assertTrue(changes.only_adds_members("Cargo.toml", MANIFEST, added))
        self.assertTrue(changes.only_adds_members("Cargo.lock", LOCK, LOCK + NEW_MEMBER))

    def test_anything_else_in_the_manifest(self):
        for new in (
            MANIFEST.replace('["system/a"]', "[]"),
            MANIFEST.replace('"2"', '"3"'),
            MANIFEST + '[workspace.dependencies]\nlibc = "0.2"\n',
            MANIFEST + "[profile.release]\nlto = true\n",
            MANIFEST.replace('["system/a"]', '["system/a", "system/*"]'),
            '[workspace]\nresolver = "2"\nmembers = ["system/a"]\n[profile.release]\ndebug = 1\n',
            'workspace = "x"\n',
            "not toml [",
        ):
            with self.subTest(new=new):
                self.assertFalse(changes.only_adds_members("Cargo.toml", MANIFEST, new))

    def test_anything_else_in_the_lock(self):
        external = '\n[[package]]\nname = "x"\nversion = "1.0.0"\nsource = "git+https://x"\n'
        for new in (
            LOCK + external,
            LOCK.replace('"0.2.0"', '"0.2.1"'),
            LOCK.replace('dependencies = ["libc"]', "dependencies = []"),
            LOCK.replace("version = 4", "version = 3"),
            "",
        ):
            with self.subTest(new=new):
                self.assertFalse(changes.only_adds_members("Cargo.lock", LOCK, new))

    def test_a_changed_type_is_a_change(self):
        # Python's == takes true for 1 and 1 for 1.0; cargo does not.
        old = MANIFEST + "[profile.release]\ndebug = true\n"
        self.assertFalse(changes.only_adds_members("Cargo.toml", old, old.replace("true", "1")))
        lock = LOCK + '\n[[package]]\nname = "c"\nversion = "0.1.0"\nn = 1\n'
        self.assertFalse(changes.only_adds_members("Cargo.lock", lock, lock.replace("n = 1", "n = 1.0")))

    def test_a_members_only_file_selects_the_audit_but_not_the_shell(self):
        self.assertEqual(
            changes.classify(["Cargo.toml", "Cargo.lock"], ["Cargo.toml", "Cargo.lock"]),
            {**NONE, "rust_audit": True},
        )
        self.assertTrue(changes.classify(["Cargo.toml", "Cargo.lock"], ["Cargo.toml"])["shell"])


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

    def test_every_area_is_a_build_or_the_audit(self):
        # A new area must be placed: a build, which merge_group never selects, or not.
        self.assertEqual(set(changes.AREAS), {*changes.BUILDS, "rust_audit"})

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

    def test_a_new_workspace_member_does_not_select_the_shell(self):
        (self.root / "Cargo.toml").write_text(MANIFEST)
        (self.root / "Cargo.lock").write_text(LOCK)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "workspace")
        (self.root / "Cargo.toml").write_text(MANIFEST.replace('"]', '", "system/b"]'))
        (self.root / "Cargo.lock").write_text(LOCK + NEW_MEMBER)
        (self.root / "system/b/src").mkdir(parents=True)
        (self.root / "system/b/src/lib.rs").write_text("\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "member")
        out = self.root / "out" / "changes.json"
        changes.main("HEAD^1", str(out), cwd=self.root)
        written = json.loads(out.read_text())
        self.assertFalse(written["shell"])
        self.assertTrue(written["rust_audit"])
        # The same member with a new external package selects the shell.
        (self.root / "Cargo.lock").write_text(
            LOCK + NEW_MEMBER + '\n[[package]]\nname = "x"\nversion = "1.0.0"\nsource = "registry+y"\n'
        )
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "external")
        changes.main("HEAD~2", str(out), cwd=self.root)
        self.assertTrue(json.loads(out.read_text())["shell"])

    def test_an_added_root_manifest_selects_the_shell(self):
        (self.root / "Cargo.toml").write_text(MANIFEST)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "workspace")
        out = self.root / "out" / "changes.json"
        changes.main("HEAD^1", str(out), cwd=self.root)
        self.assertTrue(json.loads(out.read_text())["shell"])

    def test_a_deleted_root_lock_selects_the_shell(self):
        (self.root / "Cargo.lock").write_text(LOCK)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "lock")
        self.git("rm", "-q", "Cargo.lock")
        self.git("commit", "-q", "-m", "drop")
        out = self.root / "out" / "changes.json"
        changes.main("HEAD^1", str(out), cwd=self.root)
        self.assertTrue(json.loads(out.read_text())["shell"])

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
