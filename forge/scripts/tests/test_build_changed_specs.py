"""Tests the selection of forge/scripts/build_changed_specs.sh (select_check_specs.py): a changed spec directory is
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
        for name in ("build_changed_specs.sh", "dag_orchestrator.py", "select_check_specs.py"):
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

    def run_script(self, base):
        return subprocess.run(
            ["bash", str(self.root / "forge/scripts/build_changed_specs.sh"), "--dry-run", base],
            capture_output=True, text=True, cwd=self.root,
        )

    def commit_change(self, path, content):
        (self.root / path).parent.mkdir(parents=True, exist_ok=True)
        (self.root / path).write_text(content)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "change")

    def test_bad_base_fails(self):
        self.assertNotEqual(self.run_script("no-such-ref").returncode, 0)

    def test_failing_orchestrator_fails(self):
        (self.root / "forge/scripts/dag_orchestrator.py").write_text("raise SystemExit(3)\n")
        self.assertNotEqual(self.run_script("HEAD^1").returncode, 0)

    def test_missing_orchestrator_fails(self):
        (self.root / "forge/scripts/dag_orchestrator.py").unlink()
        self.assertNotEqual(self.run_script("HEAD^1").returncode, 0)

    def test_missing_or_empty_manifest_fails(self):
        manifest = self.root / "forge/config/packages.json"
        manifest.write_text(json.dumps({"custom_packages": []}))
        self.assertNotEqual(self.run_script("HEAD^1").returncode, 0)
        manifest.unlink()
        self.assertNotEqual(self.run_script("HEAD^1").returncode, 0)
        manifest.write_text("{")
        self.assertNotEqual(self.run_script("HEAD^1").returncode, 0)

    def test_external_packages_are_not_listed(self):
        out = subprocess.run(
            ["python3", "scripts/dag_orchestrator.py", "--list-spec-dirs"],
            check=True, capture_output=True, text=True, cwd=self.root / "forge",
        ).stdout.split()
        self.assertEqual(out, ["specs/athanor-bar", "specs/plain"])

    def test_dag_spec_without_a_spec_file_is_skipped(self):
        (self.root / "forge/specs/plain/x.spec").unlink()
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "drop spec")
        out = self.run_script("HEAD^1").stdout
        self.assertIn("forge/specs/plain holds no spec after the change, skipped", out)

    def test_nothing_changed_builds_nothing(self):
        self.commit_change("README", "x")
        out = self.run_script("HEAD^1").stdout
        self.assertIn("0 spec(s) selected, 0 skipped", out)

    def test_only_dag_specs_are_built(self):
        out = self.run_script("HEAD^1").stdout
        self.assertIn("would build forge/specs/athanor-bar\n", out)
        self.assertIn("would build forge/specs/plain\n", out)
        self.assertIn("forge/specs/athanor-dead is not built by the DAG", out)
        self.assertIn("forge/specs/athanor-empty is not built by the DAG", out)
        self.assertIn("2 spec(s) selected, 2 skipped", out)


if __name__ == "__main__":
    unittest.main()
