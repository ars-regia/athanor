import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent


def digest(directory):
    return subprocess.run(
        ["bash", str(SHELL / "build-inputs.sh"), str(directory)],
        check=True, capture_output=True, text=True,
    ).stdout.strip()


class BuildInputs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        shutil.copy(SHELL / "Containerfile", self.dir)

    def tearDown(self):
        self.tmp.cleanup()

    def test_stable_and_a_sha256(self):
        self.assertRegex(digest(self.dir), r"[0-9a-f]{64}")
        self.assertEqual(digest(self.dir), digest(self.dir))

    def test_changes_with_the_containerfile(self):
        before = digest(self.dir)
        with open(self.dir / "Containerfile", "a", encoding="utf-8") as f:
            f.write("RUN true\n")
        self.assertNotEqual(before, digest(self.dir))

    def test_changes_with_the_rig_base(self):
        before = digest(self.dir)
        (self.dir / "rig-image.digest").write_text("sha256:" + "0" * 64 + "\n")
        self.assertNotEqual(before, digest(self.dir))

    def test_a_missing_containerfile_fails(self):
        (self.dir / "Containerfile").unlink()
        with self.assertRaises(subprocess.CalledProcessError):
            digest(self.dir)

    def test_the_containerfile_takes_nothing_from_the_context(self):
        # build-inputs.sh hashes only the Containerfile and rig-image.digest: a file copied or
        # mounted into the build would change the stage without changing the hash.
        text = (SHELL / "Containerfile").read_text(encoding="utf-8")
        found = re.findall(r"(?im)^\s*(?:COPY|ADD)\b.*$|--mount\b.*$", text)
        self.assertEqual(found, [], "add these inputs to build-inputs.sh, then update this test")


if __name__ == "__main__":
    unittest.main()
