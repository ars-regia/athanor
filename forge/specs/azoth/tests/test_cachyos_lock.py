"""Unit test of the CachyOS locks (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import pathlib
import subprocess
import sys
import tempfile
import unittest

AZOTH = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AZOTH))
import cachyos_lock  # noqa: E402

CONFIG = "# Options taken from CachyOS\nCONFIG_A=y\n# CONFIG_B is not set\nCONFIG_C=m\n"
PROTECTED = (
    "# CachyOS's files in the protected paths\n"
    + "100644 "
    + "a" * 64
    + "  mm/slub.c\n"
    + "000000 deleted  security/x.c\n"
)


def run(kind, lock, generated):
    with tempfile.TemporaryDirectory() as tmp:
        lock_path, gen_path = pathlib.Path(tmp, "lock"), pathlib.Path(tmp, "gen")
        lock_path.write_text(lock)
        gen_path.write_text(generated)
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(AZOTH / "cachyos_lock.py"),
                kind,
                str(lock_path),
                str(gen_path),
            ],
            capture_output=True,
            text=True,
            check=False,
        )


class ConfigLock(unittest.TestCase):
    def test_same_options_pass(self):
        self.assertEqual(
            run(
                "config", CONFIG, "CONFIG_C=m\nCONFIG_A=y\n# CONFIG_B is not set\n"
            ).returncode,
            0,
        )

    def test_new_changed_and_gone_options_are_named(self):
        result = run("config", CONFIG, "CONFIG_A=m\nCONFIG_C=m\nCONFIG_D=y\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("changed CONFIG_A: CONFIG_A=y -> CONFIG_A=m", result.stdout)
        self.assertIn("gone CONFIG_B: # CONFIG_B is not set", result.stdout)
        self.assertIn("new CONFIG_D: CONFIG_D=y", result.stdout)

    def test_a_line_removed_from_the_lock_fails(self):
        """Negative control: the lock loses a line, the generated derivation still has it."""
        result = run("config", CONFIG.replace("CONFIG_C=m\n", ""), CONFIG)
        self.assertEqual(result.returncode, 1)
        self.assertIn("new CONFIG_C", result.stdout)


class ProtectedLock(unittest.TestCase):
    def test_same_files_pass(self):
        self.assertEqual(run("protected", PROTECTED, PROTECTED).returncode, 0)

    def test_a_different_hash_is_named(self):
        result = run("protected", PROTECTED, PROTECTED.replace("a" * 64, "b" * 64))
        self.assertEqual(result.returncode, 1)
        self.assertIn("changed mm/slub.c", result.stdout)

    def test_a_line_removed_from_the_lock_fails(self):
        """Negative control: a protected file CachyOS changes is missing from the lock."""
        result = run(
            "protected", PROTECTED.replace("000000 deleted  security/x.c\n", ""), PROTECTED
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("new security/x.c: 000000 deleted", result.stdout)

    def test_a_different_mode_is_named(self):
        """A file made executable, or turned into a symlink, with the same content."""
        result = run("protected", PROTECTED, PROTECTED.replace("100644 ", "100755 "))
        self.assertEqual(result.returncode, 1)
        self.assertIn("changed mm/slub.c", result.stdout)

    def test_a_file_listed_with_another_status_is_named(self):
        result = run("protected", "M  mm/slub.c\n", "A  mm/slub.c\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("changed mm/slub.c: M -> A", result.stdout)

    def test_a_malformed_line_is_an_error(self):
        with self.assertRaises(ValueError):
            cachyos_lock.protected_entries("mm/slub.c\n")



class CommittedLocks(unittest.TestCase):
    """The locks in the tree parse, and what reads them agrees with build.sh."""

    def test_the_committed_locks_parse(self):
        config = cachyos_lock.config_entries((AZOTH / "derived.config").read_text())
        self.assertTrue(config)
        protected = cachyos_lock.protected_entries((AZOTH / "cachyos-protected.sha256").read_text())
        for path, value in protected.items():
            self.assertRegex(value, r"^[0-7]{6} ([0-9a-f]{64}|deleted)$", path)
        files = cachyos_lock.protected_entries((AZOTH / "cachyos-files.list").read_text())
        self.assertTrue(set(files.values()) <= {"M", "A", "D"}, set(files.values()))
        self.assertTrue(protected.keys() <= files.keys(), protected.keys() - files.keys())

    def test_the_protected_paths_are_those_of_the_specification(self):
        build = (AZOTH / "build.sh").read_text()
        block = build[build.index("PROTECTED=(") + len("PROTECTED=(") :]
        paths = [p.strip("'") for p in block[: block.index(")")].split()]
        doc = (AZOTH.parents[2] / "docs/architecture/doc_kernel_build.md").read_text()
        bullet = doc[doc.index("- `cachyos-protected.sha256`:") :]
        bullet = bullet[: bullet.index("\n\n")]
        for path in paths:
            self.assertTrue(f"`{path}/`" in bullet or f"`{path}`" in bullet, path)

    def test_a_lock_failure_is_a_refresh_for_the_bump(self):
        build = (AZOTH / "build.sh").read_text()
        self.assertIn('die() { echo "build.sh: $*" >&2; exit 1; }', build)
        for lock in ("cachyos-files.list says", "cachyos-protected.sha256 says", "differ from derived.config"):
            line = next(l for l in build.splitlines() if lock in l and "die " in l)
            self.assertIn('die "refresh needed: ', line)
        bump = (AZOTH.parents[2] / ".github/workflows/kernel-bump.yml").read_text()
        self.assertIn("grep -q '^build.sh: refresh needed: ' prep.log", bump)

    def test_the_locks_are_build_inputs(self):
        inputs = (AZOTH / "build-inputs.py").read_text()
        for name in ("cachyos_lock.py", "derived.config", "cachyos-protected.sha256", "cachyos-files.list"):
            self.assertIn(f'"{name}"', inputs)

if __name__ == "__main__":
    unittest.main()
