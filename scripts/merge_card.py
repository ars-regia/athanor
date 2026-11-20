#!/usr/bin/env python3
"""Read the facts of the merge card of a pull request and print it (CLAUDE.md, "Merging").

Usage: merge_card.py [-R OWNER/REPO] PR
       merge_card.py [-R OWNER/REPO] PR --verify-approval CARD_TIME --head SHA --base-sha SHA
                     [--approver LOGIN ...]

The script only reads: it never approves, merges, comments or pushes, and it reads no token.
It prints the card, then one `REFUSED:` line on stderr per precondition that does not hold,
and exits 1 when there is one. A 404 on the branch protection means only the rulesets count.
--verify-approval is the re-check just before the merge: it repeats the reading and adds that
the head and the base are the card's and that a code owner approved at the card's head after
CARD_TIME (ISO 8601, e.g. 2026-11-20T20:00:00Z). The approver is `hr-mes` (the rule of CLAUDE.md,
"Merging") unless --approver names others; the last `*` line of .github/CODEOWNERS at the base sha must
list each approver, else the script refuses.

The hunks of the card come from scripts/merge_card_categories.json.
"""

import argparse
import fnmatch
import json
import pathlib
import re
import subprocess
import sys
from datetime import datetime, timedelta

CATEGORIES = pathlib.Path(__file__).with_name("merge_card_categories.json")
FIELDS = "number,state,baseRefName,headRefOid,isDraft,mergeable,reviewDecision,reviews,statusCheckRollup"


class NotFound(Exception):
    pass


def gh(*args):
    """The stdout of `gh`; NotFound on an HTTP 404; any other failure ends the script."""
    done = subprocess.run(["gh", *args], capture_output=True, text=True, check=False)
    if done.returncode == 0:
        return done.stdout
    if "HTTP 404" in done.stderr:
        raise NotFound(done.stderr.strip())
    sys.exit(f"merge_card: gh {' '.join(args[:3])} failed: {done.stderr.strip()}")


def api(path):
    return json.loads(gh("api", path))


def when(stamp):
    return datetime.fromisoformat(stamp.replace("Z", "+00:00"))


def check_state(item):
    """pass, fail, pending or skipping for one entry of statusCheckRollup."""
    if item.get("__typename") == "StatusContext":
        return {"SUCCESS": "pass", "PENDING": "pending", "EXPECTED": "pending"}.get(
            item.get("state"), "fail"
        )
    # A queued rerun carries startedAt 0001-01-01: not a usable time, so it is not finished.
    started = item.get("startedAt") or ""
    if item.get("status") != "COMPLETED" or started[:4] < "2000":
        return "pending"
    return {"SUCCESS": "pass", "NEUTRAL": "pass", "SKIPPED": "skipping"}.get(
        item.get("conclusion"), "fail"
    )


def latest_checks(rollup):
    """(workflow, name) -> (state, entry). A rerun adds an entry: one that is not finished
    wins over an older one, otherwise the newest wins."""
    found = {}
    for item in rollup:
        key = (item.get("workflowName") or "", item.get("name") or item.get("context"))
        state = check_state(item)
        stamp = item.get("startedAt") or item.get("createdAt") or ""
        old = found.get(key)
        if (
            old is None
            or (state == "pending") > (old[0] == "pending")
            or ((state == "pending") == (old[0] == "pending") and stamp >= old[2])
        ):
            found[key] = (state, item, stamp)
    return {key: (state, item) for key, (state, item, _) in found.items()}


def required_checks(rules, protection):
    names = set()
    for rule in rules:
        if rule["type"] == "required_status_checks":
            names |= {
                c["context"] for c in rule["parameters"]["required_status_checks"]
            }
    if protection:
        checks = protection.get("required_status_checks") or {}
        names |= set(checks.get("contexts", []))
        names |= {c["context"] for c in checks.get("checks", [])}
    return names


def latest_reviews(reviews):
    """login -> state of each reviewer's latest review that counts (a comment clears nothing)."""
    latest = {}
    for r in sorted(reviews, key=lambda r: r["submittedAt"]):
        if r["state"] not in ("COMMENTED", "PENDING"):
            latest[r["author"]["login"]] = r["state"]
    return latest


def parse_diff(diff):
    """[(path, [changed line, ...])] from a unified diff.

    Split on newlines only (a form feed or a NEL inside a changed line is text). A file
    starts at a `diff --git` line that begins a line outside a hunk: inside a hunk every line
    starts with a space, `+`, `-` or a backslash. The paths come from the header lines (`--- a/`,
    `+++ b/`, `rename from`, `copy from`, and their targets), so a rename or a deletion still
    names the path it left; `+++` and `---` are headers only before the first `@@`.
    """
    files, header, paths, lines = [], False, set(), []

    def close():
        for p in sorted(paths):
            files.append((p, lines))

    for line in diff.split("\n"):
        if line.startswith("diff --git "):
            close()
            header, paths, lines = True, set(), []
            if " b/" in line:
                paths.add(line.rsplit(" b/", 1)[1])
            continue
        if header:
            if line.startswith("@@"):
                header = False
            else:
                for prefix in ("rename from ", "rename to ", "copy from ", "copy to "):
                    if line.startswith(prefix):
                        paths.add(line[len(prefix) :])
                for prefix in ("--- a/", "+++ b/"):
                    if line.startswith(prefix):
                        paths.add(line[len(prefix) :])
                continue
        if line[:1] in ("+", "-"):
            lines.append(line)
    close()
    return files


def path_matches(path, patterns):
    return any(
        path.startswith(p) if p.endswith("/") else fnmatch.fnmatch(path, p)
        for p in patterns
    )


SLASH_COMMENT = (".rs", ".c", ".h", ".cc", ".cpp", ".hpp", ".js", ".ts")


def is_comment(path, line):
    """Whether a changed line (with its +/- mark) is a comment, by the type of its file."""
    if path.endswith(".md"):
        return True
    text = line[1:].lstrip()
    if path.endswith(SLASH_COMMENT):
        return text.startswith(("//", "/*", "*"))
    return text.startswith("#")


def hunks(files, categories):
    """(code, comments): category -> evidence lines, and category -> {path: comment lines}.

    A path category lists the file. A pattern category lists the first code line that matches
    in a file, and counts the comment lines that match, so that nothing is lost.
    """
    code, comments = {}, {}
    for cat in categories:
        regexes = [re.compile(p) for p in cat["patterns"]]
        for path, lines in files:
            if path_matches(path, cat["paths"]):
                code.setdefault(cat["name"], []).append(f"{path} (path)")
                continue
            if path.startswith("docs/") or "/tests/" in path:
                continue
            matching = [x for x in lines if any(r.search(x) for r in regexes)]
            hit = next((x for x in matching if not is_comment(path, x)), None)
            if hit:
                code.setdefault(cat["name"], []).append(f"{path}: {hit.strip()[:100]}")
            n = sum(is_comment(path, x) for x in matching)
            if n:
                comments.setdefault(cat["name"], {})[path] = n
    return code, comments


def stale_base(repo, pr, base_sha, checks, has_queue):
    """None when the checks ran on the current base, else the reason they may not have."""
    if has_queue:
        return None
    if api(f"repos/{repo}/compare/{base_sha}...{pr['headRefOid']}")["behind_by"] == 0:
        return None
    base = pr["baseRefName"]
    if base.startswith("rebase/"):
        return f"the head does not contain {base} at {base_sha[:12]}, and {base} takes pushes"
    landed = when(
        api(f"repos/{repo}/commits/{base_sha}")["commit"]["committer"]["date"]
    )
    for (_, name), (_, item) in sorted(checks.items()):
        run = re.search(r"/runs/(\d+)", item.get("detailsUrl") or "")
        if not run:
            return f"the head does not contain the base and check {name} has no workflow run"
        info = api(f"repos/{repo}/actions/runs/{run.group(1)}")
        if info["event"] != "pull_request" or when(
            info["created_at"]
        ) < landed + timedelta(minutes=1):
            return f"check {name} ran before the base head landed, or not as a pull_request run"
    return None


APPROVER = "hr-mes"  # the code owner whose approval CLAUDE.md ("Merging") requires


def code_owners(repo, base_sha):
    """The owners on the last `*` line of CODEOWNERS at the pinned base sha, comments removed."""
    raw = gh(
        "api",
        "-H",
        "Accept: application/vnd.github.raw",
        f"repos/{repo}/contents/.github/CODEOWNERS?ref={base_sha}",
    )
    owners = set()
    for line in raw.split("\n"):
        parts = line.split("#", 1)[0].split()
        if parts[:1] == ["*"]:
            owners = {o.lstrip("@") for o in parts[1:]}
    return owners


def refusals(repo, pr, base_sha, rules, protection, args):
    """Every precondition of the merge that does not hold, as a sentence."""
    base = pr["baseRefName"]
    checks = latest_checks(pr["statusCheckRollup"])
    required = required_checks(rules, protection)
    reviews = latest_reviews(pr["reviews"])
    bad = []
    if pr["state"] != "OPEN":
        bad.append(f"the pull request is {pr['state']}, not OPEN")
    if not required:
        bad.append(f"the base {base} has no required check")
    for name in sorted(required):
        states = [s for (_, n), (s, _) in sorted(checks.items()) if n == name]
        for state in states or ["missing"]:
            if state != "pass":
                bad.append(f"required check {name} is {state}")
    for (workflow, name), (state, _) in sorted(checks.items()):
        if name not in required and state not in ("pass", "skipping"):
            bad.append(f"check {workflow + ' / ' if workflow else ''}{name} is {state}")
    if not any(
        r["type"] == "pull_request" and r["parameters"].get("require_code_owner_review")
        for r in rules
    ):
        bad.append(
            f"the pull_request rule of {base} does not keep require_code_owner_review"
        )
    if pr["isDraft"]:
        bad.append("the pull request is a draft")
    if pr["mergeable"] != "MERGEABLE":
        bad.append(f"mergeable is {pr['mergeable']}")
    if pr["reviewDecision"] not in ("REVIEW_REQUIRED", "APPROVED", ""):
        bad.append(f"reviewDecision is {pr['reviewDecision']}")
    bad += [
        f"{who}'s latest review is CHANGES_REQUESTED"
        for who, state in sorted(reviews.items())
        if state == "CHANGES_REQUESTED"
    ]
    stale = stale_base(
        repo, pr, base_sha, checks, any(r["type"] == "merge_queue" for r in rules)
    )
    if stale:
        bad.append(f"stale base: {stale}")
    if args.verify_approval:
        if pr["headRefOid"] != args.head:
            bad.append(f"head is {pr['headRefOid']}, the card's is {args.head}")
        if base_sha != args.base_sha:
            bad.append(f"base head is {base_sha}, the card's is {args.base_sha}")
        approvers = set(args.approver or [APPROVER])
        owners = code_owners(repo, base_sha)
        bad += [
            f"{who} is not a code owner in CODEOWNERS at {base_sha[:12]}"
            for who in sorted(approvers - owners)
        ]
        card = when(args.verify_approval)
        if not any(
            r["state"] == "APPROVED"
            and r["author"]["login"] in approvers
            and r["commit"]["oid"] == args.head
            and when(r["submittedAt"]) > card
            for r in pr["reviews"]
        ):
            bad.append(
                f"no APPROVED review by {sorted(approvers)} at {args.head[:12]} after {args.verify_approval}"
            )
    return bad


def print_card(pr, base_sha, rules, protection, files, categories):
    base = pr["baseRefName"]
    required = required_checks(rules, protection)
    queue = any(r["type"] == "merge_queue" for r in rules)
    print(f"pull request #{pr['number']}  base {base} @ {base_sha}")
    print(f"head {pr['headRefOid']}")
    print(
        f"draft {pr['isDraft']}  mergeable {pr['mergeable']}  "
        f"reviewDecision {pr['reviewDecision'] or '(empty)'}  merge_queue {'yes' if queue else 'no'}"
    )
    print("checks:")
    for (workflow, name), (state, _) in sorted(
        latest_checks(pr["statusCheckRollup"]).items()
    ):
        print(
            f"  {state:8} {workflow + ' / ' if workflow else ''}{name}{'  [required]' if name in required else ''}"
        )
    reviews = latest_reviews(pr["reviews"])
    print(
        "latest reviews: "
        + (", ".join(f"{w} {s}" for w, s in sorted(reviews.items())) or "none")
    )
    code, comments = hunks(files, categories)
    print("hunks:")
    for cat in categories:
        lines = code.get(cat["name"], [])
        for line in lines[:2]:
            print(f"  {cat['name']}: {line}")
        if len(lines) > 2:
            print(f"  {cat['name']}: and {len(lines) - 2} more")
        if cat["name"] in comments:
            where = ", ".join(comments[cat["name"]])
            n = sum(comments[cat["name"]].values())
            noun = "comment line" if n == 1 else "comment lines"
            print(f"  {cat['name']}: also {n} {noun} in {where}")
    if not code and not comments:
        print("  none of the listed categories")


def main(argv):
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("pr", type=int)
    ap.add_argument("-R", "--repo")
    ap.add_argument("--verify-approval", metavar="CARD_TIME")
    ap.add_argument("--head", help="the card's full head sha (with --verify-approval)")
    ap.add_argument(
        "--base-sha", help="the card's base head sha (with --verify-approval)"
    )
    ap.add_argument("--approver", action="append")
    args = ap.parse_args(argv)
    if args.verify_approval and not (args.head and args.base_sha):
        ap.error("--verify-approval needs --head and --base-sha")

    repo = (
        args.repo
        or json.loads(gh("repo", "view", "--json", "nameWithOwner"))["nameWithOwner"]
    )
    pr = json.loads(gh("pr", "view", str(args.pr), "-R", repo, "--json", FIELDS))
    base = pr["baseRefName"]
    base_sha = api(f"repos/{repo}/branches/{base}")["commit"]["sha"]
    rules = api(f"repos/{repo}/rules/branches/{base}")
    try:
        protection = api(f"repos/{repo}/branches/{base}/protection")
    except NotFound:
        protection = None
    # The hunks belong to the head of the card: the diff is taken between the shas read above.
    files = parse_diff(
        gh(
            "api",
            "-H",
            "Accept: application/vnd.github.diff",
            f"repos/{repo}/compare/{base_sha}...{pr['headRefOid']}",
        )
    )
    categories = json.loads(CATEGORIES.read_text())["categories"]

    print_card(pr, base_sha, rules, protection, files, categories)
    bad = refusals(repo, pr, base_sha, rules, protection, args)
    now = json.loads(gh("pr", "view", str(args.pr), "-R", repo, "--json", "headRefOid"))
    if now["headRefOid"] != pr["headRefOid"]:
        bad.append(
            f"the head moved from {pr['headRefOid']} to {now['headRefOid']} while reading"
        )
    for reason in bad:
        print(f"REFUSED: {reason}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
