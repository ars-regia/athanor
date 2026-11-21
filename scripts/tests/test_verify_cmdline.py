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

    def test_a_security_parameter_is_rejected_bare_or_with_any_value(self):
        for parameter in ["selinux=0", "enforcing=0", "lsm=yama", "init=/bin/sh", "rd.break", "rd.break=pre-mount", "cfi=off"]:
            with self.subTest(parameter=parameter):
                self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", f"page_alloc.shuffle=1 {parameter}")
                self.assertTrue(any(f"{parameter} -> " in p for p in self.problems()), self.problems())
                self.edit("forge/specs/azoth/cmdline", f"page_alloc.shuffle=1 {parameter}", "page_alloc.shuffle=1")

    def test_a_kargs_element_holding_several_parameters_is_read_word_by_word(self):
        self.edit(KARGS, '"page_alloc.shuffle=1",', '"page_alloc.shuffle=1 quiet selinux=0",')
        self.assertTrue(any("selinux=0 ->" in p for p in self.problems()), self.problems())

    def test_a_kargs_element_that_is_no_string_is_reported(self):
        self.edit(KARGS, '"page_alloc.shuffle=1",', "42,")
        self.assertTrue(any(KARGS in p and "cannot read" in p for p in self.problems()), self.problems())

    def test_a_double_quoted_parameter_is_read_as_the_kernel_reads_it(self):
        self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", 'page_alloc.shuffle=1 "selinux=0"')
        self.assertTrue(any("selinux=0 ->" in p for p in self.problems()), self.problems())

    def test_a_quoted_run_with_spaces_is_one_parameter(self):
        for words, parameter in (
            ('quiet "selinux=0 x"', "selinux=0 x"),
            ('"lsm=a b"', "lsm=a b"),
            ('quiet "selinux=0', "selinux=0"),
        ):
            with self.subTest(words=words):
                self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", f"page_alloc.shuffle=1 {words}")
                self.assertTrue(any(f"{parameter} ->" in p for p in self.problems()), self.problems())
                self.edit("forge/specs/azoth/cmdline", f"page_alloc.shuffle=1 {words}", "page_alloc.shuffle=1")

    def test_a_character_outside_ascii_is_a_finding(self):
        for text in ("a\u00e0selinux=0", "quiet\u00a0selinux=0", "quiet\x0bselinux=0"):
            with self.subTest(text=text):
                self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", f"page_alloc.shuffle=1 {text}")
                self.assertTrue(
                    any("cmdline" in p and "non-ASCII byte" in p for p in self.problems()), self.problems()
                )
                self.edit("forge/specs/azoth/cmdline", f"page_alloc.shuffle=1 {text}", "page_alloc.shuffle=1")
        self.edit(KARGS, '"page_alloc.shuffle=1",', '"page_alloc.shuffle=1 a\u00e0selinux=0",')
        self.assertTrue(any(KARGS in p and "non-ASCII byte" in p for p in self.problems()), self.problems())

    def test_a_dash_in_a_name_is_an_underscore(self):
        self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", "page_alloc.shuffle=1 amd-iommu=on x=a-b")
        found = self.problems()
        self.assertTrue(any("amd-iommu=on ->" in p for p in found), found)
        self.edit("forge/specs/azoth/cmdline", " amd-iommu=on", "")
        self.assertEqual(self.problems(), [])

    def test_a_name_that_only_starts_with_a_rejected_bare_one_passes(self):
        self.edit("forge/specs/azoth/cmdline", "page_alloc.shuffle=1", "page_alloc.shuffle=1 rd.breakpoint xselinux=0 rd.break.x")
        self.assertEqual(self.problems(), [])

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
