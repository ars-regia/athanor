"""PIPE-N06: verify.py refuses a merge-queue ruleset while a required status context is
reported by no workflow that runs on merge_group."""

import importlib.util
import json
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

QUEUE = {"type": "merge_queue", "parameters": {}}
REQUIRED = {"type": "required_status_checks",
            "parameters": {"required_status_checks": [{"context": "gate"}]}}


def tree(rules, protection, workflows):
    root = pathlib.Path(tempfile.mkdtemp())
    (root / ".github/settings").mkdir(parents=True)
    (root / ".github/workflows").mkdir()
    (root / verify.RULESETS_JSON).write_text(json.dumps({"product": {"rules": rules}}))
    (root / verify.BRANCH_PROTECTION_JSON).write_text(json.dumps(protection))
    for name, text in workflows.items():
        (root / ".github/workflows" / name).write_text(text)
    return root


PR = "on:\n  pull_request:\n  merge_group:\njobs:\n  gate:\n    runs-on: x\n"
KERNEL = "on:\n  pull_request:\njobs:\n  gate:\n    name: Kernel gate\n    runs-on: x\n"
LEGACY = {"iso-v0": {"required_status_checks": {"checks": [{"context": "Kernel gate"}]}}}


class MergeQueueTest(unittest.TestCase):
    def test_no_queue_passes_whatever_is_required(self):
        self.assertEqual([], verify.merge_queue_problems(tree([REQUIRED], LEGACY, {"k.yml": KERNEL})))

    def test_queue_with_every_context_on_merge_group_passes(self):
        self.assertEqual([], verify.merge_queue_problems(tree([REQUIRED, QUEUE], {}, {"pr.yml": PR})))

    def test_queue_with_a_context_no_merge_group_workflow_reports_fails(self):
        problems = verify.merge_queue_problems(
            tree([REQUIRED, QUEUE], LEGACY, {"pr.yml": PR, "k.yml": KERNEL}))
        self.assertEqual(1, len(problems))
        self.assertIn("'Kernel gate'", problems[0])

    def test_the_committed_settings_pass(self):
        self.assertEqual([], verify.merge_queue_problems(SCRIPT.parents[1]))


if __name__ == "__main__":
    unittest.main()
