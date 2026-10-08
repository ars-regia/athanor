"""Unit tests of the licence check in scripts/verify.py
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

OWN = verify.OWN_LICENCE
CRATE = '[package]\nname = "x"\nversion = "1.0.0"\nlicense = "%s"\n'
UPSTREAM = sorted(verify.UPSTREAM_SPECS)[0]


class SpdxTest(unittest.TestCase):
    def test_known_expressions_pass(self):
        for e in ("MIT", "MIT OR Apache-2.0", "(MIT OR Apache-2.0) AND ISC",
                  "GPL-2.0-only WITH Linux-syscall-note"):
            self.assertIsNone(verify.spdx_problem(e), e)

    def test_unknown_or_malformed_expressions_fail(self):
        for e in ("BSD", "mit", "GPLv3", "GPL-3.0", "MIT AND AND", "MIT OR", "(MIT",
                  "MIT)", "", "MIT WITH Nope", "MIT Apache-2.0"):
            self.assertIsNotNone(verify.spdx_problem(e), e)


class LicenceExceptionTest(unittest.TestCase):
    def test_the_greeter_is_declared_gpl_3_only_and_nothing_else_is(self):
        self.assertEqual(verify.licence_problems(), [])
        for path, (lic, reason) in verify.OWN_LICENCE_EXCEPTIONS.items():
            self.assertNotEqual(lic, verify.OWN_LICENCE, path)
            self.assertTrue(reason, path)

    def test_an_exception_must_match_exactly(self):
        crate, spec = sorted(verify.OWN_LICENCE_EXCEPTIONS)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            subprocess.run(["git", "-C", tmp, "init", "-q"], check=True)
            (root / "LICENSE").write_text("text")
            for rel, body in ((crate, CRATE % OWN), (spec, f"License: {OWN}\n")):
                (root / rel).parent.mkdir(parents=True, exist_ok=True)
                (root / rel).write_text(body)
            self.assertEqual(len(verify.licence_problems(root)), 2)


class LicenceProblemsTest(unittest.TestCase):
    def repo(self, files):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        subprocess.run(["git", "-C", tmp.name, "init", "-q"], check=True)
        files = {"LICENSE": "text", **files}
        for name, body in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(body)
        return root

    def test_a_clean_tree_has_no_problem(self):
        root = self.repo({"forge/tools/t/Cargo.toml": CRATE % OWN,
                          "a/own.spec": f"License: {OWN}\n"})
        self.assertEqual(verify.licence_problems(root), [])

    def test_a_crate_outside_system_and_forge_specs_is_checked(self):
        root = self.repo({"forge/tools/t/Cargo.toml": CRATE % "MIT"})
        self.assertEqual(len(verify.licence_problems(root)), 1)

    def test_an_unreadable_manifest_is_a_problem(self):
        root = self.repo({"system/x/Cargo.toml": "[package\nlicense = 1"})
        problems = verify.licence_problems(root)
        self.assertEqual(len(problems), 1)
        self.assertIn("unreadable manifest", problems[0])

    def test_an_untracked_spec_is_checked(self):
        root = self.repo({"new.spec": "License: MIT\n"})
        self.assertEqual(len(verify.licence_problems(root)), 1)

    def test_upstream_exemption_is_by_path_not_stem(self):
        stem = pathlib.PurePosixPath(UPSTREAM).name
        root = self.repo({UPSTREAM: "License: MIT\n", f"other/{stem}": "License: MIT\n"})
        problems = verify.licence_problems(root)
        self.assertEqual(len(problems), 1)
        self.assertTrue(problems[0].startswith(f"other/{stem}"), problems[0])

    def test_an_unknown_upstream_identifier_is_a_problem(self):
        root = self.repo({UPSTREAM: "License: BSD\n"})
        self.assertEqual(len(verify.licence_problems(root)), 1)

    def test_an_nfpm_license_in_flake_nix_is_checked(self):
        root = self.repo({"flake.nix": 'license: "MIT"\n'})
        problems = verify.licence_problems(root)
        self.assertEqual(len(problems), 1)
        self.assertIn("flake.nix:1", problems[0])
        root = self.repo({"flake.nix": f'license: "{OWN}"\n'})
        self.assertEqual(verify.licence_problems(root), [])

    def test_a_missing_root_licence_is_a_problem(self):
        root = self.repo({})
        (root / "LICENSE").unlink()
        self.assertEqual(len(verify.licence_problems(root)), 1)

    def test_a_directory_that_is_not_a_repository_is_a_problem_not_a_crash(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = verify.licence_problems(tmp)
        self.assertEqual(len(problems), 1)
        self.assertIn("cannot list", problems[0])


if __name__ == "__main__":
    unittest.main()
