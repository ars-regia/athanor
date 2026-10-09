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


class Refused(Exception):
    """A condition that does not hold: the push builds."""


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def _stream(response, dest, limit):
    """Write RESPONSE to DEST, at most LIMIT bytes, and return its SHA-256 in hex."""
    digest = hashlib.sha256()
    size = 0
    with open(dest, "wb") as out:
        while chunk := response.read(1 << 20):
            size += len(chunk)
            if size > limit:
                raise ValueError(f"the download is longer than the {limit} bytes the API reports")
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

    def download(self, path, dest, limit):
        """Stream PATH to DEST, at most LIMIT bytes, and return its SHA-256 in hex. The API
        redirects an artifact to storage outside GitHub: the redirect is followed without the token."""
        request = urllib.request.Request(self.url + path, headers=self.headers)
        try:
            with urllib.request.build_opener(_NoRedirect).open(request, timeout=60) as response:
                return _stream(response, dest, limit)
        except urllib.error.HTTPError as error:
            if error.code not in (301, 302, 303, 307, 308):
                raise
            location = error.headers["Location"]
            error.close()
            if not location:
                raise ValueError(f"{path}: redirect without Location")
        with urllib.request.build_opener().open(location, timeout=60) as response:
            return _stream(response, dest, limit)


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
    actual = "sha256:" + api.download(
        f"/repos/{repository}/actions/artifacts/{artifact['id']}/zip", dest, artifact["size_in_bytes"]
    )
    if actual != expected:
        raise Refused(f"condition 4: artifact {artifact['id']} has digest {actual}, the API reports {expected}")
    return artifact


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
    base_unchanged(api, repository, pull["number"])
    parent, decision["trees"] = compare_trees(repo, sha, head, base)
    decision["pushed"] = {"commit": sha, "parent": parent}
    inputs = build_inputs(repo, sha, tmp / "pushed")
    if build_inputs(repo, head, tmp / "head") != inputs:
        raise Refused("condition 3: build-inputs.py gives other inputs at the run's head than at the pushed commit")
    canonical = json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode()
    decision["build_inputs_sha256"] = hashlib.sha256(canonical).hexdigest()
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
    return decision


def promote(api, repo, repository, sha, nvr, extract):
    """decide, then unpack the artifact into EXTRACT; any failure is a decision to build."""
    try:
        with tempfile.TemporaryDirectory() as tmp:
            decision = decide(api, repo, repository, sha, nvr, tmp)
            pathlib.Path(extract).mkdir(parents=True)
            with zipfile.ZipFile(pathlib.Path(tmp) / "artifact.zip") as archive:
                archive.extractall(extract)
            return decision
    except Refused as refused:
        return {"promoted": False, "reason": str(refused)}
    # Any other failure, of the API, of git or of any archive library, means "build" too, never a
    # red job; the reason names the exception and lands in the decision and the summary.
    except Exception as error:  # noqa: BLE001 - the boundary of "any failure means build"
        detail = getattr(error, "stderr", None) or ""
        if isinstance(detail, bytes):
            detail = detail.decode(errors="replace")
        return {"promoted": False, "reason": f"error: {type(error).__name__}: {error} {detail}".strip()}


def summary(decision):
    lines = ["### Kernel promotion (ADR-0110)", ""]
    if not decision["promoted"]:
        return "\n".join([*lines, f"Not promoted, the push builds: {decision['reason']}", ""])
    run, pull, artifact = decision["source_run"], decision["pull_request"], decision["artifact"]
    lines += [
        (
            f"Promoted: the RPMs of run [{run['id']}]({run['url']}) of pull request #{pull['number']} "
            f"(head `{pull['head']}`, recorded base `{pull['base']}`), artifact {artifact['id']} "
            f"`{artifact['digest']}`. This push does not build the kernel."
        ),
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
    # A refusal is routine; an error disables promotion until someone reads it.
    level = "warning" if decision["reason"].startswith("error: ") else "notice"
    print(f"::{level} title=Kernel promotion::{outcome}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
