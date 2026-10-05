"""Tests the selection of forge/scripts/build_changed_specs.sh: a changed spec directory is
built only when the DAG builds it (python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import json
import pathlib
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parents[1]


class SelectionTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "forge/scripts").mkdir(parents=True)
        for name in ("build_changed_specs.sh", "dag_orchestrator.py"):
            shutil.copy(SCRIPTS / name, self.root / "forge/scripts" / name)
        (self.root / "forge/config").mkdir()
        (self.root / "forge/config/packages.json").write_text(
            json.dumps({"custom_packages": ["bar", "plain", "kernel-forge"]})
        )
        self.git("init", "-q", "-b", "main")
        self.git("commit", "-q", "--allow-empty", "-m", "base")
        for name in ("athanor-bar", "plain", "athanor-dead", "athanor-empty"):
            directory = self.root / "forge/specs" / name
            directory.mkdir(parents=True)
            if name != "athanor-empty":
                (directory / "x.spec").write_text("Name: x\n")
        (self.root / "forge/specs/athanor-empty/README").write_text("")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "specs")

    def git(self, *args):
        subprocess.run(
            ["git", "-C", str(self.root), "-c", "user.name=t", "-c", "user.email=t@t", *args],
            check=True,
        )

    def test_only_dag_specs_are_built(self):
        out = subprocess.run(
            ["bash", str(self.root / "forge/scripts/build_changed_specs.sh"), "--dry-run", "HEAD^1"],
            check=True, capture_output=True, text=True, cwd=self.root,
        ).stdout
        self.assertIn("would build forge/specs/athanor-bar\n", out)
        self.assertIn("would build forge/specs/plain\n", out)
        self.assertIn("forge/specs/athanor-dead is not built by the DAG", out)
        self.assertIn("forge/specs/athanor-empty is not built by the DAG", out)
        self.assertIn("built 2 spec(s), skipped 2", out)


if __name__ == "__main__":
    unittest.main()
