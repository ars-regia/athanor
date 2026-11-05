#!/usr/bin/env python3
"""Detect a deleted or altered off-GitHub evidence copy (#401, ADR-0106 item 4, secrets.md SEC16).

The CI robot token of `EVIDENCE_REGISTRY` can delete tags on quay.io, so the copy of a promotion's
signed evidence bundle can vanish; it cannot be forged, because the bundle digest is signed
keylessly and recorded in Rekor. This lists every promotion in the release history, a promotion
run of promote-stable.yml whose `promote` job succeeded with a successful GitHub deployment in
the environment `release` (a successful deployment there of any other job, of a job that did
not succeed, or whose log_url names no job of this repository fails, and so do a run with an
ISO deployment and no promote one, and finding no promotion),
and for each one:
  1. resolves its copy, `$EVIDENCE_REGISTRY/athanor-evidence:<build run>-<promotion run>`; a
     promotion with no such tag fails: the copy was deleted;
  2. pulls it and runs `system/evidence-bundle.sh verify-dir` on it, the check `verify` makes
     after its pull, against the plan the promotion approved: the `promotion.json` of the
     bundle `evidence-<build run>-<promotion run>.tar` on its GitHub Release (ADR-0131). Only
     the promotions of PRE_RELEASE_PROMOTIONS, made before the Releases, use the copy's own
     signed plan; any other promotion with no bundle on a Release fails;
  3. fails when the copy does not verify, or when its signed plan names another build run or
     another promotion run than its tag.
A registry, a bundle or the GitHub API that cannot be read fails the check: nothing passes unread.
A deployment, a promotion or a build tag that cannot be read is a finding of its own, and the
others are still checked.

Usage: check_evidence_copies.py
Environment: EVIDENCE_REGISTRY; GITHUB_REPOSITORY; GITHUB_SERVER_URL (default https://github.com);
GH_TOKEN with contents: read, actions: read and deployments: read. Needs skopeo, gh, cosign and
bash on PATH. Read-only: the registry is read anonymously.
"""

import json
import os
import pathlib
import re
import subprocess
import sys
import tarfile
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
# The `name:` of the two jobs of promote-stable.yml in the environment `release`: the promote
# job, which moves :stable, and the ISO job after it. A successful deployment of any other job
# there, or of one of these that did not succeed, is a release history this check cannot read.
PROMOTE_JOB = "Point stable at the run"
ISO_JOB = "Point the installer ISO's stable at the run"
ENVIRONMENT = "release"
EVIDENCE_IMAGE = "athanor-evidence"
# The promotions made before each promotion attached its bundle to a GitHub Release (ADR-0131;
# the first Release, promoted-40078211020, is of 2026-10-24). No Release carries their bundle, so
# their copy is verified against its own signed plan. Every later promotion must have its bundle
# on a Release: one without it fails, rather than silently falling back to the copy alone.
PRE_RELEASE_PROMOTIONS = (38546669761, 39018812032)


class Unreadable(Exception):
    """A registry, a bundle or the API could not be read: the check fails."""


def run(*cmd, env=None):
    r = subprocess.run(cmd, capture_output=True, text=True, check=False, env=env)
    if r.returncode != 0:
        raise Unreadable(f"{' '.join(cmd)}: {r.stderr.strip()}")
    return r.stdout


def gh_lines(path, jq):
    return run("gh", "api", "--paginate", path, "--jq", jq).splitlines()


def promotions(repo, server):
    """(the promotion runs whose promote job succeeded in `release`, oldest first, findings).

    Every successful deployment status in `release` must name a successful promote or ISO job
    of a run of this repository; one that does not is a finding, never skipped. A run with an
    ISO deployment and no promote deployment is a finding too: its promotion would go unchecked."""
    found, iso, bad = set(), set(), []
    for dep in gh_lines(
        f"repos/{repo}/deployments?environment={ENVIRONMENT}", ".[].id"
    ):
        try:
            urls = gh_lines(
                f"repos/{repo}/deployments/{dep}/statuses",
                '.[] | select(.state == "success") | .log_url',
            )
            for url in urls:
                m = re.fullmatch(
                    rf"{re.escape(server)}/{re.escape(repo)}/actions/runs/(\d+)/job/(\d+)",
                    url,
                )
                if not m:
                    bad.append(
                        f"deployment {dep}: a successful status in {ENVIRONMENT} whose log_url {url!r} names no job of a run of {repo}"
                    )
                    continue
                job = gh_lines(
                    f"repos/{repo}/actions/jobs/{m[2]}", '.name + "\\t" + .conclusion'
                )
                if job == [f"{PROMOTE_JOB}\tsuccess"]:
                    found.add(int(m[1]))
                elif job == [f"{ISO_JOB}\tsuccess"]:
                    iso.add(int(m[1]))
                else:
                    bad.append(
                        f"deployment {dep}: a successful deployment in {ENVIRONMENT} of job {m[2]} of run {m[1]}, "
                        f"which is {job!r}, not a successful {PROMOTE_JOB!r} or {ISO_JOB!r}"
                    )
        except Unreadable as e:
            bad.append(f"deployment {dep}: unreadable, so not checked: {e}")
    bad += [
        f"run {run}: a successful {ISO_JOB!r} deployment in {ENVIRONMENT} and no successful {PROMOTE_JOB!r} one, so its promotion is not checked"
        for run in sorted(iso - found)
    ]
    return sorted(found), bad


def release_assets(repo):
    """{asset name: Release tag} of every evidence bundle attached to a GitHub Release."""
    lines = gh_lines(
        f"repos/{repo}/releases",
        '.[] | .tag_name as $t | .assets[] | select(.name | test("^evidence-[0-9]+-[0-9]+\\\\.tar$")) | .name + "\\t" + $t',
    )
    return dict(line.split("\t", 1) for line in lines)


def unpack(archive, into):
    """Extract a tar archive; the data filter refuses links out of the tree and special files."""
    try:
        with tarfile.open(archive, "r:*") as tar:
            tar.extractall(into, filter="data")
    except (OSError, tarfile.TarError) as e:
        raise Unreadable(f"{archive.name} is not a readable bundle archive: {e}") from e


def fetch_copy(ref, into):
    """Pull the bundle image with skopeo and unpack its layers into `into`/copy."""
    raw = into / "raw"
    run("skopeo", "copy", f"docker://{ref}", f"dir:{raw}")
    tree = into / "copy"
    tree.mkdir()
    try:
        layers = json.loads((raw / "manifest.json").read_text())["layers"]
    except (OSError, KeyError, ValueError) as e:
        raise Unreadable(f"{ref} is not a bundle image: {e}") from e
    for layer in layers:
        unpack(raw / layer["digest"].removeprefix("sha256:"), tree)
    return tree


def fetch_release(repo, tag, asset, into):
    """Download a Release's evidence bundle and unpack it into `into`/release."""
    run("gh", "release", "download", tag, "-R", repo, "-p", asset, "-D", str(into))
    tree = into / "release"
    tree.mkdir()
    unpack(into / asset, tree)
    return tree


def check(promotion, tags, assets, evidence_repo, repo, server):
    """The failures of one promotion, as lines; empty when its copy is intact."""
    builds = [m[1] for t in tags if (m := re.fullmatch(rf"(\d+)-{promotion}", t))]
    if not builds:
        return [
            f"promotion run {promotion}: no copy {evidence_repo}:<build run>-{promotion}; it was deleted"
        ]
    bad = []
    # Without the job token, as the promotion verifies its own bundle (github-release.sh).
    env = {k: v for k, v in os.environ.items() if k not in ("GH_TOKEN", "GITHUB_TOKEN")}
    for build in builds:
        # One build tag that cannot be read does not hide the findings of the others.
        try:
            bad += check_tag(promotion, build, assets, evidence_repo, repo, server, env)
        except (Unreadable, KeyError, ValueError) as e:
            bad.append(
                f"promotion run {promotion}: {evidence_repo}:{build}-{promotion}: unreadable, so not checked: {e}"
            )
    return bad


def check_tag(promotion, build, assets, evidence_repo, repo, server, env):
    """The failures of the copy <build>-<promotion>, as lines; empty when it is intact."""
    tag = f"{build}-{promotion}"
    asset = f"evidence-{tag}.tar"
    if asset not in assets and promotion not in PRE_RELEASE_PROMOTIONS:
        return [
            f"promotion run {promotion}: no GitHub Release carries {asset}, the approved plan to verify {evidence_repo}:{tag} against"
        ]
    with tempfile.TemporaryDirectory() as tmp:
        copy = fetch_copy(f"{evidence_repo}:{tag}", pathlib.Path(tmp))
        if asset in assets:
            approved, against = (
                fetch_release(repo, assets[asset], asset, pathlib.Path(tmp)),
                f"the Release {assets[asset]}",
            )
        else:
            approved, against = (
                copy,
                "its own signed plan, a promotion made before the Releases",
            )
        v = subprocess.run(
            [
                "bash",
                str(ROOT / "system/evidence-bundle.sh"),
                "verify-dir",
                str(copy),
                str(approved),
            ],
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
        if v.returncode != 0:
            return [
                f"{evidence_repo}:{tag}: does not verify against {against}: {v.stderr.strip()}"
            ]
        try:
            plan = json.loads((copy / "promotion.json").read_text())
        except (OSError, ValueError) as e:
            raise Unreadable(
                f"{tag}: promotion.json of a verified copy is unreadable: {e}"
            ) from e
    trigger = f"{server}/{repo}/actions/runs/{promotion}"
    if str(plan.get("run_id")) != build or plan.get("trigger") != trigger:
        return [
            (
                f"{evidence_repo}:{tag}: its signed plan is build run {plan.get('run_id')!r} planned by "
                f"{plan.get('trigger')!r}, not build run {build} planned by {trigger}"
            )
        ]
    print(
        f"promotion run {promotion}: {evidence_repo}:{tag} verifies against {against}"
    )
    return []


def main():
    evidence = os.environ.get("EVIDENCE_REGISTRY", "")
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    server = os.environ.get("GITHUB_SERVER_URL") or "https://github.com"
    if not (evidence and repo):
        print("set EVIDENCE_REGISTRY and GITHUB_REPOSITORY", file=sys.stderr)
        return 2
    evidence_repo = f"{evidence}/{EVIDENCE_IMAGE}"
    try:
        runs, bad = promotions(repo, server)
        # skopeo follows the registry's Link header, so this is every tag, not the first page
        # (quay.io serves 100 a page; skopeo 1.22.3 listed all 412 of quay.io/prometheus/prometheus).
        tags = json.loads(run("skopeo", "list-tags", f"docker://{evidence_repo}"))[
            "Tags"
        ]
        assets = release_assets(repo)
    except (Unreadable, KeyError, ValueError) as e:
        print(f"unreadable, so not checked: {e}", file=sys.stderr)
        return 1
    if not runs:
        # The release history holds promotions since the first :stable of 2026-10-14: none
        # found means the history was not read, not that there is nothing to check.
        bad.append(
            f"no promotion found in the environment {ENVIRONMENT}: the release history was not read"
        )
    for promotion in runs:
        bad += check(promotion, tags, assets, evidence_repo, repo, server)
    for line in bad:
        print(line, file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
