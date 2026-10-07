"""resolve_node_image.sh names the hash-tagged image the brain verified, never :latest
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "resolve_node_image.sh"


class ResolveNodeImageTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.state = pathlib.Path(tmp.name)

    def resolve(self, package):
        return subprocess.run(
            ["bash", SCRIPT, package], capture_output=True, text=True,
            env={**os.environ, "DAG_STATE_DIR": str(self.state)},
        )

    def test_names_the_hash_tag_of_the_package(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        result = self.resolve("bar")
        self.assertEqual(0, result.returncode)
        self.assertEqual("athanor-forge-bar:hash-ab12", result.stdout.strip())

    def test_a_package_without_a_hash_is_an_error(self):
        (self.state / "hashes.json").write_text(json.dumps({"bar": "ab12"}))
        self.assertNotEqual(0, self.resolve("dock").returncode)

    def test_a_missing_map_is_an_error(self):
        self.assertNotEqual(0, self.resolve("bar").returncode)


if __name__ == "__main__":
    unittest.main()
