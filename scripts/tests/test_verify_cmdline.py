"""Unit tests of the kernel command line assertion of scripts/verify.py cmdline
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import shutil
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

SITES = [
    "forge/specs/azoth/cmdline",
    "forge/specs/athanor-base-config/SOURCES/usr/lib/bootc/kargs.d",
    "system/scripts/assemble_uki.sh",
    "forge/build/build_uki.sh",
    "system/athanor-install.ks",
]


class Cmdline(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for site in SITES:
            source, target = ROOT / site, self.root / site
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_dir():
                shutil.copytree(source, target)
            else:
                shutil.copy(source, target)

    def tearDown(self):
        self.tmp.cleanup()

    def edit(self, relative, old, new):
        path = self.root / relative
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new, 1))

    def problems(self):
        return verify.cmdline_problems(self.root)

    def test_the_command_lines_as_committed_have_no_problem(self):
        self.assertEqual(self.problems(), [])
        self.assertEqual(verify.cmdline_problems(), [])

    def test_a_parameter_the_decisions_reject_is_named_where_it_is_found(self):
        cases = [
            ("forge/specs/azoth/cmdline", "vsyscall=none", "vsyscall=none zswap.enabled=1", "zswap.enabled=1", "D15"),
            ("system/scripts/assemble_uki.sh", "quiet splash", "iommu=pt quiet splash", "iommu=pt", "D16"),
            ("forge/build/build_uki.sh", "quiet splash", "oops=panic quiet splash", "oops=panic", "D19"),
            ("system/athanor-install.ks", "quiet splash", "pti=on quiet splash", "pti=on", "Meltdown"),
        ]
        for site, old, new, parameter, why in cases:
            with self.subTest(site=site):
                self.edit(site, old, new)
                found = self.problems()
                self.assertTrue(any(site in p and parameter in p and why in p for p in found), found)
                self.edit(site, new, old)

    def test_a_kargs_file_is_read(self):
        self.edit("forge/specs/athanor-base-config/SOURCES/usr/lib/bootc/kargs.d/02-hardening.toml", '"slab_nomerge",', '"slab_nomerge",\n    "oops=panic",')
        self.assertTrue(any("02-hardening.toml" in p and "oops=panic" in p for p in self.problems()))

    def test_every_zswap_parameter_is_rejected(self):
        self.edit("forge/specs/azoth/cmdline", "vsyscall=none", "vsyscall=none zswap.compressor=zstd")
        self.assertTrue(any("zswap.compressor=zstd" in p for p in self.problems()))

    def test_a_parameter_that_only_contains_a_rejected_one_passes(self):
        self.edit("forge/specs/azoth/cmdline", "vsyscall=none", "vsyscall=none xiommu=pt not_iommu=pt")
        self.assertEqual(self.problems(), [])

    def test_a_site_whose_line_cannot_be_read_is_reported(self):
        self.edit("system/scripts/assemble_uki.sh", "CMDLINE_STR=", "OTHER_NAME=")
        self.assertTrue(any("assemble_uki.sh" in p and "cannot read" in p for p in self.problems()))
        (self.root / "forge/specs/azoth/cmdline").unlink()
        self.assertTrue(any("azoth/cmdline" in p and "cannot read" in p for p in self.problems()))


if __name__ == "__main__":
    unittest.main()
