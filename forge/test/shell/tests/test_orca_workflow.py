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


MATRIX = re.compile(r'"\$\{\{ matrix\.([a-z]+) \}\}"')


def compared(workflow):
    """The transcripts the workflow's steps compare, by file name: `rig.sh orca <surface> [it]`
    compares golden/orca/orca-<surface>[-it].txt; a surface `"${{ matrix.<key> }}"` stands for
    each value of the job's matrix key."""
    names = set()
    for job in workflow["jobs"].values():
        matrix = job.get("strategy", {}).get("matrix", {})
        for step in job.get("steps", []):
            run = step.get("run", "").strip()
            key = MATRIX.search(run)
            runs = [MATRIX.sub(value, run) for value in matrix[key[1]]] if key else [run]
            for run in runs:
                match = STEP.fullmatch(run)
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
    # session of 1.0 (ADR-0135): cosmic-comp never activated the GTK launcher the reference was
    # recorded from (#751; athanor-sal's is activated), and 1.11 does not speak the dock's
    # Shift+F10 menu. The dock is walked twice in each language, the GTK dock and the toolkit
    # dock (ATHANOR_DOCK_BIN), against the same transcripts.
    def test_dock_and_launcher_steps_set_the_compositor_and_always_run(self):
        workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        found = []
        toolkit = []
        for job in workflow["jobs"].values():
            for step in job.get("steps", []):
                match = STEP.fullmatch(step.get("run", "").strip())
                if match and match[1] in ("dock", "launcher"):
                    found.append(match[0])
                    env = step.get("env", {})
                    if "ATHANOR_DOCK_BIN" in env:
                        toolkit.append(match[0])
                        # rig.sh names the toolkit dock's transcripts from this path only.
                        self.assertEqual(env["ATHANOR_DOCK_BIN"], "/out/toolkit-dock/athanor-dock")
                    with self.subTest(step=match[0]):
                        self.assertEqual(env.get("RIG_COMPOSITOR"), "athanor")
                        self.assertNotIn("if", step)
        self.assertEqual(len(found), 6, found)
        self.assertEqual(
            sorted(toolkit),
            ["bash forge/test/shell/rig.sh orca dock", "bash forge/test/shell/rig.sh orca dock it"],
        )


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS",
             "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY_OWNER")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
