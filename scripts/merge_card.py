#!/usr/bin/env python3
"""Read the facts of the merge card of a pull request and print it (CLAUDE.md, "Merging").

Usage: merge_card.py [-R OWNER/REPO] [--base-rules-from FILE] PR
       merge_card.py [-R OWNER/REPO] PR --verify-approval CARD_TIME --head SHA [--base-sha SHA]
                     [--approver LOGIN ...]

The script only reads: it never approves, merges, comments or pushes, and it reads no token.
It prints the card, then one `REFUSED:` line on stderr per precondition that does not hold,
and exits 1 when there is one. A 404 on the branch protection means only the rulesets count.
--base-rules-from reads the rules of the base from a file (the JSON of
`gh api repos/<repo>/rules/branches/<base>`) in place of the API.
--verify-approval is the re-check just before the merge: it repeats the reading and adds that
the head and the base are the card's and that a code owner approved at the card's head after
CARD_TIME (ISO 8601, e.g. 2026-11-20T20:00:00Z). Without --approver the owners are those of
the `*` line of .github/CODEOWNERS on the base.

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
    if item.get("status") != "COMPLETED":
        return "pending"
    return {"SUCCESS": "pass", "NEUTRAL": "pass", "SKIPPED": "skipping"}.get(
        item.get("conclusion"), "fail"
    )


def latest_checks(rollup):
    """name -> (state, entry): the newest entry of each name, since a rerun adds one."""
    found = {}
    for item in rollup:
        name = item.get("name") or item.get("context")
        stamp = item.get("startedAt") or item.get("createdAt") or ""
        if name not in found or stamp >= found[name][0]:
            found[name] = (stamp, item)
    return {name: (check_state(item), item) for name, (_, item) in found.items()}


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
    """[(path, [changed line, ...])] from a unified diff."""
    files = []
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            files.append((line.rsplit(" b/", 1)[1], []))
        elif files and line[:1] in "+-" and not line.startswith(("+++", "---")):
            files[-1][1].append(line)
    return files


def path_matches(path, patterns):
    return any(
        path.startswith(p) if p.endswith("/") else fnmatch.fnmatch(path, p)
        for p in patterns
    )


def hunks(files, categories):
    """category name -> one line of evidence per changed file that touches it."""
    found = {}
    for cat in categories:
        regexes = [re.compile(p) for p in cat["patterns"]]
        for path, lines in files:
            if path_matches(path, cat["paths"]):
                found.setdefault(cat["name"], []).append(f"{path} (path)")
                continue
            if path.startswith("docs/") or "/tests/" in path:
                continue
            hit = next((x for x in lines if any(r.search(x) for r in regexes)), None)
            if hit:
                found.setdefault(cat["name"], []).append(f"{path}: {hit.strip()[:100]}")
    return found


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
    for name, (_, item) in sorted(checks.items()):
        run = re.search(r"/runs/(\d+)", item.get("detailsUrl") or "")
        if not run:
            return f"the head does not contain the base and check {name} has no workflow run"
        info = api(f"repos/{repo}/actions/runs/{run.group(1)}")
        if info["event"] != "pull_request" or when(
            info["created_at"]
        ) < landed + timedelta(minutes=1):
            return f"check {name} ran before the base head landed, or not as a pull_request run"
    return None


def code_owners(repo, base):
    raw = gh(
        "api",
        "-H",
        "Accept: application/vnd.github.raw",
        f"repos/{repo}/contents/.github/CODEOWNERS?ref={base}",
    )
    for line in raw.splitlines():
        parts = line.split()
        if parts[:1] == ["*"]:
            return {o.lstrip("@") for o in parts[1:]}
    return set()


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
        state = checks.get(name, ("missing",))[0]
        if state != "pass":
            bad.append(f"required check {name} is {state}")
    for name, (state, _) in sorted(checks.items()):
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
        repo, pr, base_sha, checks, any(r["type"] == "merge_queue" for r in rules)
    )
    if stale:
        bad.append(f"stale base: {stale}")
    if args.verify_approval:
        if pr["headRefOid"] != args.head:
            bad.append(f"head is {pr['headRefOid']}, the card's is {args.head}")
        if args.base_sha and base_sha != args.base_sha:
            bad.append(f"base head is {base_sha}, the card's is {args.base_sha}")
        owners = set(args.approver or code_owners(repo, base))
        card = when(args.verify_approval)
        if not any(
            r["state"] == "APPROVED"
            and r["author"]["login"] in owners
            and r["commit"]["oid"] == args.head
            and when(r["submittedAt"]) > card
            for r in pr["reviews"]
        ):
            bad.append(
                f"no APPROVED review by {sorted(owners)} at {args.head[:12]} after {args.verify_approval}"
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
    for name, (state, _) in sorted(latest_checks(pr["statusCheckRollup"]).items()):
        print(f"  {state:8} {name}{'  [required]' if name in required else ''}")
    reviews = latest_reviews(pr["reviews"])
    print(
        "latest reviews: "
        + (", ".join(f"{w} {s}" for w, s in sorted(reviews.items())) or "none")
    )
    found = hunks(files, categories)
    print("hunks:")
    for cat in categories:
        lines = found.get(cat["name"], [])
        for line in lines[:2]:
            print(f"  {cat['name']}: {line}")
        if len(lines) > 2:
            print(f"  {cat['name']}: and {len(lines) - 2} more")
    if not found:
        print("  none of the listed categories")


def main(argv):
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("pr", type=int)
    ap.add_argument("-R", "--repo")
    ap.add_argument("--base-rules-from")
    ap.add_argument("--verify-approval", metavar="CARD_TIME")
    ap.add_argument("--head", help="the card's full head sha (with --verify-approval)")
    ap.add_argument(
        "--base-sha", help="the card's base head sha (with --verify-approval)"
    )
    ap.add_argument("--approver", action="append")
    args = ap.parse_args(argv)
    if args.verify_approval and not args.head:
        ap.error("--verify-approval needs --head")

    repo = (
        args.repo
        or json.loads(gh("repo", "view", "--json", "nameWithOwner"))["nameWithOwner"]
    )
    pr = json.loads(gh("pr", "view", str(args.pr), "-R", repo, "--json", FIELDS))
    base = pr["baseRefName"]
    base_sha = api(f"repos/{repo}/branches/{base}")["commit"]["sha"]
    if args.base_rules_from:
        rules = json.loads(pathlib.Path(args.base_rules_from).read_text())
    else:
        rules = api(f"repos/{repo}/rules/branches/{base}")
    try:
        protection = api(f"repos/{repo}/branches/{base}/protection")
    except NotFound:
        protection = None
    files = parse_diff(gh("pr", "diff", str(args.pr), "-R", repo))
    categories = json.loads(CATEGORIES.read_text())["categories"]

    print_card(pr, base_sha, rules, protection, files, categories)
    bad = refusals(repo, pr, base_sha, rules, protection, args)
    for reason in bad:
        print(f"REFUSED: {reason}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
