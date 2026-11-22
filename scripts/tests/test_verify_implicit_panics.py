"""Unit tests of the implicit-panics check of `verify.py`: clippy's JSON is counted per crate once
per place and lint, a run that did not finish counts nothing, the budget only goes down and, without
clippy, fails closed on a dropped line, a member without one and a configuration that hides a
budget lint; clippy runs with the lints forced and without RUSTFLAGS, a build script may not set
them back, and no member names `clippy` or `debug_assertions` for a `cfg`
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

SHELLD = "path+file:///repo/forge/specs/athanor-shelld/athanor-shelld-1.0.0#athanor-shelld@1.0.0"
UNIT = "path+file:///repo/system/athanor-unit#1.0.0"
VENDORED = "path+file:///repo/system/athanor-sal/vendor/accesskit_atspi_common#0.21.0"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index#zbus@6.2.1"


def hit(package, lint, line, file="src/wire.rs"):
    return json.dumps({
        "reason": "compiler-message", "package_id": package,
        "message": {"code": {"code": lint}, "spans": [
            {"file_name": file, "line_start": line, "column_start": 5, "is_primary": True}]},
    })


def artifact(package):
    return json.dumps({"reason": "compiler-artifact", "package_id": package})


FINISHED = json.dumps({"reason": "build-finished", "success": True})


class CountsTest(unittest.TestCase):
    def test_each_place_and_lint_counts_once_per_crate(self):
        lines = [
            "   Compiling athanor-shelld",  # not JSON: cargo's own text
            hit(SHELLD, "clippy::indexing_slicing", 3),
            hit(SHELLD, "clippy::indexing_slicing", 3),  # the same module in a second target
            hit(SHELLD, "clippy::arithmetic_side_effects", 3),
            hit(SHELLD, "clippy::string_slice", 9),
            hit(SHELLD, "clippy::needless_return", 4),
            artifact(UNIT),
            hit(VENDORED, "clippy::indexing_slicing", 1),
            hit(REGISTRY, "clippy::indexing_slicing", 1),
            FINISHED,
        ]
        self.assertEqual(verify.implicit_panic_counts(lines), ({"athanor-shelld": 3, "athanor-unit": 0}, []))

    def test_a_build_script_that_sets_a_cfg_is_a_problem_unless_vendored(self):
        def executed(package, cfgs):
            return json.dumps({"reason": "build-script-executed", "package_id": package, "cfgs": cfgs})

        lines = [executed(SHELLD, ["not_clippy"]), executed(UNIT, []), executed(VENDORED, ["x"]),
                 executed(REGISTRY, ["y"]), artifact(UNIT), FINISHED]
        counts, problems = verify.implicit_panic_counts(lines)
        self.assertEqual([p.split(":")[0] for p in problems], ["athanor-shelld"])

    def test_a_run_that_failed_or_never_finished_is_a_problem(self):
        failed = json.dumps({"reason": "build-finished", "success": False})
        self.assertEqual(len(verify.implicit_panic_counts([artifact(UNIT), FINISHED, failed])[1]), 1)
        self.assertEqual(len(verify.implicit_panic_counts([artifact(UNIT)])[1]), 1)

    def test_a_build_script_that_sets_a_dropped_variable_is_a_problem(self):
        def executed(package, *env):
            return json.dumps({"reason": "build-script-executed", "package_id": package,
                               "env": [[key, "x"] for key in env]})

        lines = [executed(SHELLD, "CLIPPY_CONF_DIR", "OTHER"), executed(VENDORED, "CLIPPY_ARGS"),
                 executed(UNIT, "CARGO_ENCODED_RUSTFLAGS"), executed(REGISTRY, "OTHER"),
                 executed(REGISTRY), artifact(UNIT), FINISHED]
        counts, problems = verify.implicit_panic_counts(lines)
        self.assertEqual(counts, {"athanor-unit": 0})
        self.assertEqual([p.split(":")[0] for p in problems],
                         ["athanor-shelld", "accesskit_atspi_common", "athanor-unit"])
        self.assertIn("CLIPPY_CONF_DIR", problems[0])
        self.assertNotIn("OTHER", problems[0])

    def test_package_crate_reads_both_package_id_forms(self):
        self.assertEqual(verify.package_crate(SHELLD), "athanor-shelld")
        self.assertEqual(verify.package_crate(UNIT), "athanor-unit")


class BudgetTest(unittest.TestCase):
    def test_a_count_over_or_under_its_line_and_missing_or_unbuilt_crates_fail(self):
        problems, notes = verify.implicit_panic_problems(
            {"a": 3, "b": 1, "c": 0, "new": 2}, {"a": 2, "b": 4, "c": 0, "gone": 1})
        self.assertEqual([p.split(":")[0] for p in problems], ["a", "b", "gone", "new"])
        self.assertIn("lower its line in scripts/ci/implicit-panics.txt to 1", problems[1])
        self.assertEqual(notes, [])

    def test_the_budget_only_goes_down_against_the_base(self):
        raised = verify.implicit_panic_raises({"a": 3, "b": 1, "new": 9}, {"a": 2, "b": 4})
        self.assertEqual([p.split(":")[0] for p in raised], ["a"])

    def test_the_budget_file_has_one_line_per_crate(self):
        self.assertEqual(verify.implicit_panic_budget("# c\na 3\n\nb 0  # x\n"), {"a": 3, "b": 0})
        for bad in ("a\n", "a x\n", "a 1\na 2\n"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                verify.implicit_panic_budget(bad)

    def test_the_committed_budget_parses(self):
        budget = verify.implicit_panic_budget((verify.ROOT / verify.IMPLICIT_PANIC_BUDGET).read_text())
        self.assertIn("athanor-sulphur", budget)

    def test_without_the_cargo_variable_clippy_does_not_run(self):
        with (
            mock.patch.dict(os.environ, {verify.IMPLICIT_PANIC_CARGO: ""}),
            mock.patch.object(verify, "BASE_REF", None),
            mock.patch.object(verify.subprocess, "run", side_effect=verify.subprocess.run) as run,
        ):
            res = verify.check_implicit_panics()
        self.assertEqual([c for c in run.call_args_list if "clippy" in c.args[0]], [])
        self.assertEqual(res.problems, [])
        self.assertEqual(len(res.notes), 1)


class ConfigTest(unittest.TestCase):
    def test_clippy_measures_with_the_lints_forced_and_no_rustflags(self):
        calls, real = [], subprocess.run

        def run(args, **kw):
            if "clippy" not in args:
                return real(args, **kw)
            calls.append((args, kw["env"]))
            return subprocess.CompletedProcess(args, 0, stdout=FINISHED + "\n", stderr="")

        dropped = {"RUSTFLAGS": "--cap-lints=allow", "CARGO_ENCODED_RUSTFLAGS": "-Awarnings",
                   "CARGO_BUILD_RUSTFLAGS": "x", "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS": "x",
                   "CLIPPY_CONF_DIR": "/x", "CLIPPY_ARGS": "x"}
        with (
            mock.patch.dict(os.environ, {verify.IMPLICIT_PANIC_CARGO: "cargo", **dropped}),
            mock.patch.object(verify, "BASE_REF", None),
            mock.patch.object(verify.subprocess, "run", side_effect=run),
        ):
            verify.check_implicit_panics()
        self.assertEqual(len(calls), len(verify.IMPLICIT_PANIC_RUNS))
        for args, env in calls:
            lints = args[args.index("--") + 1:]
            self.assertEqual(lints, [a for lint in verify.IMPLICIT_PANIC_LINTS for a in ("--force-warn", lint)])
            self.assertEqual(sorted(dropped.keys() & env.keys()), [])
            self.assertEqual(env[verify.IMPLICIT_PANIC_CARGO], "cargo")

    def test_a_cargo_or_clippy_configuration_that_hides_a_lint_is_found(self):
        files = {
            ".cargo/config.toml": '[build]\nrustflags = ["-W", "x", "-A", "clippy::indexing_slicing", "--cap-lints=warn"]\n'
            '[target.x]\nrustflags = "-Aclippy::string_slice"\n[env]\nCLIPPY_CONF_DIR = "/x"\nOTHER = "y"\n',
            "sub/.cargo/config": '[build]\nrustflags = ["--allow", "warnings"]\n',
            "ok/.cargo/config.toml": '[build]\nrustflags = ["-D", "unsafe_code", "-C", "link-arg=-Wl,-z,now"]\n',
            "clippy.toml": 'arithmetic-side-effects-allowed = ["X"]\nmsrv = "1.80"\n',
            "x/.clippy.toml": "suppress-restriction-lint-in-const = true\n",
            "bad/clippy.toml": "not toml [",
            "config.toml": '[build]\nrustflags = ["-A", "x"]\n',
            "inc/.cargo/config.toml": 'include = ["other.toml"]\n',
            "wrap/.cargo/config.toml": '[build]\nrustc-wrapper = "w"\nrustc = "r"\njobs = 4\n',
        }
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for path, text in files.items():
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text(text)
            found = verify.implicit_panic_configs(root, list(files))
        self.assertEqual([f.split(":")[0] for f in found], [
            ".cargo/config.toml", ".cargo/config.toml", ".cargo/config.toml", ".cargo/config.toml",
            "bad/clippy.toml", "clippy.toml", "inc/.cargo/config.toml",
            "sub/.cargo/config", "wrap/.cargo/config.toml", "wrap/.cargo/config.toml", "x/.clippy.toml",
        ])

    def test_a_cfg_on_clippy_or_debug_assertions_in_a_member_is_found(self):
        files = {
            "m/src/a.rs": "#[cfg(not(clippy))]\nfn f() {}\n",
            "m/src/b.rs": "#[cfg_attr(not(debug_assertions), path = \"x.rs\")]\nmod x;\n",
            "m/src/c.rs": "macro_rules! hide { ($c:meta) => { #[cfg($c)] fn g() {} } }\nhide!(not(r#clippy));\n",
            "m/src/ok.rs": "#[allow(clippy::indexing_slicing, clippy :: string_slice)]\nfn h() {}\n",
            "m/vendor/v/src/lib.rs": "#[cfg(not(clippy))]\nfn v() {}\n",
            "m/build.toml": "clippy = 1\n",
            "other/src/lib.rs": "#[cfg(not(clippy))]\nfn o() {}\n",
        }
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for path, text in files.items():
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text(text)
            found = verify.implicit_panic_cfgs(root, list(files), ["m"])
        self.assertEqual([f.rsplit(":", 1)[0] for f in found], ["m/src/a.rs:1", "m/src/b.rs:1", "m/src/c.rs:2"])

    def test_a_name_in_a_comment_is_not_a_cfg(self):
        files = {"m/src/a.rs": "// cfg(not(clippy)) hides code\n/* debug_assertions */\nfn f() {}\n"}
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "m/src").mkdir(parents=True)
            (root / "m/src/a.rs").write_text(files["m/src/a.rs"])
            self.assertEqual(verify.implicit_panic_cfgs(root, list(files), ["m"]), [])

    def test_not_feature_bodies_must_be_listed_at_their_count(self):
        body = '#[cfg(not(feature = "gtk"))]\nfn f() {}\n// #[cfg(not(feature = "x"))]\n'
        files = {"m/src/a.rs": body, "m/src/b.rs": body, "m/src/c.rs": "#[cfg(feature = \"gtk\")]\nfn g() {}\n"}
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "m/src").mkdir(parents=True)
            for path, text in files.items():
                (root / path).write_text(text)
            with mock.patch.object(verify, "IMPLICIT_PANIC_NOT_FEATURE", {"m/src/a.rs": 1}):
                found = verify.implicit_panic_cfgs(root, list(files), ["m"])
        self.assertEqual([f.split(":")[0] for f in found], ["m/src/b.rs"])

    def test_the_listed_not_feature_bodies_match_the_tree(self):
        tracked = subprocess.run(["git", "-C", str(verify.ROOT), "ls-files", "-z"], capture_output=True,
                                 text=True, check=True).stdout.split("\0")
        members = [d.relative_to(verify.ROOT).as_posix() for d in verify.workspace_crates().values()]
        self.assertEqual([f for f in verify.implicit_panic_cfgs(verify.ROOT, tracked, members)
                          if "not(feature" in f], [])

    def test_a_pruned_name_under_src_is_walked(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for path in ("c/src/target/a.rs", "c/src/x/.cache/b.rs", "c/src/node_modules/c.rs",
                         "c/src/repo-cache/d.rs", "c/target/e.rs", "c/.cache/f.rs", "node_modules/g.rs"):
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text("")
            with mock.patch.object(verify, "ROOT", root):
                found = sorted(p.relative_to(root).as_posix() for p in verify.walk(root, ".rs"))
        self.assertEqual(found, ["c/src/node_modules/c.rs", "c/src/repo-cache/d.rs",
                                 "c/src/target/a.rs", "c/src/x/.cache/b.rs"])


class BaseTest(unittest.TestCase):
    """The budget against the base, in any locale: a file the change adds has no base, a line
    raised over the base's fails, and so does a line dropped, or a file emptied, while the crate
    is still in the tree. Every workspace member has a line."""

    def check(self, base, now):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            budget = root / verify.IMPLICIT_PANIC_BUDGET
            budget.parent.mkdir(parents=True)
            (root / "Cargo.toml").write_text('[workspace]\nmembers = ["system/a"]\n')
            for name in ("a", "b"):
                (root / "system" / name).mkdir(parents=True)
                (root / "system" / name / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n')
            git = ["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]
            subprocess.run([*git, "init", "-q"], check=True)
            if base is not None:
                budget.write_text(base)
                subprocess.run([*git, "add", "."], check=True)
            subprocess.run([*git, "commit", "-q", "--allow-empty", "-m", "base"], check=True)
            budget.write_text(now)
            with (
                mock.patch.object(verify, "ROOT", root),
                mock.patch.object(verify, "BASE_REF", "HEAD"),
                mock.patch.dict(os.environ, {verify.IMPLICIT_PANIC_CARGO: "", "LANGUAGE": "it", "LANG": "it_IT.UTF-8"}),
            ):
                return sorted(p.split(":")[0] for p in verify.check_implicit_panics().problems)

    def test_a_budget_the_change_adds_has_no_base(self):
        self.assertEqual(self.check(None, "a 3\n"), [])

    def test_a_raised_line_fails_against_the_base(self):
        self.assertEqual(self.check("a 2\nb 4\n", "a 3\nb 1\n"), ["a"])

    def test_a_line_dropped_while_its_crate_stays_fails(self):
        self.assertEqual(self.check("a 2\nb 4\n", "a 2\n"), ["b"])
        self.assertEqual(self.check("a 2\nb 4\ngone 1\n", "a 2\nb 4\n"), [])

    def test_a_rename_keeps_at_most_the_old_count(self):
        base = "a 2\nb 4\ngone 3\n"
        self.assertEqual(self.check(base, "a 2\nb 4\nnew 4\n"), ["new"])
        self.assertEqual(self.check(base, "a 2\nb 4\nnew 3\n"), [])
        self.assertEqual(self.check(base, "a 2\nb 4\nx 2\ny 1\n"), [])
        self.assertEqual(self.check(base, "a 2\nb 4\nx 2\ny 2\n"), ["x, y"])
        self.assertEqual(self.check("a 2\nb 4\n", "a 2\nb 4\nnew 9\n"), [])

    def test_an_emptied_budget_fails(self):
        self.assertEqual(self.check("a 2\nb 4\n", "# empty\n"), ["a", "a", "b"])

    def test_a_member_without_a_line_fails_without_a_base(self):
        self.assertEqual(self.check(None, "b 4\n"), ["a"])


if __name__ == "__main__":
    unittest.main()
