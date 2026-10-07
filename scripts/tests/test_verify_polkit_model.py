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

    def test_an_aliased_polkit_object_fails(self):
        self.unreadable(
            "var p = polkit;\n"
            'p.addRule(function(action, subject) { if (action.id == "org.example.a") return p.Result.YES; });\n'
        )

    def test_a_bracket_read_of_the_action_id_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a" || action["id"] == "org.example.b") return polkit.Result.YES;\n'
            "});\n"
        )

    def test_an_aliased_action_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            "    var id = action.id;\n"
            '    if (action.id == "org.example.a" || id == "org.example.b") return polkit.Result.YES;\n'
            "});\n"
        )

    def test_a_comment_marker_inside_a_string_hides_no_code(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    var u = "//"; if (action.id == "org.example.a") { return polkit.Result.NO; } '
            'if (action.id.indexOf("org.") == 0) { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_a_url_in_a_string_is_not_a_comment(self):
        files = dict(FILES)
        files[RULES_PATH] = RULES.replace(
            '"wheel"',
            '"https://example.org//wheel"',
        )
        self.assertEqual(problems(HEADER + ROWS, files), [])

    def test_a_concatenated_action_id_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example." + "a") return polkit.Result.YES;\n'
            "});\n"
        )

    def test_an_aliased_result_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            "    var r = polkit.Result;\n"
            '    if (action.id == "org.example.a") { if (subject.local) return r.YES; return polkit.Result.NO; }\n'
            "});\n"
        )

    def test_a_renamed_action_parameter_fails(self):
        self.unreadable(
            "polkit.addRule(function(a, subject) {\n"
            '    if (a.id == "org.example.b" || action.id == "org.example.a") return polkit.Result.YES;\n'
            "});\n"
        )

    def test_a_negated_action_test_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (!(action.id == "org.example.a")) { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_a_result_in_an_else_branch_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a") { } else { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_an_always_true_alternative_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a" || true) { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_an_or_after_an_and_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a" && subject.local || subject.active) { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_an_unparenthesised_or_before_an_and_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a" || action.id == "org.example.b" && subject.local) { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_a_template_literal_fails(self):
        self.unreadable(
            "var x = `${polkit.addRule(function(action, subject) { return polkit.Result.YES; })}`;\n"
        )

    def test_eval_fails(self):
        self.unreadable('eval("polkit.addRule(function(a){return polkit.Result.YES})");\n')

    def test_code_beside_the_calls_fails(self):
        self.unreadable(
            "var unused = 1;\n"
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a") { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_a_rule_registered_inside_a_rule_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a") {\n'
            "        polkit.addRule(function(action, subject) { return polkit.Result.YES; });\n"
            "        return polkit.Result.NO;\n"
            "    }\n"
            "});\n"
        )

    def test_an_identifier_shaped_like_a_literal_marker_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            "    if (action.id == S0) { return polkit.Result.YES; }\n"
            "});\n"
        )

    def test_an_id_guard_and_a_subject_condition_are_read(self):
        files = dict(FILES)
        files[RULES_PATH] = (
            "polkit.addRule(function (action, subject) {\n"
            '  if ((action.id == "org.example.mount" || action.id == "org.example.eject") && subject.user == "runner") {\n'
            "    return polkit.Result.YES;\n"
            "  }\n"
            "});\n"
        )
        rows = ROWS.replace("`yes` for wheel, `auth_admin` otherwise", "`yes` for runner")
        self.assertEqual(problems(HEADER + rows, files), [])

    def test_a_udev_rule_naming_polkit_in_a_comment_is_skipped(self):
        files = dict(FILES)
        files["pkg/udev/rules.d/72-x.rules"] = '# access is granted via polkit\nSUBSYSTEM=="usb", MODE="0660"\n'
        self.assertEqual(problems(HEADER + ROWS, files), [])

    def test_a_rules_file_outside_udev_that_never_names_addRule_fails(self):
        self.unreadable('polkit["add" + "Rule"](function(action, subject) { return "yes"; });\n')

    def test_an_identifier_outside_the_closed_set_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a") { return polkit.Result.NO; "".constructor.constructor("x")(); }\n'
            "});\n"
        )

    def test_an_admin_rule_body_outside_the_closed_set_fails(self):
        self.unreadable(
            'polkit.addAdminRule(function(action, subject) { "".constructor.constructor("x")(); return ["unix-group:wheel"]; });\n'
        )

    def test_a_return_other_than_a_polkit_result_fails(self):
        self.unreadable(
            "polkit.addRule(function(action, subject) {\n"
            '    if (action.id == "org.example.a") { if (subject.user == "x") { return polkit.Result.NO; } return "yes"; }\n'
            "});\n"
        )

    def test_a_line_separator_ends_a_comment(self):
        self.unreadable(
            RULES
            + "// x polkit.addRule(function(action, subject) { return polkit.Result.YES; });\n"
        )

    def test_a_carriage_return_ends_a_comment(self):
        self.unreadable(
            RULES + "// x\rpolkit.addRule(function(action, subject) { return polkit.Result.YES; });\n"
        )

    def test_a_backslash_outside_a_string_fails(self):
        self.unreadable(
            "polkit.addRule(function(\\u0061ction, subject) {\n"
            '    if (action.id == "org.example.a") { return polkit.Result.YES; }\n'
            "});\n"
        )

    def test_a_subject_condition_before_the_action_test_is_read(self):
        files = dict(FILES)
        files[RULES_PATH] = (
            "polkit.addRule(function (action, subject) {\n"
            '  if (subject.user == "runner" && ((action.id == "org.example.mount" || action.id == "org.example.eject"))) {\n'
            "    return polkit.Result.YES;\n"
            "  }\n"
            "});\n"
        )
        rows = ROWS.replace("`yes` for wheel, `auth_admin` otherwise", "`yes` for runner")
        self.assertEqual(problems(HEADER + rows, files), [])

    def test_two_rules_on_one_action_merge_their_results(self):
        declared, unreadable = verify.polkit_declared({
            RULES_PATH: (
                'polkit.addRule(function(action, subject) { if (action.id == "org.example.a" && subject.isInGroup("wheel")) { return polkit.Result.YES; } });\n'
                'polkit.addRule(function(action, subject) { if (action.id == "org.example.a") { return polkit.Result.NO; } });\n'
            )
        })
        self.assertEqual((declared, unreadable), ({("org.example.a", RULES_PATH): {"yes", "no"}}, []))

    def test_an_assignment_in_the_guard_fails(self):
        self.unreadable(
            'polkit.addRule(function(action, subject) { if (action.id == "org.example.a" && (polkit.Result.NO = polkit.Result.YES)) { return polkit.Result.NO; } });\n'
        )

    def test_an_assignment_in_an_admin_rule_fails(self):
        self.unreadable("polkit.addAdminRule(function(action, subject) { polkit.Result.NO = polkit.Result.YES; });\n")

if __name__ == "__main__":
    unittest.main()
