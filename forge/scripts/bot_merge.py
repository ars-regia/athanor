#!/usr/bin/env python3
"""Merge a bot pull request whose change has exactly the shape the bot produces.

Spec Build Check calls it with `spec` after a green build of the spec bot's pull request,
System Image Check with `system` after a green build of a system bump. The script reads the
changed files from the GitHub API (status, path and patch of each), checks every changed line
against the bot's shape, waits for the required checks of the branch protection and merges
at HEAD_SHA, so a push after the check makes the merge fail instead of landing unchecked.
Nothing stays armed: a pull request the script does not merge waits for a person.

spec   — branch chore/update-specs-zero-trust (forge-util-update-specs.yml); only modified
         *.spec and SOURCES/sources.sha256 of the packages forge/upstream-watch.json lists
         (read from the checkout, the base branch); only Version
         lines (numeric), Release lines (N%{?dist}) and manifest entries; every Version keeps
         its leftmost non-zero component (1.6.0 -> 1.7.1 merges, 1.9 -> 2.0 and 0.3 -> 0.4 do
         not).
system — branch bump/system-* with label system-bump (kernel-bump.yml, system group); only
         modified system/Containerfile and system/nvidia/locks/*.lock; only new digests on
         the existing FROM lines, and lock entries whose URL is an RPM directly under the
         lock's own `# repository` line.

Usage: bot_merge.py spec|system PR HEAD_SHA BUILD_RESULT
       GH_TOKEN: reads the pull request and the Actions runs; MERGE_TOKEN: merges it (a token of
       the bots GitHub App, so that the merge triggers the push workflows)

A pull request of another shape, or a build that is not `success`, is reported in the log and
the job summary and the script exits 0. A failing gh call, a failing required check or a
refused merge exits non-zero.
"""

import json
import os
import re
import subprocess
import sys
import time

NAME = r"[A-Za-z0-9._+-]+"
HEX64 = r"[0-9a-f]{64}"
SPEC_FILE = re.compile(rf"(?:{NAME}\.spec|SOURCES/sources\.sha256)")
WATCH_FILE = "forge/upstream-watch.json"
# The bot waits for the checks the branch protection of the base branch requires, read from
# the checkout: the protection does not bind administrators (enforce_admins: false) and the
# merging App is a bypass actor of the pull request rule, so this wait is what keeps a red check from merging. Switching
# the required check (docs/operations/github-settings.md section 8) is a change of that file.
BRANCH_PROTECTION = ".github/settings/branch-protection.json"
# The workflow whose newest pull_request run reports each check a branch may require, as the
# name of its gate job.
CHECK_WORKFLOWS = {
    "gate": "pr.yml",
    "Kernel gate": "kernel-build.yml",
    "Spec gate": "spec-build-check.yml",
}
SYSTEM_PATH = re.compile(rf"system/(Containerfile|nvidia/locks/{NAME}\.lock)")
VERSION = re.compile(r"Version:[ \t]+(\d+(?:\.\d+)*)")
RELEASE = re.compile(r"Release:[ \t]+\d+%\{\?dist\}")
MANIFEST = re.compile(rf"{HEX64}  {NAME}")
LOCK_ENTRY = re.compile(rf"{HEX64}  (https://\S+)")
FROM = re.compile(rf"FROM ([^\s@]+)@sha256:{HEX64} AS (\S+)")
BRANCH = {
    "spec": re.compile(r"chore/update-specs-zero-trust"),
    "system": re.compile(r"bump/system-.+"),
}


class Refused(Exception):
    pass


def run_gh(*args, env=None):
    # stderr reaches the log: a refused merge or a failing call shows gh's own reason.
    return subprocess.run(
        ["gh", *args], check=True, stdout=subprocess.PIPE, text=True, env=env
    ).stdout


def merge_gh(*args):
    return run_gh(*args, env={**os.environ, "GH_TOKEN": os.environ["MERGE_TOKEN"]})


def watched_dirs():
    """forge/specs/<package>/ of every spec the updater watches: the only specs it bumps."""
    with open(WATCH_FILE) as f:
        return {"forge/" + os.path.dirname(item["spec"]) + "/" for item in json.load(f)}


def same_major(old, new):
    """Same components up to and including the leftmost non-zero one of OLD."""
    a, b = old.split("."), new.split(".")
    i = next((k for k, part in enumerate(a) if int(part)), len(a) - 1)
    return a[: i + 1] == b[: i + 1]


def changed_lines(f):
    """(sign, text) of every added or removed line of a file's patch."""
    if f.get("patch") is None:
        raise Refused(f"{f['filename']}: no textual patch (binary or too large)")
    for line in f["patch"].split("\n"):
        if line and line[0] in "+-":
            yield line[0], line[1:]
        elif not (
            line.startswith("@@")
            or line.startswith(" ")
            or line.startswith("\\")
            or line == ""
        ):
            raise Refused(f"{f['filename']}: unexpected patch line {line!r}")


def check_spec(f):
    versions = {"-": [], "+": []}
    for sign, text in changed_lines(f):
        if f["filename"].endswith(".spec"):
            if RELEASE.fullmatch(text):
                continue
            m = VERSION.fullmatch(text)
            if not m:
                raise Refused(
                    f"{f['filename']}: changes a line other than Version or Release: {text!r}"
                )
            versions[sign].append(m.group(1))
        elif not MANIFEST.fullmatch(text):
            raise Refused(
                f"{f['filename']}: changes a line that is not a manifest entry: {text!r}"
            )
    old, new = versions["-"], versions["+"]
    if len(old) != len(new) or len(old) > 1:
        raise Refused(f"{f['filename']}: Version lines added or removed")
    if old and not same_major(old[0], new[0]):
        raise Refused(
            f"{f['filename']}: Version {old[0]} -> {new[0]} changes the major version"
        )


def check_system(f, contents):
    if f["filename"] == "system/Containerfile":
        stages = {"-": [], "+": []}
        for sign, text in changed_lines(f):
            m = FROM.fullmatch(text)
            if not m:
                raise Refused(
                    f"system/Containerfile: changes a line other than a FROM digest: {text!r}"
                )
            stages[sign].append(m.groups())
        if sorted(stages["-"]) != sorted(stages["+"]):
            raise Refused(
                "system/Containerfile: a FROM image or stage is added, removed or renamed"
            )
        return
    repos = re.findall(r"^# repository (https://\S+/)$", contents(f["filename"]), re.M)
    if len(repos) != 1:
        raise Refused(f"{f['filename']}: no single `# repository` line")
    for _, text in changed_lines(f):
        m = LOCK_ENTRY.fullmatch(text)
        rest = m.group(1).removeprefix(repos[0]) if m else ""
        if not (
            m
            and m.group(1).startswith(repos[0])
            and re.fullmatch(rf"{NAME}\.rpm", rest)
        ):
            raise Refused(
                f"{f['filename']}: changes a line that is not an RPM of {repos[0]}: {text!r}"
            )


def check(kind, pr, sha, gh):
    view = json.loads(
        gh(
            "pr",
            "view",
            pr,
            "--json",
            "state,baseRefName,headRefName,headRefOid,isCrossRepository,labels,changedFiles",
        )
    )
    if view["state"] != "OPEN":
        raise Refused("is not open")
    if view["isCrossRepository"]:
        raise Refused("comes from a fork")
    if view["headRefOid"] != sha:
        raise Refused(f"head moved past the checked commit {sha}")
    if not BRANCH[kind].fullmatch(view["headRefName"]):
        raise Refused(f"branch {view['headRefName']} is not the {kind} bot's")
    if kind == "system" and "system-bump" not in {l["name"] for l in view["labels"]}:
        raise Refused("has no system-bump label")

    repo = (
        os.environ.get("GITHUB_REPOSITORY")
        or json.loads(gh("repo", "view", "--json", "nameWithOwner"))["nameWithOwner"]
    )
    out = gh("api", "--paginate", f"repos/{repo}/pulls/{pr}/files", "--jq", ".[]")
    files = [json.loads(line) for line in out.splitlines() if line]
    if not files:
        raise Refused("changes no file")
    if len(files) != view["changedFiles"]:
        raise Refused(f"the API listed {len(files)} of {view['changedFiles']} changed files")
    if kind == "spec":
        dirs = watched_dirs()
        def allowed(name):
            d = next((d for d in dirs if name.startswith(d)), None)
            return d is not None and SPEC_FILE.fullmatch(name[len(d):]) is not None
    else:
        allowed = SYSTEM_PATH.fullmatch

    def contents(name):
        return gh(
            "api",
            "-H",
            "Accept: application/vnd.github.raw",
            f"repos/{repo}/contents/{name}?ref={sha}",
        )

    for f in files:
        if f["status"] != "modified":
            raise Refused(
                f"{f['filename']}: status {f['status']}, the bot only modifies files"
            )
        if not allowed(f["filename"]):
            raise Refused(f"touches {f['filename']}, outside what the bot changes")
        if kind == "spec":
            check_spec(f)
        else:
            check_system(f, contents)
    return view["baseRefName"]


def report(pr, message):
    line = f"bot_merge: #{pr} {message}"
    print(line)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as s:
            s.write(line + "\n")


def gate_conclusion(name, workflow, sha, gh):
    """The conclusion of job NAME in the newest WORKFLOW run on SHA, None while it is pending.

    GitHub judges a required check by the newest run of its workflow: a green gate of an
    earlier run on the same commit (a reopened pull request, a rerun) does not count while a
    newer run has not reported its own, and the merge is refused. So the newest run is
    followed, not any check run of that name, and its gate job is read in the latest attempt.
    The job, not the run: Spec Build Check calls this script from its own run, whose gate job
    has ended while the run is still in progress.
    """
    repo = os.environ["GITHUB_REPOSITORY"]
    runs = json.loads(
        gh("api", f"repos/{repo}/actions/workflows/{workflow}/runs?head_sha={sha}&event=pull_request")
    )["workflow_runs"]
    if not runs:
        return None
    latest = max(runs, key=lambda r: r["id"])
    jobs = json.loads(gh("api", f"repos/{repo}/actions/runs/{latest['id']}/jobs?per_page=100"))["jobs"]
    gate = next((j["conclusion"] for j in jobs if j["name"] == name), None)
    if gate is None and latest["status"] == "completed":
        return "missing"
    return gate


def required_checks(base):
    """The checks BRANCH_PROTECTION requires on BASE, each with the workflow reporting it.

    A base without required checks, or a check no known workflow reports, fails the run: the
    bot never merges past a check it cannot wait for.
    """
    with open(BRANCH_PROTECTION) as f:
        protection = json.load(f).get(base) or {}
    contexts = [c["context"] for c in (protection.get("required_status_checks") or {}).get("checks", [])]
    if not contexts:
        sys.exit(f"bot_merge: {BRANCH_PROTECTION} requires no check on {base}: not merged")
    unknown = [c for c in contexts if c not in CHECK_WORKFLOWS]
    if unknown:
        sys.exit(f"bot_merge: no workflow known for the required check(s) {', '.join(unknown)}: not merged")
    return {c: CHECK_WORKFLOWS[c] for c in contexts}


def wait_for_required_checks(sha, checks, gh, sleep=time.sleep, polls=160):
    """Wait for every check of CHECKS on SHA to end green; any other end fails the run."""
    pending = dict(checks)
    for _ in range(polls):
        for name, workflow in list(pending.items()):
            gate = gate_conclusion(name, workflow, sha, gh)
            if gate is None:
                continue
            if gate != "success":
                sys.exit(f"bot_merge: {name} ({workflow}) is {gate} on {sha}: not merged")
            del pending[name]
        if not pending:
            return
        sleep(30)
    sys.exit(f"bot_merge: {', '.join(pending)} did not complete on {sha} in time: not merged")


def main(kind, pr, sha, build, gh=run_gh, merge=merge_gh):
    try:
        if build != "success":
            raise Refused(f"build is {build}")
        base = check(kind, pr, sha, gh)
    except Refused as reason:
        report(pr, f"stays for a person: {reason}")
        return 0
    wait_for_required_checks(sha, required_checks(base), gh)
    merge("pr", "merge", pr, "--squash", "--match-head-commit", sha)
    report(pr, f"merged at {sha} ({kind} bot, the change has the bot's shape)")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 5 or sys.argv[1] not in BRANCH:
        sys.exit("usage: bot_merge.py spec|system PR HEAD_SHA BUILD_RESULT")
    sys.exit(main(*sys.argv[1:]))
