"""The offline build hands no key to the image build: the vmlinuz arrives signed in azoth-boot
(docs/architecture/doc_ci.md, D43; ADR-0037), and the host-key recipes of the UKI era are gone."""

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]


class NoKeyInTheImageBuild(unittest.TestCase):
    def test_build_offline_passes_no_secret(self):
        script = (ROOT / "forge/scripts/build-offline.sh").read_text()
        self.assertNotIn("--secret", script)
        self.assertNotIn("/etc/pki", script)

    def test_no_justfile_handles_signing_keys(self):
        for justfile in ("Justfile", "system/Justfile"):
            with self.subTest(justfile=justfile):
                text = (ROOT / justfile).read_text()
                self.assertNotIn("secureboot-key-audit", text)
                self.assertNotIn("/etc/pki", text)


if __name__ == "__main__":
    unittest.main()
