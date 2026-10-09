# Kernel Artifact Promotion (ADR-0110) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (the tasks are sequential) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On a push to `iso-v0` whose kernel inputs changed, publish the RPMs that the merged pull request's `pr.yml` run already built, instead of building them again, when and only when the four conditions of ADR-0110 hold; otherwise build as today.

**Architecture:** One stdlib Python script, `scripts/ci/kernel_promotion.py`, decides. It reads only GitHub API metadata (the pull request merged as the pushed commit, its events, the `pr.yml` run of its head, the run's jobs and its `kernel-build` artifact) and git objects of the pushed checkout (the compared trees and blobs, and `build-inputs.py` run from `git archive` of each commit). The only bytes it takes from the source run are the artifact zip, whose SHA-256 it compares with the digest the API reports before it opens it. Every refusal, API error or git error yields `promoted: false` with a reason, exit status 0, and the push builds. The script runs as one step of the `inputs` job of `kernel-build.yml` (mirrored, inert, in `call-kernel.yml`); on a promotion that step unpacks the RPMs into `out/` and re-uploads them under the names the `build` job uses (`kernel-build`, `kernel-boot`, `kernel-devel`), so `boot`, `kmod` and `publish` consume them unchanged, and `build` is skipped. `publish` keeps every signing step as it is and adds one keyless attestation of the decision, with a predicate type of its own, on the four images.

**Tech Stack:** Python 3 standard library (`urllib`, `hashlib`, `zipfile`, `tarfile`, `subprocess`) with `unittest` and recorded API JSON fixtures; git; GitHub Actions YAML that only calls the script and moves artifacts; cosign keyless (unchanged identity); PyYAML for the workflow tests, as `scripts/ci/tests/test_call_kernel.py` already uses.

**Spec:** `docs/decisions/0110-kernel-artifact-promotion.md` (ADR-0110, accepted 2026-10-09); `docs/architecture/doc_kernel_build.md` section 7, paragraph "Promotion" (the four conditions are binding as written); `docs/architecture/doc_pipeline.md` PL44 and its 2026-10-09 amendment; `docs/spikes/kernel-build-reuse.md` design (d); `docs/architecture/doc_ci.md` CI8, CI27, CI28; `.github/workflows/AGENTS.md`.

**Base:** `origin/iso-v0` **after PR #368 merges** ("ci(kernel): build pull request kernels once, in pr.yml"). ADR-0110 item 2 requires #369 and #373, both merged (`2ad603bf`, `dccfdc49`); #368 is open at `3173ac8d`, which merges `iso-v0` at `ba4edd80` and so contains #373. This plan was written on `ba4edd80`; the workflow snippets below were checked against the tree `origin/iso-v0` + #368, where the `build` job runs `bash scripts/ci/build-builder.sh` (`kernel-build.yml:141`, `call-kernel.yml:106`). Before Task 1, check `git log --oneline origin/iso-v0 | grep -F '(#368)'` returns one line; if not, stop.

## Decisions this plan takes

The ADR leaves these open or assumes something the code or the API contradicts. Each is a reading, not a new rule. The maintainer confirmed decisions 1 and 11 on 2026-10-09; the others are readings of the ADR's text that the review of this plan checks.

1. **The PR build must not use the shared layer cache (contradiction).** ADR-0110 says (d) rests on #373, but `scripts/ci/build-builder.sh:8-9` keeps the runner's layer cache for `pull_request` runs (`[[ ${GITHUB_EVENT_NAME:-} != pull_request ]] || fresh=()`), and the PR run is the one promoted. Task 1 builds fresh for every event. Cost: one uncached builder image per PR kernel build (not measured). Confirmed by the maintainer on 2026-10-09.
2. **`scripts/ci/build-builder.sh` is a compared path.** The build job runs it (see Base) and it is outside `forge/specs/azoth`, `pr.yml` and `call-kernel.yml`, so a change to it between the run and the push would publish RPMs from a builder the merged code does not describe. The script compares it like the two workflow files. Stricter than the ADR's list.
3. **"The base commit recorded when the run started" is the pull request's `base.sha` (contradiction).** After the merge, the run record and its check suite have `pull_requests: []` (measured on run 37614092466 of PR #270), so the run itself no longer records a base. The pull request API's `base.sha` stays the base of the last synchronize: on #270 it was `5deaa55f`, the base that run actually built, while `iso-v0` had already moved to `4f1f2e2e`. Residual hole: `pr.yml` does not run on `edited`, so after a base change the green run was built on the old base. The script refuses when the pull request's events contain `base_ref_changed` or `base_ref_force_pushed`.
4. **"Likewise at the base commit" compares the recorded base with the pushed commit's first parent,** for every compared path. Comparing the base with the pushed commit would differ for every kernel change and never promote. The pair (head = pushed, base = parent) proves the run's merge result and the pushed commit agree on those paths.
5. **"`build-inputs.py` at the pushed commit equals the run's"** is `build-inputs.py` run from the run's `head_sha` tree, never the run's own `inputs.json` (forbidden by condition 2's last sentence). Since condition 2 already proved the kernel directory identical, this check is redundant by construction; it stays because the spec states it.
6. **`out/nvr` is read from the artifact only after its digest matched,** and can only refuse. It is the one value read from the run's bytes, as `publish` reads it today.
7. **"Its merge commit" is the pull request's `merge_commit_sha`,** which the script requires to equal the pushed commit (a squash merge).
8. **The predicate is a separate attestation** of type `<server>/<repository>/kernel-promotion/v1` holding `decision.json`. The existing `--type custom` pins attestation is untouched: the reuse check (`kernel-build.yml:94-99`) and `system/kernel-artifacts.sh:233-235` compare its predicate for exact equality with `build-inputs.py`, so adding fields there would end reuse and readiness.
9. **The token is the workflow's `GITHUB_TOKEN`** with `contents: read`, `actions: read` and `pull-requests: read` on the `inputs` job. `pull-requests: read` is needed by `GET /commits/{sha}/pulls` and the pull request's events, beyond the `actions: read` the ADR's wording implies. No new secret or variable.
10. **The promotion step lives in the `inputs` job.** `test_call_kernel.py` requires `inputs`, `build`, `boot` and `kmod` to be identical in both workflows and `call-kernel.yml` to have no other job but `verdict`, so a separate job is not possible; the step runs only on `push` to `refs/heads/iso-v0`, and `pr.yml`'s `kernel` job grants the two extra read scopes because GitHub validates a called workflow's permissions at start.
11. **The SLSA provenance step stays unchanged.** It names the push run as the builder of bits the PR run built. The ADR asks only that "the predicate" name the source run; the promotion attestation does. The maintainer confirmed on 2026-10-09 that the SLSA statement stays as it is and the separate promotion attestation carries the source run.

## Global Constraints

- English for code, comments, commit messages and documentation; Conventional Commits as in `git log` (`ci(kernel): ...`, `test(ci): ...`, `docs(ci): ...`); no AI attribution, model name, "Generated with" line or co-author trailer anywhere.
- **Stop and ask.** Task 7 adds a step to the `publish` job, which signs and attests; it changes no existing signing step, but it is a signing change: show the diff to the maintainer before the push. Task 9 edits the text of an approved specification (`doc_kernel_build.md` section 7): same rule. No task touches `forge/specs/azoth/keys/`, `system/keys/`, a signing environment or a signing secret.
- **YAML is glue** (`.github/workflows/AGENTS.md`): every decision lives in `scripts/ci/kernel_promotion.py`; the step only runs it, appends its `summary.md` to the job summary, copies `promoted` from `decision.json` to `$GITHUB_OUTPUT`, and moves artifacts. Data passes through the files `promotion/decision.json` and `promotion/summary.md`.
- **Inputs of the decision:** GitHub API metadata and git objects reachable from the pushed checkout (`git fetch` of commits the API names), never the run's outputs, logs or artifact contents, except the artifact's SHA-256, compared with the API's digest, and `out/nvr` after that comparison (decision 6).
- **A condition that fails, an API error or a git error means "build"**, never a red job: the script catches them, writes the reason to `summary.md` and the job summary, and exits 0. Only an error writing its own output files fails the step.
- No `|| true`, no `continue-on-error`, no new `2>/dev/null`. The existing `2> /dev/null` at `kernel-build.yml:97` (`call-kernel.yml:62`) is known debt, out of scope.
- `boot` and `kmod` run on the promoted RPMs; `publish` signs with the unchanged `iso-v0` identity; `repro` in `kernel-weekly.yml` is not touched.
- Tests use the recorded fixtures under `scripts/ci/tests/fixtures/kernel_promotion/` and local git repositories; no test reaches the network (the redirect test uses `http.server` on `127.0.0.1` with `no_proxy=*`).
- Markdown under `docs/` is edited only through an exact-match replacement script (Task 9), never through Edit or Write; then `git diff --numstat` shows only the intended lines.
- Never `cd`; repository-relative paths, `git -C`, `python3 -B -m unittest discover -s <dir>`. Scratch files in `.scratch/` or `/var/tmp`, never `/tmp`.
- Checks before every commit: `python3 -B -m unittest discover -s scripts/ci/tests`; for workflow changes also `actionlint` and `python3 scripts/verify.py workflows ci registry`; `bash -n` on a changed shell script; `just check` before the pull request.
- One pull request, one logical change per commit, in task order: every commit leaves the workflows consistent (Task 7 references an output that Task 8 creates; until then it is empty and `publish` behaves as today).
- **Unverified:** marks what this plan could not check locally; the task that relies on it says how it is checked.

## Review Focus

1. **A run that is not the merged pull request's own `pull_request` run of `pr.yml`** must never be promoted: another workflow, a `push` or `merge_group` event, a fork head, another repository, a run still in progress, a red or skipped `kernel / build` or `kernel / Kernel verdict`, a truncated job list, a head commit that another pull request shares. Tests in Task 2 (`ConditionOneTest`).
2. **A pushed tree that differs from what the run built** must rebuild, on the head side and on the base side, including a change on the recorded base that `iso-v0` reverted before the merge, and a pull request whose base was changed. Tests in Task 3 (`test_a_change_on_the_recorded_base_reverted_before_the_merge_builds`, `test_a_pull_request_moved_to_another_base_builds`, `test_a_different_builder_script_builds`).
3. **Artifact bytes are trusted only by digest:** a mismatch, a missing or non-SHA-256 digest, an expired, missing, duplicated or foreign artifact rebuilds; `out/nvr` is read only after the digest matched. Tests in Task 5.
4. **The token never leaves GitHub's API:** the artifact download follows the redirect to storage without the `Authorization` header, the JSON calls follow no redirect at all, and a truncated download or a redirect without `Location` rebuilds instead of failing the job. Test in Task 6 (`test_the_token_goes_to_the_api_and_never_to_the_storage_it_redirects_to`).
5. **The promotion is inert in pull requests and in `call-kernel.yml`:** the step's `if` requires `push` on `refs/heads/iso-v0`, and the mirror test still passes. Tests in Task 8.
6. **Signing is unchanged except for one added attestation** (Task 7, stop and ask). Check that `publish` still requires `boot` success, never runs on `pull_request`, and that the new step neither alters nor replaces the pins attestation.
7. **The builder of a PR build starts from the checkout alone** (Task 1).

---

### Task 1: Build the builder image fresh for every event

**Files:**
- Modify: `scripts/ci/build-builder.sh`
- Test: `scripts/ci/tests/test_build_builder.py`

**Interfaces:** `bash scripts/ci/build-builder.sh` (from the repository root) always runs `podman build --no-cache --pull=always -t localhost/azoth-builder -f forge/specs/azoth/builder/Containerfile forge/specs/azoth`.

- [ ] **Step 1: Write the failing test.** Replace the whole of `scripts/ci/tests/test_build_builder.py` with:

```python
"""scripts/ci/build-builder.sh: every event builds the builder without cached layers (ADR-0110)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "build-builder.sh"

# Stands in for podman: appends its arguments, one call per line, to $FAKE_LOG.
FAKE_PODMAN = """#!/usr/bin/env bash
echo "$*" >> "$FAKE_LOG"
"""


def podman_args(event):
    """The arguments the script gives podman when GITHUB_EVENT_NAME is EVENT (unset when None)."""
    with tempfile.TemporaryDirectory() as tmp:
        fake = pathlib.Path(tmp) / "podman"
        fake.write_text(FAKE_PODMAN)
        fake.chmod(0o755)
        log = pathlib.Path(tmp) / "log"
        env = {**os.environ, "PATH": f"{tmp}:{os.environ['PATH']}", "FAKE_LOG": str(log)}
        env.pop("GITHUB_EVENT_NAME", None)
        if event is not None:
            env["GITHUB_EVENT_NAME"] = event
        subprocess.run(["bash", str(SCRIPT)], env=env, check=True)
        return log.read_text().split()


class CacheTest(unittest.TestCase):
    def test_every_event_builds_fresh(self):
        # The pull request's build is promoted to the push (ADR-0110): it gets no cached layers.
        for event in ("pull_request", "push", "schedule", "workflow_dispatch", "merge_group", None):
            with self.subTest(event=event):
                args = podman_args(event)
                self.assertIn("--no-cache", args)
                self.assertIn("--pull=always", args)

    def test_the_image_is_the_one_the_workflows_run(self):
        self.assertIn("localhost/azoth-builder", podman_args("pull_request"))


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it and see it fail.** `python3 -B -m unittest scripts/ci/tests/test_build_builder.py` → FAIL in `test_every_event_builds_fresh` (subtest `event='pull_request'`: `--no-cache` not found).

- [ ] **Step 3: Implement.** Replace the whole of `scripts/ci/build-builder.sh` with:

```bash
#!/usr/bin/env bash
# Builds the kernel builder image on the self-hosted runner, for every event every layer again
# from a freshly pulled base, so a build starts from the checkout alone. The pull request's
# build is the one the push to iso-v0 may publish (ADR-0110), so it gets no cached layers either.
# Run from the repository root.
set -euo pipefail

podman build --no-cache --pull=always -t localhost/azoth-builder -f forge/specs/azoth/builder/Containerfile forge/specs/azoth
```

- [ ] **Step 4: Run the checks.** `bash -n scripts/ci/build-builder.sh && shellcheck scripts/ci/build-builder.sh && python3 -B -m unittest scripts/ci/tests/test_build_builder.py` → 2 tests OK.

- [ ] **Step 5: Commit.** `git add scripts/ci/build-builder.sh scripts/ci/tests/test_build_builder.py && git commit -m "ci(kernel): build the builder image without cached layers for pull requests too"`

---

### Task 2: Recorded fixtures, the test harness and condition 1

**Files:**
- Create: `scripts/ci/tests/fixtures/kernel_promotion/{pulls_for_commit,issue_events,runs,jobs,artifacts}.json`
- Create: `scripts/ci/kernel_promotion.py`
- Create: `scripts/ci/tests/test_kernel_promotion.py`

**Interfaces:**
- `Refused(Exception)`: a condition that does not hold.
- `source_run(api, repository, sha) -> (pull, run)`: the one pull request merged into `iso-v0` as `sha` (from `GET /repos/{r}/commits/{sha}/pulls`), whose head `GET /repos/{r}/commits/{head}/pulls` associates with that pull request alone, and the newest completed `pull_request` run of `.github/workflows/pr.yml` for its head in this repository, whose `kernel / build` and `kernel / Kernel verdict` jobs (`filter=latest`) each appear once with conclusion `success`.
- `decide(api, repo, repository, sha, nvr, tmp) -> dict` raises `Refused`; `promote(api, repo, repository, sha, nvr, extract) -> dict` never raises for a refusal or an `ERRORS` exception: it returns `{"promoted": False, "reason": str}`.
- `api` is any object with `json(path) -> object` and `download(path, dest) -> hex sha256` (the real `Api` comes in Task 6; tests use `FakeApi`).

The fixtures are the API answers for PR #270 (merged as `13017b4e`, base `5deaa55f`, head `5a19fdac`) and its `pr.yml` run 37614092466, recorded on 2026-10-09 with `gh api` and trimmed to the fields the script reads (the job list keeps all ten jobs). `pull_requests: []` in `runs.json` is as recorded: decision 3.

- [ ] **Step 1: Create the fixtures.**

`scripts/ci/tests/fixtures/kernel_promotion/pulls_for_commit.json`:

```json
[
  {
    "base": {
      "ref": "iso-v0",
      "sha": "5deaa55fe0e6a34f5b810471b501fb49a323051e"
    },
    "head": {
      "repo": {
        "full_name": "ars-regia/athanor"
      },
      "sha": "5a19fdaca21de3281114a3a3064c5f7474552c76"
    },
    "merge_commit_sha": "13017b4e89c98e85ad2b9f88f25d6403a580b14a",
    "merged_at": "2026-10-07T14:11:03Z",
    "number": 270,
    "state": "closed"
  }
]
```

`scripts/ci/tests/fixtures/kernel_promotion/issue_events.json`:

```json
[
  {
    "created_at": "2026-10-07T11:26:28Z",
    "event": "labeled"
  },
  {
    "created_at": "2026-10-07T11:26:30Z",
    "event": "auto_squash_enabled"
  },
  {
    "created_at": "2026-10-07T14:11:03Z",
    "event": "merged"
  },
  {
    "created_at": "2026-10-07T14:11:03Z",
    "event": "closed"
  },
  {
    "created_at": "2026-10-07T14:11:05Z",
    "event": "head_ref_deleted"
  }
]
```

`scripts/ci/tests/fixtures/kernel_promotion/runs.json`:

```json
{
  "total_count": 1,
  "workflow_runs": [
    {
      "conclusion": "success",
      "event": "pull_request",
      "head_repository": {
        "full_name": "ars-regia/athanor"
      },
      "head_sha": "5a19fdaca21de3281114a3a3064c5f7474552c76",
      "html_url": "https://github.com/ars-regia/athanor/actions/runs/37614092466",
      "id": 37614092466,
      "name": "Pull Request",
      "path": ".github/workflows/pr.yml",
      "pull_requests": [],
      "repository": {
        "full_name": "ars-regia/athanor"
      },
      "run_attempt": 1,
      "status": "completed"
    }
  ]
}
```

`scripts/ci/tests/fixtures/kernel_promotion/jobs.json`:

```json
{
  "total_count": 10,
  "jobs": [
    {"conclusion": "success", "id": 112768140482, "name": "changes", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112768140631, "name": "check", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112768205877, "name": "kernel / inputs", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "skipped", "id": 112768207297, "name": "specs", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112768271492, "name": "kernel / build", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112800040517, "name": "kernel / boot", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112800040792, "name": "kernel / kmod / build (open)", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112800040906, "name": "kernel / kmod / build (legacy)", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112801343883, "name": "kernel / Kernel verdict", "run_attempt": 1, "run_id": 37614092466, "status": "completed"},
    {"conclusion": "success", "id": 112801376984, "name": "gate", "run_attempt": 1, "run_id": 37614092466, "status": "completed"}
  ]
}
```

`scripts/ci/tests/fixtures/kernel_promotion/artifacts.json`:

```json
{
  "artifacts": [
    {
      "digest": "sha256:16cfb65ea00d7b28ba7463a587211f48e008beb8a80f6150d91896482e10d7f3",
      "expired": false,
      "id": 11482498571,
      "name": "kernel-build",
      "size_in_bytes": 1312013590,
      "workflow_run": {
        "head_branch": "bump/kernel-20261007-1126",
        "head_repository_id": 1309933336,
        "head_sha": "5a19fdaca21de3281114a3a3064c5f7474552c76",
        "id": 37614092466,
        "repository_id": 1309933336
      }
    }
  ],
  "total_count": 1
}
```

Check: `git check-ignore -q scripts/ci/tests/fixtures/kernel_promotion/runs.json; echo $?` prints `1` (not ignored).

- [ ] **Step 2: Write the harness and the condition-1 tests.** Create `scripts/ci/tests/test_kernel_promotion.py`:

```python
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


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 3: Run it and see it fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → ERROR, `FileNotFoundError` for `scripts/ci/kernel_promotion.py`.

- [ ] **Step 4: Implement condition 1.** Create `scripts/ci/kernel_promotion.py`:

```python
#!/usr/bin/env python3
"""Promotion of the merged pull request's kernel build to the push of iso-v0 (ADR-0110).

The push run of kernel-build.yml publishes the RPMs that the merged pull request's pr.yml run
built, instead of building them again, only when the four conditions of
docs/architecture/doc_kernel_build.md section 7 hold:

1. the run is this repository's pull_request run of pr.yml, found by the head commit of the
   pull request merged as the pushed commit, a head no other pull request shares, and its
   `kernel / build` and `kernel / Kernel verdict` jobs succeeded;
2. every COMPARED path has the same git object at the run's head and at the pushed commit, and
   at the base the pull request recorded and at the pushed commit's parent, and the pull
   request's base was never changed after it was opened;
3. build-inputs.py gives the same inputs at the pushed commit and at the run's head, and the
   artifact's `nvr` equals the NVR of the pins;
4. the `kernel-build` artifact, fetched by id, has the SHA-256 digest the API reports.

Every value compared comes from the GitHub API and from git objects named by the pushed
checkout or by the API; nothing the run wrote is read, except the artifact, whose `nvr` is
read only after its digest matched. A condition that does not hold, an API error or a git
error means "build": the decision is written and the exit status is 0.

Usage: kernel_promotion.py --sha SHA --nvr NVR --out DIR --extract DIR [--repo DIR]
Environment: GITHUB_REPOSITORY; GH_TOKEN, the workflow's GITHUB_TOKEN with actions: read and
pull-requests: read; GITHUB_API_URL (default https://api.github.com).
Writes DIR/decision.json and DIR/summary.md; when promoted, the artifact's files under the
--extract directory, which must not exist yet.
"""

import argparse
import hashlib
import http.client
import io
import json
import os
import pathlib
import re
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile

BRANCH = "iso-v0"
WORKFLOW = ".github/workflows/pr.yml"
JOBS = ("kernel / build", "kernel / Kernel verdict")
ARTIFACT = "kernel-build"
# What the pull request's kernel build reads from the repository: the kernel directory, the two
# workflow files of the run, and the builder script the build job runs (#373).
COMPARED = (
    "forge/specs/azoth",
    ".github/workflows/pr.yml",
    ".github/workflows/call-kernel.yml",
    "scripts/ci/build-builder.sh",
)
BUILD_INPUTS = "forge/specs/azoth/build-inputs.py"
BASE_EVENTS = ("base_ref_changed", "base_ref_force_pushed")
# Failures of the API, of git or of the artifact: each one means "build", never a red job.
ERRORS = (
    OSError,
    http.client.HTTPException,
    ValueError,
    KeyError,
    TypeError,
    subprocess.CalledProcessError,
    zipfile.BadZipFile,
    tarfile.TarError,
)


class Refused(Exception):
    """A condition that does not hold: the push builds."""


def git(repo, *args):
    return subprocess.run(
        ["git", "-C", str(repo), *args], check=True, capture_output=True, text=True
    ).stdout.strip()


def source_run(api, repository, sha):
    """Condition 1: the merged pull request of SHA and its pull_request run of pr.yml."""
    pulls = [
        p
        for p in api.json(f"/repos/{repository}/commits/{sha}/pulls")
        if p["merged_at"] and p["merge_commit_sha"] == sha and p["base"]["ref"] == BRANCH
    ]
    if len(pulls) != 1:
        raise Refused(
            f"condition 1: {len(pulls)} pull requests were merged into {BRANCH} as {sha}, expected one"
        )
    pull = pulls[0]
    head = pull["head"]["sha"]
    # A head shared with another pull request (a stack, a twin against main) may have a newer
    # green run built on another base: refuse rather than guess which run belongs to this one.
    sharing = [p["number"] for p in api.json(f"/repos/{repository}/commits/{head}/pulls")]
    if sharing != [pull["number"]]:
        raise Refused(
            f"condition 1: the head {head} belongs to pull requests {sharing}, expected only #{pull['number']}"
        )
    runs = [
        r
        for r in api.json(
            f"/repos/{repository}/actions/workflows/pr.yml/runs"
            f"?event=pull_request&head_sha={head}&per_page=100"
        )["workflow_runs"]
        if r["path"] == WORKFLOW
        and r["event"] == "pull_request"
        and r["head_sha"] == head
        and r["status"] == "completed"
        and r["repository"]["full_name"] == repository
        and r["head_repository"]["full_name"] == repository
    ]
    if not runs:
        raise Refused(
            f"condition 1: no completed pull_request run of {WORKFLOW} in {repository} for the head {head}"
        )
    run = max(runs, key=lambda r: r["id"])
    jobs = api.json(f"/repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")
    if jobs["total_count"] != len(jobs["jobs"]):
        raise Refused(f"condition 1: run {run['id']} lists {jobs['total_count']} jobs, read {len(jobs['jobs'])}")
    for name in JOBS:
        found = [j["conclusion"] for j in jobs["jobs"] if j["name"] == name]
        if found != ["success"]:
            raise Refused(f"condition 1: job '{name}' of run {run['id']} concluded {found or 'absent'}")
    return pull, run


def decide(api, repo, repository, sha, nvr, tmp):
    """The decision for a promotion; raises Refused when a condition does not hold."""
    tmp = pathlib.Path(tmp)
    pull, run = source_run(api, repository, sha)
    head, base = pull["head"]["sha"], pull["base"]["sha"]
    decision = {
        "promoted": True,
        "reason": "",
        "source_run": {
            "id": run["id"],
            "attempt": run["run_attempt"],
            "url": run["html_url"],
            "workflow": WORKFLOW,
            "event": "pull_request",
        },
        "pull_request": {"number": pull["number"], "head": head, "base": base, "merge_commit": sha},
    }
    return decision


def promote(api, repo, repository, sha, nvr, extract):
    """decide, then unpack the artifact into EXTRACT; any failure is a decision to build."""
    try:
        with tempfile.TemporaryDirectory() as tmp:
            return decide(api, repo, repository, sha, nvr, tmp)
    except Refused as refused:
        return {"promoted": False, "reason": str(refused)}
    except ERRORS as error:
        detail = getattr(error, "stderr", None) or ""
        if isinstance(detail, bytes):
            detail = detail.decode(errors="replace")
        return {"promoted": False, "reason": f"error: {type(error).__name__}: {error} {detail}".strip()}
```

- [ ] **Step 5: Run the tests.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → 10 tests OK. Then `python3 -B -m unittest discover -s scripts/ci/tests` → OK.

- [ ] **Step 6: Commit.** `git add scripts/ci/kernel_promotion.py scripts/ci/tests/test_kernel_promotion.py scripts/ci/tests/fixtures/kernel_promotion && git commit -m "ci(kernel): find the merged pull request's kernel run to promote (ADR-0110 condition 1)"`

---

### Task 3: Condition 2, the compared trees and the base

**Files:**
- Modify: `scripts/ci/kernel_promotion.py` (add `base_unchanged`, `compare_trees`; extend `decide`)
- Test: `scripts/ci/tests/test_kernel_promotion.py` (add `ConditionTwoTest`)

**Interfaces:**
- `base_unchanged(api, repository, number)`: refuses when `GET /repos/{r}/issues/{n}/events?per_page=100` returns 100 or more events (more than one page) or any event in `BASE_EVENTS`.
- `compare_trees(repo, sha, head, base) -> (parent, trees)`: fetches `sha` with depth 2 and `head`, `base` with depth 1 from `origin` into the checkout `repo`, takes `parent = sha^1`, and returns `trees = {path: {"head", "pushed", "base", "parent": object id}}` for every `COMPARED` path; refuses when `head` and `pushed`, or `base` and `parent`, differ for a path.

- [ ] **Step 1: Write the failing tests.** Insert before the final `if __name__ == "__main__":` of `scripts/ci/tests/test_kernel_promotion.py`:

```python
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
```

- [ ] **Step 2: Run them and see them fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → 7 FAIL in `ConditionTwoTest` (`True is not false`: nothing refuses yet).

- [ ] **Step 3: Implement.** In `scripts/ci/kernel_promotion.py`, insert after `source_run`:

```python
def base_unchanged(api, repository, number):
    """Condition 2: the recorded base is the run's only while nobody moved the pull request to
    another base; pr.yml does not run on `edited`, so such a change leaves the old run green."""
    events = api.json(f"/repos/{repository}/issues/{number}/events?per_page=100")
    if len(events) >= 100:
        raise Refused(
            f"condition 2: pull request #{number} has more events than one page, its base history is not read"
        )
    changed = [e["event"] for e in events if e["event"] in BASE_EVENTS]
    if changed:
        raise Refused(
            f"condition 2: the base of pull request #{number} changed ({changed[0]}), "
            "the base it records may not be the run's"
        )


def compare_trees(repo, sha, head, base):
    """Condition 2: the COMPARED objects at head and the pushed commit, at base and its parent."""
    git(repo, "fetch", "--no-tags", "--depth=2", "origin", sha)
    git(repo, "fetch", "--no-tags", "--depth=1", "origin", head, base)
    parent = git(repo, "rev-parse", f"{sha}^1")
    revs = {"head": head, "pushed": sha, "base": base, "parent": parent}
    trees = {}
    for path in COMPARED:
        ids = {name: git(repo, "rev-parse", f"{rev}:{path}") for name, rev in revs.items()}
        trees[path] = ids
        if ids["head"] != ids["pushed"]:
            raise Refused(f"condition 2: {path} differs between the run's head {head} and the pushed commit {sha}")
        if ids["base"] != ids["parent"]:
            raise Refused(
                f"condition 2: {path} differs between the base {base} recorded for the run "
                f"and the pushed commit's parent {parent}"
            )
    return parent, trees
```

In `decide`, insert before `    return decision`:

```python
    base_unchanged(api, repository, pull["number"])
    parent, decision["trees"] = compare_trees(repo, sha, head, base)
    decision["pushed"] = {"commit": sha, "parent": parent}
```

- [ ] **Step 4: Run the tests.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → 17 tests OK.

- [ ] **Step 5: Commit.** `git commit -am "ci(kernel): compare the run's trees with the pushed commit (ADR-0110 condition 2)"`

---

### Task 4: Condition 3, build-inputs.py at both commits

**Files:**
- Modify: `scripts/ci/kernel_promotion.py` (add `build_inputs`; extend `decide`)
- Test: `scripts/ci/tests/test_kernel_promotion.py` (add `ConditionThreeTest`)

**Interfaces:** `build_inputs(repo, rev, tmp) -> object`: `git archive <rev> forge/specs/azoth`, extracted under `tmp` with `tarfile` filter `"data"`, then that tree's own `build-inputs.py` run with `sys.executable -B`; returns its JSON. It runs only after condition 2 proved both kernel directories identical, so the code it runs is the pushed commit's. `decide` records `build_inputs_sha256`, the SHA-256 of the canonical JSON (sorted keys, no spaces).

- [ ] **Step 1: Write the failing tests.** Insert before the final `if __name__ == "__main__":`:

```python
class ConditionThreeTest(Case):
    def test_build_inputs_run_in_the_kernel_directory_of_each_commit(self):
        shas, api, checkout = self.promote()
        inputs = promotion.build_inputs(checkout, shas["pushed"], self.tmp / "inputs")
        self.assertEqual(inputs, {"pins": BUMP[PINS]})

    def test_build_inputs_that_differ_at_the_pushed_commit_build(self):
        # With the kernel directory equal (condition 2) the inputs agree by construction; the
        # check stands as the ADR states it, so its refusal is driven directly.
        shas, api, checkout = self.promote()
        with mock.patch.object(promotion, "build_inputs", side_effect=[{"pins": "a"}, {"pins": "b"}]):
            decision = self.run_promotion(shas, api, checkout)
        self.assertRefused(decision, "condition 3: build-inputs.py")
```

- [ ] **Step 2: Run them and see them fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → ERROR `AttributeError: ... has no attribute 'build_inputs'` (both tests).

- [ ] **Step 3: Implement.** Insert after `compare_trees`:

```python
def build_inputs(repo, rev, tmp):
    """The JSON build-inputs.py prints in the kernel directory of REV."""
    archive = subprocess.run(
        ["git", "-C", str(repo), "archive", rev, "forge/specs/azoth"], check=True, capture_output=True
    ).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(tmp, filter="data")
    out = subprocess.run(
        [sys.executable, "-B", str(pathlib.Path(tmp) / BUILD_INPUTS)], check=True, capture_output=True, text=True
    ).stdout
    return json.loads(out)
```

In `decide`, insert before `    return decision`:

```python
    inputs = build_inputs(repo, sha, tmp / "pushed")
    if build_inputs(repo, head, tmp / "head") != inputs:
        raise Refused("condition 3: build-inputs.py gives other inputs at the run's head than at the pushed commit")
    canonical = json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode()
    decision["build_inputs_sha256"] = hashlib.sha256(canonical).hexdigest()
```

- [ ] **Step 4: Run the tests.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → 19 tests OK. Also run the real `build-inputs.py` through the function once, to prove it needs nothing outside its directory: `python3 -B -c 'import importlib.util as u, tempfile; s=u.spec_from_file_location("p","scripts/ci/kernel_promotion.py"); m=u.module_from_spec(s); s.loader.exec_module(m); print(sorted(m.build_inputs(".", "HEAD", tempfile.mkdtemp(dir="/var/tmp"))))'` → the same keys as `python3 forge/specs/azoth/build-inputs.py | python3 -c 'import json,sys; print(sorted(json.load(sys.stdin)))'`. **Unverified:** that `build-inputs.py` reads only files under `forge/specs/azoth` (read in its source on `ba4edd80`); this step checks it.

- [ ] **Step 5: Commit.** `git commit -am "ci(kernel): compare build-inputs.py at the run's head and the pushed commit (ADR-0110 condition 3)"`

---

### Task 5: Condition 4, the artifact by digest, the NVR and the extraction

**Files:**
- Modify: `scripts/ci/kernel_promotion.py` (add `fetch_artifact`; extend `decide` and `promote`)
- Test: `scripts/ci/tests/test_kernel_promotion.py` (extend `ConditionThreeTest`; add `ConditionFourTest`, `PromotedTest`, `ErrorTest`)

**Interfaces:**
- `fetch_artifact(api, repository, run, dest) -> artifact`: lists `GET /repos/{r}/actions/runs/{id}/artifacts?name=kernel-build&per_page=100`, requires exactly one entry named `kernel-build` whose `workflow_run.id` is the run's, not expired, with a digest matching `sha256:[0-9a-f]{64}`; downloads `GET /repos/{r}/actions/artifacts/{id}/zip` to `dest` through `api.download`, and refuses unless `"sha256:" + download` equals the API digest.
- `decide` then reads `nvr` from the verified zip and refuses unless it equals `--nvr`; `promote` creates `extract` (failing if it exists) and unpacks the zip there.
- The decision of a promotion (`decision.json`, the predicate of Task 7):

```json
{
  "promoted": true,
  "reason": "",
  "source_run": {"id": 0, "attempt": 0, "url": "", "workflow": ".github/workflows/pr.yml", "event": "pull_request"},
  "pull_request": {"number": 0, "head": "<sha>", "base": "<sha>", "merge_commit": "<pushed sha>"},
  "trees": {"<path>": {"head": "<oid>", "pushed": "<oid>", "base": "<oid>", "parent": "<oid>"}},
  "pushed": {"commit": "<sha>", "parent": "<sha>"},
  "build_inputs_sha256": "<hex>",
  "artifact": {"id": 0, "name": "kernel-build", "digest": "sha256:<hex>"},
  "nvr": "<nvr>"
}
```

- [ ] **Step 1: Write the failing tests.** Append to `ConditionThreeTest`:

```python
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
        self.assertRefused(self.run_promotion(shas, api, checkout), "holds no nvr")
```

and insert before the final `if __name__ == "__main__":`:

```python
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

    def test_an_existing_extract_directory_builds(self):
        shas, api, checkout = self.promote()
        (self.tmp / "out").mkdir()
        decision = self.run_promotion(shas, api, checkout)
        self.assertFalse(decision["promoted"])
        self.assertIn("FileExistsError", decision["reason"])
```

- [ ] **Step 2: Run them and see them fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → the condition-4, NVR, promoted and extract tests FAIL (nothing reads the artifact yet); `test_an_api_error_builds` already passes.

- [ ] **Step 3: Implement.** Insert after `build_inputs`:

```python
def fetch_artifact(api, repository, run, dest):
    """Condition 4: the one kernel-build artifact of RUN, downloaded by id, digest checked."""
    listing = api.json(f"/repos/{repository}/actions/runs/{run['id']}/artifacts?name={ARTIFACT}&per_page=100")
    found = [a for a in listing["artifacts"] if a["name"] == ARTIFACT and a["workflow_run"]["id"] == run["id"]]
    # ponytail: a run whose build job ran twice has two kernel-build artifacts and builds again;
    # pick by the latest attempt if re-runs make that common.
    if len(found) != 1:
        raise Refused(f"condition 4: run {run['id']} has {len(found)} artifacts named {ARTIFACT}, expected one")
    artifact = found[0]
    if artifact["expired"]:
        raise Refused(f"condition 4: artifact {artifact['id']} of run {run['id']} has expired")
    expected = artifact.get("digest") or ""
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", expected):
        raise Refused(f"condition 4: the API reports no SHA-256 digest for artifact {artifact['id']}")
    actual = "sha256:" + api.download(f"/repos/{repository}/actions/artifacts/{artifact['id']}/zip", dest)
    if actual != expected:
        raise Refused(f"condition 4: artifact {artifact['id']} has digest {actual}, the API reports {expected}")
    return artifact
```

In `decide`, insert before `    return decision`:

```python
    artifact = fetch_artifact(api, repository, run, tmp / "artifact.zip")
    decision["artifact"] = {"id": artifact["id"], "name": ARTIFACT, "digest": artifact["digest"]}
    # Condition 3, second half: read from the artifact only now that its digest matched.
    with zipfile.ZipFile(tmp / "artifact.zip") as archive:
        if "nvr" not in archive.namelist():
            raise Refused(f"condition 3: artifact {artifact['id']} holds no nvr")
        built = archive.read("nvr").decode().strip()
    if built != nvr:
        raise Refused(f"condition 3: artifact {artifact['id']} holds NVR {built}, the pins give {nvr}")
    decision["nvr"] = nvr
```

In `promote`, replace the line `            return decide(api, repo, repository, sha, nvr, tmp)` with:

```python
            decision = decide(api, repo, repository, sha, nvr, tmp)
            pathlib.Path(extract).mkdir(parents=True)
            with zipfile.ZipFile(pathlib.Path(tmp) / "artifact.zip") as archive:
                archive.extractall(extract)
            return decision
```

- [ ] **Step 4: Run the tests.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → 30 tests OK.

- [ ] **Step 5: Commit.** `git commit -am "ci(kernel): take the run's artifact only by its API digest (ADR-0110 condition 4)"`

---

### Task 6: The API client, the summary and the command line

**Files:**
- Modify: `scripts/ci/kernel_promotion.py` (add `_NoRedirect`, `_stream`, `Api`, `summary`, `main`)
- Test: `scripts/ci/tests/test_kernel_promotion.py` (add `MainTest`, `DownloadTest`)

**Interfaces:**
- `Api(url, token)`: `json(path)` sends `Authorization: Bearer <token>`, `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`; `download(path, dest)` streams to `dest` and returns the SHA-256 hex, and follows a 301/302/303/307/308 to the `Location` without the token (artifact downloads redirect to storage outside GitHub).
- `summary(decision) -> str` (Markdown for `$GITHUB_STEP_SUMMARY`).
- `main(argv)`: `--sha --nvr --out --extract [--repo .]`, environment `GITHUB_REPOSITORY`, `GH_TOKEN`, `GITHUB_API_URL`; writes `<out>/decision.json` and `<out>/summary.md`, prints one `::notice title=Kernel promotion::` line, returns 0.

- [ ] **Step 1: Write the failing tests.** Insert before the final `if __name__ == "__main__":`:

```python
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
            digest = api.download("/artifacts/1/zip", pathlib.Path(tmp) / "a.zip")
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
                api.download("/bare", pathlib.Path(tmp) / "a.zip")
        self.assertNotIn("/blob", seen)
```

- [ ] **Step 2: Run them and see them fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → ERROR `AttributeError` for `Api`, `main` and `summary` (4 tests).

- [ ] **Step 3: Implement.** Insert after `class Refused`:

```python
class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def _stream(response, dest):
    digest = hashlib.sha256()
    with open(dest, "wb") as out:
        while chunk := response.read(1 << 20):
            digest.update(chunk)
            out.write(chunk)
    return digest.hexdigest()


class Api:
    """The REST API with the workflow's token."""

    def __init__(self, url, token):
        self.url = url.rstrip("/")
        self.headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
        }

    def json(self, path):
        request = urllib.request.Request(self.url + path, headers=self.headers)
        # No redirect: the token is sent to the API host only, and a redirect fails as an HTTPError.
        with urllib.request.build_opener(_NoRedirect).open(request, timeout=60) as response:
            return json.load(response)

    def download(self, path, dest):
        """Stream PATH to DEST and return its SHA-256 in hex. The API redirects an artifact to
        storage outside GitHub: the redirect is followed without the token."""
        request = urllib.request.Request(self.url + path, headers=self.headers)
        try:
            with urllib.request.build_opener(_NoRedirect).open(request, timeout=60) as response:
                return _stream(response, dest)
        except urllib.error.HTTPError as error:
            if error.code not in (301, 302, 303, 307, 308):
                raise
            location = error.headers["Location"]
            error.close()
            if not location:
                raise ValueError(f"{path}: redirect without Location")
        with urllib.request.build_opener().open(location, timeout=60) as response:
            return _stream(response, dest)
```

Append at the end of the file:

```python
def summary(decision):
    lines = ["### Kernel promotion (ADR-0110)", ""]
    if not decision["promoted"]:
        return "\n".join([*lines, f"Not promoted, the push builds: {decision['reason']}", ""])
    run, pull, artifact = decision["source_run"], decision["pull_request"], decision["artifact"]
    lines += [
        f"Promoted: the RPMs of run [{run['id']}]({run['url']}) of pull request #{pull['number']} "
        f"(head `{pull['head']}`, recorded base `{pull['base']}`), artifact {artifact['id']} "
        f"`{artifact['digest']}`. This push does not build the kernel.",
        "",
        "| Path | Run's head | Pushed commit | Recorded base | Pushed commit's parent |",
        "| --- | --- | --- | --- | --- |",
    ]
    for path, ids in decision["trees"].items():
        lines.append(f"| `{path}` | `{ids['head']}` | `{ids['pushed']}` | `{ids['base']}` | `{ids['parent']}` |")
    return "\n".join([*lines, ""])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--sha", required=True)
    parser.add_argument("--nvr", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--extract", required=True)
    parser.add_argument("--repo", default=".")
    args = parser.parse_args(argv)
    api = Api(os.environ.get("GITHUB_API_URL", "https://api.github.com"), os.environ.get("GH_TOKEN", ""))
    decision = promote(api, args.repo, os.environ.get("GITHUB_REPOSITORY", ""), args.sha, args.nvr, args.extract)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "decision.json").write_text(json.dumps(decision, indent=2, sort_keys=True) + "\n")
    (out / "summary.md").write_text(summary(decision))
    outcome = "promoted" if decision["promoted"] else f"not promoted, the push builds: {decision['reason']}"
    print(f"::notice title=Kernel promotion::{outcome}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run the tests.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion.py` → 33 tests OK (measured on a prototype of this plan: 33 tests in about 5 s). `python3 -B -m unittest discover -s scripts/ci/tests` → OK. `python3 -B scripts/ci/kernel_promotion.py --help` prints the usage.

- [ ] **Step 5: Commit.** `git commit -am "ci(kernel): write the promotion decision and summary from the command line"`

---

### Task 7: `publish` accepts promoted RPMs and attests the decision (STOP AND ASK)

**STOP AND ASK: show the diff to the maintainer before the push.** This task adds a step to the job that signs. It changes no existing step of `publish`: `Publish the four OCI images` (including its `out/nvr` check), `SBOM, sign, attest`, `Provenance SLSA (kernel)`, `Retention (retention.sh)` and `Verify (gate K2)` stay byte for byte.

**Files:**
- Modify: `.github/workflows/kernel-build.yml` (job `publish` only)
- Create: `scripts/ci/tests/test_kernel_promotion_workflow.py`

**Interfaces:** Consumes `needs.inputs.outputs.promoted` (`'true'` or empty until Task 8) and the run artifact `kernel-promotion` (`decision.json`, Task 8). Produces, on a promotion only, a keyless in-toto attestation of predicate type `${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/kernel-promotion/v1` on each of the four image digests, signed by the same workflow identity as the other attestations and verified right after.

- [ ] **Step 1: Write the failing test.** Create `scripts/ci/tests/test_kernel_promotion_workflow.py`:

```python
"""The promotion of ADR-0110 is wired into kernel-build.yml and stays inert in pull requests."""

import pathlib
import unittest

import yaml

WORKFLOWS = pathlib.Path(__file__).resolve().parents[3] / ".github" / "workflows"
PROMOTED = "needs.inputs.outputs.promoted == 'true'"


def jobs(name):
    return yaml.safe_load((WORKFLOWS / name).read_text())["jobs"]


class PublishTest(unittest.TestCase):
    def setUp(self):
        self.publish = jobs("kernel-build.yml")["publish"]

    def test_publish_accepts_a_skipped_build_only_when_promoted(self):
        self.assertIn(f"(needs.build.result == 'skipped' && {PROMOTED})", self.publish["if"])
        self.assertIn("needs.boot.result == 'success'", self.publish["if"])
        self.assertIn("github.event_name != 'pull_request'", self.publish["if"])

    def test_publish_attests_the_decision_with_its_own_predicate_type(self):
        names = [s.get("name") for s in self.publish["steps"]]
        attest = names.index("Attest the promotion (ADR-0110)")
        self.assertLess(names.index("SBOM, sign, attest"), attest)
        self.assertLess(attest, names.index("Retention (retention.sh)"))
        step = self.publish["steps"][attest]
        self.assertEqual(step["if"], f"${{{{ {PROMOTED} }}}}")
        self.assertIn('--type "$type" --predicate promotion/decision.json', step["run"])
        self.assertNotIn("--type custom", step["run"])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it and see it fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion_workflow.py` → 1 FAIL (`if`), 1 ERROR (`ValueError: 'Attest the promotion (ADR-0110)' is not in list`).

- [ ] **Step 3: Implement.** In `.github/workflows/kernel-build.yml`, job `publish`, replace the end of the `if:` line

```yaml
&& needs.build.result == 'success' && needs.boot.result == 'success' }}
```

with

```yaml
&& (needs.build.result == 'success' || (needs.build.result == 'skipped' && needs.inputs.outputs.promoted == 'true')) && needs.boot.result == 'success' }}
```

and insert, between the `env:` block that ends `SBOM, sign, attest` (`SYFT_REGISTRY_AUTH_PASSWORD: ${{ secrets.GITHUB_TOKEN }}`) and `- name: Provenance SLSA (kernel)`:

```yaml
      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        if: ${{ needs.inputs.outputs.promoted == 'true' }}
        with:
          name: kernel-promotion
          path: promotion

      - name: Attest the promotion (ADR-0110)
        # The RPMs come from the merged pull request's run: a predicate of its own type names
        # the source run, the merge commit, the compared trees and the artifact digest
        # (scripts/ci/kernel_promotion.py). The pins attestation above stays as it is, because
        # the reuse check of the inputs job and system/kernel-artifacts.sh match it exactly.
        if: ${{ needs.inputs.outputs.promoted == 'true' }}
        run: |
          set -euo pipefail
          type="${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/kernel-promotion/v1"
          identity=$(bash system/kernel-artifacts.sh identity kernel)
          for group in kernel devel debuginfo microvm; do
            repo=$IMAGE
            [[ $group == kernel || $group == microvm ]] || repo="${IMAGE}-${group}"
            ref="${repo}@$(cat "digests/${group}")"
            cosign attest --yes --type "$type" --predicate promotion/decision.json "$ref"
            cosign verify-attestation --type "$type" \
              --certificate-identity-regexp "$identity" \
              --certificate-oidc-issuer https://token.actions.githubusercontent.com "$ref" > /dev/null
          done
```

The loop repeats the image reference rule of `SBOM, sign, attest`; `cosign login` from that step is still in effect. **Unverified:** that `retention.sh` keeps an attestation of a custom URI type like the `spdxjson` and `custom` ones (it keeps "what is reachable from" a release); Task 10 (A) verifies it after the run.

- [ ] **Step 4: Run the checks.** `python3 -B -m unittest discover -s scripts/ci/tests` → OK (the mirror test is unaffected: `publish` is not mirrored). `actionlint .github/workflows/kernel-build.yml` → no output. `python3 scripts/verify.py workflows ci registry` → PASS. `git diff -U0 .github/workflows/kernel-build.yml | grep '^-' | grep -v '^---'` prints only the old `if:` line of `publish`.

- [ ] **Step 5: Show the diff to the maintainer** (`git diff .github/workflows/kernel-build.yml`) and wait for an answer in the conversation. Do not push before it.

- [ ] **Step 6: Commit.** `git add .github/workflows/kernel-build.yml scripts/ci/tests/test_kernel_promotion_workflow.py && git commit -m "ci(kernel): publish promoted RPMs with an attestation of the promotion"`

---

### Task 8: The promotion step, and the jobs that follow it

**Files:**
- Modify: `.github/workflows/kernel-build.yml` (jobs `inputs`, `build`, `boot`, `kmod`, `gate`)
- Modify: `.github/workflows/call-kernel.yml` (jobs `inputs`, `build`, `boot`, `kmod`: the same text, so `test_call_kernel.py` stays green)
- Modify: `.github/workflows/pr.yml` (job `kernel`: permissions)
- Test: `scripts/ci/tests/test_kernel_promotion_workflow.py` (add `InputsTest`, `DownstreamTest`)

**Interfaces:**
- `inputs` gains job permissions `contents: read`, `actions: read`, `pull-requests: read` (it had the workflow's `contents: read` only), `timeout-minutes: 30` (was 10; a promotion moves about 1.3 GB twice, an estimate), the output `promoted` (`'true'`, `'false'`, or empty when the step is skipped), the step `promotion`, and four uploads that run only when `promoted == 'true'`: `kernel-build` (`out/`), `kernel-boot`, `kernel-devel` (the build job's paths) and `kernel-promotion` (`promotion/decision.json`).
- `build` is skipped when `promoted == 'true'`; `boot`, `kmod` run when `build` succeeded, reuse holds, or `promoted == 'true'` (they already download `kernel-boot` and `kernel-devel` whenever reuse is not true); `Kernel gate` accepts a skipped `build` when reuse or promotion holds. `Kernel verdict` of `call-kernel.yml` is unchanged: the step never runs there.
- The token is `secrets.GITHUB_TOKEN`; no secret or variable is added.

- [ ] **Step 1: Write the failing tests.** Insert before the final `if __name__ == "__main__":` of `scripts/ci/tests/test_kernel_promotion_workflow.py`:

```python
class InputsTest(unittest.TestCase):
    def setUp(self):
        self.inputs = jobs("kernel-build.yml")["inputs"]
        self.promotion = next(s for s in self.inputs["steps"] if s.get("id") == "promotion")

    def test_only_a_push_to_iso_v0_runs_the_promotion(self):
        condition = self.promotion["if"]
        self.assertIn("github.event_name == 'push'", condition)
        self.assertIn("github.ref == 'refs/heads/iso-v0'", condition)
        self.assertIn("steps.key.outputs.reuse != 'true'", condition)
        self.assertIn("python3 scripts/ci/kernel_promotion.py", self.promotion["run"])

    def test_the_token_only_reads(self):
        self.assertEqual(
            self.inputs["permissions"],
            {"contents": "read", "actions": "read", "pull-requests": "read"},
        )
        self.assertEqual(self.promotion["env"]["GH_TOKEN"], "${{ secrets.GITHUB_TOKEN }}")

    def test_the_promoted_rpms_take_the_build_jobs_artifact_names(self):
        # boot, kmod (devel-artifact) and publish download these names from the run.
        promoted = {
            s["with"]["name"]
            for s in self.inputs["steps"]
            if "upload-artifact" in s.get("uses", "")
            and s["if"] == "${{ steps.promotion.outputs.promoted == 'true' }}"
        }
        self.assertEqual(promoted, {"kernel-build", "kernel-boot", "kernel-devel", "kernel-promotion"})

    def test_pr_yml_grants_what_the_mirrored_inputs_job_asks(self):
        granted = jobs("pr.yml")["kernel"]["permissions"]
        for scope, level in self.inputs["permissions"].items():
            with self.subTest(scope=scope):
                self.assertEqual(granted.get(scope), level)


class DownstreamTest(unittest.TestCase):
    def setUp(self):
        self.jobs = jobs("kernel-build.yml")

    def test_a_promotion_skips_the_build(self):
        self.assertIn("needs.inputs.outputs.promoted != 'true'", self.jobs["build"]["if"])

    def test_boot_and_kmod_run_on_the_promoted_rpms(self):
        for name in ("boot", "kmod"):
            with self.subTest(job=name):
                self.assertIn(PROMOTED, self.jobs[name]["if"])

    def test_the_gate_accepts_a_skipped_build_when_promoted(self):
        gate = self.jobs["gate"]
        self.assertEqual(gate["env"]["PROMOTED"], "${{ needs.inputs.outputs.promoted }}")
        self.assertIn("$PROMOTED == true", gate["steps"][0]["run"])
```

- [ ] **Step 2: Run them and see them fail.** `python3 -B -m unittest scripts/ci/tests/test_kernel_promotion_workflow.py` → `InputsTest` ERROR (`StopIteration`: no step `promotion`), `DownstreamTest` FAIL.

- [ ] **Step 3: Implement, in both `kernel-build.yml` and `call-kernel.yml`** (each replacement below occurs exactly once per file, the last one twice; apply them with one exact-match script over both files so they stay identical, then check `git diff --stat`).

(a) The head of the `inputs` job. Replace

```yaml
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    outputs:
      nvr: ${{ steps.key.outputs.nvr }}
```

with

```yaml
    runs-on: ubuntu-24.04
    # A promotion downloads the 1.3 GB of RPMs and uploads them again (30 minutes: an estimate).
    timeout-minutes: 30
    # actions and pull-requests: kernel_promotion.py reads the merged pull request, its events,
    # and the run, jobs and artifact it is promoted from (ADR-0110). pr.yml's kernel job grants
    # the same, since this job is mirrored in call-kernel.yml.
    permissions:
      contents: read
      actions: read
      pull-requests: read
    outputs:
      nvr: ${{ steps.key.outputs.nvr }}
      promoted: ${{ steps.promotion.outputs.promoted }}
```

(b) The end of the `inputs` job: insert after the last line of the step `Digest of the reused kernel and kernel-devel (system/kernel-artifacts.sh)` (its `grep -E '^(kernel_digest|devel_digest|registry)=' ...` line), before the blank line and `  build:`:

```yaml

      - name: Promote the pull request's build (scripts/ci/kernel_promotion.py, ADR-0110)
        # Only on the push to iso-v0 when the inputs changed: when the four conditions of
        # doc_kernel_build.md section 7 hold, the RPMs of the merged pull request's pr.yml run go
        # into out/ and build is skipped. A condition that does not hold, or an API error, leaves
        # promoted false with the reason in the summary, and build runs as before.
        id: promotion
        if: ${{ github.event_name == 'push' && github.ref == 'refs/heads/iso-v0' && (inputs.stage || 'build') == 'build' && steps.key.outputs.reuse != 'true' }}
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          NVR: ${{ steps.key.outputs.nvr }}
        run: |
          set -euo pipefail
          python3 scripts/ci/kernel_promotion.py --sha "$GITHUB_SHA" --nvr "$NVR" --out promotion --extract out
          cat promotion/summary.md >> "$GITHUB_STEP_SUMMARY"
          echo "promoted=$(jq -r .promoted promotion/decision.json)" >> "$GITHUB_OUTPUT"

      - name: The promoted RPMs, under the build job's names
        if: ${{ steps.promotion.outputs.promoted == 'true' }}
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-build
          path: out/
          if-no-files-found: error

      - name: Artifact for the boot matrix (promoted)
        if: ${{ steps.promotion.outputs.promoted == 'true' }}
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-boot
          path: |
            out/kernel/kernel-core-*.rpm
            out/microvm/*.rpm
          if-no-files-found: error

      - name: Artifact for the NVIDIA modules (promoted)
        if: ${{ steps.promotion.outputs.promoted == 'true' }}
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-devel
          path: out/devel/kernel-devel-[0-9]*.rpm
          if-no-files-found: error

      - name: The promotion decision, which publish attests
        if: ${{ steps.promotion.outputs.promoted == 'true' }}
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-promotion
          path: promotion/decision.json
          if-no-files-found: error
```

(c) The `if:` of `build`: replace `((inputs.stage || 'build') == 'prep' || needs.inputs.outputs.reuse != 'true') }}` with `((inputs.stage || 'build') == 'prep' || (needs.inputs.outputs.reuse != 'true' && needs.inputs.outputs.promoted != 'true')) }}`.

(d) The `if:` of `boot` and of `kmod` (two occurrences per file): replace `(needs.build.result == 'success' || needs.inputs.outputs.reuse == 'true') }}` with `(needs.build.result == 'success' || needs.inputs.outputs.reuse == 'true' || needs.inputs.outputs.promoted == 'true') }}`.

**In `kernel-build.yml` only**, job `gate`: after `      REUSE: ${{ needs.inputs.outputs.reuse }}` add `      PROMOTED: ${{ needs.inputs.outputs.promoted }}`, and replace

```bash
          [[ $(result build) == success || ( $(result build) == skipped && $REUSE == true ) ]]
```

with

```bash
          [[ $(result build) == success || ( $(result build) == skipped && ( $REUSE == true || $PROMOTED == true ) ) ]]
```

**In `pr.yml`**, job `kernel`: replace

```yaml
    permissions:
      contents: read
      packages: read
    uses: ./.github/workflows/call-kernel.yml
```

with

```yaml
    # actions and pull-requests: the inputs job of call-kernel.yml asks for them, because it
    # mirrors kernel-build.yml's, whose promotion step reads the API on a push (ADR-0110).
    # GitHub validates a called workflow's permissions when the run starts.
    permissions:
      contents: read
      packages: read
      actions: read
      pull-requests: read
    uses: ./.github/workflows/call-kernel.yml
```

The `orchestrator` job needs no change: on a promotion `publish` succeeds and it dispatches as after a build. **Unverified:** that `GET /repos/{r}/issues/{n}/events` and `GET /repos/{r}/commits/{sha}/pulls` answer with `pull-requests: read` and no `issues` scope (GitHub documents both under the pull-request permission for fine-grained tokens); if not, the step reports `error: HTTPError: HTTP Error 403` and the push builds, which Task 10 (A) shows.

- [ ] **Step 4: Run the checks.** `python3 -B -m unittest discover -s scripts/ci/tests` → OK, including `test_call_kernel.py` (the mirror) and `test_pr_workflow.py`. `actionlint` → no output. `python3 scripts/verify.py workflows ci registry` → PASS. On a prototype of this plan built on the #368 + #373 tree, the whole `scripts/ci/tests` directory passed (85 tests) and `actionlint` was clean.

- [ ] **Step 5: Commit.** `git add .github/workflows/kernel-build.yml .github/workflows/call-kernel.yml .github/workflows/pr.yml scripts/ci/tests/test_kernel_promotion_workflow.py && git commit -m "ci(kernel): promote the merged pull request's kernel build on the push to iso-v0 (ADR-0110)"`

---

### Task 9: Documentation

**Files:**
- Modify: `docs/architecture/doc_ci.md` (CI8)
- Modify: `docs/architecture/doc_kernel_build.md` (section 7, paragraph "Promotion": **STOP AND ASK**, approved specification text; show the diff to the maintainer before the push)

- [ ] **Step 1: Write the replacement script** `.scratch/adr0110_docs.py`:

```python
"""Exact-match edits of the docs for ADR-0110: python3 .scratch/adr0110_docs.py <ci|spec>."""

import pathlib
import sys


def sub(path, old, new):
    path = pathlib.Path(path)
    text = path.read_text()
    assert text.count(old) == 1, f"{path}: {text.count(old)} matches of {old[:70]!r}"
    path.write_text(text.replace(old, new))


if sys.argv[1] == "ci":
    doc = "docs/architecture/doc_ci.md"
    sub(
        doc,
        "**Purpose:** the Azoth kernel: lint (CI2), prep, RPM build, boot matrix,",
        "**Purpose:** the Azoth kernel: lint (CI2), prep, RPM build (or, on a push to `iso-v0`, the"
        " promotion of the RPMs the merged pull request's CI28 run built, ADR-0110), boot matrix,",
    )
    sub(
        doc,
        "`kernel-boot-logs`, `kernel-attestations`; check `Kernel gate`.",
        "`kernel-boot-logs`, `kernel-attestations`, and `kernel-promotion` (the decision"
        " `publish` attests) when the push promotes; check `Kernel gate`.",
    )
    sub(
        doc,
        "`build-inputs.py`, `retention.sh`, `system/kernel-artifacts.sh`.",
        "`build-inputs.py`, `retention.sh`, `system/kernel-artifacts.sh`,"
        " `scripts/ci/build-builder.sh`, `scripts/ci/kernel_promotion.py`.",
    )

if sys.argv[1] == "spec":
    doc = "docs/architecture/doc_kernel_build.md"
    sub(
        doc,
        """**Promotion (ADR-0110, approved by the maintainer on 2026-10-09).** It takes effect once the
single pull request build (#368) and the trusted builder build (#373) are merged; until then
the push builds. When the inputs change,""",
        """**Promotion (ADR-0110, approved by the maintainer on 2026-10-09).** It is in effect since the
pull request that added `scripts/ci/kernel_promotion.py`, after the single pull request build
(#368) and the trusted builder build (#373); the builder image of a pull request is built
without cached layers too. When the inputs change,""",
    )
    sub(
        doc,
        """the API by the pull request's head commit; (2) the tree of `forge/specs/azoth` and the blobs of
`pr.yml` and `call-kernel.yml` are equal at the run's `head_sha` and at the pushed commit, and
likewise at the base commit recorded when the run started, with values taken from GitHub's
metadata and the pushed checkout, never from the run's outputs or artifacts; (3)""",
        """the API by the pull request's head commit; (2) the tree of `forge/specs/azoth` and the blobs of
`pr.yml`, `call-kernel.yml` and `scripts/ci/build-builder.sh` are equal at the run's `head_sha`
and at the pushed commit, and likewise at the base the pull request records and at the pushed
commit's parent, a pull request whose base was changed never promotes, and the values are taken
from GitHub's metadata and the pushed checkout, never from the run's outputs or artifacts; (3)""",
    )
```

- [ ] **Step 2: Apply the CI part and check.** `python3 .scratch/adr0110_docs.py ci && git diff --numstat docs/architecture/doc_ci.md` → `3	3	docs/architecture/doc_ci.md`. `python3 scripts/verify.py ci docs` → PASS.

- [ ] **Step 3: Commit.** `git add docs/architecture/doc_ci.md && git commit -m "docs(ci): describe the kernel promotion of CI8"`

- [ ] **Step 4: STOP AND ASK, then apply the spec part.** Show the maintainer the two replacements of the `spec` part (they record decisions 1, 2 and 3 of this plan in the approved text). Only after the answer: `python3 .scratch/adr0110_docs.py spec && git diff --numstat docs/architecture/doc_kernel_build.md` → `8	6	docs/architecture/doc_kernel_build.md` (measured on the `ba4edd80` text; #368 does not touch this paragraph). `python3 scripts/verify.py docs` → PASS. If the maintainer declines, skip Steps 4 and 5; the implementation is stricter than the text, not looser.

- [ ] **Step 5: Commit.** `git add docs/architecture/doc_kernel_build.md && git commit -m "docs(kernel): record that kernel promotion is in effect"`

- [ ] **Step 6: Before the pull request.** `just check` → green. Open one pull request against `iso-v0` with the eight or nine commits; its description lists the decisions of this plan for the maintainer and states that Tasks 7 and 9 are signing and specification changes. The merge follows the card and approval rule of `CLAUDE.md`. That pull request's own push builds normally (it changes `kernel-build.yml`, so the push runs, but the run it would promote from ran the old `call-kernel.yml`, whose blob differs: condition 2 refuses).

---

### Task 10: Acceptance runs on GitHub

The end-to-end path needs GitHub's API, a real artifact and the self-hosted runner, so it cannot run locally. Two pushes accept the change; both start after the pull request of Tasks 1-9 is merged. Every merge below follows the card and approval rule of `CLAUDE.md`. Record run ids and results in the pull request's thread or the issue that tracks ADR-0110.

**(A) A push that promotes.**

- [ ] **Step 1:** Take the next kernel bump pull request of `kernel-bump.yml` (weekly, ADR-0109), or ask the maintainer to dispatch `kernel-bump.yml`. Its `pr.yml` run must finish with `kernel / build` and `kernel / Kernel verdict` green (`gh run view <pr-run> --json jobs --jq '.jobs[] | [.name, .conclusion] | @tsv'`). Note its run id and the `kernel-build` artifact digest: `gh api "repos/$REPO/actions/runs/<pr-run>/artifacts?name=kernel-build" --jq '.artifacts[] | [.id, .digest] | @tsv'`.
- [ ] **Step 2:** Merge it without other kernel-relevant changes in between. On the push run of `kernel-build.yml` (`gh run list --workflow kernel-build.yml --branch iso-v0 --event push --limit 1`):
  - the `inputs` job summary starts with "Promoted: the RPMs of run [<pr-run>]" and lists the four compared paths with equal head/pushed and base/parent ids;
  - `build` is `skipped`; `boot`, `kmod / build (open)`, `kmod / build (legacy)`, `publish`, `Kernel gate` and `orchestrator` are `success`;
  - `gh run download <push-run> -n kernel-promotion -D /var/tmp/adr0110-a` gives a `decision.json` whose `source_run.id` is `<pr-run>`, `pull_request.merge_commit` is the pushed sha and `artifact.digest` is the digest of Step 1.
- [ ] **Step 3:** The attestation survives retention and verifies with the `iso-v0` identity: `nvr=$(bash forge/specs/azoth/nvr.sh); image=$(bash system/kernel-artifacts.sh registry)/azoth; cosign verify-attestation --type "https://github.com/$REPO/kernel-promotion/v1" --certificate-identity-regexp "$(bash system/kernel-artifacts.sh identity kernel)" --certificate-oidc-issuer https://token.actions.githubusercontent.com "$image:$nvr" | jq -r '.payload | @base64d | fromjson | .predicate.source_run.id'` prints `<pr-run>`; the same command with `--type custom` still verifies the pins (reuse unaffected); `bash system/kernel-artifacts.sh resolve` reports `state=ready`.
- [ ] **Step 4:** The published bits are the PR's: `gh run download <pr-run> -n kernel-boot -D /var/tmp/adr0110-pr` and `podman create "$image:$nvr" /kernel` + `podman cp` into `/var/tmp/adr0110-oci`, then `sha256sum` of `kernel-core-<nvr>*.rpm` in both directories are equal.

**(B) A push forced to rebuild.**

- [ ] **Step 5:** Open a kernel pull request K that changes a build input (the next bump, or, when none is due, a comment line in `forge/specs/azoth/kernel-local`, which `build-inputs.py` hashes, so the inputs job does not reuse). **Unverified:** that the config gate of `build.sh --stage prep` accepts a `#` comment line in the fragment; K's own run shows it. Let K's `pr.yml` run finish green.
- [ ] **Step 6:** Open and merge first a second pull request D that changes only a comment in `.github/workflows/pr.yml` (it builds nothing new: its kernel inputs are those of `iso-v0`, so its `kernel` job reuses). Then merge K without re-running its checks (`gate` is not strict: `rulesets.json` `strict_required_status_checks_policy: false`).
- [ ] **Step 7:** On K's push run: the `inputs` summary reads "Not promoted, the push builds: condition 2: .github/workflows/pr.yml differs between the run's head <K head> and the pushed commit <sha>"; `build` is `success` (it rebuilt), `publish` is `success`, and there is no `kernel-promotion` artifact (`gh api "repos/$REPO/actions/runs/<push-run>/artifacts?name=kernel-promotion" --jq .total_count` prints `0`). `cosign verify-attestation --type "https://github.com/$REPO/kernel-promotion/v1" ... "$image:<new nvr>"` fails with no matching attestation, and `--type custom` verifies.

An expired artifact, the other case the ADR names, follows the same path (condition 4) and is covered by `test_an_expired_artifact_builds`; waiting 90 days for it is not part of acceptance.

- [ ] **Step 8:** If either run differs from the expectation, open an issue with the run id and the summary line, and turn the promotion step off by reverting the commit of Task 8 through a pull request (the push then builds as before, and `publish` ignores the empty `promoted` output).
