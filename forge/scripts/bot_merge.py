#!/usr/bin/env python3
"""Merge a bot pull request whose change has exactly the shape the bot produces.

Spec Build Check calls it with `spec` after a green build of the spec bot's pull request,
System Image Check with `system` after a green build of a system bump. The script reads the
changed files from the GitHub API (status, path and patch of each), checks every changed line
against the bot's shape, waits for the required checks of the branch protection and merges
at HEAD_SHA, so a push after the check makes the merge fail instead of landing unchecked.
Nothing stays armed: a pull request the script does not merge waits for a person.

spec   — branch chore/update-specs-zero-trust (forge-util-update-specs.yml); only modified
         *.spec and SOURCES/sources.sha256 under forge/specs/<package>/ (azoth and
         athanor-telemetry, which Spec Build Check does not build, excluded); only Version
         lines (numeric), Release lines (N%{?dist}) and manifest entries; every Version keeps
         its leftmost non-zero component (1.6.0 -> 1.7.1 merges, 1.9 -> 2.0 and 0.3 -> 0.4 do
         not).
system — branch bump/system-* with label system-bump (kernel-bump.yml, system group); only
         modified system/Containerfile and system/nvidia/locks/*.lock; only new digests on
         the existing FROM lines, and lock entries whose URL is an RPM directly under the
         lock's own `# repository` line.

Usage: bot_merge.py spec|system PR HEAD_SHA BUILD_RESULT   (GH_TOKEN: a token that may merge)

A pull request of another shape, or a build that is not `success`, is reported in the log and
the job summary and the script exits 0. A failing gh call, a failing required check or a
refused merge exits non-zero.
"""

import json
import os
import re
import subprocess
import sys

NAME = r"[A-Za-z0-9._+-]+"
HEX64 = r"[0-9a-f]{64}"
SPEC_PATH = re.compile(
    rf"forge/specs/(?!azoth/|athanor-telemetry/){NAME}/({NAME}\.spec|SOURCES/sources\.sha256)"
)
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


def run_gh(*args):
    return subprocess.run(
        ["gh", *args], check=True, capture_output=True, text=True
    ).stdout


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
            "state,headRefName,headRefOid,isCrossRepository,labels",
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
    path = SPEC_PATH if kind == "spec" else SYSTEM_PATH

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
        if not path.fullmatch(f["filename"]):
            raise Refused(f"touches {f['filename']}, outside what the bot changes")
        if kind == "spec":
            check_spec(f)
        else:
            check_system(f, contents)


def report(pr, message):
    line = f"bot_merge: #{pr} {message}"
    print(line)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as s:
            s.write(line + "\n")


def main(kind, pr, sha, build, gh=run_gh):
    try:
        if build != "success":
            raise Refused(f"build is {build}")
        check(kind, pr, sha, gh)
    except Refused as reason:
        report(pr, f"stays for a person: {reason}")
        return 0
    # Exits non-zero when a required check fails: the job goes red and a person looks.
    gh("pr", "checks", pr, "--required", "--watch", "--fail-fast", "--interval", "30")
    gh("pr", "merge", pr, "--squash", "--match-head-commit", sha)
    report(pr, f"merged at {sha} ({kind} bot, the change has the bot's shape)")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 5 or sys.argv[1] not in BRANCH:
        sys.exit("usage: bot_merge.py spec|system PR HEAD_SHA BUILD_RESULT")
    sys.exit(main(*sys.argv[1:]))
