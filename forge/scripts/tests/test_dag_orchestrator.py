"""Unit tests of forge/scripts/dag_orchestrator.py: a package's hash covers the Cargo path
dependencies it builds from (python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "dag_orchestrator.py"
spec = importlib.util.spec_from_file_location("dag_orchestrator", SCRIPT)
dag = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dag)


def crate(root, rel, deps=""):
    directory = root / rel
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "Cargo.toml").write_text(
        f'[package]\nname = "{directory.name}"\nversion = "1.0.0"\n\n[dependencies]\n{deps}'
    )
    (directory / "lib.rs").write_text(f"// {rel}\n")
    return directory


class PathDependenciesTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name).resolve()
        crate(self.root, "system/unit")
        crate(self.root, "system/apps", 'unit = { path = "../unit" }\n')
        self.spec = self.root / "specs/athanor-dock"
        crate(
            self.root,
            "specs/athanor-dock/dock-1.0.0",
            'apps = { path = "../../../system/apps" }\ninner = { path = "inner" }\nserde = "1"\n',
        )
        crate(self.root, "specs/athanor-dock/dock-1.0.0/inner")

    def test_path_dependencies_are_followed_and_those_inside_are_skipped(self):
        self.assertEqual(
            dag.path_dependencies(str(self.spec)),
            [str(self.root / "system/apps"), str(self.root / "system/unit")],
        )

    def test_a_change_two_path_dependencies_away_changes_the_package_hash(self):
        before = dag.package_hash(str(self.spec))
        (self.root / "system/unit/lib.rs").write_text("// changed\n")
        self.assertNotEqual(dag.package_hash(str(self.spec)), before)

    def test_a_package_without_path_dependencies_keeps_its_directory_hash(self):
        plain = crate(self.root, "specs/athanor-plain/plain-1.0.0", 'serde = "1"\n').parent
        self.assertEqual(dag.package_hash(str(plain)), dag.compute_dir_hash(str(plain)))


if __name__ == "__main__":
    unittest.main()
