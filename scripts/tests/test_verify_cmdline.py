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

KARGS = "forge/specs/athanor-kernel-profile/SOURCES/usr/lib/bootc/kargs.d/10-athanor-kernel-profile.toml"
NVIDIA_KARGS = "system/nvidia/athanor-nvidia-config/SOURCES/usr/lib/bootc/kargs.d/01-nvidia.toml"
SITES = ["forge/specs/azoth/cmdline", KARGS, NVIDIA_KARGS]


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
            ("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", "page_alloc.shuffle=1 zswap.enabled=1", "zswap.enabled=1", "D15"),
            (KARGS, '"page_alloc.shuffle=1",', '"page_alloc.shuffle=1",\n    "oops=panic",', "oops=panic", "D19"),
            (NVIDIA_KARGS, '"nvidia-drm.modeset=1",', '"nvidia-drm.modeset=1", "iommu=pt",', "iommu=pt", "D16"),
        ]
        for site, old, new, parameter, why in cases:
            with self.subTest(site=site):
                self.edit(site, old, new)
                found = self.problems()
                self.assertTrue(any(site in p and parameter in p and why in p for p in found), found)
                self.edit(site, new, old)

    def test_every_zswap_parameter_is_rejected(self):
        self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", "page_alloc.shuffle=1 zswap.compressor=zstd")
        self.assertTrue(any("zswap.compressor=zstd" in p for p in self.problems()))

    def test_a_parameter_that_only_contains_a_rejected_one_passes(self):
        self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", "page_alloc.shuffle=1 xiommu=pt not_iommu=pt")
        self.assertEqual(self.problems(), [])

    def test_a_site_whose_line_cannot_be_read_is_reported(self):
        self.edit(NVIDIA_KARGS, "kargs =", "arguments =")
        self.assertTrue(any("01-nvidia.toml" in p and "cannot read" in p for p in self.problems()))
        (self.root / "forge/specs/azoth/cmdline").unlink()
        self.assertTrue(any("azoth/cmdline" in p and "cannot read" in p for p in self.problems()))


if __name__ == "__main__":
    unittest.main()
