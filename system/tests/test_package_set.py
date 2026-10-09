"""system/package-set.sh: the resolved package set of an image (ADR-0103 D16, UD28)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "package-set.sh"


class PackageSet(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        (self.dir / "bin").mkdir()
        self.out = self.dir / "packages" / "athanor-system.txt"

    def tearDown(self):
        self.tmp.cleanup()

    def podman(self, body):
        stub = self.dir / "bin" / "podman"
        stub.write_text(f'#!/usr/bin/env bash\necho "$*" >> {self.dir}/podman.args\n{body}\n')
        stub.chmod(0o755)
        env = {**os.environ, "PATH": f"{self.dir / 'bin'}:{os.environ['PATH']}"}
        return subprocess.run(["bash", str(SCRIPT), "r.example/o/athanor-system:412", str(self.out)], capture_output=True, text=True, env=env)

    def test_the_set_is_sorted_and_read_offline(self):
        r = self.podman("printf 'zlib-1.3-1.fc43.x86_64 aa\\nbash-5.2-1.fc43.x86_64 bb\\n'")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.out.read_text(), "bash-5.2-1.fc43.x86_64 bb\nzlib-1.3-1.fc43.x86_64 aa\n")
        args = (self.dir / "podman.args").read_text()
        self.assertIn("--network=none", args)
        self.assertIn("%{NEVRA} %{SHA256HEADER}", args)

    def test_a_podman_failure_leaves_no_file(self):
        r = self.podman("echo 'Error: image not known' >&2; exit 125")
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(self.out.exists())

    def test_an_empty_set_is_refused(self):
        r = self.podman("exit 0")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("no packages", r.stderr)
        self.assertFalse(self.out.exists())


if __name__ == "__main__":
    unittest.main()
