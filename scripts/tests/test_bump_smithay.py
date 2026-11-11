"""Test of scripts/bump/smithay.sh against a stub gh and cargo (python3 -B -m unittest discover -s scripts/tests -p test_bump_smithay.py)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "bump" / "smithay.sh"
MANIFEST = "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/Cargo.toml"
URL = "https://github.com/Smithay/smithay"
OLD = "0" * 39 + "1"
NEW = "a" * 39 + "2"

# gh answers master's head with $STUB_MASTER, the open bump pull requests with $STUB_PRS and
# the comparison with one commit line; cargo rewrites the lock as `cargo update` would.
STUB_GH = """#!/usr/bin/env bash
echo "gh $*" >> "$STUB_LOG"
case "$*" in
"api repos/Smithay/smithay/commits/master"*) echo "$STUB_MASTER" ;;
"pr list"*) printf '%s' "$STUB_PRS" ;;
"api repos/Smithay/smithay/compare/"*) echo "abc input: fix the grab for @someone in #12" ;;
*) exit 9 ;;
esac
"""
STUB_CARGO = """#!/usr/bin/env bash
echo "cargo $*" >> "$STUB_LOG"
sed -i "s/$STUB_OLD/$STUB_MASTER/g" "$(git rev-parse --show-toplevel)/Cargo.lock"
"""


def manifest(rev):
    return (
        f'smithay = {{ git = "{URL}", rev = "{rev}", default-features = false }}\n'
        f'smithay-drm-extras = {{ git = "{URL}", rev = "{rev}" }}\n'
    )


class BumpSmithay(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        self.work, self.out, stub = root / "work", root / "out", root / "bin"
        for directory in (self.work, stub):
            directory.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=self.work, check=True)
        (self.work / MANIFEST).parent.mkdir(parents=True)
        (self.work / MANIFEST).write_text(manifest(OLD))
        (self.work / "Cargo.lock").write_text(
            f'name = "smithay"\nsource = "git+{URL}?rev={OLD}#{OLD}"\n'
        )
        for name, text in (("gh", STUB_GH), ("cargo", STUB_CARGO)):
            (stub / name).write_text(text)
            (stub / name).chmod(0o755)
        self.log = root / "calls.log"
        self.env = {
            **os.environ,
            "PATH": f"{stub}:{os.environ['PATH']}",
            "STUB_LOG": str(self.log),
            "STUB_OLD": OLD,
        }

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, master, open_prs=""):
        env = {**self.env, "STUB_MASTER": master, "STUB_PRS": open_prs}
        return subprocess.run(
            [str(SCRIPT), str(self.out)],
            cwd=self.work,
            env=env,
            capture_output=True,
            text=True,
        )

    def calls(self):
        return self.log.read_text() if self.log.exists() else ""

    def test_master_ahead_moves_both_pins_and_the_lock(self):
        result = self.run_script(NEW)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.work / MANIFEST).read_text(), manifest(NEW))
        self.assertIn(f"?rev={NEW}#{NEW}", (self.work / "Cargo.lock").read_text())
        self.assertIn("cargo update", self.calls())
        self.assertEqual(
            (self.out / "title").read_text(),
            f"chore(sulphur): bump Smithay to {NEW[:12]}\n",
        )
        body = (self.out / "body.md").read_text()
        self.assertIn(f"{URL}/compare/{OLD}...{NEW}", body)
        self.assertIn("```text\nabc input: fix the grab for @someone in #12\n```", body)

    def test_pin_at_master_writes_nothing(self):
        result = self.run_script(OLD)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.out / "title").exists())
        self.assertEqual((self.work / MANIFEST).read_text(), manifest(OLD))
        self.assertNotIn("cargo", self.calls())

    def test_an_open_bump_writes_nothing(self):
        result = self.run_script(NEW, open_prs="1101\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("#1101 is open", result.stdout)
        self.assertFalse((self.out / "title").exists())
        self.assertEqual((self.work / MANIFEST).read_text(), manifest(OLD))
        self.assertNotIn("cargo", self.calls())

    def test_two_pins_apart_fail(self):
        (self.work / MANIFEST).write_text(manifest(OLD).replace(OLD, NEW, 1))
        result = self.run_script(NEW)
        self.assertEqual(result.returncode, 1)
        self.assertIn("at 2 revisions", result.stderr)

    def test_a_lock_cargo_did_not_move_fails(self):
        self.env["STUB_OLD"] = "nothing-to-replace"
        result = self.run_script(NEW)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Cargo.lock does not take Smithay", result.stderr)
        self.assertFalse((self.out / "title").exists())


if __name__ == "__main__":
    unittest.main()
