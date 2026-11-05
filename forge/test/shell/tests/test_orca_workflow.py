"""Every Orca transcript in golden/orca/ is compared by a step of the shell workflow (#954)
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import re
import unittest
from pathlib import Path

import yaml

SHELL = Path(__file__).resolve().parent.parent
ROOT = SHELL.parent.parent.parent
WORKFLOW = ROOT / ".github/workflows/shell-surfaces.yml"
STEP = re.compile(r"bash forge/test/shell/rig\.sh orca ([a-z-]+)( it)?")


def compared(workflow):
    """The transcripts the workflow's steps compare, by file name: `rig.sh orca <surface> [it]`
    compares golden/orca/orca-<surface>[-it].txt."""
    names = set()
    for job in workflow["jobs"].values():
        for step in job.get("steps", []):
            match = STEP.fullmatch(step.get("run", "").strip())
            if match:
                names.add(f"orca-{match[1]}{'-it' if match[2] else ''}.txt")
    return names


class EveryTranscriptHasAStep(unittest.TestCase):
    def test_each_transcript_is_compared_in_the_shell_workflow(self):
        transcripts = {path.name for path in (SHELL / "golden/orca").glob("*.txt")}
        self.assertTrue(transcripts)
        steps = compared(yaml.safe_load(WORKFLOW.read_text(encoding="utf-8")))
        self.assertEqual(
            sorted(transcripts - steps),
            [],
            "transcripts no step of shell-surfaces.yml compares",
        )
        self.assertEqual(
            sorted(steps - transcripts), [], "steps that compare no transcript"
        )


class SulphurWalksRunUnderSulphur(unittest.TestCase):
    # The dock's and the launcher's references are athanor-sulphur's transcripts, the default
    # session of 1.0 (ADR-0135): under cosmic-comp the launcher is never activated (#751) and
    # 1.11 does not speak the dock's Shift+F10 menu.
    def test_dock_and_launcher_steps_set_the_compositor_and_always_run(self):
        workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        found = []
        for job in workflow["jobs"].values():
            for step in job.get("steps", []):
                match = STEP.fullmatch(step.get("run", "").strip())
                if match and match[1] in ("dock", "launcher"):
                    found.append(match[0])
                    with self.subTest(step=match[0]):
                        self.assertEqual(step.get("env", {}).get("RIG_COMPOSITOR"), "athanor")
                        self.assertNotIn("if", step)
        self.assertEqual(len(found), 4, found)


if __name__ == "__main__":
    unittest.main()
