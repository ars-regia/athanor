"""Unit tests of scripts/verify.py udev-groups
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

RULES = "forge/specs/athanor-desktop-ui/SOURCES/etc/udev/rules.d"


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


class UdevGroups(Fixture):
    def test_the_repository_has_no_problem(self):
        self.assertEqual(verify.udev_group_problems(self.root), [])
        self.assertEqual(verify.udev_group_problems(), [])

    def test_an_undeclared_group_is_named(self):
        # The rule that block P3 removed.
        self.write(
            f"{RULES}/99-ddcutil-i2c.rules",
            'KERNEL=="i2c-[0-9]*", GROUP="i2c", MODE="0660"\n',
        )
        found = verify.udev_group_problems(self.root)
        self.assertEqual(
            found,
            [
                f'{RULES}/99-ddcutil-i2c.rules:1: GROUP="i2c" is declared by no sysusers.d file'
            ],
        )

    def test_a_declared_group_and_a_comment_pass(self):
        self.write(
            f"{RULES}/70-x.rules",
            '# GROUP="nobody-knows"\nKERNEL=="kvm", GROUP:="kvm"\n',
        )
        self.write("forge/specs/x/x.sysusers", "g lp 7 -\n")
        self.write(f"{RULES}/71-x.rules", 'KERNEL=="lp[0-9]*", GROUP="lp"\n')
        self.assertEqual(verify.udev_group_problems(self.root), [])


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
