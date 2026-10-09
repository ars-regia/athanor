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
import lzma
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
        artifacts["artifacts"][0]["size_in_bytes"] = pathlib.Path(zip_path).stat().st_size
        self.pulls, self.runs, self.jobs, self.artifacts = pulls, runs, fixture("jobs.json"), artifacts
        self.events = fixture("issue_events.json")
        self.head_pulls = [{"number": pulls[0]["number"]}]
        self.shas, self.zip_path = shas, zip_path
        self.error = None
        self.extra = {}

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
        answers.update(self.extra)
        if path not in answers:
            raise urllib.error.HTTPError(path, 404, "Not Found", {}, None)
        return copy.deepcopy(answers[path])

    def download(self, path, dest, limit):
        if path != f"/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT}/zip":
            raise urllib.error.HTTPError(path, 404, "Not Found", {}, None)
        if limit != pathlib.Path(self.zip_path).stat().st_size:
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

    def test_a_run_of_another_repository_builds(self):
        shas, api, checkout = self.promote()
        api.runs["workflow_runs"][0]["repository"]["full_name"] = "someone/athanor"
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 1: no completed")

    def test_the_newest_run_of_the_head_decides(self):
        # A re-run that failed after a green one: the older green run is never promoted.
        shas, api, checkout = self.promote()
        newer = copy.deepcopy(api.runs["workflow_runs"][0])
        newer["id"] = RUN + 1
        api.runs["workflow_runs"].append(newer)
        failed = copy.deepcopy(api.jobs)
        for job in failed["jobs"]:
            job["conclusion"] = "failure"
        api.extra[f"/repos/{REPOSITORY}/actions/runs/{RUN + 1}/jobs?filter=latest&per_page=100"] = failed
        self.assertRefused(self.run_promotion(shas, api, checkout), f"of run {RUN + 1} concluded")

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


class ConditionThreeTest(Case):
    def test_build_inputs_run_in_the_kernel_directory_of_each_commit(self):
        shas, _, checkout = self.promote()
        inputs = promotion.build_inputs(checkout, shas["pushed"], self.tmp / "inputs")
        self.assertEqual(inputs, {"pins": BUMP[PINS]})

    def test_build_inputs_that_differ_at_the_pushed_commit_build(self):
        # With the kernel directory equal (condition 2) the inputs agree by construction; the
        # check stands as the ADR states it, so its refusal is driven directly.
        shas, api, checkout = self.promote()
        with mock.patch.object(promotion, "build_inputs", side_effect=[{"pins": "a"}, {"pins": "b"}]):
            decision = self.run_promotion(shas, api, checkout)
        self.assertRefused(decision, "condition 3: build-inputs.py")

    def test_an_artifact_built_for_another_nvr_builds(self):
        shas, api, checkout = self.promote()
        self.assertRefused(
            self.run_promotion(shas, api, checkout, nvr="6.17.2-100.azoth.fc43"),
            f"holds NVR {NVR}, the pins give 6.17.2-100.azoth.fc43",
        )

    def test_an_artifact_without_nvr_builds(self):
        shas, api, checkout = self.promote()
        with zipfile.ZipFile(api.zip_path, "w") as archive:
            archive.writestr("kernel/kernel-core.rpm", b"rpm")
        api.artifacts["artifacts"][0]["digest"] = "sha256:" + sha256(api.zip_path)
        api.artifacts["artifacts"][0]["size_in_bytes"] = pathlib.Path(api.zip_path).stat().st_size
        self.assertRefused(self.run_promotion(shas, api, checkout), "holds no nvr")


class ConditionFourTest(Case):
    def test_a_digest_that_differs_from_the_api_builds(self):
        shas, api, checkout = self.promote()
        api.artifacts["artifacts"][0]["digest"] = "sha256:" + "0" * 64
        self.assertRefused(self.run_promotion(shas, api, checkout), "condition 4: artifact 11482498571 has digest")

    def test_an_artifact_without_a_digest_builds(self):
        shas, api, checkout = self.promote()
        api.artifacts["artifacts"][0]["digest"] = None
        self.assertRefused(self.run_promotion(shas, api, checkout), "reports no SHA-256 digest")

    def test_an_expired_artifact_builds(self):
        shas, api, checkout = self.promote()
        api.artifacts["artifacts"][0]["expired"] = True
        self.assertRefused(self.run_promotion(shas, api, checkout), "has expired")

    def test_a_missing_or_duplicated_artifact_builds(self):
        for artifacts in ([], "twice"):
            with self.subTest(artifacts=artifacts):
                self.tmp = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
                shas, api, checkout = self.promote()
                found = api.artifacts["artifacts"]
                api.artifacts["artifacts"] = found * 2 if artifacts == "twice" else []
                self.assertRefused(
                    self.run_promotion(shas, api, checkout), "artifacts named kernel-build, expected one"
                )

    def test_an_artifact_of_another_run_is_never_taken(self):
        shas, api, checkout = self.promote()
        api.artifacts["artifacts"][0]["workflow_run"]["id"] = RUN + 1
        self.assertRefused(self.run_promotion(shas, api, checkout), "has 0 artifacts")


    def test_an_artifact_of_another_name_in_the_run_is_ignored(self):
        shas, api, checkout = self.promote()
        other = copy.deepcopy(api.artifacts["artifacts"][0])
        other["name"], other["id"] = "kernel-promotion", ARTIFACT + 1
        api.artifacts["artifacts"].append(other)
        self.assertTrue(self.run_promotion(shas, api, checkout)["promoted"])


class PromotedTest(Case):
    def test_all_four_conditions_promote_and_unpack_the_artifact(self):
        shas, api, checkout = self.promote()
        decision = self.run_promotion(shas, api, checkout)
        self.assertTrue(decision["promoted"], decision)
        self.assertEqual((self.tmp / "out" / "nvr").read_text(), NVR + "\n")
        self.assertTrue((self.tmp / "out" / "kernel" / f"kernel-core-{NVR}.x86_64.rpm").is_file())

    def test_the_decision_names_the_source_run_the_trees_and_the_digest(self):
        shas, api, checkout = self.promote()
        decision = self.run_promotion(shas, api, checkout)
        self.assertEqual(decision["source_run"]["id"], RUN)
        self.assertEqual(decision["pull_request"]["number"], 270)
        self.assertEqual(decision["pull_request"]["merge_commit"], shas["pushed"])
        self.assertEqual(decision["pushed"], {"commit": shas["pushed"], "parent": shas["parent"]})
        self.assertEqual(decision["artifact"]["digest"], "sha256:" + sha256(self.tmp / "artifact.zip"))
        self.assertEqual(set(decision["trees"]), set(promotion.COMPARED))
        azoth = decision["trees"]["forge/specs/azoth"]
        self.assertEqual(azoth["head"], azoth["pushed"])
        self.assertEqual(azoth["base"], azoth["parent"])
        self.assertNotEqual(azoth["head"], azoth["base"])


class ErrorTest(Case):
    def test_an_api_error_builds(self):
        for error in (
            urllib.error.HTTPError("u", 502, "Bad Gateway", {}, None),
            urllib.error.URLError("timed out"),
            http.client.IncompleteRead(b"partial"),
            ValueError("not JSON"),
        ):
            with self.subTest(error=error):
                self.tmp = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
                shas, api, checkout = self.promote()
                api.error = error
                self.assertRefused(self.run_promotion(shas, api, checkout), "error: ")

    def test_an_error_of_any_library_builds(self):
        # A corrupt LZMA member raises lzma.LZMAError, which no list of zipfile errors names.
        shas, api, checkout = self.promote()
        api.error = lzma.LZMAError("Corrupt input data")
        self.assertRefused(self.run_promotion(shas, api, checkout), "error: LZMAError: Corrupt input data")

    def test_a_corrupt_member_whose_archive_digest_matches_builds(self):
        shas, api, checkout = self.promote()
        corrupt = self.tmp / "corrupt.zip"
        with zipfile.ZipFile(corrupt, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("nvr", NVR + "\n")
        data = bytearray(corrupt.read_bytes())
        with zipfile.ZipFile(corrupt) as archive:
            info = archive.getinfo("nvr")
        # The first deflate block header: BFINAL 1, BTYPE 11, a reserved type zlib refuses.
        data[info.header_offset + 30 + len(info.filename.encode()) + len(info.extra)] = 0x07
        corrupt.write_bytes(bytes(data))
        api.zip_path = corrupt
        api.artifacts["artifacts"][0]["digest"] = "sha256:" + sha256(corrupt)
        api.artifacts["artifacts"][0]["size_in_bytes"] = corrupt.stat().st_size
        self.assertRefused(self.run_promotion(shas, api, checkout), "error: ")

    def test_an_existing_extract_directory_builds(self):
        shas, api, checkout = self.promote()
        (self.tmp / "out").mkdir()
        decision = self.run_promotion(shas, api, checkout)
        self.assertFalse(decision["promoted"])
        self.assertIn("FileExistsError", decision["reason"])


class MainTest(Case):
    def test_main_writes_the_decision_and_the_summary_and_exits_zero(self):
        shas, api, checkout = self.promote()
        api.artifacts["artifacts"][0]["expired"] = True
        out = self.tmp / "promotion"
        env = {"GITHUB_REPOSITORY": REPOSITORY, "GH_TOKEN": "token"}
        with (
            mock.patch.dict(os.environ, env),
            mock.patch.object(promotion, "Api", lambda url, token: api),
            contextlib.redirect_stdout(io.StringIO()) as printed,
        ):
            status = promotion.main(
                ["--sha", shas["pushed"], "--nvr", NVR, "--out", str(out),
                 "--extract", str(self.tmp / "out"), "--repo", str(checkout)]
            )
        self.assertEqual(status, 0)
        self.assertIn("::notice title=Kernel promotion::not promoted", printed.getvalue())
        self.assertFalse(json.loads((out / "decision.json").read_text())["promoted"])
        self.assertIn("Not promoted, the push builds: condition 4", (out / "summary.md").read_text())

    def test_the_summary_of_a_promotion_lists_every_compared_path(self):
        shas, api, checkout = self.promote()
        text = promotion.summary(self.run_promotion(shas, api, checkout))
        self.assertIn(f"run [{RUN}]", text)
        for path in promotion.COMPARED:
            self.assertIn(f"| `{path}` |", text)


class DownloadTest(unittest.TestCase):
    def test_a_body_longer_than_the_reported_size_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest = pathlib.Path(tmp) / "a.zip"
            self.assertEqual(promotion._stream(io.BytesIO(b"12345"), dest, 5), hashlib.sha256(b"12345").hexdigest())
            with self.assertRaises(ValueError):
                promotion._stream(io.BytesIO(b"123456"), dest, 5)

    def test_the_token_goes_to_the_api_and_never_to_the_storage_it_redirects_to(self):
        body = b"artifact bytes"
        seen = {}

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                seen[self.path] = self.headers.get("Authorization")
                if self.path == "/api/bare":
                    self.send_response(302)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                elif self.path.startswith("/api/"):
                    self.send_response(302)
                    self.send_header("Location", f"http://127.0.0.1:{self.server.server_port}/blob")
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                else:
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

            def log_message(self, *args):
                pass

        server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.dict(os.environ, {"no_proxy": "*", "NO_PROXY": "*"}),
        ):
            api = promotion.Api(f"http://127.0.0.1:{server.server_port}/api", "secret")
            digest = api.download("/artifacts/1/zip", pathlib.Path(tmp) / "a.zip", len(body))
            self.assertEqual((pathlib.Path(tmp) / "a.zip").read_bytes(), body)
        self.assertEqual(digest, hashlib.sha256(body).hexdigest())
        self.assertEqual(seen["/api/artifacts/1/zip"], "Bearer secret")
        self.assertIsNone(seen["/blob"])
        # The JSON API does not follow a redirect at all, and a redirect without Location fails.
        seen.clear()
        with mock.patch.dict(os.environ, {"no_proxy": "*", "NO_PROXY": "*"}):
            with self.assertRaises(urllib.error.HTTPError):
                api.json("/repos/x")
            with tempfile.TemporaryDirectory() as tmp, self.assertRaises(ValueError):
                api.download("/bare", pathlib.Path(tmp) / "a.zip", len(body))
        self.assertNotIn("/blob", seen)


if __name__ == "__main__":
    unittest.main()
