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

    def test_the_repository_is_hashed_by_default(self):
        self.assertEqual(digest(SHELL), subprocess.run(
            ["bash", str(SHELL / "build-inputs.sh")], check=True, capture_output=True, text=True,
        ).stdout.strip())


if __name__ == "__main__":
    unittest.main()
