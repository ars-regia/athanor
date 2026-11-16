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
            "system/athanor-unit/src/lib.rs": READS,
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


if __name__ == "__main__":
    unittest.main()
