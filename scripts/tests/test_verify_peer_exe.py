"""Unit tests of the peer-exe check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

READS = "fn main() { let _ = std::fs::read_link(format!(\"/proc/{pid}/exe\")); }\n"
CALLS = "fn main() { athanor_unit::peer::admit_user_unit(&fd, \"a.service\")?; }\n"
PLAIN = "fn main() {}\n"
UNIT_LIB = '[package]\nname = "athanor-unit"\n'


def manifest(name, *deps):
    lines = [f'[package]\nname = "{name}"\n[dependencies]'] + [f'{d} = {{ path = "../{d}" }}' for d in deps]
    return "\n".join(lines) + "\n"


def problems(files, units):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.peer_admission_problems(verify.user_unit_texts(root, units), verify.crate_programs(root))


def unit(program, prop="PrivateNetwork=yes", key="ExecStart"):
    return f"[Service]\n{prop}\n{key}=/usr/bin/{program}\n"


class PeerExeTest(unittest.TestCase):
    def test_a_namespaced_unit_whose_program_calls_admit_user_unit_is_reported(self):
        found = problems({"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": CALLS}, {"d.service": unit("d")})
        self.assertEqual(len(found), 1)
        self.assertIn("system/d/src/main.rs:1", found[0])

    def test_a_unit_without_namespacing_passes(self):
        files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": READS}
        self.assertEqual(problems(files, {"d.service": unit("d", prop="NoNewPrivileges=yes")}), [])

    def test_a_read_in_a_library_it_depends_on_counts(self):
        files = {
            "system/d/Cargo.toml": manifest("d", "lib"),
            "system/d/src/main.rs": PLAIN,
            "system/lib/Cargo.toml": manifest("lib"),
            "system/lib/src/lib.rs": READS,
        }
        found = problems(files, {"d.service": unit("d")})
        self.assertEqual(len(found), 1)
        self.assertIn("system/lib/src/lib.rs:1", found[0])

    def test_depending_on_athanor_unit_without_the_call_passes(self):
        files = {
            "system/d/Cargo.toml": manifest("d", "athanor-unit"),
            "system/d/src/main.rs": PLAIN,
            "system/athanor-unit/Cargo.toml": UNIT_LIB,
            "system/athanor-unit/src/peer.rs": READS,
        }
        self.assertEqual(problems(files, {"d.service": unit("d")}), [])

    def test_exec_start_pre_and_post_count(self):
        files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": CALLS}
        for key in ("ExecStartPre", "ExecStartPost"):
            self.assertEqual(len(problems(files, {"d.service": unit("d", key=key)})), 1, key)

    def test_proc_subset_counts_unless_all(self):
        files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": CALLS}
        self.assertEqual(len(problems(files, {"d.service": unit("d", prop="ProcSubset=pid")})), 1)
        self.assertEqual(problems(files, {"d.service": unit("d", prop="ProcSubset=all")}), [])

    def test_an_unplaced_program_and_a_wrapper_of_another_unit_are_reported(self):
        self.assertEqual(len(problems({}, {"d.service": unit("mystery")})), 1)
        self.assertEqual(len(problems({}, {"d.service": unit("bash -c x")})), 1)
        self.assertEqual(problems({}, {"athanor-skel-sync.service": unit("bash -c x")}), [])

    def test_a_drop_in_adds_namespacing_to_a_user_unit_only(self):
        files = {
            "system/d/Cargo.toml": manifest("d"),
            "system/d/src/main.rs": CALLS,
            "forge/specs/p/SOURCES/usr/lib/systemd/user/d.service.d/10.conf": "[Service]\nPrivateIPC=yes\n",
            "forge/specs/p/SOURCES/usr/lib/systemd/system/s.service.d/10.conf": "[Service]\nPrivateIPC=yes\nExecStart=/usr/bin/mystery\n",
        }
        found = problems(files, {"d.service": unit("d", prop="NoNewPrivileges=yes")})
        self.assertEqual(len(found), 1)
        self.assertTrue(found[0].startswith("d.service:"))

    def test_an_import_under_another_name_and_a_joined_exe_path_count(self):
        aliased = "use athanor_unit::peer::admit_user_unit as admit;\nfn main() { admit(&fd, \"a\")?; }\n"
        joined = 'fn main() { let _ = std::fs::read_link(Path::new("/proc").join(pid).join("exe")); }\n'
        for source in (aliased, joined):
            files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": source}
            self.assertEqual(len(problems(files, {"d.service": unit("d")})), 1, source)

    def test_the_cgroup_admission_does_not_count(self):
        files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": "fn main() { athanor_unit::peer::admit_user_unit_cgroup(&fd, \"a\")?; }\n"}
        self.assertEqual(problems(files, {"d.service": unit("d")}), [])

    def test_every_exec_line_counts(self):
        files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": CALLS}
        for key in ("ExecCondition", "ExecReload", "ExecStop", "ExecStopPost"):
            self.assertEqual(len(problems(files, {"d.service": unit("d", key=key)})), 1, key)

    def test_systemd_false_spellings_leave_the_property_off(self):
        files = {"system/d/Cargo.toml": manifest("d"), "system/d/src/main.rs": CALLS}
        for value in ("off", "n", "f", "no", "false", "0"):
            self.assertEqual(problems(files, {"d.service": unit("d", prop=f"PrivateNetwork={value}")}), [], value)
        self.assertEqual(len(problems(files, {"d.service": unit("d", prop="PrivateBPF=yes")})), 1)

    def test_a_drop_in_outside_systemd_user_is_not_merged(self):
        files = {
            "system/d/Cargo.toml": manifest("d"),
            "system/d/src/main.rs": CALLS,
            "forge/specs/p/SOURCES/usr/share/foo/user/d.service.d/10.conf": "[Service]\nPrivateIPC=yes\n",
        }
        self.assertEqual(problems(files, {"d.service": unit("d", prop="NoNewPrivileges=yes")}), [])

    def test_only_peer_rs_of_athanor_unit_is_passed_over(self):
        files = {
            "system/d/Cargo.toml": manifest("d", "athanor-unit"),
            "system/d/src/main.rs": PLAIN,
            "system/athanor-unit/Cargo.toml": UNIT_LIB,
            "system/athanor-unit/src/peer.rs": READS,
            "system/athanor-unit/src/other.rs": READS,
        }
        found = problems(files, {"d.service": unit("d")})
        self.assertEqual(len(found), 1)
        self.assertIn("system/athanor-unit/src/other.rs:1", found[0])


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
