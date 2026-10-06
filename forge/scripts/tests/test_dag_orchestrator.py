"""Unit tests of forge/scripts/dag_orchestrator.py: a package's hash covers the Cargo path
dependencies it builds from (python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

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

    def test_a_path_dependency_inherited_from_the_workspace_is_followed(self):
        workspace = self.root / "specs/athanor-shell"
        workspace.mkdir(parents=True)
        (workspace / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["shell-1.0.0"]\n\n'
            '[workspace.dependencies]\nunit = { path = "../../system/unit" }\nserde = "1"\n'
        )
        crate(self.root, "specs/athanor-shell/shell-1.0.0", "unit = { workspace = true }\nserde = { workspace = true }\n")
        self.assertEqual(dag.path_dependencies(str(workspace)), [str(self.root / "system/unit")])


class RegistryStateTest(unittest.TestCase):
    """UD41: a custom package is dirty when the registry lacks its hash tag. The registry is
    a stub skopeo on PATH, so the whole chain runs: check_idempotency.sh --hash-only,
    retry.sh and registry_probe.sh."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name).resolve()
        (root / "forge/specs/athanor-dock").mkdir(parents=True)
        (root / "forge/specs/athanor-dock/dock.spec").write_text("Name: athanor-dock\n")
        (root / "bin").mkdir()
        stub = root / "bin/skopeo"
        stub.write_text(
            '#!/usr/bin/env bash\ncat "$ANSWER_DIR/answer" >&2\n'
            '[[ $(cat "$ANSWER_DIR/status") == 0 ]] && echo sha256:00\nexit "$(cat "$ANSWER_DIR/status")"\n'
        )
        stub.chmod(0o755)
        self.root = root
        env = {
            "PATH": f"{root / 'bin'}:{os.environ['PATH']}",
            "ANSWER_DIR": str(root),
            "GITHUB_REPOSITORY_OWNER": "Acme",
            "RETRY_ATTEMPTS": "1",
        }
        patcher = mock.patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch.object(dag, "FORGE_DIR", str(root / "forge"))
        patcher.start()
        self.addCleanup(patcher.stop)

    def answer(self, status, text=""):
        (self.root / "status").write_text(str(status))
        (self.root / "answer").write_text(text)

    def dirty(self):
        return dag.evaluate_dirty_nodes({"dock", "libx", "fp"}, {"dock": "custom", "libx": "upstream", "fp": "flatpak"})

    def test_tag_present_is_clean(self):
        self.answer(0)
        self.assertEqual(set(), self.dirty())

    def test_tag_missing_is_dirty_and_only_for_custom_packages(self):
        self.answer(1, "manifest unknown: manifest unknown")
        self.assertEqual({"dock"}, self.dirty())

    def test_never_published_package_is_dirty(self):
        self.answer(1, "Requesting bearer token: invalid status code from registry 403 (Forbidden)")
        self.assertEqual({"dock"}, self.dirty())

    def test_registry_error_stops_the_run(self):
        self.answer(1, "dial tcp: i/o timeout")
        with self.assertRaises(subprocess.CalledProcessError):
            self.dirty()

    def test_lookup_asks_for_the_hash_tag_of_the_node_image(self):
        self.answer(0)
        refs = []
        dirty = dag.evaluate_dirty_nodes(
            {"dock"}, {"dock": "custom"}, exists=lambda r: refs.append(r) or True
        )
        self.assertEqual(set(), dirty)
        self.assertRegex(refs[0], r"^ghcr\.io/Acme/athanor-forge-dock:hash-[0-9a-f]{64}$")


if __name__ == "__main__":
    unittest.main()
