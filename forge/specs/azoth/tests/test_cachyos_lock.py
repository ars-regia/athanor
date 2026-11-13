"""Unit test of the CachyOS locks (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import hashlib
import os
import pathlib
import re
import shutil
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

    def test_only_a_newline_ends_a_line(self):
        # splitlines() would also end a line at U+2028, U+0085 or \x1c: a path holding one
        # could carry a second, forged entry.
        for sep in ("\u2028", "\u0085", "\x1c", "\r"):
            entries = cachyos_lock.protected_entries(f"100644 aa  mm/a{sep}100644 bb  mm/slub.c\n")
            self.assertEqual(entries, {f"mm/a{sep}100644 bb  mm/slub.c": "100644 aa"})

    def test_a_path_listed_twice_is_an_error(self):
        with self.assertRaises(ValueError):
            cachyos_lock.protected_entries("M  mm/slub.c\nA  mm/slub.c\n")

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
        self.assertTrue(set(files.values()) <= {"M", "A", "D", "T"}, set(files.values()))
        self.assertTrue(protected.keys() <= files.keys(), protected.keys() - files.keys())

    def test_the_protected_paths_are_those_of_the_specification(self):
        build = (AZOTH / "build.sh").read_text()
        block = build[build.index("PROTECTED=(") + len("PROTECTED=(") :]
        paths = [p.strip("'") for p in block[: block.index(")")].split()]
        doc = (AZOTH.parents[2] / "docs/architecture/doc_kernel_build.md").read_text()
        bullet = doc[doc.index("- `cachyos-protected.sha256`:") :]
        bullet = bullet[: bullet.index("\n\n")]
        named = {
            token.rstrip("/")
            for token in re.findall(r"`([^`]+)`", bullet)
            if "/" in token or token == "Makefile"
        }
        self.assertEqual(named, set(paths))

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


def build_sh_part(pattern):
    """The text of build.sh that pattern, anchored on a line start, matches."""
    match = re.search(pattern, (AZOTH / "build.sh").read_text(), re.M | re.S)
    if not match:
        raise AssertionError(f"build.sh has no {pattern!r}")
    # A lazy match that ran past its own end into the next function would test other text.
    if len(re.findall(r"^\w+\(\) \{", match.group(0), re.M)) > 1:
        raise AssertionError(f"{pattern!r} matches more than one function of build.sh")
    return match.group(0)


@unittest.skipUnless(shutil.which("git") and shutil.which("bash"), "needs git and bash")
class LockFunctions(unittest.TestCase):
    """The functions of build.sh that write the locks, run on a vanilla and a CachyOS tree."""

    def run_locks(self, work):
        parts = [
            build_sh_part(r"^die\(\) \{.*?\}$"),
            build_sh_part(r"^g\(\) \{.*?\}$"),
            build_sh_part(r"^PROTECTED=\(.*?\)$"),
            build_sh_part(r"^cachyos_tree\(\) \{.*?^\}$"),
            build_sh_part(r"^plain_path\(\) \{.*?^\}$"),
            build_sh_part(r"^cachyos_files\(\) \{.*?^\}$"),
            build_sh_part(r"^protected_files\(\) \{.*?^\}$"),
        ]
        script = "\n".join([
            "set -euo pipefail", "WORK=$1", *parts,
            'VANILLA=$(g rev-parse HEAD)', "cachyos_tree",
            'echo "-- files"; cachyos_files "$CACHYOS"',
            'echo "-- protected"; protected_files "$CACHYOS"',
        ])
        return subprocess.run(["bash", "-c", script, "locks", str(work)],
                              capture_output=True, text=True, check=False)

    def trees(self, tmp):
        a, b = tmp / "a", tmp / "b"
        for rel, text in {"mm/slub.c": "v\n", "mm/gone.c": "g\n", "fs/x.c": "x\n",
                          "Makefile": "m\n", ".gitignore": "*.o\n"}.items():
            (a / rel).parent.mkdir(parents=True, exist_ok=True)
            (a / rel).write_text(text)
        os.symlink("target-v", a / "mm/link")
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        for args in (["init", "-q"], ["add", "-Af", "."], ["commit", "-qm", "vanilla"]):
            subprocess.run(["git", "-C", str(a), *args], check=True, env=env)
        shutil.copytree(a, b, symlinks=True, ignore=shutil.ignore_patterns(".git"))
        (b / "mm/slub.c").write_text("cachyos\n")
        (b / "mm/gone.c").unlink()
        (b / "mm/new.o").write_text("ignored but added\n")
        (b / "fs/x.c").write_text("outside\n")
        (b / "Makefile").chmod(0o755)
        (b / "mm/link").unlink()
        os.symlink("target-c", b / "mm/link")
        return b

    def test_both_lists_hold_every_change(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.trees(pathlib.Path(tmp))
            r = self.run_locks(tmp)
            self.assertEqual(r.returncode, 0, r.stderr)
            files, protected = r.stdout.split("-- protected\n")
            self.assertEqual(files, "-- files\nM  Makefile\nM  fs/x.c\nD  mm/gone.c\n"
                                    "M  mm/link\nA  mm/new.o\nM  mm/slub.c\n")
            sha = lambda text: hashlib.sha256(text.encode()).hexdigest()
            self.assertEqual(protected.splitlines(), [
                f"100755 {sha('m' + chr(10))}  Makefile",
                "000000 deleted  mm/gone.c",
                f"120000 {sha('target-c')}  mm/link",
                f"100644 {sha('ignored but added' + chr(10))}  mm/new.o",
                f"100644 {sha('cachyos' + chr(10))}  mm/slub.c",
            ])

    def test_a_path_with_a_newline_stops_the_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            b = self.trees(pathlib.Path(tmp))
            (b / "mm/a\n100644 x  mm").write_text("forged\n")
            r = self.run_locks(tmp)
            self.assertEqual(r.returncode, 1, r.stdout)
            self.assertIn("build.sh: CachyOS changes a path that is not printable ASCII", r.stderr)

    def test_a_path_not_in_ascii_stops_the_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            b = self.trees(pathlib.Path(tmp))
            (b / "mm/caf\u00e9.c").write_text("utf-8 name\n")
            r = self.run_locks(tmp)
            self.assertEqual(r.returncode, 1, r.stdout)
            self.assertIn("build.sh: CachyOS changes a path that is not printable ASCII", r.stderr)

    def test_a_failing_add_stops_the_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            b = self.trees(pathlib.Path(tmp))
            shutil.rmtree(b)
            r = self.run_locks(tmp)
            self.assertNotEqual(r.returncode, 0, r.stdout)
            self.assertNotIn("-- files", r.stdout)

if __name__ == "__main__":
    unittest.main()
