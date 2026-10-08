"""The inputs of a package's content hash: build scripts, workspace manifests, declared repo
paths, and a check that no spec names a repo path it has not declared
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import tempfile
import unittest
from unittest import mock

FORGE = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = FORGE / "scripts/dag_orchestrator.py"
spec = importlib.util.spec_from_file_location("dag_orchestrator", SCRIPT)
dag = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dag)


class FixtureTest(unittest.TestCase):
    """A repository with an in-place cargo spec, run from its forge directory."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name).resolve()
        for rel in ("system/style", "system/other", "forge/specs/athanor-dock", "forge/scripts", "forge/builder", ".cargo"):
            (self.root / rel).mkdir(parents=True)
        for rel in ("Cargo.toml", "Cargo.lock", ".cargo/config.toml", "flake.nix", "flake.lock",
                    "forge/scripts/build_spec.sh", "forge/scripts/run_spec_build.sh",
                    "forge/scripts/fetch_sources.sh", "forge/scripts/check_shim_link_order.py",
                    "system/style/gen.py", "system/other/x.py"):
            (self.root / rel).write_text("x\n")
        self.write_spec("%build\ncargo build --release\n")
        cwd = os.getcwd()
        os.chdir(self.root / "forge")
        self.addCleanup(os.chdir, cwd)
        patcher = mock.patch.object(dag, "SPECS_DIR", "specs")
        patcher.start()
        self.addCleanup(patcher.stop)

    def write_spec(self, body, declare=()):
        text = "".join(f"# repo-input: {d}\n" for d in declare)
        text += "Name: athanor-dock\n" + body
        (self.root / "forge/specs/athanor-dock/dock.spec").write_text(text)


class InputsTest(FixtureTest):
    def test_an_in_place_cargo_build_reads_the_workspace_and_the_shared_scripts(self):
        inputs = dag.package_inputs("dock")
        for expected in ("../Cargo.toml", "../Cargo.lock", "../.cargo/config.toml", "../flake.lock",
                         "../flake.nix", "builder", "scripts/build_spec.sh", "scripts/run_spec_build.sh",
                         "scripts/fetch_sources.sh", "scripts/check_shim_link_order.py", "specs/athanor-dock"):
            self.assertIn(expected, inputs)

    def test_a_spec_with_a_source_does_not_read_the_workspace(self):
        self.write_spec("Source0: a.tar.gz\n%build\ncargo build\n")
        self.assertNotIn("../Cargo.lock", dag.package_inputs("dock"))

    def test_declared_inputs_are_inputs_and_must_exist(self):
        self.write_spec("%build\ntrue\n", declare=["system/style"])
        self.assertIn("../system/style", dag.package_inputs("dock"))
        self.write_spec("%build\ntrue\n", declare=["system/gone"])
        with self.assertRaises(SystemExit):
            dag.package_inputs("dock")


class UndeclaredReferenceTest(FixtureTest):
    def test_a_repo_path_the_spec_reads_without_declaring_it_is_reported(self):
        self.write_spec("%build\npython3 system/style/gen.py\n")
        self.assertEqual(["system/style/gen.py"], dag.undeclared_repo_references("dock"))

    def test_the_old_absolute_spelling_is_seen_too(self):
        self.write_spec("%build\ncd /forge/system/other\n")
        self.assertEqual(["system/other"], dag.undeclared_repo_references("dock"))

    def test_a_declaration_covers_what_is_under_it(self):
        self.write_spec("%build\npython3 system/style/gen.py\n", declare=["system/style"])
        self.assertEqual([], dag.undeclared_repo_references("dock"))

    def test_own_directory_description_prose_and_build_outputs_are_not_references(self):
        self.write_spec(
            "%description\nSee system/other/x.py.\n%build\n"
            "cp forge/specs/athanor-dock/data/a /x\ncp forge/specs/%{name}/b /x\ncp system/style/out/new.png /x\n"
        )
        self.assertEqual([], dag.undeclared_repo_references("dock"))


class RealSpecsTest(unittest.TestCase):
    def test_no_spec_of_the_dag_reads_a_repo_path_it_did_not_declare(self):
        cwd = os.getcwd()
        os.chdir(FORGE)
        self.addCleanup(os.chdir, cwd)
        patcher = mock.patch.object(dag, "SPECS_DIR", "specs")
        patcher.start()
        self.addCleanup(patcher.stop)
        with open("config/packages.json") as f:
            manifest = json.load(f)
        packages = sorted(set(manifest["custom_packages"]) - dag.EXTERNAL_PACKAGES)
        found = {p: dag.undeclared_repo_references(p) for p in packages}
        self.assertEqual({}, {p: r for p, r in found.items() if r})


if __name__ == "__main__":
    unittest.main()
