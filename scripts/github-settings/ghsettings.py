#!/usr/bin/env python3
"""GitHub repository settings as code: export, diff and apply through `gh api`.

The desired state lives in .github/settings/<area>.json, one normalised file per
area. `export` writes the live state there, `diff` exits 1 when the live state
differs from the files, and `apply` prints the calls that make the live state
match the files and runs them only with --yes. Secret values are never read or
written: `apply` lists the missing secret names for a human to set.
See docs/operations/github-settings.md.
"""

import argparse
import json
import pathlib
import subprocess
import sys
import urllib.parse
from collections import namedtuple
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_DIR = ROOT / ".github" / "settings"

REPO_FIELDS = (
    "allow_auto_merge",
    "allow_forking",
    "allow_merge_commit",
    "allow_rebase_merge",
    "allow_squash_merge",
    "allow_update_branch",
    "default_branch",
    "delete_branch_on_merge",
    "description",
    "has_discussions",
    "has_issues",
    "has_projects",
    "has_wiki",
    "homepage",
    "is_template",
    "merge_commit_message",
    "merge_commit_title",
    "squash_merge_commit_message",
    "squash_merge_commit_title",
    "visibility",
    "web_commit_signoff_required",
)
PROTECTION_FLAGS = (
    "allow_deletions",
    "allow_force_pushes",
    "allow_fork_syncing",
    "block_creations",
    "enforce_admins",
    "lock_branch",
    "required_conversation_resolution",
    "required_linear_history",
    "required_signatures",
)
SECRETS_DOC = "docs/operations/secrets.md"
# Keys a settings file declares that GitHub does not store: export keeps them from the
# file, apply never reads them, and diff acts on them instead of comparing them
# (docs/operations/github-settings.md, GHS10).
DECLARATIONS = {"actions": ("personal_tokens",)}


class NotFound(Exception):
    pass


class GhError(Exception):
    pass


class SettingsError(Exception):
    pass


# One API call of a plan. A destructive call deletes something (a protection, a
# ruleset, a label, the Pages site) and runs only with --allow-destructive.
Call = namedtuple("Call", "method path body destructive note", defaults=(False, ""))

# The keys each settings file must hold, checked when the file is loaded:
# (JSON type, keys of the top object, keys of each entry of the object or list).
RULESET_KEYS = ("target", "enforcement", "conditions", "rules", "bypass_actors")
PAGES_KEYS = ("build_type", "cname", "https_enforced", "source")
SHAPES = {
    "repository": (
        dict,
        REPO_FIELDS
        + (
            "topics",
            "security_and_analysis",
            "vulnerability_alerts",
            "private_vulnerability_reporting",
        ),
        None,
    ),
    # An entry may also be {} or null: the protection of that branch is removed.
    "branch-protection": (
        dict,
        None,
        PROTECTION_FLAGS
        + ("required_status_checks", "required_pull_request_reviews", "restrictions"),
    ),
    "rulesets": (dict, None, RULESET_KEYS),
    "environments": (
        dict,
        None,
        (
            "can_admins_bypass",
            "deployment_branch_policy",
            "prevent_self_review",
            "reviewers",
            "wait_timer",
            "secrets",
            "variables",
        ),
    ),
    "labels": (list, None, ("name", "color", "description")),
    "pages": ((dict, type(None)), PAGES_KEYS, None),
    "actions": (
        dict,
        (
            "permissions",
            "selected_actions",
            "workflow_permissions",
            "fork_pr_contributor_approval",
            "secrets",
            "variables",
        ),
        None,
    ),
}


def gh(path, method="GET", body=None, paginate=False):
    """One `gh api` call; JSON in and out. A 404 raises NotFound."""
    cmd = ["gh", "api", "--method", method, path]
    if paginate:
        cmd += ["--paginate", "--slurp"]
    if body is not None:
        cmd += ["--input", "-"]
    try:
        run = subprocess.run(
            cmd,
            input=None if body is None else json.dumps(body),
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError as error:
        raise GhError(
            "the gh command is not on PATH; install the GitHub CLI"
        ) from error
    if run.returncode != 0:
        if "(HTTP 404)" in run.stderr:
            raise NotFound(path)
        raise GhError(f"gh api --method {method} {path}: {run.stderr.strip()}")
    try:
        return json.loads(run.stdout) if run.stdout.strip() else None
    except json.JSONDecodeError as error:
        raise GhError(
            f"gh api --method {method} {path}: the answer is not valid JSON: {error}"
        ) from error


def get(path, paginate=False) -> Any:
    """A GET that must answer a JSON body: an empty answer is an error, not a None."""
    answer = gh(path, paginate=paginate)
    if answer is None:
        raise GhError(f"gh api {path}: empty response where JSON was expected")
    return answer


def gh_list(path, key=None):
    """All items of a paginated list endpoint; `key` names the array of an object page."""
    sep = "&" if "?" in path else "?"
    pages = get(f"{path}{sep}per_page=100", paginate=True)
    return [item for page in pages for item in (page[key] if key else page)]


def q(name):
    return urllib.parse.quote(name, safe="")


def enabled_unless_404(path):
    """For an endpoint whose documented answer is 204 when on and 404 when off
    (Dependabot alerts). Any other failure is an error."""
    try:
        gh(path)
        return True
    except NotFound:
        return False


def check_rights(repo):
    """Every area is read from admin endpoints. Without the admin role GitHub hides
    them or answers 404, which must never be read as "off"."""
    try:
        permissions = get(f"repos/{repo}").get("permissions") or {}
    except NotFound as error:
        raise GhError(
            f"repository {repo} not found or not visible to this token"
        ) from error
    if permissions.get("admin") is not True:
        raise GhError(
            f"insufficient rights: the token is not admin on {repo} (permissions.admin is not true)"
        )


# Export: the live state of each area, normalised (sorted, no ids, URLs, timestamps or counts).


# The merge settings, read through GraphQL: the REST repository object returns them as
# null to a GitHub App installation token, even one with Administration read (observed
# 2026-10-08, maintenance.yml run 37840869909), and GraphQL returns them to both tokens.
MERGE_FIELDS = {
    "allow_auto_merge": "autoMergeAllowed",
    "allow_merge_commit": "mergeCommitAllowed",
    "allow_rebase_merge": "rebaseMergeAllowed",
    "allow_squash_merge": "squashMergeAllowed",
    "allow_update_branch": "allowUpdateBranch",
    "delete_branch_on_merge": "deleteBranchOnMerge",
    "merge_commit_message": "mergeCommitMessage",
    "merge_commit_title": "mergeCommitTitle",
    "squash_merge_commit_message": "squashMergeCommitMessage",
    "squash_merge_commit_title": "squashMergeCommitTitle",
}


def merge_settings(repo):
    owner, name = repo.split("/", 1)
    fields = " ".join(MERGE_FIELDS.values())
    query = (
        "query($o: String!, $n: String!) "
        f"{{ repository(owner: $o, name: $n) {{ {fields} }} }}"
    )
    body = {"query": query, "variables": {"o": owner, "n": name}}
    answer = gh("graphql", method="POST", body=body)
    live = ((answer or {}).get("data") or {}).get("repository")
    if not live or any(live.get(f) is None for f in MERGE_FIELDS.values()):
        raise GhError(f"gh api graphql: the merge settings of {repo} are not readable")
    return {k: live[f] for k, f in MERGE_FIELDS.items()}


def export_repository(repo):
    r = get(f"repos/{repo}")
    out: dict[str, Any] = {k: r.get(k) for k in REPO_FIELDS}
    out.update(merge_settings(repo))
    out["description"] = r.get("description") or ""
    out["homepage"] = r.get("homepage") or ""
    out["topics"] = sorted(r.get("topics") or [])
    out["security_and_analysis"] = {
        k: v["status"] for k, v in (r.get("security_and_analysis") or {}).items()
    }
    out["vulnerability_alerts"] = enabled_unless_404(
        f"repos/{repo}/vulnerability-alerts"
    )
    out["private_vulnerability_reporting"] = get(
        f"repos/{repo}/private-vulnerability-reporting"
    )["enabled"]
    return out


def _actors(a):
    if a is None:
        return None
    return {
        "users": sorted(u["login"] for u in a.get("users", [])),
        "teams": sorted(t["slug"] for t in a.get("teams", [])),
        "apps": sorted(x["slug"] for x in a.get("apps", [])),
    }


def _protection(p):
    out: dict[str, Any] = {
        k: bool((p.get(k) or {}).get("enabled", False)) for k in PROTECTION_FLAGS
    }
    checks = p.get("required_status_checks")
    out["required_status_checks"] = (
        None
        if not checks
        else {
            "strict": checks.get("strict", False),
            "checks": sorted(
                (
                    {"context": c["context"], "app_id": c.get("app_id")}
                    for c in checks.get("checks", [])
                ),
                key=lambda c: c["context"],
            ),
        }
    )
    reviews = p.get("required_pull_request_reviews")
    out["required_pull_request_reviews"] = (
        None
        if not reviews
        else {
            "dismiss_stale_reviews": reviews.get("dismiss_stale_reviews", False),
            "require_code_owner_reviews": reviews.get(
                "require_code_owner_reviews", False
            ),
            "require_last_push_approval": reviews.get(
                "require_last_push_approval", False
            ),
            "required_approving_review_count": reviews.get(
                "required_approving_review_count", 0
            ),
            "dismissal_restrictions": _actors(reviews.get("dismissal_restrictions")),
            "bypass_pull_request_allowances": _actors(
                reviews.get("bypass_pull_request_allowances")
            ),
        }
    )
    out["restrictions"] = _actors(p.get("restrictions"))
    return out


def export_branch_protection(repo):
    names = [b["name"] for b in gh_list(f"repos/{repo}/branches?protected=true")]
    return {
        n: _protection(get(f"repos/{repo}/branches/{q(n)}/protection")) for n in names
    }


def _live_rulesets(repo):
    """{name: (id, normalised)} of the rulesets this repository owns."""
    out = {}
    for item in gh_list(f"repos/{repo}/rulesets?includes_parents=false"):
        r = get(f"repos/{repo}/rulesets/{item['id']}")
        out[r["name"]] = (
            r["id"],
            {k: r.get(k) for k in RULESET_KEYS},
        )
    return out


def export_rulesets(repo):
    return {name: body for name, (_, body) in _live_rulesets(repo).items()}


# A GitHub App installation token with Administration read gets a ruleset's
# bypass_actors as null over REST and each actor as null over GraphQL; GraphQL still
# answers how many there are (observed 2026-10-08, maintenance.yml run 37845913138).
# Reading the actors needs write access to the administration settings.
def bypass_counts(repo):
    """{ruleset name: number of bypass actors} of the rulesets this repository owns."""
    owner, name = repo.split("/", 1)
    query = (
        "query($o: String!, $n: String!) { repository(owner: $o, name: $n) "
        "{ rulesets(first: 100, includeParents: false) "
        "{ nodes { name bypassActors { totalCount } } } } }"
    )
    body = {"query": query, "variables": {"o": owner, "n": name}}
    answer = gh("graphql", method="POST", body=body)
    live = ((answer or {}).get("data") or {}).get("repository") or {}
    nodes = (live.get("rulesets") or {}).get("nodes")
    if nodes is None:
        raise GhError(f"gh api graphql: the rulesets of {repo} are not readable")
    return {n["name"]: n["bypassActors"]["totalCount"] for n in nodes}


def hidden_bypass(repo, want, live):
    """Compares by count the bypass actors this token cannot read, and takes them out of
    want and live so that compare() leaves them alone. Returns (drift lines, notes)."""
    hidden = [n for n, r in live.items() if r.get("bypass_actors") is None and n in want]
    if not hidden:
        return [], []
    counts = bypass_counts(repo)
    lines, notes = [], []
    for name in sorted(hidden):
        live[name].pop("bypass_actors")
        wanted = len(want[name].pop("bypass_actors", None) or [])
        path = f"rulesets {name}.bypass_actors"
        if counts.get(name) != wanted:
            lines.append(f"drift {path}: file {wanted} actor(s), live {counts.get(name)}")
        else:
            notes.append(
                f"note {path}: {wanted} actor(s) as in the file; who they are and their "
                "bypass mode are not readable with this token"
            )
    return lines, notes


def _names(path, key):
    return sorted(item["name"] for item in gh_list(path, key))


def export_environments(repo):
    out = {}
    for e in gh_list(f"repos/{repo}/environments", "environments"):
        name, base = e["name"], f"repos/{repo}/environments/{q(e['name'])}"
        rules = {r["type"]: r for r in e.get("protection_rules", [])}
        reviewers = rules.get("required_reviewers", {})
        policy = e.get("deployment_branch_policy")
        if policy is not None:
            policy = {
                "protected_branches": policy["protected_branches"],
                "custom_branch_policies": policy["custom_branch_policies"],
            }
            if policy["custom_branch_policies"]:
                policy["policies"] = sorted(
                    (
                        {"name": b["name"], "type": b.get("type", "branch")}
                        for b in gh_list(
                            f"{base}/deployment-branch-policies", "branch_policies"
                        )
                    ),
                    key=lambda b: (b["type"], b["name"]),
                )
        out[name] = {
            "can_admins_bypass": e.get("can_admins_bypass", True),
            "deployment_branch_policy": policy,
            "prevent_self_review": reviewers.get("prevent_self_review", False),
            "reviewers": sorted(
                (
                    {
                        "type": r["type"],
                        "name": r["reviewer"].get("login") or r["reviewer"].get("slug"),
                    }
                    for r in reviewers.get("reviewers", [])
                ),
                key=lambda r: (r["type"], r["name"]),
            ),
            "wait_timer": rules.get("wait_timer", {}).get("wait_timer", 0),
            "secrets": _names(f"{base}/secrets", "secrets"),
            "variables": _names(f"{base}/variables", "variables"),
        }
    return out


def export_labels(repo):
    return sorted(
        (
            {
                "name": l["name"],
                "color": l["color"],
                "description": l.get("description") or "",
            }
            for l in gh_list(f"repos/{repo}/labels")
        ),
        key=lambda l: l["name"],
    )


def export_pages(repo):
    try:
        p = get(f"repos/{repo}/pages")
    except NotFound:  # documented: 404 when the repository has no Pages site
        return None
    return {k: p.get(k) for k in PAGES_KEYS}


def export_actions(repo):
    base = f"repos/{repo}/actions"
    perm = get(f"{base}/permissions")
    selected = None
    if perm.get("allowed_actions") == "selected":
        s = get(f"{base}/permissions/selected-actions")
        selected = {**s, "patterns_allowed": sorted(s.get("patterns_allowed", []))}
    return {
        "permissions": {
            k: perm.get(k)
            for k in ("enabled", "allowed_actions", "sha_pinning_required")
        },
        "selected_actions": selected,
        "workflow_permissions": get(f"{base}/permissions/workflow"),
        "fork_pr_contributor_approval": get(
            f"{base}/permissions/fork-pr-contributor-approval"
        )["approval_policy"],
        "secrets": _names(f"{base}/secrets", "secrets"),
        "variables": _names(f"{base}/variables", "variables"),
    }


# Plan: the calls that turn `have` into `want`, plus the steps only a human can take.


def plan_repository(repo, have, want):
    calls, manual = [], []
    patch = {k: want[k] for k in REPO_FIELDS if have.get(k) != want[k]}
    sa = {
        k: {"status": v}
        for k, v in want["security_and_analysis"].items()
        if have["security_and_analysis"].get(k) != v
    }
    if sa:
        patch["security_and_analysis"] = sa
    if patch:
        calls.append(("PATCH", f"repos/{repo}", patch))
    if have["topics"] != want["topics"]:
        calls.append(("PUT", f"repos/{repo}/topics", {"names": want["topics"]}))
    for key, path in (
        ("vulnerability_alerts", "vulnerability-alerts"),
        ("private_vulnerability_reporting", "private-vulnerability-reporting"),
    ):
        if have[key] != want[key]:
            calls.append(
                ("PUT" if want[key] else "DELETE", f"repos/{repo}/{path}", None)
            )
    return calls, manual


def _protection_body(p):
    body = {k: p[k] for k in PROTECTION_FLAGS if k != "required_signatures"}
    checks = p["required_status_checks"]
    body["required_status_checks"] = (
        None
        if checks is None
        else {
            "strict": checks["strict"],
            "checks": [
                {k: v for k, v in c.items() if v is not None} for c in checks["checks"]
            ],
        }
    )
    reviews = p["required_pull_request_reviews"]
    body["required_pull_request_reviews"] = (
        None if reviews is None else {k: v for k, v in reviews.items() if v is not None}
    )
    body["restrictions"] = p["restrictions"]
    return body


def plan_branch_protection(repo, have, want):
    calls = []
    for name, p in want.items():
        base = f"repos/{repo}/branches/{q(name)}/protection"
        old = have.get(name)
        if not p:
            if old is not None:
                calls.append(
                    Call(
                        "DELETE", base, None, True, f"branch {name}: protection removed"
                    )
                )
            continue
        if old is None or _protection_body(old) != _protection_body(p):
            calls.append(("PUT", base, _protection_body(p)))
        if (old or {}).get("required_signatures", False) != p["required_signatures"]:
            calls.append(
                (
                    "POST" if p["required_signatures"] else "DELETE",
                    f"{base}/required_signatures",
                    None,
                )
            )
    calls += [
        Call(
            "DELETE",
            f"repos/{repo}/branches/{q(n)}/protection",
            None,
            True,
            f"branch {n}: protection removed",
        )
        for n in have
        if n not in want
    ]
    return calls, []


def plan_rulesets(repo, have, want):
    live = _live_rulesets(repo) if have else {}
    calls = []
    for name, body in want.items():
        if name not in live:
            calls.append(
                Call(
                    "POST",
                    f"repos/{repo}/rulesets",
                    {"name": name, **body},
                    False,
                    f"ruleset {name}",
                )
            )
        elif live[name][1] != body:
            calls.append(
                Call(
                    "PUT",
                    f"repos/{repo}/rulesets/{live[name][0]}",
                    {"name": name, **body},
                    False,
                    f"ruleset {name}",
                )
            )
    calls += [
        Call("DELETE", f"repos/{repo}/rulesets/{rid}", None, True, f"ruleset {name}")
        for name, (rid, _) in live.items()
        if name not in want
    ]
    return calls, []


def _reviewer_id(repo, reviewer):
    if reviewer["type"] == "Team":
        return get(f"orgs/{repo.split('/')[0]}/teams/{q(reviewer['name'])}")["id"]
    return get(f"users/{q(reviewer['name'])}")["id"]


def _names_steps(scope, kind, have, want):
    steps = [
        f"set {kind} {n} in {scope} by hand (value from {SECRETS_DOC})"
        for n in want
        if n not in have
    ]
    steps += [
        f"{kind} {n} in {scope} is not in the files: delete it by hand or add it to the files"
        for n in have
        if n not in want
    ]
    return steps


def _env_core(env):
    """The part of an environment one PUT sets: no branch policy list, secrets or variables."""
    policy = env["deployment_branch_policy"]
    return {
        "deployment_branch_policy": policy
        and {k: v for k, v in policy.items() if k != "policies"},
        "prevent_self_review": env["prevent_self_review"],
        "reviewers": env["reviewers"],
        "wait_timer": env["wait_timer"],
    }


def plan_environments(repo, have, want):
    calls, manual = [], []
    for name, env in want.items():
        old = have.get(name)
        base = f"repos/{repo}/environments/{q(name)}"
        scope = f"environment {name}"
        if old is None or _env_core(old) != _env_core(env):
            calls.append(
                (
                    "PUT",
                    base,
                    {
                        "wait_timer": env["wait_timer"],
                        "prevent_self_review": env["prevent_self_review"],
                        "reviewers": [
                            {"type": r["type"], "id": _reviewer_id(repo, r)}
                            for r in env["reviewers"]
                        ],
                        "deployment_branch_policy": _env_core(env)[
                            "deployment_branch_policy"
                        ],
                    },
                )
            )
        want_pol = (env["deployment_branch_policy"] or {}).get("policies", [])
        have_pol = ((old or {}).get("deployment_branch_policy") or {}).get(
            "policies", []
        )
        if want_pol != have_pol:
            ids = {}
            if have_pol:
                ids = {
                    (b["name"], b.get("type", "branch")): b["id"]
                    for b in gh_list(
                        f"{base}/deployment-branch-policies", "branch_policies"
                    )
                }
            calls += [
                ("POST", f"{base}/deployment-branch-policies", b)
                for b in want_pol
                if b not in have_pol
            ]
            calls += [
                (
                    "DELETE",
                    f"{base}/deployment-branch-policies/{ids[(b['name'], b['type'])]}",
                    None,
                )
                for b in have_pol
                if b not in want_pol
            ]
        # A new environment starts with the bypass allowed.
        if (old or {"can_admins_bypass": True})["can_admins_bypass"] != env[
            "can_admins_bypass"
        ]:
            manual.append(
                f"{scope}: set 'Allow administrators to bypass configured protection rules' to "
                f"{env['can_admins_bypass']} (Settings > Environments > {name}); the REST API cannot set it"
            )
        manual += _names_steps(
            scope, "secret", (old or {}).get("secrets", []), env["secrets"]
        )
        manual += _names_steps(
            scope, "variable", (old or {}).get("variables", []), env["variables"]
        )
    manual += [
        f"environment {n} is not in the files: delete it by hand (Settings > Environments) "
        "after saving the secrets it holds, or add it to the files"
        for n in have
        if n not in want
    ]
    return calls, manual


def plan_labels(repo, have, want):
    old = {l["name"]: l for l in have}
    new = {l["name"]: l for l in want}
    calls = [
        ("POST", f"repos/{repo}/labels", l) for n, l in new.items() if n not in old
    ]
    calls += [
        (
            "PATCH",
            f"repos/{repo}/labels/{q(n)}",
            {"color": l["color"], "description": l["description"]},
        )
        for n, l in new.items()
        if n in old and old[n] != l
    ]
    calls += [
        Call(
            "DELETE",
            f"repos/{repo}/labels/{q(n)}",
            None,
            True,
            f"label {n}: removed from every issue and pull request",
        )
        for n in old
        if n not in new
    ]
    return calls, []


def plan_pages(repo, have, want):
    path = f"repos/{repo}/pages"
    if want is None:
        return (
            [Call("DELETE", path, None, True, "Pages site unpublished")]
            if have is not None
            else []
        ), []
    calls = []
    if have is None:
        calls.append(
            (
                "POST",
                path,
                {k: want[k] for k in ("build_type", "source") if want[k] is not None},
            )
        )
    if have != want:
        calls.append(
            (
                "PUT",
                path,
                {k: v for k, v in want.items() if v is not None or k == "cname"},
            )
        )
    return calls, []


def plan_actions(repo, have, want):
    base = f"repos/{repo}/actions/permissions"
    calls = []
    if have["permissions"] != want["permissions"]:
        calls.append(
            (
                "PUT",
                base,
                {k: v for k, v in want["permissions"].items() if v is not None},
            )
        )
    if (
        want["selected_actions"] is not None
        and have["selected_actions"] != want["selected_actions"]
    ):
        calls.append(("PUT", f"{base}/selected-actions", want["selected_actions"]))
    if have["workflow_permissions"] != want["workflow_permissions"]:
        calls.append(("PUT", f"{base}/workflow", want["workflow_permissions"]))
    if have["fork_pr_contributor_approval"] != want["fork_pr_contributor_approval"]:
        calls.append(
            (
                "PUT",
                f"{base}/fork-pr-contributor-approval",
                {"approval_policy": want["fork_pr_contributor_approval"]},
            )
        )
    manual = _names_steps(
        "repository Actions", "secret", have["secrets"], want["secrets"]
    )
    manual += _names_steps(
        "repository Actions", "variable", have["variables"], want["variables"]
    )
    return calls, manual


AREAS = {
    "repository": (export_repository, plan_repository),
    "branch-protection": (export_branch_protection, plan_branch_protection),
    "rulesets": (export_rulesets, plan_rulesets),
    "environments": (export_environments, plan_environments),
    "labels": (export_labels, plan_labels),
    "pages": (export_pages, plan_pages),
    "actions": (export_actions, plan_actions),
}


def dump(data):
    return json.dumps(data, indent=2, sort_keys=True) + "\n"


def load(directory, area):
    path = directory / f"{area}.json"
    try:
        data = json.loads(path.read_text())
    except OSError as error:
        raise SettingsError(f"cannot read {path}: {error.strerror or error}") from error
    except json.JSONDecodeError as error:
        raise SettingsError(f"{path} is not valid JSON: {error}") from error
    kind, top_keys, entry_keys = SHAPES[area]
    if not isinstance(data, kind):
        raise SettingsError(
            f"{path} holds a JSON {type(data).__name__}, not the shape this area expects"
        )
    if data is not None and top_keys:
        _require(str(path), data, top_keys)
    if entry_keys and data is not None:
        entries = data.items() if isinstance(data, dict) else enumerate(data)
        for name, entry in entries:
            if area == "branch-protection" and not entry:
                continue
            _require(f"{path} entry {name}", entry, entry_keys)
    return data


def _require(where, obj, keys):
    if not isinstance(obj, dict):
        raise SettingsError(f"{where} must be a JSON object")
    missing = sorted(set(keys) - obj.keys())
    if missing:
        raise SettingsError(f"{where} lacks the key(s): {', '.join(missing)}")


def without_declarations(area, data):
    """The part of a settings file GitHub stores (see DECLARATIONS)."""
    if not isinstance(data, dict):
        return data
    return {k: v for k, v in data.items() if k not in DECLARATIONS.get(area, ())}


def cmd_export(repo, directory, _args):
    directory.mkdir(parents=True, exist_ok=True)
    # Every area is read before the first write, so a refusal leaves no partial export.
    exported = {area: export(repo) for area, (export, _) in AREAS.items()}
    if any(r["bypass_actors"] is None for r in exported["rulesets"].values()):
        raise GhError(
            "the bypass actors of the rulesets are not readable with this token: "
            "export needs write access to the administration settings"
        )
    for area, data in exported.items():
        path = directory / f"{area}.json"
        if area in DECLARATIONS and path.exists():
            old = load(directory, area)
            data.update({k: old[k] for k in DECLARATIONS[area] if k in old})
        path.write_text(dump(data))
        print(f"wrote {path}")
    return 0


ABSENT = object()
# The field that names an entry of a list of objects, in order of preference.
ENTRY_KEYS = ("name", "context", "type")


def _show(value):
    return "absent" if value is ABSENT else json.dumps(value, sort_keys=True)


def _entry_key(want, live):
    """The field that names every entry of both lists once, or None."""
    for key in ENTRY_KEYS:
        if all(isinstance(x, dict) and key in x for x in want + live) and all(
            len({json.dumps(x[key]) for x in side}) == len(side) for side in (want, live)
        ):
            return key
    return None


def _membership(path, want, live):
    for name in sorted(want, key=json.dumps):
        if name not in live:
            yield f"{path}: {json.dumps(name)} is in the file but not live"
    for name in sorted(live, key=json.dumps):
        if name not in want:
            yield f"{path}: {json.dumps(name)} is live but not in the file"


def compare(path, want, live):
    """One line per difference between a file and the live state: objects by key, lists
    of names and lists of named entries by name (their order is not a setting), anything
    else as a whole."""
    if isinstance(want, dict) and isinstance(live, dict):
        for key in sorted(want.keys() | live.keys()):
            yield from compare(
                f"{path}.{key}" if path else key,
                want.get(key, ABSENT),
                live.get(key, ABSENT),
            )
        return
    if isinstance(want, list) and isinstance(live, list):
        if all(isinstance(x, str) for x in want + live):
            yield from _membership(path, want, live)
            return
        key = _entry_key(want, live)
        if key:
            w, l = {x[key]: x for x in want}, {x[key]: x for x in live}
            yield from _membership(path, w, l)
            for name in sorted(w.keys() & l.keys(), key=json.dumps):
                yield from compare(f"{path}[{name}]", w[name], l[name])
            return
    if want != live:
        yield f"{path}: file {_show(want)}, live {_show(live)}"


def _personal_tokens(want, live):
    """Applies the personal_tokens declaration of actions.json: the personal access
    tokens the GitHub App identities replace (PL5). While `retired` is false they are
    ordinary entries of `secrets`; once it is true, each one still set live is drift.
    Returns the lines it adds."""
    declaration = want.pop("personal_tokens", None)
    if declaration is None:
        return []
    names, retired = declaration.get("names"), declaration.get("retired")
    if not (
        isinstance(names, list)
        and all(isinstance(n, str) for n in names)
        and isinstance(retired, bool)
    ):
        raise SettingsError(
            'actions.json personal_tokens must be {"names": [names], "retired": true|false}'
        )
    if not retired:
        return []
    want["secrets"] = [s for s in want["secrets"] if s not in names]
    still_set = [s for s in live["secrets"] if s in names]
    live["secrets"] = [s for s in live["secrets"] if s not in names]
    return [
        f"secrets: {json.dumps(n)} is a retired personal token (personal_tokens) and is still set"
        for n in sorted(still_set)
    ]


def drift(repo, directory):
    """The differences between the files and the live state, one line each, and the
    notes on what this token could compare only in part."""
    # Every file is read before the first call, so a bad file stops the run early.
    desired = {area: load(directory, area) for area in AREAS}
    lines, notes = [], []
    for area, (export, _) in AREAS.items():
        want, live = desired[area], export(repo)
        extra = _personal_tokens(want, live) if area == "actions" else []
        if area == "rulesets":
            hidden, partial = hidden_bypass(repo, want, live)
            lines += hidden
            notes += partial
        for line in list(compare("", want, live)) + extra:
            # A difference of the whole area has an empty path: "drift pages: ...".
            lines.append(
                f"drift {area}{line}" if line[0] == ":" else f"drift {area} {line}"
            )
    return lines, notes


def check_read_rights(repo):
    """diff only reads, and also runs with a read-only GitHub App token, whose answer
    to repos/{r} carries no permissions.admin. A token that cannot read the
    administration settings gets 404 from endpoints where a 404 reads as "off"; this
    one answers 404 to no token that can read them, so it proves the rights first."""
    try:
        get(f"repos/{repo}/actions/permissions")
    except NotFound as error:
        raise GhError(
            f"repos/{repo}/actions/permissions is not readable with this token: it needs "
            "read access to the administration settings of the repository"
        ) from error


def cmd_diff(repo, directory, _args):
    lines, notes = drift(repo, directory)
    for line in notes + lines:
        print(line)
    if lines:
        print(f"{len(lines)} drift(s): live state differs from the files")
        return 1
    print("no drift: live state matches the files")
    return 0


def cmd_apply(repo, directory, args):
    # Every file is read and the whole plan built before the first write, so an
    # unreadable file or a failing read stops the run with nothing changed.
    desired = {area: load(directory, area) for area in AREAS}
    calls, manual = [], []
    for area, (export, plan) in AREAS.items():
        c, m = plan(repo, export(repo), desired[area])
        calls += [x if isinstance(x, Call) else Call(*x) for x in c]
        manual += m
    for c in calls:
        print(
            ("DESTRUCTIVE " if c.destructive else "")
            + f"{c.method} {c.path}"
            + ("" if c.body is None else f" {json.dumps(c.body, sort_keys=True)}")
            + (f"  ({c.note})" if c.note else "")
        )
    if not calls:
        print("nothing to change through the API")
    for step in manual:
        print(f"MANUAL: {step}")
    if not calls:
        return 0
    if not args.yes:
        print("plan only: rerun with --yes to apply it")
        return 0
    destructive = sum(c.destructive for c in calls)
    if destructive and not args.allow_destructive:
        print(
            f"ghsettings: the plan holds {destructive} DESTRUCTIVE call(s); nothing was applied. "
            "Rerun with --yes --allow-destructive to apply them",
            file=sys.stderr,
        )
        return 2
    for c in calls:
        gh(c.path, c.method, c.body)
    print(f"applied {len(calls)} call(s); run diff to confirm")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="GitHub repository settings as code: export, diff and apply through `gh api`."
    )
    parser.add_argument(
        "--repo", help="OWNER/NAME (default: the repository of the current checkout)"
    )
    parser.add_argument(
        "--dir", type=pathlib.Path, default=DEFAULT_DIR, help="settings directory"
    )
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("export", help="write the live state to the files")
    sub.add_parser("diff", help="exit 1 when the live state differs from the files")
    apply = sub.add_parser(
        "apply", help="print the plan; with --yes, make the live state match the files"
    )
    apply.add_argument("--yes", action="store_true", help="run the plan")
    apply.add_argument(
        "--allow-destructive",
        action="store_true",
        help="with --yes, also run the DESTRUCTIVE calls (protection, ruleset, label or Pages deletion)",
    )
    args = parser.parse_args(argv)
    try:
        repo = (
            args.repo
            or subprocess.run(
                [
                    "gh",
                    "repo",
                    "view",
                    "--json",
                    "nameWithOwner",
                    "--jq",
                    ".nameWithOwner",
                ],
                capture_output=True,
                text=True,
                check=True,
            ).stdout.strip()
        )
        if args.command == "diff":
            check_read_rights(repo)
        else:
            check_rights(repo)
        return {"export": cmd_export, "diff": cmd_diff, "apply": cmd_apply}[
            args.command
        ](repo, args.dir, args)
    except FileNotFoundError as error:  # `gh repo view` when gh is missing
        print(
            f"ghsettings: {error.filename} is not on PATH; install the GitHub CLI",
            file=sys.stderr,
        )
        return 2
    except (GhError, NotFound, SettingsError, subprocess.CalledProcessError) as error:
        print(f"ghsettings: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
