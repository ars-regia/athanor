#!/usr/bin/env python3
"""
Controlli strutturali di Athanor OS.

Ogni controllo qui corrisponde a un difetto trovato in ANALISI_2026-09-02.md.
Non sono test di stile: sono le sei domande che nessuno stava ponendo, e che
insieme avrebbero intercettato ogni difetto critico e alto di quell'audit.

Uso:
    python3 scripts/verify.py              # tutti i controlli
    python3 scripts/verify.py polkit       # uno solo
    python3 scripts/verify.py --list
    python3 scripts/verify.py --known-red scripts/ci/known-red.txt [--known-red-base REF]
                                           # a listed red check does not fail the run (PQ12)
    python3 scripts/verify.py specs --known-red-base REF
                                           # REF is also the base `specs` measures Release raises
                                           # against (#1202)

Exit code: numero di controlli falliti (0 = tutto a posto).
Nessuna dipendenza oltre a python3 e git. Va eseguito dalla radice del repo.
"""

import ast
import collections
import datetime
import fnmatch
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

try:
    import yaml  # PyYAML, for the D43 lint, which fails closed without it
except ImportError:
    yaml = None

ROOT = Path(__file__).resolve().parent.parent
RS_DIRS = ["system", "forge/specs"]
# Published crates the repository carries with a patch of its own, by directory: somebody else's
# code, whose manifest declares upstream's licence exactly and whose panics the budget does not
# count. Each directory's PATCHES.md says what it changes and when it goes.
VENDORED_CRATES = {
    "system/athanor-sal/vendor/accesskit_atspi_common": "MIT OR Apache-2.0",
    "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/vendor/wayland-backend": "MIT",
}

# --------------------------------------------------------------------------- #
# infrastruttura minima
# --------------------------------------------------------------------------- #

CHECKS = {}
# The revision the change is measured against, from --known-red-base: the base of the pull
# request or of the merge group in pr.yml (`just check-static`), None when none is given.
BASE_REF = None
BOLD, DIM, RED, GRN, YEL, OFF = "\033[1m", "\033[2m", "\033[31m", "\033[32m", "\033[33m", "\033[0m"
if not sys.stdout.isatty() or os.environ.get("NO_COLOR"):
    BOLD = DIM = RED = GRN = YEL = OFF = ""


def check(name, title):
    def deco(fn):
        fn.title = title
        CHECKS[name] = fn
        return fn
    return deco


class Result:
    def __init__(self):
        self.problems = []
        self.notes = []

    def fail(self, msg):
        self.problems.append(msg)

    def note(self, msg):
        self.notes.append(msg)

    @property
    def ok(self):
        return not self.problems


PRUNE = {"target", ".git", "repo-cache", ".cache", "node_modules",
         "graph-vaults", "graph-pages", ".codegraph", "graphify-out"}
# `ROOT/experimental` is pruned too, at the root only (`walk`, `links`).


# Units that sealed LUKS to the TPM or bumped its rollback counter on their own (D42, issue #148).
AUTO_TPM_UNITS = ("athanor-tpm-luks-seal", "athanor-tpm-rollback-check", "athanor-tpm-rollback-update")


def entered(here, dirnames, base):
    """The directories of `dirnames`, under `here`, that a walk of `base` enters: not a PRUNE name
    nor ROOT/experimental. Under a crate's src/ a `target` or `.cache` is a module, and is entered."""
    inside = here.relative_to(ROOT if here.is_relative_to(ROOT) else base).parts
    return [d for d in dirnames if ("src" in inside or d not in PRUNE) and here / d != ROOT / "experimental"]


def walk(base, suffix):
    """os.walk with pruning: rglob enters target/ (tens of thousands of files) and takes
    minutes on a mounted filesystem. An `experimental/` below the root is code, and is walked."""
    base = Path(base)
    if not base.is_dir():
        return
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = entered(Path(dirpath), dirnames, base)
        for f in filenames:
            if f.endswith(suffix):
                yield Path(dirpath) / f


def vendored(path):
    """Whether `path`, relative to the repository, lies in a crate of VENDORED_CRATES."""
    return any(path.startswith(d + "/") for d in VENDORED_CRATES)


def rust_files():
    for d in RS_DIRS:
        yield from walk(ROOT / d, ".rs")


def read(p):
    return p.read_text(encoding="utf-8", errors="replace")


def rel(p):
    try:
        return str(Path(p).resolve().relative_to(ROOT))
    except ValueError:
        return str(p)


# --------------------------------------------------------------------------- #
# 1. workflow — il difetto che ha spento il CI il 2026-08-31
# --------------------------------------------------------------------------- #

@check("workflows", "I workflow GitHub sono validi e non esfiltrano log")
def check_workflows():
    r = Result()
    wf_dir = ROOT / ".github" / "workflows"
    if not wf_dir.is_dir():
        r.fail("nessuna directory .github/workflows")
        return r

    for wf in sorted(wf_dir.glob("*.yml")) + sorted(wf_dir.glob("*.yaml")):
        lines = read(wf).split("\n")

        # 1a. step con solo `name:` -> GitHub rifiuta l'INTERO file
        for i, line in enumerate(lines):
            m = re.match(r"^(\s+)- name:\s*(\S.*)$", line)
            if not m:
                continue
            indent = len(m.group(1))
            body = []
            for nxt in lines[i + 1:]:
                if not nxt.strip():
                    continue
                if len(nxt) - len(nxt.lstrip()) <= indent:
                    break
                body.append(nxt.strip())
            if not any(b.startswith(("run:", "uses:")) for b in body):
                r.fail(f"{wf.name}:{i+1} step senza run/uses -> GitHub rifiuta tutto il file "
                       f"({m.group(2)[:45]})")

        # 1b. blocchi if/fi vuoti -> errore di sintassi bash, uccide lo step
        for i, line in enumerate(lines):
            if re.search(r";\s*then\s*$", line):
                nxt = lines[i + 1].strip() if i + 1 < len(lines) else ""
                if nxt == "fi":
                    r.fail(f"{wf.name}:{i+1} blocco if vuoto -> errore di sintassi bash (exit 2)")

        # 1c. log spediti fuori dal perimetro
        for i, line in enumerate(lines):
            for host in ("webhook.site", "dpaste.com", "pastebin.com", "transfer.sh", "0x0.st"):
                if host in line:
                    r.fail(f"{wf.name}:{i+1} log inviati a un servizio esterno ({host})")

        # 1f. A workflow that runs on its own sets the token's permissions, or it gets the
        # repository default (CodeQL actions/missing-workflow-permissions, 2026-11-14). A
        # reusable workflow takes them from its caller.
        if yaml is None:
            r.fail(f"{wf.name}: PyYAML missing, the permissions of the workflow are not checked")
            continue
        try:
            doc = yaml.safe_load(read(wf))
        except yaml.YAMLError as e:
            r.fail(f"{wf.name}: not valid YAML ({e})")
            continue
        if not isinstance(doc, dict):
            r.fail(f"{wf.name}: not a YAML mapping")
            continue
        events = triggers(doc)
        if events != {"workflow_call"} and "permissions" not in doc:
            r.fail(f"{wf.name}: no top-level permissions, so the token gets the repository "
                   f"default (triggers: {', '.join(sorted(events)) or 'none'})")

    # 1e. D43: signing keys only in sign-only jobs
    for problem in (signing_problems(ROOT) + merge_queue_problems(ROOT) + release_problems(ROOT)
                    + policy_pull_problems(ROOT) + release_cache_problems(ROOT)
                    + scheduled_alert_problems(ROOT) + queue_skip_problems(ROOT)):
        r.fail(problem)

    # 1d. actionlint, se disponibile
    try:
        p = subprocess.run(["actionlint", "-no-color", "-oneline"], cwd=ROOT,
                           capture_output=True, text=True, timeout=120)
        if p.returncode != 0:
            for line in (p.stdout or p.stderr).strip().split("\n")[:15]:
                if line.strip():
                    r.fail(f"actionlint: {line.strip()}")
    except FileNotFoundError:
        r.note("actionlint non installato — installalo, intercetta molto più di questo controllo")
    except Exception as e:
        r.note(f"actionlint non eseguito: {e}")

    return r


# D43 lint. A regression guard, not a security boundary: it keeps a reviewed workflow from
# drifting into handing a signing key to a build or to a third-party action, and fails closed on
# a workflow it cannot read. What protects the keys is the environment protection on GitHub
# (required reviewer, deployment branches) and the review of every workflow change.

# The actions a sign-only job may use, pinned by a full commit SHA (D43).
SIGN_ONLY_ACTIONS = re.compile(
    r"^actions/(?:checkout|download-artifact|upload-artifact)@[0-9a-f]{40}$")
# The only inputs of those actions a sign-only job may set: no other ref or repository to check
# out, no other run's or repository's artifacts, no checkout outside the workspace root (D43).
SIGN_ONLY_INPUTS = {
    "actions/checkout": {"persist-credentials"},
    "actions/download-artifact": {"name", "pattern", "path", "merge-multiple"},
    "actions/upload-artifact": {"name", "path", "if-no-files-found", "retention-days"},
    "sigstore/cosign-installer": set(),
}
# The one third-party action a job with a key runs, pinned by a full commit SHA, with no input,
# and only in sign-images of release.yml, for `cosign attest` (D43 as amended by ADR-0127).
COSIGN_INSTALLER = re.compile(r"^sigstore/cosign-installer@[0-9a-f]{40}$")
COSIGN_INSTALLER_JOB = ("release.yml", "sign-images")
# The login cosign attest reads in that job alone: cosign reads the docker config that cosign
# login writes (go-containerregistry authn.DefaultKeychain), not the auth file of skopeo login.
COSIGN_LOGIN = 'echo "${GITHUB_TOKEN}" | cosign login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin'
# Where a sign-only job may download: a directory that is not in the checkout, so an artifact
# never replaces the sign script (a download without a path lands on the workspace root).
SIGN_JOB_DOWNLOADS = {"out", "artifacts"}
# The ref check of every job of release.yml (doc_pipeline.md PL54): a release run on any ref but
# iso-v0 fails, never skips. A signing job may run it: it reads no secret and changes nothing.
RELEASE_REF_CHECK = ('[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || '
                     '{ echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }')
# The only commands a step of a signing job may run, whole. A sign-only job receives built
# artefacts and runs no build step, and an earlier step must not be able to rewrite the sign
# script or the environment of the step that holds the key ($GITHUB_ENV, BASH_ENV) (D43).
SIGN_JOB_COMMANDS = {
    "bash forge/specs/azoth/signer/run.sh inputs",
    "bash forge/specs/azoth/signer/run.sh sign",
    'echo "${GITHUB_TOKEN}" | skopeo login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin',
    'bash system/sign-images.sh --registry "ghcr.io/${GITHUB_REPOSITORY_OWNER,,}"'
    ' --kernel-digests artifacts/kernel-unsigned.txt --attestations artifacts/attestations'
    ' artifacts/image-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"',
    RELEASE_REF_CHECK,
}
# The names a signing job, its steps and its workflow may set in env besides the secrets of the
# job's environment: plain values, none read by bash, the dynamic loader or a PATH lookup.
SIGN_JOB_ENV = {"GH_TOKEN", "GITHUB_TOKEN", "GITHUB_ACTOR", "KERNEL_DIGEST", "KERNEL_REGISTRY"}
# The image sign script runs beside the key and starts no container: system/verify-images.sh
# verifies its signatures in a job without the key (D43). The kernel signer is the exception
# by design: it runs a digest-pinned signer image it verifies first. Of cosign it runs only
# `cosign attest` (ADR-0127 point 5).
IMAGE_SIGN_SCRIPT = "system/sign-images.sh"
# Every word cosign on a line of it, quoted, in a substitution or an assignment alike, and
# the bare command cosign followed by attest, which alone it may be; and eval, which runs a
# command no line names.
EVAL = re.compile(r"(?:^|(?<=[\s;&|(]))eval\b")
COSIGN_WORD = re.compile(r"\bcosign\b")
COSIGN_ATTEST = re.compile(r"(?:^|(?<=[\s;&|(]))cosign\s+attest\b")
# A container engine as a command, not the docker:// transport of an image reference.
CONTAINER_ENGINE = re.compile(r"\b(?:podman|docker|buildah|nerdctl|crun|runc)\b(?!://)")
# The runners a signing job may use: GitHub-hosted, never a self-hosted machine (D43).
GITHUB_HOSTED = re.compile(r"^ubuntu-(?:latest|\d{2}\.\d{2})$")
# The only commands a step holding a signing secret may run, whole: the sign scripts (D43).
SIGN_SCRIPTS = re.compile(
    r"^bash (?:forge/specs/azoth/signer/run\.sh sign"
    r'|system/sign-images\.sh --registry "ghcr\.io/\$\{GITHUB_REPOSITORY_OWNER,,\}"'
    r' --kernel-digests artifacts/kernel-unsigned\.txt --attestations artifacts/attestations'
    r' artifacts/image-digests\.txt \| tee -a "\$\{GITHUB_STEP_SUMMARY\}")$')
EXPRESSION = re.compile(r"\$\{\{(.*?)\}\}", re.S)
# Contexts are case-insensitive: secrets.cosign_private_key reads COSIGN_PRIVATE_KEY.
SECRET_NAME = re.compile(r"\bsecrets\s*\.\s*([A-Za-z_][A-Za-z0-9_-]*)", re.I)
SECRETS_WORD = re.compile(r"\bsecrets\b", re.I)
ENVIRONMENTS_JSON = ".github/settings/environments.json"
BRANCH_PROTECTION_JSON = ".github/settings/branch-protection.json"
RULESETS_JSON = ".github/settings/rulesets.json"
# Its "secrets" are the repository secrets, which every job can read: none of them signs.
ACTIONS_JSON = ".github/settings/actions.json"
# The secret GitHub gives every run; nobody stores it.
BUILTIN_SECRETS = {"GITHUB_TOKEN"}


def yaml_strings(node, path=()):
    """Every string of a parsed YAML node, with the keys and indexes that lead to it."""
    if isinstance(node, dict):
        for key, value in node.items():
            yield from yaml_strings(value, path + (key,))
    elif isinstance(node, list):
        for index, value in enumerate(node):
            yield from yaml_strings(value, path + (index,))
    elif isinstance(node, str):
        yield path, node


def secret_references(text):
    """The secrets the expressions of text name as secrets.NAME, upper-cased, and whether any
    of them reads secrets another way (secrets['X'], secrets[format(...)], toJSON(secrets))."""
    names, dynamic = set(), False
    for expression in EXPRESSION.findall(text):
        named = SECRET_NAME.findall(expression)
        names |= {name.upper() for name in named}
        dynamic |= len(SECRETS_WORD.findall(expression)) > len(named)
    return names, dynamic


def job_environment(job):
    """The environment name a parsed job declares, as a string or as a mapping, or None."""
    environment = job.get("environment")
    if isinstance(environment, dict):
        environment = environment.get("name")
    return environment if isinstance(environment, str) else None


def triggers(doc):
    """The event names of a parsed workflow; PyYAML reads the key `on` as True."""
    on = doc.get("on", doc.get(True))
    if isinstance(on, str):
        return {on}
    return {str(event) for event in on} if isinstance(on, (list, dict)) else set()


def protected_branches(root):
    """The branches branch-protection.json protects against force pushes and deletion."""
    protection = json.loads(read(Path(root) / BRANCH_PROTECTION_JSON))
    return {branch for branch, rules in protection.items()
            if rules and rules.get("allow_force_pushes") is False and rules.get("allow_deletions") is False}


def deployment_problem(policy, protected):
    """Where a deployment branch policy lets a job of the environment run from, when that is
    more than named protected branches; None when it is only those."""
    if not policy:
        return "any branch"
    if not policy.get("custom_branch_policies"):
        return "any protected branch"
    branches = policy.get("policies") or []
    if not branches:
        return "no branch"
    for branch in branches:
        if branch.get("type", "branch") != "branch":
            return f"{branch.get('type')} {branch.get('name')}"
        if branch.get("name") not in protected:
            return branch.get("name")
    return None


def image_key_rotation(root):
    """Image key rotation (docs/operations/secrets.md section 4.1): it lasts while system/keys
    holds both image keys, and ends when athanor-image-1.pub leaves."""
    return all((Path(root) / "system/keys" / f"athanor-image-{n}.pub").is_file() for n in (1, 2))


def signing_environments(root):
    """{secret: environment} for the environments named signing* in environments.json, the one
    place that says which environment holds which key, and the problems of those environments:
    each needs a required reviewer, no administrator bypass, and deploys only from branches
    branch-protection.json protects by name (a glob or "any protected branch" would follow
    whoever can protect a new branch). During the image key rotation the old `signing`
    environment is an alias of signing-images: its rules apply, but it holds no key of its own,
    so the keys it still keeps until the rotation ends are not counted twice."""
    environments = json.loads(read(Path(root) / ENVIRONMENTS_JSON))
    alias = "signing" if image_key_rotation(root) else None
    signing = {name: env for name, env in environments.items() if name.startswith("signing")}
    if not signing:
        return {}, [f"{ENVIRONMENTS_JSON}: no signing environment (D43)"]
    holders, problems = {}, []
    protected = protected_branches(root)
    for name, env in sorted(signing.items()):
        if not env.get("reviewers"):
            problems.append(f"{ENVIRONMENTS_JSON}: the {name} environment has no required reviewer (D43)")
        if env.get("can_admins_bypass") is not False:
            problems.append(f"{ENVIRONMENTS_JSON}: the {name} environment lets administrators bypass "
                            "its reviewer (D43)")
        where = deployment_problem(env.get("deployment_branch_policy"), protected)
        if where:
            problems.append(f"{ENVIRONMENTS_JSON}: the {name} environment deploys from {where}, not only "
                            f"from branches {BRANCH_PROTECTION_JSON} protects (D43)")
        for secret in env.get("secrets", []) if name != alias else []:
            if secret in holders:
                problems.append(f"{ENVIRONMENTS_JSON}: {secret} is in {holders[secret]} and in {name}: "
                                "each key has one environment (D43)")
            holders.setdefault(secret, name)
    return holders, problems


def ruleset_refs(ruleset):
    """The ref patterns a ruleset includes, or None when it names none or `~ALL` (every ref)."""
    include = ((ruleset.get("conditions") or {}).get("ref_name") or {}).get("include")
    return None if not include or "~ALL" in include else include


def refs_overlap(a, b):
    """Whether two lists of ref patterns (None: every ref) can match the same ref. Excludes are
    ignored, which can only report more."""
    # Two globs that overlap without one matching the other (`a*`, `*b`) read as disjoint: the
    # rulesets name literal branches or a single glob, so this holds until two globs meet.
    return a is None or b is None or any(
        fnmatch.fnmatchcase(x, y) or fnmatch.fnmatchcase(y, x) for x in a for y in b)


def merge_queue_problems(root):
    """PIPE-N06: a ruleset with a merge queue needs every status context required on the
    branches it covers reported by a workflow that runs on merge_group; otherwise a merge group
    never gets the context and cannot be satisfied. A context required only on another branch
    (`Kernel gate` on `main` for a queue on `iso-v0`) does not count. A job reports its name
    (its id when it has none); a job that calls a reusable workflow reports
    `<name> / <inner context>`, and a matrix job without a name reports `<id> (<values>)`, so
    both are matched as GitHub composes them."""
    root = Path(root)
    rulesets = json.loads(read(root / RULESETS_JSON))
    queued = sorted(name for name, rs in rulesets.items()
                    if any(rule.get("type") == "merge_queue" for rule in rs.get("rules", [])))
    if not queued:
        return []
    sources = [([f"refs/heads/{branch}"], check["context"])
               for branch, protection in json.loads(read(root / BRANCH_PROTECTION_JSON)).items()
               if protection
               for check in (protection.get("required_status_checks") or {}).get("checks", [])]
    sources += [(ruleset_refs(rs), check["context"]) for rs in rulesets.values()
                for rule in rs.get("rules", []) if rule.get("type") == "required_status_checks"
                for check in rule["parameters"]["required_status_checks"]]
    reported = set()
    for wf in sorted((root / ".github/workflows").glob("*.y*ml")):
        doc = yaml.safe_load(read(wf))
        if isinstance(doc, dict) and "merge_group" in triggers(doc):
            reported |= job_contexts(root, doc)
    problems = []
    for name in queued:
        refs = ruleset_refs(rulesets[name])
        required = {context for where, context in sources if refs_overlap(refs, where)}
        problems += [f"{RULESETS_JSON}: ruleset {name} enables a merge queue, but the required context "
                     f"{context!r} is reported by no workflow that runs on merge_group (PIPE-N06)"
                     for context in sorted(required)
                     if not any(fnmatch.fnmatchcase(context, pattern) for pattern in reported)]
    return problems


ALERT_ACTION = "./.github/actions/alert"
def scheduled_alert_problems(root):
    """PL51: a workflow with an `on.schedule` trigger ends with alert jobs that use the alert
    action on a GitHub-hosted runner, run with `always()`, and between them need every other job,
    so a scheduled failure, a lost runner or a timeout opens a ci-alert issue."""
    problems = []
    workflows = Path(root) / ".github/workflows"
    for wf in sorted(workflows.glob("*.y*ml")):
        doc = yaml.safe_load(read(wf))
        if not isinstance(doc, dict) or "schedule" not in triggers(doc):
            continue
        jobs = doc.get("jobs") or {}
        problems += alert_job_problems(wf.name, jobs, "schedule")
    return problems


def alert_jobs(jobs):
    """The jobs of a workflow that use the alert action."""
    return {name: job for name, job in jobs.items()
            if any(step.get("uses") == ALERT_ACTION for step in (job or {}).get("steps") or [])}


def alert_job_problems(name, jobs, event):
    """What keeps the alert jobs of workflow NAME from reporting every job of a run of EVENT: none
    uses the action, one has an `if` other than `always() && !cancelled() && github.event_name ==
    '<event>'` (a superseded run is cancelled and is not a failure), one is not on a hosted
    runner, one sets continue-on-error or puts an `if` on its alert step, or a job is needed by none of them."""
    alerts = alert_jobs(jobs)
    if not alerts:
        return [f"{name}: no job uses {ALERT_ACTION}, which a run on {event} needs (PL51)"]
    problems = []
    covered = set(alerts)
    for job_id, job in alerts.items():
        condition = re.sub(r"\s+", " ", re.sub(r"^\s*\$\{\{(.*)\}\}\s*$", r"\1", str(job.get("if", "")))).strip()
        allowed = f"always() && !cancelled() && github.event_name == '{event}'"
        if condition != allowed:
            problems.append(f"{name}: alert job {job_id} has the if {condition!r}, not {allowed!r} (PL51)")
        if not GITHUB_HOSTED.match(str(job.get("runs-on", ""))):
            problems.append(f"{name}: alert job {job_id} is not on a GitHub-hosted runner (PL51)")
        if "continue-on-error" in job:
            problems.append(f"{name}: alert job {job_id} sets continue-on-error, which hides its failure (PL51)")
        for step in job.get("steps") or []:
            if step.get("uses") != ALERT_ACTION:
                continue
            if "if" in step or "continue-on-error" in step:
                problems.append(f"{name}: the alert step of {job_id} sets if or continue-on-error, "
                                "so it can be skipped or fail unseen (PL51)")
            given = {key: re.sub(r"\s+", " ", str(value)).strip() for key, value in (step.get("with") or {}).items()}
            if given != {"needs": "${{ toJSON(needs) }}"} or "env" in step:
                problems.append(f"{name}: the alert step of {job_id} must pass exactly "
                                "`needs: ${{ toJSON(needs) }}` and no env (PL51)")
        needed = job.get("needs") or []
        covered |= {needed} if isinstance(needed, str) else set(needed)
    return problems + [f"{name}: no alert job needs {job_id} (PL51)" for job_id in jobs if job_id not in covered]


# ADR-0146: each build the merge queue skips (QUEUE_SKIPS of scripts/ci/changes.py) names the
# workflow whose push run on iso-v0 covers it and alerts on failure, or None with the reason.
QUEUE_COVERED_BY = {
    "kernel": (None, "built once at the pull request head and published from that build on the "
                     "push (ADR-0110); the Orchestrator has no push alert yet (#1353 follow-up)"),
    "shell": ("shell-surfaces.yml", "the push run on iso-v0 builds the merged base and alerts"),
}


def queue_skip_problems(root):
    """ADR-0146: a build the queue skips is covered after the merge by a workflow that runs on a
    push and ends with an alert job for that push, or has a stated reason why none does."""
    root = Path(root)
    spec = importlib.util.spec_from_file_location("changes", root / "scripts/ci/changes.py")
    changes = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(changes)
    problems = []
    for area in sorted(changes.QUEUE_SKIPS):
        covered = QUEUE_COVERED_BY.get(area)
        if covered is None:
            problems.append(f"changes.py: the queue skips {area}, but QUEUE_COVERED_BY says nothing about it")
            continue
        name, reason = covered
        if not reason:
            problems.append(f"QUEUE_COVERED_BY[{area!r}] has no reason")
        if name is None:
            continue
        path = root / ".github/workflows" / name
        doc = yaml.safe_load(read(path)) if path.is_file() else None
        if not isinstance(doc, dict):
            problems.append(f"QUEUE_COVERED_BY[{area!r}]: {name} does not exist")
            continue
        if "push" not in triggers(doc):
            problems.append(f"{name}: covers the skipped {area} build after the merge but does not run on push")
        problems += alert_job_problems(name, doc.get("jobs") or {}, "push")
    return problems


def job_contexts(root, doc, depth=0):
    """The status contexts a workflow's jobs report, as fnmatch patterns: an expression in a
    name and the values of an unnamed matrix match anything, and so does the inner job of a
    reusable workflow that is not in this repository or nested deeper than GitHub allows."""
    patterns = set()
    for job_id, job in (doc.get("jobs") or {}).items():
        job = job or {}
        name = re.sub(r"\$\{\{.*?\}\}", "*", str(job.get("name", job_id)))
        if "name" not in job and (job.get("strategy") or {}).get("matrix"):
            name += " (*)"
        uses = job.get("uses")
        if not uses:
            patterns.add(name)
        elif uses.startswith("./") and depth < 4 and (root / uses).is_file():
            inner = yaml.safe_load(read(root / uses)) or {}
            patterns |= {f"{name} / {context}" for context in job_contexts(root, inner, depth + 1)}
        else:
            patterns.add(f"{name} / *")
    return patterns


RELEASE_YML = ".github/workflows/release.yml"
ORCHESTRATOR_YML = ".github/workflows/athanor-forge-orchestrator.yml"
RELEASE_GROUP = "${{ github.workflow }}"
BUILD_GROUP = "${{ github.workflow }}-${{ github.ref }}"
ON_ISO_V0 = "github.ref == 'refs/heads/iso-v0'"
RELEASE_DISPATCH = 'gh workflow run release.yml --ref iso-v0 -f build_run_id="${GITHUB_RUN_ID}"'
# The whole dispatch step but its name: no continue-on-error, shell or other token may qualify it.
RELEASE_DISPATCH_STEP = {
    "if": ON_ISO_V0,
    "env": {"GH_TOKEN": "${{ github.token }}", "GH_REPO": "${{ github.repository }}"},
    "run": RELEASE_DISPATCH,
}
# ponytail: an input reached without the word, as in toJSON(github.event), is not recognised.
INPUT_IN_RUN = re.compile(r"\$\{\{(?:(?!\}\}).)*\binputs\b", re.S)
# system/tag-images.sh is the one script that moves a tag of the system images (PL35); a raw
# skopeo copy onto :latest is not recognised, nor a tag held in a variable, and ADR-0111 item 5
# accepts that the registry itself binds no workflow.
LATEST_WRITER = re.compile(r"tag-images\.sh\b[^\n]*--tag(?:=|\s+)[\"']?latest\b(?![-.\w])")
STABLE_WRITER = re.compile(r"promote(?:-iso)?\.sh\b|tag-images\.sh\b[^\n]*--tag(?:=|\s+)[\"']?stable\b(?![-.\w])")


def release_problems(root):
    """PL54 (doc_pipeline.md section 4.10): every job of release.yml runs RELEASE_REF_CHECK
    before any step but a checkout, in a step with no if or continue-on-error, so a run on
    another ref fails in each job; the build's
    request-release job needs every other job of the build, and its condition asks a success
    of system-image and a success or a skip of each other job. release.yml starts on
    workflow_dispatch alone and interpolates no input into run:; request-release dispatches it
    once, on iso-v0 alone, with the build's run id. PL57: each workflow keeps its one
    concurrency group without cancel-in-progress, and no job of release.yml declares its own.
    PL56, PL60: no job outside release.yml runs in signing or signing-images, only tag-latest
    of release.yml moves :latest of the system images, and only a job of the environment
    release runs system/promote.sh or writes :stable."""
    root = Path(root)
    if yaml is None:
        return []
    if not (root / RELEASE_YML).exists():
        return ["release.yml: missing (PL54)"]
    problems = []
    release = yaml.safe_load(read(root / RELEASE_YML))
    for job_id, job in release["jobs"].items():
        for step in job.get("steps", []):
            if step.get("run") == RELEASE_REF_CHECK:
                # Nothing may make the check optional: no if, no continue-on-error.
                if set(step) - {"name", "run"}:
                    problems.append(f"release.yml: job {job_id} qualifies RELEASE_REF_CHECK with "
                                    f"{', '.join(sorted(set(step) - {'name', 'run'}))}")
                break
            if not str(step.get("uses", "")).startswith("actions/checkout@"):
                problems.append(f"release.yml: job {job_id} runs a step before RELEASE_REF_CHECK")
                break
        else:
            problems.append(f"release.yml: job {job_id} never runs RELEASE_REF_CHECK")
    orchestrator = yaml.safe_load(read(root / ORCHESTRATOR_YML))
    jobs = orchestrator["jobs"]
    request = jobs.get("request-release")
    if request is None:
        return problems + [f"{Path(ORCHESTRATOR_YML).name}: no request-release job"]
    condition = request.get("if", "")
    if "!cancelled()" not in condition:
        problems.append(f"{Path(ORCHESTRATOR_YML).name}: request-release does not ask !cancelled()")
    # A job that needs request-release (the PL51 alert) ends the run and cannot be needed by it.
    after = {job_id for job_id, job in jobs.items() if "request-release" in (job.get("needs") or [])}
    for job_id in sorted(set(jobs) - {"request-release"} - after):
        if job_id not in request.get("needs", []):
            problems.append(f"{Path(ORCHESTRATOR_YML).name}: request-release does not need {job_id}")
        elif job_id == "system-image":
            if "needs.system-image.result == 'success'" not in condition:
                problems.append(f"{Path(ORCHESTRATOR_YML).name}: request-release does not ask a success of system-image")
        elif f"""contains(fromJSON('["success", "skipped"]'), needs.{job_id}.result)""" not in condition:
            problems.append(f"{Path(ORCHESTRATOR_YML).name}: request-release does not ask a success or a skip of {job_id}")
    dispatches = [{key: value for key, value in step.items() if key != "name"}
                  for step in request.get("steps", []) if "release.yml" in str(step.get("run", ""))]
    if dispatches != [RELEASE_DISPATCH_STEP]:
        problems.append(f"{Path(ORCHESTRATOR_YML).name}: request-release does not dispatch release.yml once, "
                        "on iso-v0 alone, with its run id (PL54)")
    if triggers(release) != {"workflow_dispatch"}:
        problems.append(f"release.yml: starts on {sorted(triggers(release))}, not on workflow_dispatch alone (PL54)")
    for name, doc, group in (("release.yml", release, RELEASE_GROUP),
                             (Path(ORCHESTRATOR_YML).name, orchestrator, BUILD_GROUP)):
        if doc.get("concurrency") != {"group": group, "cancel-in-progress": False}:
            problems.append(f"{name}: concurrency is not the group {group} without cancel-in-progress (PL57)")
    for job_id, job in release["jobs"].items():
        if "concurrency" in job:
            problems.append(f"release.yml: job {job_id} declares a concurrency group of its own (PL57)")
        if any(INPUT_IN_RUN.search(str(step.get("run", ""))) for step in job.get("steps", [])):
            problems.append(f"release.yml: job {job_id} interpolates an input into run:, which only env may carry (PL54)")
    for wf in sorted((root / ".github/workflows").glob("*.y*ml")):
        try:
            doc = yaml.safe_load(read(wf))
        except yaml.YAMLError:
            continue  # signing_problems reports a workflow it cannot read
        for job_id, job in (doc.get("jobs") or {}).items() if isinstance(doc, dict) else ():
            environment = (job_environment(job) or "").lower()
            if environment in ("signing", "signing-images") and wf.name != "release.yml":
                problems.append(f"{wf.name}: job {job_id} runs in {environment}, which only release.yml may use (PL56)")
            for step in job.get("steps", []):
                run = re.sub(r"\\\n\s*", " ", str(step.get("run", "")))
                if LATEST_WRITER.search(run) and (wf.name, job_id) != ("release.yml", "tag-latest"):
                    problems.append(f"{wf.name}: job {job_id} moves :latest of the system images, "
                                    "which only tag-latest of release.yml writes (PL35, PL56)")
                if STABLE_WRITER.search(run) and environment != "release":
                    problems.append(f"{wf.name}: job {job_id} writes :stable outside the environment release (PL60)")
    return problems


# PL10: the actions a release-path job may use, each checked to restore and save no cache,
# compared in lower case as GitHub resolves names and without the ref; a local composite action
# is also read step by step. Any other action fails the check until it is checked and listed.
RELEASE_PATH_ACTIONS = {
    "actions/attest-build-provenance", "actions/checkout", "actions/create-github-app-token",
    "actions/download-artifact", "actions/upload-artifact", "anchore/sbom-action/download-syft",
    "cachix/install-nix-action", "oras-project/setup-oras", "sigstore/cosign-installer",
    "ublue-os/remove-unwanted-software", "./.github/actions/kvm", "./.github/actions/alert",
}
# The settings that give a build a cache from outside the job: sccache on the Actions cache, and
# a BuildKit or buildah cache-from or cache-to of any type. A cache that lives only in the job's
# own disk or volume is not one.
CACHE_BACKEND = re.compile(r"\bSCCACHE_GHA_\w+|\bACTIONS_CACHE_URL\b|\btype=gha\b|\bcache-(?:from|to)\b", re.I)
LOCAL_WORKFLOW = "./.github/workflows/"
LOCAL_ACTION = "./.github/actions/"
# The token grants of a job that publishes: id-token for a keyless signature, attestation or
# provenance, packages for a push to GHCR with the job's token. A push to another registry with
# a credential of its own is not seen.
PUBLISHING_GRANTS = ("id-token", "packages")


def publishes(permissions):
    """Whether a permissions block grants the token a write that publishes (PUBLISHING_GRANTS)."""
    if permissions == "write-all":
        return True
    return isinstance(permissions, dict) and any(permissions.get(g) == "write" for g in PUBLISHING_GRANTS)


def release_path_workflows(root):
    """{workflow: (jobs, env)} for the release path of PL10: every workflow with a job in a
    signing environment (D43) or in release (PL60), or with a job whose token may sign keylessly,
    attest or push to the registry (id-token or packages write, from the job or, when the job
    sets none, from the workflow), and every reusable workflow of this repository such a
    workflow calls, at any depth. The whole workflow counts: its other jobs build what the
    publishing job signs or pushes. Workflows that are not YAML are left to signing_problems."""
    root = Path(root)
    workflows = {}
    for wf in sorted((root / ".github/workflows").glob("*.y*ml")):
        try:
            doc = yaml.safe_load(read(wf))
        except yaml.YAMLError:
            continue
        if isinstance(doc, dict) and isinstance(doc.get("jobs"), dict):
            jobs = {job_id: job for job_id, job in doc["jobs"].items() if isinstance(job, dict)}
            workflows[wf.name] = jobs, doc.get("env"), doc.get("permissions")
    def releasing(job, permissions):
        environment = (job_environment(job) or "").lower()
        return (environment.startswith("signing") or environment == "release"
                or publishes(job.get("permissions", permissions)))
    pending = [name for name, (jobs, _, permissions) in workflows.items()
               if any(releasing(job, permissions) for job in jobs.values())]
    path = {}
    while pending:
        name = pending.pop()
        if name in path or name not in workflows:
            continue
        path[name] = workflows[name][:2]
        pending += [uses[len(LOCAL_WORKFLOW):] for job in path[name][0].values()
                    if (uses := str(job.get("uses", ""))).startswith(LOCAL_WORKFLOW)]
    return path


def expanded_steps(root, steps, problems, depth=0):
    """The steps of a job, each followed by the steps of the local composite action it uses, at
    any depth GitHub allows. A manifest that is not YAML is a line of problems."""
    for step in steps or []:
        if not isinstance(step, dict):
            continue
        yield step
        uses = str(step.get("uses", ""))
        if uses.startswith(LOCAL_ACTION) and depth < 10:
            for manifest in ("action.yml", "action.yaml"):
                path = Path(root) / uses / manifest
                if not path.is_file():
                    continue
                try:
                    action = yaml.safe_load(read(path))
                except yaml.YAMLError as error:
                    problems.append(f"{uses}/{manifest}: not YAML the PL10 check can read: "
                                    f"{str(error).splitlines()[0]}")
                    continue
                runs = action.get("runs") if isinstance(action, dict) else None
                yield from expanded_steps(root, (runs or {}).get("steps") if isinstance(runs, dict) else None,
                                          problems, depth + 1)


def release_cache_problems(root):
    """PL10: no job of a release-path workflow (release_path_workflows) restores or saves a
    cache, through its own steps or those of a local composite action it uses: an action outside
    RELEASE_PATH_ACTIONS, sccache on the Actions cache, or a cache-from or cache-to. A cache is an
    input the provenance does not name (PL20), and any job of the default branch can write one."""
    if yaml is None:
        return []
    problems = []
    for name, (jobs, env) in sorted(release_path_workflows(root).items()):
        for job_id, job in jobs.items():
            called = str(job.get("uses", ""))
            if called and not (called.startswith(LOCAL_WORKFLOW) and "/" not in called[len(LOCAL_WORKFLOW):]):
                problems.append(f"{name}: job {job_id} calls {called}, which the check cannot follow: call "
                                f"a workflow of this repository as {LOCAL_WORKFLOW}<file> (PL10)")
            steps = list(expanded_steps(root, job.get("steps"), problems))
            for step in steps:
                action = str(step.get("uses", "")).split("@")[0]
                if action and action.lower() not in RELEASE_PATH_ACTIONS:
                    problems.append(f"{name}: job {job_id} uses {action}, which is not in RELEASE_PATH_ACTIONS: "
                                    "add it once it is checked to restore and save no cache (PL10)")
            if CACHE_BACKEND.search(json.dumps([env, job, steps], default=str)):
                problems.append(f"{name}: job {job_id} gives a build a cache from outside the job (sccache "
                                "on the Actions cache, cache-from or cache-to) on the release path, which "
                                "restores and saves no cache (PL10)")
    return problems


def signing_problems(root):
    """D43 (doc_ci.md; doc_kernel_profile.md, section 12 item 1): a signing secret is read only
    in the env of a step that runs nothing but a sign script, in a job of the environment that
    holds it; a job of a signing environment uses no action but checkout, download-artifact and
    upload-artifact pinned by SHA and pointed at this run and repository, calls no reusable
    workflow, runs no container and builds, installs or runs no locally built image; no secret
    is read other than by name, and no caller inherits secrets into a workflow with a signing
    job; no signing job is in a workflow that runs on workflow_call, where the secrets of its
    environment are empty unless the caller inherits them (actions/runner#4453); each signing
    environment has a required reviewer, no administrator bypass, and deploys only from
    protected branches. Every secret read is declared in the settings: held by an
    environment of environments.json, or a repository secret of actions.json that no signing
    environment holds."""
    root = Path(root)
    holders, problems = signing_environments(root)
    problems += image_sign_script_problems(root)
    if yaml is None:
        return problems + ["scripts/verify.py: PyYAML is missing, so the D43 lint cannot read the "
                           "workflows (pip install pyyaml)"]
    environments = json.loads(read(root / ENVIRONMENTS_JSON))
    signing = {name for name in environments if name.startswith("signing")}
    # GitHub compares environment names without regard to case.
    canonical = {name.lower(): name for name in environments}
    # Image key rotation (docs/operations/secrets.md section 4.1): while system/keys holds both
    # image keys, the old `signing` environment, the only holder of key 1, counts as
    # signing-images, so every rule of a signing job applies to a job that names it. The alias
    # ends when athanor-image-1.pub leaves system/keys; from then on a `signing` entry left in
    # environments.json holds the same keys as signing-images and signing-kernel, and fails.
    if image_key_rotation(root):
        canonical["signing"] = "signing-images"
    repository = set(json.loads(read(root / ACTIONS_JSON)).get("secrets") or [])
    for secret in sorted(repository & holders.keys()):
        problems.append(f"{ACTIONS_JSON}: {secret} is a repository secret, which every job reads, "
                        f"and {holders[secret]} holds it (D43)")
    declared = repository | BUILTIN_SECRETS | {
        secret for env in environments.values() for secret in env.get("secrets") or []}
    workflows = {}
    for wf in sorted((root / ".github/workflows").glob("*.y*ml")):
        try:
            doc = yaml.safe_load(read(wf))
        except yaml.YAMLError as error:
            problems.append(f"{wf.name}: not YAML the D43 lint can read: {str(error).splitlines()[0]}")
            continue
        jobs = doc.get("jobs") if isinstance(doc, dict) else None
        if not isinstance(jobs, dict) or not jobs or not all(isinstance(job, dict) for job in jobs.values()):
            problems.append(f"{wf.name}: no mapping of jobs the D43 lint can read")
            continue
        workflows[wf.name] = doc
    def environment_of(job):
        environment = job_environment(job)
        return canonical.get(environment.lower(), environment) if environment else None

    with_signing_job = {name for name, doc in workflows.items()
                        if any(environment_of(job) in signing for job in doc["jobs"].values())}
    for name, doc in workflows.items():
        if "pull_request_target" in triggers(doc):
            problems.append(f"{name}: runs on pull_request_target, which runs with the secrets of this "
                            "repository on behalf of a fork (D43)")
        if name in with_signing_job:
            if "workflow_call" in triggers(doc):
                for job_id, job in doc["jobs"].items():
                    if environment_of(job) in signing:
                        problems.append(
                            f"{name}: signing job {job_id} is in a workflow that runs on workflow_call, "
                            "where its environment's secrets are empty unless the caller inherits every "
                            "secret (actions/runner#4453): sign in a job of the workflow the event "
                            "starts (D43)")
            env = doc.get("env") or {}
            for key in sorted(set(env) - SIGN_JOB_ENV) if isinstance(env, dict) else ["env"]:
                problems.append(f"{name}: sets {key} in env, beside a signing job: only {sorted(SIGN_JOB_ENV)} (D43)")
            if "defaults" in doc:
                problems.append(f"{name}: sets defaults, beside a signing job, which changes what its steps "
                                "run (D43)")
        problems += workflow_signing_problems(name, doc["jobs"], holders, signing, with_signing_job,
                                              set(workflows), environment_of)
        undeclared = set()
        for path, text in yaml_strings(doc):
            names, dynamic = secret_references(text)
            undeclared |= names - declared
            if dynamic:
                problems.append(f"{name}: {'.'.join(map(str, path))} reads secrets other than as "
                                "secrets.NAME, which hides the secret it reads (D43)")
            if names & holders.keys() and path[:1] != ("jobs",):
                for secret in sorted(names & holders.keys()):
                    problems.append(f"{name}: {secret} at the workflow level ({path[0]}), where every job "
                                    "reads it (D43)")
        for secret in sorted(undeclared):
            problems.append(f"{name}: reads {secret}, which neither {ENVIRONMENTS_JSON} nor {ACTIONS_JSON} "
                            "declares (D43)")
    return problems


# Either order: --policy is a global flag of skopeo and may come before the subcommand.
POLICY_PULL = re.compile(r"(?=.*\bcopy\b).*--policy\b")
# verify-images.sh runs the pull in the builder image; the dev VM runs Fedora's skopeo.
POLICY_PULLERS = {"system/verify-images.sh", "scripts/devvm/acceptance/run.sh"}


def policy_pull_problems(root):
    """A pull through the shipped policy (skopeo copy --policy) happens in system/verify-images.sh
    alone, which runs skopeo in the builder image: the runner's skopeo 1.13 rejects the policy's
    keyPaths (promote-stable run 38180393678). Tests are not scanned."""
    root = Path(root)
    problems = []
    # ponytail: shell and workflow files only; extend if a pull lands in Python or a Justfile.
    files = [*walk(root, ".sh"), *walk(root / ".github/workflows", ".yml"), *walk(root / ".github/workflows", ".yaml")]
    for path in sorted(files):
        name = path.relative_to(root).as_posix()
        if name in POLICY_PULLERS or "tests" in path.relative_to(root).parts:
            continue
        code = re.sub(r"\\\n\s*", " ", read(path))
        if any(POLICY_PULL.search(line) for line in code.splitlines() if not line.lstrip().startswith("#")):
            problems.append(f"{name}: pulls through a policy outside system/verify-images.sh, which runs it "
                            "in the builder image (runner skopeo 1.13)")
    return problems


def image_sign_script_problems(root):
    """The image sign script starts no container and runs no cosign subcommand but attest: a
    line outside a comment that names a container engine as a command, or names cosign anywhere
    but as the bare command `cosign attest` (quoted, through a variable, a substitution or a path
    alike), or runs eval, is a problem (D43, ADR-0127). The lint catches mistakes, not a
    deliberate evasion: a name built from pieces passes it, and the review of sign-images.sh,
    a code-owned path, is the boundary. The scripts it runs are not read here: of those,
    scripts/ci/release_attestation.py runs cosign only in verify, which verify-images.sh calls in
    a job without the key, where the pinned installer puts cosign beside no secret."""
    path = Path(root) / IMAGE_SIGN_SCRIPT
    if not path.is_file():
        return []
    problems = []
    for number, line in enumerate(read(path).splitlines(), 1):
        code = line.strip()
        if code.startswith("#"):
            continue
        if CONTAINER_ENGINE.search(code):
            problems.append(f"{IMAGE_SIGN_SCRIPT}:{number} starts a container beside the key: verify the "
                            "signatures in a job without it, system/verify-images.sh (D43)")
        if len(COSIGN_WORD.findall(code)) != len(COSIGN_ATTEST.findall(code)):
            problems.append(f"{IMAGE_SIGN_SCRIPT}:{number} runs cosign other than cosign attest beside the "
                            "key (D43, ADR-0127)")
        if EVAL.search(code):
            problems.append(f"{IMAGE_SIGN_SCRIPT}:{number} runs eval beside the key (D43, ADR-0127)")
    return problems


def workflow_signing_problems(name, jobs, holders, signing, with_signing_job, readable, environment_of):
    """The D43 problems of the jobs of one workflow (see signing_problems)."""
    problems = []
    for job_id, job in jobs.items():
        environment = environment_of(job)
        if environment and "${{" in environment:
            problems.append(f"{name}: job {job_id} names its environment with an expression the D43 lint "
                            "cannot resolve")
        steps = job.get("steps") if isinstance(job.get("steps"), list) else []
        for path, text in yaml_strings(job):
            for secret in sorted(secret_references(text)[0] & holders.keys()):
                if environment != holders[secret]:
                    problems.append(f"{name}: job {job_id} reads {secret} without environment: "
                                    f"{holders[secret]}, which holds it (D43)")
                elif path[:1] != ("steps",) or path[2:3] != ("env",) or len(path) != 4:
                    problems.append(f"{name}: job {job_id} reads {secret} in {'.'.join(map(str, path))}, "
                                    "not in the env of the step that signs (D43)")
                else:
                    step = steps[path[1]]
                    run = step.get("run")
                    if "uses" in step or not isinstance(run, str) or not SIGN_SCRIPTS.match(run.strip()):
                        problems.append(f"{name}: job {job_id} step {path[1] + 1} hands {secret} to a step "
                                        "that does not run only a sign script (D43)")
        if job.get("secrets") == "inherit":
            called = str(job.get("uses", ""))
            local = called.removeprefix("./.github/workflows/")
            if local == called or local in with_signing_job or local not in readable:
                problems.append(f"{name}: job {job_id} inherits every secret into {called}, which has a "
                                "signing job or cannot be read: pass the secrets it needs by name (D43)")
        if environment in signing:
            problems += signing_job_problems(f"{name}: signing job {job_id}", job, steps, environment, holders,
                                             (name, job_id) == COSIGN_INSTALLER_JOB)
    return problems


# The only defaults a signing job may set: GitHub's bash shell, `bash --noprofile --norc -eo
# pipefail {0}`, so a sign script piped into tee fails the step when it fails.
SIGN_JOB_DEFAULTS = {"run": {"shell": "bash"}}


def signing_job_problems(where, job, steps, environment, holders, attests=False):
    """A job of a signing environment: GitHub-hosted, no container, no defaults but the bash
    shell, only the pinned actions with their allowed inputs (with attests, the job of
    COSIGN_INSTALLER_JOB, also the cosign installer and COSIGN_LOGIN), only the allow-listed
    commands, no shell or working directory of its own, and in env only the secrets of its
    environment and plain values."""
    problems = []
    runs_on = job.get("runs-on")
    if "uses" not in job and not (isinstance(runs_on, str) and GITHUB_HOSTED.match(runs_on)):
        problems.append(f"{where} runs on {runs_on}: only a GitHub-hosted ubuntu label (D43)")
    for key in ("container", "services"):
        if key in job:
            problems.append(f"{where} runs a {key} beside the key (D43)")
    if "defaults" in job and job["defaults"] != SIGN_JOB_DEFAULTS:
        problems.append(f"{where} sets defaults other than {SIGN_JOB_DEFAULTS}, which changes what its steps "
                        "run (D43)")
    if "uses" in job:
        problems.append(f"{where} uses {job['uses']}: only checkout, download-artifact and upload-artifact "
                        "pinned by SHA (D43)")
    allowed_env = SIGN_JOB_ENV | {secret for secret, holder in holders.items() if holder == environment}
    envs = [("env", job.get("env"))]
    envs += [(f"steps.{index + 1}.env", step.get("env")) for index, step in enumerate(steps) if isinstance(step, dict)]
    for path, env in envs:
        for key in sorted(set(env) - allowed_env) if isinstance(env, dict) else ([] if env is None else ["env"]):
            problems.append(f"{where} sets {key} in {path}: only the secrets of its environment and "
                            f"{sorted(SIGN_JOB_ENV)} (D43)")
    for index, step in enumerate(steps):
        if not isinstance(step, dict):
            problems.append(f"{where} step {index + 1} is not a mapping the D43 lint can read")
            continue
        for key in ("shell", "working-directory"):
            if key in step:
                problems.append(f"{where} step {index + 1} sets {key} (D43)")
        if "run" in step and str(step["run"]).strip() not in SIGN_JOB_COMMANDS | ({COSIGN_LOGIN} if attests else set()):
            command = str(step["run"]).strip().splitlines()[0] if str(step["run"]).strip() else ""
            problems.append(f"{where} step {index + 1} runs a command outside the allow-list of D43: {command}")
        if "uses" not in step:
            continue
        action = str(step["uses"])
        if not (SIGN_ONLY_ACTIONS.match(action) or (attests and COSIGN_INSTALLER.match(action))):
            problems.append(f"{where} uses {action}: only checkout, download-artifact and upload-artifact "
                            "pinned by SHA (D43)")
            continue
        action = action.split("@")[0]
        inputs = step.get("with") if isinstance(step.get("with"), dict) else {}
        for key in sorted(set(inputs) - SIGN_ONLY_INPUTS[action]):
            problems.append(f"{where} sets {key} on {action}: it signs only this run's artifacts of this "
                            "repository (D43)")
        if action == "actions/download-artifact" and str(inputs.get("path", "")).rstrip("/") not in SIGN_JOB_DOWNLOADS:
            problems.append(f"{where} downloads into {inputs.get('path', 'the workspace root')}: only into one of "
                            f"{sorted(SIGN_JOB_DOWNLOADS)}, outside the checkout (D43)")
    return problems


# --------------------------------------------------------------------------- #
# installer — the machine check and the MOK request run in every kickstart that installs
# --------------------------------------------------------------------------- #

# The kickstarts that install Athanor: the manual one, the ISO's own, and the acceptance
# test's, which repeats the ISO's blocks because it includes osbuild-base.ks instead. The
# first is the reference the other two must carry word for word.
INSTALLER_KICKSTARTS = (
    "system/athanor-install.ks",
    "system/disk_config/iso.toml",
    "forge/test/iso/collaudo.ks",
)
SECUREBOOT_CERT = "/usr/share/athanor/secureboot/athanor-secureboot.der"
# The SHA-256 of the certificate the image ships, in sha256sum's format; the ISO
# acceptance compares the key enrolled in MokList with it.
SECUREBOOT_PIN = "system/secureboot-pin.sha256"
# What each block must say, so a copy that still matches but no longer checks is caught too.
INSTALLER_CPU_NEEDS = (
    "[ ! -d /sys/firmware/efi ]",
    "grep -q 'x86-64-v3 (supported'",
    "exit 1",
)
INSTALLER_MOK_NEEDS = (
    f"cert={SECUREBOOT_CERT}",
    "mokutil --timeout -1",
    'mokutil --import "$cert" --hash-file /dev/stdin',
    "mokutil --generate-hash |",
)


def kickstart_text(root, relative):
    """The kickstart a file carries: the file itself, or the installer customisation of a
    bootc-image-builder TOML, with TOML's escapes already applied."""
    text = read(root / relative)
    if relative.endswith(".toml"):
        return tomllib.loads(text)["customizations"]["installer"]["kickstart"][
            "contents"
        ]
    return text


def kickstart_sections(text):
    """Every %pre and %post section of a kickstart, from its header to its %end."""
    return re.findall(r"^%(?:pre|post)\b.*?^%end$", text, re.M | re.S)


def installer_problems(root=None):
    """doc_kernel_profile.md D3, D14 and section 4: every kickstart that installs refuses a
    machine without UEFI or x86-64-v3 in a %pre, and asks shim to enrol the certificate the
    image ships in a %post, under the hash of a password given to mokutil on standard input."""
    root = root or ROOT
    problems, sections = [], {}
    for relative in INSTALLER_KICKSTARTS:
        try:
            sections[relative] = kickstart_sections(kickstart_text(root, relative))
        except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as err:
            problems.append(f"{relative}: cannot read its kickstart ({err!r})")
    reference = INSTALLER_KICKSTARTS[0]
    blocks = {
        "machine check": (
            [s for s in sections.get(reference, []) if s.startswith("%pre")],
            "%pre --erroronfail",
            INSTALLER_CPU_NEEDS,
        ),
        "MOK enrolment request": (
            [s for s in sections.get(reference, []) if "mokutil --import" in s],
            "%post --erroronfail",
            INSTALLER_MOK_NEEDS,
        ),
    }
    for name, (found, header, needs) in blocks.items():
        if len(found) != 1:
            problems.append(
                f"{reference}: {len(found)} sections hold the {name}, one is expected"
            )
            continue
        block = found[0]
        if block.splitlines()[0] != header:
            problems.append(
                f"{reference}: the {name} must open with {header!r}, so that it stops the install"
            )
        problems.extend(
            f"{reference}: the {name} lacks {need!r}"
            for need in needs
            if need not in block
        )
        for relative in INSTALLER_KICKSTARTS[1:]:
            if relative in sections and block not in sections[relative]:
                problems.append(
                    f"{relative}: lacks the {name} of {reference}, or carries a copy that drifted"
                )
    if "--generate-hash=" in "".join(blocks["MOK enrolment request"][0]):
        problems.append(
            f"{reference}: the MOK password is passed on a command line; mokutil reads it on stdin"
        )
    containerfile = "system/Containerfile"
    try:
        shipped = re.search(
            rf"^COPY (\S+) {re.escape(SECUREBOOT_CERT)}$",
            read(root / containerfile),
            re.M,
        )
    except OSError as err:
        return problems + [f"{containerfile}: cannot read ({err})"]
    if not shipped:
        problems.append(
            f"{containerfile}: does not COPY the certificate the installer enrols to {SECUREBOOT_CERT}"
        )
        return problems
    rotation = "a key rotation updates the pin in the same pull request"
    try:
        pinned, pinned_path = read(root / SECUREBOOT_PIN).split()
    except (OSError, ValueError) as err:
        return problems + [f"{SECUREBOOT_PIN}: cannot read one 'sha256  path' line ({err})"]
    if pinned_path != shipped.group(1):
        problems.append(
            f"{SECUREBOOT_PIN}: pins {pinned_path}, but {containerfile} ships {shipped.group(1)}; {rotation}"
        )
        return problems
    try:
        actual = hashlib.sha256((root / pinned_path).read_bytes()).hexdigest()
    except OSError as err:
        return problems + [f"{pinned_path}: cannot read ({err})"]
    if actual != pinned:
        problems.append(
            f"{pinned_path}: SHA-256 {actual} differs from {pinned} in {SECUREBOOT_PIN}; {rotation}"
        )
    return problems


@check(
    "installer",
    "Every installing kickstart refuses machines without UEFI or x86-64-v3 and requests the MOK enrolment",
)
def check_installer():
    r = Result()
    for problem in installer_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 2. kickstart — una direttiva che Anaconda non riconosce ferma l'installazione
# --------------------------------------------------------------------------- #

# The syntax the Anaconda shipped on the ISO reads. The ISO is Fedora 45: raise it
# with the base and no further, because a directive introduced later would pass here
# and fail on the real media.
KICKSTART_SYNTAX = "F45"


# The kickstarts that live inside another file: bootc-image-builder's TOML carries the ISO's.
EMBEDDED_KICKSTARTS = ("system/disk_config/iso.toml",)


def kickstart_problems(root=None):
    """Every kickstart ksvalidator rejects for KICKSTART_SYNTAX: the .ks files of the tree and
    the ones EMBEDDED_KICKSTARTS carry. Raises FileNotFoundError when ksvalidator is missing."""
    root = Path(root or ROOT)
    problems = []
    with tempfile.TemporaryDirectory() as tmp:
        targets = [(str(ks.relative_to(root)), ks) for ks in sorted(walk(root, ".ks"))]
        for relative in EMBEDDED_KICKSTARTS:
            extracted = Path(tmp) / Path(relative).with_suffix(".ks").name
            try:
                extracted.write_text(kickstart_text(root, relative), encoding="utf-8")
            except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as err:
                problems.append(f"{relative}: cannot read its kickstart ({err!r})")
                continue
            # ksvalidator counts lines from the start of the kickstart, not of the TOML file.
            targets.append((f"{relative}, its kickstart contents", extracted))
        for relative, path in targets:
            # Without --followincludes an %include is accepted unread: collaudo.ks includes a
            # file that exists only on the installation medium (/run/install/repo).
            p = subprocess.run(["ksvalidator", "--version", KICKSTART_SYNTAX, str(path)],
                               capture_output=True, text=True, timeout=60)
            if p.returncode != 0:
                detail = " ".join(p.stderr.split()) or " ".join(p.stdout.split())
                problems.append(f"{relative}: {detail}")
    return problems


@check("kickstart", f"I kickstart sono sintassi valida per l'Anaconda di {KICKSTART_SYNTAX}")
def check_kickstart():
    r = Result()
    try:
        problems = kickstart_problems()
    except FileNotFoundError:
        # A workstation may lack the tool; CI installs it (scripts/ci/install-tools.sh,
        # call-lint.yml), so there its absence means nothing was checked.
        missing = ("ksvalidator is not installed (`dnf install pykickstart`): no kickstart is "
                   "checked before an install")
        (r.fail if os.environ.get("GITHUB_ACTIONS") == "true" else r.note)(missing)
        return r
    except (OSError, subprocess.SubprocessError) as e:
        r.fail(f"ksvalidator did not run: {e!r}")
        return r
    for problem in problems:
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# install-channel — installed systems follow :stable, signed (UD2, ADR-0114)
# --------------------------------------------------------------------------- #

# The kickstarts that install a machine for a person, and the one that tests that install.
# Each carries the same %post, which records <repository>:stable through ostree-image-signed.
INSTALL_KICKSTARTS = ("system/athanor-install.ks", "system/disk_config/iso.toml",
                      "forge/test/iso/collaudo.ks")
STABLE_SWITCH = ('bootc switch --mutate-in-place --enforce-container-sigpolicy '
                 '--transport registry "$repository:stable"')
KICKSTART_POST = re.compile(r"^%post\b.*?^%end$", re.M | re.S)


def install_channel_problems(root=None):
    """No kickstart names :latest or runs `bootc switch` without the signature policy, and each
    install kickstart carries the one %post that records :stable, signed (doc_update_delivery.md
    UD2). Comment lines are not read: they may name what the code must not."""
    root = Path(root or ROOT)
    problems = []
    blocks = {}
    for path in sorted({*(root / f for f in INSTALL_KICKSTARTS), *walk(root, ".ks")}):
        relative = path.relative_to(root).as_posix()
        try:
            text = read(path)
        except OSError as err:
            problems.append(f"{relative}: cannot read ({err})")
            continue
        for i, line in enumerate(text.split("\n"), 1):
            code = line.strip()
            if code.startswith("#"):
                continue
            if re.search(r":latest\b", code):
                problems.append(f"{relative}:{i}: names :latest; installed systems follow :stable (UD2)")
            if re.search(r"\bbootc\s+switch\b", code) and "--enforce-container-sigpolicy" not in code:
                problems.append(f"{relative}:{i}: bootc switch without --enforce-container-sigpolicy (UD2)")
        if relative in INSTALL_KICKSTARTS:
            # A line of its own, so a comment that quotes the command does not count.
            found = [b for b in KICKSTART_POST.findall(text)
                     if STABLE_SWITCH in (line.strip() for line in b.split("\n"))]
            if len(found) != 1:
                problems.append(f"{relative}: needs one %post that runs `{STABLE_SWITCH}` (UD2)")
            elif "--erroronfail" not in found[0].split("\n", 1)[0].split():
                problems.append(f"{relative}: the :stable %post needs --erroronfail, or a failed switch "
                                f"leaves an install that follows the unverified run tag (UD2)")
            else:
                blocks[relative] = found[0]
    if len(set(blocks.values())) > 1:
        problems.append(f"the :stable %post differs between {', '.join(sorted(blocks))}: keep them identical")
    return problems


@check("install-channel", "Install kickstarts leave the machine on :stable with the signature policy enforced")
def check_install_channel():
    r = Result()
    for problem in install_channel_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# os-release — bootc-image-builder reads it to build the ISO
# --------------------------------------------------------------------------- #

OS_RELEASE = "forge/specs/athanor-base-config/SOURCES/usr/lib/os-release"
FEDORA_RELEASE = "forge/specs/athanor-base-config/SOURCES/usr/lib/fedora-release"


def os_release_problems(root=None):
    """Every non-empty line of the shipped os-release is KEY=VALUE, and fedora-release names the
    release of its VERSION_ID.

    os-release(5) allows comment lines, but bootc-image-builder does not: it stops the ISO build
    with "readOSRelease: invalid input" on any non-empty line without "=". fedora-release was left
    at 43 when os-release moved to 45.
    """
    root = root or ROOT
    texts = []
    for path in (OS_RELEASE, FEDORA_RELEASE):
        try:
            texts.append(read(root / path))
        except OSError as err:
            return [f"{path}: cannot read ({err})"]
    text, release = texts
    out = [f"{OS_RELEASE}:{number}: not KEY=VALUE, bootc-image-builder rejects it: {line.strip()}"
           for number, line in enumerate(text.splitlines(), 1)
           if line.strip() and "=" not in line]
    version = next((line.split("=", 1)[1].strip().strip('"') for line in text.splitlines()
                    if line.startswith("VERSION_ID=")), None)
    named = re.search(r" release (\S+) ", release)
    if not named or named.group(1) != version:
        out.append(f"{FEDORA_RELEASE}: names release {named.group(1) if named else 'none'}, "
                   f"os-release VERSION_ID is {version}")
    return out


@check("os-release", "Every line of os-release is KEY=VALUE, as bootc-image-builder requires, and fedora-release names its VERSION_ID")
def check_os_release():
    r = Result()
    for problem in os_release_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# toolkit-boundary: the toolkit stays ready to leave the repository (ADR-0126)
# --------------------------------------------------------------------------- #

TOOLKIT = "system/athanor-sal"
# SPDX identifier -> the licence file that carries its text.
TOOLKIT_LICENCE_FILES = {"MIT": "LICENSE-MIT", "Apache-2.0": "LICENSE-APACHE"}
# The gallery is a client of the toolkit outside the workspace: its one dependency.
GALLERY_MANIFEST = f"{TOOLKIT}/gallery/Cargo.toml"
GALLERY_DEPENDENCY = {"athanor-sal": {"path": ".."}}
# The one patch the gallery may carry: the toolkit's own patch of AccessKit's AT-SPI bridge, the
# same as the repository root's, which a workspace root of its own does not inherit (#638;
# system/athanor-sal/vendor/accesskit_atspi_common/PATCHES.md). It goes when the patch does.
GALLERY_PATCH = {"crates-io": {"accesskit_atspi_common": {"path": "../vendor/accesskit_atspi_common"}}}


def dependency_tables(data, kinds):
    """Every table of `kinds` in a manifest, at the top and under [target.*], with the
    underscore spellings cargo still accepts (`build_dependencies`, `dev_dependencies`)."""
    kinds = [spelling for kind in kinds for spelling in dict.fromkeys((kind, kind.replace("-", "_")))]
    tables = [(kind, data.get(kind, {})) for kind in kinds]
    for target, spec in data.get("target", {}).items():
        tables += [(f"target.{target}.{kind}", spec.get(kind, {})) for kind in kinds]
    return tables


def gallery_problems(root):
    """The gallery depends on the toolkit by its path and on nothing else, as an application
    outside Athanor would (ADR-0126), and patches nothing but the toolkit's AccessKit patch."""
    if not (root / GALLERY_MANIFEST).is_file():
        return []
    try:
        data = tomllib.loads(read(root / GALLERY_MANIFEST))
    except (OSError, tomllib.TOMLDecodeError) as err:
        return [f"{GALLERY_MANIFEST}: cannot read ({err})"]
    tables = dependency_tables(data, ("dependencies", "build-dependencies", "dev-dependencies"))
    out = [f"{GALLERY_MANIFEST}: [{kind}] is not allowed: it would change what the gallery builds"
           for kind in ("patch", "replace") if kind in data and data[kind] != GALLERY_PATCH]
    out += [f"{GALLERY_MANIFEST}: [workspace] {key} is not allowed: the workspace is the gallery alone"
            for key in data.get("workspace", {})]
    for table, deps in tables:
        for name, dep in deps.items():
            if table != "dependencies" or GALLERY_DEPENDENCY.get(name) != dep:
                out.append(f"{GALLERY_MANIFEST}: [{table}] {name} is not allowed: "
                           'the gallery depends only on athanor-sal = { path = ".." }')
    return out


# The one module the toolkit's root exposes: the rest of its surface is re-exported by `pub use`
# (decision 3 of #649), so an application never names a module whose signatures carry accesskit,
# vello_cpu or parley types. Widening the list is a decision of the API, not of the author.
TOOLKIT_PUBLIC_MODULES = {"driver"}


def use_paths(tree, prefix=()):
    """The full paths of a `use` tree, without `as` aliases and a leading `crate::` or `self::`."""
    out, depth, start = [], 0, 0
    for i, ch in enumerate(tree + ","):
        depth += (ch == "{") - (ch == "}")
        if ch == "," and depth == 0:
            item, start = tree[start:i].strip(), i + 1
            item = re.sub(r"\s+as\s+\w+$", "", item)
            if not item:  # a trailing comma
                continue
            head, brace, rest = item.partition("{")
            segs = prefix + tuple(x.strip() for x in head.split("::") if x.strip())
            if brace:
                out += use_paths(rest[: rest.rindex("}")], segs)
            elif segs:
                out.append([x for x in segs if x])
    return [p[1:] if p[0] in ("crate", "self") and len(p) > 1 else p for p in out]


def api_problems(root):
    """The toolkit's root declares no `pub mod` outside TOOLKIT_PUBLIC_MODULES, and WidgetId is no
    alias of an integer: any id of that type would address another Ui's widgets (#649).

    `pub use` of a private module of the root (`text`, `crate::{text as t}`, `text::*`) fails; a
    glob of the root (`crate::*`) does not.

    Limit: it does not see a foreign crate's type in a public signature; the rustdoc run with
    `--all-features -- --document-private-items` (Justfile, rig.sh) catches the broken links, a reviewer the type."""
    lib = f"{TOOLKIT}/src/lib.rs"
    try:
        text = read(root / lib)
    except OSError as err:
        return [f"{lib}: cannot read ({err})"]
    # Only the root's own items count: comments are dropped and a `mod t { .. }` body is skipped.
    text = re.sub(r"//[^\n]*|/\*.*?\*/", "", text, flags=re.S)
    depth, top, stack = 0, [], []
    for i, ch in enumerate(text):
        if ch == "{":
            stack.append(text[:i].rstrip().endswith("::"))  # a `use a::{b, c}` group is no block
            depth += not stack[-1]
        elif ch == "}" and stack:
            depth -= not stack.pop()
        top.append(ch if depth == 0 else " ")
    top = "".join(top)
    out = [f"{lib}: `pub mod {name}` is outside the public modules {sorted(TOOLKIT_PUBLIC_MODULES)}: "
           "make it pub(crate) and re-export what an application needs"
           for name in re.findall(r"\bpub\s+mod\s+(\w+)", top) if name not in TOOLKIT_PUBLIC_MODULES]
    private = set(re.findall(r"\bmod\s+(\w+)", top)) - TOOLKIT_PUBLIC_MODULES
    for use in re.findall(r"\bpub\s+use\s+([^;]*);", top):
        for path in use_paths(use):
            # A path of one segment, or `m::*`, that names a private module re-exports it whole;
            # `crate::*` and an item of a module (`crate::widget::Kind`) are left alone.
            if path[-1:] == ["*"]:
                path = path[:-1]
            if len(path) == 1 and path[0] in private:
                out.append(f"{lib}: `pub use` of the module `{path[0]}` is outside the public modules "
                           f"{sorted(TOOLKIT_PUBLIC_MODULES)}: re-export its items")
    for path in sorted(walk(root / TOOLKIT / "src", ".rs")):
        if re.search(r"^\s*pub(?:\([^)]*\))?\s+type\s+WidgetId\b", read(path), re.M):
            out.append(f"{rel(path)}: WidgetId is a type alias: it is an opaque newtype (#649)")
    return out


def toolkit_boundary_problems(root=None):
    """The toolkit names no Athanor crate, no path and no workspace dependency in [dependencies]
    or [build-dependencies], and its `license` is the set of licence files beside the manifest
    (ADR-0126)."""
    root = Path(root or ROOT)
    manifest = f"{TOOLKIT}/Cargo.toml"
    try:
        data = tomllib.loads(read(root / manifest))
    except (OSError, tomllib.TOMLDecodeError) as err:
        return [f"{manifest}: cannot read ({err})"]
    tables = dependency_tables(data, ("dependencies", "build-dependencies"))
    out = []
    for table, deps in tables:
        for name, dep in deps.items():
            dep = dep if isinstance(dep, dict) else {}
            if dep.get("workspace"):
                out.append(f"{manifest}: [{table}] {name} is inherited from the workspace: "
                           "the split leaves none, so write its version here")
            package = dep.get("package", name)
            if re.match(r"athanor[-_]", package):
                out.append(f"{manifest}: [{table}] {name} is an Athanor crate: only dev-dependencies may be")
            if "path" in dep:
                out.append(f"{manifest}: [{table}] {name} is a path dependency: only dev-dependencies may be")
    licence = data.get("package", {}).get("license")
    ids = re.split(r"\s+(?:OR|AND)\s+", licence) if licence else []
    wanted = {TOOLKIT_LICENCE_FILES.get(i) for i in ids}
    if not ids or None in wanted:
        out.append(f"{manifest}: license = {licence!r} names an identifier with no licence file "
                   f"({', '.join(sorted(TOOLKIT_LICENCE_FILES))} are known)")
    present = {f for f in TOOLKIT_LICENCE_FILES.values() if (root / TOOLKIT / f).is_file()}
    for f in sorted(wanted - {None} - present):
        out.append(f"{TOOLKIT}/{f}: missing, license = {licence!r}")
    for f in sorted(present - wanted):
        out.append(f"{TOOLKIT}/{f}: present, but license = {licence!r} does not name it")
    return out + gallery_problems(root) + api_problems(root)


@check("toolkit-boundary", "The toolkit's build needs no Athanor crate, path or workspace dependency, its gallery stays apart, its licence files match its license, and its root exposes no module but driver")
def check_toolkit_boundary():
    r = Result()
    for problem in toolkit_boundary_problems():
        r.fail(problem)
    return r


CONTACTS = "forge/config/contacts.toml"
CONTACT_FIELDS = ("id", "purpose", "destination", "where_off")


def preset_lines(root):
    """(verb, unit, file) of every enable or disable line of the presets this repository ships."""
    files = sorted(root.glob("forge/specs/**/*.preset")) + sorted(root.glob("system/**/*.preset"))
    for f in files:
        for words in commands(read(f)):
            if words[0] in ("enable", "disable"):
                for unit in words[1:]:
                    yield words[0], unit, rel(f)


def contacts_problems(root=None):
    """The privacy promise (doc_first_run.md FR12, ADR-0056): what the image contacts by itself is
    listed in forge/config/contacts.toml and the presets agree with the list. Every timer a preset
    enables is a contact, a local timer or an inert one; a unit the list calls silent is disabled by
    a preset and enabled by nothing; a listed contact with a unit is still enabled; a contact that
    names a configuration file finds its text there. A listener is a unit the image enables that
    listens and that new installs turn off in the kickstart; it may be enabled.

    Limit: this reads the presets of this repository and the explicit enables of ENABLERS (the
    Containerfile and the kickstart) only. What Fedora's 90-default.preset enables, and what the
    packages enable in their own %post, are seen only by forge/scripts/check_image_contacts.py on
    the built image in CI (call-system-image.yml)."""
    root = root or ROOT
    try:
        data = tomllib.loads(read(root / CONTACTS))
    except (OSError, tomllib.TOMLDecodeError) as err:
        return [f"{CONTACTS}: cannot read ({err})"]
    problems = []
    contacts = data.get("contact", [])
    for c in contacts:
        problems += [f"{CONTACTS}: contact {c.get('id', '?')} has no {k}" for k in CONTACT_FIELDS if not c.get(k)]
    groups = {
        "contact": {c["unit"] for c in contacts if c.get("unit")},
        "local": {u["unit"] for u in data.get("local", [])},
        "inert": {u["unit"] for u in data.get("inert", [])},
        "silent": {u["unit"] for u in data.get("silent", [])},
        "listener": {u["unit"] for u in data.get("listener", [])},
    }
    names = list(groups)
    problems += [f"{CONTACTS}: {u} is listed as both {a} and {b}"
                 for i, a in enumerate(names) for b in names[i + 1:] for u in sorted(groups[a] & groups[b])]
    for group in ("local", "inert", "silent", "listener"):
        problems += [f"{CONTACTS}: a {group} entry has no reason" for u in data.get(group, []) if not u.get("reason")]

    def unit(name):
        return name if "." in name else name + ".service"

    lines = [(verb, unit(u), where) for verb, u, where in preset_lines(root)]
    enabled = {u for verb, u, _ in lines if verb == "enable"}
    disabled = {u for verb, u, _ in lines if verb == "disable"}
    for f in ENABLERS:
        if (root / f).exists():
            enabled |= {unit(u) for words in commands(read(root / f)) for u in enabled_units(words)}
    for c in contacts:
        if c.get("unit") and not c.get("planned") and c["unit"] not in enabled:
            problems += [f"{CONTACTS}: contact {c['id']} lists {c['unit']}, which no preset enables"]
        if c.get("config"):
            wanted = c.get("config_contains", "")
            try:
                text = read(root / c["config"])
                found = all(s in text for s in ([wanted] if isinstance(wanted, str) else wanted))
            except OSError:
                found = False
            if not found:
                problems += [f"{CONTACTS}: contact {c['id']} expects {wanted!r} in {c['config']}"]
    known = groups["contact"] | groups["local"] | groups["inert"]
    problems += [f"{where}: enables {u}, a timer {CONTACTS} does not list as a contact, local or inert"
                 for verb, u, where in lines if verb == "enable" and u.endswith(".timer") and u not in known]
    for u in sorted(groups["silent"]):
        if u in enabled:
            problems += [f"{CONTACTS}: {u} is silent, and something enables it"]
        if u not in disabled:
            problems += [f"{CONTACTS}: {u} is silent, and no preset disables it"]
    return problems


@check("contacts", "What the image contacts by itself is listed in contacts.toml, and the presets agree")
def check_contacts():
    r = Result()
    for problem in contacts_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 3. polkit — ogni azione applicata dal codice deve essere dichiarata
# --------------------------------------------------------------------------- #

# An action id in code, read whole: polkit allows "-" in an id, as in apply-multi-session.
POLKIT_ACTION_IN_CODE = re.compile(r"os\.athanor\.[a-z0-9_-]+\.[a-z0-9_-]+")


@check("polkit", "Ogni action-id polkit applicata dal codice è dichiarata in un .policy")
def check_polkit():
    r = Result()

    used = {}   # action -> {file:riga}
    for p in rust_files():
        for i, line in enumerate(read(p).split("\n"), 1):
            for a in POLKIT_ACTION_IN_CODE.findall(line):
                used.setdefault(a, set()).add(f"{rel(p)}:{i}")

    declared = {}   # action -> file .policy
    for pol in walk(ROOT, ".policy"):
        for a in re.findall(r'id="([^"]+)"', read(pol)):
            declared.setdefault(a, set()).add(rel(pol))

    # un .policy che non viene installato dallo spec non esiste, sul sistema reale
    installed = set()
    for spec in walk(ROOT, ".spec"):
        txt = read(spec)
        for a in re.findall(r"([A-Za-z0-9_.]+\.policy)", txt):
            if "polkit-1/actions" in txt:
                installed.add(a)

    for action in sorted(used):
        if action not in declared:
            where = sorted(used[action])[0]
            r.fail(f"{action}: applicata in {where} ma NON dichiarata in alcun .policy "
                   f"-> CheckAuthorization nega sempre")
        else:
            for f in declared[action]:
                if Path(f).name not in installed:
                    r.fail(f"{action}: dichiarata in {f} ma quel file non è installato "
                           f"in polkit-1/actions da nessuno spec")

    for action in sorted(declared):
        if action not in used:
            r.note(f"{action}: dichiarata in {', '.join(sorted(declared[action]))} "
                   f"ma nessun codice la controlla (policy morta)")

    return r


# --------------------------------------------------------------------------- #
# 4. percorsi runtime — niente artefatti letti da target/ o stato in /tmp
# --------------------------------------------------------------------------- #

def path_problems(relative_path, text):
    """I rilievi di percorso di un file, già formattati con riga e motivo."""
    # un build script gira a build time: può legittimamente parlare di target/
    is_build_script = Path(relative_path).name == "build.rs"
    problems = []
    for i, line in enumerate(text.split("\n"), 1):
        code = line.split("//")[0]
        if not is_build_script and re.search(r'"[^"]*\btarget/[a-z0-9_./-]*"', code):
            problems.append(f"{relative_path}:{i} carica un artefatto da target/ — percorso "
                            f"dell'albero di build, inesistente su un sistema installato")
        if re.search(r'"/tmp/', code):
            problems.append(f"{relative_path}:{i} percorso hard-coded in /tmp — usa "
                            f"/run/athanor (0700) per stato privilegiato")
    return problems


@check("attestation-absent", "athanor-attestation stays deleted (ADR-0103 D10)")
def check_attestation_absent():
    r = Result()
    if (ROOT / "system" / "confidential_computing").exists():
        r.fail("system/confidential_computing exists: athanor-attestation is deleted (ADR-0103 D10)")
    return r


def inherits_from_workspace(table):
    """True if a manifest table takes any key from a parent workspace (`x = { workspace = true }`
    or `x.workspace = true`, at any depth)."""
    return isinstance(table, dict) and (
        table.get("workspace") is True or any(inherits_from_workspace(v) for v in table.values()))


def stale_lock_problems(locks, manifests):
    """A Cargo.lock beside a crate that inherits from a parent workspace and has no [workspace]
    table of its own is never read: cargo builds the crate with the parent's lock, or, when the
    parent excludes it, cannot build it at all. A crate that inherits nothing is its own root and
    keeps its lock. `locks` are paths relative to the root; `manifests` maps a lock's Cargo.toml
    to its text, or to None when there is none, as beside a spec's source archive. A vendored
    crate keeps the lock it was published with."""
    out = []
    for lock in sorted(locks):
        if "/vendor/" in f"/{lock}":
            continue
        name = str(Path(lock).with_name("Cargo.toml"))
        text = manifests.get(name)
        if text is None:
            continue
        try:
            manifest = tomllib.loads(text)
        except tomllib.TOMLDecodeError as e:
            out.append(f"{name}: not valid TOML ({e})")
            continue
        if "workspace" not in manifest and inherits_from_workspace(manifest):
            out.append(f"{lock}: its crate inherits from a parent workspace, so cargo never reads "
                       f"this lock and its pins only raise advisories; delete it")
    return out


@check("cargo-locks", "No Cargo.lock beside a crate that inherits from a parent workspace, where cargo never reads it")
def check_cargo_locks():
    r = Result()
    listed = subprocess.run(["git", "ls-files", "-z", "--", "*Cargo.lock"], cwd=ROOT,
                            capture_output=True, text=True)
    if listed.returncode != 0:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    locks = [p for p in listed.stdout.split("\0") if p]
    manifests = {}
    for lock in locks:
        manifest = Path(lock).with_name("Cargo.toml")
        manifests[str(manifest)] = read(ROOT / manifest) if (ROOT / manifest).is_file() else None
    for problem in stale_lock_problems(locks, manifests):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# translations — a string that is not extracted stays in English (issue #1211)
# --------------------------------------------------------------------------- #

# A call xgettext extracts with the keywords of the update.sh scripts: tr or tr_with on a literal.
GETTEXT_CALL = re.compile(r'\btr(?:_with)?\(\s*r?#*"')
# An out-of-line module and the attributes before it.
MOD_DECL = re.compile(
    r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?\bmod\s+(\w+)\s*;"
)
# An inline module, whose out-of-line children resolve under its name.
INLINE_MOD = re.compile(
    r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?\bmod\s+(\w+)\s*\{"
)
RUST_CHAR = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'")
RUST_RAW = re.compile(r'b?r(#*)"')


def block_end(masked, brace):
    """The index just past the brace that closes the one at `brace`, in text whose strings,
    characters and comments are blanked."""
    depth, end = 0, brace
    while end < len(masked):
        depth += {"{": 1, "}": -1}.get(masked[end], 0)
        end += 1
        if depth == 0:
            break
    return end


def rust_code(text):
    """`text` as a build reads it, twice and of the same length: with its comments and its
    `#[cfg(test)]` items blanked, and the same with the contents of its strings blanked too,
    where braces and declarations can be searched."""
    code, masked = list(text), list(text)

    def blank(chars, start, end):
        for k in range(start, end):
            if chars[k] != "\n":
                chars[k] = " "

    i, n = 0, len(text)
    while i < n:
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            blank(code, i, end)
            blank(masked, i, end)
            i = end
        elif text.startswith("/*", i):
            # ponytail: a nested block comment ends at its first */; no source here nests one.
            end = text.find("*/", i + 2)
            end = n if end < 0 else end + 2
            blank(code, i, end)
            blank(masked, i, end)
            i = end
        elif text[i] == "'":
            char = RUST_CHAR.match(text, i)
            if char:
                # '{' and '}' would unbalance the braces of a test module.
                blank(masked, i + 1, char.end() - 1)
            i = char.end() if char else i + 1
        elif (raw := RUST_RAW.match(text, i)) and (
            i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")
        ):
            close = '"' + raw.group(1)
            end = text.find(close, raw.end())
            end = n if end < 0 else end
            blank(masked, raw.end(), end)
            i = end + len(close)
        elif text[i] == '"':
            k = i + 1
            while k < n and text[k] != '"':
                k += 2 if text[k] == "\\" else 1
            blank(masked, i + 1, min(k, n))
            i = k + 1
        else:
            i += 1
    flat = "".join(masked)
    for attr in re.finditer(r"#\[cfg\(test\)\]", flat):
        start = attr.end()
        brace, semi = flat.find("{", start), flat.find(";", start)
        if brace < 0 or 0 <= semi < brace:
            end = n if semi < 0 else semi + 1
        else:
            end = block_end(flat, brace)
        blank(code, attr.start(), end)
        blank(masked, attr.start(), end)
    return "".join(code), "".join(masked)


def cfg_value(predicate, features):
    """Whether `#[cfg(predicate)]` holds in a build with `features`, and not under test: True,
    False, or None for a predicate this cannot judge, such as `target_os`."""
    tokens = re.findall(r'"[^"]*"|\w+|[(),=]', predicate)

    # True, False, or None for what cannot be judged.
    def parse(i):
        name, i = tokens[i], i + 1
        if i < len(tokens) and tokens[i] == "(":
            args, i = [], i + 1
            while tokens[i] != ")":
                value, i = parse(i)
                args.append(value)
                if tokens[i] == ",":
                    i += 1
            if name == "not":
                value = None if args[0] is None else not args[0]
            elif name == "all":
                value = False if False in args else None if None in args else True
            elif name == "any":
                value = True if True in args else None if None in args else False
            else:
                value = None
            return value, i + 1
        if i < len(tokens) and tokens[i] == "=":
            value = tokens[i + 1].strip('"') in features if name == "feature" else None
            return value, i + 2
        return (False if name == "test" else None), i

    try:
        return parse(0)[0]
    except IndexError:
        return None


def cfg_holds(predicate, features):
    """Whether `#[cfg(predicate)]` holds; a predicate `cfg_value` cannot judge holds, and so
    does its `not`: its module is kept."""
    return cfg_value(predicate, features) is not False


def cfg_attr_paths(attrs, features):
    """The `path = "..."` of each `#[cfg_attr(predicate, path = "...")]` in `attrs`, with the
    predicate's `cfg_value`: [(True|False|None, path)]. Each attribute is read on its
    own, its parentheses balanced."""
    found = []
    for start in re.finditer(r"#\[cfg_attr\(", attrs):
        depth, quoted, parts, begin, i = 1, False, [], start.end(), start.end()
        while i < len(attrs) and depth:
            c = attrs[i]
            if c == '"':
                quoted = not quoted
            elif not quoted:
                depth += {"(": 1, ")": -1}.get(c, 0)
                if c == "," and depth == 1:
                    parts.append(attrs[begin:i])
                    begin = i + 1
            i += 1
        parts.append(attrs[begin : i - 1])
        for part in parts[1:]:
            path = re.fullmatch(r'\s*path\s*=\s*"([^"]*)"\s*', part)
            if path:
                found.append((cfg_value(parts[0], features), path.group(1)))
    return found


def module_files(root_file, features):
    """The source files of the module tree under `root_file` that a build with `features`
    compiles, test modules left out, and the modules declared there whose file is missing:
    (files, [(declaring file, expected file)])."""
    files, missing, todo = [], [], [(Path(root_file), True)]

    def holds(code, match):
        attrs = code[match.start(1) : match.end(1)]
        return all(
            cfg_holds(c, features) for c in re.findall(r"#\[cfg\((.*)\)\]", attrs)
        )

    while todo:
        path, owns_dir = todo.pop()
        if path in files:
            continue
        files.append(path)
        code, masked = rust_code(read(path))
        base = (
            path.parent
            if owns_dir or path.name == "mod.rs"
            else path.parent / path.stem
        )
        inline = [
            (m.end() - 1, block_end(masked, m.end() - 1), m.group(2), holds(code, m))
            for m in INLINE_MOD.finditer(masked)
        ]
        for decl in MOD_DECL.finditer(masked):
            outer = sorted(b for b in inline if b[0] < decl.start() < b[1])
            if not holds(code, decl) or not all(b[3] for b in outer):
                continue
            here = base.joinpath(*(b[2] for b in outer))
            attrs = code[decl.start(1) : decl.end(1)]
            explicit = re.search(r'#\[path\s*=\s*"([^"]*)"\]', attrs)
            # `#[cfg_attr(predicate, path = "...")]`: the path applies when the predicate
            # holds; when it cannot be judged, that file or any other the module may take
            # will do, and every one of them that exists is scanned, as a build for another
            # target compiles it.
            conditional = cfg_attr_paths(attrs, features)
            name = decl.group(2)
            at = here if outer else path.parent
            normal = [(here / f"{name}.rs", False), (here / name / "mod.rs", True)]
            holding = [(at / target, True) for value, target in conditional if value]
            undecided = [(at / t, True) for value, t in conditional if value is None]
            candidates = holding[-1:] or undecided + normal
            if explicit:
                candidates = [(at / explicit.group(1), True)]
            found = [c for c in candidates if c[0].is_file()]
            if not found:
                missing.append((path, candidates[0][0]))
            todo += found
    return files, missing


def crate_features(manifest, requested, defaults):
    """The features of a crate that `requested` turn on, with its default ones when `defaults`,
    and what they ask of its dependencies: (features, {dependency key: features},
    optional dependencies turned on)."""
    table = manifest.get("features", {})
    on = set(requested) | ({"default"} if defaults and "default" in table else set())
    asked, optional, todo = collections.defaultdict(set), set(), list(on)
    while todo:
        for entry in table.get(todo.pop(), []):
            if entry.startswith("dep:"):
                optional.add(entry[4:])
            elif "/" in entry:
                dep, feature = entry.split("/", 1)
                asked[dep.rstrip("?")].add(feature)
                if not dep.endswith("?"):
                    optional.add(dep)
            elif entry not in on:
                on.add(entry)
                todo.append(entry)
    optional |= on
    return on, asked, optional


def potfiles_problems(root):
    """Every source file a translated program compiles, its own or a path dependency's, that
    calls tr or tr_with on a literal is listed in the program's po/POTFILES.in, which its
    update.sh hands to xgettext: a file left out never reaches the catalog. A dependency counts
    with the features the program turns on in it, as the gtk and sal pages of a shared crate
    belong to different programs; the program itself counts with every feature, as each builds
    one of its binaries."""
    root = Path(root).resolve()
    problems = []
    for potfiles in sorted(walk(root / "forge/specs", "POTFILES.in")):
        package = potfiles.parent.parent
        listed = {
            (package / line.strip()).resolve()
            for line in read(potfiles).splitlines()
            if line.strip() and not line.startswith("#")
        }
        manifests, state, todo = {}, {}, [package]

        def load(crate):
            """The manifest of `crate`, or None after a problem says why it cannot be read."""
            if crate not in manifests:
                try:
                    manifests[crate] = tomllib.loads(read(crate / "Cargo.toml"))
                except (OSError, tomllib.TOMLDecodeError) as err:
                    problems.append(
                        f"{potfiles.relative_to(root)}: {(crate / 'Cargo.toml').relative_to(root)} "
                        f"cannot be read ({err})"
                    )
                    manifests[crate] = None
            return manifests[crate]

        def workspace(crate):
            """The directory and manifest of the workspace `crate` belongs to, or None."""
            for d in [crate, *crate.parents]:
                if d != root and root not in d.parents:
                    return None
                if (d / "Cargo.toml").is_file() and "workspace" in (load(d) or {}):
                    return d, manifests[d]
            return None

        if load(package) is None:
            continue
        state[package] = (set(manifests[package].get("features", {})), True)
        while todo:
            crate = todo.pop()
            manifest = manifests[crate]
            on, asked, optional = crate_features(manifest, *state[crate])
            if crate == package:
                optional |= {
                    k
                    for k, v in manifest.get("dependencies", {}).items()
                    if isinstance(v, dict)
                }
            tables = [manifest.get("dependencies", {})] + [
                t.get("dependencies", {}) for t in manifest.get("target", {}).values()
            ]
            for key, spec in ((k, v) for table in tables for k, v in table.items()):
                if not isinstance(spec, dict):
                    continue
                if spec.get("optional") and key not in optional:
                    continue
                base = crate
                if spec.get("workspace"):
                    ws = workspace(crate)
                    entry = ws and ws[1].get("workspace", {}).get("dependencies", {}).get(key)
                    if entry is None:
                        problems.append(
                            f"{potfiles.relative_to(root)}: {(crate / 'Cargo.toml').relative_to(root)} "
                            f"takes {key} from a workspace that does not declare it"
                        )
                        continue
                    if not isinstance(entry, dict):
                        continue
                    # The workspace sets the source and default-features; the member adds features.
                    base, spec = ws[0], {
                        **entry,
                        "features": entry.get("features", []) + spec.get("features", []),
                    }
                if "path" not in spec:
                    continue
                dep = (base / spec["path"]).resolve()
                if load(dep) is None:
                    continue
                features, defaults = state.get(dep, (set(), False))
                merged = (
                    features | set(spec.get("features", [])) | asked[key],
                    defaults or spec.get("default-features", True),
                )
                if dep not in state or merged != (features, defaults):
                    state[dep] = merged
                    todo.append(dep)
            state[crate] = (on, state[crate][1])
        for crate, (features, defaults) in sorted(state.items()):
            on = crate_features(manifests[crate], features, defaults)[0]
            manifest = manifests[crate]
            implicit, roots = [], []
            if "path" in manifest.get("lib", {}):
                roots.append(crate / manifest["lib"]["path"])
            else:
                implicit.append(crate / "src/lib.rs")
            if crate == package:
                implicit += [crate / "src/main.rs"]
                implicit += sorted((crate / "src/bin").glob("*.rs"))
                implicit += sorted((crate / "src/bin").glob("*/main.rs"))
                roots += [
                    crate / b["path"] for b in manifest.get("bin", []) if "path" in b
                ]
            for named in roots:
                if not named.is_file():
                    problems.append(
                        f"{potfiles.relative_to(root)}: {(crate / 'Cargo.toml').relative_to(root)} "
                        f"names {named.relative_to(root)}, which does not exist"
                    )
            roots = [r for r in roots + implicit if r.is_file()]
            sources = set()
            for found, missing in (module_files(r, on) for r in roots):
                sources |= set(found)
                for declaring, expected in missing:
                    problems.append(
                        f"{potfiles.relative_to(root)}: {declaring.relative_to(root)} declares a "
                        f"module whose file {expected.relative_to(root)} does not exist"
                    )
            for source in sorted(sources):
                if source.resolve() not in listed and GETTEXT_CALL.search(
                    rust_code(read(source))[0]
                ):
                    problems.append(
                        f"{potfiles.relative_to(root)}: {source.relative_to(root)} calls tr or tr_with "
                        f"and is compiled into the program, but is not listed: xgettext never extracts "
                        f"its strings, and they stay in English"
                    )
    return list(dict.fromkeys(problems))


@check(
    "potfiles",
    "Every source a translated program compiles that calls tr or tr_with is in its POTFILES.in",
)
def check_potfiles():
    r = Result()
    for problem in potfiles_problems(ROOT):
        r.fail(problem)
    return r


@check("paths", "Nessun artefatto runtime da target/, nessuno stato privilegiato in /tmp")
def check_paths():
    r = Result()
    for p in rust_files():
        for problem in path_problems(rel(p), read(p)):
            r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 5. packaging — un crate che compila e basta non è nel prodotto
# --------------------------------------------------------------------------- #

def has_binary_target(crate_dir):
    """True se il crate produce un eseguibile: src/main.rs, src/bin/ o una sezione [[bin]]."""
    cargo = crate_dir / "Cargo.toml"
    if not cargo.exists():
        return True  # non giudicabile: resta soggetto al controllo
    return ((crate_dir / "src" / "main.rs").exists()
            or (crate_dir / "src" / "bin").is_dir()
            or re.search(r"^\s*\[\[\s*bin\s*\]\]", read(cargo), re.M) is not None)


COSMIC_OVERLAY = "/usr/share/athanor/cosmic-defaults"


def cosmic_defaults_problems(root):
    """How Calmo reaches COSMIC (doc_shell.md, SH5): a data directory of our own, first
    in XDG_DATA_DIRS. cosmic-config resolves system defaults through that variable, so
    the defaults are shipped only if the files exist, the package installs them, and the
    variable is set both for the user manager and for the compositor."""
    root = Path(root)
    problems = []
    generated = root / "system/athanor-style/calmo/generated/cosmic/cosmic"
    if not generated.is_dir() or not any(p.is_file() for p in generated.rglob("*")):
        problems.append("no generated COSMIC defaults: run forge/tools/calmo-cosmic-theme/derive.sh")

    env_file = root / "forge/specs/athanor-calmo/SOURCES/usr/lib/environment.d/60-athanor-cosmic-defaults.conf"
    env_text = read(env_file) if env_file.exists() else ""
    if not re.search(rf"^XDG_DATA_DIRS={re.escape(COSMIC_OVERLAY)}:", env_text, re.M):
        problems.append(f"environment.d: {COSMIC_OVERLAY} is not first in XDG_DATA_DIRS for the user manager")

    session = root / "forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session"
    session_text = read(session) if session.exists() else ""
    if not re.search(rf'^export XDG_DATA_DIRS="?{re.escape(COSMIC_OVERLAY)}:', session_text, re.M):
        problems.append(f"athanor-session does not export XDG_DATA_DIRS with {COSMIC_OVERLAY} first: "
                        f"cosmic-comp and its children are not started by the user manager")

    spec = root / "forge/specs/athanor-calmo/athanor-calmo.spec"
    files = read(spec).split("%files", 1)[-1] if spec.exists() else ""
    for shipped in (COSMIC_OVERLAY, "/usr/lib/environment.d/60-athanor-cosmic-defaults.conf"):
        if not re.search(rf"^{re.escape(shipped)}$", files, re.M):
            problems.append(f"athanor-calmo.spec: %files does not list {shipped}")
    return problems


def crates_built_by_specs(root=None):
    """crate -> spec directory, for every `-p <crate>` a spec under forge/specs builds.
    A package may build more than one crate (one crate per program, doc_shell.md SH4)."""
    built = {}
    for spec in walk((root or ROOT) / "forge" / "specs", ".spec"):
        text = read(spec)
        name = re.search(r"^Name:\s*(\S+)", text, re.M)
        if not name:
            continue
        for line in text.split("\n"):
            if "cargo build" not in line:
                continue
            for crate in re.findall(r"-p\s+(\S+)", line):
                built[crate.replace("%{name}", name.group(1))] = spec.parent.name
    return built


UPDATE_SOURCES = "forge/specs/athanor-update/SOURCES"
UPDATE_SHIPPED = [
    "/usr/bin/athanor-update", "/usr/bin/athanor-update-notify", "/usr/libexec/athanor-update/render-policy",
    "/usr/share/athanor/containers/templates/policy.json.in",
    "/usr/share/athanor/containers/templates/attachments-policy.json.in",
    "/usr/share/athanor/containers/templates/athanor.yaml.in",
    "/usr/lib/systemd/system/athanor-update-check.timer", "/usr/lib/systemd/system/athanor-update-check.service",
    "/usr/lib/systemd/system/athanor-update.service", "/usr/lib/systemd/system/athanor-update-state.service",
    "/usr/lib/systemd/system/athanor-update-migrate.service", "/usr/lib/systemd/user/athanor-update-notify.service",
    "/usr/lib/systemd/system-preset/80-athanor-update.preset", "/usr/lib/systemd/user-preset/80-athanor-update.preset",
    "/usr/lib/tmpfiles.d/athanor-update.conf", "/usr/share/dbus-1/system.d/os.athanor.Update1.conf",
    "/usr/share/dbus-1/system-services/os.athanor.Update1.service", "/usr/share/polkit-1/actions/os.athanor.update.policy",
]
SYSTEM_IMAGES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]


def update_trust_problems(root=None):
    """The update and trust package ships what docs/architecture/doc_update_trust.md says
    (UT1, UT3, UT5, UT6, UT7), and the wiring it replaces is gone."""
    root = root or ROOT
    problems = []
    spec = root / "forge/specs/athanor-update/athanor-update.spec"
    if not spec.exists():
        return [f"{rel(spec)}: missing"]
    files = read(spec).split("%files", 1)[-1]
    for shipped in UPDATE_SHIPPED:
        if not re.search(rf"^{re.escape(shipped)}$", files, re.M):
            problems.append(f"athanor-update.spec: %files does not list {shipped}")
        source = root / UPDATE_SOURCES / shipped.lstrip("/")
        if "/usr/bin/" not in shipped and not source.exists():
            problems.append(f"{UPDATE_SOURCES}{shipped}: missing")

    templates = root / UPDATE_SOURCES / "usr/share/athanor/containers/templates"
    scopes = [f"@REGISTRY@/{name}" for name in SYSTEM_IMAGES]
    try:
        policy = json.loads(read(templates / "policy.json.in").replace("@KEY_PATHS@", '"/k.pub"'))
        attachments = json.loads(read(templates / "attachments-policy.json.in"))
        registries = read(templates / "athanor.yaml.in")
    except (OSError, ValueError) as err:
        return problems + [f"policy templates: {err}"]
    for name, doc in (("policy.json.in", policy), ("attachments-policy.json.in", attachments)):
        if doc.get("default") != [{"type": "reject"}]:
            problems.append(f"{name}: `default` must be reject (bootc refuses insecureAcceptAnything; "
                            f"the attachments policy must open our repositories only)")
    docker = policy.get("transports", {}).get("docker", {})
    if sorted(docker) != sorted([""] + scopes):
        problems.append(f"policy.json.in: docker scopes are {sorted(docker)}, expected the three system images and \"\"")
    for scope in scopes:
        for req in docker.get(scope, [{}]):
            if req.get("type") != "sigstoreSigned" or req.get("signedIdentity") != {"type": "matchRepository"} or not req.get("keyPaths"):
                problems.append(f"policy.json.in: {scope} must be sigstoreSigned with keyPaths and matchRepository")
    for transport in ("docker-archive", "oci", "oci-archive", "dir", "containers-storage", "docker-daemon"):
        if policy.get("transports", {}).get(transport) != {"": [{"type": "insecureAcceptAnything"}]}:
            problems.append(f"policy.json.in: transport {transport} must stay open, or podman users lose it")
    if sorted(attachments.get("transports", {}).get("docker", {})) != sorted(scopes):
        problems.append("attachments-policy.json.in: docker scopes must be exactly the three system images")
    declared = re.findall(r"^  (\S+):$", registries, re.M)
    if declared != scopes or registries.count("use-sigstore-attachments: true") != 3:
        problems.append("athanor.yaml.in: must declare use-sigstore-attachments for exactly the three system images "
                        "(a missing entry makes a signed image read as unsigned, a wider scope collides with default.yaml)")

    preset = root / "forge/specs/athanor-system-config/SOURCES/usr/lib/systemd/system-preset/80-athanor-system.preset"
    if preset.exists() and re.search(r"^enable bootc-fetch-apply\.timer$", read(preset), re.M):
        problems.append(f"{rel(preset)}: enables bootc-fetch-apply.timer, which does not exist")
    override = root / "forge/specs/athanor-base-config/SOURCES/usr/lib/systemd/system/bootc-fetch-apply-updates.service.d/override.conf"
    if override.exists():
        problems.append(f"{rel(override)}: calls `bootc upgrade --stage`, a flag bootc 1.16 does not have")
    for literal in walk(root / "forge/specs/athanor-update", ""):
        if literal.is_file() and "target" not in literal.parts and "vectors" not in literal.parts:
            for owner in literal_owners(read(literal)):
                problems.append(f"{rel(literal)}: literal registry owner {owner}; "
                                f"the registry comes from the build's variables")
    # D2: the Secure Boot daemon is retired. D42 (issue #148): nothing in the image seals LUKS to
    # the TPM or increments a rollback counter by itself, so none of these units may be shipped.
    for spec in walk(root / "forge/specs", ".spec"):
        # The changelog may name what was retired: only what the spec installs counts.
        text = read(spec).split("%changelog", 1)[0]
        if "athanor-secure-boot.service" in text:
            problems.append(f"{rel(spec)}: still ships athanor-secure-boot.service (retired by D2)")
        for unit in AUTO_TPM_UNITS:
            if unit in text:
                problems.append(f"{rel(spec)}: ships {unit}, which seals or counts without the user's action (D42)")
    for tree in ("forge", "system"):
        for found in walk(root / tree, ""):
            if found.is_file() and any(found.name.startswith(unit) for unit in AUTO_TPM_UNITS):
                problems.append(f"{rel(found)}: {found.name} is shipped; D42 forbids auto-sealing and the rollback-counter units")
    for name in ("system/Containerfile", "system/athanor-install.ks"):
        installer = root / name
        if installer.exists() and any(unit in read(installer) for unit in AUTO_TPM_UNITS):
            problems.append(f"{name}: names one of {', '.join(AUTO_TPM_UNITS)} (D42)")
    return problems


# The one RPM signature override of the image build: athanor-base-config from the digest-pinned
# tier 0 image, at rpm's digest level (UD44: the forge's RPMs are not signed).
TIER0_DIGEST_INSTALL = "rpm --define '_pkgverify_level digest' -Uvh --replacefiles --replacepkgs --nodeps \"$BASE_CONFIG\"; \\"
SIGNATURE_OVERRIDE = re.compile(r"_pkgverify_|--nosignature|--nodigest|--no-?gpgchecks?|gpgcheck=(0|no|off|false)", re.I)


def image_policy_problems(root=None):
    """system/Containerfile puts the policy in force and system/keys holds public keys only."""
    root = root or ROOT
    problems = []
    containerfile = read(root / "system/Containerfile")
    if not re.search(r"^ARG IMAGE_REGISTRY$", containerfile, re.M) or "render-policy" not in containerfile or "--link-etc /etc" not in containerfile:
        problems.append("system/Containerfile: does not run render-policy --link-etc /etc with ARG IMAGE_REGISTRY: "
                        "the policy is never in force and no machine verifies an image")
    elif containerfile.index("render-policy") > containerfile.index("systemctl preset-all"):
        problems.append("system/Containerfile: render-policy runs after preset-all")
    if "COPY system/keys/ /usr/share/athanor/keys/" not in containerfile:
        problems.append("system/Containerfile: does not copy system/keys to /usr/share/athanor/keys")
    keys = sorted((root / "system/keys").glob("*")) if (root / "system/keys").is_dir() else []
    if not any(key.suffix == ".pub" for key in keys):
        problems.append("system/keys: no *.pub: the rendered policy would name no key")
    for key in keys:
        if key.suffix != ".pub":
            problems.append(f"system/keys/{key.name}: only *.pub files belong under system/keys")
    for number, line in enumerate(containerfile.splitlines(), 1):
        if SIGNATURE_OVERRIDE.search(line) and (line.strip() != TIER0_DIGEST_INSTALL
                                                or "BASE_CONFIG=$(find /tmp/tier0-repo " not in containerfile):
            problems.append(f"system/Containerfile:{number}: overrides RPM signature checking; only "
                            "athanor-base-config from the tier 0 image installs at the digest level (UD44)")
    return problems


# Packages the image no longer ships (maintainer decision A2-10, issue #149). The check
# reads what the image would contain: the manifest, every install and enable in the
# Containerfile and the kickstart, the preset files, and the Requires of shipped specs.
# cosmic-store is not listed: it stays until Software ships, as the only graphical way to
# install an application (doc_software.md, decision 5). QEMU is not listed either: the
# self-hosted runner and the development VM run it from the image (ADR-0091), but the
# qemu-kvm metapackage stays out.
REMOVED_PACKAGES = {
    "antigravity", "astro-toolchain", "cargo-tools", "ide-bootstrap", "qa",
    "cosmic-term", "cosmic-files", "cosmic-edit", "cosmic-player",
    "foot", "swaybg", "swaylock", "Thunar", "thunar-archive-plugin", "thunar-volman",
    "virt-manager", "qemu-kvm", "compiler-rt",
}
REMOVED_PACKAGES |= {f"athanor-{n}" for n in ("antigravity", "astro-toolchain", "cargo-tools", "ide-bootstrap", "qa")}
# Units and authselect features the image must not enable: accounts stay classic. Keylime
# stays installed and disabled until a verifier exists for its agent to report to.
REMOVED_UNITS = {"systemd-homed", "systemd-homed.service", "systemd-homed-activate.service", "with-systemd-homed"}
KEYLIME_UNITS = {"keylime_agent", "keylime_agent.service", "keylime-agent", "keylime-agent.service"}
# Known debt, one entry per (spec, package): athanor-shell-rs runs foot from its legacy
# search and leaves with doc_portal.md (doc_software.md, decision 7). Delete the entry then.
REQUIRES_DEBT = {("athanor-shell-rs", "foot")}
INSTALLERS = {"dnf5", "dnf", "yum", "microdnf", "rpm-ostree"}
# Files that enable units: the image build and the installer; preset files are added by glob.
ENABLERS = ["system/Containerfile", "system/athanor-install.ks"]


def commands(text):
    """The commands of a script: continuations joined, comments dropped, split on && ; | and
    on the closing of a conditional, each as a list of words."""
    text = re.sub(r"\\\n", " ", text)
    for line in text.split("\n"):
        if re.match(r"\s*#", line):
            continue
        for part in re.split(r"&&|\|\||[;|]", line):
            words = part.split()
            if words:
                yield words


def enabled_units(words):
    """The units, features or packages-to-be a command enables, or an empty list."""
    if "systemctl" in words:
        rest = words[words.index("systemctl") + 1:]
        if "enable" in rest:
            return [w for w in rest[rest.index("enable") + 1:] if not w.startswith("-")]
    if "authselect" in words and "enable-feature" in words:
        return words[words.index("enable-feature") + 1:]
    if words[0] == "services" or words[0].startswith("--enabled"):
        return [u for w in words for flag in [w.partition("--enabled=")] if flag[1] for u in flag[2].split(",")]
    if words[0] == "enable":  # a preset line
        return words[1:]
    return []


def installed_packages(words):
    for i, w in enumerate(words):
        if w in INSTALLERS and "install" in words[i + 1:]:
            return [x for x in words[words.index("install", i) + 1:] if not x.startswith("-")]
    return []


def spec_requires(spec_text):
    for line in spec_text.split("\n"):
        m = re.match(r"Requires(?:\([^)]*\))?:\s*(.*)", line)
        if m:
            yield from re.findall(r"[A-Za-z0-9._+-]+", re.sub(r"[<>=]+\s*\S+", "", m.group(1)))


def removed_name_problems(root=None):
    """What the image would contain: no removed package, no removed or Keylime unit enabled."""
    root = root or ROOT
    problems = []
    manifest = root / "forge/config/packages.json"
    dag = set()
    if manifest.exists():
        def walk_lists(node, key):
            if isinstance(node, list):
                yield key, node
            elif isinstance(node, dict):
                for k, v in node.items():
                    yield from walk_lists(v, k)
        data = json.loads(read(manifest))
        dag = set(data.get("custom_packages", []))
        for key, names in walk_lists(data, ""):
            problems += [f"forge/config/packages.json: {key} lists the removed package {n}"
                         for n in names if n in REMOVED_PACKAGES]
    files = [root / n for n in ENABLERS]
    files += sorted(root.glob("forge/specs/**/*.preset")) + sorted(root.glob("system/**/*.preset"))
    for f in files:
        if not f.exists():
            continue
        for words in commands(read(f)):
            problems += [f"{rel(f)}: installs the removed package {n}" for n in installed_packages(words) if n in REMOVED_PACKAGES]
            problems += [f"{rel(f)}: enables {u}, which the image no longer ships" for u in enabled_units(words) if u in REMOVED_UNITS]
            problems += [f"{rel(f)}: enables the Keylime agent, and no verifier is configured for it to report to"
                         for u in enabled_units(words) if u in KEYLIME_UNITS]
    for spec in sorted(root.glob("forge/specs/*/*.spec")):
        name = spec.stem
        if name not in dag and name.removeprefix("athanor-") not in dag:
            continue
        problems += [f"{rel(spec)}: Requires the removed package {n}" for n in sorted(set(spec_requires(read(spec))))
                     if n in REMOVED_PACKAGES and (name, n) not in REQUIRES_DEBT]
    return problems


def keylime_problems(root=None):
    return [p for p in removed_name_problems(root) if "Keylime" in p]


@check("shipped", "Ogni crate del workspace è impacchettato, o è dichiarato sperimentale")
def check_shipped():
    r = Result()
    cargo = ROOT / "Cargo.toml"
    if not cargo.exists():
        r.fail("nessun Cargo.toml in radice")
        return r

    members = re.findall(r'^\s*"([^"]+)",?\s*$',
                         re.search(r"members\s*=\s*\[(.*?)\]", read(cargo), re.S).group(1),
                         re.M)

    specs_dir = ROOT / "forge" / "specs"
    spec_dirs = {d.name for d in specs_dir.iterdir() if d.is_dir()} if specs_dir.is_dir() else set()

    pkgs_file = ROOT / "forge" / "config" / "packages.json"
    dag = set()
    tiers = set()
    if pkgs_file.exists():
        d = json.loads(read(pkgs_file))
        dag = set(d.get("custom_packages", []))
        for k in d:
            if k.startswith("custom_tier"):
                tiers |= set(d[k])

    exempt_file = ROOT / "experimental" / "EXEMPT"
    exempt = set()
    if exempt_file.exists():
        exempt = {ln.strip() for ln in read(exempt_file).split("\n")
                  if ln.strip() and not ln.startswith("#")}

    built = crates_built_by_specs()
    for m in members:
        name = Path(m).name
        name = re.sub(r"-\d+\.\d+\.\d+$", "", name)
        if name in exempt:
            continue
        if not has_binary_target(ROOT / m):
            # Un crate libreria finisce dentro i binari che lo usano: niente da spedire.
            continue
        short = name.replace("athanor-", "")
        has_spec = name in spec_dirs or f"athanor-{short}" in spec_dirs
        in_dag = name in dag or short in dag
        owner = built.get(name)
        if owner and not has_spec:
            # Built and installed by another package's spec: shipped if that package is.
            has_spec, in_dag = True, owner in dag or owner.replace("athanor-", "") in dag
        if not (has_spec and in_dag):
            why = []
            if not has_spec:
                why.append("no .spec")
            if not in_dag:
                why.append("not in packages.json")
            r.fail(f"{name}: {', '.join(why)} -> it builds but never reaches the system. "
                   f"Package it, move it under experimental/, or list it in experimental/EXEMPT")

    for p in sorted(dag - tiers):
        r.fail(f"{p}: in custom_packages ma in nessun tier -> costruito e mai installato")
    for p in sorted(tiers - dag):
        r.fail(f"{p}: in un tier ma non in custom_packages -> riferimento pendente")

    for problem in cosmic_defaults_problems(ROOT):
        r.fail(problem)
    for problem in update_trust_problems():
        r.fail(problem)
    for problem in image_policy_problems():
        r.fail(problem)
    for problem in removed_name_problems():
        r.fail(problem)

    return r


# --------------------------------------------------------------------------- #
# 6. documentazione — i link devono risolvere e non essere assoluti
# --------------------------------------------------------------------------- #

# Documents removed on purpose (issue #149); neither the file nor a link to it may come back,
# for instance through a merge of an old branch.
FORBIDDEN_DOCS = ("doc_core_daemons.md", "doc_telemetry.md", "athanor-telemetry.md", "athanor_telemetry.md")


def forbidden_doc_problems(root=None):
    root = root or ROOT
    problems = [f"{rel(p)}: removed document, delete it"
                for p in walk(root, ".md") if p.name in FORBIDDEN_DOCS]
    for t in [rel(p) for p in walk(root / "docs", ".md")] + ["README.md", "system/README.md", "system/ARCHITECTURE.md"]:
        f = root / t
        if not f.exists():
            continue
        for m in re.finditer(r"\[[^\]]*\]\(([^)]+)\)|href=[\"']([^\"']+)[\"']", read(f)):
            target = m.group(1) or m.group(2)
            if Path(target.split("#")[0]).name in FORBIDDEN_DOCS:
                problems.append(f"{t}: links the removed document {target}")
    return problems


REQUIREMENT_HEAD = re.compile(r"^\s*(?:[-*]\s+)?\*\*([A-Z]{1,3}\d+[a-z]?)\.")
NEEDS_LINE = re.compile(r"^\s*(?:[-*]\s+|\d+\.\s+)?Needs:\s*(.*)$")
REQUIREMENT_ID = re.compile(r"[A-Z]{1,3}\d+[a-z]?")


def needs_graph(root=None):
    """The cross-specification dependency graph of docs/architecture/doc_session.md, SN11.

    A line `Needs: A1, B2.` belongs to the nearest requirement head (`**A1.` or `- **A1.`) above
    it in the same section: a Markdown heading ends a requirement, so a Needs line below one and
    before the next requirement is a problem, not silently attributed. Fenced code blocks are
    skipped. Returns (problems, graph), graph mapping each requirement to the set it needs."""
    root = root or ROOT
    files = sorted((root / "docs" / "architecture").glob("*.md"))
    defined, needs, problems = {}, {}, []
    for f in files:
        name = f.relative_to(root)
        current, fenced = None, False
        for number, line in enumerate(read(f).splitlines(), 1):
            if line.lstrip().startswith(("```", "~~~")):
                fenced = not fenced
                continue
            if fenced:
                continue
            if line.startswith("#"):
                current = None
                continue
            head = REQUIREMENT_HEAD.match(line)
            if head:
                current = head.group(1)
                defined.setdefault(current, []).append(f"{name}:{number}")
                continue
            m = NEEDS_LINE.match(line)
            if not m:
                continue
            where = f"{name}:{number}"
            if current is None:
                problems.append(f"{where}: Needs line outside a requirement")
                continue
            tokens = [t.strip() for t in m.group(1).rstrip().rstrip(".").split(",")]
            for token in tokens:
                if not REQUIREMENT_ID.fullmatch(token):
                    problems.append(f"{where}: {current} needs {token!r}, not a requirement identifier")
                elif token == current:
                    problems.append(f"{where}: {current} needs itself")
                else:
                    needs.setdefault(current, {})[token] = where
    graph = {}
    for source, targets in needs.items():
        if len(defined[source]) > 1:
            problems.append(f"{source} is defined more than once: {', '.join(defined[source])}")
        for target, where in targets.items():
            if target not in defined:
                problems.append(f"{where}: {source} needs {target}, which no specification defines")
            elif len(defined[target]) > 1:
                problems.append(f"{where}: {source} needs {target}, defined more than once: {', '.join(defined[target])}")
            graph.setdefault(source, set()).add(target)
    # Depth-first search with three colours; a grey node met again closes a cycle.
    colour, stack = {}, []

    def visit(node):
        colour[node] = "grey"
        stack.append(node)
        for nxt in sorted(graph.get(node, ())):
            if colour.get(nxt) == "grey":
                cycle = stack[stack.index(nxt):] + [nxt]
                problems.append("Needs cycle: " + " -> ".join(cycle))
            elif nxt not in colour:
                visit(nxt)
        stack.pop()
        colour[node] = "black"

    for node in sorted(graph):
        if node not in colour:
            visit(node)
    return problems, graph


REGISTER = "docs/architecture/shell-features.md"
REGISTER_STATUSES = ("have", "partial", "missing", "excluded (proposed)", "excluded")


def register_count_problems(text):
    """Where the Counts section of the shell feature register differs from its rows: each
    `## <surface>` table of `| F-... |` rows against the `### <surface>` table under Counts."""
    rows, stated = {}, {}
    surface, in_counts = None, False
    for line in text.split("\n"):
        if line.startswith("## "):
            surface, in_counts = line[3:].strip(), line.strip() == "## Counts"
            continue
        if in_counts and line.startswith("### "):
            surface = line[4:].strip()
            stated[surface] = {}
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if in_counts and surface in stated and len(cells) == 2 and cells[1].isdigit():
            stated[surface][cells[0]] = int(cells[1])
        elif not in_counts and surface and cells[0].startswith("F-") and len(cells) >= 5:
            rows.setdefault(surface, []).append((cells[0], cells[3]))

    problems = []
    for surface, entries in rows.items():
        for entry, status in entries:
            if status not in REGISTER_STATUSES:
                problems.append(f"{REGISTER}: {entry} has the unknown status '{status}'")
        if surface not in stated:
            problems.append(f"{REGISTER}: surface '{surface}' has no table under Counts")
            continue
        actual = {s: sum(1 for _, st in entries if st == s) for s in REGISTER_STATUSES}
        actual["total"] = len(entries)
        for status, n in actual.items():
            if stated[surface].get(status) != n:
                problems.append(f"{REGISTER}: Counts of '{surface}' say {status} "
                                f"{stated[surface].get(status)}, the rows give {n}")
    for surface in stated.keys() - rows.keys():
        problems.append(f"{REGISTER}: Counts name '{surface}', which has no rows")
    return problems


FENCE = re.compile(r"^\s*(`{3,}|~{3,})")
CODE_SPAN = re.compile(r"(`+)(?!`).*?(?<!`)\1(?!`)", re.S)
LINK = re.compile(r"\[([^\]]*)\]\(([^)]+)\)")


def prose_lines(text):
    """The lines of a Markdown text outside fenced blocks (``` or ~~~, closed only by a run of
    the same character at least as long), with an empty line where a block closed: a fence
    left open runs to the end of the file, as CommonMark has it."""
    prose, fence = [], None
    for line in text.splitlines():
        m = FENCE.match(line)
        if fence is None:
            if m:
                fence = m.group(1)
            else:
                prose.append(line)
        elif m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) \
                and not line.strip().lstrip(fence[0]):
            fence = None
            prose.append("")
    return prose


def markdown_links(text):
    """(label, target) of every inline link outside code. Fenced blocks and code spans are
    examples, not links."""
    # A code span never crosses a paragraph, so a stray backtick stays in its own.
    paragraphs = "\n".join(prose_lines(text)).split("\n\n")
    plain = "\n\n".join(CODE_SPAN.sub(" ", para) for para in paragraphs)
    return [(m.group(1), m.group(2)) for m in LINK.finditer(plain)]


PIPELINE_DOC = "docs/architecture/doc_pipeline.md"


def pipeline_workflow_problems(doc, workflows):
    """The first column of section 3.5 of doc_pipeline.md names every workflow file of
    `workflows` once and nothing else, and the count in its heading is the number of rows.
    A missing document, section or table fails; lines in code fences are neither headings
    nor rows."""
    if not doc.exists():
        return [f"{PIPELINE_DOC}: missing, section 3.5 cannot be checked against .github/workflows"]
    heading, rows = None, []
    for line in prose_lines(read(doc)):
        if heading is None:
            if line.startswith("### 3.5 "):
                heading = line
        elif re.match(r"#{1,3} ", line):
            break
        elif m := re.match(r"\| `([^`]+\.ya?ml)`", line):
            rows.append(m.group(1))
    if heading is None:
        return [f"{PIPELINE_DOC}: section 3.5 not found"]
    if not rows:
        return [f"{PIPELINE_DOC}: section 3.5 lists no workflow file"]
    files = {p.name for p in workflows.iterdir() if p.suffix in (".yml", ".yaml")}
    problems = [f"{PIPELINE_DOC}: {name} has no row in section 3.5"
                for name in sorted(files - set(rows))]
    problems += [f"{PIPELINE_DOC}: section 3.5 names {name}, which is not under .github/workflows/"
                 for name in sorted(set(rows) - files)]
    problems += [f"{PIPELINE_DOC}: section 3.5 lists {name} twice"
                 for name in sorted({name for name in rows if rows.count(name) > 1})]
    count = re.search(r"\d+", heading[len("### 3.5 "):])
    if not count:
        problems.append(f"{PIPELINE_DOC}: the heading of section 3.5 states no workflow count")
    elif int(count.group()) != len(rows):
        problems.append(f"{PIPELINE_DOC}: the heading of section 3.5 counts {count.group()} "
                        f"workflows, its table has {len(rows)} rows")
    return problems


@check("docs", "I link nella documentazione risolvono e sono portabili")
def check_docs():
    r = Result()
    for problem in forbidden_doc_problems():
        r.fail(problem)
    targets = ["README.md", "system/README.md", "system/ARCHITECTURE.md",
               "ANALISI_2026-09-02.md", "PIANO_RIPARTENZA.md", "CLAUDE.md", "ROADMAP.md",
               "AGENTS.md", "forge/AGENTS.md", "forge/specs/azoth/AGENTS.md", "system/AGENTS.md",
               ".github/workflows/AGENTS.md"]
    targets += [rel(p) for p in walk(ROOT / "docs", ".md")]

    for t in targets:
        f = ROOT / t
        if not f.exists():
            continue
        base = f.parent
        for label, target in markdown_links(read(f)):
            link = target.split("#")[0].strip()
            if not link or link.startswith(("http://", "https://", "mailto:")):
                continue
            if link.startswith("file://"):
                r.fail(f"{t}: link assoluto della macchina di sviluppo -> {link[:70]}")
                continue
            if not (base / link).exists():
                r.fail(f"{t}: link rotto [{label[:30]}] -> {link}")
    for problem in pipeline_workflow_problems(ROOT / PIPELINE_DOC, ROOT / ".github" / "workflows"):
        r.fail(problem)
    problems, graph = needs_graph()
    for problem in problems:
        r.fail(problem)
    edges = sum(len(targets) for targets in graph.values())
    r.note(f"Needs graph: {len(set(graph) | set().union(*graph.values()))} requirements, {edges} edges")
    register = ROOT / REGISTER
    if register.exists():
        for problem in register_count_problems(read(register)):
            r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 7. panic — il budget attuale è 1 unwrap in 60k righe. Difendilo.
# --------------------------------------------------------------------------- #

# Valori misurati sul repo il 2026-09-30, senza tests/, benches/ ed examples/. Sono un cricchetto: si abbassano,
# non si alzano. Se un controllo fallisce qui, propaga con `?`.
# Every macro below ends the process under `panic = "abort"`; `debug_assert!` is compiled out of release
# and is not counted. The two assertions left are in the fuzz entry of shelld (`shelld.rs`) and in
# `Ui::add` of the toolkit, documented under `# Panics` (measured 2026-11-04).
BUDGET = {
    ".unwrap()": 0,
    ".expect(": 0,
    "panic!(": 0,
    "assert!(": 1,
    "assert_eq!(": 1,
    "assert_ne!(": 0,
    "unreachable!(": 0,
    "todo!(": 0,
    "unimplemented!(": 0,
    "process::abort(": 2,  # the fuzz entry and the DRM lease handler of Sulphur, #1186
}


def count_panic(code, k):
    """The occurrences of `k` in `code`. A macro name does not match inside a longer one
    (`assert!(` is not `debug_assert!(`), and a macro counts with any delimiter and with
    space before it (`assert! {`, `assert![`)."""
    if k.endswith("!("):
        return len(re.findall(r"(?<![\w])" + re.escape(k[:-1]) + r"\s*[(\[{]", code))
    return code.count(k)


def is_test_file(p):
    """Integration tests, benchmarks and examples are not code that runs in a daemon:
    a panic there fails a test, it does not end a service."""
    parts = Path(rel(p)).parts
    return any(d in parts for d in ("tests", "benches", "examples")) or Path(p).name == "tests.rs"


TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")


def test_module_files(paths):
    """The files of the modules declared `#[cfg(test)] mod name;`: test helpers kept in a file
    of their own, compiled into the tests only, as a `#[cfg(test)]` block is."""
    found = set()
    for p in map(Path, paths):
        here = p.parent if p.name in ("lib.rs", "main.rs", "mod.rs") else p.parent / p.stem
        for name in TEST_MODULE.findall(read(p)):
            found.update({here / f"{name}.rs", here / name / "mod.rs"})
    return found


RAW_STRING = re.compile(r'[bc]?r(#*)"')
# What follows `#[cfg(test)]` when it applies to an item: more attributes, a visibility, then
# the item's keyword. Otherwise it applies to a field, variant, argument or arm.
TEST_ITEM = re.compile(
    r"\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:(?:fn|mod|impl|struct|enum|union|use|const|static|type|trait|extern|unsafe|async|let)\b"
    r"|macro_rules!)"
)


def skip_rust(txt, i):
    """The index past the string, character literal or comment that starts at `i`, or `i`."""
    if txt.startswith("//", i):
        end = txt.find("\n", i)
        return len(txt) if end < 0 else end
    if txt.startswith("/*", i):
        # Block comments nest.
        depth, j = 1, i + 2
        while depth and j < len(txt):
            if txt.startswith("/*", j):
                depth, j = depth + 1, j + 2
            elif txt.startswith("*/", j):
                depth, j = depth - 1, j + 2
            else:
                j += 1
        return j
    raw = RAW_STRING.match(txt, i)
    if raw and not (i and (txt[i - 1].isalnum() or txt[i - 1] == "_")):
        end = txt.find('"' + raw.group(1), raw.end())
        return len(txt) if end < 0 else end + 1 + len(raw.group(1))
    if txt[i] == '"':
        j = i + 1
        while j < len(txt) and txt[j] != '"':
            j += 2 if txt[j] == "\\" else 1
        return j + 1
    if txt[i] == "'":
        # A character literal is one character ('{') or one escape ('\n', '\'', '\u{7b}');
        # anything else is a lifetime ('a), which has no closing quote.
        if txt.startswith("'", i + 2) and txt[i + 1] != "\\":
            return i + 3
        end = txt.find("'", i + 3)
        if txt.startswith("\\", i + 1) and 0 < end - i <= 11:
            return end + 1
    return i


def item_end(txt, i):
    """The index past what `#[cfg(test)]` at `i` applies to: an item, from its attributes to
    its `;` or the brace that closes its body; a field, variant or argument, up to its `,` or
    the bracket that closes the list it is in. Strings, comments and brackets are skipped."""
    item = TEST_ITEM.match(txt, i) is not None
    depth, body = 0, False
    while i < len(txt):
        j = skip_rust(txt, i)
        if j != i:
            i = j
            continue
        c = txt[i]
        if c in "([{":
            depth += 1
            body = body or c == "{"
        elif c in ")]}":
            depth -= 1
            if depth < 0:
                return i
            if c == "}" and body and depth == 0:
                return i + 1
        elif depth == 0 and (c == ";" or c == "," and not item):
            return i + 1
        i += 1
    return len(txt)


def without_test_items(txt):
    """`txt` with each item under `#[cfg(test)]` blanked, newlines kept so that line numbers
    hold. Code after a test item is production code, and stays; the marker counts only as
    code, not inside a string or a comment."""
    out, at, i = [], 0, 0
    while i < len(txt):
        j = skip_rust(txt, i)
        if j != i:
            i = j
        elif txt.startswith("#[cfg(test)]", i):
            end = item_end(txt, i + len("#[cfg(test)]"))
            out.append(txt[at:i])
            out.append(re.sub(r"[^\n]", " ", txt[i:end]))
            at = i = end
        else:
            i += 1
    out.append(txt[at:])
    return "".join(out)


def without_literals(txt):
    """`txt` with each string, character literal and comment (line or block) blanked, newlines
    kept so that line numbers hold: a budget key in text that is not a call is not a panic.
    The body of a `macro_rules!` is code and stays: it panics where the macro is expanded."""
    out, at, i = [], 0, 0
    while i < len(txt):
        j = skip_rust(txt, i)
        if j == i:
            i += 1
            continue
        out.append(txt[at:i])
        out.append(re.sub(r"[^\n]", " ", txt[i:j]))
        at = i = j
    out.append(txt[at:])
    return "".join(out)


@check("panics", "Il budget di panic in codice non di test non cresce")
def check_panics():
    r = Result()
    counts = {k: 0 for k in BUDGET}
    where = {k: [] for k in BUDGET}

    files = list(rust_files())
    test_modules = test_module_files(files)
    for p in files:
        if is_test_file(p) or p in test_modules or vendored(rel(p)):
            continue
        txt = without_literals(without_test_items(read(p)))
        for i, code in enumerate(txt.split("\n"), 1):
            for k in BUDGET:
                n = count_panic(code, k)
                if n:
                    counts[k] += n
                    where[k].append(f"{rel(p)}:{i}")

    for k, budget in BUDGET.items():
        if counts[k] > budget:
            # One finding per occurrence, so that scripts/ci/known-red.txt can list each and
            # a new one is red even while an old one is fixed.
            for at in where[k]:
                r.fail(f"{at}: {k} over the budget of {budget}, propagate with `?`")
            r.note(f"{k}: {counts[k]} occorrenze, budget {budget}")
        elif counts[k] < budget:
            r.note(f"{k}: {counts[k]} (budget {budget}) — abbassa il budget in scripts/verify.py")

    return r


# --------------------------------------------------------------------------- #
# 7-bis. shell-failures — a script does not hide a failure
# --------------------------------------------------------------------------- #

# A line of a script that discards stderr, or the whole output, or swallows a failure with
# `|| true`: AGENTS.md "No hidden failures". Each tree has its own budget, measured on the repo
# on 2026-10-25 (forge/test) and 2026-10-31; like BUDGET they only go down. A probe whose failure
# is the answer, `kill -0 "$pid" 2> /dev/null`, does not count (PROBE); the other probes
# (`command -v`, `skopeo inspect`, `grep -q`) count until they are written as such a rule.
SHELL_BUDGETS = {
    "forge/build": 7,
    "forge/builder": 1,
    "forge/scripts": 14,
    "forge/test": 9,
    "forge/tools": 0,
    "scripts": 40,
    "system": 7,
}
# The same constructs in the steps of a workflow or of an action, with `continue-on-error`.
WORKFLOW_DIRS = (".github/workflows", ".github/actions")
WORKFLOW_BUDGET = 20
# The guest's shell of the ISO test, held in the Python's byte strings.
GUEST_SHELL_DIR = "forge/test/iso"
GUEST_SHELL_FILE = "console.py"
GUEST_SHELL_BUDGET = 8
DEV_NULL = r"""["']?/dev/null["']?"""
# Not covered: `|& cmd >/dev/null`, which no simple pattern tells from a plain stdout discard;
# a quote that spans lines; text run through `eval` or `$(...)`; a construct split across byte
# strings that the guest's shell joins with `+`; a here-document opened inside a double-quoted
# `$(...)` that spans lines (`out="$(cat <<EOF`), whose body is read as code (a false positive);
# a `#` after an escaped space (`\ #`), read as a comment (a false negative).
NOOP = r"(?:(?:/usr)?/bin/true|true|:|exit\s+0|return\s+0)"
HIDDEN_FAILURE = re.compile(
    rf"2>[>|]?\s*{DEV_NULL}|&>>?\s*{DEV_NULL}|>&\s*{DEV_NULL}|>\|?\s*{DEV_NULL}\s+2>&1|2>&-"
    rf"|\|\|\s*(?:{NOOP}(?![\w-])|\{{\s*{NOOP}\s*;?\s*\}}|\(\s*{NOOP}\s*;?\s*\))"
    rf"|\|\|(?!\s*[{{(])[^;|&]*(?:;|&&)\s*{NOOP}(?![\w-])"
    r"|\|\|\s*[{(][^;|&]*;\s*(?:true|:)\s*;?\s*[})]"
    r"|2>&1\s*\|\|\s*(?:break|continue)\b")
# `kill -0 PID 2> /dev/null` only asks whether the process lives: the exit status is the answer
# and the one error sent away is "No such process". Only a plain operand (no substitution) and
# only `kill` in command position are excused; `echo kill -0 $p 2>/dev/null` still counts.
_VAR = r"\$\{?\w+\}?"
PROBE = re.compile(
    r"(?:^\s*(?:!\s*)?|(?:[;(!{]|&&|\|\||\||(?<![\w-])(?:if|while|until|then|do|elif)(?![\w-]))\s*)"
    rf"kill\s+-0\s+(?:\"[^\"$`()]*\"|\"{_VAR}\"|'[^']*'|{_VAR}|\w+|-?\d+)\s+2>\s*{DEV_NULL}")
CONTINUE_ON_ERROR = re.compile(r"^\s*(?:-\s+)?continue-on-error:(?!\s*false\b)")
SHELL_SHEBANG = re.compile(rb"#!.*\b(?:ba|da)?sh\b")
HEREDOC_START = re.compile(r"(?<!<)<<(?!<)(-?)")
HEREDOC_WORD = re.compile(r"""\s*\\?(['"]?)(\w+)\1""")
ARITHMETIC = re.compile(r"\(\([^\n]*?\)\)")


def shell_code(line):
    """`(code, bare)` of `line`. `code` is the line without its comment: a `#` starts one only
    outside quotes and after a space or at the start, so `${#x}`, `$#` and "a #b" stay. `bare`
    is `code` with the text of every quoted string and every arithmetic expression blanked,
    where a here-document can be looked for."""
    quote, i, bare = "", 0, []
    while i < len(line):
        c = line[i]
        if quote:
            if c == "\\" and quote != "'":
                bare.append("  ")
                i += 2
                continue
            if c == quote[-1]:
                quote = ""
                bare.append(c)
            else:
                bare.append(" ")
        elif line.startswith("$'", i):
            # $'...': a backslash escapes the quote, unlike in a single-quoted string.
            quote = "$'"
            bare.append("$'")
            i += 1
        elif c == "\\":
            bare.append("  ")
            i += 2
            continue
        elif c in "'\"":
            quote = c
            bare.append(c)
        elif c == "#" and (i == 0 or line[i - 1] in " \t"):
            line = line[:i]
            break
        else:
            bare.append(c)
        i += 1
    return line, ARITHMETIC.sub(lambda m: " " * len(m.group()), "".join(bare))


OR_CONTINUED = re.compile(r"\|\|\s*\\$")


def or_continued(carry, code):
    """`(code, wait)`: `code` joined to the `|| \\` carried from the line above, and whether it
    ends with another one, so that the next line is joined in its turn."""
    if carry:
        code = carry + code.lstrip()
    return OR_CONTINUED.sub("|| ", code), bool(OR_CONTINUED.search(code))


def hidden(code):
    """The constructs of `code` that hide a failure, a probe left out."""
    return HIDDEN_FAILURE.findall(PROBE.sub(" ", code))


def scan_shell(text):
    """`(found, open_from)`: the numbers of the lines of shell `text` with a construct that
    hides a failure, and the line of a here-document still open at the end, whose rest no one
    checked (0 when none is). Neither a comment nor the body of a here-document counts."""
    found, pending, carry, start = [], [], "", 0
    for i, line in enumerate(text.split("\n"), 1):
        if pending:
            word, tabs, _ = pending[0]
            if (line.lstrip("\t") if tabs else line) == word:
                pending.pop(0)
            continue
        code, bare = shell_code(line)
        for m in HEREDOC_START.finditer(bare):
            w = HEREDOC_WORD.match(code, m.end())
            if w:
                pending.append((w.group(2), m.group(1) == "-", i))
        # A `|| \\` goes on in the next line: the two are read as one, reported at the first.
        code, wait = or_continued(carry, code)
        if wait:
            carry, start = code, start or i
        else:
            if hidden(code):
                found.append(start or i)
            carry, start = "", 0
    if carry and hidden(carry):
        found.append(start)
    return found, pending[0][2] if pending else 0


def hidden_failures(text):
    return scan_shell(text)[0]


def links(base):
    """The links under `base`, to a file or a directory: `walk` follows neither."""
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = entered(Path(dirpath), dirnames, base)
        for name in dirnames + filenames:
            if (Path(dirpath) / name).is_symlink():
                yield Path(dirpath) / name


def shell_scripts(base):
    """The shell scripts under `base`: a `.sh` file, or one whose first line names a shell. A
    link is no script, as `links` refuses it, nor is a socket or a pipe a run left behind."""
    for p in walk(base, ""):
        if p.is_symlink() or not p.is_file():
            continue
        if p.suffix == ".sh":
            yield p
            continue
        with open(p, "rb") as f:
            if SHELL_SHEBANG.match(f.readline(256)):
                yield p


def workflow_failures(text):
    """The numbers of the lines of workflow `text` that hide a failure. A step's script is
    indented in the YAML, so a here-document's end is not looked for: every line is read."""
    found, carry, start = [], "", 0
    for i, line in enumerate(text.split("\n"), 1):
        code, wait = or_continued(carry, shell_code(line)[0])
        if wait:
            carry, start = code, start or i
            continue
        if CONTINUE_ON_ERROR.match(line) or hidden(code):
            found.append(start or i)
        carry, start = "", 0
    if carry and hidden(carry):
        found.append(start)
    return found


def guest_failures(text):
    """`(found, opened)` of the byte strings of Python `text`: the line of each string holding
    shell that hides a failure, once per construct, as one string chains many commands on a
    line, and the lines of those with a here-document still open at the end."""
    found, opened = [], []
    for node in ast.walk(ast.parse(text)):
        if isinstance(node, ast.Constant) and isinstance(node.value, bytes):
            shell = node.value.decode("utf-8", "replace")
            lines, open_from = scan_shell(shell)
            code = shell.split("\n")
            for i in lines:
                found += [node.lineno] * len(hidden(shell_code(code[i - 1])[0]))
            if open_from:
                opened.append(node.lineno)
    return sorted(found), opened


def over_budget(r, found, budget, constant, what):
    """Fails `r` per finding when `found` is over `budget`, or notes that it can come down."""
    if len(found) > budget:
        # One finding per line, as for panics, so that known-red.txt can list each.
        for at in found:
            r.fail(f"{at}: hides a failure, over the budget of {budget} of {what}; "
                   "send the output to a log or report it on stderr")
        r.note(f"shell-failures: {what}: {len(found)} occorrenze, budget {budget}")
    elif len(found) < budget:
        r.note(f"shell-failures: {what}: {len(found)} (budget {budget}) — "
               f"abbassa {constant} in scripts/verify.py")


@check("shell-failures", "Il budget di fallimenti nascosti negli script e nei workflow non cresce")
def check_shell_failures():
    r = Result()
    for tree, budget in SHELL_BUDGETS.items():
        found = []
        for p in sorted(links(ROOT / tree)):
            r.fail(f"{p.relative_to(ROOT)}: a link hides a script from the budget: "
                   "move the script here, or call it by its own path")
        for p in sorted(shell_scripts(ROOT / tree)):
            lines, opened = scan_shell(read(p))
            found += [f"{rel(p)}:{i}" for i in lines]
            if opened:
                # Fail closed: the scanner skips a here-document body, so one it cannot close
                # leaves the rest of the file unchecked.
                r.fail(f"{rel(p)}:{opened}: unterminated here-document from line {opened}: "
                       "the scanner cannot check the rest")
        over_budget(r, found, budget, "SHELL_BUDGETS", tree)
    found = [f"{rel(p)}:{i}" for tree in WORKFLOW_DIRS
             for p in sorted(walk(ROOT / tree, (".yml", ".yaml")))
             for i in workflow_failures(read(p))]
    over_budget(r, found, WORKFLOW_BUDGET, "WORKFLOW_BUDGET", "workflows")
    found = []
    for p in walk(ROOT / GUEST_SHELL_DIR, GUEST_SHELL_FILE):
        lines, opened = guest_failures(read(p))
        found += [f"{rel(p)}:{i}" for i in lines]
        for i in opened:
            r.fail(f"{rel(p)}:{i}: unterminated here-document: the scanner cannot check the rest")
    over_budget(r, found, GUEST_SHELL_BUDGET, "GUEST_SHELL_BUDGET", GUEST_SHELL_FILE)
    return r


# --------------------------------------------------------------------------- #
# 7-ter. css — the shell's CSS takes its lengths and its motion from the tokens
# --------------------------------------------------------------------------- #

# doc_visual_language.md VL11: a length in `px` that does not come from a token, a font size in
# `px`, and a transition or an animation whose duration is not one of `[sal.motion]`'s three or
# whose curve is not its `curve` fail. The GTK shell keeps today's CSS until it is removed (VL1,
# ADR-0124 decision 7), so what it holds today stands here by file and kind, a budget that only
# goes down; a file not named, the toolkit's included, has a budget of zero.
CSS_DIRS = ["system", "forge/specs"]
CSS_TOKENS = "system/athanor-style/calmo/tokens.toml"
CSS_BUDGETS = {
    "forge/specs/athanor-recovery/athanor-recovery-1.0.0/src/main.rs": {"px": 31, "font-px": 2},
    "system/athanor-style/calmo/templates/surfaces.css.in": {"px": 71, "font-px": 13},
    "system/athanor-style/src/accent_template.css": {"px": 2},
    "system/athanor-style/src/style.css": {"px": 178, "font-px": 15, "duration": 9, "curve": 15},
}
# A literal length in pixels; `${token}px` in a template, or `{}px` in Rust, is not one.
CSS_PX = re.compile(r"(?<![\w.])(\d+(?:\.\d+)?|\.\d+)px\b")
# A value runs to `;` or a brace; a template's `${token}` is part of it.
CSS_DECLARATION = re.compile(r"([a-z-]+)\s*:\s*((?:\$\{\w+\}|[^;{}])*)")
# A string literal of Rust source holds CSS when it holds a declaration, ended by `;` or by the
# `}` of its rule. Only whole literals are read: CSS built with `concat!` or `format!`, or split
# across literals, is seen piece by piece, and a piece without a declaration is not read.
CSS_IN_RUST = re.compile(r"[a-z-]+\s*:\s*[^;{}\n]+(?:;|\})")
CSS_MOTION = {"transition", "transition-duration", "transition-timing-function", "transition-delay",
              "animation", "animation-duration", "animation-timing-function", "animation-delay"}
CSS_TIME = re.compile(r"(?<![\w.])(\d+(?:\.\d+)?|\.\d+)(ms|s)\b")
CSS_CURVE = re.compile(r"cubic-bezier\(([^)]*)\)|\b(?:ease(?:-in-out|-in|-out)?|linear|step-start|step-end|steps)\b")


def css_texts(path, text):
    """[(line, css)]: a stylesheet whole, or each string literal of Rust source outside its
    test items that holds a CSS declaration, with the line it starts on."""
    if path.suffix != ".rs":
        return [(1, text)]
    code, found, i, line = without_test_items(text), [], 0, 1
    while i < len(code):
        j = skip_rust(code, i)
        if j == i:
            line += code[i] == "\n"
            i += 1
            continue
        if code[i] not in "/'" and CSS_IN_RUST.search(code, i, j):
            found.append((line, code[i:j]))
        line += code.count("\n", i, j)
        i = j
    return found


def css_problems(css, motion):
    """[(line, kind, text)] of `css`, lines counted from 0: each literal `px` length (kind
    `px`), font size in `px` (`font-px`), and duration or delay (`duration`) or curve (`curve`)
    of a transition or an animation that `motion`, the `[sal.motion]` table, does not hold; a
    time of zero is none."""
    durations = {motion["fast"], motion["normal"], motion["slow"]}
    curve = [float(x) for x in motion["curve"]]
    found = []
    for m in CSS_PX.finditer(css):
        if float(m.group(1)):
            found.append((css.count("\n", 0, m.start()), "px", m.group(0)))
    for m in CSS_DECLARATION.finditer(css):
        name, value = m.group(1), m.group(2)
        at = css.count("\n", 0, m.start())
        if name in ("font", "font-size") and "px" in value:
            found.append((at, "font-px", f"{name}: {value.strip()}"))
        if name not in CSS_MOTION:
            continue
        for t in CSS_TIME.finditer(value):
            ms = round(float(t.group(1)) * (1 if t.group(2) == "ms" else 1000), 3)
            if ms and ms not in durations:
                found.append((at, "duration", t.group(0)))
        for c in CSS_CURVE.finditer(value):
            if c.group(1) is None or [float(x) for x in c.group(1).split(",")] != curve:
                found.append((at, "curve", c.group(0)))
    return found


def css_findings(root=ROOT):
    """{path: [(line, kind, text)]} over the stylesheets, templates and Rust sources of
    CSS_DIRS, generated sheets, vendored crates and test code aside."""
    motion = tomllib.loads(read(root / CSS_TOKENS))["sal"]["motion"]
    rust = [p for d in CSS_DIRS for p in walk(root / d, ".rs")]
    tests = test_module_files(rust)
    files = [p for d in CSS_DIRS for suffix in (".css", ".css.in") for p in walk(root / d, suffix)]
    files += [p for p in rust if not (is_test_file(p) or p in tests)]
    found = {}
    for p in sorted(files):
        path = str(p.relative_to(root))
        if "/generated/" in path or vendored(path):
            continue
        for start, css in css_texts(p, read(p)):
            for line, kind, text in css_problems(css, motion):
                found.setdefault(path, []).append((start + line, kind, text))
    return found


def css_budget(r, found, budgets):
    """Fails `r` per finding of a file and kind over its budget in `budgets`, and once per
    budget above its findings, so that the budget comes down with them."""
    for path in sorted(set(found) | set(budgets)):
        for kind in ("px", "font-px", "duration", "curve"):
            hits = [f for f in found.get(path, []) if f[1] == kind]
            budget = budgets.get(path, {}).get(kind, 0)
            if len(hits) > budget:
                for line, _, text in hits:
                    r.fail(f"{path}:{line}: {text}: {kind} over the budget of {budget}; take the "
                           "value from a token of tokens.toml (doc_visual_language.md VL11)")
            elif len(hits) < budget:
                r.fail(f"{path}: {kind}: {len(hits)} finding(s) under the budget of {budget}; "
                       "lower CSS_BUDGETS in scripts/verify.py to match")


@check("css", "The shell's CSS takes its lengths and motion from the tokens (VL11)")
def check_css():
    r = Result()
    css_budget(r, css_findings(), CSS_BUDGETS)
    return r


# --------------------------------------------------------------------------- #
# 7a. allow-list — a shell client that needs a privileged global has its line
# --------------------------------------------------------------------------- #

# The compositor's allow-list (doc_own_stack.md OS4) and the class of every global it offers.
ALLOW_LIST = "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/privileged-clients"
GLOBALS_FILE = "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/globals"
LAYER_SHELL = "zwlr_layer_shell_v1"
# Outside the list until it moves (doc_session.md revision 3, SN12; doc_own_stack.md section 6,
# step 5): the greeter runs on cosmic-comp in greetd's own session, not under this compositor.
# A line for it, or no spec installing it, ends the exemption with a failure.
OUTSIDE_THE_LIST = {"/usr/bin/athanor-greeter-ui"}
# Upstream clients with a line (doc_session.md revision 4, SN12 and SN15): no spec under
# forge/specs installs them and no crate of the tree builds them, so neither the executable nor
# its interfaces can be derived here; their interfaces come from SN12's derivation run. Each
# must keep one line, for the unit it runs in, and a spec that comes to install one ends its
# exemption with a failure. A scope is the one the supervisor of SESSION_COMPONENTS starts it in.
UPSTREAM = {"/usr/bin/cosmic-greeter": "cosmic-greeter-locker.scope",
            "/usr/bin/cosmic-osd": "cosmic-osd.service",
            "/usr/bin/cosmic-randr": "app-athanor-com.system76.CosmicSettings*@.service"}
SESSION_COMPONENTS = "forge/specs/athanor-system-config/SOURCES/usr/libexec/athanor/session-components"
PACKAGES_JSON = "forge/config/packages.json"
# Settings keeps the main socket for this client's sake, and no longer than it stays upstream.
SETTINGS_CLIENT = "/usr/bin/cosmic-randr"
# The compositor serves those globals and the layers, and enforces the list: no client to check.
# Its installed path and its bin target both, so that another binary installed there is checked.
SERVER = ("athanor-sulphur", "/usr/bin/athanor-sulphur")
# The name of the layer enum (gtk4-layer-shell's, the zwlr protocol's) and each alias of it: a
# surface on its top or overlay layer, however the variant is reached, is refused without a grant.
LAYER_ALIAS = re.compile(r"\bLayer\s+as\s+(\w+)|\btype\s+(\w+)\s*=\s*(?:[\w:]+::)?Layer\s*;")
# What a spec puts under these directories is an executable a line can name.
EXEC_DIRS = {"%{_bindir}": "/usr/bin", "%{_sbindir}": "/usr/sbin", "%{_libexecdir}": "/usr/libexec",
             "%{_prefix}": "/usr"}
EXEC_ROOTS = ("/usr/bin/", "/usr/sbin/", "/usr/libexec/")
INSTALL = {"install", "%{__install}", "%__install", "/usr/bin/install"}
COPY = {"cp", "%{__cp}", "%__cp", "/usr/bin/cp"}
# A file of a path dependency that one public call alone enters counts for a target only when
# code outside that crate makes the call, written as a method or as a path: athanor-sulphur-
# client binds the capture globals at the first `Client::capture`, the window actions at the
# first action of `Client::window_actions`, the keyboard group at the first
# `Client::watch_keyboard_group`, athanor-sulphur's accessibility manager at the first
# `Client::watch_accessibility`, its tiling manager at the first `Client::watch_tiling`, and the
# toplevel list and athanor-sulphur's toplevel manager at the first `Client::watch_windows`;
# capture.rs, actions.rs, keyboard_group.rs, a11y.rs, workspace_tiling.rs and windows.rs hold
# every name of them in the crate. connection.rs binds the rest when a client connects, so it
# counts for a target only when code outside the crate names `Client`, the one type that
# connects, under any alias: a crate that reads only the crate's `idle` module, as athanor-idle
# does, binds nothing through it (maintainer decision, 2026-11-07).
ON_DEMAND = {"system/athanor-sulphur-client/src/capture.rs":
             ("system/athanor-sulphur-client", re.compile(r"\.\s*capture\b|::\s*capture\b(?!\s*::)")),
             "system/athanor-sulphur-client/src/actions.rs":
             ("system/athanor-sulphur-client",
              re.compile(r"\.\s*window_actions\b|::\s*window_actions\b(?!\s*::)")),
             "system/athanor-sulphur-client/src/keyboard_group.rs":
             ("system/athanor-sulphur-client",
              re.compile(r"\.\s*watch_keyboard_group\b|::\s*watch_keyboard_group\b(?!\s*::)")),
             "system/athanor-sulphur-client/src/a11y.rs":
             ("system/athanor-sulphur-client",
              re.compile(r"\.\s*watch_accessibility\b|::\s*watch_accessibility\b(?!\s*::)")),
             "system/athanor-sulphur-client/src/workspace_tiling.rs":
             ("system/athanor-sulphur-client",
              re.compile(r"\.\s*watch_tiling\b|::\s*watch_tiling\b(?!\s*::)")),
             "system/athanor-sulphur-client/src/windows.rs":
             ("system/athanor-sulphur-client",
              re.compile(r"\.\s*watch_windows\b|::\s*watch_windows\b(?!\s*::)")),
             "system/athanor-sulphur-client/src/connection.rs":
             ("system/athanor-sulphur-client",
              re.compile(r"\bClient\b"))}
CLIENT_CRATE = "system/athanor-sulphur-client"
# Where athanor-sulphur-client names the privileged globals it binds at connect, at the
# first capture, at the first window action, at the first watch of the keyboard group, at the
# first watch of the accessibility state, at the first watch of the tiling and at the first watch
# of the windows. Each names at least one, or the check no longer sees what the client binds.
CLIENT_BINDS = {"system/athanor-sulphur-client/src/connection.rs": "at connect",
                "system/athanor-sulphur-client/src/capture.rs": "at the first capture",
                "system/athanor-sulphur-client/src/actions.rs": "at the first window action",
                "system/athanor-sulphur-client/src/keyboard_group.rs":
                "at the first watch of the keyboard group",
                "system/athanor-sulphur-client/src/a11y.rs":
                "at the first watch of the accessibility state",
                "system/athanor-sulphur-client/src/workspace_tiling.rs":
                "at the first watch of the tiling",
                "system/athanor-sulphur-client/src/windows.rs":
                "at the first watch of the windows"}
# The classes of data/globals, as the compositor's fixture reads them (`classified`).
CLASSES = ("standard", "unconfined", "layer-gated", "privileged")


def top_layer(files):
    """Where the first of `files` reaches the top or overlay variant of the layer enum, by its
    name or an alias, a variant import or a glob import of it; None when none does."""
    names = {"Layer"} | {a for text in files.values() for m in LAYER_ALIAS.finditer(text) for a in m.groups() if a}
    variant = re.compile(rf"\b(?:{'|'.join(sorted(names))})\s*::\s*"
                         r"(?:\*|\{[^}]*\b(?:Top|Overlay)\b|(?:Top|Overlay)\b)")
    for where, text in sorted(files.items()):
        found = variant.search(text)
        if found:
            return f"{where}:{text.count(chr(10), 0, found.start()) + 1}"
    return None


def spec_installs(spec):
    """(source, executable) of each file an install or a cp of the spec puts under /usr/bin,
    /usr/sbin or /usr/libexec: continued lines joined, a command after `do`, `then` or `else`,
    quoted words, a target directory given by -t or as the last argument, a destination
    directory given with or without its trailing slash."""
    for command in re.split(r"&&|\|\||;|\n", spec.replace("\\\n", " ")):
        words = [w.strip("\"'") for w in command.split()]
        if words[:1] in (["do"], ["then"], ["else"]):
            words = words[1:]
        if not words or words[0] not in INSTALL | COPY:
            continue
        args, directory, i = [], None, 1
        while i < len(words):
            w = words[i]
            if w in ("--target-directory", "--mode", "--owner", "--group", "--suffix"):
                directory, i = (words[i + 1] if w == "--target-directory" and i + 1 < len(words) else directory), i + 1
            elif w.startswith("--target-directory="):
                directory = w.split("=", 1)[1]
            elif w == "--directory" and words[0] in INSTALL:
                break
            elif w.startswith("-") and len(w) > 1 and not w.startswith("--"):
                for k, c in enumerate(w[1:], 1):
                    if c == "d" and words[0] in INSTALL:
                        args = None
                        break
                    if c in "mogSt":
                        value = w[k + 1:]
                        if not value and i + 1 < len(words):
                            i, value = i + 1, words[i + 1]
                        directory = value if c == "t" else directory
                        break
                if args is None:
                    break
            elif not w.startswith("--"):
                args.append(w)
            i += 1
        if not args:
            continue  # install -d makes directories
        if directory is None:
            if len(args) < 2:
                continue
            *sources, directory = args
            if len(sources) == 1 and not directory.endswith("/") and \
                    executable_path(directory) not in EXEC_DIRS.values():
                if executable_path(directory).startswith(EXEC_ROOTS):
                    yield sources[0], executable_path(directory)
                continue
        else:
            sources = args
        for source in sources:
            executable = executable_path(f"{directory}/{source.rsplit('/', 1)[-1]}")
            if executable.startswith(EXEC_ROOTS):
                yield source, executable


def executable_path(destination):
    """`destination` of a spec's %install as the path it has on the installed system."""
    for root in ("%{buildroot}", "%buildroot", "${RPM_BUILD_ROOT}", "$RPM_BUILD_ROOT"):
        destination = destination.replace(root, "")
    destination = re.sub(r"%(_\w+)", r"%{\1}", destination)  # %_bindir is %{_bindir}
    for macro, value in EXEC_DIRS.items():
        destination = destination.replace(macro, value)
    return re.sub(r"/+", "/", destination.strip("\"'")).rstrip("/") or "/"


def binaries(crate, manifest):
    """{name: entry file} of each bin target of `crate`, Cargo's auto-discovered ones (src/main.rs,
    src/bin/*.rs, src/bin/*/main.rs) and those of `[[bin]]`."""
    package = manifest.get("package", {})
    found = {}
    if package.get("autobins", True):
        if package.get("name") and (crate / "src/main.rs").is_file():
            found[package["name"]] = crate / "src/main.rs"
        found.update((p.stem, p) for p in sorted((crate / "src/bin").glob("*.rs")))
        found.update((p.parent.name, p) for p in sorted((crate / "src/bin").glob("*/main.rs")))
    for b in manifest.get("bin", []):
        entry = crate / b["path"] if "path" in b else found.get(b["name"], crate / "src/main.rs")
        found = {n: p for n, p in found.items() if p != entry}
        found[b["name"]] = entry
    return found


def without_comments(txt):
    """`txt` with each comment blanked, newlines kept; strings stay, a global can be bound
    by its name."""
    out, at, i = [], 0, 0
    while i < len(txt):
        j = skip_rust(txt, i)
        if j == i:
            i += 1
            continue
        if txt.startswith(("//", "/*"), i):
            out.append(txt[at:i])
            out.append(re.sub(r"[^\n]", " ", txt[i:j]))
            at = j
        i = j
    out.append(txt[at:])
    return "".join(out)


def crate_sources(crate, start, entry=None):
    """The production files of the target whose entry point, `entry` (the library when None),
    lies in `start`: minus the crate's other bin targets inside it, each with its modules'
    directory, tests, and the modules and items under `#[cfg(test)]`. Each as (path, text
    without test items and comments)."""
    manifest = tomllib.loads(read(crate / "Cargo.toml"))
    others = [p for p in binaries(crate, manifest).values() if p != entry]
    dirs = [p.parent if p.name == "main.rs" else p.with_suffix("") for p in others]
    dirs = [d for d in dirs if d != start and d.is_relative_to(start)]
    files = [p for p in walk(start, ".rs") if p not in others and not any(p.is_relative_to(d) for d in dirs)]
    test_modules = test_module_files(files)
    return [(p, without_comments(without_test_items(read(p))))
            for p in files if not is_test_file(p) and p not in test_modules]


def enabled_dependencies(manifest, features, defaults):
    """The optional dependencies `features` turn on (and `default` with `defaults`), and the
    features each enables on a dependency, as Cargo reads `[features]`."""
    table = manifest.get("features", {})
    todo = list(features) + (["default"] if defaults else [])
    seen, on, sub, weak = set(), set(), collections.defaultdict(set), []
    while todo:
        f = todo.pop()
        if f in seen:
            continue
        seen.add(f)
        if f.startswith("dep:"):
            on.add(f[4:])
        elif "/" in f:
            dep, feature = f.split("/", 1)
            if dep.endswith("?"):
                weak.append((dep[:-1], feature))
            else:
                on.add(dep)
                sub[dep].add(feature)
        elif f not in table:
            on.add(f)  # the implicit feature of an optional dependency
        else:
            todo.extend(table[f])
    for dep, feature in weak:
        if dep in on:
            sub[dep].add(feature)
    return on, sub


def path_dependencies(crate, manifest, root):
    """(name in code, crate directory, entry) of each path dependency of `[dependencies]` and
    of `[target.*.dependencies]`, through `workspace = true` too."""
    workspace = tomllib.loads(read(root / "Cargo.toml")).get("workspace", {}).get("dependencies", {}) \
        if (root / "Cargo.toml").is_file() else {}
    tables = [manifest.get("dependencies", {})]
    tables += [t.get("dependencies", {}) for t in manifest.get("target", {}).values()]
    for table in tables:
        for name, entry in table.items():
            if not isinstance(entry, dict):
                continue
            base, path = crate, entry.get("path")
            if entry.get("workspace") and isinstance(workspace.get(name), dict):
                base, path = root, workspace[name].get("path")
            if path:
                yield name, entry, (base / path).resolve()


def depends_on(root, where, owner):
    """Whether the crate of `where`, a file relative to `root`, reaches the crate at `owner`,
    relative to `root` too, through the path dependencies of its Cargo.toml, under any name,
    directly or through the dependencies of those."""
    root = root.resolve()
    crate = next((d for d in (root / where).parents if (d / "Cargo.toml").is_file()), None)
    seen, todo = set(), [crate] if crate else []
    while todo:
        crate = todo.pop()
        if crate in seen:
            continue
        seen.add(crate)
        manifest = tomllib.loads(read(crate / "Cargo.toml"))
        todo += [dep for _, _, dep in path_dependencies(crate, manifest, root)]
    return (root / owner).resolve() in seen


def target_files(crate, entry, features, defaults, root):
    """The production files a binary target is built from: its own, and those of the path
    dependencies its code names, followed transitively with the features the build enables
    (an optional dependency counts only when they turn it on)."""
    files, done = {}, {}
    todo = [(crate.resolve(), entry.parent.resolve(), entry.resolve(), set(features), defaults)]
    while todo:
        crate, start, entry, features, defaults = todo.pop()
        before = done.get((crate, start))
        if before and features <= before[0] and (before[1] or not defaults):
            continue
        features = features | (before[0] if before else set())
        defaults = defaults or bool(before and before[1])
        done[(crate, start)] = (features, defaults)
        manifest = tomllib.loads(read(crate / "Cargo.toml"))
        sources = crate_sources(crate, start, entry)
        files.update(sources)
        # A bin target outside the library's directory (src/bin/*.rs) is built with the
        # package's own library, which its code reaches by the package's name.
        lib = crate / manifest.get("lib", {}).get("path", "src/lib.rs")
        if entry is not None and lib.is_file() and lib not in dict(sources):
            todo.append((crate, lib.parent, None, features, defaults))
        code = "\n".join(text for _, text in sources)
        on, sub = enabled_dependencies(manifest, features, defaults)
        for name, entry, dep in path_dependencies(crate, manifest, root):
            if entry.get("optional") and name not in on:
                continue
            ident = name if "package" in entry else \
                tomllib.loads(read(dep / "Cargo.toml")).get("lib", {}).get("name", name)
            # A dependency the code never names adds nothing that runs.
            if not re.search(rf"\b{ident.replace('-', '_')}\b", code):
                continue
            lib = tomllib.loads(read(dep / "Cargo.toml")).get("lib", {}).get("path", "src/lib.rs")
            wanted = set(entry.get("features", [])) | sub[name]
            todo.append((dep, (dep / lib).parent, None, wanted, entry.get("default-features", True)))
    return files


def build_flags(spec, package):
    """(features, defaults, all features) of the spec's `cargo build` of `package`."""
    for line in spec.splitlines():
        words = line.split()
        if words[:2] != ["cargo", "build"]:
            continue
        named = [words[i + 1] for i, w in enumerate(words[:-1]) if w == "-p"]
        if named and package not in named:
            continue
        features = set()
        for i, w in enumerate(words):
            if w in ("--features", "-F") and i + 1 < len(words):
                features.update(re.split(r"[ ,]+", words[i + 1].strip("\"'")))
            elif w.startswith("--features="):
                features.update(re.split(r"[ ,]+", w.split("=", 1)[1].strip("\"'")))
        return features, "--no-default-features" not in words, "--all-features" in words
    return set(), True, False


# The allow-list's mark of a trusted prompt, the one name of a line that is no interface
# (doc_own_stack.md OS5, doc_accessibility.md AX11; identity.rs `MARK`).
TRUSTED_PROMPT = "trusted-prompt"


def allow_list_problems(root=None, notes=None, outside=None, upstream=None):
    """What `verify.py allow-list` reports: each binary a spec installs to /usr/bin, built from
    a crate of the tree, that asks for the top or overlay layer or names a privileged global
    of data/globals has a line in data/privileged-clients, and each of its lines grants
    zwlr_layer_shell_v1 when it asks for those layers and every privileged global its code
    names; each line names an executable a spec installs, and only interfaces of the
    privileged and layer-gated rows. A grant beyond those is the maintainer's, never
    derived here. An executable of `outside` (OUTSIDE_THE_LIST by default) is appended to
    `notes` in place of its problem; one of `upstream` (UPSTREAM by default) has a line and
    is installed by no spec."""
    root = Path(root or ROOT)
    notes = [] if notes is None else notes
    outside = OUTSIDE_THE_LIST if outside is None else outside
    upstream = UPSTREAM if upstream is None else upstream
    problems = []
    classes = {}
    for number, line in enumerate(read(root / GLOBALS_FILE).splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        words = line.split()
        if len(words) != 2 or words[1] not in CLASSES:
            problems.append(f"{GLOBALS_FILE}:{number}: not `<interface> <class>` with a class of "
                            f"{', '.join(CLASSES)}, which the compositor's fixture refuses: {line}")
            continue
        classes[words[0]] = words[1]
    privileged = {i for i, c in classes.items() if c == "privileged"}
    grantable = {i for i, c in classes.items() if c in ("privileged", "layer-gated")}
    if not privileged:
        problems.append(f"{GLOBALS_FILE} has no privileged row: no client would need a line")
    if (root / CLIENT_CRATE / "Cargo.toml").is_file():
        for where, when in CLIENT_BINDS.items():
            text = without_comments(without_test_items(read(root / where))) if (root / where).is_file() else ""
            if not any(re.search(rf"\b{i}\b", text) for i in privileged):
                problems.append(f"{where} names no privileged global of {GLOBALS_FILE}: the check no "
                                f"longer sees what athanor-sulphur-client binds {when}")
    lines = collections.defaultdict(list)
    for line in read(root / ALLOW_LIST).splitlines():
        words = line.split()
        if len(words) in (1, 2) and not words[0].startswith("#"):
            problems.append(f"{ALLOW_LIST}: not `<executable> <unit> <interface>...`, which makes the "
                            f"compositor grant nothing at all: {line.strip()}")
        elif words and not words[0].startswith("#"):
            lines[words[0]].append(words[1:])
            problems += [f"{ALLOW_LIST}: the line of {words[0]} for {words[1]} "
                         f"names {name}, which is no privileged or layer-gated row of {GLOBALS_FILE}"
                         for name in words[2:] if name not in grantable and name != TRUSTED_PROMPT]

    bins = {}
    for manifest_path in (p for d in RS_DIRS for p in walk(root / d, "Cargo.toml")):
        manifest = tomllib.loads(read(manifest_path))
        name = manifest.get("package", {}).get("name")
        for b, entry in binaries(manifest_path.parent, manifest).items():
            bins[b] = (manifest_path.parent, name, entry)

    installed, targets = set(), []
    for spec_path in sorted((root / "forge/specs").glob("*/*.spec")):
        spec = read(spec_path)
        name = re.search(r"^Name:\s*(\S+)", spec, re.M)
        if name:
            spec = spec.replace("%{name}", name.group(1))
        in_tree = any(walk(spec_path.parent, "Cargo.toml"))
        for source, executable in spec_installs(spec):
            installed.add(executable)
            built = re.search(r"(?:^|/)release/([^/\s]+)$", source)
            if built:
                targets.append((built.group(1), executable, spec, spec_path, in_tree))

    checked = 0
    for built, executable, spec, spec_path, in_tree in targets:
        if (built, executable) == SERVER:
            continue
        if built not in bins:
            # Built from a source outside the tree (cosmic-comp, matugen), unless the spec
            # carries a crate: then no bin target of the tree has the name it installs, or
            # the name is a shell variable, a glob or a macro this check cannot expand.
            if in_tree and re.search(r"[$*?\[{%`]", built):
                problems.append(f"{spec_path.relative_to(root)}: an install of target/release/{built} "
                                "names its binary through a shell variable, a glob or a macro the check "
                                "cannot expand: name each binary in the spec")
            elif in_tree:
                problems.append(f"{executable}: {spec_path.relative_to(root)} installs target/release/"
                                f"{built}, which no bin target of a crate under {', '.join(RS_DIRS)} builds")
            continue
        checked += 1
        crate, package, entry = bins[built]
        features, defaults, every = build_flags(spec, package)
        if every:
            features = set(tomllib.loads(read(crate / "Cargo.toml")).get("features", {}))
        layer, names = None, {}
        files = {path.relative_to(root.resolve()): text for path, text in
                 target_files(crate, entry, features, defaults, root).items()}
        # The call is looked for only in crates whose Cargo.toml depends on the owner: a crate
        # that does not cannot reach its calls, whatever its code names (Sal's
        # `DecorationMode::Client` is no `Client` of athanor-sulphur-client; maintainer
        # decision, 2026-11-07).
        for where in [w for w in files if w.as_posix() in ON_DEMAND]:
            owner, call = ON_DEMAND[where.as_posix()]
            if not any(call.search(text) for other, text in files.items()
                       if not other.is_relative_to(owner) and depends_on(root, other, owner)):
                del files[where]
        layer = top_layer(files)
        for where, text in sorted(files.items()):
            for i, line in enumerate(text.split("\n"), 1):
                for interface in privileged:
                    if interface not in names and re.search(rf"\b{interface}\b", line):
                        names[interface] = f"{where}:{i}"
        if layer is None and not names:
            continue
        asks = ([f"the top or overlay layer ({layer})"] if layer else []) + \
            [f"{n} ({at})" for n, at in sorted(names.items())]
        if executable in outside and executable not in lines:
            notes.append(f"{executable}: asks for {', '.join(asks)}, outside the list until it moves "
                         "(doc_session.md SN12)")
        elif executable not in lines:
            problems.append(f"{executable}: asks for {', '.join(asks)} and {ALLOW_LIST} has no line for it")
        else:
            for line in lines[executable]:
                if layer and LAYER_SHELL not in line[1:]:
                    problems.append(f"{executable}: asks for the top or overlay layer ({layer}) and its line "
                                    f"for {line[0]} does not name {LAYER_SHELL}: the surface is closed (#683)")
                problems += [f"{executable}: binds {n} ({at}) and its line for {line[0]} does not grant it"
                             for n, at in sorted(names.items()) if n not in line[1:]]

    if not checked:
        problems.append(f"no spec under forge/specs installs a binary that a crate under {', '.join(RS_DIRS)} "
                        "builds: the check found no target")
    for executable in lines:
        if executable not in installed and executable not in upstream:
            problems.append(f"{ALLOW_LIST}: {executable} is installed by no spec under forge/specs")
    components = read(root / SESSION_COMPONENTS) if (root / SESSION_COMPONENTS).is_file() else ""
    packages = json.loads(read(root / PACKAGES_JSON)) if (root / PACKAGES_JSON).is_file() else {}
    image = {name for key, names in packages.items() if key.startswith("upstream_") for name in names}
    launch = root / CLIENT_CRATE / "src/launch.rs"
    if SETTINGS_CLIENT not in upstream and launch.is_file() and "fn keeps_main_socket" in read(launch):
        problems.append(f"{launch.relative_to(root)}: keeps_main_socket outlives {SETTINGS_CLIENT}: "
                        "remove it (doc_settings.md SE23)")
    for executable, unit in sorted(upstream.items()):
        if executable not in lines:
            problems.append(f"{executable} is upstream and has no line in {ALLOW_LIST}: "
                            "remove it from UPSTREAM (doc_session.md SN12)")
        elif [line[0] for line in lines[executable]] != [unit]:
            problems.append(f"{ALLOW_LIST}: {executable} is upstream and needs one line, for {unit}, "
                            "the unit it runs in")
        started = rf"^[ \t]*systemd-run(?:[ \t]+-\S*)*[ \t]+--unit={re.escape(unit)}(?:[ \t]|$)"
        if unit.endswith(".scope") and not re.search(started, components, re.MULTILINE):
            problems.append(f"{executable}: {SESSION_COMPONENTS} starts no {unit}, the scope its line "
                            "names: the grant would reach nothing")
        if Path(executable).name not in image:
            problems.append(f"{executable} is upstream and {PACKAGES_JSON} no longer installs it: "
                            f"remove its line and its entry in UPSTREAM (doc_settings.md SE23)")
        if executable in installed:
            problems.append(f"{executable} is upstream and a spec under forge/specs installs it: "
                            "remove it from UPSTREAM, so that its line is derived")
    for executable in sorted(outside):
        if executable in lines:
            problems.append(f"{executable} has a line in {ALLOW_LIST} and is still outside the list: "
                            "remove it from OUTSIDE_THE_LIST (doc_session.md SN12)")
        if executable not in installed:
            problems.append(f"{executable} is outside the list and installed by no spec under forge/specs: "
                            "remove it from OUTSIDE_THE_LIST")
    return problems


# What an application behind a security context must not see, one list for the acceptance
# scripts: the compositor's own privileged and layer-gated rows, and the names cosmic-comp has
# for the same powers, which no table of the tree carries (#886).
FORBIDDEN_GLOBALS = "forge/test/shell/forbidden-globals.txt"
FORBIDDEN_READERS = ("forge/test/shell/compositor_e2e.py", "scripts/devvm/compositor-acceptance.sh",
                     "scripts/devvm/dock-acceptance.sh", "scripts/devvm/launcher-acceptance.sh")


def forbidden_globals_problems(root=None):
    """What `verify.py forbidden-globals` reports: a privileged or layer-gated row of
    data/globals that forge/test/shell/forbidden-globals.txt does not name, and a script of
    FORBIDDEN_READERS that does not read that file."""
    root = Path(root or ROOT)
    forbidden = set(read(root / FORBIDDEN_GLOBALS).split())
    rows = [line.split() for line in read(root / GLOBALS_FILE).splitlines() if not line.startswith("#")]
    gated = {row[0] for row in rows if len(row) == 2 and row[1] in ("privileged", "layer-gated")}
    problems = [f"{GLOBALS_FILE} has no privileged row: the list is checked against nothing"] if not gated else []
    problems += [f"{FORBIDDEN_GLOBALS} does not name {name}, a global of {GLOBALS_FILE} no confined "
                 "application may see" for name in sorted(gated - forbidden)]
    problems += [f"{reader} does not read {FORBIDDEN_GLOBALS}"
                 for reader in FORBIDDEN_READERS
                 if Path(FORBIDDEN_GLOBALS).name not in read(root / reader)]
    return problems


@check("forbidden-globals", "The acceptance scripts forbid every global the compositor privileges")
def check_forbidden_globals():
    r = Result()
    for problem in forbidden_globals_problems():
        r.fail(problem)
    return r


@check("allow-list", "Every shipped client that asks for a privileged global or layer has its allow-list line")
def check_allow_list():
    r, notes = Result(), []
    for problem in allow_list_problems(notes=notes):
        r.fail(problem)
    for note in notes:
        r.note(note)
    return r


# --------------------------------------------------------------------------- #
# 7b. riga di comando del kernel — le decisioni di doc_kernel_profile.md
# --------------------------------------------------------------------------- #

# Parameters the kernel profile decided against (docs/architecture/doc_kernel_profile.md,
# "Command line"). A name ending in a dot is a prefix. profile.toml is the single source of
# the base command line and kernel_profile.py check holds its generated files to it; this
# keeps a contradicting parameter out of the manifest and of the variant files beside it.
REJECTED_PARAMETERS = {
    "iommu=pt": "identity mapping for every device, against D16 (lazy translation, strict for untrusted ports)",
    "oops=panic": "the first oops panics before oops_limit is consulted, against D19",
    "zswap.": "swap is on zram and zswap is off, against D15",
    "pti=on": "forces page table isolation on CPUs not affected by Meltdown",
    "amd_iommu=on": "not a parameter of the amd_iommu driver, which logs it as unknown",
    "lam=on": "not an x86 parameter",
    "arm64.mte=on": "not an x86 parameter",
}

# The files that carry the kernel command line: the boot line of azoth and every kargs.d
# file the image ships (the base one generated from profile.toml, the NVIDIA variants').
CMDLINE_FILE = "forge/specs/azoth/cmdline"
KARGS_FILES = [
    "forge/specs/athanor-kernel-profile/SOURCES/usr/lib/bootc/kargs.d/10-athanor-kernel-profile.toml",
    "system/nvidia/athanor-nvidia-config/SOURCES/usr/lib/bootc/kargs.d/01-nvidia.toml",
]


def cmdline_problems(root=None):
    """No place that writes the kernel command line carries a parameter the profile rejects."""
    root = root or ROOT
    problems = []
    found = {}

    def read_site(relative, extract):
        try:
            found[relative] = extract(read(root / relative))
        except (OSError, ValueError, KeyError, tomllib.TOMLDecodeError) as err:
            problems.append(f"{relative}: cannot read the command line ({err})")

    read_site(CMDLINE_FILE, str.split)
    for relative in KARGS_FILES:
        read_site(relative, lambda text: tomllib.loads(text)["kargs"])

    for site, parameters in found.items():
        for parameter in parameters:
            for rejected, why in REJECTED_PARAMETERS.items():
                if parameter == rejected or (rejected.endswith(".") and parameter.startswith(rejected)):
                    problems.append(f"{site}: {parameter} -> {why}")
    return problems


@check("cmdline", "La riga di comando del kernel non contraddice le decisioni del profilo")
def check_cmdline():
    r = Result()
    for problem in cmdline_problems():
        r.fail(problem)
    return r


# The base sysctls have one source, the [base.sysctl] table of profile.toml, which
# kernel_profile.py writes into the sysctl.d file of athanor-kernel-profile. A second
# sysctl.d file that sets one of them either repeats it or, sorting later, overrides it
# (99-bore.conf did both, doc_kernel_profile.md section 6).
PROFILE_MANIFEST = "forge/specs/athanor-kernel-profile/profile.toml"
PROFILE_SYSCTL = "forge/specs/athanor-kernel-profile/SOURCES/usr/lib/sysctl.d/90-athanor-kernel-profile.conf"
SYSCTL_GLOBS = ("forge/specs/**/sysctl.d/*.conf", "system/**/sysctl.d/*.conf")


def sysctl_problems(root=None):
    """No sysctl.d file of the repository but the generated one sets a base sysctl."""
    root = root or ROOT
    try:
        base = set(tomllib.loads(read(root / PROFILE_MANIFEST))["base"]["sysctl"])
    except (OSError, KeyError, tomllib.TOMLDecodeError) as err:
        return [f"{PROFILE_MANIFEST}: cannot read the base sysctls ({err})"]
    problems = []
    files = sorted({path for pattern in SYSCTL_GLOBS for path in root.glob(pattern)})
    for path in files:
        relative = path.relative_to(root).as_posix()
        if relative == PROFILE_SYSCTL:
            continue
        for number, line in enumerate(read(path).splitlines(), 1):
            name = line.split("=", 1)[0].strip().lstrip("-").replace("/", ".")
            if "=" in line and not line.lstrip().startswith(("#", ";")) and name in base:
                problems.append(f"{relative}:{number}: sets {name}, a base sysctl of {PROFILE_MANIFEST}")
    return problems


@check("sysctl", "The base sysctls have one source, profile.toml")
def check_sysctl():
    r = Result()
    for problem in sysctl_problems():
        r.fail(problem)
    return r


# A Cargo.lock whose workspace cargo-deny does not read is read by no advisory check unless
# scripts/ci/security-audit.sh audits it with `cargo audit --file`. That script reads its list from
# scripts/ci/audited-lockfiles.txt; a lockfile missing from it is never checked (#465).
AUDITED_LOCKFILES = "scripts/ci/audited-lockfiles.txt"
# The lockfiles of the workspaces security-audit.sh runs `cargo deny check advisories` on.
DENY_LOCKFILES = ("Cargo.lock", "system/athanor-sal/gallery/Cargo.lock", "fuzz/Cargo.lock")
UNAUDITED = re.compile(r"unaudited (\S+) #\d+")


def advisory_lock_problems(root=None):
    """Every tracked Cargo.lock is read by an advisory check: cargo-deny's (DENY_LOCKFILES), or
    cargo-audit's through AUDITED_LOCKFILES, or it is listed there as `unaudited PATH #ISSUE`; every
    entry is tracked. A member of the root workspace, or a crate its [patch] tables point at, is
    resolved by the root Cargo.lock, and a lockfile of its own is never used, so it is not asked for."""
    root = root or ROOT
    manifest = root / "Cargo.toml"
    data = tomllib.loads(read(manifest)) if manifest.exists() else {}
    members = set(data.get("workspace", {}).get("members", []))
    members |= {d["path"] for table in data.get("patch", {}).values() for d in table.values()
                if isinstance(d, dict) and "path" in d}
    listed, unaudited, problems = [], [], []
    # Split on "\n" only, as the shell reader does: CRLF, \x0b, \x0c and \x85 stay in the line.
    raw = (root / AUDITED_LOCKFILES).read_bytes().decode("utf-8", errors="replace")
    for number, line in enumerate(raw.split("\n"), 1):
        if line != line.strip():
            # security-audit.sh takes a line as it is, so a stray blank is part of a path there
            problems.append(f"{AUDITED_LOCKFILES}:{number}: leading or trailing whitespace, which "
                            "security-audit.sh does not strip")
            continue
        if not line or line.startswith("#"):
            continue
        if line.startswith("unaudited"):
            match = UNAUDITED.fullmatch(line)
            if match is None:
                problems.append(f"{AUDITED_LOCKFILES}: expected `unaudited PATH #ISSUE`, got `{line}`")
            else:
                unaudited.append(match[1])
        else:
            listed.append(line)
    script = root / "scripts/ci/security-audit.sh"
    if script.exists():
        loop = re.search(r"^for manifest in (.+); do$", read(script), re.M)
        manifests = loop[1].split() if loop else []
        if sorted(m.replace("Cargo.toml", "Cargo.lock") for m in manifests) != sorted(DENY_LOCKFILES):
            problems.append(f"{script.relative_to(root)}: its cargo-deny loop ({' '.join(manifests)}) "
                            "does not match DENY_LOCKFILES in scripts/verify.py")
    tracked = subprocess.run(["git", "-C", str(root), "ls-files", "-z"],
                             capture_output=True, text=True, check=True).stdout.split("\0")
    locks = {f for f in tracked if f == "Cargo.lock" or f.endswith("/Cargo.lock")}
    found = {f for f in locks - set(DENY_LOCKFILES) if f.removesuffix("/Cargo.lock") not in members}
    problems += [f"{p}: a tracked lockfile that no advisory check reads; add it to {AUDITED_LOCKFILES}"
                 for p in sorted(found - set(listed) - set(unaudited))]
    problems += [f"{AUDITED_LOCKFILES}: {p} does not exist" for p in listed + unaudited if p not in found]
    return problems


@check("advisory-locks", "Every tracked Cargo.lock is read by an advisory check (security-audit.sh), or excluded with an issue")
def check_advisory_locks():
    r = Result()
    for problem in advisory_lock_problems():
        r.fail(problem)
    return r


# A udev rule that sets GROUP= to a group no sysusers.d file declares makes udev log
# "Unknown group" at every boot and fall back to root (doc_kernel_profile.md section 14,
# boot ordering). A group the Fedora base declares goes into athanor-base-config's
# 10-athanor-hw-groups.conf, which also declares it early in boot, before a rule names it.
UDEV_GROUP = re.compile(r'\bGROUP\s*:?=\s*"([^"]+)"')
SYSUSERS_GROUP = re.compile(r"^\s*[gu]\s+(\S+)", re.MULTILINE)


def udev_group_problems(root=None):
    """Every GROUP= of a repository udev rule names a group a repository sysusers.d file declares."""
    root = root or ROOT
    declared = set()
    for pattern in ("forge/specs/**/sysusers.d/*.conf", "forge/specs/**/*.sysusers", "system/**/sysusers.d/*.conf"):
        for path in root.glob(pattern):
            declared.update(SYSUSERS_GROUP.findall(read(path)))
    problems = []
    rules = sorted({p for pattern in ("forge/specs/**/rules.d/*.rules", "system/**/rules.d/*.rules") for p in root.glob(pattern)})
    for path in rules:
        relative = path.relative_to(root).as_posix()
        for number, line in enumerate(read(path).splitlines(), 1):
            if line.lstrip().startswith("#"):
                continue
            for group in UDEV_GROUP.findall(line):
                if group not in declared:
                    problems.append(f"{relative}:{number}: GROUP=\"{group}\" is declared by no sysusers.d file")
    return problems


@check("udev-groups", "Every GROUP= of a udev rule names a group a sysusers.d file declares")
def check_udev_groups():
    r = Result()
    for problem in udev_group_problems():
        r.fail(problem)
    return r


PAM_CONTAINERFILE = "system/Containerfile"
NULLOK_GUARD = re.compile(r"^RUN authselect enable-feature without-nullok\b", re.MULTILINE)
PWQUALITY_MINLEN = "'minlen = 12'"
FAILLOCK_SETTINGS = {"deny": "5", "fail_interval": "900", "unlock_time": "600"}
FAILLOCK_CONF = "/etc/security/faillock.conf"


def nullok_problems(root=None):
    """The image build drops nullok from every authselect stack (decision A2-23)."""
    root = root or ROOT
    try:
        text = read(root / PAM_CONTAINERFILE)
    except OSError as err:
        return [f"{PAM_CONTAINERFILE}: cannot read ({err})"]
    if not NULLOK_GUARD.search(text):
        return [f"{PAM_CONTAINERFILE}: no 'RUN authselect enable-feature without-nullok' step (A2-23)"]
    return []


def pwquality_problems(root=None):
    """The image build asks a new password for twelve characters."""
    root = root or ROOT
    try:
        text = read(root / PAM_CONTAINERFILE)
    except OSError as err:
        return [f"{PAM_CONTAINERFILE}: cannot read ({err})"]
    if any(PWQUALITY_MINLEN in line and not line.lstrip().startswith("#") for line in text.splitlines()):
        return []
    return [f"{PAM_CONTAINERFILE}: no pwquality drop-in with {PWQUALITY_MINLEN}"]


def faillock_problems(root=None):
    """The image build enables authselect's with-faillock with the thresholds of ADR-0090."""
    import shlex
    root = root or ROOT
    try:
        text = read(root / PAM_CONTAINERFILE)
    except OSError as err:
        return [f"{PAM_CONTAINERFILE}: cannot read ({err})"]
    steps = [s.strip() for s in text.replace("\\\n", " ").splitlines() if s.strip().startswith("RUN ")]
    where, problems = PAM_CONTAINERFILE, []
    if any("disable-feature with-faillock" in s for s in steps):
        problems.append(f"{where}: a step disables with-faillock (ADR-0090)")
    if not any("authselect enable-feature with-faillock" in s for s in steps):
        problems.append(f"{where}: no 'RUN authselect enable-feature with-faillock' step (ADR-0090)")
    writers = [s for s in steps if ">> " + FAILLOCK_CONF in s]
    if len(writers) != 1:
        return problems + [f"{where}: expected one RUN step appending to {FAILLOCK_CONF}, found {len(writers)} (ADR-0090)"]
    step = writers[0]
    if "pam_faillock.so preauth" not in step or "pam_faillock.so authfail" not in step:
        problems.append(f"{where}: the faillock step does not check preauth and authfail in the stacks")
    try:
        words = shlex.split(step)
    except ValueError as err:
        return problems + [f"{where}: cannot parse the faillock step ({err})"]
    if "printf" not in words:
        return problems + [f"{where}: the faillock step has no printf"]
    args = []
    for word in words[words.index("printf") + 2:]:
        if word in ("&&", ">>", ";"):
            break
        args.append(word)
    last = {}
    for arg in args:
        m = re.match(r"^\s*(\w+)\s*=\s*(\S+)\s*$", arg)
        if m:
            last[m.group(1)] = m.group(2)
        if re.match(r"^\s*(even_deny_root|admin_group)\b", arg):
            problems.append(f"{where}: faillock.conf must not set {arg.strip()!r}: root is not locked out (ADR-0090)")
    for key, value in FAILLOCK_SETTINGS.items():
        if last.get(key) != value:
            problems.append(f"{where}: faillock.conf must end with {key} = {value}, not {last.get(key)} (ADR-0090)")
    return problems


LOCK_PAM_DIR = "forge/specs/athanor-unlockd/SOURCES/usr/lib/pam.d"
LOCK_PAM = {
    "athanor-lock": [
        "auth requisite pam_faillock.so preauth",
        "auth include system-auth",
        "account include system-auth",
    ],
    "athanor-lock-fingerprint": [
        "auth requisite pam_faillock.so preauth silent",
        "auth [success=done authinfo_unavail=die user_unknown=die default=bad] pam_fprintd.so max-tries=1",
        "auth required pam_faillock.so authfail",
        "auth required pam_deny.so",
        "account include system-auth",
    ],
}
LOCK_PAM_BANNED = ("pam_gnome_keyring", "pam_oo7", "nullok")


def lock_pam_problems(root=None):
    """The session lock's stacks are LP8's lines: no keyring, no nullok, only system-auth included."""
    root = root or ROOT
    problems = []
    for name, expected in LOCK_PAM.items():
        where = f"{LOCK_PAM_DIR}/{name}"
        try:
            text = read(root / where)
        except OSError as err:
            problems.append(f"{where}: cannot read ({err})")
            continue
        lines = [" ".join(line.split()) for line in text.splitlines()
                 if line.strip() and not line.lstrip().startswith("#")]
        for number, line in enumerate(lines, 1):
            for word in LOCK_PAM_BANNED:
                if word in line:
                    problems.append(f"{where}: rule {number} names {word} (LP8)")
            words = line.split()
            if len(words) > 2 and words[1] in ("include", "substack") and words[2] != "system-auth":
                problems.append(f"{where}: rule {number} includes {words[2]}, not system-auth (LP8)")
        if lines != expected:
            problems.append(f"{where}: the rules are not LP8's {len(expected)} lines: {lines}")
    return problems


@check("pam", "No empty passwords (A2-23), no new password under twelve characters, account lockout (ADR-0090), the lock's stacks (LP8)")
def check_pam():
    r = Result()
    for problem in nullok_problems() + pwquality_problems() + faillock_problems() + lock_pam_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 8. polkit, il lato codice — il subject deve essere il CHIAMANTE
# --------------------------------------------------------------------------- #

@check("polkit-subject", "Il subject polkit identifica il chiamante, e i default sono raggiungibili")
def check_polkit_subject():
    r = Result()

    for p in rust_files():
        txt = read(p)
        if "check_authorization" not in txt and "PolicyKitAuthority" not in txt:
            continue
        for i, line in enumerate(txt.split("\n"), 1):
            code = line.split("//")[0]
            if "peer_creds" in code or "peer_credentials" in code:
                r.fail(f"{rel(p)}:{i} peer_creds() in un percorso polkit: restituisce le "
                       f"credenziali del bus all'altro capo del socket, MAI quelle del "
                       f"chiamante. Usa PolkitSubject::system_bus_name(sender)")
            if re.search(r"unix_user_id\(\)\s*==\s*Some\(0\)", code):
                r.fail(f"{rel(p)}:{i} short-circuit su uid 0 derivato dal socket peer: "
                       f"se il bus gira come root questo autorizza chiunque")

    # un default auth_* è irraggiungibile se il sito di chiamata vieta l'interazione
    declared = {}
    for pol in walk(ROOT, ".policy"):
        txt = read(pol)
        for m in re.finditer(r'<action id="([^"]+)">(.*?)</action>', txt, re.S):
            act = re.search(r"<allow_active>(\w+)</allow_active>", m.group(2))
            if act:
                declared[m.group(1)] = act.group(1)

    for p in rust_files():
        for i, line in enumerate(read(p).split("\n"), 1):
            m = re.search(r'"(os\.athanor\.[a-z0-9_.]+)"\s*,\s*(true|false)\s*\)', line)
            if not m:
                continue
            action, interactive = m.group(1), m.group(2) == "true"
            default = declared.get(action)
            if default and default.startswith("auth_") and not interactive:
                r.fail(f"{rel(p)}:{i} {action} è dichiarata {default} ma qui "
                       f"allow_user_interaction=false: polkit non può mostrare il dialogo, "
                       f"quindi l'autorizzazione fallisce sempre. Passa true, o abbassa il "
                       f"default sapendo cosa comporta")

    return r


# --------------------------------------------------------------------------- #
# 9. spec RPM — la sorgente di `install` deve esistere da dove gira %install
# --------------------------------------------------------------------------- #

# Packages that dedicated workflows build, outside the DAG (forge/scripts/dag_orchestrator.py).
EXTERNAL_PACKAGES = {"kernel", "kernel-forge"}


def manifest_spec_problems(root=None):
    """Every custom_* entry of packages.json has a package directory under forge/specs."""
    root = root or ROOT
    manifest = root / "forge" / "config" / "packages.json"
    specs = root / "forge" / "specs"
    if not manifest.is_file():
        return []
    problems = []
    for key, pkgs in json.loads(read(manifest)).items():
        if not key.startswith("custom_"):
            continue
        for pkg in pkgs:
            if pkg in EXTERNAL_PACKAGES:
                continue
            if not ((specs / f"athanor-{pkg}").is_dir() or (specs / pkg).is_dir()):
                problems.append(f"{rel(manifest)}: {key} lists {pkg}, but forge/specs has neither "
                                f"athanor-{pkg} nor {pkg}")
    return problems


# Specs whose Release does not carry %{?athanor_content}: they follow an upstream release or the
# kernel's, and still raise Release by hand on a payload change (forge/AGENTS.md).
RELEASE_BY_HAND = {
    "forge/specs/azoth/microvm/azoth-microvm.spec",
    "forge/specs/cosmic-comp/cosmic-comp.spec",
    "forge/specs/greenboot-rs/greenboot-rs.spec",
    "forge/specs/polkit/polkit.spec",
}
# The RELEASE_BY_HAND spec whose Version-Release is the kernel's NVR: build.sh defines kversion
# and krelease from nvr.sh, which reads the kernel pins, so the spec cannot raise it, and the
# kernel pipeline already rebuilds and tests a change to microvm/ (build-inputs.py hashes its
# spec and kernel-local, kernel_unchanged.py checks the whole directory).
RELEASE_FROM_KERNEL = {"forge/specs/azoth/microvm/azoth-microvm.spec"}
# Directories under a RELEASE_BY_HAND spec's directory that no Source line and no section of the
# spec reads, so a change to them leaves the package as it is and raises no Release (#1202). The
# spec file itself is never listed: a comment-only edit of a spec raises Release too.
RELEASE_UNPACKAGED = {
    "forge/specs/greenboot-rs/tests/": "unittest suites of the patches, run by just check, not packaged",
    "forge/specs/polkit/tests/": "unittest suites of the agent helper, run by just check, not packaged",
}


def load_module(path):
    """The Python module at PATH, relative to the repository, loaded by its file path: the
    directories of scripts/ci and forge/scripts are not packages verify.py can import."""
    spec = importlib.util.spec_from_file_location(Path(path).stem, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def unraised_releases(root, base):
    """The RELEASE_BY_HAND specs of ROOT (but RELEASE_FROM_KERNEL) whose directory differs from
    revision BASE outside RELEASE_UNPACKAGED while their Version-Release did not rise (#1202).

    The working tree is compared with BASE, a two-tree diff as scripts/ci/changes.py makes: in
    pr.yml BASE is the first parent of the checked-out merge, so the diff is the pull request's.
    A spec absent at BASE is new, one absent from the tree is removed, and one whose base Release
    still carried %{?athanor_content} had no hand Release to raise: none of them is compared.
    Raises ValueError when BASE is not a commit or git cannot diff it.
    """
    rpms = load_module("forge/scripts/check_image_rpms.py")
    classes = load_module("scripts/ci/release_class.py")

    def git(*args):
        return subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)

    if git("rev-parse", "--verify", "--quiet", f"{base}^{{commit}}").returncode != 0:
        raise ValueError(f"{base} is not a commit of {root}")
    diff = git("diff", "--name-only", "--no-renames", "-z", base, "--")
    if diff.returncode != 0:
        raise ValueError(f"git diff {base} failed: {diff.stderr.strip()}")
    changed = [p for p in diff.stdout.split("\0") if p]

    problems = []
    for spec in sorted(RELEASE_BY_HAND - RELEASE_FROM_KERNEL):
        directory = spec.rsplit("/", 1)[0] + "/"
        content = [p for p in changed if p.startswith(directory)
                   and not any(p.startswith(d) for d in RELEASE_UNPACKAGED)]
        if not content or not (Path(root) / spec).is_file():
            continue
        listed = git("ls-tree", "-z", "--name-only", base, "--", spec)
        if listed.returncode != 0:
            problems.append(f"{spec}: cannot list it at {base}: {listed.stderr.strip()}")
            continue
        if not listed.stdout:
            continue
        before = git("show", f"{base}:{spec}")
        if before.returncode != 0:
            problems.append(f"{spec}: cannot read it at {base}: {before.stderr.strip()}")
            continue
        if re.search(r"^Release:.*%\{\?athanor_content\}", before.stdout, re.M):
            continue
        with tempfile.TemporaryDirectory() as tmp:
            old_spec = Path(tmp) / Path(spec).name
            old_spec.write_text(before.stdout, encoding="utf-8")
            try:
                old = rpms.read_spec(old_spec)[1]()
                new = rpms.read_spec(Path(root) / spec)[1]()
                raised = classes.evrcmp(new, old) > 0
            except (KeyError, ValueError, classes.ClassError) as e:
                problems.append(f"{spec}: cannot read its Version-Release: {e}")
                continue
        if not raised:
            problems.append(
                f"{spec}: {', '.join(c[len(directory):] for c in content)} changed but "
                f"Version-Release {new} is not higher than {old} at {base}: raise Release and add "
                f"a %changelog entry, or add a path no build reads to RELEASE_UNPACKAGED in "
                f"scripts/verify.py, only with its reason")
    return problems


@check("specs", "Le spec installano da percorsi che esistono dalla loro working directory")
def check_specs():
    r = Result()
    for problem in manifest_spec_problems():
        r.fail(problem)
    if BASE_REF is None:
        r.note("no base revision (--known-red-base REF): the Release raises of RELEASE_BY_HAND "
               "were not checked")
    else:
        try:
            for problem in unraised_releases(ROOT, BASE_REF):
                r.fail(problem)
        except ValueError as e:
            r.fail(f"Release raises against {BASE_REF}: {e}")
        else:
            commits = {
                subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--verify", "--quiet",
                                f"{ref}^{{commit}}"], capture_output=True, text=True).stdout
                for ref in (BASE_REF, "HEAD")
            }
            if len(commits) == 1:
                r.note(f"the base {BASE_REF} is the checked-out commit, so only uncommitted edits were compared for "
                       "Release raises: pass the pull request's base, e.g. "
                       "`just check-static origin/iso-v0`")
    import shlex

    def sections(text):
        """Divide una spec nelle sue sezioni %prep / %build / %install / ..."""
        out, cur = {}, None
        for line in text.split("\n"):
            m = re.match(r"^%(\w+)\b", line)
            if m and m.group(1) in ("prep", "build", "install", "files", "changelog",
                                    "post", "preun", "postun", "description", "package"):
                cur = m.group(1)
                out.setdefault(cur, [])
                continue
            if cur:
                out[cur].append(line)
        return {k: "\n".join(v) for k, v in out.items()}

    def install_source(line):
        """La sorgente di un `install SRC DEST`, o None se la riga non lo è."""
        try:
            toks = shlex.split(line)
        except ValueError:
            return None
        if not toks or toks[0] != "install":
            return None
        pos, skip = [], False
        for tok in toks[1:]:
            if skip:
                skip = False
                continue
            if tok in ("-m", "-o", "-g", "-t", "--mode", "--owner", "--group"):
                skip = True
                continue
            if tok.startswith("-"):
                continue
            pos.append(tok)
        if len(pos) < 2 or "buildroot" not in pos[-1]:
            return None
        return pos[-2]

    for spec in walk(ROOT, ".spec"):
        text = read(spec)
        release = re.search(r"^Release:[ \t]*(\S+)", text, re.M)
        by_hand = rel(spec) in RELEASE_BY_HAND
        if rel(spec).startswith("forge/specs/") and release and by_hand == ("%{?athanor_content}" in release.group(1)):
            r.fail(f"{rel(spec)}: Release {release.group(1)} " + (
                "carries %{?athanor_content}: drop the spec from RELEASE_BY_HAND" if by_hand else
                "lacks %{?athanor_content}: write <N>%{?athanor_content}%{?dist}, or list the spec in "
                "RELEASE_BY_HAND and raise its Release by hand (forge/AGENTS.md)"))
        sec = sections(text)
        prep = sec.get("prep", "")
        # senza %setup/%autosetup rpmbuild non entra in una sottodirectory:
        # %install parte da %{_builddir}, che per questo forge è la radice del repo
        stub_prep = not re.search(r"^\s*%(auto)?setup\b", prep, re.M)

        for i, line in enumerate(text.split("\n"), 1):
            stripped = line.strip()

            # residuo del generatore: crea un file VUOTO che install poi spedisce
            m = re.match(r"^mkdir -p \$\(dirname (\S+)\) && touch \1$", stripped)
            if m:
                r.fail(f"{rel(spec)}:{i} `touch {m.group(1)}` crea un file vuoto nella "
                       f"working directory: se poi viene installato, spedisci 0 byte")
                continue

            if not stub_prep:
                continue
            src = install_source(stripped)
            if src is None:
                continue
            if src.startswith(("%", "$", "/", "~")) or "/" in src:
                continue
            r.fail(f"{rel(spec)}:{i} install da nome nudo `{src}`: %prep non fa "
                   f"%setup, quindi %install gira dalla radice del build e quel file "
                   f"non è lì. Usa il percorso completo dalla radice del repo")

    return r


# --------------------------------------------------------------------------- #
# 10. boundary — COSMIC stays behind the compositor client (doc_shell.md, SH2)
# --------------------------------------------------------------------------- #

# SH2: the compositor client is the protocol boundary; the theme tool generates COSMIC's
# theme files. Nothing else may depend on COSMIC.
COSMIC_ALLOWED = (
    "system/athanor-sulphur-client/",
    "forge/tools/calmo-cosmic-theme/",
    # The own compositor reads cosmic-comp's keyboard layouts (doc_own_stack.md decision 27).
    "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/xkb_config.rs",
    # It reads and writes cosmic-comp's colour filter and reads its accessibility_zoom, so both
    # sessions share one state, until cosmic-comp's session leaves the greeter
    # (doc_own_stack.md decision 28, doc_accessibility.md AX10).
    "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/screen_filter.rs",
    # It reads the theme of the focus outline, read-only, until the store moves with the
    # compositor client (doc_own_stack.md decision 31).
    "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/focus_theme.rs",
    # Its volume, brightness and screen-reader keys call cosmic-settings-daemon until
    # athanor-osd and athanor-a11y take them (doc_own_stack.md decision 29).
    "forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/src/bindings/cosmic.rs",
)
BOUNDARY_DIRS = ("system", "forge/specs", "forge/tools")
COSMIC_APPLICATION_ID = re.compile(r"com\.system76\.\w+\.desktop\b|app-athanor-com\.system76\.\w+\*@\.service\b")
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "dev_dependencies",
                     "build-dependencies", "build_dependencies")


def cosmic_dependencies(manifest):
    """The crate names of a parsed Cargo manifest's dependencies that are libcosmic or
    cosmic-*. A renamed dependency (`package = "cosmic-..."`) is found by its crate name."""
    tables = [manifest.get(name, {}) for name in DEPENDENCY_TABLES]
    tables += [target.get(name, {}) for target in manifest.get("target", {}).values()
               for name in DEPENDENCY_TABLES]
    tables.append(manifest.get("workspace", {}).get("dependencies", {}))
    found = []
    for table in tables:
        for key, value in table.items():
            crate = value.get("package", key) if isinstance(value, dict) else key
            if crate == "libcosmic" or crate.startswith("cosmic-"):
                found.append(crate)
    return found


def boundary_problems(root):
    """What crosses the COSMIC boundary under root: a crate that depends on COSMIC, or Rust
    code that names a com.system76 configuration, outside COSMIC_ALLOWED."""
    root = Path(root)
    problems = []
    for base in BOUNDARY_DIRS:
        for path in walk(root / base, ".toml"):
            relative = path.relative_to(root).as_posix()
            if path.name != "Cargo.toml" or relative.startswith(COSMIC_ALLOWED):
                continue
            try:
                manifest = tomllib.loads(read(path))
            except tomllib.TOMLDecodeError as error:
                problems.append(f"{relative}: not valid TOML ({error})")
                continue
            for crate in cosmic_dependencies(manifest):
                problems.append(f"{relative} depends on {crate}: only athanor-sulphur-client "
                                f"may depend on COSMIC")
        for path in walk(root / base, ".rs"):
            relative = path.relative_to(root).as_posix()
            if relative.startswith(COSMIC_ALLOWED):
                continue
            for i, line in enumerate(read(path).split("\n"), 1):
                # A desktop id names an application, not a configuration (the favourites'
                # successors of COSMIC's former default apps, doc_bar.md BR7), and so does the
                # family of units an allow-list line names for it.
                if "com.system76" in COSMIC_APPLICATION_ID.sub("", line.split("//")[0]):
                    problems.append(f"{relative}:{i} names a com.system76 configuration: read it "
                                    f"through athanor-sulphur-client")
    return problems


@check("boundary", "Only the compositor client depends on COSMIC or reads its configuration")
def check_boundary():
    r = Result()
    for problem in boundary_problems(ROOT):
        r.fail(problem)
    return r


# A confined process that renders on the GPU through /dev/dri needs, on the -nvidia images,
# the NVIDIA driver's nodes and /sys/module/nvidia as well, or GTK falls back to software
# rendering (#535). A client whose unit sets GSK_RENDERER=cairo never renders on the GPU.
NVIDIA_NODES_FN = "fn nvidia_device_nodes("
DRM_GRANTS = ('"/dev/dri"', "DRM_DEVICE_DIR")
NVIDIA_GRANT = 'nvidia_device_nodes(Path::new("/dev"))'
CAIRO_RENDERER = "GSK_RENDERER=cairo"
DRM_BIND = re.compile(r"--dev-bind(?:-try)?\s+/dev/dri\s")
NVIDIA_BINDS = ("--ro-bind-try /sys/module/nvidia /sys/module/nvidia",
                "--ro-bind /usr/bin/true /usr/bin/nvidia-modprobe")
USR_BIND = "--ro-bind /usr /usr"
# A bind whose source is under /dev, other than a device bind, makes bwrap remount
# the filesystem under /dev read-only, which SELinux refuses xdm_t: the greeter never starts.
DEV_SOURCE_BIND = re.compile(r"--(?:ro-)?bind(?:-try)?\s+[\"']?/dev(?:[/\"'\s]|$)")


def uncommented(text, marker):
    """The lines of text that are not comments starting with marker."""
    return "\n".join(line for line in text.splitlines()
                     if not line.lstrip().startswith(marker))


def nvidia_nodes_function(text):
    """fn nvidia_device_nodes in a Rust source, up to its closing brace at column 0, or None."""
    start = text.find(NVIDIA_NODES_FN)
    if start < 0:
        return None
    end = text.find("\n}\n", start)
    return text[start:] if end < 0 else text[start:end + 2]


def renders_with_cairo(source):
    """Whether a unit of the crate holding source pins GTK to the cairo renderer."""
    for parent in source.parents:
        if (parent / "Cargo.toml").is_file():
            return any(CAIRO_RENDERER in read(unit)
                       for unit in walk(parent / "data", ".service"))
    return False


def wrapper_problems(relative, text):
    """A bwrap wrapper that binds /dev/dri binds the NVIDIA module and masks nvidia-modprobe,
    passes its extra arguments after /usr, so that the mask lands on top of it, and binds
    nothing from /dev but devices."""
    code = uncommented(text, "#")
    if not DRM_BIND.search(code):
        return []
    problems = [f"{relative}: binds /dev/dri but lacks '{bind}'"
                for bind in NVIDIA_BINDS if bind not in code]
    problems += [f"{relative}: '{match.group(0).strip()}' binds from /dev without "
                 f"--dev-bind: SELinux refuses xdm_t the read-only remount under /dev"
                 for match in DEV_SOURCE_BIND.finditer(code)]
    invocation = code[code.find("exec bwrap"):]
    if '"$@"' in invocation and invocation.find('"$@"') < invocation.find(USR_BIND):
        problems.append(f"{relative}: passes \"$@\" before '{USR_BIND}', which would cover "
                        f"the nvidia-modprobe mask")
    return problems


def gpu_grant_problems(root):
    """A Landlock ruleset that grants the DRM nodes grants the NVIDIA nodes too, through
    copies of nvidia_device_nodes that are all the same, unless the client renders with
    cairo; a bwrap wrapper that binds /dev/dri binds /sys/module/nvidia, which the driver
    reads to learn its module is loaded, and masks the setuid nvidia-modprobe, which
    no-new-privileges refuses (#535)."""
    root = Path(root)
    problems, copies = [], {}
    for base in RS_DIRS:
        for path in sorted(walk(root / base, "")):
            relative = path.relative_to(root).as_posix()
            if path.suffix == ".rs":
                code = uncommented(read(path), "//")
                if (any(grant in code for grant in DRM_GRANTS) and NVIDIA_GRANT not in code
                        and not renders_with_cairo(path)):
                    problems.append(f"{relative}: grants the DRM nodes to Landlock but not the "
                                    f"NVIDIA nodes ({NVIDIA_GRANT})")
                body = nvidia_nodes_function(code)
                if body is not None:
                    copies[relative] = body
            elif not path.suffix and path.is_file():
                text = read(path)
                if text.startswith("#!"):
                    problems += wrapper_problems(relative, text)
    if len(set(copies.values())) > 1:
        problems.append("the copies of nvidia_device_nodes differ: " + ", ".join(sorted(copies)))
    return problems


@check("gpu-grants", "Confined GPU clients reach the NVIDIA driver as well as /dev/dri (#535)")
def check_gpu_grants():
    r = Result()
    for problem in gpu_grant_problems(ROOT):
        r.fail(problem)
    return r


# landlock::PathFd opens a path following links, and a ruleset can name any descriptor: a
# write grant built from either widens to wherever a link put at the path points (#1052).
# athanor_unit::sandbox takes its write grants as dirs::Checked, which only athanor_unit::dirs
# builds. This check holds the rest: no crate but athanor-unit depends on the landlock crate,
# so every other one confines through athanor_unit::sandbox; no other Rust file names PathFd,
# under any name a `use ... as` gives it; and in athanor_unit's sandbox.rs, PathFd appears only
# in its import and as the parent of a read grant.
LANDLOCK_SANDBOX = "system/athanor-unit/src/sandbox.rs"
LANDLOCK_CRATE = "system/athanor-unit/Cargo.toml"
PATH_FD = re.compile(r"\bPathFd\b")
PATH_FD_READ = re.compile(r"PathBeneath::new\(\s*PathFd::new\([^()]*\)\??\s*,\s*AccessFs::from_read\([^()]*\)\s*,?\s*\)")
LANDLOCK_USE = re.compile(r"\buse\s+landlock::\{[^}]*\}\s*;")
# In sandbox.rs, outside its tests module: a rule's descriptor is `<name>.as_fd()`; a
# public function takes its grants only as these types, so a write grant is a dirs::Checked or
# a dirs::CheckedNode; and nothing there opens a descriptor or builds rules from paths.
PATH_BENEATH = re.compile(r"PathBeneath::new\(")
PATH_BENEATH_FD = re.compile(r"PathBeneath::new\(\s*\w+\.as_fd\(\)\s*,")
SANDBOX_PARAMETERS = {"&[&Path]", "&[Checked]", "&[CheckedNode]"}
SANDBOX_PUB_FN = re.compile(r"\bpub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)")
SANDBOX_OPENS = re.compile(r"\b(path_beneath_rules|File::|OpenOptions|OwnedFd|BorrowedFd|from_raw_fd|openat)\b")


def landlock_dependencies(manifest):
    """The dependency tables of a Cargo manifest that name the landlock crate, by its own
    name or as the `package` of a renamed one."""
    tables = {name: manifest.get(name, {}) for name in ("dependencies", "dev-dependencies", "build-dependencies")}
    for target, spec in manifest.get("target", {}).items():
        for name in ("dependencies", "dev-dependencies", "build-dependencies"):
            tables[f"target.{target}.{name}"] = spec.get(name, {})
    return [table for table, deps in tables.items()
            if any(name == "landlock" or (isinstance(dep, dict) and dep.get("package") == "landlock")
                   for name, dep in deps.items())]


def without_tests_module(text):
    """text without its `#[cfg(test)] mod tests { ... }`, matched at the start of a line and cut
    at its closing brace, so code before or after it is still read. A module whose braces do not
    balance leaves the whole text, which the tests' own descriptors then fail: closed, not open."""
    match = re.search(r"^#\[cfg\(test\)\]\s*mod\s+tests\s*\{", text, re.M)
    if not match:
        return text
    depth = 0
    for pos in range(match.end() - 1, len(text)):
        depth += {"{": 1, "}": -1}.get(text[pos], 0)
        if depth == 0:
            return text[:match.start()] + "\n" * text.count("\n", match.start(), pos + 1) + text[pos + 1:]
    return text


def sandbox_grant_problems(relative, code, read_grants):
    """What in athanor_unit's sandbox.rs, its tests left out, could grant a write by something
    other than a dirs::Checked or dirs::CheckedNode."""
    found = []
    def line(pos):
        return code.count("\n", 0, pos) + 1

    for match in PATH_BENEATH.finditer(code):
        if any(start <= match.start() < end for start, end in read_grants):
            continue
        # One built on PathFd is the PathFd finding below; this one is any other descriptor.
        if not PATH_BENEATH_FD.match(code, match.start()) and not re.match(r"\s*PathFd\b", code[match.end():]):
            found.append(f"{relative}:{line(match.start())}: a rule whose descriptor is not "
                         f"`<name>.as_fd()`; a write grant is a dirs::Checked descriptor")
    for match in SANDBOX_PUB_FN.finditer(code):
        for parameter in filter(None, (p.strip() for p in match.group(2).split(","))):
            kind = re.sub(r"\s+", "", parameter.partition(":")[2])
            if kind not in SANDBOX_PARAMETERS:
                found.append(f"{relative}:{line(match.start())}: {match.group(1)} takes "
                             f"`{parameter}`; a grant is one of {', '.join(sorted(SANDBOX_PARAMETERS))}")
    for match in SANDBOX_OPENS.finditer(code):
        found.append(f"{relative}:{line(match.start())}: {match.group(1)} opens a descriptor or "
                     f"builds rules from paths; grants come from athanor_unit::dirs")
    return found


def landlock_fd_problems(root):
    """No crate but athanor-unit depends on landlock; no Rust file names PathFd but
    athanor_unit's sandbox.rs, and that only in its import and as the parent of a read grant.
    So a Landlock write grant is never built from a path."""
    root = Path(root)
    problems = []
    # Every file git would commit, not RS_DIRS: a crate under forge/tools or fuzz links landlock
    # as well as one under system, and git-ignored trees (.scratch, worktrees) are not the code.
    listed = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        capture_output=True, text=True)
    if listed.returncode:
        return [f"cannot list the repository files: {listed.stderr.strip()}"]
    files = sorted({name for name in listed.stdout.split("\0") if name and (root / name).is_file()})
    for relative in (name for name in files if name.rsplit("/", 1)[-1] == "Cargo.toml"):
        path = root / relative
        if relative == LANDLOCK_CRATE:
            continue
        try:
            manifest = tomllib.loads(read(path))
        except tomllib.TOMLDecodeError as err:
            problems.append(f"{relative}: not valid TOML: {err}")
            continue
        for table in landlock_dependencies(manifest):
            problems.append(f"{relative}: [{table}] names the landlock crate; confine through "
                            f"athanor_unit::sandbox, whose grants are dirs::Checked")
    for relative in (name for name in files if name.endswith(".rs")):
        path = root / relative
        text = read(path)
        allowed = []
        if relative == LANDLOCK_SANDBOX:
            allowed = [match.span() for match in PATH_FD_READ.finditer(text)]
            allowed += [match.span() for match in LANDLOCK_USE.finditer(text)
                        if not re.search(r"\bPathFd\s+as\b", match.group())]
        if relative == LANDLOCK_SANDBOX:
            problems += sandbox_grant_problems(relative, without_tests_module(text), allowed)
        for match in PATH_FD.finditer(text):
            if any(start <= match.start() < end for start, end in allowed):
                continue
            line = text.count("\n", 0, match.start()) + 1
            if relative == LANDLOCK_SANDBOX:
                problems.append(f"{relative}:{line}: PathFd other than the parent of a read "
                                f"grant; a write grant is a dirs::Checked descriptor")
            else:
                problems.append(f"{relative}:{line}: landlock's PathFd grants by path, following "
                                f"links; grant by an athanor_unit::dirs::Checked descriptor")
    return problems


@check("landlock-fd", "Landlock write grants go by a checked descriptor, never by path (#1052)")
def check_landlock_fd():
    r = Result()
    for problem in landlock_fd_problems(ROOT):
        r.fail(problem)
    return r


# Placeholders the unit tests use for an owner: they name no real registry namespace.
PLACEHOLDER_OWNERS = {"owner", "o"}
# Where the pipeline names its images.
REGISTRY_DIRS = (".github/workflows", "scripts", "forge/scripts")
REGISTRY_FILES = ("Justfile", "forge/Justfile", "system/Justfile", "system/Containerfile", "flake.nix")
# The file types that name images: workflows, scripts, recipes, container builds and Nix.
REGISTRY_SUFFIXES = {".yml", ".yaml", ".sh", ".py", ".just", ".nix"}
REGISTRY_NAMES = {"Justfile", "Containerfile"}


def literal_owners(text):
    """The hard-coded ghcr.io owners in text: ghcr.io/ followed by a name, not by a variable."""
    found = re.findall(r"ghcr\.io/([A-Za-z0-9][A-Za-z0-9_.-]*)", text)
    return [f"ghcr.io/{o}" for o in dict.fromkeys(found) if o not in PLACEHOLDER_OWNERS]


def registry_problems(root):
    """Every image the pipeline names comes from REGISTRY_HOST and the repository owner, each
    with a default, never from a literal owner (standing rule of 2026-09-10)."""
    root = Path(root)
    paths = [root / f for f in REGISTRY_FILES]
    for base in REGISTRY_DIRS:
        paths += walk(root / base, "")
    problems = []
    for path in sorted(paths):
        relative = path.relative_to(root)
        # Unit test fixtures need a literal owner to prove the check reports one.
        if (not path.is_file() or relative.parts[:2] == ("scripts", "tests")
                or (path.suffix not in REGISTRY_SUFFIXES and path.name not in REGISTRY_NAMES)):
            continue
        for i, line in enumerate(read(path).split("\n"), 1):
            for owner in literal_owners(line):
                problems.append(f"{relative.as_posix()}:{i}: literal registry owner {owner}; use "
                                f"REGISTRY_HOST and the repository owner, each with a default")
    return problems


@check("registry", "Images come from REGISTRY_HOST and the repository owner, never a literal owner")
def check_registry():
    r = Result()
    for problem in registry_problems(ROOT):
        r.fail(problem)
    return r


# PL8: a tool runs from the nixpkgs revision of flake.lock (`nix shell --inputs-from <checkout>
# nixpkgs#<tool>`), never from whatever a bare `nixpkgs#` resolves to at run time (F12, #671).
# --inputs-from pins only the bare `nixpkgs#` of its own nix command: `nixpkgs/<ref>#`,
# `nixpkgs-<alias>#`, `nixpkgs?<query>#`, `flake:nixpkgs#` and `github:NixOS/nixpkgs[/<ref>]#`
# name a source of their own.
PINNING_DIRS = REGISTRY_DIRS + (".github/actions", "system")
NIXPKGS_REF = re.compile(r"(?:github:|git\+https://github\.com/)nixos/nixpkgs[\w./?=&-]*#[\w.+-]+"
                         r"|(?:flake|indirect|registry):nixpkgs[\w./?=&-]*#[\w.+-]+"
                         r"|(?<![\w./:+-])nixpkgs[\w./?=&-]*#[\w.+-]+", re.I)
# Where one nix command ends: a shell separator, the -c/--command of nix shell, the next nix.
NIX_COMMAND_BREAK = re.compile(r";|&&|\|\|?|\s(?:-c|--command)\s|(?<![\w./-])nix(?=\s)")


def logical_lines(text):
    """(first line number, text) of each command, its backslash continuations joined; comment
    lines are dropped."""
    start, parts = None, []
    for i, line in enumerate(text.split("\n"), 1):
        if line.lstrip().startswith("#"):
            continue
        if start is None:
            start = i
        parts.append(line[:-1] if line.endswith("\\") else line)
        if not line.endswith("\\"):
            yield start, " ".join(parts)
            start, parts = None, []
    if parts:
        yield start, " ".join(parts)


def pinning_problems(root):
    """Every nixpkgs#<tool> a workflow, action, recipe or script runs is a bare nixpkgs#
    reference after --inputs-from in the same nix command, so nixpkgs is the locked input of
    the repository flake."""
    root = Path(root)
    paths = [root / f for f in REGISTRY_FILES]
    for base in PINNING_DIRS:
        paths += walk(root / base, "")
    problems = []
    for path in sorted(set(paths)):
        relative = path.relative_to(root)
        # Unit test fixtures need an unpinned reference to prove the check reports one.
        if (not path.is_file() or relative.parts[1:2] == ("tests",)
                or (path.suffix not in REGISTRY_SUFFIXES and path.name not in REGISTRY_NAMES)):
            continue
        for i, line in logical_lines(read(path)):
            for ref in NIXPKGS_REF.finditer(line):
                command = NIX_COMMAND_BREAK.split(line[:ref.start()])[-1]
                if not ref.group().startswith("nixpkgs#") or "--inputs-from " not in command:
                    problems.append(f"{relative.as_posix()}:{i}: {ref.group()} is not pinned; run "
                                    f"nixpkgs#<tool> with --inputs-from <checkout> in the same "
                                    f"nix command, so nixpkgs is flake.lock's")
    return problems


@check("pinning", "Tools come from the locked flake, never an unpinned nixpkgs# reference (PL8)")
def check_pinning():
    r = Result()
    for problem in pinning_problems(ROOT):
        r.fail(problem)
    return r


# Golden rules 2 and 3 of docs/architecture/doc_forge_development_guide.md.
SPEC_SECTION = re.compile(
    r"^%(package|description|prep|generate_buildrequires|conf|build|install|check|clean|files|"
    r"changelog|pre|post|preun|postun|pretrans|posttrans|preuntrans|postuntrans|verify|"
    r"(?:trans)?filetrigger(?:in|un|postun)|trigger(?:prein|in|un|postun))\b")
SCRIPTLETS = {"pre", "post", "preun", "postun", "pretrans", "posttrans", "preuntrans",
              "postuntrans", "verify", "triggerprein", "triggerin", "triggerun", "triggerpostun",
              "filetriggerin", "filetriggerun", "filetriggerpostun", "transfiletriggerin",
              "transfiletriggerun", "transfiletriggerpostun"}
# /usr and /etc, spelled out or through the macros that expand below them.
IMAGE_PATH = (r"(?:/usr|/etc)\b|%\{?_(?:sysconfdir|prefix|exec_prefix|bindir|sbindir|libdir|"
              r"libexecdir|datadir|datarootdir|includedir|mandir|docdir|unitdir|userunitdir|"
              r"presetdir|userpresetdir|tmpfilesdir|sysusersdir|udevrulesdir|modprobedir|"
              r"sysctldir|environmentdir)\b")
MUTATION = re.compile(r"(?:^|[\s;&|(])(?:cp|mv|install|chmod|chown|chgrp|ln|rm|mkdir|touch|tee|"
                      r"truncate|rsync|sed\s+-i\S*)\s[^;&|]*?(?:" + IMAGE_PATH + r")"
                      r"|>>?\s*(?:" + IMAGE_PATH + r")")
WEAKENING = re.compile(r"\b(?:repo_)?gpgcheck\s*=\s*(?:0|false|no)\b|--nogpgcheck\b"
                       r"|%undefine\s+_(?:fortify_source|hardened_build)\b"
                       r"|%(?:define|global)\s+_(?:fortify_level|hardened_build)\s+0\b"
                       r"|-U_FORTIFY_SOURCE\b|-D_FORTIFY_SOURCE=0\b|-fno-stack-protector\b"
                       r"|-fcf-protection=none\b|-z\s*(?:norelro|execstack)\b|-no-pie\b|-fno-PIE\b")


def forge_rule_problems(root):
    """Rule 2: no scriptlet writes under /usr or /etc, which belong to the image (%install and
    tmpfiles do). Rule 3: no spec turns off a signature check or a hardening flag."""
    root = Path(root)
    problems = []
    for path in sorted(root.glob("forge/specs/*/*.spec")):
        relative = path.relative_to(root).as_posix()
        section = None
        for i, line in enumerate(read(path).split("\n"), 1):
            header = SPEC_SECTION.match(line)
            if header:
                section = header.group(1)
            if section == "changelog":
                break
            if header or line.lstrip().startswith("#"):
                continue
            if section in SCRIPTLETS and MUTATION.search(line):
                problems.append(f"{relative}:{i}: rule 2, %{section} writes under /usr or /etc: "
                                f"install the file in %install or create it with tmpfiles.d")
            if WEAKENING.search(line):
                problems.append(f"{relative}:{i}: rule 3, turns off a signature check or a "
                                f"hardening flag")
    return problems


@check("forge-rules", "Specs keep golden rules 2 and 3 of the forge guide")
def check_forge_rules():
    r = Result()
    for problem in forge_rule_problems(ROOT):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# licence: one licence for our own code (maintainer decision A2-3, 2026-10-05)
# --------------------------------------------------------------------------- #

OWN_LICENCE = "GPL-3.0-or-later"
# Specs that package somebody else's software, by repository path: they declare
# upstream's licence, which must be an expression of known SPDX identifiers. Every
# other spec, every Cargo.toml and every nfpm `license:` field is our own code and
# must say OWN_LICENCE.
UPSTREAM_SPECS = {
    "forge/specs/athanor-cliphist/athanor-cliphist.spec",
    "forge/specs/athanor-dart-sass/athanor-dart-sass.spec",
    "forge/specs/athanor-matugen/athanor-matugen.spec",
    "forge/specs/athanor-tetragon/athanor-tetragon.spec",
    "forge/specs/azoth/microvm/azoth-microvm.spec",
    "forge/specs/cosmic-comp/cosmic-comp.spec",
    "forge/specs/greenboot-rs/greenboot-rs.spec",
    "forge/specs/polkit/polkit.spec",
}
# Own code that is declared under a different licence than OWN_LICENCE, by repository
# path: path -> (licence, reason, dependency). An entry is a decision, not a tolerance: the
# declared licence must match exactly, so the entry cannot hide a drift in either direction.
# `dependency` names the crate whose licence forces the exception, and the entry holds only
# while the manifest depends on it; None marks an exception the maintainer chose, whose reason
# names the decision record.
OWN_LICENCE_EXCEPTIONS = {
    # greetd_ipc is GPL-3.0-only; the greeter binary links it, so the binary can only be
    # distributed under GPL-3.0-only terms (maintainer decision, audit 4, LIC-N02).
    "forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/Cargo.toml":
        ("GPL-3.0-only", "links greetd_ipc (GPL-3.0-only)", "greetd_ipc"),
    "forge/specs/athanor-greeter-ui/athanor-greeter-ui.spec":
        ("GPL-3.0-only", "links greetd_ipc (GPL-3.0-only)", "greetd_ipc"),
    # The toolkit is permissive so that applications outside Athanor can use it (ADR-0121,
    # doc_own_stack.md OS7).
    "system/athanor-sal/Cargo.toml":
        ("MIT OR Apache-2.0", "permissive toolkit, ADR-0121", None),
    "system/athanor-sal/gallery/Cargo.toml":
        ("MIT OR Apache-2.0", "the toolkit's gallery, ADR-0126", None),
}
# The crate whose dependency forces the greeter's exception above; a crate that depends on it
# must be in the table with it as its dependency.
GPL3_ONLY_DEPENDENCY = "greetd_ipc"
# Files that carry packaging metadata outside Cargo.toml and *.spec.
NFPM_FILES = ["flake.nix"]
# The SPDX identifiers the repository actually uses. A new one is added here on purpose.
SPDX_IDS = {
    "0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "BSL-1.0", "CC0-1.0",
    "GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0-only", "GPL-3.0-or-later", "ISC",
    "LGPL-2.0-or-later", "LGPL-2.1-or-later", "LGPL-3.0-or-later", "MIT", "MPL-2.0", "Unicode-3.0",
    "Unicode-DFS-2016", "Unlicense", "Zlib",
}
SPDX_EXCEPTIONS = {"LLVM-exception", "Linux-syscall-note"}


def spdx_problem(expr):
    """Why `expr` is not an SPDX expression of known identifiers, or None."""
    tokens = re.findall(r"\(|\)|[^\s()]+", expr)
    if not tokens:
        return "empty"
    depth, expect = 0, "id"  # id: identifier or "(" ; exc: exception id ; op: operator or ")"
    for tok in tokens:
        if tok == "(":
            if expect != "id":
                return "unexpected '('"
            depth += 1
        elif tok == ")":
            depth -= 1
            if expect != "op" or depth < 0:
                return "unbalanced ')'"
        elif expect == "id":
            if tok not in SPDX_IDS:
                return f"'{tok}' is not a known SPDX identifier"
            expect = "op"
        elif expect == "exc":
            if tok not in SPDX_EXCEPTIONS:
                return f"'{tok}' is not a known SPDX exception"
            expect = "op"
        elif tok in ("AND", "OR"):
            expect = "id"
        elif tok == "WITH":
            expect = "exc"
        else:
            return f"expected AND/OR/WITH, found '{tok}'"
    if expect != "op" or depth:
        return "incomplete expression"
    return None


def licence_problems(root=None):
    root = Path(root or ROOT)
    out = []
    git = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard"],
        capture_output=True, text=True)
    if git.returncode:
        return [f"cannot list the repository files: {git.stderr.strip()}"]
    files = [f for f in git.stdout.split("\n") if (root / f).is_file()]
    for f in sorted(files):
        name = f.rsplit("/", 1)[-1]
        if name == "Cargo.toml":
            try:
                pkg = tomllib.loads(read(root / f)).get("package")
            except tomllib.TOMLDecodeError as e:
                out.append(f"{f}: unreadable manifest ({e})")
                continue
            if pkg is None:
                continue
            lic = pkg.get("license")
            want = VENDORED_CRATES.get(f.rsplit("/", 1)[0]) or OWN_LICENCE_EXCEPTIONS.get(f, (OWN_LICENCE,))[0]
            if lic != want:
                out.append(f"{f}: license = {lic!r}, expected {want!r}")
        elif name.endswith(".spec"):
            m = re.search(r"^License:\s*(.*?)\s*$", read(root / f), re.M)
            if not m:
                out.append(f"{f}: no License: field")
            elif f in UPSTREAM_SPECS:
                why = spdx_problem(m.group(1))
                if why:
                    out.append(f"{f}: License: {m.group(1)} ({why})")
            elif m.group(1) != OWN_LICENCE_EXCEPTIONS.get(f, (OWN_LICENCE,))[0]:
                want = OWN_LICENCE_EXCEPTIONS.get(f, (OWN_LICENCE,))[0]
                out.append(f"{f}: License: {m.group(1)}, expected {want}")
        elif f in NFPM_FILES:
            for n, line in enumerate(read(root / f).split("\n"), 1):
                m = re.match(r"""^\s*license:\s*["']?([^"'\s]*)""", line)
                if m and m.group(1) != OWN_LICENCE:
                    out.append(f"{f}:{n}: nfpm license: {m.group(1)}, expected {OWN_LICENCE}")
    out += exception_problems(root, files)
    if not (root / "LICENSE").is_file():
        out.append("LICENSE: missing at the repository root")
    return out


def exception_problems(root, files):
    """OWN_LICENCE_EXCEPTIONS must match the dependency that justifies it, both ways; an
    exception without a dependency must not depend on GPL3_ONLY_DEPENDENCY either."""
    out = []
    dependents = set()
    for f in files:
        if f.rsplit("/", 1)[-1] != "Cargo.toml":
            continue
        try:
            manifest = tomllib.loads(read(root / f))
        except tomllib.TOMLDecodeError:
            continue  # reported as unreadable above
        if "package" not in manifest:
            continue
        if GPL3_ONLY_DEPENDENCY in manifest.get("dependencies", {}):
            dependents.add(f)
    for f in sorted(dependents):
        if OWN_LICENCE_EXCEPTIONS.get(f, (None, None, None))[2] != GPL3_ONLY_DEPENDENCY:
            out.append(f"{f}: depends on {GPL3_ONLY_DEPENDENCY} (GPL-3.0-only) and has no entry in OWN_LICENCE_EXCEPTIONS")
    for f, (_, reason, dependency) in sorted(OWN_LICENCE_EXCEPTIONS.items()):
        if not reason:
            out.append(f"{f}: OWN_LICENCE_EXCEPTIONS entry without a reason")
        if (dependency and f.endswith("Cargo.toml") and (root / f).is_file()
                and f not in dependents):
            out.append(f"{f}: OWN_LICENCE_EXCEPTIONS entry, but it no longer depends on {dependency}")
    return out


@check("licence", "own crates and specs declare GPL-3.0-or-later, upstream specs a valid SPDX expression")
def check_licence():
    r = Result()
    for problem in licence_problems():
        r.fail(problem)
    return r


CI_DOC = "docs/architecture/doc_ci.md"
# A workflow file name, bare or under .github/workflows/; not the tail of another path such as
# .github/actions/kvm/action.yml.
WORKFLOW_NAME = re.compile(r"(?<![\w./-])(?:\.github/workflows/)?([\w-][\w.-]*\.ya?ml)\b")
SECRET_OR_VAR = re.compile(r"\b(?:secrets|vars)\.([A-Za-z_][A-Za-z0-9_]*)")


def ci_problems(root):
    """doc_ci.md describes every workflow and names every secret and variable they read, and
    names no workflow that does not exist."""
    root = Path(root)
    doc = root / CI_DOC
    if not doc.is_file():
        return [f"{CI_DOC}: missing"]
    text = read(doc)
    workflows = sorted(p for p in (root / ".github/workflows").glob("*.y*ml")
                       if p.suffix in (".yml", ".yaml"))
    present = {p.name for p in workflows}
    named = set(WORKFLOW_NAME.findall(text))
    problems = [f".github/workflows/{name}: not described in {CI_DOC}"
                for name in sorted(present - named)]
    problems += [f"{CI_DOC}: names {name}, which is not in .github/workflows"
                 for name in sorted(named - present)]
    for path in workflows:
        seen = set()
        for i, line in enumerate(read(path).split("\n"), 1):
            for name in SECRET_OR_VAR.findall(line):
                if name not in seen and not re.search(rf"\b{name}\b", text):
                    seen.add(name)
                    problems.append(f"{path.relative_to(root).as_posix()}:{i}: {name} is not named in {CI_DOC}")
    return problems


@check("ci", "every workflow, secret and variable is described in docs/architecture/doc_ci.md")
def check_ci():
    r = Result()
    for problem in ci_problems(ROOT):
        r.fail(problem)
    return r


DECISION_FIELDS = ("id", "title", "date", "status", "issues", "areas")
DECISION_ID = r"[A-Z0-9][A-Za-z0-9-]*"
DECISION_STATUS = re.compile(
    rf"^(accepted|(?:superseded|amended) by {DECISION_ID}(?:, {DECISION_ID})*)$")


def decision_front_matter(text):
    """Front matter of a decision record as a dict of raw strings, or None."""
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None
    fields = {}
    for line in m.group(1).split("\n"):
        k, sep, v = line.partition(":")
        if sep:
            fields[k.strip()] = v.strip()
    return fields


def decision_problems(root=None):
    root = Path(root or ROOT)
    folder = root / "docs" / "decisions"
    out = []
    files = sorted(p for p in folder.glob("[0-9][0-9][0-9][0-9]-*.md"))
    records = {}
    for f in files:
        name = f.name
        fm = decision_front_matter(read(f))
        if fm is None:
            out.append(f"{name}: no front matter")
            continue
        for field in DECISION_FIELDS:
            if not fm.get(field) and field not in ("issues",):
                out.append(f"{name}: front matter lacks '{field}'")
            elif field == "issues" and field not in fm:
                out.append(f"{name}: front matter lacks 'issues'")
        id_ = fm.get("id", "")
        if id_ and not re.fullmatch(DECISION_ID, id_):
            out.append(f"{name}: malformed id '{id_}'")
        if id_ in records:
            out.append(f"{name}: id {id_} already used by {records[id_][0]}")
        elif id_:
            records[id_] = (name, fm.get("status", ""), fm.get("title", "").strip('"'))
        status = fm.get("status")
        if status and not DECISION_STATUS.match(status):
            out.append(f"{name}: status '{status}' is not accepted, superseded by <id> or amended by <id>")
        for h in ("Context", "Decision", "Consequences"):
            if f"\n## {h}\n" not in read(f):
                out.append(f"{name}: missing section '## {h}'")
    for id_, (name, status, _) in records.items():
        for target in re.findall(DECISION_ID, status.partition(" by ")[2]):
            if target not in records:
                out.append(f"{name}: status points to {target}, which has no record")
    readme = folder / "README.md"
    if not readme.is_file():
        out.append("docs/decisions/README.md: missing")
        return out
    listed = re.findall(
        r"^\| ([^|\s]+) \| (\d{4}) \| \[([^\]]*)\]\(([^)]+)\) \| ([^|]*?) \|",
        read(readme), re.M)
    index_files = [row[3] for row in listed]
    for f in files:
        if f.name not in index_files:
            out.append(f"README.md: index does not list {f.name}")
    for id_, num, title, n, status in listed:
        if not (folder / n).is_file():
            out.append(f"README.md: index lists {n}, which does not exist")
        elif id_ not in records or records[id_][0] != n:
            out.append(f"README.md: index row {id_} does not match {n}")
        elif title != records[id_][2]:
            out.append(f"README.md: index title of {id_} differs from {n}")
        elif status != records[id_][1]:
            out.append(f"README.md: index status of {id_} differs from {n}")
        elif not n.startswith(num + "-"):
            out.append(f"README.md: index number {num} does not match {n}")
    if len(index_files) != len(set(index_files)):
        out.append("README.md: index lists a file twice")
    return out


@check("decisions", "decision records have valid front matter, unique ids, a matching README index and existing targets")
def check_decisions():
    r = Result()
    for problem in decision_problems():
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# 11. services — every shipped service sets NoNewPrivileges or a capability allow-list
# --------------------------------------------------------------------------- #

# docs/architecture/doc_threat_model.md, TM8 (maintainer decision A2-9 (#151)). Each entry
# names a unit that does not meet the rule and why; an entry that no longer matches a failing
# unit fails the check, so the list cannot outlive its reasons.
SERVICE_EXEMPT = {}

TRUE_VALUES = {"1", "yes", "true", "on"}
# Settings that make a drop-in run a command of ours in the unit it extends.
EXEC_KEYS = {
    "ExecCondition",
    "ExecStartPre",
    "ExecStart",
    "ExecStartPost",
    "ExecReload",
    "ExecStop",
    "ExecStopPost",
}
# Settings that change the privileges the unit it extends runs with.
PRIVILEGE_KEYS = {
    "User",
    "Group",
    "SupplementaryGroups",
    "AmbientCapabilities",
    "CapabilityBoundingSet",
    "NoNewPrivileges",
}
# A heredoc: its opening line (whose `> file` names the target, before or after `<<`), its
# delimiter, and the body up to the delimiter's own line.
HEREDOC = re.compile(
    r"^([^\n]*<<-?[ \t]*['\"]?(\w+)['\"]?[^\n]*)\n(.*?)^[ \t]*\2[ \t]*$", re.M | re.S
)


def unit_directives(text):
    """(key, value) of every assignment in the [Service] section of a unit or drop-in."""
    section, found = None, []
    # A trailing backslash continues the line (systemd.syntax(7)): join before parsing.
    for line in re.sub(r"\\[ \t]*\r?\n", " ", text).splitlines():
        line = line.strip()
        if not line or line[0] in "#;":
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1]
        elif section == "Service" and "=" in line:
            key, value = line.split("=", 1)
            found.append((key.strip(), value.strip()))
    return found


# Capabilities that give root back to a process that keeps them (doc_threat_model.md).
ROOT_CAPS = {"CAP_SYS_ADMIN", "CAP_SYS_MODULE", "CAP_DAC_OVERRIDE", "CAP_SYS_PTRACE"}


def is_hardened(directives):
    """True if the merged directives end with NoNewPrivileges= true, or with a capability
    bounding set that is an explicit allow-list without CAP_SYS_ADMIN, CAP_SYS_MODULE,
    CAP_DAC_OVERRIDE or CAP_SYS_PTRACE.

    The bound follows systemd.exec(5): the first list sets it, a later list is merged by OR,
    a `~` list is removed by AND NOT, an empty assignment is the empty set and `~` alone the
    full one. Only an allow-list counts as a bound: a deny-list (`~CAP_X ...`) keeps every
    capability it does not name, CAP_SYS_MODULE or CAP_SYS_ADMIN among them unless it lists
    them, so it fails unless NoNewPrivileges= is also set."""
    no_new_privileges, allow, caps = False, None, set()
    for key, value in directives:
        if key == "NoNewPrivileges":
            no_new_privileges = value.lower() in TRUE_VALUES
        elif key == "CapabilityBoundingSet":
            named = set(value.lstrip("~").split())
            if not value or value == "~":
                allow, caps = not value, set()
            elif value.startswith("~"):
                caps = caps - named if allow else caps | named
                allow = bool(allow)
            elif allow is False:
                caps -= named
            else:
                allow, caps = True, caps | named
    return no_new_privileges or (allow is True and not caps & ROOT_CAPS)


def dropin_dirs(unit):
    """The drop-in directories that apply to a service, least specific first (systemd.unit(5)):
    `service.d`, each dash prefix (`foo-.service.d` for `foo-bar.service`), the template's
    (`foo@.service.d` for `foo@x.service`) and the unit's own."""
    stem = unit[: -len(".service")]
    parts = stem.split("-")
    dirs = ["service.d"]
    dirs += ["-".join(parts[:i]) + "-.service.d" for i in range(1, len(parts))]
    template, at, instance = stem.partition("@")
    if at and instance:
        dirs.append(f"{template}@.service.d")
    dirs.append(f"{unit}.d")
    return dirs


def service_units(files):
    """{id: (unit name, text)} for every systemd service unit among {relative path: text}:
    unit files, and units a package specification writes through a heredoc. Empty unit files
    (masks) and D-Bus activation files are not units that run anything."""
    units = {}
    for path, text in files.items():
        name = Path(path).name
        if path.endswith(".spec"):
            for m in HEREDOC.finditer(text):
                if "[Service]" in m.group(3):
                    target = re.search(r">\s*(\S+\.service)\b", m.group(1))
                    unit = (
                        Path(target.group(1)).name
                        if target
                        else f"heredoc-{m.group(2)}"
                    )
                    unit = unit.replace("%{name}", Path(path).stem)
                    units[f"{path}:{unit}"] = (unit, m.group(3))
        elif (
            name.endswith(".service") and text.strip() and "[D-BUS Service]" not in text
        ):
            units[path] = (name, text)
    return units


def service_problems(files):
    """Services among {relative path: text} that set neither NoNewPrivileges= nor an allow-list
    capability bound, after every drop-in that applies to them (dropin_dirs) is merged in file
    name order, a file in a more specific directory replacing one of the same name.

    A drop-in directory that is not the own directory of a unit in `files` (an upstream unit, a
    dash prefix, a template, `service.d`) counts as a service of its own when it adds a command
    (EXEC_KEYS) or changes the unit's privileges (PRIVILEGE_KEYS): that command or privilege is
    ours. The upstream unit's own settings are not visible, so the drop-in alone must meet the
    rule."""
    dropins = {}
    for path in files:
        parent = Path(path).parent.name
        if path.endswith(".conf") and (
            parent == "service.d" or parent.endswith(".service.d")
        ):
            dropins.setdefault(parent, []).append(path)

    def merged(unit):
        chosen = {}
        for d in dropin_dirs(unit):
            for p in dropins.get(d, []):
                chosen[Path(p).name] = p
        return [chosen[name] for name in sorted(chosen)]

    units = service_units(files)
    own = {f"{unit}.d" for unit, _ in units.values()}
    for d, paths in dropins.items():
        if d not in own and any(
            key in EXEC_KEYS | PRIVILEGE_KEYS
            for p in paths
            for key, _ in unit_directives(files[p])
        ):
            unit = d[:-2] if d != "service.d" else ".service"
            units[str(Path(sorted(paths)[0]).parent)] = (unit, "")

    failing = set()
    for ident, (unit, text) in units.items():
        directives = unit_directives(text)
        for p in merged(unit):
            directives += unit_directives(files[p])
        if not is_hardened(directives):
            failing.add(ident)

    problems = [
        f"{ident}: sets neither NoNewPrivileges=yes nor an allow-list "
        f"CapabilityBoundingSet= (doc_threat_model.md, TM8)"
        for ident in sorted(failing - SERVICE_EXEMPT.keys())
    ]
    problems += [
        f"{ident}: exempt in SERVICE_EXEMPT but meets the rule or no longer exists; "
        f"remove the entry"
        for ident in sorted(SERVICE_EXEMPT.keys() - failing)
    ]
    return problems


@check("services", "Every shipped service sets NoNewPrivileges or a capability allow-list")
def check_services():
    r = Result()
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.service", "*service.d/*.conf", "*.spec"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if listed.returncode != 0:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    files = {
        p: read(ROOT / p)
        for p in listed.stdout.split("\0")
        if p and (ROOT / p).is_file()
    }
    for problem in service_problems(files):
        r.fail(problem)
    for ident, why in SERVICE_EXEMPT.items():
        r.note(f"exempt: {ident}: {why}")
    return r


# --------------------------------------------------------------------------- #
# 11b. session-units — the shipped user units follow doc_session.md's rules
# --------------------------------------------------------------------------- #

# doc_session.md, SN12: every user unit Athanor ships for the session, with its crash class of
# SN6, whether it connects to the compositor (SN3) and how the session starts it: through a
# user preset under athanor-session.target, through D-Bus activation, by name in
# athanor-session.target itself (the gate through Requires=, the others through Wants=), or by
# its session's script alone.
SESSION_UNITS = {
    "athanor-bar.service": ("surface", True, "preset"),
    "athanor-dock.service": ("surface", True, "preset"),
    "athanor-shelld.service": ("surface", False, "preset"),
    "athanor-idle.service": ("guard", True, "preset"),
    "athanor-lock.service": ("guard", True, "preset"),
    "athanor-wallpaper.service": ("surface", True, "preset"),
    "athanor-launcher.service": ("surface", True, "activated"),
    "athanor-control-center.service": ("surface", True, "activated"),
    # Started by nothing until its bus service file comes with doc_osd.md's retirement step
    # (OD15, OD17 step 5).
    "athanor-osd.service": ("surface", True, "activated"),
    "athanor-update-notify.service": ("surface", False, "preset"),
    "athanor-sessiond.service": ("guard", False, "preset"),
    # SN6 names no class for the Super+C oneshot (doc_control_center.md, CC9); it runs once
    # per login and never restarts, as a session step does.
    "athanor-control-center-shortcut.service": ("step", False, "preset"),
    # Its file chooser, dialogs and pickers are windows (doc_portal.md).
    "xdg-desktop-portal-athanor.service": ("helper", True, "activated"),
    "athanor-desktop.service": ("step", False, "target"),
    "athanor-skel-sync.service": ("step", False, "target"),
    "cosmic-osd.service": ("cosmic", True, "target"),
    "cosmic-settings-daemon.service": ("cosmic", True, "target"),
    # The own compositor (SN15): athanor-sulphur-session starts it and waits on it, and its
    # ExecStartPost= starts athanor-session.target, so it precedes the gate, which probes it.
    "athanor-sulphur.service": ("compositor", False, "script"),
}
SESSION_TARGET = "athanor-session.target"
# Shipped user units that are not session units, and why.
NOT_SESSION_UNITS = {
    "athanor-launcher-rates.service": "the launcher's exchange-rate job, started by its timer",
    "athanor-launcher-rates.timer": "the launcher's exchange-rate timer, under timers.target",
    "athanor-wallpaper-decode.socket": "the wallpaper decoder's static socket, wanted by athanor-wallpaper.service (doc_session_daemons.md, SD17)",
    "athanor-wallpaper-decode@.service": "the wallpaper decoder, one instance per connection to its socket",
}
# SN6's policy per class: each setting's required value, None where the unit must not set it.
# A time span is compared in seconds. Restart= unset is Restart=no (systemd.service(5)).
SESSION_CLASSES = {
    "guard": {
        "Restart": "always",
        "RestartSec": 0.1,
        "RestartSteps": "5",
        "RestartMaxDelaySec": 30,
        "StartLimitIntervalSec": 0,
    },
    "surface": {
        "Restart": "on-failure",
        "RestartSec": 0.1,
        "RestartSteps": None,
        "RestartMaxDelaySec": None,
        "StartLimitBurst": "10",
        "StartLimitIntervalSec": 600,
    },
    "helper": {"Restart": "no"},
    "step": {"Type": "oneshot", "Restart": "no"},
    # COSMIC's units keep their own policy until they leave (SN6, exceptions).
    "cosmic": {"Restart": "on-failure", "RestartSec": 2},
    # The own compositor has no Restart=: its end ends the session (SN6, exceptions; SN15).
    "compositor": {"Restart": "no"},
}
SESSION_TIME_KEYS = {"RestartSec", "RestartMaxDelaySec", "StartLimitIntervalSec"}
# Findings the units do not meet yet, each the exact text of the check's finding for that unit,
# and why. An entry that no longer matches a finding fails the check, so the list cannot
# outlive its reasons, and it covers that finding only: a new divergence of the same unit fails.
SN3_CITE = "(doc_session.md, SN3)"
NOT_THE_TARGET = f"WantedBy=graphical-session.target, not athanor-session.target {SN3_CITE}"
ACTIVATED_INSTALL = f"D-Bus-activated, so no [Install] {SN3_CITE}"
ACTIVATED_PRESET = f"D-Bus-activated, so no user preset enabling it {SN3_CITE}"
SCRIPT_STARTED = (
    f"started by its session's script alone, so no [Install], no user preset and not pulled in by "
    f"{SESSION_TARGET} (doc_session.md, SN15)"
)
SURFACE_WANTS = "the surface class of doc_session.md, SN6, wants"
SESSION_EXEMPT = {
    ("athanor-skel-sync.service", f"no PartOf=graphical-session.target {SN3_CITE}"):
        "SN13 step 1 does not list it",
    ("athanor-skel-sync.service", NOT_THE_TARGET): "the target wants it; SN13 step 1 does not list it",
    ("cosmic-osd.service", NOT_THE_TARGET): "the target wants it; SN13 step 1 does not list it",
    ("cosmic-settings-daemon.service", NOT_THE_TARGET): "the target wants it; SN13 step 1 does not list it",
    ("athanor-lock.service", f"started eagerly, so WantedBy={SESSION_TARGET} and enabled by a user preset {SN3_CITE}"):
        "installed and not enabled until its preset at the switch (doc_lock_and_prompts.md, LP18 step 3)",
    ("athanor-launcher.service", ACTIVATED_INSTALL):
        "enabled by hand until the switch (doc_launcher.md, LA12)",
    ("athanor-control-center.service", ACTIVATED_INSTALL):
        "[Install] kept for a preset that disables it (doc_control_center.md, CC2)",
    (
        "athanor-control-center-shortcut.service",
        f"headless, so neither After= nor Requisite=athanor-desktop.service {SN3_CITE}",
    ): "SN6 names no class for it, and SN3 does not say whether it connects to the compositor",
    ("athanor-update-notify.service", f"RestartSec=30, {SURFACE_WANTS} 0.1"):
        "SN13 step 1 changes only its [Install]",
    ("athanor-update-notify.service", f"StartLimitBurst=(unset), {SURFACE_WANTS} 10"):
        "SN13 step 1 changes only its [Install]",
    ("athanor-update-notify.service", f"StartLimitIntervalSec=(unset), {SURFACE_WANTS} 600"):
        "SN13 step 1 changes only its [Install]",
    (
        "xdg-desktop-portal-athanor.service",
        f"connects to the compositor, so After= and Requisite=athanor-desktop.service {SN3_CITE}",
    ): "its dialogs connect to the compositor (doc_portal.md); SN13 step 1 does not list it",
    ("xdg-desktop-portal-athanor.service", "Restart=on-failure, the helper class of doc_session.md, SN6, wants no"):
        "until doc_portal.md PT3's next revision (doc_session.md, section 3)",
}


def unit_section(text, section):
    """(key, value) of every assignment in one section of a unit file."""
    current, found = None, []
    for line in re.sub(r"\\[ \t]*\r?\n", " ", text).splitlines():
        line = line.strip()
        if not line or line[0] in "#;":
            continue
        if line.startswith("[") and line.endswith("]"):
            current = line[1:-1]
        elif current == section and "=" in line:
            key, value = line.split("=", 1)
            found.append((key.strip(), value.strip()))
    return found


def unit_names(pairs, key):
    """The unit names every `key=` assignment lists, an empty assignment resetting the list."""
    names = set()
    for k, value in pairs:
        if k == key:
            names = names | set(value.split()) if value else set()
    return names


def time_span(value):
    """A systemd time span in seconds (systemd.time(7)); a bare number is seconds."""
    scale = {"us": 1e-6, "ms": 1e-3, "": 1, "s": 1, "sec": 1, "m": 60, "min": 60, "h": 3600}
    parts = re.findall(r"(\d+(?:\.\d+)?)\s*([a-z]*)", value)
    if not parts or "".join(n + u for n, u in parts) != value.replace(" ", ""):
        return None
    if any(u not in scale for _, u in parts):
        return None
    return sum(float(n) * scale[u] for n, u in parts)


def session_unit_problems(units, enabled):
    """((unit, rule), finding) for every rule of doc_session.md a shipped user unit breaks.

    `units` maps every shipped user unit to its text; `enabled` holds the units a user preset
    enables. The rules: SN3's binding, `[Install]` and compositor edges, SN4's edges in force
    and SN6's policy per class."""
    found = []

    def fail(unit, rule, message):
        found.append(((unit, rule), message))

    for unit in sorted(units.keys() - SESSION_UNITS.keys() - NOT_SESSION_UNITS.keys() - {SESSION_TARGET}):
        fail(unit, "class", "a shipped user unit with no entry in SESSION_UNITS (doc_session.md, SN6)")
    for unit in sorted((SESSION_UNITS.keys() | NOT_SESSION_UNITS.keys() | {SESSION_TARGET}) - units.keys()):
        fail(unit, "class", "listed in SESSION_UNITS but not shipped; remove the entry")

    target = unit_section(units.get(SESSION_TARGET, ""), "Unit")
    if "graphical-session.target" not in unit_names(target, "BindsTo"):
        fail(SESSION_TARGET, "order", "no BindsTo=graphical-session.target (doc_session.md, SN3)")
    if "xdg-desktop-autostart.target" not in unit_names(target, "Before"):
        fail(SESSION_TARGET, "order", "not Before=xdg-desktop-autostart.target (doc_session.md, SN4)")
    pulled = unit_names(target, "Wants") | unit_names(target, "Requires")

    for unit, (cls, graphical, start) in sorted(SESSION_UNITS.items()):
        if unit not in units:
            continue
        head = unit_section(units[unit], "Unit")
        service = unit_section(units[unit], "Service")
        install = unit_section(units[unit], "Install")
        if "graphical-session.target" not in unit_names(head, "PartOf"):
            fail(unit, "binding", "no PartOf=graphical-session.target (doc_session.md, SN3)")

        wanted_by = unit_names(install, "WantedBy")
        if wanted_by - {SESSION_TARGET}:
            fail(unit, "install", f"WantedBy={' '.join(sorted(wanted_by))}, not {SESSION_TARGET} (doc_session.md, SN3)")
        if start == "preset" and (wanted_by != {SESSION_TARGET} or unit not in enabled):
            fail(unit, "install", f"started eagerly, so WantedBy={SESSION_TARGET} and enabled by a user preset (doc_session.md, SN3)")
        if start == "activated" and install:
            fail(unit, "install", ACTIVATED_INSTALL)
        if start == "activated" and unit in enabled:
            fail(unit, "install", ACTIVATED_PRESET)
        if start == "target" and unit not in pulled:
            fail(unit, "install", f"marked as started by {SESSION_TARGET}, which neither wants nor requires it")
        if start == "script" and (install or unit in enabled or unit in pulled):
            fail(unit, "install", SCRIPT_STARTED)

        gate = "athanor-desktop.service"
        edges = (gate in unit_names(head, "After"), gate in unit_names(head, "Requisite"))
        if graphical and edges != (True, True):
            fail(unit, "edge", f"connects to the compositor, so After= and Requisite={gate} (doc_session.md, SN3)")
        if not graphical and any(edges):
            fail(unit, "edge", f"headless, so neither After= nor Requisite={gate} (doc_session.md, SN3)")

        # The start limits belong to [Unit], the rest to [Service] (systemd.unit(5), systemd.service(5)).
        settings = dict(service)
        settings.update((k, v) for k, v in head if k.startswith("StartLimit"))
        for key, want in SESSION_CLASSES[cls].items():
            have = settings.get(key, "no" if key == "Restart" else None)
            if want is not None and key in SESSION_TIME_KEYS:
                same = have is not None and time_span(have) == want
            else:
                same = have == want
            if not same:
                fail(unit, "class", f"{key}={have if have is not None else '(unset)'}, the {cls} class of doc_session.md, SN6, wants {want if want is not None else 'it unset'}")

    bar = unit_section(units.get("athanor-bar.service", ""), "Unit")
    if not {"athanor-shelld.service"} <= unit_names(bar, "Wants") & unit_names(bar, "After"):
        fail("athanor-bar.service", "order", "does not want athanor-shelld.service and start after it (doc_session.md, SN4)")
    return found


def session_exempt_problems(found):
    """The findings SESSION_EXEMPT does not name, and its entries no finding matches."""
    failing = {(key[0], message) for key, message in found}
    problems = [f"{unit}: {message}" for unit, message in sorted(failing - SESSION_EXEMPT.keys())]
    problems += [
        f"{unit}: exempt in SESSION_EXEMPT from '{finding}' but meets the rule; remove the entry"
        for unit, finding in sorted(SESSION_EXEMPT.keys() - failing)
    ]
    return problems


# A line that starts one of these sections ends the %files section; any other %word stays in it.
FILES_END = re.compile(
    r"^%(?:prep|build|install|check|clean|conf|changelog|description|package|pre|post|preun|postun"
    r"|pretrans|posttrans|verifyscript|sepolicy|generate_buildrequires|patchlist|sourcelist"
    r"|trigger\w*|filetrigger\w*|transfiletrigger\w*)\b"
)
# What may precede a path in %files: %attr(), %config, %config(), %verify(), %defattr().
FILES_DIRECTIVES = re.compile(r"^(?:%(?:attr|config|verify|defattr)(?:\([^)]*\))?\s+)+")
USER_UNIT_DIR = r"(?:/usr/lib/systemd/user|%\{_userunitdir\})"
USER_UNIT_PATH = re.compile(USER_UNIT_DIR + r"/([^/\s*?\[\]%]+\.(?:service|target|timer|socket))")
# A drop-in directory, or a file in one, is not a unit: its unit is shipped elsewhere.
USER_DROPIN_PATH = re.compile(USER_UNIT_DIR + r"/[^/\s*?\[\]%]+\.d(?:/[^/\s*?\[\]%]+\.conf)?")
USER_UNIT_MENTION = re.compile(r"systemd/user(?![-\w])|_userunitdir|systemd_user")


def spec_user_units(text):
    """(names, problems): the user units a spec's %files lists, and each %files line that
    mentions the user unit directory but that this scan cannot classify (a glob, a macro it
    does not know), which it would otherwise not see."""
    names, problems, in_files = [], [], False
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("%files"):
            in_files = True
            continue
        if FILES_END.match(line):
            in_files = False
        if not in_files or line.startswith("#") or not USER_UNIT_MENTION.search(line):
            continue
        path = FILES_DIRECTIVES.sub("", line)
        path = re.sub(r"^%dir\s+", "", path)
        unit = USER_UNIT_PATH.fullmatch(path)
        if unit:
            names.append(unit.group(1))
        elif not USER_DROPIN_PATH.fullmatch(path):
            problems.append(line)
    return names, problems


def shipped_user_units(root):
    """({unit: text}, problems) for every unit a spec under forge/specs installs in
    /usr/lib/systemd/user, read from the one file of that name in the spec's directory."""
    units, problems = {}, []
    for spec in sorted(root.glob("forge/specs/*/*.spec")):
        names, unclassified = spec_user_units(read(spec))
        problems += [f"{rel(spec)}: cannot tell which user unit '{line}' ships; list each unit as a plain path, optionally after %attr(), %config or %verify()" for line in unclassified]
        for name in names:
            sources = [p for p in walk(spec.parent, name) if p.name == name and "skel" not in p.parts]
            if len(sources) != 1:
                problems.append(f"{rel(spec)}: ships {name}, found {len(sources)} source files of that name")
                continue
            units[name] = read(sources[0])
    return units, problems


# A tmpfiles.d line that creates a directory under the user's state directory.
TMPFILES_DIRECTORY = re.compile(r"^[dD]!?\s+%S/([^/\s]+)")


def shipped_user_state_directories(root):
    """The top-level names under the user's state directory that a user tmpfiles.d file creates
    (`d %S/<name>`), counting only the files a spec lists in its %files."""
    names = set()
    for spec in sorted(root.glob("forge/specs/*/*.spec")):
        text = read(spec)
        files = text.split("%files", 1)[1].split("%changelog", 1)[0] if "%files" in text else ""
        for conf in sorted((spec.parent / "SOURCES/usr/share/user-tmpfiles.d").glob("*.conf")):
            if f"/usr/share/user-tmpfiles.d/{conf.name}" not in files:
                continue
            for line in read(conf).splitlines():
                match = TMPFILES_DIRECTORY.match(line.strip())
                if match:
                    names.add(match.group(1))
    return names


def state_link_problems(units, created):
    """A finding for every top-level name a user unit takes as StateDirectory= that `created`
    does not hold. When that directory is missing and ~/.config/<name> exists, whoever made it,
    systemd 254 and later links the first to the second, and the state lives in the configuration
    (#1238); a tmpfiles.d entry creates it before any unit starts.
    ponytail: top-level names only; a nested StateDirectory= whose own ~/.config path exists
    would still be linked, check the full path if one ever appears."""
    state = {}
    for unit, text in units.items():
        for path in unit_names(unit_section(text, "Service"), "StateDirectory"):
            state.setdefault(path.split(":")[0].split("/")[0], set()).add(unit)
    return [
        f"'{name}' is StateDirectory= of {', '.join(sorted(state[name]))} and no shipped user tmpfiles.d "
        f"line 'd %S/{name}' creates it: systemd links a missing ~/.local/state/{name} to an existing "
        f"~/.config/{name} (#1238)"
        for name in sorted(state.keys() - created)
    ]


@check("session-units", "The shipped user units follow doc_session.md's binding, order and crash classes")
def check_session_units():
    r = Result()
    units, problems = shipped_user_units(ROOT)
    for problem in problems:
        r.fail(problem)
    enabled = {unit for verb, unit, _ in preset_lines(ROOT) if verb == "enable"}
    for problem in session_exempt_problems(session_unit_problems(units, enabled)):
        r.fail(problem)
    for problem in state_link_problems(units, shipped_user_state_directories(ROOT)):
        r.fail(problem)
    for (unit, finding), why in SESSION_EXEMPT.items():
        r.note(f"exempt: {unit}: {finding}: {why}")
    return r


# Properties that put a user unit in namespaces of its own. Under the user manager each one
# gives the unit a user namespace, a sibling of every other unit's, and the kernel refuses a
# ptrace-read (`readlink /proc/<pid>/exe`) between siblings (doc_session_daemons.md SD17).
NAMESPACING_PROPERTY = re.compile(
    r"^(Private(?:Network|IPC|Users|Tmp|PIDs|Devices|Mounts)|ProtectSystem|ProtectHome|Protect(?:Kernel\w+|ControlGroups|Proc|Hostname|Clock)"
    r"|TemporaryFileSystem|InaccessiblePaths|ReadOnlyPaths|ReadWritePaths|BindPaths|BindReadOnlyPaths|MountAPIVFS|RootDirectory|RootImage"
    r"|ProcSubset|ExecPaths|NoExecPaths|MountImages|ExtensionDirectories|ExtensionImages|PrivateBPF"
    r"|ReadWriteDirectories|ReadOnlyDirectories|InaccessibleDirectories)\s*=(.*)$"
)
# The values that leave a namespacing property off: empty, a false boolean in any of systemd's
# spellings, ProtectProc=default or ProcSubset=all.
NAMESPACING_OFF = ("", "0", "no", "n", "false", "f", "off", "default", "all")
# The command lines of a unit, each a program its namespaces hold.
UNIT_EXEC = re.compile(r"(Exec(?:Start(?:Pre|Post)?|Condition|Reload|Stop(?:Post)?))\s*=\s*[-@:+!]*(\S+)")
# A read of a process's executable: any mention of `admit_user_unit` but its definition, an
# import under another name included, or a path `/proc/<pid>/exe` built in code.
EXE_READING_ADMISSION = re.compile(r"(?<!fn )\badmit_user_unit\b|/proc/\{[^}]*\}/exe\b|\.join\(\s*\"exe\"\s*\)")
# The namespaced user units' programs that are no binary of a crate here, each with why it
# admits no peer by its executable. A wrapper (env, sh -c, systemd-run) is listed only with its
# unit, so a wrapper that starts one of this repository's daemons is never passed by name.
FOREIGN_PROGRAMS = {
    ("athanor-launcher-rates.service", "mkdir"): "an ExecStartPre= that creates qalculate's directory",
    ("athanor-launcher-rates.service", "qalc"): "Qalculate's command line refreshes the currency rates and accepts no connection",
    ("athanor-skel-sync.service", "bash"): "a script that copies /etc/skel once and accepts no connection",
    ("cosmic-osd.service", "cosmic-osd"): "upstream COSMIC's program; no code of this repository runs in it",
    ("cosmic-settings-daemon.service", "cosmic-settings-daemon"): "upstream COSMIC's program; no code of this repository runs in it",
}
# A library file whose exe read is the body of a function matched by name at each call site, so
# depending on its crate reads nothing: athanor-unit's peer.rs, the read of admit_user_unit.
READ_BY_CALL = {"system/athanor-unit/src/peer.rs"}


def crate_programs(root):
    """({binary: first file:line of an exe-reading call or None}) for every crate under system/
    and forge/specs: its package name, its [[bin]] names and its src/bin/*.rs files. A read in
    a crate of this repository that the binary's crate depends on, directly or not, counts."""
    crates = {}
    for base in ("system", "forge/specs"):
        for manifest in walk(root / base, "Cargo.toml"):
            try:
                package = tomllib.loads(read(manifest))
            except tomllib.TOMLDecodeError:
                continue
            name = package.get("package", {}).get("name")
            bins = {name} | {b.get("name") for b in package.get("bin", [])}
            bins |= {f.stem for f in (manifest.parent / "src" / "bin").glob("*.rs")}
            bins.discard(None)
            tables = [package.get("dependencies", {})] + [t.get("dependencies", {}) for t in package.get("target", {}).values()]
            deps = {(spec.get("package", key) if isinstance(spec, dict) else key) for table in tables for key, spec in table.items()}
            reads = []
            for source in walk(manifest.parent / "src", ".rs"):
                for number, line in enumerate(read(source).split("\n"), 1):
                    if EXE_READING_ADMISSION.search(line.split("//")[0]):
                        reads.append((source.relative_to(root).as_posix(), number))
            crates[name or rel(manifest)] = (bins, deps, reads)

    def reads_of(name, seen):
        bins, deps, reads = crates[name]
        own = next((f"{path}:{number}" for path, number in reads if not seen or path not in READ_BY_CALL), None)
        if own:
            return own
        seen.add(name)
        return next((r for d in sorted(deps) if d in crates and d not in seen for r in [reads_of(d, seen)] if r), None)

    programs = {}
    for name, (bins, _, _) in crates.items():
        reads = reads_of(name, set())
        for binary in bins:
            programs[binary] = programs.get(binary) or reads
    return programs


def user_unit_texts(root, units):
    """{unit: text} of the shipped user units with every drop-in of `<unit>.d/*.conf` appended."""
    texts = dict(units)
    for conf in walk(root / "forge" / "specs", ".conf"):
        if conf.parent.name.endswith(".service.d") and conf.parts[-4:-2] == ("systemd", "user") and "skel" not in conf.parts:
            unit = conf.parent.name[:-2]
            texts[unit] = texts.get(unit, "") + "\n" + read(conf)
    return texts


def peer_admission_problems(units, programs):
    """A unit that sets a namespacing property and runs a program that reads its peer's
    executable fails with EACCES and refuses every peer. A unit that sets one and runs a
    program this scan cannot place is a problem too: it would otherwise pass unseen."""
    problems = []
    for unit, text in sorted(units.items()):
        lines = [line.strip() for line in text.split("\n")]
        namespaced = next((m for m in map(NAMESPACING_PROPERTY.match, lines) if m and m.group(2).strip().lower() not in NAMESPACING_OFF), None)
        if not namespaced:
            continue
        for line in lines:
            m = UNIT_EXEC.match(line)
            if not m:
                continue
            program = m.group(2).rsplit("/", 1)[-1]
            if program in programs:
                if programs[program]:
                    problems.append(f"{unit}: runs {program}, which reads a peer's executable at {programs[program]}, and sets {namespaced.group(1)}=: the unit's user namespace cannot read the peer's /proc/<pid>/exe, so every peer is refused. Admit the peer by control group (athanor_unit::peer::admit_user_unit_cgroup, doc_session_daemons.md SD17)")
            elif (unit, program) not in FOREIGN_PROGRAMS:
                problems.append(f"{unit}: sets {namespaced.group(1)}= and its {m.group(1)}= runs {m.group(2)}, which is neither a binary of a crate under system/ or forge/specs nor in FOREIGN_PROGRAMS: say which it is")
    return problems


@check("peer-exe", "A user unit with namespacing properties runs no program that reads its peer's executable")
def check_peer_exe():
    r = Result()
    units, problems = shipped_user_units(ROOT)
    for problem in problems:
        r.fail(problem)
    for problem in peer_admission_problems(user_unit_texts(ROOT, units), crate_programs(ROOT)):
        r.fail(problem)
    for (unit, program), why in FOREIGN_PROGRAMS.items():
        r.note(f"foreign: {unit}: {program}: {why}")
    return r


# The polkit model (doc_threat_model.md TM9, maintainer decision T3 of 2026-10-07): every
# action the repository declares in a .policy file or overrides in a polkit .rules file has
# one row in TM9's table, with its result for the active session.
POLKIT_RULES_WORDS = {"polkit", "addRule", "addAdminRule", "Result", "function", "if", "else", "return", "action", "id", "subject", "user", "groups", "isInGroup", "isInNetGroup", "local", "active", "session", "seat", "pid"}
POLKIT_MODEL_DOC = "docs/architecture/doc_threat_model.md"
POLKIT_RESULTS = {
    "no",
    "yes",
    "auth_self",
    "auth_self_keep",
    "auth_admin",
    "auth_admin_keep",
    "not_handled",
}


def js_without_comments_and_strings(text):
    """(code, literals): comments dropped and every string literal replaced by \\0<n>\\0,
    its index in literals, so that no pattern matches inside a string or a comment and no
    identifier can pass for a literal. None when the text holds a template literal or NUL.
    """
    # ponytail: no JS regex literals; a rule using one (/"/) may misread, add when one ships
    out, literals, i, n = [], [], 0, len(text)
    if "\0" in text:
        return None
    while i < n:
        if text.startswith("//", i):
            j = re.search("[\n\r\u2028\u2029]", text[i:])
            i = n if j is None else i + j.start()
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
            out.append(" ")
        elif text[i] == "`":
            return None
        elif text[i] in "\"'":
            j = i + 1
            while j < n and text[j] != text[i]:
                j += 2 if text[j] == "\\" else 1
            literals.append(text[i + 1 : j])
            out.append(f"\0{len(literals) - 1}\0")
            i = j + 1
        else:
            out.append(text[i])
            i += 1
    return "".join(out), literals


def js_close(code, i):
    """Index after the bracket that closes code[i], or -1."""
    depth = 0
    for j in range(i, len(code)):
        depth += (code[j] in "([{") - (code[j] in ")]}")
        if depth == 0:
            return j + 1
    return -1


def js_top_split(code, sep):
    """code split at every sep outside brackets."""
    parts, depth, start, i = [], 0, 0, 0
    while i < len(code):
        depth += (code[i] in "([{") - (code[i] in ")]}")
        if depth == 0 and code.startswith(sep, i):
            parts.append(code[start:i])
            start = i = i + len(sep)
            continue
        i += 1
    return parts + [code[start:]]


def polkit_rule_ids(cond, literals):
    """The action ids an if condition requires, or None when it is not in the one form read:
    `action.id == L` or several joined by `||`, alone or as one operand of `&&` whose other
    operands do not name the action. Redundant parentheses are allowed."""

    def bare(e):
        e = e.strip()
        while e.startswith("(") and js_close(e, 0) == len(e):
            e = e[1:-1].strip()
        return e

    def ids_of(e):
        tests = [
            re.fullmatch(r"action\s*\.\s*id\s*===?\s*\x00(\d+)\x00", bare(alt))
            for alt in js_top_split(bare(e), "||")
        ]
        return [literals[int(t.group(1))] for t in tests] if all(tests) else None

    cond = bare(cond)
    if len(js_top_split(cond, "||")) > 1:
        return ids_of(cond)
    guards = []
    for part in js_top_split(cond, "&&"):
        ids = ids_of(part)
        if ids:
            guards.append(ids)
        elif re.search(r"\baction\b", part):
            return None
    return guards[0] if len(guards) == 1 else None


def polkit_rules_declared(path, text, cannot):
    """{(action, path): results} of one polkit .rules file, and why it cannot be read.

    Only one shape is read. At the top level, nothing but polkit.addRule(...) and
    polkit.addAdminRule(...) calls. Each addRule function is
    `function(action, subject) { if (<ids>) { ... } }` with nothing after the block, where
    <ids> is polkit_rule_ids's form, and the block returns only polkit.Result.X and does
    not name the action. Anything else fails, so a rule cannot override an action unseen.
    """

    def unreadable(why):
        return {}, [f"{path}: {why}, {cannot}"]

    tokens = js_without_comments_and_strings(text)
    if tokens is None:
        return unreadable("a template literal or a NUL byte is not read")
    code, literals = tokens
    words = set(re.findall(r"[A-Za-z_$][\w$]*", code))
    if words - POLKIT_RULES_WORDS - {r.upper() for r in POLKIT_RESULTS}:
        return unreadable(
            f"it uses {', '.join(sorted(words - POLKIT_RULES_WORDS - {r.upper() for r in POLKIT_RESULTS}))}, "
            "outside the identifiers a rule is read with"
        )
    if re.sub(r"\x00\d+\x00|[A-Za-z_]+", "", code).strip(" \t\r\n(){}[].,;=|&"):
        return unreadable("it uses characters other than identifiers, strings and ( ) { } [ ] . , ; = | &")
    if any(len(run) not in (2, 3) for run in re.findall(r"=+", code)):
        return unreadable("it assigns: only == and === are read")
    members = (
        r"\bpolkit\s*\.\s*(?:addRule|addAdminRule|Result\s*\.\s*[A-Z_]+)\b"
    )
    if len(re.findall(r"\bpolkit\b", code)) != len(re.findall(members, code)):
        return unreadable(
            "polkit is used other than through polkit.addRule, polkit.Result.X and its other members"
        )
    if len(re.findall(r"\bResult\b", code)) != len(
        re.findall(r"\bpolkit\s*\.\s*Result\s*\.\s*[A-Z_]+", code)
    ):
        return unreadable("Result is used other than as polkit.Result.X")
    declared, rules, pos = {}, 0, 0
    call = re.compile(r"[\s;]*polkit\s*\.\s*(addRule|addAdminRule)\s*\(")
    while m := call.match(code, pos):
        pos = js_close(code, m.end() - 1)
        if pos < 0:
            return unreadable("a polkit call is not closed")
        if m.group(1) == "addAdminRule":
            continue
        rules += 1
        arg = code[m.end() : pos - 1]
        head = re.match(
            r"\s*function\s*\(\s*action\s*(?:,\s*subject\s*)?\)\s*\{\s*if\s*\(", arg
        )
        cond_end = js_close(arg, head.end() - 1) if head else -1
        brace = re.compile(r"\s*\{").match(arg, cond_end) if cond_end > 0 else None
        block_end = js_close(arg, brace.end() - 1) if brace else -1
        if block_end < 0 or arg[block_end:].strip() != "}":
            return unreadable(
                "an addRule function is not `function(action, subject) { if (...) { ... } }` "
                "with nothing after the block"
            )
        ids = polkit_rule_ids(arg[head.end() : cond_end - 1], literals)
        block = arg[brace.end() : block_end - 1]
        results = {
            r.lower()
            for r in re.findall(r"\bpolkit\s*\.\s*Result\s*\.\s*([A-Z_]+)", block)
        }
        returns = len(re.findall(r"\breturn\b", block))
        if returns != len(re.findall(r"\breturn\s+polkit\s*\.\s*Result\s*\.\s*[A-Z_]+\s*[;}]", block + ";")):
            ids = None
        if not ids or not results or re.search(r"\b(?:action|function)\b", block):
            return unreadable(
                'an addRule tests the action other than as action.id == "..." joined by || '
                "(and, in parentheses, && conditions on the subject), or its block names the "
                "action or returns no polkit.Result"
            )
        for a in ids:
            declared.setdefault((a, path), set()).update(results)
    if code[pos:].strip(" \t\r\n;"):
        return unreadable(
            "code other than polkit.addRule(...) and polkit.addAdminRule(...) stands at the top level"
        )
    if len(re.findall(r"\baddRule\b", code)) != rules:
        return unreadable(
            "addRule is reached other than as a top-level polkit.addRule(...)"
        )
    return declared, []


def polkit_declared(files):
    """{(action, file): results} for the active session, and the files that cannot be read.

    A .policy gives its `allow_active` default (`no` when absent). A polkit .rules file gives,
    for each action its addRule functions test, every polkit.Result the function returns
    (polkit_rules_declared). A .rules file that has neither `polkit.` nor `addRule` is a
    udev rule and is skipped.
    """
    cannot = f"so its rows of {POLKIT_MODEL_DOC} TM9 cannot be checked"
    declared, unreadable = {}, []
    for path, text in sorted(files.items()):
        if path.endswith(".policy"):
            try:
                root = ET.fromstring(text)
            except ET.ParseError as e:
                unreadable.append(f"{path}: not valid XML ({e}), {cannot}")
                continue
            for action in root.iter("action"):
                if not action.get("id"):
                    unreadable.append(f"{path}: an <action> has no id, {cannot}")
                    continue
                active = (action.findtext("defaults/allow_active") or "no").strip()
                declared[(action.get("id"), path)] = {active}
        elif path.endswith(".rules") and "udev/rules.d/" not in path:
            found, problems = polkit_rules_declared(path, text, cannot)
            declared.update(found)
            unreadable += problems
    return declared, unreadable


def polkit_table(doc):
    """[(action, file, results)] from the table whose header starts with | Action | Declared in |."""
    rows, inside = [], False
    for line in doc.split("\n"):
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if line.startswith("| Action") and len(cells) > 2 and cells[1] == "Declared in":
            inside = True
            continue
        if not inside:
            continue
        if not line.startswith("|"):
            break
        if set(cells[0]) <= set("-: "):
            continue
        tokens = set(re.findall(r"`([a-z_]+)`", cells[2]))
        rows.append((cells[0].strip("`"), cells[1].strip("`"), tokens & POLKIT_RESULTS))
    return rows


def polkit_model_problems(files, doc):
    declared, problems = polkit_declared(files)
    rows = polkit_table(doc)
    if not rows:
        problems.append(
            f"{POLKIT_MODEL_DOC}: no table with the header | Action | Declared in | ..."
        )
    seen = set()
    for action, path, results in rows:
        key = (action, path)
        if key in seen:
            problems.append(f"{POLKIT_MODEL_DOC} TM9: {action} ({path}) has two rows")
            continue
        seen.add(key)
        if key not in declared:
            problems.append(
                f"{POLKIT_MODEL_DOC} TM9: {action} is not declared in {path}: remove the row"
            )
        elif results != declared[key]:
            problems.append(
                f"{POLKIT_MODEL_DOC} TM9: {action} gives {', '.join(sorted(declared[key]))} for the "
                f"active session in {path}, the table says {', '.join(sorted(results)) or 'nothing'}"
            )
    for action, path in sorted(declared.keys() - seen):
        problems.append(f"{path}: {action} has no row in {POLKIT_MODEL_DOC} TM9")
    return problems


@check(
    "polkit-model",
    "Every polkit action the repository declares or overrides has its row in TM9",
)
def check_polkit_model():
    r = Result()
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.policy", "*.rules"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if listed.returncode != 0:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    files = {
        p: read(ROOT / p)
        for p in listed.stdout.split("\0")
        if p and (ROOT / p).is_file()
    }
    for problem in polkit_model_problems(files, read(ROOT / POLKIT_MODEL_DOC)):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# coverage: every component has an inventory entry (A2-34)
# --------------------------------------------------------------------------- #

COMPONENTS = "docs/architecture/components.toml"
# Every child directory of these is a component; "" is the repository root.
COMPONENT_PARENTS = ("", "system", "forge", "forge/specs", "scripts")
# Children of COMPONENT_PARENTS that hold components instead of being one.
COMPONENT_CONTAINERS = {"docs", "forge", "forge/specs", "scripts", "system"}
COMPONENT_KINDS = {"crate", "rpm spec", "script tool", "config", "image", "tests", "site"}
COMPONENT_AREAS = {"kernel", "build-ci", "signing-update", "security", "shell", "apps",
                   "platform", "docs"}
COMPONENT_STATUSES = {"specified", "out-of-1.0", "missing"}


def component_dirs(files):
    """The component directories that the tracked files imply: each child directory of a
    COMPONENT_PARENTS entry, except hidden directories and the containers."""
    dirs = set()
    for f in files:
        parts = f.split("/")
        for parent in COMPONENT_PARENTS:
            depth = len(parent.split("/")) if parent else 0
            if len(parts) > depth + 1 and "/".join(parts[:depth]) == parent:
                child = "/".join(parts[:depth + 1])
                if not parts[depth].startswith(".") and child not in COMPONENT_CONTAINERS:
                    dirs.add(child)
    return dirs


def entry_problems(root, entry):
    """The problems of one inventory entry, and whether its specification awaits a merge."""
    path = entry["path"]
    out = []
    if not (root / path).exists():
        out.append("the path does not exist")
    if entry.get("kind") not in COMPONENT_KINDS:
        out.append(f"kind {entry.get('kind')!r} is not one of {sorted(COMPONENT_KINDS)}")
    if entry.get("area") not in COMPONENT_AREAS:
        out.append(f"area {entry.get('area')!r} is not one of {sorted(COMPONENT_AREAS)}")
    if not entry.get("purpose"):
        out.append("no purpose")
    status = entry.get("status")
    if status not in COMPONENT_STATUSES:
        out.append(f"status {status!r} is not one of {sorted(COMPONENT_STATUSES)}")
    if status == "out-of-1.0" and not (isinstance(entry.get("issue"), int) and entry["issue"] > 0):
        out.append("out-of-1.0 without an issue number")
    if status == "specified" and not (entry.get("spec") and entry.get("item")):
        out.append("specified without both spec and item")
    pending = False
    spec = entry.get("spec")
    if spec and not (root / "docs/architecture" / spec).is_file():
        if entry.get("branch"):
            pending = True
        else:
            out.append(f"{spec} is absent from docs/architecture")
    return out, pending


def coverage_problems(root=None, files=None):
    """Failures and warnings of the component inventory against the tree. A component
    without a specification (status missing) is a warning, not a failure."""
    root = Path(root or ROOT)
    if files is None:
        git = subprocess.run(["git", "-C", str(root), "ls-files", "--cached", "-z"],
                             capture_output=True, text=True)
        if git.returncode:
            return [f"cannot list the repository files: {git.stderr.strip()}"], []
        files = [f for f in git.stdout.split("\0") if f]
    try:
        entries = tomllib.loads(read(root / COMPONENTS)).get("component", [])
    except FileNotFoundError:
        return [f"{COMPONENTS}: missing"], []
    except tomllib.TOMLDecodeError as e:
        return [f"{COMPONENTS}: unreadable ({e})"], []
    problems, missing, pending, seen = [], [], [], set()
    for n, entry in enumerate(entries, 1):
        path = entry.get("path")
        if not path:
            problems.append(f"{COMPONENTS}: entry {n} has no path")
            continue
        if path in seen:
            problems.append(f"{COMPONENTS}: {path}: listed twice")
        seen.add(path)
        found, waits = entry_problems(root, entry)
        problems += [f"{COMPONENTS}: {path}: {p}" for p in found]
        if waits:
            pending.append(f"{path} ({entry['spec']} on {entry['branch']})")
        if entry.get("status") == "missing":
            missing.append(path)
    for d in sorted(component_dirs(files) - seen):
        problems.append(f"{d}: component directory without an entry in {COMPONENTS}")
    notes = []
    if missing:
        notes.append(f"{len(missing)} components have no specification (status missing): "
                     + ", ".join(missing))
    if pending:
        notes.append(f"{len(pending)} entries cite a specification not merged here yet: "
                     + ", ".join(pending))
    return problems, notes


@check("coverage", "Every component has an inventory entry, and every entry a spec or an issue")
def check_coverage():
    r = Result()
    problems, notes = coverage_problems()
    for problem in problems:
        r.fail(problem)
    for note in notes:
        r.note(note)
    return r


BUS_API_MESH_IDENT = re.compile(r"\b(?:mesh\w*|\w*mesh\w*|NodeIdentityPayload|AiDecisionPayload|KernelTelemetry)\b", re.I)
BUS_API_DENIED_DEPS = ("x25519-dalek", "ed25519-dalek", "ring", "rand_core", "zeroize", "subtle")
BUS_API_DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def bus_api_residue(root):
    """The mesh and post-quantum residue ADR-0103 D10 deletes from athanor-bus-api.

    Only lib.rs and polkit.rs may exist under src/, deliberately: the crate is the shared
    polkit client and nothing else (ADR-0103 D10). Any other file in the crate, tests/ and
    build.rs included, is searched for mesh identifiers (case-insensitive), and every
    dependency table of Cargo.toml (target-specific ones too) is checked against a deny-list."""
    crate = root / "system" / "athanor-bus-api"
    problems = []
    for f in sorted(crate.rglob("*.rs")):
        rel = f.relative_to(root)
        if f.parent == crate / "src" and f.name in ("lib.rs", "polkit.rs"):
            pass
        elif f.parent == crate / "src" or "src" in f.relative_to(crate).parts:
            problems.append(f"{rel}: module outside lib.rs and polkit.rs (ADR-0103 D10)")
        for m in sorted({m.group(0) for m in BUS_API_MESH_IDENT.finditer(f.read_text())}):
            problems.append(f"{rel}: {m} is mesh or post-quantum residue (ADR-0103 D10)")
    manifest = crate / "Cargo.toml"
    if manifest.exists():
        data = tomllib.loads(manifest.read_text())
        tables = [data.get(t, {}) for t in BUS_API_DEP_TABLES]
        for target in data.get("target", {}).values():
            tables += [target.get(t, {}) for t in BUS_API_DEP_TABLES]
        for dep in sorted({d for t in tables for d in t}):
            if dep in BUS_API_DENIED_DEPS or dep.startswith("pqcrypto"):
                problems.append(f"{manifest.relative_to(root)}: dependency {dep} is post-quantum residue (ADR-0103 D10)")
    return problems


@check("bus-api-residue", "athanor-bus-api holds no mesh or post-quantum residue (ADR-0103 D10)")
def check_bus_api_residue():
    r = Result()
    for problem in bus_api_residue(ROOT):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# zbus-replies: an object-server method does not await a reply on its own connection
# --------------------------------------------------------------------------- #

ZBUS_INTERFACE = re.compile(r"#\[(?:zbus::)?interface\b")
ZBUS_CONNECTION_ARG = re.compile(r"#\[zbus\(connection\)\]\s*(\w+)\s*:")
# What a method may do with its connection without waiting for a reply: build an emitter, name
# the object server, emit.
ZBUS_NO_REPLY_BEFORE = re.compile(r"(?:SignalEmitter::new|\bemit_\w+|\bbar_emitter\([^()]*,)\s*\(?\s*&?$")
ZBUS_NO_REPLY_AFTER = re.compile(r"\s*\.(?:object_server|unique_name|emit_signal|request_name|release_name)\b")


def matching(txt, at, open_, close):
    """The index just past the bracket that closes the one at `txt[at]`, or len(txt)."""
    depth = 0
    for i in range(at, len(txt)):
        depth += txt[i] == open_
        depth -= txt[i] == close
        if depth == 0:
            return i + 1
    return len(txt)


def zbus_replies(files, root):
    """Findings for the Rust `files` (paths under `root`): a method of an `#[interface]` impl
    that takes `#[zbus(connection)] conn` and awaits a statement that passes `conn` on, or calls
    on it. zbus 5.19 reads one socket per connection and hands each message to every stream
    with a queue of 64. A handler that awaits a reply on that connection while a stream of it
    fills stops the reader, so the reply is never read and the wait is for good
    (issue #1035). The fix is a second connection that serves nothing, subscribes to nothing and
    owns no name, with each query under `tokio::time::timeout`; a method then reads it from
    `self`, not from the argument.

    A source scan, not a type check. It follows no helper: a statement that awaits and names the
    argument is a finding, except an emitter (`SignalEmitter::new`, `emit_*`, `bar_emitter`) or
    `conn.object_server()`, `.unique_name()` and the name calls. A connection reached through
    `self`, and a loop over a stream of a connection whose body awaits a call on that
    connection, are not seen. Code under `#[cfg(test)]`, tests, benches and examples is skipped.
    """
    problems = []
    for path in files:
        txt = without_literals(without_test_items(read(path)))
        for iface in ZBUS_INTERFACE.finditer(txt):
            brace = txt.find("{", iface.end())
            if brace < 0:
                continue
            block = txt[brace:matching(txt, brace, "{", "}")]
            for fn in re.finditer(r"\bfn\s+(\w+)[^(]*\(", block):
                params_end = matching(block, fn.end() - 1, "(", ")")
                arg = ZBUS_CONNECTION_ARG.search(block, fn.end(), params_end)
                body_start = block.find("{", params_end)
                if not arg or body_start < 0:
                    continue
                body = block[body_start:matching(block, body_start, "{", "}")]
                name = re.escape(arg.group(1))
                for use in re.finditer(rf"(?<![\w.]){name}\b", body):
                    start = max(body.rfind(c, 0, use.start()) for c in ";{}") + 1
                    end = body.find(";", use.end())
                    statement = body[start:end if end >= 0 else len(body)]
                    if ".await" not in statement:
                        continue
                    if ZBUS_NO_REPLY_BEFORE.search(body[start:use.start()]) or ZBUS_NO_REPLY_AFTER.match(body, use.end()):
                        continue
                    at = txt[:brace + body_start + use.start()].count("\n") + 1
                    problems.append(f"{Path(path).relative_to(root)}:{at}: `{fn.group(1)}` awaits a reply on the connection that serves it")
                    break
    return problems


@check("zbus-replies", "an object-server method awaits no reply on the connection that serves it (issue #1035)")
def check_zbus_replies():
    r = Result()
    files = list(rust_files())
    skip = test_module_files(files)
    for problem in zbus_replies([p for p in files if not is_test_file(p) and p not in skip and not vendored(rel(p))], ROOT):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# landlock-proc: no Landlock read grant names /proc
# --------------------------------------------------------------------------- #

# A Landlock grant in Rust source: `athanor_unit::sandbox::restrict`, a crate's own copy, or a
# rule built directly.
LANDLOCK_GRANT = re.compile(r"\brestrict\s*\(|\bPathBeneath::new\b")
# /proc itself, or the directory of this process or thread: a literal a grant can name.
PROC_LITERAL = re.compile(r'"/proc(?:/(?:self|thread-self))?/?"')


def crate_of(path, root):
    """The directory of the nearest Cargo.toml above `path`, up to `root`, or None."""
    for parent in Path(path).parents:
        if (parent / "Cargo.toml").is_file():
            return parent
        if parent == root:
            return None
    return None


def landlock_proc_problems(root):
    """A crate that confines itself with Landlock names no /proc in its non-test sources. Landlock
    grants a whole tree and cannot narrow /proc to one file of one process, so a read grant of
    /proc reads the environ, cmdline and maps of every process of the user, where tokens live
    (issue #1047). A caller's unit comes from the bus and systemd (athanor_unit::bus_caller);
    a read of /proc before the ruleset, such as the thread count, names a deeper path and passes.

    A source scan, not a type check: a crate with a grant call anywhere is scanned whole, since
    its grants may be listed in one file and applied in another. Comments, code under
    `#[cfg(test)]`, tests, benches and examples are skipped."""
    root = Path(root)
    problems = []
    for base in RS_DIRS:
        files = list(walk(root / base, ".rs"))
        skip = test_module_files(files)
        crates = {}
        for path in sorted(files):
            relative = path.relative_to(root).as_posix()
            if is_test_file(path) or path in skip or vendored(relative):
                continue
            # Comment lines blanked, not dropped, so that line numbers hold.
            code = "\n".join("" if line.lstrip().startswith("//") else line
                             for line in without_test_items(read(path)).split("\n"))
            crates.setdefault(crate_of(path, root), []).append((relative, code))
        for sources in crates.values():
            if not any(LANDLOCK_GRANT.search(code) for _, code in sources):
                continue
            for relative, code in sources:
                for i, line in enumerate(code.split("\n"), 1):
                    if PROC_LITERAL.search(line):
                        problems.append(f"{relative}:{i}: a Landlock-confined crate names /proc; "
                                        f"ask the bus and systemd for a caller's unit "
                                        f"(athanor_unit::bus_caller), never grant /proc")
    return problems


@check("landlock-proc", "No Landlock read grant of a daemon names /proc (issue #1047)")
def check_landlock_proc():
    r = Result()
    for problem in landlock_proc_problems(ROOT):
        r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# agent-docs: the instructions agents read name things that exist
# --------------------------------------------------------------------------- #

# ADR-0074 holds the root guide to about 100 lines; Claude Code loads every other one whole.
AGENT_DOC_LINES = {"AGENTS.md": 100}
AGENT_DOC_DEFAULT_LINES = 60
CODE_TOKEN = re.compile(r"`([^`\n]+)`")
LINE_SUFFIX = re.compile(r"(?::\d+(?:-\d+)?)?(?:#\S*)?$")
VERIFY_CITATION = re.compile(r"verify\.py((?:[ \t]+[a-z][a-z0-9-]*)+)")
RULE_GLOB = re.compile(r"""^\s*-\s*["']?([^"'#]+?)["']?\s*$""")


def agent_docs(tracked):
    """The tracked files agents load as instructions: every AGENTS.md and CLAUDE.md, and the
    Markdown under .claude/ but its skills, which are tools, not rules."""
    return sorted(f for f in tracked
                  if Path(f).name in ("AGENTS.md", "CLAUDE.md")
                  or (f.startswith(".claude/") and f.endswith(".md")
                      and not f.startswith(".claude/skills/")))


def glob_regex(glob):
    """A compiled regex for a path glob as Claude Code's `paths:` reads it: `**/` is any number
    of directories, `*` and `?` stay inside one, `{a,b}` is either."""
    out, i = [], 0
    while i < len(glob):
        if glob.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif glob.startswith("**", i):
            out.append(".*")
            i += 2
        elif glob[i] == "*":
            out.append("[^/]*")
            i += 1
        elif glob[i] == "?":
            out.append("[^/]")
            i += 1
        elif glob[i] == "{" and "}" in glob[i:]:
            end = glob.index("}", i)
            out.append("(?:" + "|".join(map(re.escape, glob[i + 1:end].split(","))) + ")")
            i = end + 1
        else:
            out.append(re.escape(glob[i]))
            i += 1
    return re.compile("".join(out))


def path_exists(path, files, dirs):
    if any(c in path for c in "*?{"):
        pattern = glob_regex(path)
        return any(pattern.fullmatch(f) for f in files) or any(pattern.fullmatch(d) for d in dirs)
    return path in files or path in dirs


def agent_doc_problems(doc, text, tracked, ignored, checks):
    """What an agent document gets wrong about the tree: a code span naming a repository path
    that is neither tracked nor git-ignored, a `paths:` glob of a rule that matches no tracked
    file, a `verify.py <name>` naming no check, and a document over its line budget. A span is
    a path only when its first segment is an entry of the root or of the document's directory:
    `origin/iso-v0` or `actions/checkout` are not."""
    files = set(tracked)
    dirs = {str(parent) for f in files for parent in Path(f).parents if str(parent) != "."}
    here = Path(doc).parent
    roots = {Path(f).parts[0] for f in files}
    local = {Path(f).relative_to(here).parts[0] for f in files
             if here != Path(".") and Path(f).is_relative_to(here) and f != doc}
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    problems = []

    budget = AGENT_DOC_LINES.get(doc, AGENT_DOC_DEFAULT_LINES)
    if len(lines) > budget:
        problems.append(f"{doc}: {len(lines)} lines, the budget is {budget}")

    if doc.startswith(".claude/rules/") and lines and lines[0].strip() == "---":
        in_paths = False
        for n, line in enumerate(lines[1:], 2):
            if line.strip() == "---":
                break
            if not line.startswith((" ", "\t", "-")):
                in_paths = line.split(":")[0].strip() == "paths"
                continue
            m = RULE_GLOB.match(line)
            if in_paths and m and not path_exists(m.group(1), files, set()):
                problems.append(f"{doc}:{n}: paths glob {m.group(1)} matches no tracked file")

    fence = None
    for n, line in enumerate(lines, 1):
        m = FENCE.match(line)
        if fence is None and m:
            fence = m.group(1)
            continue
        if fence is not None:
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence):
                fence = None
                continue
            code = [line.split(" #")[0]]
        else:
            code = CODE_TOKEN.findall(line)
            for token in code:
                token = LINE_SUFFIX.sub("", token.strip())
                if (not token or "/" not in token or any(c in token for c in " <>=$@")
                        or token.startswith(("/", "~", "-", "http"))):
                    continue
                first = token.split("/")[0]
                candidates = ([token] if first in roots else []) \
                    + ([str(here / token)] if first in local else [])
                if candidates and not any(path_exists(c.rstrip("/"), files, dirs) or ignored(c)
                                          for c in candidates):
                    problems.append(f"{doc}:{n}: {token} is neither tracked nor git-ignored")
        for piece in code:
            for m in VERIFY_CITATION.finditer(piece):
                for name in m.group(1).split():
                    if name not in checks:
                        problems.append(f"{doc}:{n}: verify.py has no check named {name}")
    return problems


@check("agent-docs", "The agent instructions name paths, rule globs and checks that exist")
def check_agent_docs():
    r = Result()
    listed = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], capture_output=True, text=True)
    if listed.returncode:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    tracked = [f for f in listed.stdout.split("\0") if f]

    def ignored(path):
        return subprocess.run(["git", "-C", str(ROOT), "check-ignore", "-q", "--no-index", path],
                              capture_output=True).returncode == 0

    for doc in agent_docs(tracked):
        if not (ROOT / doc).is_file():
            r.fail(f"{doc}: tracked but missing from the worktree")
            continue
        for problem in agent_doc_problems(doc, read(ROOT / doc), tracked, ignored, set(CHECKS)):
            r.fail(problem)
    return r


# --------------------------------------------------------------------------- #
# line-citations: a `path:N` in an architecture document still names what the sentence names
# --------------------------------------------------------------------------- #

# What a backtick span cites: `path:N`, `path:N-M`, `path:N, M-K`, or `:N` for the path named last.
LINE_CITATION = re.compile(r"([\w.+-]+(?:/[\w.+-]+)*)?:(\d+(?:-\d+)?(?:,\s*\d+(?:-\d+)?)*)")
# What a sentence names in code: a backticked identifier (hyphens and a dotted suffix allowed:
# `cosmic-osd.service`), or a bare snake_case or `::` path.
CITED_TOKEN = re.compile(r"(?<=`)[A-Za-z_][\w.-]*(?:::\w+)*(?=`)|(?<![\w`/.-])[A-Za-z]\w*(?:_\w+|::\w+)+(?![\w/.`-])")
FILE_LIKE = re.compile(r"/|\.(?:rs|toml|py|md|json|sh|spec|ya?ml|ks|target|conf|lock|txt|ron)$")
CRATE_ROOT = re.compile(r"^(.+?)/(?:src|data|tests|benches)/")
# The length from which a range is too long for one citation: such a range holds almost any word, so it proves nothing.
LINE_RANGE_LIMIT = 200
# A markdown line that holds no words: blank, or a table's rule (`| --- | --- |`).
EMPTY_LINE = re.compile(r"[\s|:-]*")
# The row a sentence names in a table of a cited document: `Archives row`.
ROW_NAME = re.compile(r"\b([A-Z][a-z]+) row\b")
# The id of a section of a specification (`SH3`, `BR2`); a section starts at its `**BR2.` line and
# ends at the next such line or heading.
SECTION_ID = re.compile(r"\b[A-Z]{1,4}\d+\b")
SECTION_START = re.compile(r"\*\*[A-Z]{1,4}\d+[a-z]?\b|#{1,3} ")
# A path with a directory and an extension written in plain text, with or without its `:N` span
# (`forge/config/packages.json:128`): read as if it were in backticks.
PLAIN_PATH = re.compile(r"(?<![\w/.`:-])((?:[\w.+-]+/)+[\w.+-]*\w\.[A-Za-z]+"
                        r"(?::\d+(?:-\d+)?(?:,\s*\d+(?:-\d+)?)*)?)(?![\w/`-])")
# The files whose entries are named by words that are rarely in backticks: a citation of one
# that names nothing the check can match is a finding, and its hyphenated lowercase words
# (`cosmic-osd`, not in backticks) count as tokens when the file holds them.
LIST_FILES = {"packages.json", "Cargo.toml"}
LIST_GAP = re.compile(r"[\s,`]*(?:and|or)?[\s,`]*")
# The verb between a list of entries and the key that holds them: `are in` takes the list before it,
# `is in` one name only, so "`x` is in `a` and `y` is in `b`" does not join `a` to `b`.
LIST_KEY_GAP = re.compile(r"[\s`]*(are|is)(?:\s+listed)?\s+(?:in|under)[\s`]*")
# A name that such a verb precedes is the key of an earlier clause, where a run that crossed one stops.
LIST_KEY_BEFORE = re.compile(r"\b(?:are|is)(?:\s+listed)?\s+(?:in|under)[\s`]*$")
BARE_NAME = re.compile(r"(?<![\w`/.-])[a-z][a-z0-9]*(?:-[a-z0-9]+)+(?![\w/`-])")


def backtick_plain_paths(line):
    """`line` with each path written outside backticks put in backticks."""
    parts = line.split("`")
    for i in range(0, len(parts), 2):
        parts[i] = PLAIN_PATH.sub(lambda m: f"`{m.group(1)}`", parts[i])
    return "`".join(parts)


def section_lines(rows, ident):
    """The first and the last line (from 1) of the section `ident` of a document, or None."""
    for first, row in enumerate(rows, 1):
        if row.startswith(f"**{ident}."):
            end = next((n for n in range(first + 1, len(rows) + 1) if SECTION_START.match(rows[n - 1])),
                       len(rows) + 1)
            return first, end - 1
    return None

# Findings the heuristic cannot settle, each (document, cited path, cited lines, token, n) with the
# reason: one entry covers one citation, the n-th (from 1) of the document that has the same path,
# lines and token. An entry that matches no finding fails, so the list only shrinks.
LINE_CITATIONS_KEPT = {
    ("doc_tetragon.md", "system/Containerfile", "244", "tmpfiles.d", 1):
        "the sentence lists several changes; the citation belongs to `systemctl enable tetragon.service`",
    ("doc_software.md", "forge/config/packages.json", "2-84", None, 1):
        "the crate is absent from the lists: the citation names the lists, and the entry is not in them",
}
# Another project's file that a document cites by a path a tracked file also ends with, each
# (document, path as written) with the project it belongs to: the path resolves to no file, sets no
# crate, and a `:N` after it cites nothing. An entry whose path no citation of its document writes
# fails, so the list only shrinks.
LINE_CITATIONS_UPSTREAM = {
    ("doc_osd.md", "src/wayland.rs"): "cosmic-settings-daemon's, not athanor-idle's",
    ("doc_settings.md", "src/state.rs"): "cosmic-comp 1.8.0's, not athanor-sulphur's",
}


def holds_token(text, token, quoted=False):
    """Whether `text` has `token` as a whole identifier (the last segment of a `A::b` path): not
    inside `open_layer` or `reopen`, but in `fn open` and in a comment that says open. With
    `quoted` (a JSON file) the token must be a whole string: `"cosmic-settings"`, not
    `cosmic-settings-daemon` or `cosmic-osd.service`."""
    if quoted:
        return f'"{token}"' in text
    word = re.escape(token.rsplit("::", 1)[-1])
    return re.search(rf"(?<![\w-]){word}(?![\w-])", text) is not None


HUNK_HEADER = re.compile(r"@@ -(\d+)(?:,(\d+))? \+\d+(?:,(\d+))? @@")
DIFF_FILE = re.compile(r"diff --git a/(.+) b/\1")


def diff_hunks(root, base):
    """The hunks of the two-tree diff of the working tree against revision BASE, as
    {path: [(first old line, old lines, new lines)]}: what `git diff -U0` prints, a count of 0 being
    an insertion after the old line (or a deletion before the new one). Raises ValueError when git
    cannot diff BASE."""
    diff = subprocess.run(["git", "-C", str(root), "-c", "core.quotepath=off", "diff", "-U0",
                           "--no-renames", "--no-ext-diff", "--no-textconv", "--no-color",
                           "--src-prefix=a/", "--dst-prefix=b/", base, "--"], capture_output=True, text=True)
    if diff.returncode != 0:
        raise ValueError(f"git diff {base} failed: {diff.stderr.strip()}")
    out, hunks = {}, None
    for line in diff.stdout.split("\n"):
        if m := DIFF_FILE.fullmatch(line):
            hunks = out.setdefault(m[1], [])
        elif hunks is not None and (m := HUNK_HEADER.match(line)):
            hunks.append((int(m[1]), 1 if m[2] is None else int(m[2]), 1 if m[3] is None else int(m[3])))
    if diff.stdout.strip() and not out:
        raise ValueError(f"git diff {base} printed no `diff --git a/X b/X` header: the hunks cannot be read")
    return out


# A backtick span that may cite lines: `path:120`, `path:3, 9-12`, or the short `:120`.
CITATION_SPAN = re.compile(r"`([^`\s]*):\d+(?:-\d+)?(?:,\s*\d+(?:-\d+)?)*`")


def without_citation_numbers(line, tracked):
    """LINE with the numbers of its citations blanked: a span whose path is a tracked file, and a short
    `:N` that follows such a span on the line. Any other `:N` (`fedora:43`, `quay.io:443`) is text.
    A `:N` after a tracked file is read as that file's citation, as the grammar says, so a change of
    such a span alone (a port after `src/x.rs:120`) is not an edit."""
    cited = False

    def blank(m):
        nonlocal cited
        if m[1] and not any(f == m[1] or f.endswith("/" + m[1]) for f in tracked):
            return m[0]
        if not m[1] and not cited:
            return m[0]
        cited = True
        return f"`{m[1]}:N`"

    return CITATION_SPAN.sub(blank, line)


def moved_ranges(ranges, hunks, same=None):
    """The ranges (first, last) of a citation, as the lines of the file are numbered after the
    HUNKS of `diff_hunks`; None when a hunk edited or removed one of the cited lines, which a reader
    must place again. `same(old, new)`, given the number of a cited line before and after, says that
    an edit that kept the line count left the line as it was: a line that only renumbers citations."""
    def shifted(n):
        return n + sum(new - old for first, old, new in hunks if (first + old - 1 if old else first) < n)

    def placed(first, old, new, start, last):
        return bool(same) and old == new and all(
            same(n, shifted(n)) for n in range(max(start, first), min(last, first + old - 1) + 1))

    if any(old and first <= last and first + old - 1 >= start and not placed(first, old, new, start, last)
           for start, last in ranges for first, old, new in hunks):
        return None
    return [(shifted(start), shifted(last)) for start, last in ranges]


# What a backtick span cites by name: `path#symbol`, or `#symbol` for the path named last. A colour
# (`#e62d42`, the placeholder `#rrggbb`) is no citation.
SYMBOL_CITATION = re.compile(r"([\w.+-]+(?:/[\w.+-]+)*)?#([A-Za-z_][^`]*)")
SCHEME = re.compile(r"[A-Za-z][\w+.-]*:")
COLOUR = re.compile(r"#(?:[0-9A-Fa-f]{3,8}|r+g+b+a*)")
# The files whose citations stay `path:N`, each with its reason: a file with no symbol to name (a
# data file, a generated list). An entry that no `path:N` citation names fails, so the list only shrinks.
CITATION_LINES_KEPT = {}
# Whether the documents cite by symbol (#1278). Until the migration sets it, `line-citations` checks
# every `path:N` and symbol-citations only the `path#symbol` form; once set, symbol-citations also
# checks `path:N`, which only a file of CITATION_LINES_KEPT may take.
CITATION_LINES_RETIRED = False

RUST_ITEM = re.compile(r"(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe|default|extern(?:\s+\"[^\"]*\")?)\s+)*"
                       r"(fn|struct|enum|union|trait|type|const|static|mod|macro_rules!)\s+(?:mut\s+)?(?:r#)?([A-Za-z_]\w*)")
RUST_IMPL = re.compile(r"(\s*)(?:unsafe\s+)?impl\b(.*)")
SHELL_FUNCTION = re.compile(r"(\s*)(?:function\s+([\w:.-]+)\s*(?:\(\s*\))?|([\w:.-]+)\s*\(\s*\))\s*\{?\s*(?:#.*)?$")
TOML_KEY = r"(?:[\w-]+|\"[^\"]*\"|'[^']*')(?:\s*\.\s*(?:[\w-]+|\"[^\"]*\"|'[^']*'))*"
TOML_HEADER = re.compile(rf"\[\[?\s*({TOML_KEY})\s*\]\]?\s*(?:#.*)?")
TOML_ASSIGN = re.compile(rf"({TOML_KEY})\s*=(.*)")
MARKDOWN_SECTION = re.compile(r"\*\*([A-Z]{1,4}\d+[a-z]?)\.")
MARKDOWN_HEADING = re.compile(r"(#{1,6})\s+(.*?)\s*#*\s*")


def qualified(chain, name, sep):
    """The spellings that select `name` nested in the containers `chain`: `name`, then each longer
    suffix of `chain::name`."""
    return {sep.join([*chain[k:], name]) for k in range(len(chain) + 1)}


def indent_of(row):
    return len(row) - len(row.lstrip())


def rust_definitions(rows):
    """The items of a Rust file, each ({spellings}, first line, last line). An item (`fn`, `struct`,
    `enum`, `union`, `trait`, `type`, `const`, `static`, `mod`, `macro_rules!`) is spelled by its
    name, or qualified by the `mod`, `trait` and `impl` blocks around it (`Lock::new`, an impl being
    named by its type); an impl block is spelled as its header, without the impl's own parameters
    (`impl Lock`, `impl Drop for Lock`, `impl Dispatch<WlSurface, ()> for Client`).
    An item starts at the attributes and comments right above it and ends on its line when that line
    ends with `;` or `}`, else at the next line of its indent that starts with `}`, `)` or `]` and
    opens no block. Limit: rustfmt's layout is assumed, and an item-like line inside a string
    (a raw-string fixture) counts as an item."""
    found = []
    for i, row in enumerate(rows):
        if (m := RUST_ITEM.match(row)):
            kind, name = m.group(2), m.group(3)
        elif (m := RUST_IMPL.match(row)):
            head = re.split(r"\{|\bwhere\b", m.group(2))[0].strip()
            depth = 0
            for k, char in enumerate(head if head.startswith("<") else ""):
                depth += {"<": 1, ">": -1}.get(char, 0)
                if depth == 0:
                    head = head[k + 1:]
                    break
            kind, name = "impl", " ".join(head.split())
            if not name:
                continue
        else:
            continue
        indent = len(m.group(1))
        first = i
        while first > 0 and indent_of(rows[first - 1]) == indent:
            above = rows[first - 1].strip()
            if above.startswith(("#[", "//")) and not above.startswith("//!"):
                first -= 1
            elif above in (")]", "]"):
                opener = next((k for k in range(first - 2, -1, -1) if indent_of(rows[k]) == indent
                               and rows[k].strip().startswith("#[")), None)
                if opener is None:
                    break
                first = opener
            else:
                break
        last = i
        if not row.rstrip().endswith((";", "}")):
            for j in range(i + 1, len(rows)):
                text = rows[j].strip()
                if not text:
                    continue
                if indent_of(rows[j]) < indent and text[0] in "})]":
                    break
                if indent_of(rows[j]) == indent:
                    if text[0] in "})]" and not text.endswith("{"):
                        last = j
                        break
                    if not text.startswith(("where", "{", ")", "]", "->")):
                        break
                last = j
        found.append((kind, name, indent, first + 1, i + 1, last + 1))
    out = []
    for kind, name, indent, first, line, last in found:
        chain = [(re.sub(r"<.*", "", target.rsplit(" for ", 1)[-1]).split("::")[-1].strip("&* ") if k == "impl" else target)
                 for k, target, _, _, start, end in found
                 if k in ("impl", "trait", "mod") and start < line <= end]
        out.append(({f"impl {name}"} if kind == "impl" else qualified(chain, name, "::"), first, last))
    return out


def python_definitions(body):
    """The functions and classes of a Python file, spelled by their name or qualified by the classes
    and functions around them (`Result.fail`), with their decorators; and the names a module-level
    assignment binds. Raises ValueError when the file does not parse."""
    try:
        tree = ast.parse(body)
    except SyntaxError as e:
        raise ValueError(f"does not parse as Python (line {e.lineno})") from e
    out = []

    def walk(nodes, chain):
        for node in nodes:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                first = min([node.lineno, *(d.lineno for d in node.decorator_list)])
                out.append((qualified(chain, node.name, "."), first, node.end_lineno))
                walk(node.body, [*chain, node.name])
            elif not chain and isinstance(node, (ast.Assign, ast.AnnAssign)):
                for target in node.targets if isinstance(node, ast.Assign) else [node.target]:
                    if isinstance(target, ast.Name):
                        out.append(({target.id}, node.lineno, node.end_lineno))

    walk(tree.body, [])
    return out


def shell_definitions(rows):
    """The functions of a shell script (`name() {`, `function name {`), from the comments right above
    one to the next line of its indent that starts with `}`."""
    out = []
    for i, row in enumerate(rows):
        if not (m := SHELL_FUNCTION.match(row)) or row.lstrip().startswith("#"):
            continue
        indent = len(m.group(1))
        first = i
        while first > 1 and indent_of(rows[first - 1]) == indent and rows[first - 1].lstrip().startswith("#"):
            first -= 1
        last = i if row.rstrip().endswith("}") else next(
            (j for j in range(i + 1, len(rows)) if indent_of(rows[j]) == indent
             and rows[j].strip().startswith("}")), len(rows) - 1)
        out.append(({m.group(2) or m.group(3)}, first + 1, last + 1))
    return out


def toml_definitions(rows):
    """The tables and keys of a TOML file, spelled by their full dotted path (`workspace.members`):
    a table from its header to the line before the next header, a key from its line to the end of
    its value."""
    def path(raw):
        return ".".join(p.strip().strip("\"'") for p in re.findall(r"\"[^\"]*\"|'[^']*'|[^.]+", raw))

    out, table, depth, quote, key = [], "", 0, None, None
    for n, row in enumerate(rows, 1):
        text = row.strip()
        resumed = bool(quote)
        if quote:
            if quote not in text:
                continue
            text, quote = text.split(quote, 1)[1], None  # what follows the closing quotes counts as brackets
        if depth == 0 and not resumed:
            if (m := TOML_HEADER.fullmatch(text)):
                table = path(m.group(1))
                out.append([{table}, n, n])
                continue
            if not (m := TOML_ASSIGN.match(text)) or text.startswith("#"):
                continue
            key = [{f"{table}.{path(m.group(1))}" if table else path(m.group(1))}, n, n]
            out.append(key)
            text = m.group(2)
        bare = re.sub(r"\"\"\"|'''", "\0", text)
        bare = re.sub(r"\"(?:\\.|[^\"\\])*\"|'[^']*'", "", bare)
        bare = bare.split("#", 1)[0]
        if bare.count("\0") % 2:
            quote = '"""' if '"""' in text else "'''"
        depth += bare.count("[") + bare.count("{") - bare.count("]") - bare.count("}")
        if key and depth == 0 and not quote:
            key[2], key = n, None
    headers = [first for names, first, last in out if first == last and TOML_HEADER.fullmatch(rows[first - 1].strip())]
    for entry in out:
        if entry[1] in headers:
            end = next((h - 1 for h in headers if h > entry[1]), len(rows))
            while end > entry[1] and not rows[end - 1].strip():
                end -= 1
            entry[2] = end
    return [tuple(entry) for entry in out]


def yaml_definitions(body):
    """The keys of a YAML file, spelled by their dotted path (`jobs.build`), an item of a list that
    has a `name` spelled by it (`jobs.build.steps[name=Sign]`), each from its line to the end of its
    value. Raises ValueError without PyYAML or when the file does not parse."""
    if yaml is None:
        raise ValueError("needs PyYAML to read the symbols of a YAML file")
    try:
        root = yaml.compose(body)
    except yaml.YAMLError as e:
        raise ValueError(f"does not parse as YAML: {e}") from e
    out, rows = [], body.split("\n")

    def end(node, first):
        # A block value ends where the next token starts: on the line before it when only blanks
        # precede that token on its line; trailing blank and comment lines are left out.
        mark = node.end_mark
        last = mark.line if mark.line >= len(rows) or not rows[mark.line][:mark.column].strip() else mark.line + 1
        while last > first and rows[last - 1].strip()[:1] in ("", "#"):
            last -= 1
        return last

    def walk(node, prefix):
        if isinstance(node, yaml.MappingNode):
            for key, value in node.value:
                if isinstance(key, yaml.ScalarNode):
                    name = f"{prefix}.{key.value}" if prefix else key.value
                    out.append(({name}, key.start_mark.line + 1, end(value, key.start_mark.line + 1)))
                    walk(value, name)
        elif isinstance(node, yaml.SequenceNode):
            for item in node.value:
                titles = [v.value for k, v in item.value if k.value == "name" and isinstance(v, yaml.ScalarNode)
                          ] if isinstance(item, yaml.MappingNode) else []
                if titles:
                    name = f"{prefix}[name={titles[0]}]"
                    out.append(({name}, item.start_mark.line + 1, end(item, item.start_mark.line + 1)))
                    walk(item, name)

    walk(root, "")
    return out


def markdown_slug(heading):
    """The anchor GitHub gives a heading: lower case, punctuation dropped, spaces as hyphens."""
    return re.sub(r"[^\w\- ]", "", heading.strip().lower()).replace(" ", "-")


def markdown_definitions(rows):
    """The sections of a markdown document: a `**SH3.` section, to the next such line or heading of
    level 1 to 3 (as section_lines reads it), spelled by its id; and a heading, to the next heading
    of its level or above, spelled by its GitHub anchor and by the id it starts with (`CT3.`). Lines
    in a fenced block are no heading."""
    heads, fenced = [], False
    for n, row in enumerate(rows, 1):
        if row.lstrip().startswith(("```", "~~~")):
            fenced = not fenced
        elif not fenced and (m := MARKDOWN_HEADING.fullmatch(row)):
            heads.append((n, len(m.group(1)), m.group(2)))
    out = []
    for i, (n, level, title) in enumerate(heads):
        end = next((h - 1 for h, lv, _ in heads[i + 1:] if lv <= level), len(rows))
        names = {markdown_slug(title)}
        if (m := re.match(r"([A-Z]{1,4}\d+[a-z]?)\.", title)):
            names.add(m.group(1))
        out.append((names, n, end))
    starts = [n for n, row in enumerate(rows, 1) if SECTION_START.match(row)]
    for n, row in enumerate(rows, 1):
        if (m := MARKDOWN_SECTION.match(row)):
            out.append(({m.group(1)}, n, next((s - 1 for s in starts if s > n), len(rows))))
    trimmed = []
    for names, first, last in out:
        while last > first and not rows[last - 1].strip():
            last -= 1
        trimmed.append((names, first, last))
    return trimmed


def symbol_definitions(path, body):
    """The definitions a `path#symbol` citation may name, as [({spellings}, first line, last line)],
    by the kind of the file: Rust, Python, shell (by extension or `#!` line), TOML, YAML and
    markdown. None for a file of another kind, which has no symbol to name. Raises ValueError when
    the file cannot be read as its kind."""
    rows = body.split("\n")
    head = rows[0] if rows else ""
    if path.endswith(".rs"):
        return rust_definitions(rows)
    if path.endswith(".py") or re.match(r"#!.*python", head):
        return python_definitions(body)
    if path.endswith((".sh", ".bash")) or re.match(r"#!.*\b(?:ba|da|k|z)?sh\b|# shellcheck shell=", head):
        return shell_definitions(rows)
    if path.endswith(".toml"):
        try:
            tomllib.loads(body)
        except tomllib.TOMLDecodeError as e:
            raise ValueError(f"does not parse as TOML: {e}") from e
        return toml_definitions(rows)
    if path.endswith((".yml", ".yaml")):
        return yaml_definitions(body)
    if path.endswith(".md"):
        return markdown_definitions(rows)
    return None


def citation_findings(doc, text, tracked, read_file, hunks=None, base_text="", lines_kept=None, cited=None,
                      base_file=None, upstream=None):
    """The citations of the document's `text` that no longer name what they cite, as
    (line, path, lines, token, message); `token` is None when the finding is not a moved line.
    `tracked` is the repository's files, `read_file(path)` the text of one.

    A citation is `path:N`, `path:N-M` or a list `path:N, M-K`; `:N` alone cites the file named
    last in the same line, with or without a line of its own. A backtick span that is no file name
    (an identifier) leaves that file as it was; a file name that resolves to no file, or a
    citation of one, clears it, and a `:N` after that cites nothing and is not checked. A path resolves as written, then under the crate whose full path the line names
    before it, then by the one tracked file whose path ends with it; a name that several files
    end with (`Cargo.toml`) is not read. A path that starts with a top-level directory of the
    repository (read from `tracked`: `experimental/`, `.claude/`, `docs/`, ...) and resolves to no
    file is a finding, as is a bare `*.md` name that no tracked file ends with. A path that
    `upstream` ({(document, path)}, LINE_CITATIONS_UPSTREAM by default) lists for `doc` is another
    project's and resolves to no file. A range is no line range when it starts at 0, ends before it starts,
    reaches past the end of the file, or spans LINE_RANGE_LIMIT lines or more. A cited markdown
    document (`doc_x.md:N`) is read as any file; its first cited line must also hold words, a
    sentence that names a table row (`the Archives row`) names a word the cited lines must hold,
    and the section ids of the sentence (`BR2`) that the document has as a `**BR2.` line fix the
    sections the cited lines lie in (or name the id themselves, as a list of other documents does).

    The token is the nearest code token before the citation in its sentence (after the last `. `,
    `; `, `: `, `), ` or ` | `) that the cited file holds as a whole word; the cited lines must
    hold it, as a whole identifier. For a list the lines of all its entries count together: a list names the places of
    several words of one sentence (`packages.json:2, 51` for two lists), so the token cannot be
    attributed to one entry; each entry must still be a line range.

    A path written in plain text (`forge/config/packages.json:128`) is read as one in backticks. For
    `packages.json` and `Cargo.toml`, lists of entries, the check is stricter: the entries a
    citation names are the run of names next to it that only a comma, `and` or `or` separates (before
    it, else after it), and that reaches across an `are in` (the whole list) or an `is in` (one name)
    to the key nearest the citation; each must be in the cited lines, a name is a backticked
    identifier or a hyphenated lowercase word (`cosmic-osd`), a hyphenated name in backticks that
    the file no longer holds stays in the run and is a finding, a JSON name matches a whole string
    (`"cosmic-settings"`, not `cosmic-settings-daemon`), and a citation that names none is a finding.

    Limit: a sentence with no such token is not checked, nor is a citation that names a wrong
    place holding the token, and a word that the cited lines hold in a comment or a string
    satisfies the check as it does in code. A citation of another document with no token, no row
    name and no section id in its sentence is checked only for a first line that holds words, so
    a stale one that still lands on text goes unseen (#1089). In a list file, a run reaches the key
    only across the exact `are` or `is`, then an optional `listed`, then `in` or `under`: with other words between
    them ("are both in"), only the key is demanded, and a removed single-word entry is not seen.

    With `hunks` (diff_hunks against the revision the document was at, whose text is `base_text`),
    a citation on a line of the document that the change left as it was is also read through the
    hunks of its file: it fails with the lines it should name when the change moved them, and
    when the change edited or removed a cited line it fails for a reader to place. A line of
    the document that the change wrote or edited is the author's own placing, and is not read so:
    the limit: every citation on an edited line is skipped, also one the edit did not touch. A cited
    line of a Markdown file that the change edited without adding or removing lines, and whose only
    difference from the revision is the numbers of its own citations (`base_file(path)` is the file
    there), is not an edit.

    A citation by name is `path#symbol`, or `#symbol` for the file named last (a colour, `#e62d42`, is
    none unless the last file defines the symbol, `#add`; an `x#y` whose `x` is no file, a flake
    reference or a URL, is none and leaves the file named last as it was); its spec in a finding starts with `#`. The symbol must spell exactly one of the
    definitions that symbol_definitions reads in the file: none means it was renamed or removed,
    several that it must be qualified (`Lock::new`), and a file of a kind with no symbols cannot be
    cited so. The definition's lines are then read as cited lines, for the token and the sections
    of the sentence; a symbol does not move, so the hunks are not read for it, and its lines may
    span LINE_RANGE_LIMIT or more: a long definition holds more words, and proves less. A `#symbol`
    on a line that names no file before it is a finding; after a name that resolves to no file (code
    outside the repository), it is not checked. A `path#symbol` whose path is the end of several tracked
    files is a finding ("ambiguous"); a `path:N` of one is not. Trade-off: an upstream crate's
    `src/main.rs#main` is reported as ambiguous, and the fix is a longer path or no `#`; a path that
    `upstream` lists for `doc` is another project's, is not ambiguous, and its `#symbol` is not checked.

    With `lines_kept` (the files that keep `path:N`), a `path:N` of any other file is a finding, as
    is a bare `:N` on a line that names no file before it. `cited`, a set, gets the file of every
    `path:N`."""
    known = set(tracked)
    unchanged = {backtick_plain_paths(row) for row in base_text.split("\n")} if hunks else set()
    prefixes = tuple(sorted({f"{path.split('/')[0]}/" for path in known if "/" in path}))
    suffixes = {}
    for path in known:
        parts = path.split("/")
        for i in range(len(parts)):
            suffixes.setdefault("/".join(parts[i:]), []).append(path)

    foreign = {path for d, path in (LINE_CITATIONS_UPSTREAM if upstream is None else upstream) if d == doc}

    def resolve(name, crate):
        if name in foreign:
            return None
        for candidate in (f"{crate}/{name}" if crate else None, name):
            if candidate in known:
                return candidate
        found = suffixes.get(name, [])
        return found[0] if len(found) == 1 else None

    def names_a_file(name, crate):
        """NAME is a tracked file or the end of some tracked files' paths."""
        return name in suffixes or bool(crate and f"{crate}/{name}" in known)

    def defines(path, symbol):
        """The file PATH holds a definition spelled SYMBOL."""
        try:
            return path is not None and any(symbol in names for names, _, _ in symbol_definitions(
                path, read_file(path)) or ())
        except ValueError:
            return False

    def counts_as_named(name, crate):
        """NAME is a file of the repository, or code outside it that the document cites by a relative
        path (`glib/gmain.c`), whose lines a bare `:N` may follow; an absolute host path
        (`/usr/bin/x`) is neither."""
        return names_a_file(name, crate) or not name.startswith("/")

    def names_a_gone_file(name):
        """NAME is under a top-level directory of the repository, or is a bare `*.md` name that no
        tracked file ends with: if it resolves to nothing, the file was removed."""
        return name.startswith(prefixes) or (
            name.endswith(".md") and "/" not in name and not any(t.endswith("/" + name) for t in known))

    out = []
    for number, line in enumerate(text.split("\n"), 1):
        last = crate = None
        named_any = False
        line = backtick_plain_paths(line)
        for span in re.finditer(r"`([^`]+)`", line):
            m = LINE_CITATION.fullmatch(span.group(1))
            if not m:
                m = SYMBOL_CITATION.fullmatch(span.group(1))
                if m and (not (names_a_file(m[1], crate) or names_a_gone_file(m[1])) if m[1] else
                          COLOUR.fullmatch(span.group(1)) and not defines(last, m[2].strip())):
                    m = None  # a flake reference, or a colour that its file does not define
            if not m:
                head = span.group(1).partition("#")[0]
                if SCHEME.match(span.group(1)) or (
                        "#" in span.group(1) and not (names_a_file(head, crate) or names_a_gone_file(head))):
                    continue  # a URL or a flake reference, which is no file of the repository
                last = resolve(span.group(1), crate) if FILE_LIKE.search(span.group(1)) else last
                named_any = named_any or bool(FILE_LIKE.search(span.group(1)) and counts_as_named(span.group(1), crate))
                continue
            named, spec = m.groups()
            symbol = spec.strip() if m.re is SYMBOL_CITATION else None
            spec, sep = (f"#{symbol}", "") if symbol else (spec, ":")
            path = last if named is None else resolve(named, crate)
            if path is None and named and named not in foreign and names_a_gone_file(named):
                out.append((number, named, spec, None,
                            f"{doc}:{number}: `{named}{sep}{spec}` names a file that does not exist"))
            last = path
            if path is None:
                if symbol and named and named not in foreign and len(suffixes.get(named, ())) > 1:
                    out.append((number, named, spec, None, f"{doc}:{number}: `{named}{spec}` is ambiguous: "
                                "several files end with it, give a longer path"))
                if named is None and not named_any and (symbol or lines_kept is not None):
                    out.append((number, None, spec, None, f"{doc}:{number}: `{sep}{spec}` cites the file named "
                                "before it on its line, and the line names none: name its path"))
                named_any = named_any or (named is not None and counts_as_named(named, crate))
                continue
            named_any = True
            if (c := CRATE_ROOT.match(path)):
                crate = c.group(1)
            if not symbol and cited is not None:
                cited.add(path)
            if not symbol and lines_kept is not None and path not in lines_kept:
                out.append((number, path, spec, None, f"{doc}:{number}: `{path}:{spec}` cites lines: cite a "
                            f"symbol, `{path}#name`, or keep a file with no symbol in CITATION_LINES_KEPT"))
                continue
            body = read_file(path)
            if symbol:
                try:
                    defs = symbol_definitions(path, body)
                except ValueError as e:
                    out.append((number, path, spec, None, f"{doc}:{number}: `{path}{spec}`: the file {e}"))
                    continue
                ranges = [(f, e) for names, f, e in defs or () if symbol in names]
                problem = ("cites a file with no symbols: cite its lines, and keep the file in CITATION_LINES_KEPT"
                           if defs is None else "names no definition of its file: the symbol was renamed or removed"
                           if not ranges else f"names {len(ranges)} definitions: qualify it (`Type::name`)"
                           if len(ranges) > 1 else None)
                if problem:
                    out.append((number, path, spec, None, f"{doc}:{number}: `{path}{spec}` {problem}"))
                    continue
            else:
                ranges = []
                for part in re.split(r",\s*", spec):
                    first, _, end = part.partition("-")
                    ranges.append((int(first), int(end or first)))
            rows = body.split("\n")
            if rows[-1] == "":
                rows.pop()
            if not symbol and any(f < 1 or e < f or e > len(rows) or e - f + 1 >= LINE_RANGE_LIMIT for f, e in ranges):
                out.append((number, path, spec, None,
                            f"{doc}:{number}: `{path}:{spec}` is not a line range of its file "
                            f"(fewer than {LINE_RANGE_LIMIT} lines)"))
                continue
            if not symbol and line in unchanged and path in hunks:
                before = base_file(path).split("\n") if base_file else []
                new = moved_ranges(ranges, hunks[path], lambda old, now: path.endswith(".md")
                                   and old <= len(before) and now <= len(rows)
                                   and without_citation_numbers(before[old - 1], tracked)
                                   == without_citation_numbers(rows[now - 1], tracked))
                if new is None:
                    out.append((number, path, spec, "edited", f"{doc}:{number}: `{path}:{spec}` cites a line "
                                "that this change edited or removed: place it again"))
                    continue
                if new != ranges:
                    named = ", ".join(f"{a}-{b}" if "-" in part else str(a) for part, (a, b)
                                      in zip(re.split(r",\s*", spec), new))
                    out.append((number, path, spec, "moved", f"{doc}:{number}: `{path}:{spec}` is moved by "
                                f"this change: it should name `{path}:{named}`"))
                    continue
            if not symbol and path.endswith(".md") and EMPTY_LINE.fullmatch(rows[ranges[0][0] - 1]):
                out.append((number, path, spec, None,
                            f"{doc}:{number}: `{path}:{spec}` starts on a line with no words: the lines moved"))
                continue
            start = max(line.rfind(sep, 0, span.start()) for sep in (". ", "; ", ": ", "), ", " | "))
            own = {part.split(".")[0] for part in path.split("/")}
            listed = path.rsplit("/", 1)[-1] in LIST_FILES
            sentence = line[start + 1:span.start()]
            found = [(m.start(), m.end(), m.group(0)) for m in CITED_TOKEN.finditer(sentence)]
            if listed:
                found += [(m.start(), m.end(), m.group(0)) for m in BARE_NAME.finditer(sentence)]
            if path.endswith(".md"):
                found += [(m.start(1), m.end(1), m.group(1)) for m in ROW_NAME.finditer(sentence)]
            if path.endswith(".md"):
                spans = {i: section_lines(rows, i) for i in dict.fromkeys(SECTION_ID.findall(sentence))}
                spans = {i: lines for i, lines in spans.items() if lines}
                if spans:
                    low = min(first for first, _ in spans.values())
                    high = max(last for _, last in spans.values())
                    if any(not (low <= f and e <= high or any(holds_token("\n".join(rows[f - 1:e]), i)
                                                               for i in spans)) for f, e in ranges):
                        named = ", ".join(spans)
                        out.append((number, path, spec, named, f"{doc}:{number}: `{path}{sep}{spec}` is not in "
                                    f"the section of {named}, which its sentence names: the lines moved"))
                        continue
            quoted = path.endswith(".json")
            def usable(items):
                return [i for i in sorted(items) if not FILE_LIKE.search(i[2]) and i[2] not in own
                        and holds_token(body, i[2], quoted)]

            tokens = usable(found)
            if listed:
                # The items the citation names: the run of tokens next to it that only a comma,
                # `and` or `or` separates, reaching from the key nearest the citation across an
                # `are in` (`nix` and `nix-daemon` are in `upstream_core`) or, for one name, an
                # `is in`; with none before it, the run that follows it. A hyphenated name in
                # backticks stays in the run when the file no longer holds it, so a removed entry
                # is a finding.
                run, single, crossed = [], False, False
                for item in reversed(sorted({*tokens, *(
                        i for i in found if sentence[i[0] - 1:i[0]] == "`" == sentence[i[1]:i[1] + 1]
                        and BARE_NAME.fullmatch(i[2]) and i[2] not in own)})):
                    if run:
                        gap = sentence[item[1]:run[-1][0]]
                        key = len(run) == 1 and LIST_KEY_GAP.fullmatch(gap)
                        if single or not (key or LIST_GAP.fullmatch(gap)) or (
                                crossed and LIST_KEY_BEFORE.search(sentence[:item[0]])):
                            break
                        single = bool(key) and key.group(1) == "is"
                        crossed = crossed or bool(key)
                    run.append(item)
                if not run:
                    after = line[span.end():]
                    ends = [i for i in (after.find(". "), after.find("; ")) if i >= 0]
                    after = after[:min(ends)] if ends else after
                    items = usable([(m.start(), m.end(), m.group(0))
                                    for m in (*CITED_TOKEN.finditer(after), *BARE_NAME.finditer(after))])
                    for item in items:
                        if run and not LIST_GAP.fullmatch(after[run[-1][1]:item[0]]):
                            break
                        run.append(item)
                tokens = run
            else:
                tokens = tokens[-1:]
            held = "\n".join("\n".join(rows[f - 1:e]) for f, e in ranges)
            if listed and not tokens:
                out.append((number, path, spec, None, f"{doc}:{number}: `{path}{sep}{spec}` names no token "
                            "that the file holds: put the entry's name in backticks, or hyphenated, "
                            "in its sentence"))
            elif missing := next((t for _, _, t in tokens if not holds_token(held, t, quoted)), None):
                out.append((number, path, spec, missing, f"{doc}:{number}: `{path}{sep}{spec}` does not hold "
                            f"`{missing}`, which its sentence names: the lines moved"))
    return out


@check("line-citations", "A path:line citation of an architecture document holds the code its sentence names")
def check_line_citations():
    r = Result()
    listed = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], capture_output=True, text=True)
    if listed.returncode:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    tracked = {f for f in listed.stdout.split("\0") if f}

    def read_file(path):
        try:
            return read(ROOT / path)
        except OSError:
            return ""

    hunks = None
    if BASE_REF is None:
        r.note("no base revision (--known-red-base REF): the citations that the change moved were not checked")
    else:
        try:
            hunks = diff_hunks(ROOT, BASE_REF)
        except ValueError as e:
            r.fail(str(e))

    def base_text(doc):
        shown = subprocess.run(["git", "-C", str(ROOT), "show", f"{BASE_REF}:{doc.relative_to(ROOT).as_posix()}"],
                               capture_output=True, text=True)
        return shown.stdout if shown.returncode == 0 else ""  # a document the change adds has no base text

    def base_file(path):
        shown = subprocess.run(["git", "-C", str(ROOT), "show", f"{BASE_REF}:{path}"], capture_output=True, text=True)
        return shown.stdout if shown.returncode == 0 else ""  # a file the change adds has no base text

    seen = set()
    for doc in sorted([*(ROOT / "docs/architecture").glob("doc_*.md"),
                       ROOT / "docs/architecture/shell-features.md"]):
        counted = {}
        for _, path, spec, token, problem in citation_findings(
                doc.name, read(doc), tracked, read_file, hunks, base_text(doc) if hunks else "",
                base_file=base_file):
            if spec.startswith("#"):
                continue  # a citation by name, which symbol-citations checks
            n = counted[path, spec, token] = counted.get((path, spec, token), 0) + 1
            seen.add((doc.name, path, spec, token, n))
            if (doc.name, path, spec, token, n) not in LINE_CITATIONS_KEPT:
                r.fail(problem)
    for key in sorted(set(LINE_CITATIONS_KEPT) - seen):
        r.fail(f"LINE_CITATIONS_KEPT names {key}, which matches no finding: remove the entry")
    for problem in upstream_citation_problems(ROOT, tracked, LINE_CITATIONS_UPSTREAM):
        r.fail(problem)
    return r


def upstream_citation_problems(root, tracked, upstream):
    """The entries of `upstream` (LINE_CITATIONS_UPSTREAM) that do not hold: a document that is not
    under docs/architecture, a path that is itself a `tracked` file, which the entry would read
    as another project's, and a path that no citation of the document writes."""
    found = []
    for doc, path in sorted(upstream):
        name = f"LINE_CITATIONS_UPSTREAM names `{path}` of {doc}"
        try:
            text = read(root / "docs/architecture" / doc)
        except (OSError, UnicodeDecodeError) as e:
            found.append(f"{name}, a document that cannot be read ({e}): rename or remove the entry")
            continue
        if path in tracked:
            found.append(f"{name}, a file of this repository, not another project's: remove the entry")
        elif not re.search(rf"`{re.escape(path)}(?::[\d,\s-]+)?`", text):
            found.append(f"{name}, which no citation writes: remove the entry")
    return found


@check("symbol-citations", "A path#symbol citation of an architecture document names one definition, which holds the code its sentence names")
def check_symbol_citations():
    r = Result()
    listed = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], capture_output=True, text=True)
    if listed.returncode:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    tracked = {f for f in listed.stdout.split("\0") if f}

    def read_file(path):
        try:
            return read(ROOT / path)
        except OSError:
            return ""

    hunks = None
    if not CITATION_LINES_RETIRED:
        r.note("the documents still cite lines (CITATION_LINES_RETIRED): line-citations checks `path:N`")
    elif BASE_REF is None:
        r.note("no base revision (--known-red-base REF): the citations that the change moved were not checked")
    else:
        try:
            hunks = diff_hunks(ROOT, BASE_REF)
        except ValueError as e:
            r.fail(str(e))

    def base_text(doc):
        shown = subprocess.run(["git", "-C", str(ROOT), "show", f"{BASE_REF}:{doc.relative_to(ROOT).as_posix()}"],
                               capture_output=True, text=True)
        return shown.stdout if shown.returncode == 0 else ""  # a document the change adds has no base text

    cited = set()
    kept = CITATION_LINES_KEPT if CITATION_LINES_RETIRED else None
    for doc in sorted([*(ROOT / "docs/architecture").glob("doc_*.md"),
                       ROOT / "docs/architecture/shell-features.md"]):
        for _, _, spec, _, problem in citation_findings(
                doc.name, read(doc), tracked, read_file, hunks, base_text(doc) if hunks else "", kept, cited):
            if CITATION_LINES_RETIRED or spec.startswith("#"):
                r.fail(problem)
    for path in sorted(set(CITATION_LINES_KEPT) - cited):
        r.fail(f"CITATION_LINES_KEPT names {path}, which no `path:N` citation cites: remove the entry")
    return r


OWN_ALLOW = "forge/config/own-allow.txt"
# The toolkits a crate built from the repository may not reach (ADR-0141 decision 7).
OWN_REFUSED_CRATES = {"gtk4", "libadwaita", "libcosmic", "iced"}


def own_refused(name):
    """NAME is a refused toolkit or one of its subcrates (gtk4-sys, libadwaita-sys, iced_core).
    iced's subcrates take an underscore, so iced-x86, an unrelated disassembler, is not one."""
    return name in OWN_REFUSED_CRATES or re.match(r"gtk4[-_]|libadwaita[-_]|iced_", name) is not None


def third_party_cargo(path):
    """A manifest or lockfile of a crate Athanor vendors or packages from upstream."""
    return "/vendor/" in path or "/SOURCES/" in path


def manifest_dependencies(manifest, renames, kinds):
    """The package names of the KINDS tables of MANIFEST (e.g. "dependencies"), in any target
    table and in the underscore spelling. RENAMES maps a [workspace.dependencies] key to its package, for `workspace = true`."""
    out = set()
    for _, table in dependency_tables(manifest, kinds):
        for key, dep in table.items():
            dep = dep if isinstance(dep, dict) else {}
            out.add(dep.get("package") or (renames.get(key, key) if dep.get("workspace") else key))
    return out


def built_dependencies(manifests, workspace_deps, root):
    """{member: the package names of its dependencies and build-dependencies that are built}, for the
    members that ROOT builds with its default features: an optional dependency is built when a
    feature turns it on (enabled_dependencies), and a member gets the features its dependents ask
    for and its defaults unless they all say `default-features = false`. WORKSPACE_DEPS holds the
    [workspace.dependencies] tables, for `workspace = true`."""
    asked = {root: (set(), True)}  # member -> (features requested, defaults requested)
    built = {}
    changed = True
    while changed:
        changed = False
        for name, (features, defaults) in list(asked.items()):
            on, sub = enabled_dependencies(manifests[name], features, defaults)
            built[name] = set()
            for _, table in dependency_tables(manifests[name], ("dependencies", "build-dependencies")):
                for key, dep in table.items():
                    dep = dep if isinstance(dep, dict) else {}
                    inherited = workspace_deps.get(key, {}) if dep.get("workspace") else {}
                    if dep.get("optional") and key not in on:
                        continue
                    package = dep.get("package") or inherited.get("package") or key
                    built[name].add(package)
                    if package in manifests:
                        before = asked.get(package, (set(), False))
                        default = ((inherited.get("default-features", True) or dep.get("default-features", False))
                                   if dep.get("workspace") else dep.get("default-features", True))
                        wanted = (before[0] | {*dep.get("features", []), *inherited.get("features", []), *sub[key]},
                                  before[1] or default)
                        if wanted != asked.get(package):
                            asked[package] = wanted
                            changed = True
    return built


def toolkit_crates(root=None):
    """({crate: the refused toolkits it reaches}, [crates no lockfile holds]), for every crate
    built from the repository, athanor-* or not (xdg-desktop-portal-athanor, calmo-cosmic-theme),
    over every manifest and lockfile of the repository but the vendored and packaged ones. A
    lockfile holds the optional dependencies of every feature, so the edges of a member of the
    repository are read from its manifest instead (built_dependencies): a crate counts while the
    features it builds with reach a toolkit, `default-features = false` and an optional dependency
    left off included. A registry crate's features cannot be read, and all its edges count. The
    dev-dependencies of a member are left out, as they are not shipped (toolkit-boundary does the
    same), and a registry crate's never reach the lockfile.
    A crate no lockfile holds counts when it names a toolkit directly, and is returned in the
    second list otherwise, as its transitive dependencies cannot be read. A vendored crate that a
    lockfile holds without a source, patched in by path (`[patch.crates-io]`), is no root of its
    own: it counts through the crates of the repository that reach it."""
    root = Path(root or ROOT)
    listed = subprocess.run(["git", "-C", str(root), "ls-files", "-z", "--", "*Cargo.toml", "*Cargo.lock"],
                            capture_output=True, text=True, check=True).stdout.split("\0")
    paths = [p for p in listed if p and not third_party_cargo(p)]
    manifests, renames, workspace_deps = {}, {}, {}
    for path in (p for p in paths if p.endswith("Cargo.toml")):
        data = tomllib.loads(read(root / path))
        for key, dep in data.get("workspace", {}).get("dependencies", {}).items():
            workspace_deps[key] = dep if isinstance(dep, dict) else {}
            if isinstance(dep, dict) and dep.get("package"):
                renames[key] = dep["package"]
        if "package" in data:
            manifests[data["package"]["name"]] = data
    hits, locked = {}, set()
    for lock in (p for p in paths if p.endswith("Cargo.lock")):
        packages = tomllib.loads(read(root / lock)).get("package", [])
        locked.update(p["name"] for p in packages if "source" not in p)
        by_name = {}
        for package in packages:
            by_name.setdefault(package["name"], []).append(package)

        def dependencies(package, built):
            member = "source" not in package and package["name"] in manifests
            for dep in package.get("dependencies", []):
                name, *version = dep.split(" ")
                if not member or package["name"] not in built or name in built[package["name"]]:
                    yield from (c for c in by_name.get(name, []) if not version or c["version"] == version[0])

        for package in packages:
            if "source" in package or package["name"] not in manifests:
                continue
            built = built_dependencies(manifests, workspace_deps, package["name"])
            seen, stack = set(), [package]
            while stack:
                for dep in dependencies(stack.pop(), built):
                    key = (dep["name"], dep["version"])
                    if key not in seen:
                        seen.add(key)
                        stack.append(dep)
                        if own_refused(dep["name"]):
                            hits.setdefault(package["name"], set()).add(dep["name"])
    unlocked = []
    for name in sorted(n for n in manifests if n not in locked):
        direct = set(filter(own_refused, manifest_dependencies(manifests[name], renames,
                                                               ("dependencies", "build-dependencies"))))
        if direct:
            hits[name] = direct
        else:
            unlocked.append(name)
    return hits, unlocked


def own_base_entries(root, base, parse):
    """The (kind, name) entries of OWN_ALLOW at revision BASE, None when BASE has no such file
    (the adoption); ValueError when BASE is not a commit, as load_known_red_base."""
    def git(*args):
        return subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)

    commit = git("rev-parse", "--verify", "--quiet", f"{base}^{{commit}}")
    if commit.returncode != 0:
        raise ValueError(f"{base} is not a commit of {root}")
    blob = f"{commit.stdout.strip()}:{OWN_ALLOW}"
    if git("cat-file", "-e", blob).returncode != 0:
        return None
    return {(kind, name) for kind, name, _ in parse(git("show", blob).stdout)}


def own_problems(root=None, base=None):
    """ADR-0141 decision 7 on the crates: every crate built from the repository that reaches a refused toolkit is
    listed in OWN_ALLOW, every crate entry still reaches one, and against BASE the list adds no
    entry. The rpm entries are checked on the built image (forge/scripts/check_image_own.py)."""
    root = Path(root or ROOT)
    parse = load_module("forge/scripts/check_image_own.py").parse_allow_list
    try:
        entries = parse(read(root / OWN_ALLOW))
    except (OSError, ValueError) as err:
        return [f"{OWN_ALLOW}: {err}"]
    crates = {name for kind, name, _ in entries if kind == "crate"}
    hits, unlocked = toolkit_crates(root)
    problems = [f"{name}: no tracked Cargo.lock holds it, so the toolkits it reaches cannot be "
                "read; add its lockfile (ADR-0141 decision 7)" for name in unlocked]
    problems += [f"{name}: reaches {', '.join(sorted(hits[name]))}, and {OWN_ALLOW} does not list it "
                "(ADR-0141 decision 7)" for name in sorted(set(hits) - crates)]
    problems += [f"{OWN_ALLOW}: crate {name} reaches none of {', '.join(sorted(OWN_REFUSED_CRATES))} or their subcrates; "
                 "remove its entry" for name in sorted(crates - set(hits))]
    if base is not None:
        try:
            before = own_base_entries(root, base, parse)
        except ValueError as err:
            return problems + [f"{OWN_ALLOW} at {base}: {err}"]
        if before is not None:
            problems += [f"{OWN_ALLOW}: {kind} {name} is not listed at {base}; the list only shrinks"
                         for kind, name, _ in entries if (kind, name) not in before]
    return problems


@check("own", "No crate built from the repository reaches gtk4, libadwaita, libcosmic or iced unless own-allow.txt lists it, and the list only shrinks (ADR-0141 decision 7)")
def check_own():
    r = Result()
    for problem in own_problems(base=BASE_REF):
        r.fail(problem)
    if BASE_REF is None:
        r.note(f"no base revision (--known-red-base REF): entries added to {OWN_ALLOW} were not checked")
    return r


def load_toml(name, text):
    """The parsed TOML `text`, or a ValueError that names the file `name`."""
    try:
        return tomllib.loads(text)
    except tomllib.TOMLDecodeError as e:
        raise ValueError(f"{name}: not valid TOML ({e})") from e


def dependency_paths(manifest):
    """The `path` of every dependency, target dependency, workspace dependency and [patch] entry
    of a parsed manifest, in each spelling of DEPENDENCY_TABLES. A key that is not a table holds
    none."""
    def table(parent, key):
        value = parent.get(key) if isinstance(parent, dict) else None
        return value if isinstance(value, dict) else {}
    tables = [table(manifest, k) for k in DEPENDENCY_TABLES]
    tables += [table(target, k) for target in table(manifest, "target").values()
               for k in DEPENDENCY_TABLES]
    tables += list(table(manifest, "patch").values())
    tables.append(table(table(manifest, "workspace"), "dependencies"))
    return [dep["path"] for table in tables if isinstance(table, dict)
            for dep in table.values() if isinstance(dep, dict) and isinstance(dep.get("path"), str)]


def sub_locks(locks, manifests):
    """The locks of workspaces of their own, other than the root, any of whose manifests builds a
    crate outside the workspace's directory by path: they resolve the root's crates with a lock
    that Dependabot never moves, since it updates only the dependencies a directory's own
    manifest declares (#1313). `locks` are paths relative to the root; `manifests` maps every
    tracked Cargo.toml to its text."""
    out = []
    for lock in sorted(locks):
        top = Path(lock).parent
        root_manifest = str(top / "Cargo.toml")
        if lock == "Cargo.lock" or "/vendor/" in f"/{lock}" or root_manifest not in manifests:
            continue
        if "workspace" not in load_toml(root_manifest, manifests[root_manifest]):
            continue
        inside = (ROOT / top).resolve()
        for name, text in sorted(manifests.items()):
            crate = Path(name).parent
            if not crate.is_relative_to(top):
                continue
            if any(not (ROOT / crate / path).resolve().is_relative_to(inside)
                   for path in dependency_paths(load_toml(name, text))):
                out.append(lock)
                break
    return out


def semver_compatible(a, b):
    """True if cargo can move `a` to `b` under a caret requirement: the same first non-zero
    component, and every component before it, pre-release and build metadata aside."""
    def key(v):
        parts = v.split("+")[0].split("-")[0].split(".")
        for i, part in enumerate(parts):
            if part != "0":
                return parts[: i + 1]
        return parts
    return key(a) == key(b)


def lock_version_problems(root, sub, path):
    """A package of the lock `path` (text `sub`) from the registry or from git, at a version and
    source the root lock (text `root`) does not hold: the same crate builds twice, and only the
    root one is tested and moved by Dependabot. A registry crate the root patches by path must
    take the patch's version. Whether it applies the patch is its own manifest's policy, not
    drift: the gallery builds as an application outside Athanor would and patches only AccessKit
    (GALLERY_PATCH, ADR-0126). A [[patch.unused]] entry is a patch the lock no longer applies
    (#638). A crate the root does not use is the sub-workspace's own, and a path crate is built
    from the same tree."""
    held = {}
    for package in load_toml("Cargo.lock", root).get("package", []):
        source = package.get("source", "")
        held.setdefault(package["name"], set()).add(
            (package["version"], source if source.startswith("git+") else "registry"))
    manifest = str(Path(path).with_name("Cargo.toml"))
    lock = load_toml(path, sub)
    out = [f"{path}: [[patch.unused]] {p.get('name')} {p.get('version')}: the patch in {manifest} "
           "is not applied" for p in lock.get("patch", {}).get("unused", [])]
    for package in lock.get("package", []):
        name, old, source = package["name"], package["version"], package.get("source")
        if source is None or name not in held:
            continue
        git = source.startswith("git+")
        if (old, source if git else "registry") in held[name]:
            continue
        versions = sorted({v for v, s in held[name] if s == "registry"})
        new = [v for v in versions if semver_compatible(old, v)]
        if git and any(k.startswith("git+") for _, k in held[name]):
            fix = f"cargo update --manifest-path {manifest} -p {name}"
        elif git or not versions:
            fix = f"take the root's source of {name} in {manifest}"
        elif new:
            fix = f"cargo update --manifest-path {manifest} -p {name}@{old} --precise {new[0]}"
        else:
            fix = f"no semver-compatible root version ({', '.join(versions)})"
        out.append(f"{path}: {name} {old} ({source}) is not in the root Cargo.lock: {fix}")
    return sorted(out)


FUZZ_MANIFEST = "fuzz/Cargo.toml"


def fuzz_patch_problems(manifests):
    """Every [patch] entry of the root Cargo.toml that fuzz/Cargo.toml does not carry, from the
    same directory: the fuzzed compositor must parse the wire as the shipped one does, and a
    workspace of its own takes no [patch] of the root. The gallery patches only AccessKit instead
    (GALLERY_PATCH, ADR-0126). `manifests` maps every tracked Cargo.toml to its text."""
    if FUZZ_MANIFEST not in manifests:
        return [f"{FUZZ_MANIFEST} is not tracked"]

    def patches(name, written=None):
        data = load_toml(name, manifests[name])
        table = data.get("patch") if isinstance(data.get("patch"), dict) else {}
        out = {}
        for registry, entries in table.items():
            for crate, spec in entries.items() if isinstance(entries, dict) else ():
                path = spec.get("path") if isinstance(spec, dict) else None
                out[(registry, crate)] = ((ROOT / name).parent / path).resolve() if isinstance(path, str) else spec
                if written is not None and isinstance(path, str):
                    written[(registry, crate)] = path
        return out
    fuzz = patches(FUZZ_MANIFEST)
    shown = {}
    root_patches = patches("Cargo.toml", shown)
    return [f"{FUZZ_MANIFEST}: [patch.{registry}] {crate} is not the root Cargo.toml's entry, "
            f"{shown.get((registry, crate), spec)}"
            for (registry, crate), spec in sorted(root_patches.items())
            if fuzz.get((registry, crate)) != spec]


@check("lock-versions", "A workspace of its own that builds the root's crates by path holds the root Cargo.lock's version of every crate they share")
def check_lock_versions():
    r = Result()
    listed = subprocess.run(["git", "ls-files", "-z", "--", "*Cargo.lock", "*Cargo.toml"],
                            cwd=ROOT, capture_output=True, text=True)
    if listed.returncode != 0:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    files = [p for p in listed.stdout.split("\0") if p]
    locks = [p for p in files if p.endswith("Cargo.lock")]
    try:
        manifests = {p: read(ROOT / p) for p in files
                     if p.endswith("Cargo.toml") and (ROOT / p).is_file()}
        subs = sub_locks(locks, manifests)
        if not subs:
            r.fail("no workspace of its own builds the root's crates by path: fuzz/ and the Sal "
                   "gallery did")
        root = read(ROOT / "Cargo.lock")
        for path in subs:
            for problem in lock_version_problems(root, read(ROOT / path), path):
                r.fail(problem)
        for problem in fuzz_patch_problems(manifests):
            r.fail(problem)
    except (OSError, ValueError) as e:
        r.fail(f"a Cargo.lock or Cargo.toml could not be read: {e}")
    return r


# --------------------------------------------------------------------------- #
# runner
# --------------------------------------------------------------------------- #

# --------------------------------------------------------------------------- #
# known-red: checks red when `just check` became the gate (doc_pipeline.md PQ12)
# --------------------------------------------------------------------------- #

KNOWN_RED_LINE = re.compile(r"(\S+)\s+(#\d+)\s+(\d{4}-\d{2}-\d{2})\s+(\S.*)")
LINE_NUMBER = re.compile(r"(?<=\S):\d+")


def known_red_finding(problem):
    """The stable text of a problem: the problem without its line numbers (`a.rs:18` reads
    `a.rs`), so that a finding keeps its entry when unrelated lines move it."""
    return LINE_NUMBER.sub("", problem)


def parse_known_red(text):
    """[(check, issue, expires, finding)] from the text of scripts/ci/known-red.txt.

    One entry per line, `check #issue YYYY-MM-DD finding`: the check may report that finding
    (known_red_finding of a problem) until `expires`, inclusive, while the issue tracks the
    fix. A finding the check reports n times is listed n times. Blank lines and `#` comments
    are ignored; anything else raises ValueError.
    """
    entries = []
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        match = KNOWN_RED_LINE.fullmatch(line)
        if not match:
            raise ValueError(
                f"line {number}: expected `check #issue YYYY-MM-DD finding`: {line}"
            )
        name, issue, expires, finding = match.groups()
        try:
            entries.append((name, issue, datetime.date.fromisoformat(expires), finding))
        except ValueError:
            raise ValueError(f"line {number}: {expires} is not a date") from None
    return entries


def load_known_red_base(root, ref, path):
    """The list as revision REF has it, None when REF has no such file (the adoption)."""

    def git(*args):
        return subprocess.run(
            ["git", "-C", str(root), *args], capture_output=True, text=True
        )

    commit = git("rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}")
    if commit.returncode != 0:
        raise ValueError(f"{ref} is not a commit of {root}")
    blob = f"{commit.stdout.strip()}:{path}"
    if git("cat-file", "-e", blob).returncode != 0:
        return None
    return parse_known_red(git("show", blob).stdout)


def judge_known_red(entries, base, results, today):
    """(tolerated, problems): the checks whose failure the list excuses, and what is wrong
    with the list itself.

    A check that ran is tolerated while every finding it reports is listed, as many times as
    it reports it, and none of its entries has expired. A listed finding the check no longer
    reports must leave the list. Against BASE (the list of the base revision, None at
    adoption) the list may only shrink: no finding the base does not list, no later expiry;
    the issue of an entry may change.
    """
    problems, listed, expired = [], {}, set()
    for name, issue, expires, finding in entries:
        if name not in CHECKS:
            problems.append(f"{name}: no such check")
            continue
        listed.setdefault(name, collections.Counter())[finding] += 1
        if today > expires:
            expired.add(name)
            problems.append(f"{name}: expired on {expires} ({issue}): {finding}")
    if base is not None:
        latest = {}
        for name, _, expires, finding in base:
            key = (name, finding)
            latest[key] = max(expires, latest.get(key, expires))
        added = collections.Counter(
            (n, f) for n, _, _, f in entries
        ) - collections.Counter((n, f) for n, _, _, f in base)
        for name, finding in added.elements():
            problems.append(
                f"{name}: not in the base list, the list may only shrink: {finding}"
            )
        for name, _, expires, finding in entries:
            was = latest.get((name, finding))
            if was is not None and expires > was:
                problems.append(
                    f"{name}: expiry moved from {was} to {expires}: {finding}"
                )

    tolerated = set()
    for name, known in sorted(listed.items()):
        if name not in results:
            continue
        found = collections.Counter(
            known_red_finding(p) for p in results[name].problems
        )
        new, fixed = found - known, known - found
        problems += [f"{name}: not in the list: {f}" for f in new.elements()]
        problems += [f"{name}: fixed, remove its entry: {f}" for f in fixed.elements()]
        if found and not new and name not in expired:
            tolerated.add(name)
    return tolerated, problems

# --------------------------------------------------------------------------- #
# implicit-panics — the panics that no token shows
# --------------------------------------------------------------------------- #

# Under `panic = "abort"` with `overflow-checks = true` ([profile.release] of Cargo.toml) an
# overflow, an index out of bounds or a slice off a character boundary ends the process, and no
# `.unwrap()` marks the place. Clippy finds them: their count per crate is a ratchet, as BUDGET is,
# that only goes down (#1352). The lints are passed on clippy's command line as `--force-warn`,
# which no `allow` or `expect`, no `warnings` group and no [lints] table can lower: the compiler,
# not a scan of the sources, decides what hides a hit. `--cap-lints` still caps them, and a
# clippy.toml can relax them, so implicit_panic_configs reads the tracked configurations, the
# measuring run drops from its environment what implicit_panic_env does, and implicit_panic_counts
# refuses a build script that sets it back. Code that clippy does not compile is not measured:
# implicit_panic_cfgs refuses a `cfg` on `clippy` or on `debug_assertions`.
IMPLICIT_PANIC_LINTS = ("clippy::arithmetic_side_effects", "clippy::indexing_slicing", "clippy::string_slice")
IMPLICIT_PANIC_BUDGET = "scripts/ci/implicit-panics.txt"
# Two runs, as in security-audit.sh: athanor-preview-render needs zbus on async-io. No test target:
# a panic in a test fails the test, it does not end a service.
IMPLICIT_PANIC_RUNS = (
    ["--workspace", "--exclude", "athanor-preview-render"],
    ["-p", "athanor-preview-render"],
)
# The cargo command the check runs clippy with. The GTK crates build only where their libraries
# are, so measuring is opt-in: scripts/ci/security-audit.sh sets it in the builder (pr.yml's
# rust_audit job), and `ATHANOR_IMPLICIT_PANICS_CARGO="bash forge/test/shell/rig.sh cargo"` locally.
# Without it the check still fails closed on the budget file and the configurations: a line raised
# or dropped against the base, a member without a line, a configuration that hides a lint.
IMPLICIT_PANIC_CARGO = "ATHANOR_IMPLICIT_PANICS_CARGO"
# What a tracked Cargo configuration may not hold: a rustflags flag that allows a lint or caps
# them all, an [env] variable that points clippy at another configuration or other arguments, an
# `include` of a configuration not read here, and another compiler or a wrapper around it, which
# could drop the command line's lints.
IMPLICIT_PANIC_FLAGS = re.compile(r"-A.*|--allow(?:=.*)?|--cap-lints(?:=.*)?")
IMPLICIT_PANIC_ENV = {"CLIPPY_CONF_DIR", "CLIPPY_ARGS"}
IMPLICIT_PANIC_BUILD_KEYS = {"rustc", "rustc-wrapper", "rustc-workspace-wrapper"}
# A clippy.toml key that relaxes a budget lint: the operations arithmetic_side_effects lets
# through, or no restriction lint in a `const` context.
IMPLICIT_PANIC_CLIPPY_KEY = re.compile(
    r".*(?:arithmetic-side-effects|indexing-slicing|string-slice).*|suppress-restriction-lint-in-const")


def implicit_panic_env_key(key):
    """Whether the variable `key` may cap or relax the lints of the measuring run: each RUSTFLAGS
    form cargo reads (RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_RUSTFLAGS,
    CARGO_TARGET_<triple>_RUSTFLAGS, ...) and IMPLICIT_PANIC_ENV."""
    return key.endswith("RUSTFLAGS") or key in IMPLICIT_PANIC_ENV


def implicit_panic_env(env):
    """`env` without the variables implicit_panic_env_key names. rig.sh passes no host variable into
    its container but those it names."""
    return {k: v for k, v in env.items() if not implicit_panic_env_key(k)}


def implicit_panic_budget(text):
    """{crate: count} of the budget file: one `crate count` line per crate, `#` comments."""
    budget = {}
    for n, line in enumerate(text.split("\n"), 1):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        name, _, count = line.partition(" ")
        if not count.strip().isdigit() or name in budget:
            raise ValueError(f"{IMPLICIT_PANIC_BUDGET}:{n}: not one `crate count` line per crate: {line}")
        budget[name] = int(count)
    return budget


def package_crate(package_id):
    """The crate of a local `package_id`: `path+file:///d/dir#name@1.0`, or `path+file:///d/name#1.0`
    when the directory is named after the crate."""
    path, _, fragment = package_id.partition("#")
    return fragment.partition("@")[0] if "@" in fragment else path.rsplit("/", 1)[-1]


def implicit_panic_counts(lines):
    """({crate: hits}, problems) of the JSON lines of clippy runs. A hit counts once per place and
    lint, though two targets or two runs compile the same module; every local crate built has an
    entry, zero included, and vendored crates have none. A run that did not finish, or failed, is
    a problem: a crate it did not reach would read as zero. So is a build script, vendored or not,
    that sets for its crate's compilation a variable implicit_panic_env_key names: cargo reports
    it on every run, a cached one included, however the script spells its name.

    The count is keyed by a hit's primary span, so the expansions of one macro, or the calls of
    one function that indexes, are one hit: the budget counts the places in the source that can
    panic, not the paths that reach them."""
    counts, seen, problems, finished = {}, set(), [], 0
    for line in lines:
        if not line.startswith("{"):
            continue
        msg = json.loads(line)
        if msg.get("reason") == "build-script-executed":
            hidden = sorted({key for key, _ in msg.get("env") or [] if implicit_panic_env_key(key)})
            if hidden:
                problems.append(f"{package_crate(msg.get('package_id', ''))}: its build script sets "
                                f"{', '.join(hidden)}, which changes what clippy is run with")
            continue
        if msg.get("reason") == "build-finished":
            finished += 1
            if msg.get("success") is not True:
                problems.append("a clippy run did not succeed, so its crates were not all counted")
            continue
        package = msg.get("package_id", "")
        if not package.startswith("path+") or "/vendor/" in package:
            continue
        crate = package_crate(package)
        counts.setdefault(crate, 0)
        lint = ((msg.get("message") or {}).get("code") or {}).get("code")
        if msg.get("reason") != "compiler-message" or lint not in IMPLICIT_PANIC_LINTS:
            continue
        spans = [(s["file_name"], s["line_start"], s["column_start"])
                 for s in msg["message"]["spans"] if s.get("is_primary")]
        key = (crate, lint, *spans[:1])
        if key not in seen:
            seen.add(key)
            counts[crate] += 1
    if not finished:
        problems.append("no clippy run finished")
    return counts, problems


def implicit_panic_problems(counts, budget):
    """(problems, notes) of the measured `counts` against `budget`: a line holds its crate's count
    exactly, so a count that went down lowers it in the same change, and a new crate enters at its
    count. (`panics` only notes a lower count.)"""
    problems, notes = [], []
    for crate in sorted(counts.keys() | budget.keys()):
        have, allowed = counts.get(crate), budget.get(crate)
        if have is None:
            problems.append(f"{crate}: in {IMPLICIT_PANIC_BUDGET} but built by no clippy run: remove its line")
        elif allowed is None:
            problems.append(f"{crate}: {have} implicit panics and no line in {IMPLICIT_PANIC_BUDGET}")
        elif have > allowed:
            problems.append(f"{crate}: {have} implicit panics over the budget of {allowed}: use checked or "
                            f"saturating arithmetic, `.get()`, or a split on a character boundary")
        elif have < allowed:
            problems.append(f"{crate}: {have} implicit panics under the budget of {allowed}: lower its line "
                            f"in {IMPLICIT_PANIC_BUDGET} to {have}")
    return problems, notes


def implicit_panic_raises(budget, base):
    """The crates whose line `budget` raises over `base`: the budget only goes down."""
    return [f"{crate}: {IMPLICIT_PANIC_BUDGET} raises its budget from {base[crate]} to {n}"
            for crate, n in sorted(budget.items()) if n > base.get(crate, n)]


def implicit_panic_dropped(budget, base, crates):
    """The crates whose line `budget` drops from `base` while a manifest of theirs is still among
    `crates`: a line goes with its crate, never alone, and an emptied file drops them all."""
    found = [f"{crate}: {IMPLICIT_PANIC_BUDGET} drops its line of {base[crate]} while the crate is still in the tree"
             for crate in sorted(base.keys() - budget.keys()) if crate in crates]
    return found + implicit_panic_renamed(budget, base)


def implicit_panic_renamed(budget, base):
    """A change that drops lines and adds lines of names the base has none for is read as a
    rename: the added lines may hold at most what the dropped ones held, or a crate renamed
    would carry a higher count past implicit_panic_raises."""
    dropped = {crate: base[crate] for crate in base.keys() - budget.keys()}
    added = {crate: budget[crate] for crate in budget.keys() - base.keys()}
    if not dropped or sum(added.values()) <= sum(dropped.values()):
        return []
    return [f"{', '.join(sorted(added))}: {IMPLICIT_PANIC_BUDGET} adds {sum(added.values())} where it drops "
            f"{sum(dropped.values())} ({', '.join(sorted(dropped))}): a rename keeps at most the old count"]


def implicit_panic_unlisted(budget, members):
    """The workspace `members` with no line in `budget`: each has one, zero included."""
    return [f"{crate}: a workspace member with no line in {IMPLICIT_PANIC_BUDGET}: add it at its measured count"
            for crate in sorted(members - budget.keys())]


def implicit_panic_configs(root, tracked):
    """What hides a budget lint in the tracked Cargo configurations and clippy.toml files among
    `tracked` (paths relative to `root`): IMPLICIT_PANIC_FLAGS in a rustflags, IMPLICIT_PANIC_ENV
    in [env], an `include`, an IMPLICIT_PANIC_BUILD_KEYS key, an IMPLICIT_PANIC_CLIPPY_KEY. A
    configuration that is no TOML is a finding too."""
    found = []
    for path in sorted(tracked):
        parts = Path(path).parts
        cargo = len(parts) > 1 and parts[-2] == ".cargo" and parts[-1] in ("config", "config.toml")
        if not cargo and parts[-1] not in ("clippy.toml", ".clippy.toml"):
            continue
        try:
            doc = tomllib.loads(read(root / path))
        except (OSError, tomllib.TOMLDecodeError) as e:
            found.append(f"{path}: cannot be read: {e}")
            continue
        if not cargo:
            found += [f"{path}: `{key}` relaxes a lint of {IMPLICIT_PANIC_BUDGET}"
                      for key in doc if IMPLICIT_PANIC_CLIPPY_KEY.fullmatch(key)]
            continue
        found += [f"{path}: [env] {key} changes what clippy is run with"
                  for key in doc.get("env", {}) if key in IMPLICIT_PANIC_ENV]
        found += [f"{path}: `include` reads a configuration this check does not"] if "include" in doc else []
        found += [f"{path}: [build] {key} replaces or wraps the compiler clippy runs"
                  for key in doc.get("build", {}) if key in IMPLICIT_PANIC_BUILD_KEYS]
        tables = [doc]
        while tables:
            table = tables.pop()
            for key, value in table.items():
                if isinstance(value, dict):
                    tables.append(value)
                elif key == "rustflags":
                    flags = value.split() if isinstance(value, str) else value
                    found += [f"{path}: rustflags `{flag}` allows or caps the lints of {IMPLICIT_PANIC_BUDGET}"
                              for flag in flags if isinstance(flag, str) and IMPLICIT_PANIC_FLAGS.fullmatch(flag)]
    return found


# `clippy` or `debug_assertions` as a name, not as the head of a `clippy::` lint path.
IMPLICIT_PANIC_CFG_NAME = re.compile(r"\b(clippy|debug_assertions)\b(?!\s*::)")


def implicit_panic_cfgs(root, tracked, members):
    """The tracked .rs files of the workspace members (directories relative to `root`), vendored
    crates aside as in implicit_panic_counts, that name
    `clippy` or `debug_assertions` outside a `clippy::` path. Under `cfg(not(clippy))` code is
    hidden from clippy, and under `cfg(not(debug_assertions))` from the dev profile clippy builds,
    while the release build compiles it. The whole file is read, not only the `cfg(` and
    `cfg_attr(` attributes: a macro can take the predicate as an argument."""
    found = []
    for path in sorted(tracked):
        if not path.endswith(".rs") or "/vendor/" in path or not any(path.startswith(f"{m}/") for m in members):
            continue
        try:
            text = read(root / path)
        except OSError as e:
            found.append(f"{path}: cannot be read: {e}")
            continue
        for n, line in enumerate(text.split("\n"), 1):
            found += [f"{path}:{n}: names `{m.group(1)}`, which a `cfg` can hide code from clippy with"
                      for m in IMPLICIT_PANIC_CFG_NAME.finditer(line)]
    return found


def workspace_crates(root=None):
    """{package name: directory} of the members of the root workspace."""
    root = root or ROOT
    crates = {}
    for member in tomllib.loads(read(root / "Cargo.toml"))["workspace"]["members"]:
        crates[tomllib.loads(read(root / member / "Cargo.toml"))["package"]["name"]] = root / member
    return crates


def local_crates():
    """The package names of the manifests under RS_DIRS, workspace members or not."""
    return {tomllib.loads(read(m)).get("package", {}).get("name") for d in RS_DIRS for m in walk(ROOT / d, "Cargo.toml")}


@check("implicit-panics", "Implicit panics (overflow, index, string slice) per crate do not grow")
def check_implicit_panics():
    r = Result()
    try:
        budget = implicit_panic_budget(read(ROOT / IMPLICIT_PANIC_BUDGET))
        crates = workspace_crates()
    except (OSError, ValueError, KeyError) as e:
        r.fail(str(e))
        return r
    members = set(crates)
    listed = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z"], capture_output=True, text=True)
    if listed.returncode:
        r.fail(f"git ls-files failed: {listed.stderr.strip()}")
        return r
    tracked = [f for f in listed.stdout.split("\0") if f]
    member_dirs = [d.relative_to(ROOT).as_posix() for d in crates.values()]
    for problem in implicit_panic_configs(ROOT, tracked) + implicit_panic_cfgs(ROOT, tracked, member_dirs):
        r.fail(problem)
    for problem in implicit_panic_unlisted(budget, members):
        r.fail(problem)
    if BASE_REF is not None:
        # git's own words tell a budget the change adds from a base that cannot be read.
        shown = subprocess.run(["git", "-C", str(ROOT), "show", f"{BASE_REF}:{IMPLICIT_PANIC_BUDGET}"],
                               capture_output=True, text=True, env={**os.environ, "LC_ALL": "C"})
        try:
            if shown.returncode == 0:
                base = implicit_panic_budget(shown.stdout)
                for problem in implicit_panic_raises(budget, base) + implicit_panic_dropped(budget, base, local_crates()):
                    r.fail(problem)
            elif "does not exist" not in shown.stderr and "but not in" not in shown.stderr:
                r.fail(f"git show {BASE_REF}:{IMPLICIT_PANIC_BUDGET}: {shown.stderr.strip()}")
        except ValueError as e:
            r.fail(f"{BASE_REF}: {e}")
    cargo = os.environ.get(IMPLICIT_PANIC_CARGO)
    if not cargo:
        r.note(f"clippy not run, only the budget read: {IMPLICIT_PANIC_CARGO}=\"bash forge/test/shell/rig.sh "
               f"cargo\" measures it")
        return r
    lines = []
    for run in IMPLICIT_PANIC_RUNS:
        args = [*cargo.split(), "clippy", "--locked", *run, "--all-features", "--message-format=json",
                "--", *(arg for lint in IMPLICIT_PANIC_LINTS for arg in ("--force-warn", lint))]
        try:
            p = subprocess.run(args, cwd=ROOT, stdout=subprocess.PIPE, text=True, env=implicit_panic_env(os.environ))
        except OSError as e:
            r.fail(f"{' '.join(args)}: {e}")
            return r
        if p.returncode != 0:
            r.fail(f"{' '.join(args)}: exit {p.returncode}")
            return r
        lines += p.stdout.split("\n")
    try:
        counts, problems = implicit_panic_counts(lines)
    except (ValueError, KeyError, TypeError) as e:
        r.fail(f"clippy's JSON could not be read: {e!r}")
        return r
    if not problems:
        problems, notes = implicit_panic_problems(counts, budget)
        r.notes += notes
    for problem in problems:
        r.fail(problem)
    return r

# --------------------------------------------------------------------------- #
# panic-hook — a shipped program logs its panic before it aborts
# --------------------------------------------------------------------------- #

# The first statement of every shipped program's `main` (#1352): with core dumps off (doc_session.md
# SN8) the hook's message, place, thread and backtrace are all a crash leaves.
PANIC_HOOK = "athanor_unit::panic::install();"
# A program whose `main` starts with something else: the hook follows it at once. The greeter is
# confined first, while it is the only thread, and refuses core dumps, and then installs the hook
# (maintainer decision, 2026-11-21).
PANIC_HOOK_AFTER = {
    "athanor-greeter-ui": r"if\s+let\s+Err\(err\)\s*=\s*sandbox::ensure_single_threaded\(\)[^{]*"
                          r"apply_landlock_sandbox\(\)[^{]*forbid_core_dumps\(\)[^{]*\{[^{}]*\}\s*",
}
# athanor-lock does not depend on athanor-unit yet: the lock pull request (LP18 step 2) adds the
# hook and removes this entry. The set only shrinks.
PANIC_HOOK_PENDING = {"athanor-lock"}
# What a hooked program may take from tracing_subscriber: a filter, and the trait that installs
# the subscriber athanor_unit::journal builds. Any other subscriber writes the hook's report to
# the journal without the `<3>` that sets its priority, and with colour codes.
PANIC_HOOK_SUBSCRIBER = re.compile(r"\btracing_subscriber\b(?:\s*::\s*(\{[^}]*\}|[\w\s:]+))?")
PANIC_HOOK_SUBSCRIBER_ITEM = re.compile(r"EnvFilter\b.*|util::SubscriberInitExt|util::\{SubscriberInitExt\}")


def program_mains(crate_dir):
    """The source files of a crate's programs: src/main.rs, src/bin/, and each [[bin]] path."""
    manifest = tomllib.loads(read(crate_dir / "Cargo.toml"))
    found = {crate_dir / b["path"] for b in manifest.get("bin", []) if "path" in b}
    found |= {p for p in [crate_dir / "src" / "main.rs"] if p.exists()}
    bins = crate_dir / "src" / "bin"
    if bins.is_dir():
        found |= set(bins.glob("*.rs")) | set(bins.glob("*/main.rs"))
    return sorted(found)


def panic_hook_problems(root=None):
    """The shipped programs whose `main` does not start with PANIC_HOOK, or with what
    PANIC_HOOK_AFTER names and then PANIC_HOOK: every program of a workspace member that
    experimental/EXEMPT does not list, but PANIC_HOOK_PENDING."""
    root = root or ROOT
    exempt_file = root / "experimental" / "EXEMPT"
    exempt = ({ln.strip() for ln in read(exempt_file).split("\n") if ln.strip() and not ln.startswith("#")}
              if exempt_file.exists() else set())
    problems = []
    for name, crate_dir in workspace_crates(root).items():
        if name in exempt or name in PANIC_HOOK_PENDING:
            continue
        first = re.compile(r"\bfn\s+main\s*\(\s*\)[^{]*\{\s*" + PANIC_HOOK_AFTER.get(name, "") + re.escape(PANIC_HOOK))
        mains = program_mains(crate_dir)
        for main in mains:
            if not main.exists():
                problems.append(f"{main.relative_to(root)}: a [[bin]] path of {name} that does not exist")
            elif not first.search(without_literals(without_test_items(read(main)))):
                problems.append(f"{main.relative_to(root)}: `main` does not start with `{PANIC_HOOK}`")
        if mains:
            problems += panic_hook_subscriber_problems(root, crate_dir)
    return problems


def panic_hook_subscriber_problems(root, crate_dir):
    """`path:line` of each subscriber a hooked program sets up outside athanor_unit::journal,
    outside tests: a use of tracing_subscriber other than PANIC_HOOK_SUBSCRIBER_ITEM, or a
    `set_global_default`. Limit: a subscriber of another crate that the program calls is not seen."""
    files = sorted(walk(crate_dir / "src", ".rs"))
    test_modules = test_module_files(files)
    found = []
    for p in files:
        if is_test_file(p) or p in test_modules:
            continue
        txt = without_literals(without_test_items(read(p)))
        bad = [m for m in PANIC_HOOK_SUBSCRIBER.finditer(txt)
               if not (m.group(1) and all(PANIC_HOOK_SUBSCRIBER_ITEM.fullmatch(item)
                                          for item in re.split(r"\s*,\s*", re.sub(r"\s+", "", m.group(1)).strip("{},"))))]
        bad += list(re.finditer(r"\bset_global_default\s*\(", txt))
        found += [f"{p.relative_to(root)}:{txt.count(chr(10), 0, m.start()) + 1}: a subscriber other than "
                  "athanor_unit::journal's, which writes the panic report at no journal priority"
                  for m in sorted(bad, key=lambda m: m.start())]
    return found


@check("panic-hook", "Every shipped program installs the panic hook first in main and logs through athanor_unit::journal")
def check_panic_hook():
    r = Result()
    for problem in panic_hook_problems():
        r.fail(problem)
    return r


# A test that runs a script reaching system/tier-digests.sh and hands it a copy of
# os.environ inherits the CI run's trust environment: on a pull request into rebase/*,
# GITHUB_BASE_REF makes tier-digests.sh read that branch's rules over the network (#1257).
# The names are the ones tier-digests.sh reads: a rebase/* head switches the base and calls
# `gh api` (GITHUB_BASE_REF, GITHUB_HEAD_REF, GITHUB_REF, GITHUB_REPOSITORY, GITHUB_SERVER_URL,
# TIER_TRUSTED_REFS, the tokens), and the registry owner comes from GITHUB_REPOSITORY_OWNER.
TRUST_ENV_NAMES = ("GITHUB_BASE_REF", "GITHUB_HEAD_REF", "GITHUB_REF", "GITHUB_REPOSITORY",
                   "GITHUB_REPOSITORY_OWNER", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS", "GITHUB_ACTIONS",
                   "GH_TOKEN", "GITHUB_TOKEN")
TIER_DIGESTS = "system/tier-digests.sh"
TEMPLATE_MODULE = "forge/scripts/tests/test_tier_cache.py"  # carries the isolation block every other copies
SHARED_TRUST_ENV = "forge/scripts/tests/signed_registry.py"
COMMENT_LINE = re.compile(r"(?m)^\s*#.*$")


def dotted(node, aliases):
    """`a.b.c` of a Name or Attribute chain whose first name is expanded through the import aliases,
    "" for anything else."""
    parts = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    if not isinstance(node, ast.Name):
        return ""
    parts.append(aliases.get(node.id, node.id))
    return ".".join(reversed(parts))


def string_literals(tree):
    """The module-level `NAME = (strings...)` tuples, lists and sets of a module, as {NAME: {strings}}."""
    return {n.targets[0].id: {e.value for e in n.value.elts if isinstance(e, ast.Constant) and isinstance(e.value, str)}
            for n in tree.body if isinstance(n, ast.Assign) and len(n.targets) == 1
            and isinstance(n.targets[0], ast.Name) and isinstance(n.value, (ast.Tuple, ast.List, ast.Set))}


def scope_nodes(body):
    """Every node that runs when a module is imported: the statements of `body` and what they hold, a class
    body included; of a function or lambda only its decorators and default values, never its body."""
    stack = list(body)
    while stack:
        node = stack.pop()
        yield node
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)):
            stack.extend(getattr(node, "decorator_list", []))
            stack.extend(node.args.defaults + [d for d in node.args.kw_defaults if d is not None])
        else:
            stack.extend(ast.iter_child_nodes(node))


def bindings(tree, name):
    """How many times the module scope binds `name`: a def, a class, an assignment, an import, a `for` or
    `with` target, a match capture, a deletion. `import a` and `import a.b` bind `a` together, once; any `from` import binds on its own."""
    count, plain = 0, set()
    for n in scope_nodes(tree.body):
        if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            count += n.name == name
        elif isinstance(n, ast.Name):
            count += n.id == name and isinstance(n.ctx, (ast.Store, ast.Del))
        elif isinstance(n, ast.ImportFrom):
            count += sum((a.asname or a.name) == name for a in n.names)
        elif isinstance(n, ast.Import):
            for a in n.names:
                if (a.asname or a.name.split(".")[0]) == name:
                    if a.asname:
                        count += 1
                    else:
                        plain.add(name)
        elif isinstance(n, (ast.ExceptHandler, ast.MatchAs, ast.MatchStar)):
            count += n.name == name
        elif isinstance(n, ast.MatchMapping):
            count += n.rest == name
    return count + len(plain)


def top_import_aliases(tree):
    """{bound name: dotted target} of the imports that are direct children of the module."""
    aliases = {}
    for n in tree.body:
        if isinstance(n, ast.Import):
            for a in n.names:
                aliases[a.asname or a.name.split(".")[0]] = a.name if a.asname else a.name.split(".")[0]
        elif isinstance(n, ast.ImportFrom) and n.module:
            for a in n.names:
                aliases[a.asname or a.name] = f"{n.module}.{a.name}"
    return aliases


def anchored(tree, name):
    """Whether `name` is bound once, by an import that is a direct child of the module, and is neither
    imported inside a function or a block nor declared `global`."""
    if bindings(tree, name) != 1 or name not in top_import_aliases(tree):
        return False
    top = {id(n) for n in tree.body}
    for n in ast.walk(tree):
        if isinstance(n, ast.Global) and name in n.names:
            return False
        if isinstance(n, (ast.Import, ast.ImportFrom)) and id(n) not in top \
                and any((a.asname or a.name.split(".")[0]) == name for a in n.names):
            return False
    return True


def environ_gaps(tree, aliases, exempt):
    """The lines, at import time (module level, class bodies, function defaults and decorators), of a reference
    to os.environ that is not a read of one variable that TRUST_ENV_NAMES does not hold: os.environ.get(<constant>
    [, default]), os.getenv(<constant>[, default]) or os.environ[<constant>] in a load. `exempt` holds the ids of
    the nodes allowed (the argument of the module's patch statement). Anything else is refused, which fails
    closed: write `with mock.patch.dict(os.environ, ...)` inside the test in place of the decorator form,
    read os.environ.get("<name outside the trust list>") in place of a membership test, and move a write into
    the test or setUpModule. A load of `os` itself, one that is not the base of an attribute access (`o = os`,
    `f(os)`, `[os]`), is refused too: an alias would reach the mapping unseen."""
    scope = list(scope_nodes(tree.body))
    bases = {id(n.value) for n in scope if isinstance(n, ast.Attribute)}
    const = lambda a: isinstance(a, ast.Constant) and isinstance(a.value, str) and a.value not in TRUST_ENV_NAMES  # noqa: E731
    allowed = set(exempt)
    lines = set()
    for n in scope:
        if isinstance(n, ast.Call) and n.args and isinstance(n.func, ast.Attribute) and n.func.attr == "get" \
                and dotted(n.func.value, aliases) == "os.environ":
            if const(n.args[0]):
                allowed.add(id(n.func.value))
        elif isinstance(n, ast.Subscript) and isinstance(n.ctx, ast.Load) and dotted(n.value, aliases) == "os.environ":
            if const(n.slice):
                allowed.add(id(n.value))
        elif isinstance(n, ast.Call) and dotted(n.func, aliases) == "os.getenv" and not (n.args and const(n.args[0])):
            lines.add(n.lineno)
    lines |= {n.lineno for n in scope if isinstance(n, (ast.Name, ast.Attribute))
              and dotted(n, aliases) == "os.environ" and id(n) not in allowed}
    lines |= {n.lineno for n in scope if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Load)
              and aliases.get(n.id) == "os" and id(n) not in bases}
    return sorted(lines)


def mutated(tree, name):
    """Whether `name` is rebound or changed other than by its one binding: a method call, an item assignment
    or deletion, a `global` statement."""
    for n in ast.walk(tree):
        if isinstance(n, ast.Call) and isinstance(n.func, ast.Attribute) and isinstance(n.func.value, ast.Name) \
                and n.func.value.id == name:
            return True
        if isinstance(n, ast.Subscript) and isinstance(n.ctx, (ast.Store, ast.Del)) \
                and isinstance(n.value, ast.Name) and n.value.id == name:
            return True
        if isinstance(n, ast.Global) and name in n.names:
            return True
    return False


def passed(tree, name):
    """Whether a statement that runs at import time passes `name` to a call, as `list.clear(NAMES)` does."""
    return any(isinstance(n, ast.Call) and any(isinstance(x, ast.Name) and x.id == name
                                               for a in [*n.args, *(k.value for k in n.keywords)]
                                               for x in ast.walk(a))
               for n in scope_nodes(tree.body))


def isolation_gaps(tree, shared):
    """What a test module lacks of the isolation block, which it copies from TEMPLATE_MODULE: a module-level
    `P = mock.patch.dict(os.environ)`, a setUpModule that begins with `P.start()` and then
    `for name in NAMES: os.environ.pop(name, None)`, a tearDownModule that begins with `P.stop()`; NAMES is
    a literal list of strings (or the signed_registry import) that holds every TRUST_ENV_NAMES entry, bound
    once and never changed. The leading statements are compared as trees, so nothing before or between them
    can skip them. At import time the module reads no variable of TRUST_ENV_NAMES and no os.environ but one
    named variable."""
    aliases = top_import_aliases(tree)
    lists = string_literals(tree)
    if aliases.get("TRUST_ENV") == "signed_registry.TRUST_ENV":
        lists["TRUST_ENV"] = shared
    patches = [n for n in tree.body if isinstance(n, ast.Assign) and len(n.targets) == 1
               and isinstance(n.targets[0], ast.Name) and isinstance(n.value, ast.Call)
               and dotted(n.value.func, aliases) == "unittest.mock.patch.dict" and n.value.args
               and dotted(n.value.args[0], aliases) == "os.environ"]
    if not patches:
        return ["a module-level mock.patch.dict(os.environ)"]
    patch = patches[0].targets[0].id
    os_names = [k for k, v in aliases.items() if v == "os"]
    gaps = []
    if bindings(tree, patch) != 1:
        gaps.append(f"a patch variable {patch} bound once")
    root = lambda node: dotted(node, {}).split(".")[0]  # noqa: E731
    used = {root(patches[0].value.func), root(patches[0].value.args[0]), *os_names}
    for name in sorted(used):
        if not anchored(tree, name):
            gaps.append(f"{name}, which the isolation block uses, bound once by a top-level import and never "
                        "rebound or imported again elsewhere")

    def expected(source):
        return ast.dump(ast.parse(source).body[0])

    def hook(name):
        defs = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == name]
        if len(defs) == 1 and not defs[0].decorator_list and bindings(tree, name) == 1:
            return defs[0].body
        gaps.append(f"a single undecorated {name}")
        return None

    setup, teardown = hook("setUpModule"), hook("tearDownModule")
    if setup is not None:
        if not (setup and ast.dump(setup[0]) == expected(f"{patch}.start()")):
            gaps.append(f"a setUpModule that starts with {patch}.start()")
        loop = setup[1] if len(setup) > 1 else None
        names = loop.iter.id if isinstance(loop, ast.For) and isinstance(loop.iter, ast.Name) else None
        pops = [expected(f"for name in {names}:\n    {o}.environ.pop(name, None)") for o in os_names]
        if names is None or ast.dump(loop) not in pops:
            gaps.append("a setUpModule whose second statement is `for name in NAMES: os.environ.pop(name, None)`")
        elif bindings(tree, names) != 1 or mutated(tree, names) or passed(tree, names):
            gaps.append(f"a list {names} bound once and never changed")
        else:
            missing = [n for n in TRUST_ENV_NAMES if n not in lists.get(names, set())]
            if missing:
                gaps.append(f"a setUpModule that pops {', '.join(missing)}")
    if teardown is not None and not (teardown and ast.dump(teardown[0]) == expected(f"{patch}.stop()")):
        gaps.append(f"a tearDownModule that starts with {patch}.stop()")
    reads = environ_gaps(tree, aliases, {id(patches[0].value.args[0])})
    if reads:
        gaps.append("no use of os.environ at import time but the read of one named variable that is not a "
                    f"trust variable (line {', '.join(map(str, reads))}); write the patch as "
                    "`with mock.patch.dict(os.environ, ...)` inside the test, or read "
                    "os.environ.get(\"<name outside the trust list>\")")
    return gaps


def trust_env_problems(root=None, reached=None):
    """Every tracked test_*.py that names a script reaching tier-digests.sh carries the isolation block
    (isolation_gaps); `reached`, when given, is filled with those tests. A script reaches it when a
    non-comment line of a tracked .sh names tier-digests.sh or a script that reaches it, as a whole file
    name. A Python intermediary is out of scope: the closure through the .py files that name a reaching
    script would cover almost every test, and a test would carry the block for a script it never runs."""
    root = root or ROOT
    tracked = [f for f in subprocess.run(["git", "-C", str(root), "ls-files", "-z"], capture_output=True,
                                         text=True, check=True).stdout.split("\0") if f]
    is_test = re.compile(r"(?:^|/)test_[^/]*\.py$")
    problems, texts = [], {}
    for f in tracked:
        if f.endswith(".sh") or is_test.search(f):
            try:
                texts[f] = COMMENT_LINE.sub("", read(root / f))
            except OSError as e:
                problems.append(f"{f}: tracked but unreadable ({e.strerror})")

    def naming(names, candidates):
        if not names:
            return set()
        found = re.compile(r"(?<![\w.-])(?:" + "|".join(map(re.escape, sorted(names))) + r")(?![\w.-])")
        return {f for f in candidates if found.search(texts[f])}

    base = lambda f: f.rsplit("/", 1)[-1]  # noqa: E731
    if TIER_DIGESTS not in texts:
        return problems + [f"{TIER_DIGESTS}: not tracked, so no test can be found that reaches it "
                           "(renamed or removed? update TIER_DIGESTS in scripts/verify.py)"]
    reaching = {TIER_DIGESTS}
    pending, rest = {base(f) for f in reaching}, {f for f in texts if not is_test.search(f)} - reaching
    while pending and rest:
        more = naming(pending, rest)
        rest -= more
        reaching |= more
        pending = {base(f) for f in more}
    names = {base(f) for f in reaching}
    tests = sorted(f for f in naming(names, [f for f in texts if is_test.search(f)]))
    if reached is not None:
        reached.extend(tests)
    shared, shared_error = set(), None
    try:
        shared = string_literals(ast.parse(read(root / SHARED_TRUST_ENV))).get("TRUST_ENV", set())
    except (OSError, SyntaxError) as e:
        shared_error = f"{SHARED_TRUST_ENV}: cannot be read as the shared TRUST_ENV ({e})"
    for f in tests:
        try:
            tree = ast.parse(read(root / f))
        except SyntaxError as e:
            problems.append(f"{f}: not parseable ({e.msg}, line {e.lineno})")
            continue
        if shared_error and "signed_registry" in texts[f]:
            problems.append(f"{f}: {shared_error}")
            continue
        gaps = isolation_gaps(tree, shared)
        if gaps:
            problems.append(f"{f}: reaches tier-digests.sh without the isolation block; it lacks "
                            + "; ".join(gaps) + f". Copy the block from {TEMPLATE_MODULE}")
    return problems


@check("test-trust-env", "A test that reaches tier-digests.sh isolates the CI run's trust environment (module-level patch, popped in setUpModule)")
def check_test_trust_env():
    """Static, so with known limits: a rebinding through globals(), exec or setattr, and a helper function called at
    import time that copies the environment, are not seen (nor a Python intermediary between a test and a
    reaching script, nor a test that reaches the script through a name built at run time). The adversarial
    class is out too: reaching os.environ without naming it, or running a script at import time; one line each:
    `from os import *`; os.environb or posix.environ; sys.modules; getattr or __import__; a mutation of the
    names list through an alias; statements after the first two of setUpModule. Refused, so not limits: a
    module-level rebinding of the os or mock name the patch statement uses, a call that passes the names list
    as an argument, and a load of os that is not an attribute base. Ambient names outside
    TRUST_ENV_NAMES are not dropped either: REGISTRY_HOST (read by tier-digests.sh in resolve), GH_HOST,
    GH_REPO, COSIGN_* and REGISTRY_AUTH_FILE. REGISTRY_HOST stays out because the block comparison needs the
    names literal of every carrying module (46 files) and signed_registry.TRUST_ENV to change with it."""
    r = Result()
    for problem in trust_env_problems():
        r.fail(problem)
    return r


def take_option(argv, name):
    """Removes `NAME VALUE` from argv and returns VALUE, None when NAME is absent."""
    if name not in argv:
        return None
    at = argv.index(name)
    if at + 1 >= len(argv) or argv[at + 1].startswith("-"):
        raise ValueError(f"{name} needs a value")
    value = argv[at + 1]
    del argv[at : at + 2]
    return value


# The branches whose runs may sign or move a tag, by the variable that overrides the default and
# the script that holds it (issue #1346). ADR-0125 keeps rebase/* out of every one of them.
TRUSTED_REF_DEFAULTS = (
    ("system/tier-digests.sh", "TIER_TRUSTED_REFS"),
    ("system/kernel-artifacts.sh", "KERNEL_TRUSTED_REFS"),
    ("system/tag-images.sh", "KERNEL_TRUSTED_REFS"),
)


def trusted_ref_problems(scripts):
    """`scripts` maps a path to its text. A default of a trusted-ref variable, `${VAR:-a b}`,
    names exactly the branch iso-v0; the retired main and any rebase/* branch are not signers."""
    out = []
    for path, var in TRUSTED_REF_DEFAULTS:
        text = scripts.get(path)
        if text is None:
            out.append(f"{path}: cannot be read")
            continue
        defaults = re.findall(r"\$\{" + var + r":-([^}]*)\}", text)
        if not defaults:
            out.append(f"{path}: has no default for {var}")
        out.extend(f"{path}: the default of {var} is '{d}', not 'iso-v0' (ADR-0125, issue #1346)"
                   for d in defaults if d.split() != ["iso-v0"])
    return out


# Files that run signing or publishing on a branch: `main` is not one of the branches that start
# them. A line holds it when a `branches: [...]` list or a `refs/heads/main` condition names it.
MAIN_BRANCH_FILES = (
    ".github/workflows/azoth-signer.yml",
    ".github/workflows/kernel-build.yml",
    "forge/scripts/publish_tiers.sh",
)
MAIN_BRANCH_LINE = re.compile(
    r"branches:\s*\[[^\]]*(?<![\w-])main(?![\w-])[^\]]*\]|refs/heads/main(?![\w-])"
    r"""|^\s*-\s*['"]?main['"]?\s*(#.*)?$|ref_name\s*==\s*['"]main['"]""")


def main_branch_problems(files):
    """`files` maps a path to its text; a comment line is not read."""
    out = []
    for path in MAIN_BRANCH_FILES:
        text = files.get(path)
        if text is None:
            out.append(f"{path}: cannot be read")
            continue
        out.extend(f"{path}:{n}: names the retired main branch (issue #1346)"
                   for n, line in enumerate(text.splitlines(), 1)
                   if not line.lstrip().startswith("#") and MAIN_BRANCH_LINE.search(line))
    return out


@check("trusted-refs", "Every trusted-ref default of the signing scripts names only iso-v0, and no signing or publishing trigger names main (issue #1346)")
def check_trusted_refs():
    r = Result()
    texts = {path: (ROOT / path).read_text() if (ROOT / path).is_file() else None
             for path in [path for path, _ in TRUSTED_REF_DEFAULTS] + list(MAIN_BRANCH_FILES)}
    for problem in trusted_ref_problems(texts) + main_branch_problems(texts):
        r.fail(problem)
    return r


def main(argv):
    argv = list(argv)
    try:
        known_red_path = take_option(argv, "--known-red")
        known_red_ref = take_option(argv, "--known-red-base")
    except ValueError as e:
        print(e, file=sys.stderr)
        return 2
    global BASE_REF
    BASE_REF = known_red_ref

    if "--list" in argv:
        for n, f in CHECKS.items():
            print(f"  {n:12s} {f.title}")
        return 0

    wanted = [a for a in argv if not a.startswith("-")] or list(CHECKS)
    unknown = [w for w in wanted if w not in CHECKS]
    if unknown:
        print(f"controllo sconosciuto: {', '.join(unknown)}", file=sys.stderr)
        print(f"disponibili: {', '.join(CHECKS)}", file=sys.stderr)
        return 2

    entries, base = [], None
    if known_red_path:
        try:
            path = Path(known_red_path)
            entries = parse_known_red(path.read_text())
            if known_red_ref:
                relative = path.resolve().relative_to(ROOT).as_posix()
                base = load_known_red_base(ROOT, known_red_ref, relative)
        except (OSError, ValueError) as e:
            print(f"known-red: {e}", file=sys.stderr)
            return 2

    failed = 0
    problems = 0
    print(f"{BOLD}Athanor OS — controlli strutturali{OFF}  {DIM}({ROOT}){OFF}\n")

    results = {name: CHECKS[name]() for name in wanted}
    tolerated, list_problems = judge_known_red(
        entries, base, results, datetime.date.today()
    )

    for name, res in results.items():
        fn = CHECKS[name]
        n = len(res.problems)
        if res.ok:
            print(
                f"  {GRN}PASS{OFF}  {BOLD}{name}{OFF}  {GRN}0{OFF}  {DIM}{fn.title}{OFF}"
            )
        else:
            if name in tolerated:
                own = [(issue, expires) for check, issue, expires, _ in entries if check == name]
                issues = ", ".join(sorted({issue for issue, _ in own}))
                print(
                    f"  {YEL}KNOWN-RED{OFF}  {BOLD}{name}{OFF}  {YEL}{n}{OFF}  "
                    f"{DIM}{fn.title} ({issues}, until {min(e for _, e in own)}){OFF}"
                )
            else:
                failed += 1
                problems += n
                print(
                    f"  {RED}FAIL{OFF}  {BOLD}{name}{OFF}  {RED}{n}{OFF}  {DIM}{fn.title}{OFF}"
                )
            for pr in res.problems[:25]:
                print(f"          {pr}")
            if len(res.problems) > 25:
                print(f"          {DIM}… e altri {len(res.problems) - 25}{OFF}")
        for n in res.notes[:8]:
            print(f"          {YEL}nota{OFF} {DIM}{n}{OFF}")
        print()

    if known_red_path:
        if list_problems:
            failed += 1
            problems += len(list_problems)
            print(
                f"  {RED}FAIL{OFF}  {BOLD}known-red{OFF}  {RED}{len(list_problems)}{OFF}  "
                f"{DIM}{known_red_path}{OFF}"
            )
            for pr in list_problems:
                print(f"          {pr}")
        else:
            print(
                f"  {GRN}PASS{OFF}  {BOLD}known-red{OFF}  {GRN}0{OFF}  {DIM}{known_red_path}{OFF}"
            )
        print()

    total = len(wanted)
    if failed:
        print(f"{BOLD}Problemi totali: {problems}{OFF}")
        print(
            f"{RED}{failed} controlli falliti su {total}.{OFF} "
            f"Ogni riga sopra cita file:riga: nessuna va interpretata."
        )
    else:
        print(
            f"{GRN}{total}/{total} controlli superati"
            f"{f', {len(tolerated)} noti rossi' if tolerated else ''}.{OFF}"
        )
    return failed


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
