"""Unit tests of athanor-profile-check against a fake root.

Run: python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests -v
"""

import gzip
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from importlib.machinery import SourceFileLoader

PACKAGE = pathlib.Path(__file__).resolve().parents[1]
CHECKER = PACKAGE / "SOURCES" / "usr" / "bin" / "athanor-profile-check"
PROFILES = PACKAGE / "SOURCES" / "usr" / "share" / "athanor" / "kernel-profile"


def load_checker():
    loader = SourceFileLoader("athanor_profile_check", str(CHECKER))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
    loader.exec_module(module)
    return module


check = load_checker()


class FakeSystem:
    """A root directory holding the files athanor-profile-check reads."""

    def __init__(self, root: pathlib.Path) -> None:
        self.root = root
        self.write("proc/cmdline", "BOOT_IMAGE=/vmlinuz root=UUID=1 rw\n")
        self.config([])
        self.write("sys/kernel/security/lockdown", "none [integrity] confidentiality\n")
        self.write(
            "sys/kernel/security/lsm", "lockdown,capability,yama,selinux,bpf,landlock,ipe,ima,evm"
        )

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def config(self, lines: list) -> None:
        path = self.root / "proc" / "config.gz"
        path.parent.mkdir(parents=True, exist_ok=True)
        with gzip.open(path, "wt") as handle:
            handle.write("\n".join(lines) + "\n")

    def sysctl(self, name: str, value) -> None:
        self.write("proc/sys/" + name.replace(".", "/"), f"{value}\n")


def profile(directory: pathlib.Path, name: str, settings: dict) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    full = {"kconfig": {}, "sysctl": {}, "runtime": {}}
    for kind, values in settings.items():
        full[kind] = {key: {"value": value, "decision": "test"} for key, value in values.items()}
    document = {"schema": 1, "combination": name, "roles": [], "settings": full}
    (directory / f"{name}.json").write_text(json.dumps(document))


def run(root: pathlib.Path, profiles: pathlib.Path, *extra: str) -> tuple[int, str]:
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = check.main(["--root", str(root), "--profiles", str(profiles), *extra])
    return code, out.getvalue() + err.getvalue()


class Checker(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.system = FakeSystem(pathlib.Path(self.tmp.name) / "root")
        self.profiles = pathlib.Path(self.tmp.name) / "profiles"

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_every_setting_holds(self) -> None:
        self.system.config(["CONFIG_RUST=y", 'CONFIG_DEFAULT_TCP_CONG="bbr3"'])
        self.system.sysctl("kernel.kptr_restrict", 2)
        profile(
            self.profiles,
            "base",
            {
                "kconfig": {"CONFIG_RUST": "y", "CONFIG_DEFAULT_TCP_CONG": '"bbr3"'},
                "sysctl": {"kernel.kptr_restrict": "2"},
                "runtime": {"lockdown": "integrity", "lsm": ["lockdown", "yama", "ipe"]},
            },
        )
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 0, text)
        self.assertIn("base: 5/5 settings hold", text)

    def test_sysctl_drift_is_reported(self) -> None:
        self.system.sysctl("kernel.kptr_restrict", 0)
        profile(self.profiles, "base", {"sysctl": {"kernel.kptr_restrict": "2"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 1, text)
        self.assertIn("DRIFT sysctl.kernel.kptr_restrict: expected '2', found '0'", text)
        self.assertIn("base: 0/1 settings hold", text)

    def test_option_that_is_not_set_reads_as_n(self) -> None:
        self.system.config(["# CONFIG_LOCK_DOWN_KERNEL_FORCE_INTEGRITY is not set"])
        profile(self.profiles, "base", {"kconfig": {"CONFIG_LOCK_DOWN_KERNEL_FORCE_INTEGRITY": "y"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 1, text)
        self.assertIn("found 'n'", text)

    def test_missing_lsm_is_drift_and_extra_lsms_are_not(self) -> None:
        profile(self.profiles, "base", {"runtime": {"lsm": ["lockdown", "tomoyo"]}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 1, text)
        self.assertIn("DRIFT runtime.lsm", text)

    def test_quiet_prints_nothing(self) -> None:
        self.system.sysctl("kernel.kptr_restrict", 0)
        profile(self.profiles, "base", {"sysctl": {"kernel.kptr_restrict": "2"}})
        code, text = run(self.system.root, self.profiles, "--quiet")
        self.assertEqual(code, 1)
        self.assertEqual(text, "")

    def test_roles_on_the_command_line_select_their_profile(self) -> None:
        self.system.write("proc/cmdline", "root=UUID=1 athanor.role=laptop athanor.role=desktop\n")
        profile(self.profiles, "desktop+laptop", {"runtime": {"lockdown": "integrity"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 0, text)
        self.assertIn("desktop+laptop: 1/1 settings hold", text)

    def test_missing_profile_cannot_be_checked(self) -> None:
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_repository_base_profile_holds_on_a_matching_system(self) -> None:
        """The generated base profile holds on a system built from it: the generator and
        the checker agree on setting names and value formats."""
        settings = json.loads((PROFILES / "base.json").read_text())["settings"]
        self.system.config([f"{name}={s['value']}" for name, s in settings["kconfig"].items()])
        for name, s in settings["sysctl"].items():
            self.system.sysctl(name, s["value"])
        arguments = " ".join(f"{name}={s['value']}" for name, s in settings["cmdline"].items())
        self.system.write("proc/cmdline", f"BOOT_IMAGE=/vmlinuz root=UUID=1 {arguments} rw\n")
        code, text = run(self.system.root, PROFILES)
        self.assertEqual(code, 0, text)
        self.assertIn("base: 119/119 settings hold", text)

    def test_cmdline_drift_is_reported(self) -> None:
        self.system.write("proc/cmdline", "BOOT_IMAGE=/vmlinuz lockdown=none vsyscall=none rw\n")
        profile(
            self.profiles,
            "base",
            {"cmdline": {"lockdown": "integrity", "vsyscall": "none", "init_on_free": "1"}},
        )
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 1, text)
        self.assertIn("DRIFT cmdline.lockdown: expected 'integrity', found 'none'", text)
        self.assertIn("DRIFT cmdline.init_on_free: expected '1', found None", text)
        self.assertIn("base: 1/3 settings hold", text)

    def test_cmdline_repeated_parameter_keeps_its_last_value(self) -> None:
        self.system.write("proc/cmdline", "debugfs=on quiet debugfs=off\n")
        profile(self.profiles, "base", {"cmdline": {"debugfs": "off"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 0, text)

    def test_malformed_profile_cannot_be_checked(self) -> None:
        """Profiles with wrong shapes (null settings, leaf strings) are unreadable."""
        # Test settings=null
        with self.subTest(malformation="settings is null"):
            doc = {"schema": 1, "combination": "base", "roles": [], "settings": None}
            (self.profiles / "base.json").parent.mkdir(parents=True, exist_ok=True)
            (self.profiles / "base.json").write_text(json.dumps(doc))
            code, text = run(self.system.root, self.profiles)
            self.assertEqual(code, 2, text)
            self.assertIn("cannot check the profile", text)

        # Test leaf string instead of {"value":..,"decision":..}
        with self.subTest(malformation="leaf is string not dict"):
            doc = {"schema": 1, "combination": "base", "roles": [], "settings": {"sysctl": {"kernel.kptr_restrict": "2"}}}
            (self.profiles / "base.json").write_text(json.dumps(doc))
            code, text = run(self.system.root, self.profiles)
            self.assertEqual(code, 2, text)
            self.assertIn("cannot check the profile", text)

    def test_unknown_setting_kind_cannot_be_checked(self) -> None:
        doc = {"schema": 1, "combination": "base", "roles": [], "settings": {"firmware": {"x": {"value": "1", "decision": "t"}}}}
        (self.profiles / "base.json").parent.mkdir(parents=True, exist_ok=True)
        (self.profiles / "base.json").write_text(json.dumps(doc))
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_wrong_schema_cannot_be_checked(self) -> None:
        doc = {"schema": 99, "combination": "base", "roles": [], "settings": {"sysctl": {"kernel.kptr_restrict": {"value": "2", "decision": "t"}}}}
        (self.profiles / "base.json").parent.mkdir(parents=True, exist_ok=True)
        (self.profiles / "base.json").write_text(json.dumps(doc))
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_empty_settings_cannot_be_checked(self) -> None:
        doc = {"schema": 1, "combination": "base", "roles": [], "settings": {}}
        (self.profiles / "base.json").parent.mkdir(parents=True, exist_ok=True)
        (self.profiles / "base.json").write_text(json.dumps(doc))
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_unknown_runtime_key_is_still_reported_as_drift(self) -> None:
        profile(self.profiles, "base", {"runtime": {"selinux-mode": "enforcing"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 1, text)
        self.assertIn("no reader for this setting", text)

    def test_invalid_json_cannot_be_checked(self) -> None:
        (self.profiles).mkdir(parents=True, exist_ok=True)
        (self.profiles / "base.json").write_text("{not json")
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_truncated_config_gz_cannot_be_checked(self) -> None:
        self.system.config(["CONFIG_RUST=y"])
        path = self.system.root / "proc" / "config.gz"
        path.write_bytes(path.read_bytes()[:10])
        profile(self.profiles, "base", {"kconfig": {"CONFIG_RUST": "y"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_non_gzip_config_cannot_be_checked(self) -> None:
        (self.system.root / "proc" / "config.gz").write_bytes(b"not a gzip file at all")
        profile(self.profiles, "base", {"kconfig": {"CONFIG_RUST": "y"}})
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("cannot check the profile", text)

    def test_traversal_role_name_cannot_be_checked(self) -> None:
        # A profile that would hold if the checker followed the traversal instead of
        # rejecting the role name outright.
        self.profiles.mkdir(parents=True, exist_ok=True)
        profile(self.profiles.parent, "x", {"runtime": {"lockdown": "integrity"}})
        self.system.write("proc/cmdline", "root=UUID=1 athanor.role=../x\n")
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("../x", text)

    def test_base_role_name_cannot_be_checked(self) -> None:
        # A "base" profile that would hold if the checker accepted "base" as a role name.
        profile(self.profiles, "base", {"runtime": {"lockdown": "integrity"}})
        self.system.write("proc/cmdline", "root=UUID=1 athanor.role=base\n")
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("base", text)

    def test_role_name_with_plus_cannot_be_checked(self) -> None:
        # A profile matching the raw cmdline value, which would hold if the checker did
        # not reject a single athanor.role= value that already contains "+".
        profile(self.profiles, "desktop+laptop", {"runtime": {"lockdown": "integrity"}})
        self.system.write("proc/cmdline", "root=UUID=1 athanor.role=desktop+laptop\n")
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("desktop+laptop", text)

    def test_missing_combination_names_it_unknown_or_rejected(self) -> None:
        self.system.write("proc/cmdline", "root=UUID=1 athanor.role=mesh\n")
        code, text = run(self.system.root, self.profiles)
        self.assertEqual(code, 2, text)
        self.assertIn("no profile", text)
        self.assertIn("unknown or rejected", text)

    def test_kinds_match_the_generator(self) -> None:
        sys.path.insert(0, str(PACKAGE))
        import kernel_profile as kp

        self.assertEqual(check.KINDS, kp.KINDS)


if __name__ == "__main__":
    unittest.main()
