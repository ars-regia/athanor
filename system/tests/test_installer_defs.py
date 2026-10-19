"""The installer package list exists for the release the image is built on
(python3 -B -m unittest discover -s system/tests -v).

bootc-image-builder looks for `defs/<ID>-<VERSION_ID>.yaml`; without it the ISO step has no
installer packages. Raising `ARG FEDORA_VERSION` in system/Containerfile must come with a new
`system/disk_config/defs/athanor-<N>.yaml`, so the missing file fails here, not at ISO time.
"""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


def fedora_version(containerfile):
    # The same read as system/build-image.sh: the first `ARG FEDORA_VERSION=<major>`.
    m = re.search(r"^ARG FEDORA_VERSION=(\d+)$", containerfile.read_text(), re.M)
    return m.group(1) if m else None


class InstallerDefsTest(unittest.TestCase):
    def check(self, containerfile, defs):
        n = fedora_version(containerfile)
        self.assertIsNotNone(n, f"{containerfile} declares no ARG FEDORA_VERSION=<major>")
        self.assertTrue((defs / f"athanor-{n}.yaml").is_file(), f"no athanor-{n}.yaml in {defs}")

    def test_definition_exists_for_the_image_release(self):
        self.check(ROOT / "system/Containerfile", ROOT / "system/disk_config/defs")

    def test_a_release_without_a_definition_fails(self):
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            cf = pathlib.Path(d) / "Containerfile"
            cf.write_text("ARG FEDORA_VERSION=99\n")
            with self.assertRaises(AssertionError):
                self.check(cf, ROOT / "system/disk_config/defs")


if __name__ == "__main__":
    unittest.main()
