"""A cancelled polkit dialog is not a pam_faillock failure (issue #447, polkit-org/polkit#713)
(python3 -B -m unittest discover -s forge/specs/polkit/tests -v).

athanor-agent-helper-cancel-before-answer.patch ends the agent helper when the agent hangs
up before answering any prompt, instead of returning PAM_CONV_ERR into an auth stack that
runs on to pam_faillock's authfail. These tests hold the spec to applying the patch and to
running, in %check, the C check that drives the patched conversation.
"""

import re
import unittest
from pathlib import Path

SPEC_DIR = Path(__file__).resolve().parents[1]
PATCH = SPEC_DIR / "SOURCES/athanor-agent-helper-cancel-before-answer.patch"
CHECK = SPEC_DIR / "SOURCES/agent-helper-hangup-check.c"
SECTIONS = (
    "prep build install check post preun postun files changelog package description"
)


def spec_text():
    return (SPEC_DIR / "polkit.spec").read_text()


def section(spec, name):
    """The body of the spec section NAME, up to the next section header."""
    body = spec.split(f"\n{name}\n", 1)[1]
    return re.split(
        r"(?m)^%(?:" + "|".join(SECTIONS.split()) + r")\b", body, maxsplit=1
    )[0]


class AgentHelperCancel(unittest.TestCase):
    def test_spec_applies_the_patch(self):
        spec = spec_text()
        self.assertRegex(spec, r"(?m)^Patch\d+:\s*" + re.escape(PATCH.name) + "$")
        self.assertIn("%autosetup -p1", section(spec, "%prep"))

    def test_patch_exits_only_before_an_answer(self):
        text = PATCH.read_text()
        self.assertEqual(
            re.findall(r"(?m)^diff --git a/(\S+)", text),
            ["src/polkitagent/polkitagenthelper-pam.c"],
        )
        added = "\n".join(
            line[1:]
            for line in text.splitlines()
            if line.startswith("+") and not line.startswith("+++")
        )
        # The exit sits behind the answered flag, and a read answer sets the flag.
        self.assertRegex(added, r"if \(!answered\)\s*\{[^}]*_exit \(2\);")
        self.assertIn("answered = TRUE;", added)

    def test_check_runs_the_hangup_cases(self):
        spec = spec_text()
        source = re.search(r"(?m)^Source(\d+):\s*" + re.escape(CHECK.name) + "$", spec)
        self.assertIsNotNone(source, "the C check is not a Source of polkit.spec")
        check = section(spec, "%check")
        self.assertIn(f"%{{SOURCE{source.group(1)}}}", check)
        self.assertIn("build/agent-helper-hangup-check || fail=1", check)
        self.assertTrue(check.rstrip().endswith("exit $fail"))
        cases = CHECK.read_text()
        for case in (
            'run ("", hang_up_before_answer), 2)',
            'run ("guess\\n", line_then_hang_up), 0)',
            'run ("gu", partial_then_hang_up), 0)',
            'run ("\\n", empty_then_hang_up), 0)',
        ):
            self.assertIn(case, cases)


if __name__ == "__main__":
    unittest.main()
