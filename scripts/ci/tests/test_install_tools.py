"""scripts/ci/install-tools.sh: a download that does not match its pinned digest is refused."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "install-tools.sh"

# Writes a payload that cannot match any pinned digest to the -o target.
FAKE_CURL = """#!/usr/bin/env bash
while [[ $# -gt 0 ]]; do
    if [[ $1 == -o ]]; then printf 'not the archive\\n' > "$2"; shift; fi
    shift
done
"""


class ChecksumTest(unittest.TestCase):
    def test_a_mismatching_download_stops_the_install(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = pathlib.Path(tmp) / "fake"
            fake.mkdir()
            (fake / "curl").write_text(FAKE_CURL)
            (fake / "curl").chmod(0o755)
            out = pathlib.Path(tmp) / "tools"
            env = {**os.environ, "LC_ALL": "C", "PATH": f"{fake}:{os.environ['PATH']}"}
            run = subprocess.run(
                ["bash", str(SCRIPT), str(out)],
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("FAILED", run.stdout + run.stderr)
            self.assertFalse((out / "bin" / "just").exists())

    def test_the_directory_is_required(self):
        run = subprocess.run(["bash", str(SCRIPT)], capture_output=True, text=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("usage", run.stderr)


if __name__ == "__main__":
    unittest.main()
