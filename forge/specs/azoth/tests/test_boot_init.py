"""The assertions of forge/specs/azoth/boot/init, run against files and stub commands
the test writes (python3 -B -m unittest discover -s forge/specs/azoth/tests)."""

import gzip
import pathlib
import re
import subprocess
import tempfile
import unittest

INIT = pathlib.Path(__file__).resolve().parents[1] / "boot" / "init"


def function(name):
    """The text of `name() { ... }` in init, up to its closing brace at column 0."""
    match = re.search(rf"^{name}\(\) \{{.*?^\}}$", INIT.read_text(), re.M | re.S)
    assert match, f"init has no function {name}"
    return match.group(0)


class BootAssertions(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def run_check(self, name, lsm=None, controllers=None):
        lsm_file, controllers_file = self.dir / "lsm", self.dir / "cgroup.controllers"
        if lsm is not None:
            lsm_file.write_text(lsm)
        if controllers is not None:
            controllers_file.write_text(controllers)
        script = f'lsm_list="{lsm_file}"\ncgroup_controllers="{controllers_file}"\n{function(name)}\n{name}\n'
        return subprocess.run(["sh", "-c", script], capture_output=True, text=True)

    def test_landlock_in_the_active_list_passes(self):
        self.assertEqual(self.run_check("landlock", lsm="lockdown,capability,yama,selinux,bpf,landlock,ipe,ima,evm").returncode, 0)

    def test_landlock_missing_from_the_list_fails(self):
        result = self.run_check("landlock", lsm="lockdown,capability,yama,selinux,bpf,ipe,ima,evm")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("lsm: lockdown", result.stdout)

    def test_a_name_that_only_contains_landlock_fails(self):
        self.assertNotEqual(self.run_check("landlock", lsm="lockdown,notlandlock,landlock2").returncode, 0)

    def test_an_unreadable_list_fails(self):
        self.assertNotEqual(self.run_check("landlock").returncode, 0)

    def test_the_memory_controller_available_passes(self):
        self.assertEqual(self.run_check("memcg", controllers="cpuset cpu io memory hugetlb pids\n").returncode, 0)

    def test_the_memory_controller_missing_fails(self):
        result = self.run_check("memcg", controllers="cpuset cpu io pids\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("controllers: cpuset", result.stdout)

    def test_a_controller_that_only_contains_memory_fails(self):
        self.assertNotEqual(self.run_check("memcg", controllers="cpu memory_hotplug\n").returncode, 0)

    def test_init_runs_both_checks_and_mounts_what_they_read(self):
        text = INIT.read_text()
        for needle in ("check landlock    landlock", "check memcg       memcg", "mount -t cgroup2 cgroup2 /sys/fs/cgroup"):
            self.assertIn(needle, text)


def run_function(name, prelude="", args=""):
    script = f"{prelude}\n{function(name)}\n{name} {args}\n"
    return subprocess.run(["sh", "-c", script], capture_output=True, text=True)


class PlatformAssertions(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def log(self, text):
        (self.dir / "dmesg").write_text(text)
        return f'dmesg() {{ cat "{self.dir}/dmesg"; }}'

    def test_preempt_lazy_passes(self):
        self.assertEqual(run_function("preempt_lazy", self.log("[0.1] Dynamic Preempt: lazy\n")).returncode, 0)

    def test_preempt_full_fails(self):
        self.assertNotEqual(run_function("preempt_lazy", self.log("[0.1] Dynamic Preempt: full\n")).returncode, 0)

    def test_preempt_line_missing_fails(self):
        self.assertNotEqual(run_function("preempt_lazy", self.log("[0.1] nothing\n")).returncode, 0)

    def vm(self, bits, compat):
        (self.dir / "mmap_rnd_bits").write_text(f"{bits}\n")
        (self.dir / "mmap_rnd_compat_bits").write_text(f"{compat}\n")
        return f'vm_dir="{self.dir}"'

    def test_aslr_bits_pass(self):
        self.assertEqual(run_function("aslr_bits", self.vm(32, 16)).returncode, 0)

    def test_aslr_bits_too_low_fail(self):
        self.assertNotEqual(run_function("aslr_bits", self.vm(28, 8)).returncode, 0)

    def groups(self, *types, log="[0.1] nothing\n"):
        for i, kind in enumerate(types):
            (self.dir / "groups" / str(i)).mkdir(parents=True)
            (self.dir / "groups" / str(i) / "type").write_text(f"{kind}\n")
        (self.dir / "groups").mkdir(exist_ok=True)
        return f'iommu_groups="{self.dir}/groups"\n' + self.log(log)

    def test_lazy_domains_pass(self):
        self.assertEqual(run_function("iommu_domains", self.groups("DMA-FQ", "DMA-FQ")).returncode, 0)

    def test_a_strict_domain_fails(self):
        self.assertNotEqual(run_function("iommu_domains", self.groups("DMA-FQ", "DMA")).returncode, 0)

    def test_no_iommu_group_fails(self):
        self.assertNotEqual(run_function("iommu_domains", self.groups()).returncode, 0)

    def test_an_identity_domain_fails(self):
        self.assertNotEqual(run_function("iommu_domains", self.groups("identity")).returncode, 0)

    def test_strict_domains_forced_by_a_virtual_iommu_pass(self):
        log = "[0.2] AMD-Vi: Using strict mode due to virtualization\n"
        self.assertEqual(run_function("iommu_domains", self.groups("DMA", "DMA", log=log)).returncode, 0)

    def test_an_identity_domain_fails_even_on_a_virtual_iommu(self):
        log = "[0.2] AMD-Vi: Using strict mode due to virtualization\n"
        self.assertNotEqual(run_function("iommu_domains", self.groups("DMA", "identity", log=log)).returncode, 0)

    def config(self, text):
        with gzip.open(self.dir / "config.gz", "wt") as handle:
            handle.write(text)
        return f'kernel_config="{self.dir}/config.gz"'

    def test_module_or_builtin_passes(self):
        prelude = self.config("CONFIG_WIREGUARD=m\nCONFIG_KVM_AMD=y\n")
        self.assertEqual(run_function("kconfig_enabled", prelude, "WIREGUARD KVM_AMD").returncode, 0)

    def test_a_missing_option_fails_and_is_named(self):
        prelude = self.config("CONFIG_WIREGUARD=m\n# CONFIG_UDMABUF is not set\n")
        result = run_function("kconfig_enabled", prelude, "WIREGUARD UDMABUF")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("UDMABUF", result.stdout)

    def cmdline(self, text):
        (self.dir / "cmdline").write_text(text + "\n")
        return f'cmdline_file="{self.dir}/cmdline"\n' + function("not_on_cmdline")

    def lockdown(self, cmdline):
        (self.dir / "lockdown").write_text("none [integrity] confidentiality\n")
        return self.cmdline(cmdline) + f'\nlockdown_file="{self.dir}/lockdown"'

    def test_forced_lockdown_passes_without_the_parameter(self):
        self.assertEqual(run_function("lockdown_forced", self.lockdown("intel_iommu=on")).returncode, 0)

    def test_lockdown_from_the_command_line_does_not_count(self):
        self.assertNotEqual(run_function("lockdown_forced", self.lockdown("lockdown=integrity")).returncode, 0)

    def test_init_on_free_from_the_build(self):
        log = self.log("mem auto-init: stack:all(zero), heap alloc:on, heap free:on\n")
        self.assertEqual(run_function("init_on_free_built_in", self.cmdline("x=1") + "\n" + log).returncode, 0)

    def test_init_on_free_off_fails(self):
        log = self.log("mem auto-init: stack:all(zero), heap alloc:on, heap free:off\n")
        self.assertNotEqual(run_function("init_on_free_built_in", self.cmdline("x=1") + "\n" + log).returncode, 0)

    def test_init_on_free_from_the_command_line_does_not_count(self):
        log = self.log("mem auto-init: stack:all(zero), heap alloc:on, heap free:on\n")
        prelude = self.cmdline("init_on_free=1") + "\n" + log
        self.assertNotEqual(run_function("init_on_free_built_in", prelude).returncode, 0)

    def maps(self, text):
        (self.dir / "maps").write_text(text)
        return self.cmdline("x=1") + f'\nmaps_file="{self.dir}/maps"'

    def test_a_vsyscall_page_fails(self):
        prelude = self.maps("ffffffffff600000-ffffffffff601000 --xp 00000000 00:00 0 [vsyscall]\n")
        self.assertNotEqual(run_function("vsyscall_none", prelude).returncode, 0)

    def test_no_vsyscall_page_passes(self):
        prelude = self.maps("7ffd1000-7ffd3000 r-xp 00000000 00:00 0 [vdso]\n")
        self.assertEqual(run_function("vsyscall_none", prelude).returncode, 0)

    def test_debugfs_that_mounts_fails(self):
        prelude = self.cmdline("x=1") + f'\nmount() {{ return 0; }}\ndebugfs_dir="{self.dir}/debugfs"'
        self.assertNotEqual(run_function("debugfs_off", prelude).returncode, 0)

    def test_debugfs_refused_passes(self):
        prelude = self.cmdline("x=1") + f'\nmount() {{ return 19; }}\ndebugfs_dir="{self.dir}/debugfs"'
        self.assertEqual(run_function("debugfs_off", prelude).returncode, 0)

    def keyring(self, count):
        ids = " ".join(str(100 + i) for i in range(count))
        return f'keyctl() {{ case $1 in rlist) echo "{ids}" ;; list) echo "{count} keys" ;; esac; }}'

    def test_the_expected_number_of_builtin_keys_passes(self):
        self.assertEqual(run_function("builtin_exact", self.keyring(2) + "\nbuiltin_expected=2").returncode, 0)

    def test_an_extra_builtin_key_fails(self):
        self.assertNotEqual(run_function("builtin_exact", self.keyring(3) + "\nbuiltin_expected=2").returncode, 0)

    def test_a_missing_expectation_fails(self):
        self.assertNotEqual(run_function("builtin_exact", self.keyring(2) + "\nbuiltin_expected=").returncode, 0)

    def test_an_unreadable_keyring_fails(self):
        prelude = "keyctl() { echo 'keyctl: Required key not available' >&2; return 1; }\nbuiltin_expected=0"
        self.assertNotEqual(run_function("builtin_exact", prelude).returncode, 0)

    def test_init_runs_the_platform_checks(self):
        text = INIT.read_text()
        for needle in ("check preempt", "check aslr", "check iommu", "check mesh-platform",
                       "check lockdown    lockdown_forced", "check init-on-free", "check vsyscall", "check debugfs",
                       "check builtin-set builtin_exact"):
            self.assertIn(needle, text)


if __name__ == "__main__":
    unittest.main()
