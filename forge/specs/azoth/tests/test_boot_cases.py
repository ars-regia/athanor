"""The cases of the boot matrix (forge/specs/azoth/boot.sh, case_args), run on the host:
python3 -B -m unittest discover -s forge/specs/azoth/tests"""

import pathlib
import re
import subprocess
import unittest

BOOT = pathlib.Path(__file__).resolve().parents[1] / "boot.sh"


def function(name):
    match = re.search(rf"^{name}\(\) \{{.*?^\}}$", BOOT.read_text(), re.M | re.S)
    assert match, f"boot.sh has no function {name}"
    return match.group(0)


def case_args(name, accel="kvm"):
    script = (
        f'ACCEL={accel} OUT=/o WORK=/w VMLINUZ=/w/vmlinuz OVMF_CODE=/c BIOS_CMDLINE="base k3.x=1"\n'
        'die() { echo "error: $*" >&2; exit 1; }\n'
        f"{function('case_args')}\n"
        f'case_args {name}\nprintf "%s\\n" "${{CASE_ARGS[@]}}"\n'
    )
    result = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    return result.returncode, result.stdout.splitlines(), result.stderr


class Cases(unittest.TestCase):
    def test_the_default_matrix_has_six_cases_and_no_nehalem(self):
        text = BOOT.read_text()
        self.assertIn("CASES=(bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd)", text)
        self.assertNotIn("Nehalem", text)
        self.assertNotIn("nehalem", text)

    def test_penryn_is_the_baseline_cpu(self):
        code, args, _ = case_args("bios-penryn")
        self.assertEqual(code, 0)
        self.assertEqual(args[args.index("-cpu") + 1], "Penryn")

    def test_host_becomes_max_under_tcg(self):
        _, args, _ = case_args("uefi-host", accel="tcg")
        self.assertEqual(args[args.index("-cpu") + 1], "max")
        self.assertIn("if=pflash,format=raw,file=/w/vars-uefi-host.fd", args)

    def test_intel_iommu_case(self):
        code, args, _ = case_args("iommu-intel")
        self.assertEqual(code, 0)
        self.assertIn("kernel-irqchip=split", args[args.index("-machine") + 1])
        self.assertLess(args.index("intel-iommu,intremap=on"), args.index("virtio-rng-pci"))
        self.assertEqual(args[args.index("-append") + 1], "base k3.x=1 k3.iommu=intel")

    def test_amd_iommu_case(self):
        code, args, _ = case_args("iommu-amd")
        self.assertEqual(code, 0)
        self.assertIn("amd-iommu,intremap=on", args)
        self.assertEqual(args[args.index("-append") + 1], "base k3.x=1 k3.iommu=amd")

    def test_bios_cases_boot_the_kernel_directly(self):
        _, args, _ = case_args("bios-host")
        self.assertEqual(args[args.index("-kernel") + 1], "/w/vmlinuz")
        self.assertNotIn("k3.iommu", " ".join(args))

    def test_an_unknown_case_fails(self):
        code, _, err = case_args("bios-nehalem")
        self.assertNotEqual(code, 0)
        self.assertIn("unknown case: bios-nehalem", err)


def insmod_spec(spec):
    script = 'die() { echo "error: $*" >&2; exit 1; }\n' + function("insmod_spec") + f'\ninsmod_spec "{spec}"\n'
    result = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    return result.returncode, result.stdout.split()


class InsmodSpecs(unittest.TestCase):
    def test_one_errno_applies_to_both_firmwares(self):
        self.assertEqual(insmod_spec("a.ko:ENODEV"), (0, ["a.ko", "ENODEV", "ENODEV"]))

    def test_a_bios_errno_overrides(self):
        self.assertEqual(insmod_spec("a.ko:ENODEV:EKEYREJECTED"), (0, ["a.ko", "ENODEV", "EKEYREJECTED"]))

    def test_malformed_insmod_specs_are_refused(self):
        for spec in ("a.ko", "a.ko:", "a.ko:ENODEV:", "a.ko:EPERM", "a.ko:ENODEV:EPERM", ":ENODEV"):
            with self.subTest(spec):
                self.assertNotEqual(insmod_spec(spec)[0], 0)


if __name__ == "__main__":
    unittest.main()
