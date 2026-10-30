#!/usr/bin/env python3
"""The candidate of a release (docs/architecture/doc_pipeline.md, PL55 and PL58): the build's
own request, checked against the API's record of the build run, and the order of the
candidate against each repository's :latest. release.yml signs, verifies and tags only what
check wrote; tag-latest runs order again just before :latest moves.

  release_candidate.py check --out DIR    BUILD_RUN_ID in env
  release_candidate.py order --dir DIR

Both need GITHUB_REPOSITORY, GITHUB_REF and gh authenticated (GH_TOKEN, GH_REPO). Exit 1 when
the release is refused, with every reason on stderr.
"""

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time

import release_class
import schema

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD_WORKFLOW = ".github/workflows/athanor-forge-orchestrator.yml"
BRANCH = "iso-v0"
RELEASE_REF = f"refs/heads/{BRANCH}"
REQUEST_JOB = "request-release"
# The job of call-system-image.yml that builds and pushes the images, as the API names it
# (caller job / called job); PB12 renames it.
IMAGE_JOB = "system-image / 💿 System Image & Disk Build"
ARTIFACTS = ["release-request", "image-digests", "kernel-digests"]
FILES = ["image-digests.txt", "kernel-digests.txt", "kernel-unsigned.txt"]
COMMIT = re.compile(r"^[0-9a-f]{40}$")
TIMEOUT = 300  # seconds for one external command
# How long the API's record of the build may lag the jobs that requested this release. Seen: 6.3 min.
SETTLE = 600  # seconds; RELEASE_CANDIDATE_SETTLE replaces it in the tests
SETTLE_PAUSE = 15  # seconds between two reads
CREATED = "org.opencontainers.image.created"
REVISION = "org.opencontainers.image.revision"


class Refused(Exception):
    pass


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def sh(args, **kw):
    """subprocess.run, with a timeout that refuses the release instead of hanging it."""
    try:
        return subprocess.run(args, capture_output=True, text=True, timeout=TIMEOUT, **kw)
    except subprocess.TimeoutExpired:
        raise Refused(f"{' '.join(args)}: timed out after {TIMEOUT} s") from None


def tool(*args):
    r = sh(args)
    if r.returncode:
        raise Refused(f"{' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def api(path, *flags):
    return json.loads(tool("gh", "api", *flags, path))


def build_record(repo, run_id):
    """The API's record of the build run and of the jobs of its attempt."""
    run = api(f"/repos/{repo}/actions/runs/{run_id}")
    pages = api(
        f"/repos/{repo}/actions/runs/{run_id}/attempts/{run['run_attempt']}/jobs?per_page=100",
        "--paginate",
        "--slurp",
    )
    return run, [j for page in pages for j in page["jobs"]]


def behind(run, jobs):
    """Whether the record has not yet caught up with the jobs that requested the release."""
    return (
        run["status"] == "queued"
        or IMAGE_JOB not in [j["name"] for j in jobs]
        or any(j["name"] != REQUEST_JOB and j["conclusion"] is None for j in jobs)
    )


def settled(repo, run_id, attempt):
    """The build run and its jobs, once the API's record has caught up with request-release, or
    as they stand after SETTLE seconds. The record can show a finished job as unfinished and the
    run as queued for minutes, and one read of it would end the release of this build. The
    checks that follow stay as strict: a run that stays behind is refused by them, and one at
    another attempt than the request's will never catch up, so it is not waited for."""
    deadline = time.monotonic() + float(os.environ.get("RELEASE_CANDIDATE_SETTLE", SETTLE))
    while True:
        try:
            run, jobs = build_record(repo, run_id)
        except Refused:
            if time.monotonic() >= deadline:
                raise
        else:
            if (
                run["run_attempt"] != attempt
                or not behind(run, jobs)
                or time.monotonic() >= deadline
            ):
                return run, jobs
        time.sleep(SETTLE_PAUSE)


def compare(repo, base, head):
    """identical or ahead: base is an ancestor of head. Commit ids only, so no tag can stand in."""
    return tool(
        "gh", "api", f"/repos/{repo}/compare/{base}...{head}", "--jq", ".status"
    ).strip()


def labels(ref):
    return (
        json.loads(tool("skopeo", "inspect", "--config", f"docker://{ref}"))[
            "config"
        ].get("Labels")
        or {}
    )


def when(text, what):
    if not isinstance(text, str) or not text:
        raise Refused(f"{what}: no {CREATED} label")
    try:
        time = datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        raise Refused(f"{what}: {CREATED} label '{text}' is not a time") from None
    if time.tzinfo is None:
        raise Refused(f"{what}: {CREATED} label '{text}' names no time zone")
    return time.astimezone(datetime.timezone.utc)


def order(repo, digests, commit):
    """PL58, per repository of the digests file. Every problem is collected, then refused."""
    tip = tool(
        "gh", "api", f"/repos/{repo}/branches/{BRANCH}", "--jq", ".commit.sha"
    ).strip()
    if not COMMIT.match(tip):
        raise Refused(f"branch {BRANCH} has no commit id ('{tip}')")
    verdicts, why = [], []
    for line in digests.read_text().splitlines():
        repository, _tag, candidate = line.split()
        entry = {
            "repository": repository,
            "candidate_digest": candidate,
            "latest_digest": None,
            "latest_created": None,
            "verdict": "absent",
        }
        verdicts.append(entry)
        r = sh(
            [
                "skopeo",
                "inspect",
                "--format",
                "{{.Digest}}",
                f"docker://{repository}:latest",
            ]
        )
        if r.returncode:
            if "manifest unknown" not in r.stderr:
                why.append(f"{repository}:latest cannot be read: {r.stderr.strip()}")
            continue
        entry["latest_digest"] = latest = r.stdout.strip()
        if latest == candidate:
            entry["verdict"] = "equal"
            continue
        entry["verdict"] = "newer"
        try:
            old, new = (
                labels(f"{repository}@{latest}"),
                labels(f"{repository}@{candidate}"),
            )
            latest_time = when(old.get(CREATED), latest)
            entry["latest_created"] = latest_time.strftime("%Y-%m-%dT%H:%M:%SZ")
            # The build checks the label (system/check-image-revision.sh); the release does too.
            if new.get(REVISION) != commit:
                why.append(
                    f"{repository}: candidate names revision '{new.get(REVISION)}', not the request's commit {commit}"
                )
            if not when(new.get(CREATED), candidate) > latest_time:
                why.append(
                    f"{repository}: candidate created {new.get(CREATED)} is not later than :latest {old.get(CREATED)}"
                )
            # A :latest from main, or unlabelled, is ordered by build time alone (PL58).
            revision = old.get(REVISION) or ""
            if revision and not COMMIT.match(revision):
                why.append(
                    f"{repository}: :latest names revision '{revision}', not a commit"
                )
            elif (
                revision
                and compare(repo, revision, tip) in ("identical", "ahead")
                and compare(repo, revision, commit) not in ("identical", "ahead")
            ):
                why.append(
                    f"{repository}: :latest commit {revision} is not an ancestor of {commit}"
                )
        except Refused as error:
            why.append(f"{repository}: {error}")
    if why:
        raise Refused("; ".join(why))
    return verdicts


def superseded(repo, run, verdicts):
    """Successful builds of iso-v0 between :latest and the candidate (PL55), for the record.
    ponytail: ordered by time from the oldest :latest a release moves, so a build that main's
    old jobs or PB13's interim jobs released shows too; exact once PB4b marks releases."""
    times = sorted(
        v["latest_created"]
        for v in verdicts
        if v["verdict"] == "newer" and v["latest_created"]
    )
    if not times:
        return []
    path = (
        f"/repos/{repo}/actions/workflows/{pathlib.Path(BUILD_WORKFLOW).name}/runs"
        f"?branch={BRANCH}&status=success&created=>={times[0]}&per_page=100"
    )
    runs = [
        r for page in api(path, "--paginate", "--slurp") for r in page["workflow_runs"]
    ]
    return sorted(
        r["id"]
        for r in runs
        if r["id"] != run["id"] and r["created_at"] < run["created_at"]
    )


def classes(verdicts, commit, sources):
    """PL55: one class record per repository, the :latest of each as its baseline."""
    records = []
    for v in verdicts:
        repository = v["repository"]
        if v["verdict"] == "equal":
            # Nothing is compared, so a security release would read as feature. The re-run
            # rule of PB4b (read back the recorded class) is later work; until then refuse.
            raise Refused(
                f"{repository}: :latest is already the candidate; deriving its class needs "
                "the re-run rule of PB4b, which is not built yet"
            )
        baseline = None
        if v["latest_digest"]:
            ref = f"{repository}@{v['latest_digest']}"
            baseline = release_class.Baseline(
                ref, v["latest_digest"], labels(ref).get(REVISION) or None
            )
        candidate = release_class.Image(
            f"{repository}@{v['candidate_digest']}", v["candidate_digest"]
        )
        try:
            records.append(release_class.derive(repository, baseline, candidate, commit, sources))
        except release_class.ClassError as error:
            raise Refused(str(error)) from None
    return records


def check(out):
    run_id = os.environ.get("BUILD_RUN_ID", "")
    if not re.fullmatch(r"[1-9][0-9]*", run_id):
        raise Refused(f"build_run_id '{run_id}' is not a run id")
    ref = os.environ.get("GITHUB_REF", "")
    if ref != RELEASE_REF:
        raise Refused(f"release.yml releases only from {RELEASE_REF}, not {ref}")
    repo = os.environ["GITHUB_REPOSITORY"]
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        for name in ARTIFACTS:
            tool("gh", "run", "download", run_id, "-n", name, "-D", str(tmp))
        try:
            request = json.loads((tmp / "release-request.json").read_text())
            schema.validate(request, schema.load("release-request"))
        except ValueError as error:
            raise Refused(f"release-request.json: {error}") from None
        run, jobs = settled(repo, run_id, request["build_run_attempt"])
        why = []
        if request["build_run_id"] != int(run_id):
            why.append(f"request names run id {request['build_run_id']}, not {run_id}")
        if request["workflow_path"] != BUILD_WORKFLOW:
            why.append(f"request names workflow {request['workflow_path']}")
        if request["build_run_attempt"] != run["run_attempt"]:
            why.append(
                f"request names attempt {request['build_run_attempt']}, the run is at {run['run_attempt']}"
            )
        if request["ref"] != RELEASE_REF:
            why.append(f"request names ref {request['ref']}")
        if request["commit"] != run["head_sha"]:
            why.append(
                f"request names commit {request['commit']}, the run built {run['head_sha']}"
            )
        for name in FILES:
            if sha256(tmp / name) != request["sha256"][name]:
                why.append(f"{name} differs from the hash of the request")
        why += [
            f"job {j['name']} {j['conclusion']}"
            for j in jobs
            if j["name"] != REQUEST_JOB
            and j["conclusion"] not in ("success", "skipped")
        ]
        if [j["conclusion"] for j in jobs if j["name"] == IMAGE_JOB] != ["success"]:
            why.append(f"image job '{IMAGE_JOB}' did not succeed once")
        trusted = sh(
            [
                "bash",
                str(ROOT / "system/trusted-run.sh"),
                "--in-progress",
                run_id,
                BUILD_WORKFLOW,
            ],
            env={**os.environ, "RELEASE_BRANCH": BRANCH},
        )
        if trusted.returncode:
            why.append(
                trusted.stderr.strip() or f"trusted-run.sh exited {trusted.returncode}"
            )
        if why:
            raise Refused("; ".join(why))
        out.mkdir(parents=True, exist_ok=True)
        for name in FILES:
            shutil.copy(tmp / name, out / name)
    verdicts = order(repo, out / "image-digests.txt", request["commit"])
    records = classes(verdicts, request["commit"], release_class.PodmanSources(ROOT))
    notes = [f"- {r['repository']}: class_reason `{r['class_reason']}`\n" for r in records if r["class_reason"]]
    if notes and os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as f:
            f.write("Class derived by fallback (PL55) or across a change of Fedora release (#681):\n" + "".join(notes))
    else:
        sys.stderr.write("".join(notes))
    candidate = {
        "schema_version": 2,
        "request": request,
        # The start of the attempt that built the images: the build_time of PB4b (ADR-0127).
        "build_time": run["run_started_at"],
        "order": verdicts,
        "classes": records,
        "superseded": superseded(repo, run, verdicts),
    }
    try:
        schema.validate(candidate, schema.load("release-candidate"))
    except ValueError as error:
        raise Refused(f"release-candidate.json: {error}") from None
    (out / "release-candidate.json").write_text(json.dumps(candidate, indent=2) + "\n")
    return candidate


def main(argv):
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check").add_argument("--out", required=True, type=pathlib.Path)
    sub.add_parser("order").add_argument("--dir", required=True, type=pathlib.Path)
    a = p.parse_args(argv)
    try:
        if a.cmd == "check":
            result = check(a.out)
        else:
            if os.environ.get("GITHUB_REF") != RELEASE_REF:
                raise Refused(
                    f"release.yml releases only from {RELEASE_REF}, not {os.environ.get('GITHUB_REF', '')}"
                )
            candidate = json.loads((a.dir / "release-candidate.json").read_text())
            result = order(
                os.environ["GITHUB_REPOSITORY"],
                a.dir / "image-digests.txt",
                candidate["request"]["commit"],
            )
    except Refused as error:
        print(f"release_candidate.py {a.cmd}: refused: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
