"""Unit tests of scripts/verify.py sysctl
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

TWEAKS = "forge/specs/athanor-system-tweaks/SOURCES/usr/lib/sysctl.d"


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for relative in (
            verify.PROFILE_MANIFEST,
            verify.PROFILE_SYSCTL,
            "forge/specs/athanor-base-config/SOURCES/usr/lib/sysusers.d/10-athanor-hw-groups.conf",
        ):
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / relative, target)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, relative, text):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


class Sysctl(Fixture):
    def test_the_repository_has_no_problem(self):
        self.assertEqual(verify.sysctl_problems(self.root), [])
        self.assertEqual(verify.sysctl_problems(), [])

    def test_another_file_setting_a_base_sysctl_is_named(self):
        # The lines of the 99-bore.conf that block P3 removed.
        self.write(
            f"{TWEAKS}/99-bore.conf",
            "# tuning\nkernel.sched_bore = 1\nkernel.kptr_restrict = 2\n"
            "net.core.default_qdisc = fq\n",
        )
        found = verify.sysctl_problems(self.root)
        self.assertEqual(len(found), 2, found)
        self.assertTrue(
            any("99-bore.conf:3: sets kernel.kptr_restrict" in p for p in found), found
        )
        self.assertTrue(
            any("99-bore.conf:4: sets net.core.default_qdisc" in p for p in found),
            found,
        )

    def test_slash_and_dash_spellings_are_the_same_sysctl(self):
        self.write("system/extra/sysctl.d/50-x.conf", "-kernel/yama/ptrace_scope=0\n")
        self.assertTrue(
            any(
                "kernel.yama.ptrace_scope" in p
                for p in verify.sysctl_problems(self.root)
            )
        )

    def test_comments_and_other_sysctls_pass(self):
        self.write(
            f"{TWEAKS}/99-network-security.conf",
            "# kernel.kptr_restrict = 0\n; fs.suid_dumpable = 1\nnet.ipv4.tcp_syncookies = 1\n",
        )
        self.assertEqual(verify.sysctl_problems(self.root), [])

    def test_an_unreadable_manifest_is_reported(self):
        (self.root / verify.PROFILE_MANIFEST).unlink()
        self.assertTrue(
            any("cannot read" in p for p in verify.sysctl_problems(self.root))
        )


if __name__ == "__main__":
    unittest.main()
