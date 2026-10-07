"""Unit tests of the polkit model check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

POLICY_PATH = "pkg/os.athanor.x.policy"
POLICY = """<?xml version="1.0"?>
<policyconfig>
  <action id="os.athanor.x.apply">
    <defaults><allow_any>auth_admin</allow_any><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="os.athanor.x.peek">
    <defaults><allow_any>no</allow_any></defaults>
  </action>
</policyconfig>
"""
RULES_PATH = "pkg/etc/polkit-1/rules.d/10-x.rules"
RULES = """polkit.addRule(function(action, subject) {
    if (action.id == "org.example.mount" || action.id == "org.example.eject") {
        if (subject.isInGroup("wheel")) {
            return polkit.Result.YES;
        }
        return polkit.Result.AUTH_ADMIN;
    }
});
"""
HEADER = (
    "| Action | Declared in | Active session | Notes |\n| --- | --- | --- | --- |\n"
)
ROWS = (
    "| `os.athanor.x.apply` | `pkg/os.athanor.x.policy` | `auth_admin_keep` | n |\n"
    "| `os.athanor.x.peek` | `pkg/os.athanor.x.policy` | `no` | allow_active absent |\n"
    "| `org.example.mount` | `pkg/etc/polkit-1/rules.d/10-x.rules` | `yes` for wheel, `auth_admin` otherwise | n |\n"
    "| `org.example.eject` | `pkg/etc/polkit-1/rules.d/10-x.rules` | `yes` for wheel, `auth_admin` otherwise | n |\n"
)
FILES = {POLICY_PATH: POLICY, RULES_PATH: RULES}


def problems(doc, files=FILES):
    return verify.polkit_model_problems(files, "intro\n\n" + doc + "\nafter\n")


class PolkitModelTest(unittest.TestCase):
    def test_a_complete_table_passes(self):
        self.assertEqual(problems(HEADER + ROWS), [])

    def test_a_declared_action_missing_from_the_table_fails(self):
        rows = "".join(r for r in ROWS.splitlines(True) if "x.peek" not in r)
        found = problems(HEADER + rows)
        self.assertEqual(len(found), 1)
        self.assertIn("os.athanor.x.peek", found[0])

    def test_an_overridden_action_missing_from_the_table_fails(self):
        rows = "".join(r for r in ROWS.splitlines(True) if "eject" not in r)
        self.assertEqual(len(problems(HEADER + rows)), 1)

    def test_a_policy_result_that_differs_fails(self):
        rows = ROWS.replace("| `auth_admin_keep` |", "| `yes` |")
        found = problems(HEADER + rows)
        self.assertEqual(len(found), 1)
        self.assertIn("auth_admin_keep", found[0])

    def test_a_rule_result_that_differs_fails(self):
        rows = ROWS.replace(
            "| `org.example.mount` | `pkg/etc/polkit-1/rules.d/10-x.rules` | `yes` for wheel, `auth_admin` otherwise |",
            "| `org.example.mount` | `pkg/etc/polkit-1/rules.d/10-x.rules` | `yes` |",
        )
        self.assertEqual(len(problems(HEADER + rows)), 1)

    def test_a_row_for_an_action_the_repository_no_longer_declares_fails(self):
        extra = "| `os.athanor.gone` | `pkg/os.athanor.x.policy` | `yes` | n |\n"
        self.assertEqual(len(problems(HEADER + ROWS + extra)), 1)

    def test_a_row_naming_the_wrong_file_fails_twice(self):
        rows = ROWS.replace(
            "| `os.athanor.x.peek` | `pkg/os.athanor.x.policy` |",
            "| `os.athanor.x.peek` | `pkg/other.policy` |",
        )
        self.assertEqual(len(problems(HEADER + rows)), 2)

    def test_a_duplicated_row_fails(self):
        first = ROWS.splitlines(True)[0]
        self.assertEqual(len(problems(HEADER + ROWS + first)), 1)

    def test_no_table_fails_once_per_declared_action_and_once_for_the_table(self):
        self.assertEqual(len(problems("")), 5)

    def test_udev_rules_are_not_polkit_rules(self):
        files = dict(FILES)
        files["pkg/udev/rules.d/71-x.rules"] = 'KERNEL=="nvidia*", TAG+="uaccess"\n'
        self.assertEqual(problems(HEADER + ROWS, files), [])

    def test_a_polkit_rule_whose_actions_cannot_be_read_fails(self):
        files = dict(FILES)
        files["pkg/rules.d/20-y.rules"] = (
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id.indexOf("org.example.") == 0) { return polkit.Result.YES; }\n'
            "});\n"
        )
        found = problems(HEADER + ROWS, files)
        self.assertEqual(len(found), 1)
        self.assertIn("20-y.rules", found[0])


    def unreadable(self, text):
        files = dict(FILES)
        files["pkg/rules.d/20-y.rules"] = text
        found = problems(HEADER + ROWS, files)
        self.assertEqual(len(found), 1, found)
        self.assertIn("20-y.rules", found[0])
        self.assertIn("cannot be checked", found[0])

    def test_a_prefix_match_beside_an_exact_one_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a") { return polkit.Result.NO; }\n'
            '    if (action.id.indexOf("org.freedesktop.") == 0) { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_an_aliased_addrule_fails(self):
        self.unreadable(
            "var r = polkit.addRule;\n"
            'r(function(action, subject) { if (action.id == "org.example.a") return polkit.Result.YES; });\n'
        )

    def test_spacing_and_single_quotes_are_read(self):
        files = dict(FILES)
        files[RULES_PATH] = RULES.replace("polkit.addRule(", "polkit.addRule (").replace(
            '"org.example.eject"', "'org.example.eject'"
        ).replace("==", "===")
        self.assertEqual(problems(HEADER + ROWS, files), [])

    def test_comments_are_not_rules(self):
        files = dict(FILES)
        files[RULES_PATH] = (
            '/* was: action.id == "org.example.old", polkit.Result.NO, polkit.addRule( */\n'
            "// polkit.Result.AUTH_SELF\n" + RULES
        )
        self.assertEqual(problems(HEADER + ROWS, files), [])

    def test_not_handled_can_be_written_in_the_table(self):
        files = dict(FILES)
        files[RULES_PATH] = RULES.replace("polkit.Result.AUTH_ADMIN", "polkit.Result.NOT_HANDLED")
        rows = ROWS.replace("`auth_admin` otherwise", "`not_handled` otherwise")
        self.assertEqual(problems(HEADER + rows, files), [])

    def test_a_policy_action_without_an_id_fails(self):
        files = dict(FILES)
        files["pkg/bad.policy"] = "<policyconfig><action><defaults/></action></policyconfig>"
        found = problems(HEADER + ROWS, files)
        self.assertEqual(len(found), 1, found)
        self.assertIn("bad.policy", found[0])
        self.assertIn("cannot be checked", found[0])

if __name__ == "__main__":
    unittest.main()
