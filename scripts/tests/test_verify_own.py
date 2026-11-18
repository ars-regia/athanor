"""Unit tests of scripts/verify.py own, ADR-0141 decision 7 on the crates
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

LOCK = """\
version = 4

[[package]]
name = "athanor-bar"
version = "1.0.0"
dependencies = ["athanor-style", "zbus"]

[[package]]
name = "athanor-style"
version = "1.0.0"
dependencies = ["gtk-glue"]

[[package]]
name = "gtk-glue"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = ["gtk4"]

[[package]]
name = "gtk4"
version = "0.10.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "zbus"
version = "5.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"""


class Own(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.write("Cargo.toml", '[workspace]\nmembers = ["bar", "style"]\n')
        self.write("Cargo.lock", LOCK)
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nzbus = "5"\n'
            '\n[dev-dependencies]\nathanor-style = { path = "../style" }\n',
        )
        self.write(
            "style/Cargo.toml",
            '[package]\nname = "athanor-style"\n\n[dependencies]\ngtk-glue = "0.1"\n',
        )
        self.write(verify.OWN_ALLOW, "# header\ncrate athanor-style #1292\n")
        self.commit()

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *args):
        subprocess.run(
            ["git", "-C", str(self.root), *args], check=True, capture_output=True
        )

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text))
        self.git("add", rel)

    def commit(self):
        self.git(
            "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base"
        )

    def problems(self, base=None):
        return verify.own_problems(self.root, base)

    def test_the_repository_passes(self):
        self.assertEqual(verify.own_problems(), [])

    def test_a_listed_crate_reaching_gtk4_passes(self):
        self.assertEqual(self.problems("HEAD"), [])

    def test_a_dev_dependency_does_not_count(self):
        self.assertNotIn("athanor-bar", verify.toolkit_crates(self.root)[0])

    def test_a_runtime_dependency_counts(self):
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nzbus = "5"\n'
            'athanor-style = { path = "../style" }\n',
        )
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("athanor-bar: reaches gtk4", problems[0])

    def test_a_build_dependency_counts(self):
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dev-dependencies]\n'
            'athanor-style = { path = "../style" }\n\n[build-dependencies]\n'
            'athanor-style = { path = "../style" }\n',
        )
        self.assertIn("athanor-bar", verify.toolkit_crates(self.root)[0])

    def test_a_workspace_dev_dependency_under_another_key_does_not_count(self):
        self.write(
            "Cargo.toml",
            '[workspace]\nmembers = ["bar", "style"]\n\n[workspace.dependencies]\n'
            'style = { package = "athanor-style", path = "style" }\n',
        )
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nzbus = "5"\n'
            "\n[dev-dependencies]\nstyle = { workspace = true }\n",
        )
        self.assertNotIn("athanor-bar", verify.toolkit_crates(self.root)[0])

    def test_an_underscore_dev_dependency_does_not_count(self):
        self.write(
            "bar/Cargo.toml",
            '[package]\nname = "athanor-bar"\n\n[dependencies]\nzbus = "5"\n'
            '\n[dev_dependencies]\nathanor-style = { path = "../style" }\n',
        )
        self.assertNotIn("athanor-bar", verify.toolkit_crates(self.root)[0])

    def test_an_underscore_build_dependency_counts(self):
        self.write("kiosk/Cargo.toml", '[package]\nname = "athanor-kiosk"\n\n[build_dependencies]\n'
                   'gtk4 = "0.10"\n')
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("athanor-kiosk: reaches gtk4", problems[0])

    def test_a_subcrate_of_a_toolkit_counts(self):
        for sub in ("gtk4-sys", "libadwaita-sys", "iced_core"):
            with self.subTest(sub=sub):
                self.write("Cargo.lock", LOCK.replace('name = "gtk4"', f'name = "{sub}"')
                           .replace('dependencies = ["gtk4"]', f'dependencies = ["{sub}"]'))
                self.assertEqual(verify.toolkit_crates(self.root)[0].get("athanor-style"), {sub})

    def test_a_crate_that_only_shares_a_prefix_does_not_count(self):
        self.write("Cargo.lock", LOCK.replace('name = "gtk4"', 'name = "gtk4ish"')
                   .replace('dependencies = ["gtk4"]', 'dependencies = ["gtk4ish"]'))
        self.assertNotIn("athanor-style", verify.toolkit_crates(self.root)[0])

    def test_an_entry_that_no_longer_applies_fails(self):
        self.write(
            verify.OWN_ALLOW, "crate athanor-style #1292\ncrate athanor-bar #1292\n"
        )
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("crate athanor-bar reaches none", problems[0])

    def test_an_unlisted_crate_fails(self):
        self.write(verify.OWN_ALLOW, "# empty\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("athanor-style: reaches gtk4", problems[0])

    def test_an_entry_added_against_the_base_fails(self):
        self.write(
            verify.OWN_ALLOW, "crate athanor-style #1292\nrpm cosmic-comp #1292\n"
        )
        problems = self.problems("HEAD")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("rpm cosmic-comp is not listed at HEAD", problems[0])

    def test_a_changed_issue_is_not_an_added_entry(self):
        self.write(verify.OWN_ALLOW, "crate athanor-style #2000\n")
        self.assertEqual(self.problems("HEAD"), [])

    def test_a_vendored_lockfile_is_not_read(self):
        self.write(
            "x/vendor/y/Cargo.lock", LOCK.replace("athanor-style", "athanor-other")
        )
        self.assertNotIn("athanor-other", verify.toolkit_crates(self.root)[0])

    def test_a_crate_no_lockfile_holds_counts_when_it_names_a_toolkit(self):
        self.write("kiosk/Cargo.toml", '[package]\nname = "athanor-kiosk"\n\n[dependencies]\n'
                   'gtk4 = { workspace = true }\n')
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("athanor-kiosk: reaches gtk4", problems[0])

    def test_a_crate_no_lockfile_holds_fails_when_its_toolkits_cannot_be_read(self):
        self.write("kiosk/Cargo.toml", '[package]\nname = "athanor-kiosk"\n\n[dependencies]\n'
                   'relm4 = "0.9"\n')
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("athanor-kiosk: no tracked Cargo.lock holds it", problems[0])

    def test_an_unknown_base_fails(self):
        problems = self.problems("deadbeef")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("deadbeef is not a commit", problems[0])

    def test_a_base_without_the_list_adopts_it(self):
        self.git("rm", "-q", verify.OWN_ALLOW)
        self.commit()
        self.write(verify.OWN_ALLOW, "crate athanor-style #1292\n")
        self.assertEqual(self.problems("HEAD"), [])

    def test_a_malformed_list_fails(self):
        self.write(verify.OWN_ALLOW, "crate athanor-style\n")
        problems = self.problems()
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("expected `rpm|crate NAME #ISSUE`", problems[0])


if __name__ == "__main__":
    unittest.main()
