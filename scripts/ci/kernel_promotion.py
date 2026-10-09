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
