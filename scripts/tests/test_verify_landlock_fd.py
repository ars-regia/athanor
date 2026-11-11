"""Unit tests of the landlock-fd check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

GRANT = "ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, writes))?;\n"
READ_GRANT = """use landlock::{
    AccessFs, PathBeneath, PathFd, Ruleset,
};

    for path in read {
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(path)?,
            AccessFs::from_read(ABI::V1),
        ))?;
    }
"""
LANDLOCK = '[dependencies]\nlandlock = { workspace = true }\n'


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.landlock_fd_problems(root)


class LandlockFdTest(unittest.TestCase):
    def test_the_pending_rulesets_pass(self):
        self.assertEqual(problems({name: GRANT for name in verify.LANDLOCK_PATH_PENDING}), [])

    def test_the_shared_sandbox_reads_through_path_fd_and_never_writes(self):
        self.assertEqual(problems({verify.LANDLOCK_SANDBOX: READ_GRANT}), [])
        found = problems({verify.LANDLOCK_SANDBOX: READ_GRANT + GRANT})
        self.assertEqual(len(found), 1, found)
        self.assertIn("sandbox.rs:11: PathFd other than the parent of a read grant", found[0])

    def test_the_real_sandbox_passes(self):
        real = (pathlib.Path(__file__).resolve().parents[2] / verify.LANDLOCK_SANDBOX).read_text()
        self.assertEqual(problems({verify.LANDLOCK_SANDBOX: real}), [])

    def test_a_restrict_that_takes_any_descriptor_is_refused(self):
        real = (pathlib.Path(__file__).resolve().parents[2] / verify.LANDLOCK_SANDBOX).read_text()
        code = real.replace("write: &[Checked])", "write: &[impl AsFd])", 1)
        self.assertNotEqual(code, real)
        found = problems({verify.LANDLOCK_SANDBOX: code})
        self.assertTrue(any("restrict takes `write: &[impl AsFd]`" in p for p in found), found)

    def test_code_around_the_tests_module_is_still_read(self):
        real = (pathlib.Path(__file__).resolve().parents[2] / verify.LANDLOCK_SANDBOX).read_text()
        for code in ("// #[cfg(test)]\n" + real.replace("write: &[Checked])", "write: &[impl AsFd])", 1),
                     "#[cfg(test)]\nuse std::fs::File;\n" + real.replace("write: &[Checked])", "write: &[impl AsFd])", 1),
                     real + "\npub fn widen(write: &[impl AsFd]) {}\n"):
            found = problems({verify.LANDLOCK_SANDBOX: code})
            self.assertTrue(any("&[impl AsFd]" in p for p in found), found)

    def test_a_grant_opened_in_the_sandbox_is_refused(self):
        for grant in ("PathBeneath::new(std::fs::File::open(p)?, writes)",
                      "landlock::path_beneath_rules(paths, writes)"):
            found = problems({verify.LANDLOCK_SANDBOX: READ_GRANT + f"    ruleset.add_rules({grant})?;\n"})
            self.assertTrue(found, grant)

    def test_a_read_grant_widened_with_writes_is_refused(self):
        code = READ_GRANT.replace("AccessFs::from_read(ABI::V1),",
                                  "AccessFs::from_read(ABI::V1) | AccessFs::from_write(ABI::V3),")
        found = problems({verify.LANDLOCK_SANDBOX: code})
        self.assertEqual(len(found), 1, found)
        self.assertIn("PathFd other than the parent of a read grant", found[0])

    def test_a_crate_outside_system_and_forge_specs_is_scanned(self):
        found = problems({"fuzz/Cargo.toml": LANDLOCK,
                          "forge/tools/x/src/main.rs": "use landlock::PathFd;\n"})
        self.assertEqual(len(found), 2, found)

    def test_a_git_ignored_tree_is_not_scanned(self):
        self.assertEqual(problems({".gitignore": ".scratch/\n",
                                   ".scratch/x/Cargo.toml": LANDLOCK,
                                   ".scratch/x/src/main.rs": "use landlock::PathFd;\n"}), [])

    def test_an_alias_in_the_shared_sandbox_is_refused(self):
        code = "use landlock::{PathFd as Fd, AccessFs};\n"
        self.assertEqual(len(problems({verify.LANDLOCK_SANDBOX: code})), 1)

    def test_only_athanor_unit_and_the_pending_crates_depend_on_landlock(self):
        allowed = {name: LANDLOCK for name in (verify.LANDLOCK_CRATE, *verify.LANDLOCK_CRATES_PENDING)}
        self.assertEqual(problems(allowed), [])
        found = problems({"forge/specs/athanor-dock/athanor-dock-1.0.0/Cargo.toml": LANDLOCK,
                          "system/athanor-idle/Cargo.toml":
                              '[target.\'cfg(unix)\'.dev-dependencies]\nll = { package = "landlock" }\n'})
        self.assertEqual(len(found), 2, found)
        self.assertTrue(any("athanor-dock-1.0.0/Cargo.toml: [dependencies] names the landlock crate"
                            in problem for problem in found), found)
        self.assertTrue(any("[target.cfg(unix).dev-dependencies]" in problem for problem in found), found)

    def test_path_fd_anywhere_else_is_reported(self):
        found = problems({"forge/specs/athanor-dock/src/start.rs": GRANT,
                          "system/athanor-idle/src/confine.rs": "use landlock::PathFd;\n"})
        self.assertEqual(len(found), 2, found)
        self.assertTrue(all("PathFd grants by path" in problem for problem in found))

    def test_an_alias_is_caught_where_it_is_imported(self):
        code = "use landlock::{PathFd as Fd};\n\nlet dir = Fd::new(path)?;\n"
        found = problems({"system/athanor-idle/src/confine.rs": code})
        self.assertEqual(len(found), 1, found)
        self.assertIn("confine.rs:1:", found[0])

    def test_text_after_a_string_holding_two_slashes_is_still_read(self):
        code = 'let url = "https://x"; let dir = PathFd::new(path)?;\n'
        self.assertEqual(len(problems({"system/athanor-idle/src/confine.rs": code})), 1)

    def test_a_descriptor_grant_passes(self):
        code = "ruleset = ruleset.add_rule(PathBeneath::new(dir.as_fd(), writes))?;\n"
        self.assertEqual(problems({"system/athanor-unit/src/dirs.rs": code}), [])


if __name__ == "__main__":
    unittest.main()
