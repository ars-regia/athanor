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
    + "a" * 64
    + "  mm/slub.c\n"
    + "deleted  security/x.c\n"
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
            "protected", PROTECTED.replace("deleted  security/x.c\n", ""), PROTECTED
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("new security/x.c: deleted", result.stdout)

    def test_a_malformed_line_is_an_error(self):
        with self.assertRaises(ValueError):
            cachyos_lock.protected_entries("mm/slub.c\n")


if __name__ == "__main__":
    unittest.main()
