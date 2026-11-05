#!/usr/bin/env python3
"""Detect a `:stable` that no promotion moved (#405, doc_pipeline.md PL54, ADR-0111 item 5).

The GITHUB_TOKEN of any run with `packages: write` can push a GHCR tag, so `skopeo copy` can
put an already signed digest on `:stable` and skip the evidence, the dwell and the `release`
approval. This reads, for each system image repository, `:stable`, `:stable-previous` and the
newest `:stable-<YYYYMMDD>`, and fails, naming the digest, unless the chain of the newest
promotion record holds: `:stable` is its digest (an older recorded digest copied back is a
rollback no promotion made), `:stable-previous` its `previous_stable`, and the newest dated
tag the digest of the last record of that day. A dated tag that is no date, or lies in the
future, fails (promote.sh never writes one). Once an image has a record, a missing tag fails;
an image with no record and no `:stable` is skipped.

A promotion record is the `promotion.json` of an evidence bundle in the registry off GitHub
(`$EVIDENCE_REGISTRY/athanor-evidence:<build run>-<promotion run>`, system/evidence-bundle.sh)
that counts only when all of this holds:
  - its keyless signature verifies (`evidence-bundle.sh verify-dir`);
  - its `trigger` is the promotion run the tag names;
  - that run's `promote` job concluded success, and the GitHub deployment of that job in the
    environment `release` succeeded: a plan nobody approved, or a bundle pushed from outside,
    is not a record.
A bundle that fails one of these is reported on stderr and is no record. A registry, a
bundle or the GitHub API that cannot be read fails the check: nothing passes unread.

Usage: stable_promotions.py [--image NAME]...
Environment: REGISTRY (default <REGISTRY_HOST or ghcr.io>/<GITHUB_REPOSITORY_OWNER>); EVIDENCE_REGISTRY;
GITHUB_REPOSITORY; GH_TOKEN with actions: read and deployments: read. Needs skopeo, gh,
cosign and bash on PATH. Read-only.
"""

import argparse
from datetime import datetime, timezone
import json
import os
import pathlib
import re
import subprocess
import sys
import tarfile
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
IMAGES = ("athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy")
# The `name:` of the promote job of promote-stable.yml; the job in the environment `release`.
PROMOTE_JOB = "Point stable at the run"
ENVIRONMENT = "release"
EVIDENCE_IMAGE = "athanor-evidence"


class Unreadable(Exception):
    """A registry, a bundle or the API could not be read: the check fails."""


def run(*cmd):
    r = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if r.returncode != 0:
        raise Unreadable(f"{' '.join(cmd)}: {r.stderr.strip()}")
    return r.stdout


def tag_digest(repo, tag):
    """The digest a tag names, None when the tag does not exist (the only tolerated failure)."""
    r = subprocess.run(
        ["skopeo", "inspect", "--format", "{{.Digest}}", f"docker://{repo}:{tag}"],
        capture_output=True,
        text=True,
        check=False,
    )
    if r.returncode == 0:
        return r.stdout.strip()
    if "manifest unknown" in r.stderr:
        return None
    raise Unreadable(f"skopeo inspect {repo}:{tag}: {r.stderr.strip()}")


def tags(repo):
    return json.loads(run("skopeo", "list-tags", f"docker://{repo}"))["Tags"]


def fetch_bundle(ref, into):
    """Pull the bundle image with skopeo and unpack its layers into `into`."""
    raw = into / "raw"
    run("skopeo", "copy", f"docker://{ref}", f"dir:{raw}")
    tree = into / "tree"
    tree.mkdir()
    try:
        for layer in json.loads((raw / "manifest.json").read_text())["layers"]:
            with tarfile.open(
                raw / layer["digest"].removeprefix("sha256:"), "r:*"
            ) as tar:
                tar.extractall(tree, filter="data")
    except (OSError, KeyError, ValueError, tarfile.TarError) as e:
        raise Unreadable(f"{ref} is not a readable bundle image: {e}") from e
    return tree


class Release:
    """Which promotion runs ran their promote job in the environment `release`."""

    def __init__(self, repo):
        self.repo = repo
        self.jobs = None  # {(run, job id)} of successful deployments in the environment

    def deployed_jobs(self):
        if self.jobs is None:
            self.jobs = set()
            ids = run(
                "gh",
                "api",
                "--paginate",
                f"repos/{self.repo}/deployments?environment={ENVIRONMENT}",
                "--jq",
                ".[].id",
            ).split()
            for dep in ids:
                urls = run(
                    "gh",
                    "api",
                    "--paginate",
                    f"repos/{self.repo}/deployments/{dep}/statuses",
                    "--jq",
                    '.[] | select(.state == "success") | .log_url',
                ).split()
                for url in urls:
                    m = re.search(r"/actions/runs/(\d+)/job/(\d+)$", url)
                    if m:
                        self.jobs.add((m[1], m[2]))
        return self.jobs

    def promoted(self, promotion_run):
        found = run(
            "gh",
            "api",
            "--paginate",
            f"repos/{self.repo}/actions/runs/{promotion_run}/jobs",
            "--jq",
            f'.jobs[] | select(.name == "{PROMOTE_JOB}" and .conclusion == "success") | .id',
        ).split()
        return any((promotion_run, job) in self.deployed_jobs() for job in found)


def records(evidence_repo, release):
    """The verified, approved promotions as dicts, oldest promotion run first."""
    found = []
    for tag in sorted(tags(evidence_repo)):
        m = re.fullmatch(r"(\d+)-(\d+)", tag)
        if not m:
            continue
        with tempfile.TemporaryDirectory() as tmp:
            tree = fetch_bundle(f"{evidence_repo}:{tag}", pathlib.Path(tmp))
            v = subprocess.run(
                ["bash", str(ROOT / "system/evidence-bundle.sh"), "verify-dir", str(tree), str(tree)],
                capture_output=True,
                text=True,
                check=False,
            )
            if v.returncode != 0:
                print(f"{tag}: not a record, the bundle does not verify: {v.stderr.strip()}", file=sys.stderr)
                continue
            try:
                plan = json.loads((tree / "promotion.json").read_text())
            except (OSError, ValueError) as e:
                raise Unreadable(f"{tag}: promotion.json of a verified bundle is unreadable: {e}") from e
        if not str(plan.get("trigger", "")).endswith(f"/actions/runs/{m[2]}"):
            print(f"{tag}: not a record, its trigger is {plan.get('trigger')!r}, not promotion run {m[2]}", file=sys.stderr)
        elif not release.promoted(m[2]):
            print(f"{tag}: not a record, the promote job of run {m[2]} did not succeed in the environment {ENVIRONMENT}", file=sys.stderr)
        else:
            found.append({"run": int(m[2]), "day": plan.get("day"), "images": {i["name"]: i for i in plan["images"]}})
    return sorted(found, key=lambda r: r["run"])


def valid_day(tag):
    """The date of a dated tag that is a real date, not in the future; None otherwise."""
    try:
        day = datetime.strptime(tag.removeprefix("stable-"), "%Y%m%d").date()
    except ValueError:
        return None
    return day if day <= datetime.now(timezone.utc).date() else None


def check_image(name, repo, found):
    """The failures of one image, as lines; empty when its tags are the newest record's chain."""
    mine = [r for r in found if name in r["images"]]
    stable = tag_digest(repo, "stable")
    if not mine:
        # No record: any of the tags is a copy no promotion made.
        present = ["stable", "stable-previous", *(x for x in tags(repo) if re.fullmatch(r"stable-\d{8}", x))]
        seen = {x: tag_digest(repo, x) for x in dict.fromkeys(present)}
        seen = {x: d for x, d in seen.items() if d}
        if not seen:
            print(f"{name}: never promoted, no :stable, :stable-previous or dated tag")
        return [f"{name}:{x}: {d} has no promotion record from a promote job that ran in the environment {ENVIRONMENT}" for x, d in seen.items()]
    newest = mine[-1]
    want = newest["images"][name]
    bad = []
    stopped = (
        "; a promote that stopped between its copies leaves the tags out of step: re-run its failed job, "
        "otherwise a tag was moved from outside"
    )
    if stable != want["digest"]:
        bad.append(
            f"{name}:stable: {stable or 'missing'}, but the newest promotion (run {newest['run']}) set {want['digest']}: "
            "a rollback or a copy no promotion made, or a promote still running"
        )
    previous = want.get("previous_stable")
    have = tag_digest(repo, "stable-previous")
    if not previous:
        # The first promotion of an image has no :stable to keep, and promote.sh writes
        # :stable-previous only from one.
        if have:
            bad.append(f"{name}:stable-previous: {have}, but the newest promotion (run {newest['run']}) recorded no previous stable{stopped}")
    else:
        if have != previous:
            bad.append(f"{name}:stable-previous: {have or 'missing'}, but the newest promotion (run {newest['run']}) recorded {previous}{stopped}")
    dated = {t: valid_day(t) for t in tags(repo) if re.fullmatch(r"stable-\d{8}", t)}
    for t, day in sorted(dated.items()):
        if day is None:
            bad.append(f"{name}:{t}: not a date, or a date in the future; promote.sh never writes one")
    valid = {t: d for t, d in dated.items() if d}
    if valid:
        top = max(valid, key=valid.get)
        last = [r for r in mine if r["day"] == top.removeprefix("stable-")]
        have = tag_digest(repo, top)
        if not last:
            bad.append(f"{name}:{top}: {have}, but no promotion record has that day{stopped}")
        elif have != last[-1]["images"][name]["digest"]:
            bad.append(f"{name}:{top}: {have}, but the last promotion of that day (run {last[-1]['run']}) set {last[-1]['images'][name]['digest']}{stopped}")
    if not bad:
        print(f"{name}: :stable, :stable-previous and {max(valid, key=valid.get) if valid else 'no dated tag'} follow promotion run {newest['run']}")
    return bad


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--image", action="append", help="image name to check (default: the system images)")
    args = ap.parse_args()
    owner = os.environ.get("GITHUB_REPOSITORY_OWNER", "").lower()
    registry = os.environ.get("REGISTRY") or (f"{os.environ.get('REGISTRY_HOST') or 'ghcr.io'}/{owner}" if owner else "")
    evidence = os.environ.get("EVIDENCE_REGISTRY", "")
    gh_repo = os.environ.get("GITHUB_REPOSITORY", "")
    if not (registry and evidence and gh_repo):
        print("set REGISTRY (or GITHUB_REPOSITORY_OWNER), EVIDENCE_REGISTRY and GITHUB_REPOSITORY", file=sys.stderr)
        return 2
    try:
        found = records(f"{evidence}/{EVIDENCE_IMAGE}", Release(gh_repo))
        bad = []
        for name in args.image or IMAGES:
            bad += check_image(name, f"{registry}/{name}", found)
    except Unreadable as e:
        print(f"unreadable, so not checked: {e}", file=sys.stderr)
        return 1
    for line in bad:
        print(line, file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
