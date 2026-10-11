"""system/check-image-revision.sh with a podman stub that answers the label of each image
(python3 -B -m unittest discover -s system/tests -v)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "check-image-revision.sh"
REV = "a" * 40


class CheckImageRevision(unittest.TestCase):
    def run_with(self, labels, *args):
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = pathlib.Path(tmp)
            # The stub prints the label of the image named last, or "<no value>" as podman does
            # for a label the image lacks.
            cases = "".join(
                f'  "{image}") echo "{label}" ;;\n' for image, label in labels.items()
            )
            (bin_dir / "podman").write_text(
                '#!/usr/bin/env bash\ncase "${@: -1}" in\n'
                + cases
                + '  *) echo "<no value>" ;;\nesac\n'
            )
            (bin_dir / "podman").chmod(0o755)
            return subprocess.run(
                ["bash", str(SCRIPT), *args],
                capture_output=True,
                text=True,
                env={**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}"},
            )

    def test_images_that_name_the_commit_pass(self):
        r = self.run_with({"r/a:1": REV, "r/b:1": REV}, REV, "r/a:1", "r/b:1")
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_an_image_without_the_label_fails(self):
        r = self.run_with({"r/a:1": REV}, REV, "r/a:1", "r/b:1")
        self.assertEqual(r.returncode, 1)
        self.assertIn("r/b:1", r.stderr)

    def test_an_image_of_another_commit_fails(self):
        self.assertEqual(self.run_with({"r/a:1": "b" * 40}, REV, "r/a:1").returncode, 1)

    def test_a_revision_that_is_not_a_commit_is_a_usage_error(self):
        self.assertEqual(self.run_with({}, "iso-v0", "r/a:1").returncode, 2)


if __name__ == "__main__":
    unittest.main()
