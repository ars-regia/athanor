"""The LSM and memory controller assertions of forge/specs/azoth/boot/init, run against
files the test writes (python3 -B -m unittest discover -s forge/specs/azoth/tests)."""

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


if __name__ == "__main__":
    unittest.main()
