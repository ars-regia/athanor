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

    def test_a_reusable_workflow_caller_reports_its_inner_jobs(self):
        caller = "on:\n  merge_group:\njobs:\n  kernel:\n    uses: ./.github/workflows/k.yml\n"
        inner = "on:\n  workflow_call:\njobs:\n  gate:\n    name: Kernel gate\n    runs-on: x\n"
        required = {"iso-v0": {"required_status_checks": {"checks": [{"context": "kernel"}]}}}
        nested = {"iso-v0": {"required_status_checks": {"checks": [{"context": "kernel / Kernel gate"}]}}}
        workflows = {"pr.yml": PR, "c.yml": caller, "k.yml": inner}
        self.assertEqual(1, len(verify.merge_queue_problems(tree([REQUIRED, QUEUE], required, workflows))))
        self.assertEqual([], verify.merge_queue_problems(tree([REQUIRED, QUEUE], nested, workflows)))

    def test_an_unnamed_matrix_job_reports_one_context_per_combination(self):
        matrix = "on:\n  merge_group:\njobs:\n  gate:\n    strategy:\n      matrix:\n        n: [1, 2]\n    runs-on: x\n"
        self.assertEqual(1, len(verify.merge_queue_problems(tree([REQUIRED, QUEUE], {}, {"pr.yml": matrix}))))
        combo = {"iso-v0": {"required_status_checks": {"checks": [{"context": "gate (1)"}]}}}
        only_combo = [{"type": "merge_queue", "parameters": {}}]
        self.assertEqual([], verify.merge_queue_problems(tree(only_combo, combo, {"pr.yml": matrix})))

    def test_the_committed_settings_with_a_queue_report_the_legacy_contexts(self):
        root = SCRIPT.parents[1]
        problems = verify.merge_queue_problems(tree(
            [REQUIRED, QUEUE], json.loads((root / verify.BRANCH_PROTECTION_JSON).read_text()),
            {p.name: p.read_text() for p in (root / ".github/workflows").glob("*.y*ml")}))
        self.assertTrue(problems)
        self.assertFalse(any("'gate'" in p for p in problems))

    def test_the_committed_settings_pass(self):
        self.assertEqual([], verify.merge_queue_problems(SCRIPT.parents[1]))


if __name__ == "__main__":
    unittest.main()
