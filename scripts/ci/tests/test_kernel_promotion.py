"""scripts/ci/kernel_promotion.py: the four conditions of ADR-0110, on recorded API fixtures.

The fixtures under fixtures/kernel_promotion are the API answers for PR #270 and its pr.yml run
37614092466, trimmed to the fields the script reads. Each test rewrites the commit fields to the
commits of a local history, and the artifact digest to the one of a local zip.
"""

import contextlib
import copy
import hashlib
import http.client
import http.server
import importlib.util
import io
import json
import os
import pathlib
import subprocess
import tempfile
import threading
import unittest
import urllib.error
import zipfile
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "kernel_promotion.py"
spec = importlib.util.spec_from_file_location("kernel_promotion", SCRIPT)
promotion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(promotion)

FIXTURES = HERE / "fixtures" / "kernel_promotion"
REPOSITORY = "ars-regia/athanor"
RUN = 37614092466
ARTIFACT = 11482498571
NVR = "6.17.1-100.azoth.fc43"
PINS = "forge/specs/azoth/pins.env"
CALL_KERNEL = ".github/workflows/call-kernel.yml"
PR_YML = ".github/workflows/pr.yml"
# A stand-in for build-inputs.py, which reads only its own directory.
STUB = """import json, pathlib
k = pathlib.Path(__file__).resolve().parent
print(json.dumps({"pins": (k / "pins.env").read_text()}))
"""
ROOT = {
    "forge/specs/azoth/build-inputs.py": STUB,
    PINS: "FEDORA_KERNEL_NVR=6.17.0-100.fc43\n",
    PR_YML: "pr 1\n",
    CALL_KERNEL: "call-kernel 1\n",
    "scripts/ci/build-builder.sh": "builder 1\n",
    "docs/note": "root\n",
}
BUMP = {PINS: "FEDORA_KERNEL_NVR=6.17.1-100.fc43\n"}


def git(cwd, *args):
    return subprocess.run(
        ["git", "-C", str(cwd), *args], check=True, capture_output=True, text=True
    ).stdout.strip()


def commit(repo, message, files):
    for path, text in files.items():
        target = repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "--allow-empty", "-m", message)
    return git(repo, "rev-parse", "HEAD")


def history(tmp, base=None, head=None, parent=None, pushed=None):
    """origin: root, base (the base the pull request recorded), parent (iso-v0 moved on),
    pushed (the squash of the pull request); head branches from base. checkout is a depth-1
    clone of pushed, as actions/checkout makes it."""
    origin = tmp / "origin"
    origin.mkdir()
    git(origin, "init", "-q", "-b", "iso-v0")
    git(origin, "config", "user.email", "test@example.invalid")
    git(origin, "config", "user.name", "test")
    git(origin, "config", "uploadpack.allowAnySHA1InWant", "true")
    commit(origin, "root", ROOT)
    shas = {"base": commit(origin, "base", {"docs/note": "base\n", **(base or {})})}
    shas["parent"] = commit(origin, "parent", {"docs/note": "parent\n", **(parent or {})})
    shas["pushed"] = commit(origin, "pushed", {**BUMP, **(pushed or {})})
    git(origin, "checkout", "-q", shas["base"])
    shas["head"] = commit(origin, "head", {**BUMP, **(head or {})})
    git(origin, "checkout", "-q", "iso-v0")
    checkout = tmp / "checkout"
    git(tmp, "clone", "-q", "--depth=1", "--branch", "iso-v0", origin.as_uri(), str(checkout))
    return shas, checkout


def artifact_zip(path, nvr=NVR):
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("nvr", nvr + "\n")
        archive.writestr(f"kernel/kernel-core-{nvr}.x86_64.rpm", b"rpm")
        archive.writestr(f"devel/kernel-devel-{nvr}.x86_64.rpm", b"rpm")
    return path


def fixture(name):
    return json.loads((FIXTURES / name).read_text())


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


class FakeApi:
    """The recorded answers, rewritten to the local history."""

    def __init__(self, shas, zip_path):
        pulls = fixture("pulls_for_commit.json")
        pulls[0]["merge_commit_sha"] = shas["pushed"]
        pulls[0]["head"]["sha"] = shas["head"]
        pulls[0]["base"]["sha"] = shas["base"]
        runs = fixture("runs.json")
        runs["workflow_runs"][0]["head_sha"] = shas["head"]
        artifacts = fixture("artifacts.json")
        artifacts["artifacts"][0]["workflow_run"]["head_sha"] = shas["head"]
        artifacts["artifacts"][0]["digest"] = "sha256:" + sha256(zip_path)
        self.pulls, self.runs, self.jobs, self.artifacts = pulls, runs, fixture("jobs.json"), artifacts
        self.events = fixture("issue_events.json")
        self.head_pulls = [{"number": pulls[0]["number"]}]
        self.shas, self.zip_path = shas, zip_path
        self.error = None

    def json(self, path):
        if self.error:
            raise self.error
        shas = self.shas
        answers = {
            f"/repos/{REPOSITORY}/commits/{shas['pushed']}/pulls": self.pulls,
            f"/repos/{REPOSITORY}/commits/{shas['head']}/pulls": self.head_pulls,
            f"/repos/{REPOSITORY}/issues/270/events?per_page=100": self.events,
            f"/repos/{REPOSITORY}/actions/workflows/pr.yml/runs"
            f"?event=pull_request&head_sha={shas['head']}&per_page=100": self.runs,
            f"/repos/{REPOSITORY}/actions/runs/{RUN}/jobs?filter=latest&per_page=100": self.jobs,
            f"/repos/{REPOSITORY}/actions/runs/{RUN}/artifacts?name=kernel-build&per_page=100": self.artifacts,
        }
        if path not in answers:
            raise urllib.error.HTTPError(path, 404, "Not Found", {}, None)
        return copy.deepcopy(answers[path])

    def download(self, path, dest):
        if path != f"/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT}/zip":
            raise urllib.error.HTTPError(path, 404, "Not Found", {}, None)
        pathlib.Path(dest).write_bytes(pathlib.Path(self.zip_path).read_bytes())
        return sha256(dest)


class Case(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))

    def promote(self, **changes):
        shas, checkout = history(self.tmp, **changes)
        api = FakeApi(shas, artifact_zip(self.tmp / "artifact.zip"))
        return shas, api, checkout

    def run_promotion(self, shas, api, checkout, nvr=NVR):
        return promotion.promote(api, checkout, REPOSITORY, shas["pushed"], nvr, self.tmp / "out")

    def assertRefused(self, decision, text):
        self.assertFalse(decision["promoted"], decision)
        self.assertIn(text, decision["reason"])
        self.assertFalse((self.tmp / "out").exists())


class ConditionOneTest(Case):
    def test_a_commit_no_pull_request_was_merged_as_builds(self):
        shas, api, checkout = self.promote()
        api.pulls[0]["merge_commit_sha"] = shas["parent"]
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: 0 pull requests")

    def test_a_pull_request_merged_into_another_branch_builds(self):
        shas, api, checkout = self.promote()
        api.pulls[0]["base"]["ref"] = "main"
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: 0 pull requests")

    def test_an_unmerged_pull_request_builds(self):
        shas, api, checkout = self.promote()
        api.pulls[0]["merged_at"] = None
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: 0 pull requests")

    def test_a_head_shared_with_another_pull_request_builds(self):
        shas, api, checkout = self.promote()
        api.head_pulls.append({"number": 271})
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: the head")

    def test_a_run_of_another_workflow_or_event_builds(self):
        for field, value in (("path", ".github/workflows/kernel-build.yml"), ("event", "push")):
            with self.subTest(field=field):
                self.tmp = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
                shas, api, checkout = self.promote()
                api.runs["workflow_runs"][0][field] = value
                self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: no completed")

    def test_a_run_from_a_fork_builds(self):
        shas, api, checkout = self.promote()
        api.runs["workflow_runs"][0]["head_repository"]["full_name"] = "someone/athanor"
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: no completed")

    def test_a_run_still_in_progress_builds(self):
        shas, api, checkout = self.promote()
        api.runs["workflow_runs"][0]["status"] = "in_progress"
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: no completed")

    def test_a_build_or_verdict_that_did_not_succeed_builds(self):
        for name in promotion.JOBS:
            for conclusion in ("failure", "skipped", "cancelled"):
                with self.subTest(job=name, conclusion=conclusion):
                    self.tmp = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
                    shas, api, checkout = self.promote()
                    for job in api.jobs["jobs"]:
                        if job["name"] == name:
                            job["conclusion"] = conclusion
                    self.assertRefused(self.run_promotion(shas, api, checkout), f"job '{name}'")

    def test_a_missing_verdict_job_builds(self):
        shas, api, checkout = self.promote()
        api.jobs["jobs"] = [j for j in api.jobs["jobs"] if j["name"] != "kernel / Kernel verdict"]
        api.jobs["total_count"] = len(api.jobs["jobs"])
        self.assertRefused(self.run_promotion(shas, api, checkout), "concluded absent")

    def test_a_truncated_job_list_builds(self):
        shas, api, checkout = self.promote()
        api.jobs["total_count"] = 101
        self.assertRefused(self.run_promotion(shas, api, checkout), "lists 101 jobs")


class ConditionTwoTest(Case):
    def test_a_kernel_tree_that_differs_from_the_runs_head_builds(self):
        shas, api, checkout = self.promote(pushed={"forge/specs/azoth/kernel-local": "CONFIG_X=y\n"})
        self.assertRefused(
            self.run_promotion(shas, api, checkout),
            "condition 2: forge/specs/azoth differs between the run's head",
        )

    def test_a_different_pr_yml_blob_builds(self):
        shas, api, checkout = self.promote(pushed={PR_YML: "pr 2\n"})
        self.assertRefused(self.run_promotion(shas, api, checkout), f"condition 2: {PR_YML} differs")

    def test_a_different_builder_script_builds(self):
        shas, api, checkout = self.promote(parent={"scripts/ci/build-builder.sh": "builder 2\n"})
        self.assertRefused(
            self.run_promotion(shas, api, checkout), "condition 2: scripts/ci/build-builder.sh differs"
        )

    def test_a_change_on_the_recorded_base_reverted_before_the_merge_builds(self):
        # The run built head merged with a base that held another call-kernel.yml; iso-v0
        # reverted it before the merge, so head and pushed agree and only the base side differs.
        shas, api, checkout = self.promote(
            base={CALL_KERNEL: "call-kernel X\n"},
            head={CALL_KERNEL: "call-kernel 1\n"},
            parent={CALL_KERNEL: "call-kernel 1\n"},
        )
        self.assertRefused(
            self.run_promotion(shas, api, checkout),
            f"condition 2: {CALL_KERNEL} differs between the base {shas['base']} recorded for the run",
        )

    def test_a_pull_request_moved_to_another_base_builds(self):
        # pr.yml does not run on `edited`: after a base change the green run is the one built
        # on the old base, and the base the pull request records is the new one.
        shas, api, checkout = self.promote()
        api.events.insert(1, {"created_at": "2026-10-07T12:00:00Z", "event": "base_ref_changed"})
        self.assertRefused(self.run_promotion(shas, api, checkout), "changed (base_ref_changed)")

    def test_a_base_history_longer_than_a_page_builds(self):
        shas, api, checkout = self.promote()
        api.events = api.events * 20
        self.assertRefused(self.run_promotion(shas, api, checkout), "more events than one page")

    def test_a_head_commit_git_cannot_fetch_builds(self):
        shas, api, checkout = self.promote()
        api.pulls[0]["head"]["sha"] = "0" * 40
        api.runs["workflow_runs"][0]["head_sha"] = "0" * 40
        api.shas = {**shas, "head": "0" * 40}
        self.assertRefused(self.run_promotion(shas, api, checkout), "error: CalledProcessError")


if __name__ == "__main__":
    unittest.main()
