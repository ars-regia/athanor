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
CARD_TIME (ISO 8601 with a time zone, e.g. 2026-11-20T20:00:00Z). The approver is `hr-mes` (the
rule of CLAUDE.md, "Merging") unless --approver names others; the last `*` line of
.github/CODEOWNERS at the base sha must list each approver, else the script refuses.

Everything comes from the GitHub API, paginated, and no diff is parsed: the files from
pulls/<n>/files (the number must equal changedFiles and stay under the API's 3000 cap), the
reviews from pulls/<n>/reviews, the checks from commits/<head>/check-runs and /status. The
head, the base, its sha, the state and the reviews are read again at the end and must not
have changed. Without a merge_queue rule on the base, the checks must have run on the current
base: the head must contain the base head (ancestry only; the other way of workflow.md, the
timing of each run, is not used, so the reading fails closed on every base without a queue).

The categories of the card come from scripts/merge_card_categories.json. Paths are exact; the
patterns are heuristic, and a file with no patch (binary, too large) is listed, never clean.
"""

import argparse
import fnmatch
import json
import pathlib
import re
import subprocess
import sys
from datetime import datetime

CATEGORIES = pathlib.Path(__file__).with_name("merge_card_categories.json")
FIELDS = (
    "number,state,baseRefName,headRefOid,isDraft,mergeable,reviewDecision,changedFiles"
)
APPROVER = "hr-mes"  # the code owner whose approval CLAUDE.md ("Merging") requires
FILE_CAP = 3000  # the pulls/<n>/files API lists at most this many files
SLASH_COMMENT = (".rs", ".c", ".h", ".cc", ".cpp", ".hpp", ".js", ".ts")


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


def pages(path):
    """The pages of a paginated endpoint, none dropped."""
    return json.loads(gh("api", "--paginate", "--slurp", path))


def when(stamp):
    return datetime.fromisoformat(stamp.replace("Z", "+00:00"))


def check_state(item):
    """pass, fail, pending or skipping for one entry of a check."""
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


def read_checks(repo, head):
    """The check runs and the commit statuses of the head, as entries of one shape."""
    items = []
    for page in pages(f"repos/{repo}/commits/{head}/check-runs"):
        for r in page["check_runs"]:
            items.append(
                {
                    "__typename": "CheckRun",
                    "name": r["name"],
                    "status": (r["status"] or "").upper(),
                    "conclusion": (r["conclusion"] or "").upper(),
                    "startedAt": r["started_at"] or "",
                    "workflowName": str(r["check_suite"]["id"]),
                }
            )
    for page in pages(f"repos/{repo}/commits/{head}/status"):
        for s in page["statuses"]:
            items.append(
                {
                    "__typename": "StatusContext",
                    "context": s["context"],
                    "state": s["state"].upper(),
                    "createdAt": s["created_at"],
                }
            )
    return items


def latest_checks(items):
    """(workflow, name) -> (state, entry). A rerun adds an entry: one that is not finished
    wins over an older one, otherwise the newest wins."""
    found = {}
    for item in items:
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


def read_reviews(repo, n):
    return [
        {
            "author": {"login": (r["user"] or {}).get("login", "ghost")},
            "state": r["state"],
            "submittedAt": r["submitted_at"] or "",
            "commit": {"oid": r["commit_id"]},
        }
        for page in pages(f"repos/{repo}/pulls/{n}/reviews")
        for r in page
    ]


def latest_reviews(reviews):
    """login -> state of each reviewer's latest review that counts (a comment clears nothing)."""
    latest = {}
    for r in sorted(reviews, key=lambda r: r["submittedAt"]):
        if r["state"] not in ("COMMENTED", "PENDING"):
            latest[r["author"]["login"]] = r["state"]
    return latest


def read_files(repo, n):
    return [e for page in pages(f"repos/{repo}/pulls/{n}/files") for e in page]


def path_matches(path, patterns):
    return any(
        path.startswith(p) if p.endswith("/") else fnmatch.fnmatch(path, p)
        for p in patterns
    )


def is_comment(path, line):
    """Whether a changed line (with its +/- mark) is a comment, by the type of its file."""
    if path.endswith(".md"):
        return True
    text = line[1:].lstrip()
    if path.endswith(SLASH_COMMENT):
        return text.startswith(("//", "/*", "*"))
    return text.startswith("#")


def is_test(path):
    return path.startswith("tests/") or "/tests/" in path


def changed_lines(entry):
    """The + and - lines of a file's patch (a patch holds hunks only, no file header)."""
    return [
        x for x in (entry.get("patch") or "").split("\n") if x.startswith(("+", "-"))
    ]


def hunks(files, categories):
    """(code, comments, tests) by category, from the entries of pulls/<n>/files.

    code: evidence lines; a path category lists the file by `filename` or `previous_filename`;
    a pattern category lists the first code line that matches in a file. comments:
    {path: count} of matching comment lines. tests: the files under a tests directory that
    match a pattern. Only `docs/**/*.md` prose is left out of the patterns.
    """
    code, comments, tests = {}, {}, {}
    for cat in categories:
        regexes = [re.compile(p) for p in cat["patterns"]]
        for entry in files:
            path = entry["filename"]
            named = [path, entry.get("previous_filename") or path]
            hit = next(
                (
                    p
                    for p in named
                    if path_matches(p, cat["paths"])
                    and not path_matches(p, cat.get("exclude", []))
                ),
                None,
            )
            if hit:
                code.setdefault(cat["name"], []).append(f"{hit} (path)")
                continue
            if path.startswith("docs/") and path.endswith(".md"):
                continue
            matching = [
                x for x in changed_lines(entry) if any(r.search(x) for r in regexes)
            ]
            if not matching:
                continue
            if is_test(path):
                tests.setdefault(cat["name"], {})[path] = len(matching)
                continue
            first = next((x for x in matching if not is_comment(path, x)), None)
            if first:
                code.setdefault(cat["name"], []).append(
                    f"{path}: {first.strip()[:100]}"
                )
            n = sum(is_comment(path, x) for x in matching)
            if n:
                comments.setdefault(cat["name"], {})[path] = n
    return code, comments, tests


def stale_base(repo, pr, base_sha, has_queue):
    """None when the head contains the base head, else the reason the checks may be stale."""
    if has_queue:
        return None
    if api(f"repos/{repo}/compare/{base_sha}...{pr['headRefOid']}")["behind_by"] == 0:
        return None
    return f"the head does not contain {pr['baseRefName']} at {base_sha[:12]}"


def code_owners(repo, base_sha):
    """The owners on the last `*` line of CODEOWNERS at the pinned base sha, or None."""
    try:
        raw = gh(
            "api",
            "-H",
            "Accept: application/vnd.github.raw",
            f"repos/{repo}/contents/.github/CODEOWNERS?ref={base_sha}",
        )
    except NotFound:
        return None
    owners = set()
    for line in raw.split("\n"):
        parts = line.split("#", 1)[0].split()
        if parts[:1] == ["*"]:
            owners = {o.lstrip("@") for o in parts[1:]}
    return owners


def card_time(text):
    """The time of the card, or None when it is not an ISO 8601 time with a zone."""
    try:
        stamp = when(text)
    except ValueError:
        return None
    return stamp if stamp.tzinfo else None


def refusals(repo, pr, base_sha, rules, protection, args, files):
    """Every precondition of the merge that does not hold, as a sentence."""
    base = pr["baseRefName"]
    checks = latest_checks(pr["checks"])
    required = required_checks(rules, protection)
    reviews = latest_reviews(pr["reviews"])
    bad = []
    if pr["state"] != "OPEN":
        bad.append(f"the pull request is {pr['state']}, not OPEN")
    if len(files) != pr["changedFiles"]:
        bad.append(
            f"{len(files)} files read, the pull request has {pr['changedFiles']}"
        )
    if len(files) >= FILE_CAP:
        bad.append(
            f"{len(files)} files reach the API cap of {FILE_CAP}: the list is incomplete"
        )
    if not required:
        bad.append(f"the base {base} has no required check")
    for name in sorted(required):
        states = [s for (_, n), (s, _) in sorted(checks.items()) if n == name]
        for state in states or ["missing"]:
            if state != "pass":
                bad.append(f"required check {name} is {state}")
    for (_, name), (state, _) in sorted(checks.items()):
        if name not in required and state not in ("pass", "skipping"):
            bad.append(f"check {name} is {state}")
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
        repo, pr, base_sha, any(r["type"] == "merge_queue" for r in rules)
    )
    if stale:
        bad.append(f"stale base: {stale}")
    if args.verify_approval is not None:
        card = card_time(args.verify_approval)
        if card is None:
            bad.append(
                f"the card time {args.verify_approval!r} is not an ISO 8601 time with a time zone"
            )
        if pr["headRefOid"] != args.head:
            bad.append(f"head is {pr['headRefOid']}, the card's is {args.head}")
        if base_sha != args.base_sha:
            bad.append(f"base head is {base_sha}, the card's is {args.base_sha}")
        approvers = set(args.approver or [APPROVER])
        owners = code_owners(repo, base_sha)
        if owners is None:
            bad.append(f"CODEOWNERS is not found at {base_sha[:12]}")
        else:
            bad += [
                f"{who} is not a code owner in CODEOWNERS at {base_sha[:12]}"
                for who in sorted(approvers - owners)
            ]
        if card is not None and not any(
            r["state"] == "APPROVED"
            and r["author"]["login"] in approvers
            and r["commit"]["oid"] == args.head
            and r["submittedAt"]
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
    for (_, name), (state, _) in sorted(latest_checks(pr["checks"]).items()):
        print(f"  {state:8} {name}{'  [required]' if name in required else ''}")
    reviews = latest_reviews(pr["reviews"])
    print(
        "latest reviews: "
        + (", ".join(f"{w} {s}" for w, s in sorted(reviews.items())) or "none")
    )
    code, comments, tests = hunks(files, categories)
    print(
        f"hunks ({len(files)} files, the list is complete; the patterns are heuristic, the paths exact):"
    )
    for cat in categories:
        name = cat["name"]
        lines = code.get(name, [])
        for line in lines[:2]:
            print(f"  {name}: {line}")
        if len(lines) > 2:
            print(f"  {name}: and {len(lines) - 2} more")
        if name in comments:
            n = sum(comments[name].values())
            noun = "comment line" if n == 1 else "comment lines"
            print(f"  {name}: also {n} {noun} in {', '.join(comments[name])}")
        if name in tests:
            print(f"  {name}: in tests: {', '.join(tests[name])}")
    if not (code or comments or tests):
        print("  none of the listed categories")
    for entry in files:
        if not entry.get("patch"):
            print(
                f"  no patch shown: review the whole file: {entry['filename']} ({entry['status']})"
            )


def snapshot(repo, n, base):
    """What the reading rests on, read again at the end: it must not have moved."""
    now = json.loads(
        gh("pr", "view", str(n), "-R", repo, "--json", "headRefOid,baseRefName,state")
    )
    return (
        now["headRefOid"],
        now["baseRefName"],
        now["state"],
        api(f"repos/{repo}/branches/{base}")["commit"]["sha"],
        read_reviews(repo, n),
    )


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
    if args.verify_approval is not None and not (args.head and args.base_sha):
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
    pr["checks"] = read_checks(repo, pr["headRefOid"])
    pr["reviews"] = read_reviews(repo, args.pr)
    files = read_files(repo, args.pr)
    categories = json.loads(CATEGORIES.read_text())["categories"]

    print_card(pr, base_sha, rules, protection, files, categories)
    bad = refusals(repo, pr, base_sha, rules, protection, args, files)
    before = (pr["headRefOid"], base, pr["state"], base_sha, pr["reviews"])
    after = snapshot(repo, args.pr, base)
    for label, old, new in zip(
        ("head", "base", "state", "base head", "reviews"), before, after
    ):
        if old != new:
            bad.append(f"the {label} changed while reading")
    for reason in bad:
        print(f"REFUSED: {reason}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
