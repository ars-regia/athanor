"""Unit tests of forge/scripts/dag_orchestrator.py: path_dependencies finds the Cargo path
dependencies a package builds from, and the registry decides what is dirty
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import subprocess
import tempfile
import threading
import time
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
        nodes = {"dock": "custom", "libx": "upstream", "fp": "flatpak"}
        return dag.evaluate_dirty_nodes(dag.custom_hashes(set(nodes), nodes))

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
        with self.assertRaisesRegex(RuntimeError, "i/o timeout"):
            self.dirty()

    def test_every_custom_hash_is_written_for_the_system_image_build(self):
        self.answer(0)
        state = self.root / "state"
        with mock.patch.dict(os.environ, {"DAG_STATE_DIR": str(state)}):
            hashes = dag.custom_hashes({"dock", "libx"}, {"dock": "custom", "libx": "upstream"})
            dag.write_hashes(hashes)
        self.assertEqual(hashes, json.loads((state / "hashes.json").read_text()))
        self.assertEqual(["dock"], list(hashes))

    def test_lookups_run_concurrently_within_the_bound(self):
        lock = threading.Lock()
        running = peak = 0

        def exists(ref):
            nonlocal running, peak
            with lock:
                running += 1
                peak = max(peak, running)
            time.sleep(0.02)
            with lock:
                running -= 1
            return True

        hashes = {f"p{i}": "0" * 64 for i in range(40)}
        self.assertEqual(set(), dag.evaluate_dirty_nodes(hashes, exists=exists))
        self.assertGreater(peak, 1)
        self.assertLessEqual(peak, dag.PROBE_WORKERS)

    def test_the_first_failure_in_completion_order_ends_the_run(self):
        release = threading.Event()
        self.addCleanup(release.set)
        started = []

        def exists(ref):
            started.append(ref)
            if "athanor-forge-bad:" in ref:
                raise RuntimeError("registry down")
            release.wait(5)  # slower than the failure: it must not be waited for
            return True

        # The failing node is submitted after the slow ones and ahead of the pending queue.
        hashes = {f"slow{i}": "0" * 64 for i in range(7)}
        hashes["bad"] = "0" * 64
        hashes.update({f"late{i}": "0" * 64 for i in range(30)})
        begin = time.monotonic()
        with self.assertRaisesRegex(RuntimeError, "registry down"):
            dag.evaluate_dirty_nodes(hashes, exists=exists)
        self.assertLess(time.monotonic() - begin, 2)
        self.assertLessEqual(len(started), dag.PROBE_WORKERS + 1)  # pending lookups cancelled

    def test_lookup_asks_for_the_hash_tag_of_the_node_image(self):
        self.answer(0)
        refs = []
        dirty = dag.evaluate_dirty_nodes(
            dag.custom_hashes({"dock"}, {"dock": "custom"}), exists=lambda r: refs.append(r) or True
        )
        self.assertEqual(set(), dirty)
        self.assertRegex(refs[0], r"^ghcr\.io/Acme/athanor-forge-dock:hash-[0-9a-f]{64}$")


if __name__ == "__main__":
    unittest.main()
