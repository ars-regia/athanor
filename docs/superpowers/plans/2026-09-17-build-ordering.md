# Build Ordering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One Orchestrator chain turns a published kernel into signed NVIDIA modules and system images, every input identified by a verified digest, with no red cycle waiting for a human rerun.

**Architecture:** `system/kernel-artifacts.sh` is the single decider: it verifies the kernel and the module tags of the pins in the registry, writes `kernel-artifacts/kernel-artifacts.env`, and answers the ordering questions of the Orchestrator and of System Image Check. Every workflow calls it and routes on its file. The work lands in two pull requests.

- **PR A (Tasks 1–6, plus Task 8 brought forward), branch `build-ordering` into `iso-v0`:**
  - the script with its offline tests;
  - NVIDIA kmod as a reusable workflow publishing digest-bound tags;
  - the images built from the verified digests;
  - Kernel Build dispatching the Orchestrator;
  - System Image Check following the O7 table;
  - Task 8, the janitor excluding `azoth*` — moved here from PR B by the final whole-branch
    review of PR A (finding 2): once `forge-ghcr-cleanup.yml` is repaired it would delete
    the untagged cosign bundles and O2 module tags this PR's chain depends on, and PR A's
    own merge (the O10 bootstrap) is the point those tags start existing.

  PR A changes the chain as one merge, so its merge is the O10 bootstrap.
- **PR B (Tasks 7, 9), branch `build-ordering-retention` from `iso-v0` after PR A is merged and its bootstrap is green:**
  - one pruner per package set (Task 8's janitor exclusion already landed in PR A);
  - the documentation owed by spec section 5.

  The `nvidia` pruner deletes the old `<nvr>-open|legacy` tags, so it must not run before the new chain has published its own tags.
- **Task 10** is the bootstrap and the acceptance. Execution order: Tasks 1–6 and 8, Task 10 Part 1, Tasks 7 and 9, Task 10 Part 2.

**Tech Stack:** Bash (`set -euo pipefail`, `inherit_errexit`), jq, skopeo, cosign v3 (keyless, GitHub OIDC), git, Python 3 standard library `unittest` with fake tools on `PATH`, podman/buildah, GitHub Actions (`workflow_call`, `workflow_dispatch`, job-level concurrency), `gh`.

**Spec:** `docs/architecture/doc_build_ordering.md` (approved 2026-09-17). Amended by it: `docs/architecture/doc_kernel_build.md` section 10, `docs/architecture/doc_system_image.md` S6 and S8, `docs/architecture/doc_naming.md`.

## Global Constraints

- **O1:**
  - The Orchestrator is the only workflow that turns a kernel into modules and images.
  - It no longer triggers on pushes that touch only `forge/specs/azoth/**`, and it does trigger on changes to `nvidia-kmod.yml` and `nvidia-build.yml`.
  - Kernel Build, on a push, dispatches the Orchestrator on the same ref, passing the commit `sha`, only when it published a new `azoth:<nvr>` or the script does not answer `ready`.
  - NVIDIA kmod becomes `workflow_call`, called directly by the Orchestrator (not through `call-system-image.yml`), and keeps `workflow_dispatch`.
  - The Orchestrator grants `attestations: write` and `actions: read`.
  - `MODULE_SIGNING_KEY` still resolves in the kmod job that declares `environment: signing`; no secret is inherited for it.
- **O2:**
  - The tag is `azoth-nvidia:<nvr>-k<first 12 hex of the azoth:<nvr> digest>-open-<NVIDIA_OPEN_VERSION>`, and the same with `-legacy-<NVIDIA_LEGACY_VERSION>`.
  - The custom predicate records the digests of `azoth:<nvr>` and `azoth-devel:<nvr>` actually pulled.
  - A tag that exists with a valid signature and attestation is never overwritten. A tag without them may be overwritten.
- **O3:**
  - The file holds `state=ready`, `state=modules-missing` or `state=kernel-missing`, followed by the verified digests.
  - Exit 0 for all three states; non-zero only for an error (registry, Rekor, network, malformed data).
  - Registry and Rekor calls go through `retry.sh`; a persistent outage is a red run, never a skip.
  - In workflows, a step runs the script and copies `state` into a job output, a two-line step with no decision logic.
- **O4:**
  - Jobs in order: `kernel-artifacts`, then `nvidia-kmod` (only on `modules-missing`, receiving the kernel digest), then `kernel-artifacts-final` (requires `ready`; the single source of the digests), then `build-repo` and `dag-system-image`.
  - Tier 0 pulls `azoth@<digest>`; `system/build-image.sh` passes the module digests as build arguments.
  - The image jobs run when `has_changes` is true, or step 1 did not answer `ready`, or the dispatch sets `force_image`. Kernel Build always sets it.
  - `kernel-missing` at step 1:
    - on a push, when `before..after` touches Kernel Build's path filters, a notice and no image, otherwise red; a zero `before` is red;
    - on a dispatch from Kernel Build, a notice if the branch HEAD moved past `sha`, otherwise red;
    - on the schedule, red.
- **O5:**
  - kmod runs the script at its start; with `ready` it ends with a notice, no build and no approval.
  - It takes the kernel digest as an input, or uses the one resolved.
  - `nvidia-build.yml`, `sign` and `boot` pull `azoth-devel` and `azoth` by digest.
  - If the kernel of the current HEAD is not published, a notice.
- **O6:**
  - The Orchestrator has `cancel-in-progress: false`.
  - kmod has no workflow-level concurrency group.
  - The jobs from signing to publication, retention and verification share the job-level group `azoth-nvidia-publish` with `cancel-in-progress: false`.
  - Only NVIDIA kmod prunes `azoth-nvidia`, inside that group. It keeps every tag whose attestation matches a retained `azoth` release, and the tags referenced by the last published system images on each branch.
  - `forge-ghcr-cleanup.yml` excludes `azoth*`.
- **O7:**
  - `ready` builds the three images.
  - `kernel-missing` with a diff touching only the pin files skips every build and the package delta, with a warning.
  - `modules-missing` with a diff moving only the NVIDIA pins builds the default image, skips the two variants, with a warning.
  - Any other `kernel-missing` or `modules-missing` fails, and so does an error.
- **O8:** pure kernel or NVIDIA pin bumps keep auto-merge on a green prep; bumps touching `system/Containerfile` or `system/nvidia/locks` stay without auto-merge. No code change: `kernel-bump.yml` already behaves so.
- **O9:**
  - The `FROM` lines this touches take registry and owner from an `ARG` with the current value as default, and the script reads the same variable (`KERNEL_REGISTRY`, default `ghcr.io/hr-mes`).
  - No decision logic is repeated in YAML.
- **O10:** the merge of PR A triggers Kernel Build and the Orchestrator. The push run stops with a notice, and the dispatched run gets `modules-missing`, calls kmod once and builds the images.
- **Observed tool behaviour (2026-09-17, cosign v3.1.3, skopeo on ghcr.io):**
  - `skopeo inspect` of a missing tag exits 2 with `manifest unknown`; a DNS or connection failure exits 1 with other text.
  - `cosign verify` of an unsigned image exits 10 with `no signatures found`.
  - A missing attestation exits 1 with `no matching attestations: ` and nothing after it.
  - A signature or attestation from another identity exits 1 with `no matching attestations: failed to verify certificate identity: no matching CertificateIdentity found`.
  - Signatures and attestations of `azoth-nvidia` are made by `https://github.com/hr-mes/athanor/.github/workflows/nvidia-kmod.yml@refs/heads/iso-v0`.
  - The custom predicate is read with `.payload | @base64d | fromjson | .predicate.Data | fromjson`.
  - `podman build` never pulls a stage the target does not use, even when its `FROM` expands an unset digest `ARG`; `FROM ${KERNEL_REGISTRY}/azoth-nvidia@${NVIDIA_OPEN_DIGEST}` pulls and copies the modules.
- **Project rules:**
  - English for code, comments, commits and new documentation; Italian stays inside `doc_kernel_build.md`, `doc_naming.md` and `KERNEL.md`.
  - Formal, idiomatic solutions: no `|| true`, no `continue-on-error`, no band-aid that hides a failure.
  - Pipeline portable: logic lives in scripts under the repository; workflow YAML only checks out, calls scripts and uploads their output, with no `run:` block beyond a few lines. Steps exchange data through files in a known directory (`kernel-artifacts/`, `nvidia-publish/`). No hard-coded `ghcr.io/hr-mes`: a variable with a default.
  - Zero-trust: signature and attestation checks are real `cosign verify` and `cosign verify-attestation` calls, bound to the publishing workflow's identity; no placeholder in a security path.
  - Validate every workflow change locally with `actionlint`, `python3 scripts/verify.py workflows` and `bash -n` (plus `shellcheck` on scripts).
  - Never push `forge/**` while an Orchestrator cycle runs (`gh run list --workflow athanor-forge-orchestrator.yml --status in_progress`).
  - One commit per problem. No `cd` in commands; `python3 -B`.
- **Commits** end with
  `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4`
- **Scratch:** `SCRATCH=/tmp/claude-1000/-var-home-hr-mes-athanor/a017eddd-482c-4c5e-9fdb-3bfc898b9a39/scratchpad`. Commands that reach ghcr.io or Sigstore need the Claude Code sandbox disabled.

## File Structure

| Path | Responsibility | Task |
| --- | --- | --- |
| `system/kernel-artifacts.sh` | resolve and verify the kernel and module artifacts of the pins, write `kernel-artifacts/kernel-artifacts.env`, decide the Orchestrator cycle (O4) and the check plan (O7), expose `get`, `has` and retried registry probes | 1, 2 |
| `system/tests/fake_registry.py` | offline `skopeo`, `cosign` and `gh` answering from a JSON fixture | 1 |
| `system/tests/test_kernel_artifacts.py` | resolve states and errors, cycle and check-plan decisions | 1, 2 |
| `.github/workflows/call-lint.yml`, `.gitignore` | run `system/tests`; ignore `kernel-artifacts/` and `nvidia-publish/` | 1 |
| `forge/specs/azoth/nvidia-publish.sh` | push, SBOM, sign and attest the module tags of O2, skipping published ones | 3 |
| `.github/workflows/nvidia-kmod.yml` | reusable workflow: artifacts check, build, sign, boot, publish in `azoth-nvidia-publish` | 3 |
| `.github/workflows/nvidia-build.yml` | `devel-digest` input, pull by digest | 3 |
| `.github/workflows/kernel-build.yml` | devel digest for the kmod gate (3); dispatch of the Orchestrator (5); `retention.sh kernel` (7) | 3, 5, 7 |
| `system/Containerfile`, `system/build-image.sh`, `system/tests/test_build_image.py` | modules by digest from `KERNEL_REGISTRY`; digests read from the file; labels | 4 |
| `forge/scripts/fetch_repo_rpms.sh`, `forge/Justfile` | tier 0 pulls `azoth@<digest>` | 4 |
| `.github/workflows/athanor-forge-orchestrator.yml` | inputs `sha` and `force_image`, permissions, concurrency, the three kernel jobs (4); trigger paths (5) | 4, 5 |
| `.github/workflows/call-system-image.yml` | download the verified file; builder image by owner | 4 |
| `.github/workflows/system-image-check.yml` | resolver job on a GitHub runner (4); O7 plan (6) | 4, 6 |
| `forge/specs/azoth/retention.sh`, `system/tests/test_azoth_retention.py` | `kernel` and `nvidia` pruners | 7 |
| `forge/scripts/clean_ghcr.sh`, `.github/workflows/forge-ghcr-cleanup.yml`, `system/tests/test_clean_ghcr.py` | janitor as a script, `azoth*` excluded | 8 |
| `docs/architecture/doc_kernel_build.md`, `doc_system_image.md`, `doc_naming.md`, `forge/specs/azoth/KERNEL.md` | spec section 5 | 9 |

---

## PR A — the ordering chain (branch `build-ordering`, into `iso-v0`)

### Task 1: `system/kernel-artifacts.sh resolve` and the offline registry

**Files:**
- Create: `system/kernel-artifacts.sh`, `system/tests/fake_registry.py`, `system/tests/test_kernel_artifacts.py`
- Modify: `.github/workflows/call-lint.yml` (append one step), `.gitignore` (two lines)

**Interfaces:**
- Produces:
  - `bash system/kernel-artifacts.sh resolve [--expect-kernel-digest sha256:…]` writes `$KERNEL_ARTIFACTS_DIR/kernel-artifacts.env` (default `<repo>/kernel-artifacts/`). It deletes any previous file first, so an error leaves none.
  - Keys in order:
    - `state`, `nvr`, `registry`;
    - then, unless `kernel-missing`: `kernel_digest`, `devel_digest`, `nvidia_open_version`, `nvidia_open_tag`, `[nvidia_open_digest]`, `nvidia_legacy_version`, `nvidia_legacy_tag`, `[nvidia_legacy_digest]`.
    - A `nvidia_<branch>_digest` line exists only for a verified branch.
  - `require-ready`: resolve, exit 1 unless `state=ready`.
  - `get KEY`: print the value, exit 1 if the key is absent.
  - `has KEY`: exit 0 if the value is non-empty.
  - `digest REF`: print `sha256:…`, or nothing for a missing tag.
  - `signed REF kernel|modules`: print `signed` or `unsigned`.
  - `predicates REF modules`: print one predicate JSON per line, or `unverified`.
  - The three probes above retry through `forge/scripts/retry.sh` (`RETRY_ATTEMPTS`); `probe digest|signed|predicates …` is their single attempt.
  - Identities: `kernel` = `^<server>/<repo>/\.github/workflows/kernel-build\.yml@refs/heads/`, `modules` = the same with `nvidia-kmod\.yml`.
  - A module branch is verified when its tag exists, is signed by `modules`, and has a custom predicate with `.driver == <branch>`, `.kernel_digest == kernel_digest`, and `.pins` equal to every `NVIDIA_<BRANCH>_*` pin of `pins.env`.
  - The kernel counts as present when `azoth:<nvr>` and `azoth-devel:<nvr>` exist and both are signed by `kernel`; otherwise `kernel-missing`.
  - Environment: `KERNEL_ARTIFACTS_DIR`, `KERNEL_REGISTRY` (default `ghcr.io/<lowercase GITHUB_REPOSITORY_OWNER or hr-mes>`), `GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`.
  - `system/tests/fake_registry.py`: tools linked by name, fixture in `FAKE_REGISTRY`, calls appended to `FAKE_LOG` as JSON arrays.
  - `test_kernel_artifacts.Tool`: the test base class (tmp dir, fakes on `PATH`, `self.env`, `self.registry(fx)`, `self.run_script(*args, cwd=None)`, `self.state_file()`), plus constants `REG`, `KMOD`, `KERNEL_BUILD`, `NVR`, `PINS`. Tasks 2, 7 and 8 import them.

- [ ] **Step 1: Write the offline registry**

`system/tests/fake_registry.py` (mode 0755):

```python
#!/usr/bin/env python3
"""Offline stand-in for skopeo, cosign and gh in the tests under system/tests.

Linked under the tool's name on PATH, it answers from the JSON fixture named by FAKE_REGISTRY
and appends every call to FAKE_LOG. Its error messages are the ones cosign v3.1.3 and skopeo
print against ghcr.io (observed 2026-09-17), because system/kernel-artifacts.sh classifies them.

Fixture keys:
  tags          {"registry/repo:tag" or "registry/repo@digest": "sha256:..."}
  errors        ["ref", ...]  transport failure for that reference, in every tool
  signatures    {"registry/repo@digest": "signing workflow identity"}
  attestations  {"registry/repo@digest": [{"identity": "...", "predicate": {...}}]}
  configs       {"registry/repo@digest": {label: value}}
  raw           {"registry/repo:tag": manifest JSON}
  packages      {"package": [package versions as the GitHub API returns them]}
  user_packages ["package", ...]  the container packages of the owner
  runs          [{"databaseId": 1, "headBranch": "iso-v0"}]
"""

import base64
import json
import os
import re
import sys


def fail(message, code=1):
    print(message, file=sys.stderr)
    return code


def skopeo(args, fx):
    ref = args[-1].removeprefix("docker://")
    if ref in fx.get("errors", []):
        return fail(f'time="2026-09-17T00:00:00Z" level=fatal msg="Error parsing image name \\"docker://{ref}\\": pinging container registry: dial tcp: i/o timeout"')
    unknown = f'time="2026-09-17T00:00:00Z" level=fatal msg="Error parsing image name \\"docker://{ref}\\": reading manifest in {ref}: manifest unknown"'
    if "--config" in args:
        if ref not in fx.get("configs", {}):
            return fail(unknown, 2)
        print(json.dumps({"config": {"Labels": fx["configs"][ref]}}))
        return 0
    if "--raw" in args:
        if ref not in fx.get("raw", {}):
            return fail(unknown, 2)
        print(json.dumps(fx["raw"][ref]))
        return 0
    digest = fx.get("tags", {}).get(ref)
    if digest is None:
        return fail(unknown, 2)
    print(digest)
    return 0


def cosign(args, fx):
    ref = args[-1]
    if ref in fx.get("errors", []):
        return fail("Error: getting trusted root: GET https://tuf-repo-cdn.sigstore.dev/timestamp.json: 502 Bad Gateway")
    regex = args[args.index("--certificate-identity-regexp") + 1]
    if args[0] == "verify":
        identity = fx.get("signatures", {}).get(ref)
        if identity is None:
            return fail("Error: no signatures found\nerror during command execution: no signatures found", 10)
        if not re.search(regex, identity):
            return fail(f'Error: no matching attestations: failed to verify certificate identity: no matching CertificateIdentity found, last error: expected SAN value to match regex "{regex}", got "{identity}"')
        return 0
    if args[0] == "verify-attestation":
        entries = [e for e in fx.get("attestations", {}).get(ref, []) if re.search(regex, e["identity"])]
        if not entries:
            return fail("Error: no matching attestations: \nerror during command execution: no matching attestations: ")
        for entry in entries:
            statement = {"predicateType": "https://cosign.sigstore.dev/attestation/v1",
                         "predicate": {"Data": json.dumps(entry["predicate"]), "Timestamp": "2026-09-17T00:00:00Z"}}
            print(json.dumps({"payloadType": "application/vnd.in-toto+json",
                              "payload": base64.b64encode(json.dumps(statement).encode()).decode()}))
        return 0
    return fail(f"fake cosign: unsupported call {args}", 2)


def gh(args, fx):
    if args[:2] == ["run", "list"]:
        print(json.dumps(fx.get("runs", [])))
        return 0
    if args[0] == "api":
        if "DELETE" in args:
            return 0
        path = next(a for a in args[1:] if a.startswith("/"))
        if path.startswith("/users/") and path.split("?")[0].endswith("/packages"):
            print(json.dumps([{"name": name} for name in fx.get("user_packages", [])]))
            return 0
        match = re.match(r"/users/[^/]+/packages/container/([^/?]+)(/versions)?", path)
        if not match or match.group(1) not in fx.get("packages", {}):
            return fail("gh: Not Found (HTTP 404)")
        if match.group(2):
            print(json.dumps(fx["packages"][match.group(1)]))
        return 0
    return fail(f"fake gh: unsupported call {args}", 2)


def main():
    tool = os.path.basename(sys.argv[0])
    args = sys.argv[1:]
    with open(os.environ["FAKE_REGISTRY"]) as f:
        fx = json.load(f)
    with open(os.environ["FAKE_LOG"], "a") as log:
        log.write(json.dumps([tool] + args) + "\n")
    return {"skopeo": skopeo, "cosign": cosign, "gh": gh}[tool](args, fx)


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 2: Write the failing tests**

`system/tests/test_kernel_artifacts.py`:

```python
"""Unit tests of system/kernel-artifacts.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import os
import pathlib
import re
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPT = ROOT / "system" / "kernel-artifacts.sh"
PINS = dict(re.findall(r"^(\w+)=(.*)$", (ROOT / "forge/specs/azoth/pins.env").read_text(), re.M))
NVR = subprocess.run(["bash", str(ROOT / "forge/specs/azoth/nvr.sh")], capture_output=True, text=True, check=True).stdout.strip()
REG = "ghcr.io/hr-mes"
KERNEL = "sha256:" + "1" * 64
DEVEL = "sha256:" + "2" * 64
OTHER_KERNEL = "sha256:" + "9" * 64
MODULE = {"open": "sha256:" + "3" * 64, "legacy": "sha256:" + "4" * 64}
KERNEL_BUILD = "https://github.com/hr-mes/athanor/.github/workflows/kernel-build.yml@refs/heads/iso-v0"
KMOD = "https://github.com/hr-mes/athanor/.github/workflows/nvidia-kmod.yml@refs/heads/iso-v0"


def tag(branch, kernel=KERNEL):
    return f"{NVR}-k{kernel[7:19]}-{branch}-{PINS[f'NVIDIA_{branch.upper()}_VERSION']}"


def predicate(branch, kernel=KERNEL):
    return {"driver": branch, "version": PINS[f"NVIDIA_{branch.upper()}_VERSION"], "kernel": f"{NVR}.x86_64",
            "kernel_digest": kernel, "devel_digest": DEVEL,
            "pins": {k: v for k, v in PINS.items() if k.startswith("NVIDIA_")}}


def published(branches=("open", "legacy")):
    """A registry holding the signed kernel of the pins and the attested modules of BRANCHES."""
    fx = {
        "tags": {f"{REG}/azoth:{NVR}": KERNEL, f"{REG}/azoth-devel:{NVR}": DEVEL},
        "signatures": {f"{REG}/azoth@{KERNEL}": KERNEL_BUILD, f"{REG}/azoth-devel@{DEVEL}": KERNEL_BUILD},
        "attestations": {},
        "errors": [],
    }
    for branch in branches:
        ref = f"{REG}/azoth-nvidia@{MODULE[branch]}"
        fx["tags"][f"{REG}/azoth-nvidia:{tag(branch)}"] = MODULE[branch]
        fx["signatures"][ref] = KMOD
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": predicate(branch)}]
    return fx


class Tool(unittest.TestCase):
    """A temporary directory with the fakes on PATH, the artifacts directory and a git identity."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        for tool in ("skopeo", "cosign", "gh"):
            (bin_dir / tool).symlink_to(HERE / "fake_registry.py")
        self.artifacts = self.dir / "artifacts"
        self.env = {
            "PATH": f"{bin_dir}:{os.environ['PATH']}", "HOME": str(self.dir), "RETRY_ATTEMPTS": "1",
            "KERNEL_ARTIFACTS_DIR": str(self.artifacts), "FAKE_REGISTRY": str(self.dir / "registry.json"),
            "FAKE_LOG": str(self.dir / "calls.log"), "GITHUB_REPOSITORY_OWNER": "hr-mes",
            "GITHUB_REPOSITORY": "hr-mes/athanor", "GITHUB_SERVER_URL": "https://github.com",
            "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
        }
        self.registry({})

    def tearDown(self):
        self.tmp.cleanup()

    def registry(self, fx):
        (self.dir / "registry.json").write_text(json.dumps(fx))

    def run_script(self, *args, cwd=None):
        return subprocess.run(["bash", str(SCRIPT), *args], capture_output=True, text=True, env=self.env, cwd=cwd or self.dir)

    def state_file(self):
        path = self.artifacts / "kernel-artifacts.env"
        return dict(line.split("=", 1) for line in path.read_text().splitlines()) if path.exists() else None


class Resolve(Tool):
    def test_ready_records_every_digest_and_the_tag_form(self):
        self.registry(published())
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "ready")
        self.assertEqual((got["kernel_digest"], got["devel_digest"]), (KERNEL, DEVEL))
        self.assertEqual((got["nvidia_open_digest"], got["nvidia_legacy_digest"]), (MODULE["open"], MODULE["legacy"]))
        self.assertEqual(got["nvidia_open_tag"], f"{NVR}-k{'1' * 12}-open-{PINS['NVIDIA_OPEN_VERSION']}")

    def test_missing_module_tag_is_modules_missing(self):
        self.registry(published(branches=("open",)))
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "modules-missing")
        self.assertEqual(got["nvidia_open_digest"], MODULE["open"])
        self.assertNotIn("nvidia_legacy_digest", got)
        self.assertEqual(got["nvidia_legacy_tag"], tag("legacy"))

    def test_absent_kernel_is_kernel_missing(self):
        fx = published()
        del fx["tags"][f"{REG}/azoth:{NVR}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file(), {"state": "kernel-missing", "nvr": NVR, "registry": REG})

    def test_unsigned_kernel_is_kernel_missing(self):
        fx = published()
        del fx["signatures"][f"{REG}/azoth@{KERNEL}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_registry_error_fails_and_leaves_no_file(self):
        self.registry(published())
        self.assertEqual(self.run_script("resolve").returncode, 0)
        fx = published()
        fx["errors"].append(f"{REG}/azoth-nvidia:{tag('legacy')}")
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("i/o timeout", r.stderr)
        self.assertIsNone(self.state_file())

    def test_rekor_error_fails(self):
        fx = published()
        fx["errors"].append(f"{REG}/azoth-nvidia@{MODULE['open']}")
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("502 Bad Gateway", r.stderr)
        self.assertIsNone(self.state_file())

    def test_unsigned_module_tag_is_modules_missing(self):
        fx = published()
        del fx["signatures"][f"{REG}/azoth-nvidia@{MODULE['legacy']}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_legacy_digest", self.state_file())

    def test_module_signed_by_another_workflow_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        fx["signatures"][ref] = KERNEL_BUILD
        fx["attestations"][ref] = [{"identity": KERNEL_BUILD, "predicate": predicate("open")}]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_attestation_for_another_kernel_digest_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": predicate("open", kernel=OTHER_KERNEL)}]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_attestation_for_other_nvidia_pins_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        stale = predicate("open")
        stale["pins"]["NVIDIA_OPEN_COMMIT"] = "0" * 40
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": stale}]
        self.registry(fx)
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "modules-missing")

    def test_republished_kernel_fails_against_the_expected_digest(self):
        self.registry(published())
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("republished", r.stderr)

    def test_require_ready(self):
        self.registry(published(branches=("open",)))
        self.assertEqual(self.run_script("require-ready").returncode, 1)
        self.registry(published())
        self.assertEqual(self.run_script("require-ready").returncode, 0)

    def test_get_and_has(self):
        self.registry(published(branches=("open",)))
        self.run_script("resolve")
        self.assertEqual(self.run_script("get", "kernel_digest").stdout.strip(), KERNEL)
        self.assertEqual(self.run_script("get", "nvidia_legacy_digest").returncode, 1)
        self.assertEqual(self.run_script("has", "nvidia_open_digest").returncode, 0)
        self.assertEqual(self.run_script("has", "nvidia_legacy_digest").returncode, 1)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `chmod 0755 system/tests/fake_registry.py && python3 -B -m unittest discover -s system/tests -v`

Expected: 13 tests, every one FAIL or ERROR; the stderr of `bash` shows `No such file or directory` for `system/kernel-artifacts.sh`.

- [ ] **Step 4: Write `system/kernel-artifacts.sh`** (mode 0755)

```bash
#!/usr/bin/env bash
# The kernel and NVIDIA module artifacts of the current pins, verified in the registry and
# identified by digest (docs/architecture/doc_build_ordering.md, O2-O5, O7). Every workflow of
# the build ordering runs this script and reads its file; none repeats its decisions.
#
#   resolve [--expect-kernel-digest D]  write kernel-artifacts.env: state=ready, modules-missing
#                                       or kernel-missing, then the verified digests. Exit 0 for
#                                       the three states; 1 on a registry, Rekor, network or data
#                                       error, or when azoth:<nvr> is not D (republished since)
#   require-ready                       resolve, then exit 1 unless state=ready
#   get KEY                             print the value of KEY; exit 1 when the file lacks it
#   has KEY                             exit 0 when KEY has a non-empty value
#   digest REF                          the digest of REF, empty when the tag does not exist
#   signed REF kernel|modules           signed or unsigned, by the workflow that publishes it
#   predicates REF modules              the custom predicates of REF, one JSON per line, or
#                                       unverified
#   probe digest|signed|predicates ...  one attempt of the three above (they retry it)
#
# The file is $KERNEL_ARTIFACTS_DIR/kernel-artifacts.env (default: kernel-artifacts/ at the
# repository root). KERNEL_REGISTRY is the registry and owner (default ghcr.io/ followed by
# GITHUB_REPOSITORY_OWNER, else hr-mes); GITHUB_SERVER_URL and GITHUB_REPOSITORY name the
# workflows whose signatures are trusted. Needs skopeo, cosign and jq.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SELF=$HERE/$(basename "${BASH_SOURCE[0]}")
ROOT=$(dirname "$HERE")
DIR=${KERNEL_ARTIFACTS_DIR:-$ROOT/kernel-artifacts}
FILE=$DIR/kernel-artifacts.env
PINS=$ROOT/forge/specs/azoth/pins.env
owner=${GITHUB_REPOSITORY_OWNER:-hr-mes}
REGISTRY=${KERNEL_REGISTRY:-ghcr.io/${owner,,}}
ISSUER=https://token.actions.githubusercontent.com
workflows="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-hr-mes/athanor}/.github/workflows"
# Owner, repository and host names hold no regex metacharacter other than the dot.
workflows=${workflows//./\\.}
declare -A IDENTITY=(
  [kernel]="^${workflows}/kernel-build\.yml@refs/heads/"
  [modules]="^${workflows}/nvidia-kmod\.yml@refs/heads/"
)
# cosign v3 reports a missing or foreign signature or attestation with these messages. Any
# other failure (registry, Rekor, TUF, network) is an error, never a missing artifact.
UNVERIFIED='no signatures found|no matching signatures|no matching attestations: *$|no matching CertificateIdentity'

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

usage() { sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$SELF" >&2; exit 2; }
die() { echo "kernel-artifacts: $*" >&2; exit 1; }
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }
ask() { retry bash "$SELF" probe "$@"; }
identity() { [[ -n ${IDENTITY[${1:-}]:-} ]] || usage; echo "${IDENTITY[$1]}"; }

probe_digest() {
  local out status=0
  out=$(skopeo inspect --format '{{.Digest}}' "docker://$1" 2> "$TMP/err") || status=$?
  if [[ $status -ne 0 ]]; then
    grep -q 'manifest unknown' "$TMP/err" && return 0
    cat "$TMP/err" >&2
    return 1
  fi
  [[ $out =~ ^sha256:[0-9a-f]{64}$ ]] || die "$1: malformed digest '$out'"
  echo "$out"
}

probe_signed() {
  local status=0
  cosign verify --certificate-identity-regexp "$(identity "$2")" --certificate-oidc-issuer "$ISSUER" "$1" > /dev/null 2> "$TMP/err" || status=$?
  if [[ $status -eq 0 ]]; then
    echo signed
  elif grep -qE "$UNVERIFIED" "$TMP/err"; then
    echo unsigned
  else
    cat "$TMP/err" >&2
    return 1
  fi
}

probe_predicates() {
  local status=0
  cosign verify-attestation --type custom --certificate-identity-regexp "$(identity "$2")" --certificate-oidc-issuer "$ISSUER" "$1" > "$TMP/out" 2> "$TMP/err" || status=$?
  if [[ $status -eq 0 ]]; then
    jq -ce '.payload | @base64d | fromjson | .predicate.Data | fromjson' "$TMP/out" || die "$1: malformed attestation"
  elif grep -qE "$UNVERIFIED" "$TMP/err"; then
    echo unverified
  else
    cat "$TMP/err" >&2
    return 1
  fi
}

get() {
  local line
  [[ -f $FILE ]] || die "$FILE does not exist: run kernel-artifacts.sh resolve first"
  line=$(grep -m1 "^$1=" "$FILE") || die "$FILE has no $1"
  echo "${line#*=}"
}

write() { # write STATE LINE...: replace the file in one step
  mkdir -p "$DIR"
  printf '%s\n' "state=$1" "${@:2}" > "$FILE.tmp"
  mv "$FILE.tmp" "$FILE"
  echo "kernel-artifacts: state=$1"
}

module_verdict() { # module_verdict REF BRANCH KERNEL_DIGEST: verified or unverified
  local signed predicates pins
  signed=$(ask signed "$1" modules)
  [[ $signed == signed ]] || { echo unverified; return 0; }
  predicates=$(ask predicates "$1" modules)
  [[ $predicates != unverified ]] || { echo unverified; return 0; }
  pins=$(sed -n "s/^\(NVIDIA_${2^^}_[A-Z0-9_]*\)=\(.*\)$/\1\t\2/p" "$PINS" | jq -Rn '[inputs | split("\t") | {(.[0]): .[1]}] | add')
  jq -sr --arg branch "$2" --arg kernel "$3" --argjson pins "$pins" '
    if any(.[]; .driver == $branch and .kernel_digest == $kernel and (.pins as $p | $pins | to_entries | all(.value == $p[.key])))
    then "verified" else "unverified" end' <<< "$predicates"
}

resolve() {
  local expect='' nvr kernel devel kernel_signed devel_signed branch version tag digest verdict state=ready
  while [[ $# -gt 0 ]]; do
    case $1 in
      --expect-kernel-digest) [[ $# -ge 2 ]] || usage; expect=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  rm -f "$FILE"
  nvr=$(bash "$ROOT/forge/specs/azoth/nvr.sh")
  local -a lines=("nvr=$nvr" "registry=$REGISTRY")
  kernel=$(ask digest "$REGISTRY/azoth:$nvr")
  devel=$(ask digest "$REGISTRY/azoth-devel:$nvr")
  if [[ -z $kernel || -z $devel ]]; then
    write kernel-missing "${lines[@]}"
    return 0
  fi
  [[ -z $expect || $kernel == "$expect" ]] || die "$REGISTRY/azoth:$nvr is $kernel, the caller resolved $expect: the kernel was republished since"
  kernel_signed=$(ask signed "$REGISTRY/azoth@$kernel" kernel)
  devel_signed=$(ask signed "$REGISTRY/azoth-devel@$devel" kernel)
  if [[ $kernel_signed != signed || $devel_signed != signed ]]; then
    write kernel-missing "${lines[@]}"
    return 0
  fi
  lines+=("kernel_digest=$kernel" "devel_digest=$devel")
  for branch in open legacy; do
    version=$(sed -n "s/^NVIDIA_${branch^^}_VERSION=//p" "$PINS")
    [[ -n $version ]] || die "NVIDIA_${branch^^}_VERSION is not set in $PINS"
    tag="$nvr-k${kernel:7:12}-$branch-$version"
    lines+=("nvidia_${branch}_version=$version" "nvidia_${branch}_tag=$tag")
    digest=$(ask digest "$REGISTRY/azoth-nvidia:$tag")
    verdict=unverified
    [[ -z $digest ]] || verdict=$(module_verdict "$REGISTRY/azoth-nvidia@$digest" "$branch" "$kernel")
    if [[ $verdict == verified ]]; then
      lines+=("nvidia_${branch}_digest=$digest")
    else
      state=modules-missing
    fi
  done
  write "$state" "${lines[@]}"
}

[[ $# -ge 1 ]] || usage
command=$1
shift
case $command in
  resolve) resolve "$@" ;;
  require-ready)
    [[ $# -eq 0 ]] || usage
    resolve
    state=$(get state)
    [[ $state == ready ]] || die "state=$state: the kernel of the pins and both NVIDIA module branches must be published, signed and attested"
    ;;
  get) [[ $# -eq 1 ]] || usage; get "$1" ;;
  has) [[ $# -eq 1 ]] || usage; [[ -f $FILE ]] && grep -q "^$1=." "$FILE" ;;
  digest) [[ $# -eq 1 ]] || usage; ask digest "$1" ;;
  signed) [[ $# -eq 2 ]] || usage; ask signed "$1" "$2" ;;
  predicates) [[ $# -eq 2 ]] || usage; ask predicates "$1" "$2" ;;
  probe)
    [[ $# -ge 2 ]] || usage
    case $1 in
      digest) [[ $# -eq 2 ]] || usage; probe_digest "$2" ;;
      signed) [[ $# -eq 3 ]] || usage; probe_signed "$2" "$3" ;;
      predicates) [[ $# -eq 3 ]] || usage; probe_predicates "$2" "$3" ;;
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac
```

- [ ] **Step 5: Run the tests and the linters**

Run: `chmod 0755 system/kernel-artifacts.sh && python3 -B -m unittest discover -s system/tests -v && shellcheck system/kernel-artifacts.sh && bash -n system/kernel-artifacts.sh`

Expected: `Ran 13 tests` … `OK`; shellcheck prints nothing.

- [ ] **Step 6: Check the resolver against the live registry (read-only, sandbox disabled)**

Run: `KERNEL_ARTIFACTS_DIR=$SCRATCH/ka bash system/kernel-artifacts.sh resolve && cat $SCRATCH/ka/kernel-artifacts.env && bash system/kernel-artifacts.sh signed ghcr.io/hr-mes/azoth-nvidia:7.2.5-100.azoth.fc43-open kernel`

Expected, with today's pins:
- `state=modules-missing`, `kernel_digest=sha256:65d471b3…`, `nvidia_open_tag=7.2.5-100.azoth.fc43-k65d471b36b95-open-610.57.04`, and no `nvidia_*_digest` line (no O2 tag exists yet);
- the last command prints `unsigned`: a module image is never accepted under the kernel identity.

If the pins moved, the NVR and digest differ but the shape is the same.

- [ ] **Step 7: Run the tests in CI and ignore the output directories**

Append to `.github/workflows/call-lint.yml`, after the `NVIDIA lock and build gate (unit tests)` step:

```yaml
      - name: Build ordering scripts (unit tests against an offline registry)
        run: python3 -B -m unittest discover -s system/tests -v
```

In `.gitignore`, after the line `forge/*.tar.gz` of the `# Build Artifacts & Worktrees` block, add:

```
kernel-artifacts/
nvidia-publish/
```

Run: `actionlint .github/workflows/call-lint.yml && python3 scripts/verify.py workflows && git status --short`

Expected: actionlint silent. `verify.py workflows` reports no new failure; compare with `git stash && python3 scripts/verify.py workflows; git stash pop` if unsure. `git status` lists only the five paths of this task.

- [ ] **Step 8: Commit**

```bash
git add system/kernel-artifacts.sh system/tests/fake_registry.py system/tests/test_kernel_artifacts.py .github/workflows/call-lint.yml .gitignore
git commit -m "feat(build-ordering): resolve the kernel and NVIDIA module artifacts by verified digest" -m "system/kernel-artifacts.sh verifies azoth, azoth-devel and the digest-bound azoth-nvidia tags of the pins with cosign against the publishing workflow's identity, and writes state=ready|modules-missing|kernel-missing with the verified digests (doc_build_ordering.md, O2, O3). Registry and Rekor errors fail after retry.sh; offline tests run in call-lint.yml." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

### Task 2: The cycle of an Orchestrator run and the plan of System Image Check

**Files:**
- Modify: `system/kernel-artifacts.sh`, `system/tests/test_kernel_artifacts.py`

**Interfaces:**
- Consumes: `resolve`, `get`, `retry`, `die`, `usage` (Task 1).
- Produces:
  - `cycle --event push|workflow_dispatch|schedule [--before SHA] [--after SHA] [--sha SHA] [--head SHA]`, run in the repository checkout after `resolve`. It appends `cycle=build` or `cycle=defer` and exits 1 on a red row. With `state=ready` it always builds. Otherwise:
    - **push:**
      - `before` zero or empty: build on `modules-missing`, red on `kernel-missing`;
      - `before..after` touches `KERNEL_BUILD_PATHS`: defer with a notice;
      - otherwise build on `modules-missing`, red on `kernel-missing`.
    - **workflow_dispatch:** on `kernel-missing`, defer when `--sha` is set and differs from `--head`, otherwise red; on `modules-missing`, build.
    - **schedule:** red on `kernel-missing`, build on `modules-missing`.
  - `check-plan --base REV --head REV`, run after `resolve`. It appends `check_gpus=<space-separated GPUs>` and `check_delta=true|false` and exits 1 on a failing O7 row.
  - Pin file sets:
    - `NVIDIA_PIN_FILES`: `forge/specs/azoth/pins.env`, `forge/specs/azoth/KERNEL.md`, `forge/specs/azoth/nvidia/sources.sha256`, `system/nvidia/locks/open.lock`, `system/nvidia/locks/legacy.lock`;
    - `KERNEL_PIN_FILES`: those plus `forge/specs/azoth/SOURCES/sources.sha256` and `forge/specs/azoth/{builder,boot,nvidia}/Containerfile`.
  - `KERNEL_BUILD_PATHS`: `forge/specs/azoth/*`, `.github/workflows/kernel-build.yml`, `.github/workflows/nvidia-build.yml`. A test keeps it equal to `kernel-build.yml`.
  - `changed_files BASE HEAD` fetches `BASE` with `git fetch --no-tags --depth=1 origin` when the shallow checkout lacks it.

- [ ] **Step 1: Write the failing tests**

In `system/tests/test_kernel_artifacts.py`, insert before the final `if __name__ == "__main__":` block:

```python
class Repo(Tool):
    """A git repository with a bare origin and a shallow clone, as actions/checkout leaves it."""

    def setUp(self):
        super().setUp()
        self.origin = self.dir / "origin.git"
        self.work = self.dir / "work"
        self.git("init", "-q", "--bare", "-b", "iso-v0", str(self.origin), cwd=self.dir)
        self.git("clone", "-q", str(self.origin), str(self.work), cwd=self.dir)
        self.commit({"README.md": "x\n", "forge/specs/azoth/pins.env": "".join(f"{k}={v}\n" for k, v in PINS.items())})

    def git(self, *args, cwd=None):
        return subprocess.run(["git", *args], cwd=cwd or self.work, env=self.env, check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, files):
        for rel, text in files.items():
            path = self.work / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "change")
        self.git("push", "-q", "origin", "HEAD:iso-v0")
        return self.git("rev-parse", "HEAD")

    def shallow(self):
        """A depth-1 clone of origin, like the checkout of a push event."""
        clone = self.dir / "shallow"
        self.git("clone", "-q", "--depth=1", "-b", "iso-v0", f"file://{self.origin}", str(clone), cwd=self.dir)
        return clone

    def state(self, state, **extra):
        self.artifacts.mkdir(exist_ok=True)
        lines = {"state": state, "nvr": NVR, "registry": REG, **extra}
        (self.artifacts / "kernel-artifacts.env").write_text("".join(f"{k}={v}\n" for k, v in lines.items()))

    def pin_change(self, **pins):
        text = "".join(f"{k}={pins.get(k, v)}\n" for k, v in PINS.items())
        return {"forge/specs/azoth/pins.env": text}


class Cycle(Repo):
    def cycle(self, *args, cwd=None):
        return self.run_script("cycle", *args, cwd=cwd or self.shallow())

    def test_kernel_build_paths_match_the_workflow(self):
        text = (ROOT / ".github/workflows/kernel-build.yml").read_text()
        block = re.search(r"^  push:\n(?:    .*\n)*?    paths:\n((?:      - .*\n)+)", text, re.M).group(1)
        paths = [line.strip()[2:].replace("**", "*") for line in block.splitlines()]
        script = re.search(r"^KERNEL_BUILD_PATHS=\((.*)\)$", SCRIPT.read_text(), re.M).group(1)
        self.assertEqual(paths, re.findall(r"'([^']+)'", script))

    def test_ready_builds(self):
        self.state("ready")
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_push_starting_kernel_build_defers_a_missing_kernel(self):
        self.state("kernel-missing")
        before = self.git("rev-parse", "HEAD")
        after = self.commit({**self.pin_change(FEDORA_KERNEL_NVR="7.2.6-100.fc43"), "system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "defer")
        self.assertIn("Kernel Build", r.stdout)

    def test_push_starting_kernel_build_defers_missing_modules(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        before = self.git("rev-parse", "HEAD")
        after = self.commit({".github/workflows/kernel-build.yml": "name: x\n", "system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "defer")

    def test_push_outside_kernel_build_with_missing_kernel_fails(self):
        self.state("kernel-missing")
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not start Kernel Build", r.stderr)
        self.assertNotIn("cycle", self.state_file())

    def test_push_outside_kernel_build_with_missing_modules_builds(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_push_without_previous_commit_and_missing_kernel_fails(self):
        self.state("kernel-missing")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", "0" * 40, "--after", after)
        self.assertEqual(r.returncode, 1)
        self.assertIn("previous commit", r.stderr)

    def test_dispatch_superseded_by_a_newer_push_defers(self):
        self.state("kernel-missing")
        r = self.cycle("--event", "workflow_dispatch", "--sha", "a" * 40, "--head", "b" * 40)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "defer")

    def test_dispatch_at_head_with_missing_kernel_fails(self):
        self.state("kernel-missing")
        r = self.cycle("--event", "workflow_dispatch", "--sha", "a" * 40, "--head", "a" * 40)
        self.assertEqual(r.returncode, 1)
        self.assertIn("is not published", r.stderr)

    def test_manual_dispatch_with_missing_modules_builds(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.cycle("--event", "workflow_dispatch", "--head", "a" * 40)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_schedule_with_missing_kernel_fails(self):
        self.state("kernel-missing")
        self.assertEqual(self.cycle("--event", "schedule").returncode, 1)


class CheckPlan(Repo):
    def plan(self, files):
        self.commit(files)
        return self.run_script("check-plan", "--base", "HEAD^1", "--head", "HEAD", cwd=self.work)

    def test_ready_builds_three_images_with_the_delta(self):
        self.state("ready")
        r = self.plan({"system/x": "1\n"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state_file()["check_gpus"], self.state_file()["check_delta"]), ("none nvidia nvidia-legacy", "true"))

    def test_kernel_pin_bump_skips_every_build(self):
        self.state("kernel-missing")
        r = self.plan({**self.pin_change(FEDORA_KERNEL_NVR="7.2.6-100.fc43"), "forge/specs/azoth/SOURCES/sources.sha256": "h\n", "forge/specs/azoth/KERNEL.md": "k\n"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state_file()["check_gpus"], self.state_file()["check_delta"]), ("", "false"))
        self.assertIn("warning", r.stdout)

    def test_kernel_pin_bump_mixed_with_other_changes_fails(self):
        self.state("kernel-missing")
        r = self.plan({**self.pin_change(FEDORA_KERNEL_NVR="7.2.6-100.fc43"), "system/Containerfile": "FROM x\n"})
        self.assertEqual(r.returncode, 1)
        self.assertIn("own pull request", r.stderr)

    def test_nvidia_pin_bump_builds_the_default_image(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.plan({**self.pin_change(NVIDIA_OPEN_VERSION="615.71.09"), "system/nvidia/locks/open.lock": "l\n"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state_file()["check_gpus"], self.state_file()["check_delta"]), ("none", "true"))

    def test_missing_modules_with_unchanged_pins_fail(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.plan({"system/x": "1\n"})
        self.assertEqual(r.returncode, 1)
        self.assertIn("Orchestrator", r.stderr)

    def test_nvidia_and_kernel_pins_together_with_missing_modules_fail(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.plan(self.pin_change(NVIDIA_OPEN_VERSION="615.71.09", CACHYOS_PATCHES_COMMIT="f" * 40))
        self.assertEqual(r.returncode, 1)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `python3 -B -m unittest discover -s system/tests -v`

Expected:
- the 13 resolve tests pass;
- `test_kernel_build_paths_match_the_workflow` ERRORs (`AttributeError: 'NoneType' object has no attribute 'group'`, no `KERNEL_BUILD_PATHS` in the script);
- the other new tests FAIL, because the script prints its usage and exits 2.

- [ ] **Step 3: Add the decisions to `system/kernel-artifacts.sh`**

1. In the header, after the `require-ready` line, insert:

```bash
#   cycle --event E [--before B] [--after A] [--sha S] [--head H]
#                                       after resolve, for an Orchestrator run (O4): append
#                                       cycle=build or cycle=defer; exit 1 when the kernel is
#                                       missing and no other cycle owns it
#   check-plan --base REV --head REV    after resolve, for System Image Check (O7): append
#                                       check_gpus and check_delta; exit 1 for a failing row
```

2. Replace the header line `# workflows whose signatures are trusted. Needs skopeo, cosign and jq.` with:

```bash
# workflows whose signatures are trusted. cycle and check-plan run git in the repository
# checkout that is the current directory. Needs skopeo, cosign and jq for the registry.
```

3. After the `UNVERIFIED=…` line, insert:

```bash
# The push paths of .github/workflows/kernel-build.yml (a unit test keeps them equal).
KERNEL_BUILD_PATHS=('forge/specs/azoth/*' '.github/workflows/kernel-build.yml' '.github/workflows/nvidia-build.yml')
# The files .github/workflows/kernel-bump.yml regenerates with the pins, except
# system/Containerfile: a base bump is reviewed with its package delta (O8).
NVIDIA_PIN_FILES=(forge/specs/azoth/pins.env forge/specs/azoth/KERNEL.md forge/specs/azoth/nvidia/sources.sha256 system/nvidia/locks/open.lock system/nvidia/locks/legacy.lock)
KERNEL_PIN_FILES=("${NVIDIA_PIN_FILES[@]}" forge/specs/azoth/SOURCES/sources.sha256 forge/specs/azoth/builder/Containerfile forge/specs/azoth/boot/Containerfile forge/specs/azoth/nvidia/Containerfile)
```

4. After the `die() …` line, insert:

```bash
annotate() { # annotate notice|warning MESSAGE
  if [[ ${GITHUB_ACTIONS:-} == true ]]; then echo "::$1 title=Kernel artifacts::$2"; else echo "kernel-artifacts: $1: $2"; fi
}
```

5. Between the closing `}` of `resolve()` and the line `[[ $# -ge 1 ]] || usage`, insert:

```bash
changed_files() { # changed_files BASE HEAD
  git cat-file -e "$1^{commit}" 2> /dev/null || retry git fetch --no-tags --depth=1 origin "$1"
  git diff --name-only "$1" "$2"
}

matches_any() { # matches_any PATH PATTERN...
  local path=$1 pattern
  shift
  for pattern in "$@"; do
    # shellcheck disable=SC2053 # the right-hand side is a glob on purpose
    [[ $path == $pattern ]] && return 0
  done
  return 1
}

kernel_build_touched() { # kernel_build_touched BASE HEAD: yes or no
  local files file
  files=$(changed_files "$1" "$2")
  while IFS= read -r file; do
    if [[ -n $file ]] && matches_any "$file" "${KERNEL_BUILD_PATHS[@]}"; then
      echo yes
      return 0
    fi
  done <<< "$files"
  echo no
}

cycle() {
  local event='' before='' after='' sha='' head='' state nvr decision=build touched
  while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
      --event) event=$2 ;;
      --before) before=$2 ;;
      --after) after=$2 ;;
      --sha) sha=$2 ;;
      --head) head=$2 ;;
      *) usage ;;
    esac
    shift 2
  done
  state=$(get state)
  nvr=$(get nvr)
  if [[ $state != ready ]]; then
    case $event in
      push)
        if [[ -z $before || $before =~ ^0+$ ]]; then
          [[ $state == modules-missing ]] || die "azoth:$nvr is not published, and a push without a previous commit (new branch, force push) cannot tell whether Kernel Build owns it"
        else
          touched=$(kernel_build_touched "$before" "$after")
          if [[ $touched == yes ]]; then
            decision=defer
            annotate notice "$state for azoth:$nvr, and this push starts Kernel Build, which dispatches the Orchestrator for it: no image in this run"
          else
            [[ $state == modules-missing ]] || die "azoth:$nvr is not published and this push does not start Kernel Build"
          fi
        fi
        ;;
      workflow_dispatch)
        if [[ $state == kernel-missing ]]; then
          [[ -n $sha && $sha != "$head" ]] || die "azoth:$nvr is not published"
          decision=defer
          annotate notice "azoth:$nvr is not published and the branch moved from $sha to $head: the newer push has its own cycle"
        fi
        ;;
      schedule)
        [[ $state == modules-missing ]] || die "azoth:$nvr is not published"
        ;;
      *) die "cycle: unknown event '$event'" ;;
    esac
  fi
  echo "cycle=$decision" >> "$FILE"
  echo "kernel-artifacts: cycle=$decision"
}

check_plan() {
  local base='' head='' state nvr files file keys key only_kernel_pins=true only_nvidia_pins=true nvidia_moved=false other_moved=false gpus delta
  while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
      --base) base=$2 ;;
      --head) head=$2 ;;
      *) usage ;;
    esac
    shift 2
  done
  [[ -n $base && -n $head ]] || usage
  state=$(get state)
  nvr=$(get nvr)
  files=$(changed_files "$base" "$head")
  while IFS= read -r file; do
    [[ -n $file ]] || continue
    matches_any "$file" "${KERNEL_PIN_FILES[@]}" || only_kernel_pins=false
    matches_any "$file" "${NVIDIA_PIN_FILES[@]}" || only_nvidia_pins=false
  done <<< "$files"
  keys=$(git diff -U0 "$base" "$head" -- forge/specs/azoth/pins.env | sed -n 's/^[-+]\([A-Z_][A-Z0-9_]*\)=.*/\1/p' | sort -u)
  while IFS= read -r key; do
    [[ -n $key ]] || continue
    if [[ $key == NVIDIA_* ]]; then nvidia_moved=true; else other_moved=true; fi
  done <<< "$keys"
  case $state in
    ready)
      gpus='none nvidia nvidia-legacy' delta=true
      ;;
    kernel-missing)
      [[ $only_kernel_pins == true && -n $keys ]] || die "azoth:$nvr is not published: a pin bump mixed with other changes cannot be checked, move the pins in their own pull request"
      gpus='' delta=false
      annotate warning "azoth:$nvr is not published yet: Kernel Build on this pull request proves the kernel and the modules build and boot; the images are built after the merge"
      ;;
    modules-missing)
      [[ $only_nvidia_pins == true && $nvidia_moved == true && $other_moved == false ]] || die "the NVIDIA modules of azoth:$nvr are not published: with unchanged NVIDIA pins publishing them is the Orchestrator's job (bootstrap or interrupted publication), and NVIDIA pins move in their own pull request"
      gpus=none delta=true
      annotate warning "the NVIDIA modules of the new pins are not published yet: only the default image is built; the variants are built and gated after the merge"
      ;;
    *) die "$FILE: unknown state '$state'" ;;
  esac
  printf '%s\n' "check_gpus=$gpus" "check_delta=$delta" >> "$FILE"
  echo "kernel-artifacts: check_gpus='$gpus' check_delta=$delta"
}
```

6. In the final `case $command in`, after the `require-ready)` arm (its `;;`), insert:

```bash
  cycle) cycle "$@" ;;
  check-plan) check_plan "$@" ;;
```

The result is byte-identical to the script validated while writing this plan, reproduced in full in the appendix *Final `system/kernel-artifacts.sh`* at the end of this document.

- [ ] **Step 4: Run the tests and the linters**

Run: `python3 -B -m unittest discover -s system/tests -v && shellcheck system/kernel-artifacts.sh`

Expected: `Ran 30 tests` … `OK`; shellcheck silent.

- [ ] **Step 5: Commit**

```bash
git add system/kernel-artifacts.sh system/tests/test_kernel_artifacts.py
git commit -m "feat(build-ordering): decide the Orchestrator cycle and the System Image Check plan from the artifacts" -m "cycle defers a run whose missing kernel or modules belong to the Kernel Build cycle of the same push or of a newer commit, and fails otherwise (doc_build_ordering.md, O4, O10). check-plan maps state and pull request diff to the images to build (O7)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

### Task 3: NVIDIA kmod as a reusable workflow publishing digest-bound tags

**Files:**
- Create: `forge/specs/azoth/nvidia-publish.sh`
- Replace: `.github/workflows/nvidia-kmod.yml`
- Modify: `.github/workflows/nvidia-build.yml`, `.github/workflows/kernel-build.yml` (`inputs` and `kmod` jobs)

**Interfaces:**
- Consumes: `resolve --expect-kernel-digest`, `require-ready`, `get`, `has` (Task 1); `forge/scripts/sign_attest.sh IMAGE SBOM`; `forge/scripts/retry.sh`.
- Produces:
  - `nvidia-kmod.yml`:
    - `on.workflow_call.inputs.kernel_digest` (string, required) and `on.workflow_dispatch.inputs.kernel_digest` (string, default `""`);
    - no workflow-level `concurrency`;
    - jobs `artifacts` (outputs `state`, `devel_digest`; artifact `nvidia-kernel-artifacts`), then `build` (only on `modules-missing`), then `sign`, `boot`, `publish`, each with `concurrency: {group: azoth-nvidia-publish, cancel-in-progress: false}`;
    - `publish` has `actions: read` for the Task 7 retention.
  - `nvidia-build.yml`: `on.workflow_call.inputs.devel-digest` (string, default `""`), required when `devel-artifact` is empty.
  - `nvidia-publish.sh SIGNED_DIR`:
    - skips a branch for which `has nvidia_<branch>_digest`;
    - otherwise pushes `<registry>/azoth-nvidia:<nvidia_<branch>_tag>`, writes `nvidia-publish/{digests/<branch>,sbom/<branch>.spdx.json,pins-<branch>.json,summary.md}`, signs and attests;
    - the predicate is `{driver, version, kernel: "<nvr>.x86_64", kernel_digest, devel_digest, pins: {every NVIDIA_* pin}}`.
  - Kernel Build `inputs` job output `devel_digest`: set only when the kernel is reused.
- Interim: Kernel Build still dispatches `nvidia-kmod.yml` without inputs, and the old tags stay in place for the unchanged Orchestrator. This commit leaves the pipeline working.

- [ ] **Step 1: Write `forge/specs/azoth/nvidia-publish.sh`** (mode 0755)

```bash
#!/usr/bin/env bash
# Publication of the signed NVIDIA modules (docs/architecture/doc_kernel_build.md, section 10;
# docs/architecture/doc_build_ordering.md, O2): one scratch image per branch, with
# lib/modules/<kver>/extra/nvidia/*.ko plus `version` and `kver`, under the tag that
# system/kernel-artifacts.sh names for the kernel digest; an SPDX SBOM, a keyless signature
# and the custom attestation of the NVIDIA pins and of the azoth and azoth-devel digests the
# modules were built against. A branch whose tag kernel-artifacts.env already lists with a
# digest is published, signed and attested, and is never overwritten.
#
# Usage: nvidia-publish.sh SIGNED_DIR. Run system/kernel-artifacts.sh resolve first, inside the
# azoth-nvidia-publish concurrency group. Needs buildah and cosign logged in to the registry,
# syft, and SIGNED_DIR/<branch>/ as nvidia.sh sign leaves it. Writes nvidia-publish/: sbom/,
# digests/, pins-<branch>.json and summary.md.
set -euo pipefail
shopt -s inherit_errexit

SIGNED=${1:?usage: nvidia-publish.sh SIGNED_DIR}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
OUT=nvidia-publish
artifact() { bash "$ROOT/system/kernel-artifacts.sh" "$@"; }
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }

registry=$(artifact get registry)
nvr=$(artifact get nvr)
kernel=$(artifact get kernel_digest)
devel=$(artifact get devel_digest)
image=$registry/azoth-nvidia
mkdir -p "$OUT/sbom" "$OUT/digests"
echo "### NVIDIA modules for azoth@${kernel}" > "$OUT/summary.md"

for driver in open legacy; do
  tag=$(artifact get "nvidia_${driver}_tag")
  if artifact has "nvidia_${driver}_digest"; then
    echo "- \`${image}:${tag}\`: already published, signed and attested, not overwritten" | tee -a "$OUT/summary.md"
    continue
  fi
  version=$(artifact get "nvidia_${driver}_version")
  built=$(cat "$SIGNED/$driver/version")
  kver=$(cat "$SIGNED/$driver/kver")
  [[ $built == "$version" ]] || { echo "the ${driver} modules are ${built}, the pins ${version}" >&2; exit 1; }
  [[ $kver == "${nvr}.x86_64" ]] || { echo "the ${driver} modules were built for ${kver}, the kernel is ${nvr}.x86_64" >&2; exit 1; }

  ctr=$(buildah from scratch)
  buildah copy "$ctr" "$SIGNED/$driver/" /
  buildah config \
    --label org.opencontainers.image.title="azoth-nvidia ${driver}" \
    --label org.opencontainers.image.version="$tag" \
    --label org.opencontainers.image.revision="${GITHUB_SHA:?}" \
    --label org.opencontainers.image.source="${GITHUB_SERVER_URL:?}/${GITHUB_REPOSITORY:?}" \
    --label io.athanor.azoth.digest="$kernel" \
    "$ctr"
  buildah commit --omit-timestamp "$ctr" "${image}:${tag}"
  retry buildah push --digestfile "$OUT/digests/${driver}" "${image}:${tag}"
  ref="${image}@$(cat "$OUT/digests/${driver}")"

  pins=$(sed -n 's/^\(NVIDIA_[A-Z0-9_]*\)=\(.*\)$/\1\t\2/p' "$ROOT/forge/specs/azoth/pins.env" | jq -Rn '[inputs | split("\t") | {(.[0]): .[1]}] | add')
  jq -n --arg driver "$driver" --arg version "$version" --arg kver "$kver" --arg kernel "$kernel" --arg devel "$devel" --argjson pins "$pins" \
    '{driver: $driver, version: $version, kernel: $kver, kernel_digest: $kernel, devel_digest: $devel, pins: $pins}' > "$OUT/pins-${driver}.json"
  syft scan "registry:${ref}" -o "spdx-json=$OUT/sbom/${driver}.spdx.json"
  jq -e '[.packages[] | select(.name | startswith("nvidia"))] | length > 0' "$OUT/sbom/${driver}.spdx.json" > /dev/null \
    || { echo "the SBOM of ${driver} lacks the nvidia modules" >&2; exit 1; }
  bash "$ROOT/forge/scripts/sign_attest.sh" "$ref" "$OUT/sbom/${driver}.spdx.json"
  retry cosign attest --yes --type custom --predicate "$OUT/pins-${driver}.json" "$ref"
  echo "- \`${image}:${tag}\`: published as \`${ref}\`" | tee -a "$OUT/summary.md"
done
```

Run: `chmod 0755 forge/specs/azoth/nvidia-publish.sh && shellcheck forge/specs/azoth/nvidia-publish.sh && bash -n forge/specs/azoth/nvidia-publish.sh`

Expected: silent.

There is no offline test of this script. The `Verify (gate K4)` step runs `kernel-artifacts.sh require-ready` right after it in the same job, and fails unless both tags are signed by this workflow and attested for the pins and the kernel digest.

- [ ] **Step 2: Replace `.github/workflows/nvidia-kmod.yml`**

The `Retention (retention.sh)` step keeps calling `retention.sh` without arguments until Task 7.

```yaml
name: NVIDIA kmod

# The NVIDIA kernel modules, signed and published (forge/specs/azoth/nvidia.sh,
# docs/architecture/doc_kernel_build.md, section 10; docs/architecture/doc_build_ordering.md,
# O1, O2, O5, O6). artifacts: system/kernel-artifacts.sh resolves the kernel of the pins by
# digest and the module tags bound to it; unless the modules are missing (both branches
# already published, signed and attested, or the kernel of the pins not published) the run
# ends there with a notice, asking for no approval. build: nvidia-build.yml, the two branches
# (open, legacy) against azoth-devel by digest. sign: on a GitHub runner in the `signing`
# environment, a job that only sees the .ko files and the module signing key. boot: in QEMU,
# every case of the matrix: the signed nvidia.ko of each branch must pass the signature check
# against the certificate compiled into the kernel (ENODEV: no GPU), while an unsigned copy and
# a copy signed by an enrolled Secure Boot-profile MOK must be rejected (EKEYREJECTED).
# publish: nvidia-publish.sh, azoth-nvidia:<nvr>-k<12 hex of the kernel digest>-<branch>-<version>
# with SBOM, signature and attestation, never overwriting a published tag; then the retention
# and the verification. sign, boot and publish share the azoth-nvidia-publish concurrency
# group and are never cancelled, so no two runs interleave a push and its signature. The
# Orchestrator calls this workflow when the modules are missing; it also runs by hand. The
# gate on PRs is the kmod job of Kernel Build.

on:
  workflow_call:
    inputs:
      kernel_digest:
        description: "digest of azoth:<nvr> that the caller resolved; the run fails if the registry holds another"
        type: string
        required: true
  workflow_dispatch:
    inputs:
      kernel_digest:
        description: "digest of azoth:<nvr>; empty: the one system/kernel-artifacts.sh resolves"
        type: string
        default: ""

permissions:
  contents: read

jobs:
  artifacts:
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    outputs:
      state: ${{ steps.artifacts.outputs.state }}
      devel_digest: ${{ steps.artifacts.outputs.devel_digest }}
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262

      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6

      - name: Resolve the kernel artifacts (system/kernel-artifacts.sh)
        id: artifacts
        env:
          KERNEL_DIGEST: ${{ inputs.kernel_digest }}
        run: |
          set -euo pipefail
          bash system/kernel-artifacts.sh resolve --expect-kernel-digest "$KERNEL_DIGEST"
          grep -E '^(state|devel_digest)=' kernel-artifacts/kernel-artifacts.env >> "$GITHUB_OUTPUT"

      - name: Nothing to build
        if: ${{ steps.artifacts.outputs.state != 'modules-missing' }}
        env:
          STATE: ${{ steps.artifacts.outputs.state }}
        run: |
          echo "::notice title=NVIDIA kmod::state ${STATE}: the modules of the pins are published, or their kernel is not; no build and no approval"

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-kernel-artifacts
          path: kernel-artifacts/kernel-artifacts.env
          if-no-files-found: error

  build:
    needs: artifacts
    if: ${{ needs.artifacts.outputs.state == 'modules-missing' }}
    permissions:
      contents: read
      packages: read
    uses: ./.github/workflows/nvidia-build.yml
    with:
      devel-digest: ${{ needs.artifacts.outputs.devel_digest }}

  sign:
    needs: build
    runs-on: ubuntu-24.04
    timeout-minutes: 20
    concurrency:
      group: azoth-nvidia-publish
      cancel-in-progress: false
    permissions:
      contents: read
      packages: read
    environment: signing
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          pattern: nvidia-*-unsigned
          path: out
          merge-multiple: true

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: nvidia-kernel-artifacts
          path: kernel-artifacts

      - name: Build the nvidia image
        env:
          BUILDAH_ISOLATION: chroot
        run: podman build -t localhost/azoth-nvidia forge/specs/azoth/nvidia

      - name: Kernel-devel from the published image, by digest
        # Only for scripts/sign-file, which nvidia.sh extracts from the RPM.
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          set -euo pipefail
          registry=$(bash system/kernel-artifacts.sh get registry)
          devel=$(bash system/kernel-artifacts.sh get devel_digest)
          echo "${GH_TOKEN}" | podman login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
          trap 'podman logout ghcr.io' EXIT
          ctr=$(podman create "${registry}/azoth-devel@${devel}" /kernel-devel)
          mkdir -p devel && podman cp "${ctr}:/." devel/ && podman rm "$ctr" > /dev/null

      - name: Sign a copy with an enrolled-MOK-shaped key
        # The negative sample of the boot job: nvidia.ko of the open branch signed by an
        # ephemeral key with the Secure Boot profile, whose certificate boot.sh enrols in
        # MokList. The kernel must refuse it: a Secure Boot key never authorises a module.
        run: |
          set -euo pipefail
          mkdir -p mok
          cp -a out/open mok/open
          openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
            -config forge/specs/azoth/keys/profiles/secureboot.cnf -subj '/CN=Athanor OS K3 test MOK/' \
            -keyout "$RUNNER_TEMP/test-mok.key" -out mok/test-mok.pem 2> /dev/null
          podman run --rm \
            -v "$GITHUB_WORKSPACE:/forge" \
            -v "$RUNNER_TEMP/test-mok.key:/run/test-mok.key:ro" \
            -w /forge localhost/azoth-nvidia \
            bash forge/specs/azoth/nvidia.sh sign --key /run/test-mok.key \
              --cert mok/test-mok.pem --devel /forge/devel --out /forge/mok

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-mok-signed
          path: mok/
          if-no-files-found: error

      - name: Sign with the module signing key
        # The key lives in a 0600 file in the runner's tmp for the duration of the
        # command, mounted read-only in the container; the public certificate is in the
        # repository and compiled into the kernel (kernel-local).
        env:
          MODULE_SIGNING_KEY: ${{ secrets.MODULE_SIGNING_KEY }}
        run: |
          set -euo pipefail
          [[ -n ${MODULE_SIGNING_KEY:-} ]] || { echo "MODULE_SIGNING_KEY is not available to this job: check the signing environment" >&2; exit 1; }
          umask 077
          printf '%s\n' "$MODULE_SIGNING_KEY" > "$RUNNER_TEMP/module.key"
          trap 'rm -f "$RUNNER_TEMP/module.key"' EXIT
          podman run --rm \
            -v "$GITHUB_WORKSPACE:/forge" \
            -v "$RUNNER_TEMP/module.key:/run/module.key:ro" \
            -w /forge localhost/azoth-nvidia \
            bash forge/specs/azoth/nvidia.sh sign --key /run/module.key \
              --cert forge/specs/azoth/keys/modules/athanor-modules.pem --devel /forge/devel --out /forge/out

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-signed
          path: out/
          if-no-files-found: error

  boot:
    needs: sign
    runs-on: ubuntu-24.04
    timeout-minutes: 30
    concurrency:
      group: azoth-nvidia-publish
      cancel-in-progress: false
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: nvidia-signed
          path: signed

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        # The negative check: the same nvidia.ko before signing.
        with:
          name: nvidia-open-unsigned
          path: unsigned

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        # The negative check of key separation: nvidia.ko signed by an enrolled MOK.
        with:
          name: nvidia-mok-signed
          path: mok

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: nvidia-kernel-artifacts
          path: kernel-artifacts

      - name: Kernel-core from the published image, by digest
        run: |
          set -euo pipefail
          registry=$(bash system/kernel-artifacts.sh get registry)
          kernel=$(bash system/kernel-artifacts.sh get kernel_digest)
          nvr=$(bash system/kernel-artifacts.sh get nvr)
          ctr=$(podman create "${registry}/azoth@${kernel}" /kernel-core)
          mkdir -p out && podman cp "${ctr}:/." out/ && podman rm "$ctr" > /dev/null
          echo "nvr=${nvr}" >> "$GITHUB_ENV"

      - uses: ./.github/actions/kvm

      - name: Build the boot-matrix image
        env:
          BUILDAH_ISOLATION: chroot
        run: podman build -t localhost/azoth-boot forge/specs/azoth/boot

      - name: Run boot.sh with the signed modules
        # Every case: the trust in the module signing key is compiled into the kernel, so
        # SeaBIOS proves it holds without firmware, and the UEFI cases prove that a MOK
        # enrolled in MokList does not extend it.
        run: |
          set -euo pipefail
          mkdir -p boot-out
          ko="lib/modules/${nvr}.x86_64/extra/nvidia/nvidia.ko"
          podman run --rm --device /dev/kvm -v "$GITHUB_WORKSPACE:/forge" -w /forge localhost/azoth-boot \
            bash forge/specs/azoth/boot.sh --rpms /forge/out --out /forge/boot-out \
              --mok mok/test-mok.pem \
              --insmod "signed/open/${ko}:ENODEV" \
              --insmod "signed/legacy/${ko}:ENODEV" \
              --insmod "unsigned/open/${ko}:EKEYREJECTED" \
              --insmod "mok/open/${ko}:EKEYREJECTED"

      - name: Summary
        if: ${{ always() }}
        run: |
          set -euo pipefail
          if [[ -f boot-out/summary.md ]]; then cat boot-out/summary.md >> "$GITHUB_STEP_SUMMARY"; fi

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        if: ${{ always() && hashFiles('boot-out/**') != '' }}
        with:
          name: nvidia-boot-logs
          path: boot-out/
          if-no-files-found: error

  publish:
    needs: [sign, boot]
    runs-on: ubuntu-24.04
    timeout-minutes: 45
    concurrency:
      group: azoth-nvidia-publish
      cancel-in-progress: false
    permissions:
      contents: read
      packages: write
      id-token: write
      attestations: write
      actions: read
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: nvidia-signed
          path: out

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: nvidia-kernel-artifacts
          path: resolved

      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6

      - uses: anchore/sbom-action/download-syft@3ad7283483fc7af8ff2b4ea19663c2d5ca935e26

      - name: Resolve again inside the publication group
        # A run that published the same tags while this one waited for the group leaves them
        # in place: nvidia-publish.sh skips every branch the file lists with a digest (O2).
        run: |
          set -euo pipefail
          expected=$(KERNEL_ARTIFACTS_DIR=resolved bash system/kernel-artifacts.sh get kernel_digest)
          bash system/kernel-artifacts.sh resolve --expect-kernel-digest "$expected"

      - name: Publish (nvidia-publish.sh)
        env:
          BUILDAH_ISOLATION: chroot
          SYFT_REGISTRY_AUTH_AUTHORITY: ghcr.io
          SYFT_REGISTRY_AUTH_USERNAME: ${{ github.actor }}
          SYFT_REGISTRY_AUTH_PASSWORD: ${{ secrets.GITHUB_TOKEN }}
        run: |
          set -euo pipefail
          echo "${GH_TOKEN}" | buildah login -u "${GITHUB_ACTOR}" --password-stdin ghcr.io
          echo "${GH_TOKEN}" | cosign login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
          bash forge/specs/azoth/nvidia-publish.sh out
          cat nvidia-publish/summary.md >> "$GITHUB_STEP_SUMMARY"

      - name: Retention (retention.sh)
        run: bash forge/specs/azoth/retention.sh

      - name: Verify (gate K4)
        # The resolver verifies both tags again after the retention: signature by this
        # workflow, and an attestation naming the pins and the kernel digest.
        run: bash system/kernel-artifacts.sh require-ready

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: nvidia-attestations
          path: nvidia-publish/
          if-no-files-found: error
```

- [ ] **Step 3: Pull kernel-devel by digest in `.github/workflows/nvidia-build.yml`**

Replace the header comment's last sentence block:

```yaml
# built artifact, or of the published image when the kernel is reused) and NVIDIA kmod
# calls it for the signing and publication chain. Uploads the nvidia-<branch>-unsigned
# artifacts.
```

with:

```yaml
# built artifact, or of the published image when the kernel is reused) and NVIDIA kmod
# calls it for the signing and publication chain. A published kernel-devel is pulled by
# the digest system/kernel-artifacts.sh verified, never by tag
# (docs/architecture/doc_build_ordering.md, O5). Uploads the nvidia-<branch>-unsigned
# artifacts.
```

Replace the `devel-artifact` input with:

```yaml
      devel-artifact:
        description: "artifact holding kernel-devel-*.rpm; empty: the published azoth-devel image named by devel-digest"
        type: string
        default: ""
      devel-digest:
        description: "digest of the published azoth-devel:<nvr>, required when devel-artifact is empty"
        type: string
        default: ""
```

Replace the whole `Kernel-devel from the published image` step with:

```yaml
      - name: Kernel-devel from the published image, by digest
        # The caller resolved the digest with system/kernel-artifacts.sh. Login with the job
        # token: the package is public, but the podman of a self-hosted runner may hold stale
        # ghcr credentials in auth.json, and ghcr answers 403 to the anonymous token. Logout
        # at the end of the step, so none are left.
        if: ${{ inputs.devel-artifact == '' }}
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          DEVEL_DIGEST: ${{ inputs.devel-digest }}
        run: |
          set -euo pipefail
          [[ $DEVEL_DIGEST =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "devel-digest is required when devel-artifact is empty, got '${DEVEL_DIGEST}'" >&2; exit 1; }
          echo "${GH_TOKEN}" | podman login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
          trap 'podman logout ghcr.io' EXIT
          ctr=$(podman create "${KERNEL_IMAGE}-devel@${DEVEL_DIGEST}" /kernel-devel)
          mkdir -p devel && podman cp "${ctr}:/." devel/ && podman rm "$ctr" > /dev/null
```

- [ ] **Step 4: Give the Kernel Build kmod gate the reused devel digest**

In `.github/workflows/kernel-build.yml`, job `inputs`, replace:

```yaml
    outputs:
      nvr: ${{ steps.key.outputs.nvr }}
      reuse: ${{ steps.key.outputs.reuse }}
```

with:

```yaml
    outputs:
      nvr: ${{ steps.key.outputs.nvr }}
      reuse: ${{ steps.key.outputs.reuse }}
      devel_digest: ${{ steps.artifacts.outputs.devel_digest }}
```

After the `key` step (after its line `echo "reuse=${reuse}" >> "$GITHUB_OUTPUT"`), add:

```yaml

      - name: Digest of the reused kernel-devel (system/kernel-artifacts.sh)
        # The kmod gate builds against the published azoth-devel by digest, never by tag.
        id: artifacts
        if: ${{ steps.key.outputs.reuse == 'true' }}
        run: |
          set -euo pipefail
          bash system/kernel-artifacts.sh resolve
          sed -n 's/^devel_digest=/devel_digest=/p' kernel-artifacts/kernel-artifacts.env >> "$GITHUB_OUTPUT"
```

In job `kmod`, replace:

```yaml
    with:
      devel-artifact: ${{ needs.inputs.outputs.reuse != 'true' && 'kernel-devel' || '' }}
```

with:

```yaml
    with:
      devel-artifact: ${{ needs.inputs.outputs.reuse != 'true' && 'kernel-devel' || '' }}
      devel-digest: ${{ needs.inputs.outputs.devel_digest }}
```

- [ ] **Step 5: Validate**

Run: `actionlint .github/workflows/nvidia-kmod.yml .github/workflows/nvidia-build.yml .github/workflows/kernel-build.yml && python3 scripts/verify.py workflows`

Expected: actionlint silent; no new `verify.py` failure.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/azoth/nvidia-publish.sh .github/workflows/nvidia-kmod.yml .github/workflows/nvidia-build.yml .github/workflows/kernel-build.yml
git commit -m "feat(nvidia-kmod): publish module tags bound to the kernel digest from a reusable workflow" -m "NVIDIA kmod becomes callable, resolves the artifacts first and ends with a notice when nothing is missing, pulls azoth and azoth-devel by digest, and publishes azoth-nvidia:<nvr>-k<kernel digest>-<branch>-<version> without overwriting a signed and attested tag. Signing, boot and publication share the azoth-nvidia-publish group, never cancelled (doc_build_ordering.md, O1, O2, O5, O6)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

### Task 4: System images built from the verified digests

**Files:**
- Create: `system/tests/test_build_image.py`
- Modify: `system/build-image.sh`, `system/Containerfile`, `forge/scripts/fetch_repo_rpms.sh`, `forge/Justfile`, `.github/workflows/athanor-forge-orchestrator.yml`, `.github/workflows/call-system-image.yml`
- Replace: `.github/workflows/system-image-check.yml` (interim form)

**Interfaces:**
- Consumes: `resolve`, `cycle`, `require-ready`, `get` (Tasks 1–2); `nvidia-kmod.yml` with `kernel_digest` (Task 3).
- Produces:
  - `build-image.sh` reads `nvr`, `registry`, `kernel_digest`, and `nvidia_open_digest` or `nvidia_legacy_digest` for its GPU, from `$KERNEL_ARTIFACTS_DIR/kernel-artifacts.env`.
    - It passes `--build-arg AZOTH_NVR|KERNEL_REGISTRY|NVIDIA_OPEN_DIGEST|NVIDIA_LEGACY_DIGEST` and the labels `io.athanor.azoth.digest` and `io.athanor.azoth-nvidia.digest` (variants only). Task 7's retention reads the latter.
    - It exits 2 when the file was resolved for other pins.
  - Containerfile: `ARG KERNEL_REGISTRY=ghcr.io/hr-mes`, `ARG NVIDIA_OPEN_DIGEST`, `ARG NVIDIA_LEGACY_DIGEST`.
  - Orchestrator:
    - `workflow_dispatch.inputs.sha` (string, default `""`) and `force_image` (boolean, default `false`);
    - jobs `kernel-artifacts` (outputs `state`, `cycle`, `kernel_digest`), `nvidia-kmod`, and `kernel-artifacts-final`, which uploads artifact `kernel-artifacts` = `kernel-artifacts/kernel-artifacts.env`;
    - `call-system-image.yml` downloads that artifact into `kernel-artifacts/` in `build-repo` and `dag-system-image`, and loses its `has_changes` input.
  - `force_image`: `dag_orchestrator.py` sets `has_changes` only from dirty DAG nodes, and the kernel is external to the DAG (`external_pkgs = {"kernel", "kernel-forge"}`). The input is a `workflow_dispatch` boolean read only in the `system-image` job condition; neither script changes.
    - Today `has_changes` is true on every hosted run anyway, because `.cache/*.hash` is not persisted and `ATHANOR_REDIS_URL` is unset.
    - The input keeps the ordering correct once that cache exists.
- Interim:
  - the trigger paths and the Kernel Build dispatch are unchanged until Task 5; a kernel bump push still triggers the Orchestrator, whose `cycle` defers to Kernel Build, and the images wait for the daily schedule until Task 5;
  - System Image Check builds all three images until Task 6.

- [ ] **Step 1: Write the failing test**

`system/tests/test_build_image.py`:

```python
"""Unit tests of the kernel artifacts in system/build-image.sh, with a podman stub that records
its arguments (python3 -B -m unittest discover -s system/tests -v)."""

import os
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD = ROOT / "system" / "build-image.sh"
NVR = subprocess.run(["bash", str(ROOT / "forge/specs/azoth/nvr.sh")], capture_output=True, text=True, check=True).stdout.strip()
KERNEL = "sha256:" + "1" * 64
OPEN = "sha256:" + "3" * 64
LEGACY = "sha256:" + "4" * 64


class BuildImage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        (bin_dir / "podman").write_text(textwrap.dedent(f"""\
            #!/bin/bash
            printf '%s\\n' "$@" > {self.dir}/podman.args
            """))
        (bin_dir / "podman").chmod(0o755)
        self.artifacts = self.dir / "artifacts"
        self.artifacts.mkdir()
        # SECUREBOOT_SIGNING_KEY set: the stub never reads it, and no throwaway key is generated.
        self.env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}", KERNEL_ARTIFACTS_DIR=str(self.artifacts),
                        SECUREBOOT_SIGNING_KEY="unused by the stub")

    def tearDown(self):
        self.tmp.cleanup()

    def artifacts_file(self, **values):
        lines = {"state": "ready", "nvr": NVR, "registry": "ghcr.io/hr-mes", "kernel_digest": KERNEL,
                 "nvidia_open_digest": OPEN, "nvidia_legacy_digest": LEGACY, **values}
        (self.artifacts / "kernel-artifacts.env").write_text("".join(f"{k}={v}\n" for k, v in lines.items() if v is not None))

    def build(self, gpu):
        r = subprocess.run(["bash", str(BUILD), "--gpu", gpu, "--registry", "localhost", "--tag", "check"],
                           capture_output=True, text=True, env=self.env)
        args = (self.dir / "podman.args").read_text().splitlines() if (self.dir / "podman.args").exists() else []
        return r, args

    def test_nvidia_builds_from_the_open_module_digest(self):
        self.artifacts_file()
        r, args = self.build("nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        for expected in (f"AZOTH_NVR={NVR}", "KERNEL_REGISTRY=ghcr.io/hr-mes", f"NVIDIA_OPEN_DIGEST={OPEN}",
                         f"io.athanor.azoth.digest={KERNEL}", f"io.athanor.azoth-nvidia.digest={OPEN}"):
            self.assertIn(expected, args)
        self.assertNotIn(f"NVIDIA_LEGACY_DIGEST={LEGACY}", args)

    def test_default_image_builds_while_modules_are_missing(self):
        self.artifacts_file(state="modules-missing", nvidia_open_digest=None, nvidia_legacy_digest=None)
        r, args = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(any(a.startswith("NVIDIA_") for a in args))

    def test_variant_without_its_module_digest_is_refused(self):
        self.artifacts_file(state="modules-missing", nvidia_legacy_digest=None)
        r, args = self.build("nvidia-legacy")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("nvidia_legacy_digest", r.stderr)
        self.assertEqual(args, [])

    def test_missing_kernel_is_refused(self):
        self.artifacts_file(state="kernel-missing", kernel_digest=None, nvidia_open_digest=None, nvidia_legacy_digest=None)
        r, args = self.build("none")
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(args, [])

    def test_artifacts_of_other_pins_are_refused(self):
        self.artifacts_file(nvr="7.0.0-100.azoth.fc43")
        r, args = self.build("none")
        self.assertEqual(r.returncode, 2)
        self.assertIn("resolve again", r.stderr)
        self.assertEqual(args, [])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it to verify it fails**

Run: `python3 -B -m unittest discover -s system/tests -p 'test_build_image.py' -v`

Expected:
- `test_nvidia_builds_from_the_open_module_digest` FAILs (`'KERNEL_REGISTRY=ghcr.io/hr-mes' not found`);
- `test_variant_without_its_module_digest_is_refused`, `test_missing_kernel_is_refused` and `test_artifacts_of_other_pins_are_refused` FAIL (exit 0, the file is not read);
- `test_default_image_builds_while_modules_are_missing` passes.

- [ ] **Step 3: Read the digests in `system/build-image.sh`**

Replace the header lines:

```bash
# Builds one Athanor system image (docs/architecture/doc_system_image.md, S2, S8) from
# system/Containerfile, in CI and locally.
```

with:

```bash
# Builds one Athanor system image (docs/architecture/doc_system_image.md, S2, S8) from
# system/Containerfile, in CI and locally, from the kernel and NVIDIA module digests that
# system/kernel-artifacts.sh verified (docs/architecture/doc_build_ordering.md, O4): run its
# resolve (or require-ready) first. Every image carries the digests it was built from as labels.
```

Replace the line:

```bash
args=(--layers --pull=newer --format docker --build-arg "AZOTH_NVR=$(bash "$ROOT/forge/specs/azoth/nvr.sh")" --build-arg "GPU=$GPU")
```

with:

```bash
artifact() { bash "$ROOT/system/kernel-artifacts.sh" get "$1"; }
nvr=$(artifact nvr)
pinned=$(bash "$ROOT/forge/specs/azoth/nvr.sh")
[[ $nvr == "$pinned" ]] || { echo "${0##*/}: the kernel artifacts were resolved for ${nvr}, the pins give ${pinned}: run system/kernel-artifacts.sh resolve again" >&2; exit 2; }
registry=$(artifact registry)
kernel=$(artifact kernel_digest)
args=(--layers --pull=newer --format docker --build-arg "AZOTH_NVR=$nvr" --build-arg "GPU=$GPU"
  --build-arg "KERNEL_REGISTRY=$registry" --label "io.athanor.azoth.digest=$kernel")
case $GPU in
  nvidia) modules=$(artifact nvidia_open_digest); args+=(--build-arg "NVIDIA_OPEN_DIGEST=$modules" --label "io.athanor.azoth-nvidia.digest=$modules") ;;
  nvidia-legacy) modules=$(artifact nvidia_legacy_digest); args+=(--build-arg "NVIDIA_LEGACY_DIGEST=$modules" --label "io.athanor.azoth-nvidia.digest=$modules") ;;
esac
```

Every value is assigned on its own line: a failing `get` inside an array literal would not stop the script.

Run: `python3 -B -m unittest discover -s system/tests -v && shellcheck system/build-image.sh`

Expected: `Ran 35 tests` … `OK`; shellcheck silent.

- [ ] **Step 4: Copy the modules by digest in `system/Containerfile`**

Replace:

```dockerfile
# vendor packages; the final stage gates, assembles the UKI and lints. AZOTH_NVR is passed by
# system/build-image.sh from forge/specs/azoth/nvr.sh.
ARG AZOTH_NVR
ARG GPU=none

FROM ghcr.io/hr-mes/azoth-nvidia:${AZOTH_NVR}-open AS nvidia-modules-open
FROM ghcr.io/hr-mes/azoth-nvidia:${AZOTH_NVR}-legacy AS nvidia-modules-legacy
```

with:

```dockerfile
# vendor packages; the final stage gates, assembles the UKI and lints. system/build-image.sh
# passes AZOTH_NVR, the registry and the module digests from the file system/kernel-artifacts.sh
# wrote (docs/architecture/doc_build_ordering.md, O4, O9). A stage the target does not use is
# never pulled, so the default image builds without module digests.
ARG AZOTH_NVR
ARG GPU=none
ARG KERNEL_REGISTRY=ghcr.io/hr-mes
ARG NVIDIA_OPEN_DIGEST
ARG NVIDIA_LEGACY_DIGEST

FROM ${KERNEL_REGISTRY}/azoth-nvidia@${NVIDIA_OPEN_DIGEST} AS nvidia-modules-open
FROM ${KERNEL_REGISTRY}/azoth-nvidia@${NVIDIA_LEGACY_DIGEST} AS nvidia-modules-legacy
```

Run (sandbox disabled; proves the two properties on a small file, since `base-atomic:43` cannot be pulled locally):

```bash
mkdir -p $SCRATCH/cf && cat > $SCRATCH/cf/Containerfile <<'EOF'
ARG KERNEL_REGISTRY=ghcr.io/hr-mes
ARG NVIDIA_OPEN_DIGEST
ARG NVIDIA_LEGACY_DIGEST
ARG GPU=none
FROM ${KERNEL_REGISTRY}/azoth-nvidia@${NVIDIA_OPEN_DIGEST} AS mo
FROM ${KERNEL_REGISTRY}/azoth-nvidia@${NVIDIA_LEGACY_DIGEST} AS ml
FROM registry.fedoraproject.org/fedora:43 AS system
FROM system AS gpu-none
FROM system AS gpu-nvidia
COPY --from=mo /lib/modules/ /usr/lib/modules/
FROM system AS gpu-nvidia-legacy
COPY --from=ml /lib/modules/ /usr/lib/modules/
FROM gpu-${GPU} AS final
RUN find /usr/lib/modules -name nvidia.ko
EOF
podman build --format docker -f $SCRATCH/cf/Containerfile $SCRATCH/cf
podman build --format docker --build-arg GPU=nvidia --build-arg NVIDIA_OPEN_DIGEST=$(skopeo inspect --format '{{.Digest}}' docker://ghcr.io/hr-mes/azoth-nvidia:7.2.5-100.azoth.fc43-open) -f $SCRATCH/cf/Containerfile $SCRATCH/cf
```

Expected:
- the first build succeeds without pulling any `azoth-nvidia` image;
- the second prints `/usr/lib/modules/7.2.5-100.azoth.fc43.x86_64/extra/nvidia/nvidia.ko`.

Remove the two images afterwards with `podman image prune -f`.

- [ ] **Step 5: Tier 0 by digest in `forge/scripts/fetch_repo_rpms.sh` and `forge/Justfile`**

In `fetch_repo_rpms.sh`, replace:

```bash
# Per-tier package images. An entry without a tag means :latest. The kernel is
# published by kernel-build.yml under the NVR derived from the pins (:latest exists
# only for builds from main). The NVIDIA modules (azoth-nvidia:<nvr>-<branch>) are
# not RPMs: system/Containerfile copies them from their image.
KERNEL_NVR=$(bash "$(dirname "${BASH_SOURCE[0]}")/../specs/azoth/nvr.sh")
```

with:

```bash
# Per-tier package images. An entry without a tag means :latest. The kernel is the
# azoth image of the pins by the digest system/kernel-artifacts.sh verified, never by tag
# (docs/architecture/doc_build_ordering.md, O4): run its require-ready first. The NVIDIA
# modules are not RPMs: system/Containerfile copies them from their image by digest.
ARTIFACTS="$(dirname "${BASH_SOURCE[0]}")/../../system/kernel-artifacts.sh"
KERNEL_STATE=$(bash "$ARTIFACTS" get state)
[[ $KERNEL_STATE == ready ]] || { echo "[FATAL] kernel artifacts are ${KERNEL_STATE}, not ready: run system/kernel-artifacts.sh require-ready first" >&2; exit 1; }
KERNEL_DIGEST=$(bash "$ARTIFACTS" get kernel_digest)
```

Then replace `# kernel is here as azoth:<nvr> in tier 0;` with `# kernel is here as azoth@<digest> in tier 0;`, and replace:

```bash
TIER0_IMAGES=(
  "azoth:${KERNEL_NVR}"
)
```

with:

```bash
TIER0_IMAGES=(
  "azoth@${KERNEL_DIGEST}"
)
```

`pull_and_extract` already treats a reference containing `:` as tagged, so `ghcr.io/<owner>/azoth@sha256:…` is used as is. The tier directory becomes `repo-tier0/azoth@sha256:<hex>`; the previous `azoth:<nvr>` directory is pruned by the existing "no longer in the manifest" loop.

In `forge/Justfile`, recipe `fetch-repo-rpms`, replace `    bash scripts/fetch_repo_rpms.sh "{{ owner }}"` with:

```
    bash ../system/kernel-artifacts.sh require-ready
    bash scripts/fetch_repo_rpms.sh "{{ owner }}"
```

Run: `bash -n forge/scripts/fetch_repo_rpms.sh && just --justfile forge/Justfile --summary > /dev/null`

Expected: both silent, exit 0.

- [ ] **Step 6: The kernel jobs of the Orchestrator**

In `.github/workflows/athanor-forge-orchestrator.yml`:

Replace:

```yaml
  workflow_dispatch:
  schedule:
```

with:

```yaml
  workflow_dispatch:
    inputs:
      sha:
        description: "commit whose Kernel Build dispatched this run; empty for a manual run"
        type: string
        default: ""
      force_image:
        description: "build the system images even when no DAG node changed: the kernel is outside the DAG (Kernel Build sets it)"
        type: boolean
        default: false
  schedule:
```

Replace:

```yaml
permissions:
  contents: write
  packages: write
  pull-requests: write
  id-token: write

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

with:

```yaml
permissions:
  contents: write
  packages: write
  pull-requests: write
  id-token: write
  # NVIDIA kmod, called below, attests its modules and reads the run list for its
  # retention: a called workflow cannot exceed the permissions of its caller.
  attestations: write
  actions: read

# A newer run waits instead of cancelling one that may be pushing or signing images; GitHub
# keeps only the newest waiting run of the group (docs/architecture/doc_build_ordering.md, O6).
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: false
```

After the `build-builder` job, insert:

```yaml
  kernel-artifacts:
    # The kernel and module artifacts of the pins and whether this run owns their cycle
    # (docs/architecture/doc_build_ordering.md, O3, O4): system/kernel-artifacts.sh decides,
    # this job only routes on its answer.
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    permissions:
      contents: read
    outputs:
      state: ${{ steps.artifacts.outputs.state }}
      cycle: ${{ steps.artifacts.outputs.cycle }}
      kernel_digest: ${{ steps.artifacts.outputs.kernel_digest }}
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6
      - name: Resolve the kernel artifacts and the cycle (system/kernel-artifacts.sh)
        id: artifacts
        env:
          EVENT: ${{ github.event_name }}
          BEFORE: ${{ github.event.before }}
          AFTER: ${{ github.event.after }}
          SHA: ${{ inputs.sha }}
        run: |
          set -euo pipefail
          bash system/kernel-artifacts.sh resolve
          bash system/kernel-artifacts.sh cycle --event "$EVENT" --before "$BEFORE" --after "$AFTER" --sha "$SHA" --head "$GITHUB_SHA"
          grep -E '^(state|cycle|kernel_digest)=' kernel-artifacts/kernel-artifacts.env >> "$GITHUB_OUTPUT"

  nvidia-kmod:
    # Only when the modules of the pins are missing; its approval comes before the image's.
    needs: [kernel-artifacts]
    if: ${{ needs.kernel-artifacts.outputs.cycle == 'build' && needs.kernel-artifacts.outputs.state == 'modules-missing' }}
    permissions:
      contents: read
      packages: write
      id-token: write
      attestations: write
      actions: read
    uses: ./.github/workflows/nvidia-kmod.yml
    with:
      kernel_digest: ${{ needs.kernel-artifacts.outputs.kernel_digest }}

  kernel-artifacts-final:
    # The single source of the digests the images are built from: resolved again after
    # NVIDIA kmod and required to be ready. A job cannot run twice, hence a second job.
    needs: [kernel-artifacts, nvidia-kmod]
    if: ${{ !cancelled() && needs.kernel-artifacts.result == 'success' && needs.kernel-artifacts.outputs.cycle == 'build' && (needs.nvidia-kmod.result == 'success' || needs.nvidia-kmod.result == 'skipped') }}
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6
      - name: Require ready (system/kernel-artifacts.sh)
        run: bash system/kernel-artifacts.sh require-ready
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-artifacts
          path: kernel-artifacts/kernel-artifacts.env
          if-no-files-found: error
```

Replace the `system-image` job with:

```yaml
  system-image:
    # The images run when a DAG node changed, when the kernel artifacts were not ready at
    # the start (the kernel is outside the DAG), or when the dispatch forces them (O4).
    needs: [orchestrator-brain, dag-compile, build-builder, kernel-artifacts, kernel-artifacts-final]
    if: ${{ !cancelled() && needs.kernel-artifacts-final.result == 'success' && needs.build-builder.result == 'success' && (needs.dag-compile.result == 'success' || needs.dag-compile.result == 'skipped') && (needs.orchestrator-brain.outputs.has_changes == 'true' || needs.kernel-artifacts.outputs.state != 'ready' || inputs.force_image) }}
    uses: ./.github/workflows/call-system-image.yml
    with:
      builder_content_hash: ${{ needs.build-builder.outputs.content_hash }}
    secrets: inherit
```

- [ ] **Step 7: The verified file in `.github/workflows/call-system-image.yml`**

1. Delete the `has_changes` input (its five lines under `inputs:`), and the line `    if: ${{ inputs.has_changes }}` of `build-repo`. The only caller gates the job itself now.
2. In `build-repo`, between the checkout step and `❄️ Install Nix (Bedrock Cache)`, insert:

```yaml
      - name: Kernel artifacts verified by the Orchestrator
        # fetch_repo_rpms.sh pulls azoth by the digest in this file (doc_build_ordering.md, O4).
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: kernel-artifacts
          path: kernel-artifacts
```

3. In `dag-system-image`, between `📥 Checkout Repository` and `❄️ Install Nix (Bedrock Cache & Security Tools)`, insert:

```yaml
      - name: Kernel artifacts verified by the Orchestrator
        # build-image.sh builds from the kernel and module digests in this file (O4).
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: kernel-artifacts
          path: kernel-artifacts
```

4. In both `podman run` invocations, replace `"ghcr.io/hr-mes/athanor-builder:${BUILDER_CONTENT_HASH}"` with `"ghcr.io/${GITHUB_REPOSITORY_OWNER,,}/athanor-builder:${BUILDER_CONTENT_HASH}"`. Both steps already export `GITHUB_REPOSITORY_OWNER`.

The container mounts the workspace at `/workspace` and runs in `/workspace/forge`. `fetch_repo_rpms.sh` finds `../../system/kernel-artifacts.sh`, which reads `/workspace/kernel-artifacts/kernel-artifacts.env`, with bash, grep and sed only.

- [ ] **Step 8: System Image Check resolves on a GitHub runner (interim form)**

The self-hosted runner guest carries only podman, git, tar and gzip (`scripts/runner/build-image.sh`), so the resolver runs in its own job on `ubuntu-24.04`. Replace `.github/workflows/system-image-check.yml` with:

```yaml
name: System Image Check

# Pull requests that change the system images build them on the self-hosted runner, with the
# UKI signed by a throwaway key and without pushing, and report the package delta of the
# default image against the published athanor-system (docs/architecture/doc_system_image.md,
# S2, S8, section 4). The gates of system/nvidia/gate.sh run inside the builds, from the
# kernel and module digests system/kernel-artifacts.sh resolves on a GitHub runner, which has
# skopeo and cosign (docs/architecture/doc_build_ordering.md, O4); the build job reads its file.

on:
  pull_request:
    paths:
      - system/**
      - forge/config/packages.json
      # The check installs the athanor-base-config RPM published in tier0:latest, not the spec
      # of the pull request.
      - forge/specs/athanor-base-config/**
      - forge/specs/athanor-secure-boot/SOURCES/**
      - forge/specs/azoth/pins.env
      - forge/specs/azoth/nvr.sh
      - forge/scripts/retry.sh
      - .containerignore
      - .github/workflows/system-image-check.yml

permissions:
  contents: read
  packages: read

concurrency:
  group: system-image-check-${{ github.event.pull_request.number }}
  cancel-in-progress: true

jobs:
  kernel-artifacts:
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4

      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6

      - name: Kernel artifacts (system/kernel-artifacts.sh)
        run: bash system/kernel-artifacts.sh resolve

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-artifacts
          path: kernel-artifacts/kernel-artifacts.env
          if-no-files-found: error

  build:
    needs: kernel-artifacts
    # The self-hosted runner executes only code of this repository: a pull request from a
    # fork never reaches it.
    if: ${{ github.event.pull_request.head.repo.full_name == github.repository }}
    # A KVM guest created per job, whose podman storage lives on the cache disk that persists
    # across jobs (scripts/runner/runner.env): builds pull with --pull=newer, and the last
    # step removes the images of this job.
    runs-on: self-hosted
    timeout-minutes: 300
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: kernel-artifacts
          path: kernel-artifacts

      - name: Login to GHCR
        # system/Containerfile binds ghcr.io/<owner>/athanor-forge-tier{0..3}-repo:latest and
        # pulls the azoth-nvidia modules by digest: the build needs a registry session before it
        # starts, not only the delta step at the end.
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          GITHUB_ACTOR: ${{ github.actor }}
          TMPDIR: ${{ runner.temp }}
        run: |
          set -euo pipefail
          echo "${GITHUB_TOKEN}" | podman login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin

      - name: Build the three images (throwaway UKI key)
        # The runner guest's system disk is small, and podman stages downloads and build copies
        # in TMPDIR (default /var/tmp); TMPDIR points to the job's temp directory on the scratch disk.
        env:
          TMPDIR: ${{ runner.temp }}
        run: |
          set -euo pipefail
          for gpu in none nvidia nvidia-legacy; do
            bash system/build-image.sh --gpu "$gpu" --registry localhost --tag check
          done

      - name: Package delta of the default image
        env:
          OWNER: ${{ github.repository_owner }}
          TMPDIR: ${{ runner.temp }}
        # tee: the job summary cannot be read through the REST API, the log can (gh run view --log).
        run: |
          set -euo pipefail
          bash system/package-delta.sh "ghcr.io/${OWNER,,}/athanor-system:latest" localhost/athanor-system:check | tee -a "$GITHUB_STEP_SUMMARY"

      - name: Remove the images of this job
        if: always()
        env:
          TMPDIR: ${{ runner.temp }}
        run: |
          set -euo pipefail
          for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
            if podman image exists "localhost/$name:check"; then podman rmi "localhost/$name:check"; fi
          done
          podman image prune -f
```

- [ ] **Step 9: Validate**

Run: `actionlint .github/workflows/athanor-forge-orchestrator.yml .github/workflows/call-system-image.yml .github/workflows/system-image-check.yml && python3 scripts/verify.py workflows && python3 -B -m unittest discover -s system/tests && bash -n system/build-image.sh forge/scripts/fetch_repo_rpms.sh`

Expected: actionlint silent; no new `verify.py` failure; `Ran 35 tests` `OK`.

- [ ] **Step 10: Commit**

```bash
git add system/build-image.sh system/Containerfile system/tests/test_build_image.py forge/scripts/fetch_repo_rpms.sh forge/Justfile .github/workflows/athanor-forge-orchestrator.yml .github/workflows/call-system-image.yml .github/workflows/system-image-check.yml
git commit -m "feat(orchestrator): build the system images from the verified kernel and module digests" -m "The Orchestrator resolves the kernel artifacts, calls NVIDIA kmod when the modules are missing, requires ready in a second job and hands its file to build-repo, which pulls azoth by digest into tier 0, and to build-image.sh, which copies the modules by digest and labels each image with them. Image jobs also run when the artifacts were not ready or force_image is set; runs are no longer cancelled (doc_build_ordering.md, O4, O6, O9)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

### Task 5: Kernel Build hands the chain to the Orchestrator

**Files:**
- Modify: `.github/workflows/kernel-build.yml` (header comment, job `nvidia` becomes `orchestrator`), `.github/workflows/athanor-forge-orchestrator.yml` (push paths)

**Interfaces:**
- Consumes: Orchestrator inputs `sha` and `force_image` (Task 4); `resolve` (Task 1).
- Produces:
  - Kernel Build job `orchestrator` runs `gh workflow run athanor-forge-orchestrator.yml --ref <branch> -f sha=<GITHUB_SHA> -f force_image=true` when `publish` succeeded or `state != ready`;
  - Kernel Build no longer dispatches `nvidia-kmod.yml`;
  - the Orchestrator ignores pushes that touch only `forge/specs/azoth/**` and triggers on `nvidia-kmod.yml` and `nvidia-build.yml`.

- [ ] **Step 1: Dispatch the Orchestrator from Kernel Build**

In the header comment of `.github/workflows/kernel-build.yml`, replace:

```yaml
# boots but does not publish. nvidia: on pushes, downstream of publish, dispatches
# nvidia-kmod.yml (signing, boot with Secure Boot, publication of the modules). inputs:
```

with:

```yaml
# boots but does not publish. orchestrator: on pushes, downstream of publish, dispatches the
# Athanor Forge Orchestrator for this commit with force_image when a new kernel was published
# or system/kernel-artifacts.sh does not answer ready; the Orchestrator calls nvidia-kmod.yml
# (signing, boot with Secure Boot, publication of the modules) and builds the system images
# (docs/architecture/doc_build_ordering.md, O1). inputs:
```

Replace the whole `nvidia` job with:

```yaml
  orchestrator:
    # The kernel is published or reused: the Orchestrator owns the chain from here (modules,
    # then images). A dispatch, because workflow_run only fires from the default branch. A
    # push that published nothing and leaves the artifacts ready (a document under the kernel
    # directory) costs no image cycle.
    needs: [inputs, boot, kmod, publish]
    if: ${{ !cancelled() && github.event_name != 'pull_request' && needs.boot.result == 'success' && needs.kmod.result == 'success' && (needs.publish.result == 'success' || (needs.publish.result == 'skipped' && needs.inputs.outputs.reuse == 'true')) }}
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    permissions:
      contents: read
      actions: write
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262

      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6

      - name: Resolve the kernel artifacts (system/kernel-artifacts.sh)
        id: artifacts
        run: |
          set -euo pipefail
          bash system/kernel-artifacts.sh resolve
          grep -E '^state=' kernel-artifacts/kernel-artifacts.env >> "$GITHUB_OUTPUT"

      - name: Dispatch the Orchestrator
        if: ${{ needs.publish.result == 'success' || steps.artifacts.outputs.state != 'ready' }}
        run: gh workflow run athanor-forge-orchestrator.yml --repo "$GITHUB_REPOSITORY" --ref "$GITHUB_REF_NAME" -f sha="$GITHUB_SHA" -f force_image=true
```

- [ ] **Step 2: Move the kernel directory out of the Orchestrator's push paths**

In `.github/workflows/athanor-forge-orchestrator.yml`, replace:

```yaml
      - "!forge/test/**"
```

with:

```yaml
      - "!forge/test/**"
      # The kernel directory belongs to Kernel Build, which dispatches this workflow when a
      # kernel is published or its modules are missing (docs/architecture/doc_build_ordering.md,
      # O1). No DAG node hashes anything under it.
      - "!forge/specs/azoth/**"
```

and replace:

```yaml
      - ".github/workflows/call-*.yml"
  workflow_dispatch:
```

with:

```yaml
      - ".github/workflows/call-*.yml"
      # The module chain this workflow calls.
      - ".github/workflows/nvidia-kmod.yml"
      - ".github/workflows/nvidia-build.yml"
  workflow_dispatch:
```

- [ ] **Step 3: Validate**

Run: `actionlint .github/workflows/kernel-build.yml .github/workflows/athanor-forge-orchestrator.yml && python3 scripts/verify.py workflows && python3 -B -m unittest discover -s system/tests -p 'test_kernel_artifacts.py'`

Expected:
- actionlint silent; no new `verify.py` failure;
- `Ran 30 tests` `OK`, including `test_kernel_build_paths_match_the_workflow`, which must still hold because Kernel Build's paths did not change.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/kernel-build.yml .github/workflows/athanor-forge-orchestrator.yml
git commit -m "feat(kernel-build): dispatch the Orchestrator instead of NVIDIA kmod" -m "Kernel Build dispatches the Orchestrator for its commit with force_image when it published a kernel or the artifacts are not ready, and a push that touches only the kernel directory no longer starts the Orchestrator on its own (doc_build_ordering.md, O1)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

### Task 6: System Image Check follows the kernel artifacts

**Files:**
- Replace: `.github/workflows/system-image-check.yml`

**Interfaces:**
- Consumes: `check-plan` (Task 2): `check_gpus`, `check_delta` in the file.
- Produces: the job `kernel-artifacts` has output `check_delta`; the build job builds `get check_gpus` and runs the delta only when `check_delta == 'true'`.

- [ ] **Step 1: Replace the workflow**

```yaml
name: System Image Check

# Pull requests that change the system images build them on the self-hosted runner, with the
# UKI signed by a throwaway key and without pushing, and report the package delta of the
# default image against the published athanor-system (docs/architecture/doc_system_image.md,
# S2, S8, section 4). The gates of system/nvidia/gate.sh run inside the builds. Which images
# are built follows the kernel artifacts of the pull request's pins
# (docs/architecture/doc_build_ordering.md, O7): all three when they are ready; only the
# default image, with a warning, for a pure NVIDIA pin bump whose modules are not published
# yet; none, with a warning, for a pure kernel pin bump; any other pull request with missing
# artifacts fails. system/kernel-artifacts.sh decides on a GitHub runner, which has skopeo and
# cosign; the build job reads its file.

on:
  pull_request:
    paths:
      - system/**
      - forge/config/packages.json
      # The check installs the athanor-base-config RPM published in tier0:latest, not the spec
      # of the pull request.
      - forge/specs/athanor-base-config/**
      - forge/specs/athanor-secure-boot/SOURCES/**
      - forge/specs/azoth/pins.env
      - forge/specs/azoth/nvr.sh
      - forge/scripts/retry.sh
      - .containerignore
      - .github/workflows/system-image-check.yml

permissions:
  contents: read
  packages: read

concurrency:
  group: system-image-check-${{ github.event.pull_request.number }}
  cancel-in-progress: true

jobs:
  kernel-artifacts:
    runs-on: ubuntu-24.04
    timeout-minutes: 15
    outputs:
      check_delta: ${{ steps.plan.outputs.check_delta }}
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
        with:
          # The merge commit and its first parent, the base: check-plan reads the diff between them.
          fetch-depth: 2

      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6

      - name: Kernel artifacts and the check plan (system/kernel-artifacts.sh)
        id: plan
        run: |
          set -euo pipefail
          bash system/kernel-artifacts.sh resolve
          bash system/kernel-artifacts.sh check-plan --base HEAD^1 --head HEAD
          grep -E '^check_delta=' kernel-artifacts/kernel-artifacts.env >> "$GITHUB_OUTPUT"

      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: kernel-artifacts
          path: kernel-artifacts/kernel-artifacts.env
          if-no-files-found: error

  build:
    needs: kernel-artifacts
    # The self-hosted runner executes only code of this repository: a pull request from a
    # fork never reaches it.
    if: ${{ github.event.pull_request.head.repo.full_name == github.repository }}
    # A KVM guest created per job, whose podman storage lives on the cache disk that persists
    # across jobs (scripts/runner/runner.env): builds pull with --pull=newer, and the last
    # step removes the images of this job.
    runs-on: self-hosted
    timeout-minutes: 300
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4

      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: kernel-artifacts
          path: kernel-artifacts

      - name: Login to GHCR
        # system/Containerfile binds ghcr.io/<owner>/athanor-forge-tier{0..3}-repo:latest and
        # pulls the azoth-nvidia modules by digest: the build needs a registry session before it
        # starts, not only the delta step at the end.
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          GITHUB_ACTOR: ${{ github.actor }}
          TMPDIR: ${{ runner.temp }}
        run: |
          set -euo pipefail
          echo "${GITHUB_TOKEN}" | podman login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin

      - name: Build the planned images (throwaway UKI key)
        # The runner guest's system disk is small, and podman stages downloads and build copies
        # in TMPDIR (default /var/tmp); TMPDIR points to the job's temp directory on the scratch disk.
        env:
          TMPDIR: ${{ runner.temp }}
        run: |
          set -euo pipefail
          gpus=$(bash system/kernel-artifacts.sh get check_gpus)
          for gpu in $gpus; do
            bash system/build-image.sh --gpu "$gpu" --registry localhost --tag check
          done

      - name: Package delta of the default image
        if: ${{ needs.kernel-artifacts.outputs.check_delta == 'true' }}
        env:
          OWNER: ${{ github.repository_owner }}
          TMPDIR: ${{ runner.temp }}
        # tee: the job summary cannot be read through the REST API, the log can (gh run view --log).
        run: |
          set -euo pipefail
          bash system/package-delta.sh "ghcr.io/${OWNER,,}/athanor-system:latest" localhost/athanor-system:check | tee -a "$GITHUB_STEP_SUMMARY"

      - name: Remove the images of this job
        if: always()
        env:
          TMPDIR: ${{ runner.temp }}
        run: |
          set -euo pipefail
          for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
            if podman image exists "localhost/$name:check"; then podman rmi "localhost/$name:check"; fi
          done
          podman image prune -f
```

- [ ] **Step 2: Validate**

Run: `actionlint .github/workflows/system-image-check.yml && python3 scripts/verify.py workflows && python3 -B -m unittest discover -s system/tests`

Expected: actionlint silent; no new `verify.py` failure; `Ran 35 tests` `OK`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/system-image-check.yml
git commit -m "feat(system-image-check): build the images the kernel artifacts allow on a pull request" -m "All three images when the artifacts are ready; only the default image, with a warning, on a pure NVIDIA pin bump; no build, with a warning, on a pure kernel pin bump; a failure for any other pull request with missing artifacts (doc_build_ordering.md, O7)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

- [ ] **Step 4: Push and open PR A (with the maintainer's consent)**

Check `gh run list --workflow athanor-forge-orchestrator.yml --branch iso-v0 --status in_progress` is empty. Then:

```bash
git push -u origin build-ordering
gh pr create --base iso-v0 --head build-ordering --title "Build ordering: one Orchestrator chain from kernel to images, by verified digest" --body-file $SCRATCH/pr-a.md
```

`$SCRATCH/pr-a.md`, in English:
- the summary of Tasks 1–6;
- a "Merge" note: this merge is the O10 bootstrap. Kernel Build dispatches the Orchestrator, whose run calls NVIDIA kmod: two `signing` approvals follow the merge;
- an "Expected red check" note: System Image Check fails on this PR with `the NVIDIA modules of azoth:<nvr> are not published`. No O2 tag exists before the bootstrap, and O7 fails missing modules with unchanged pins. It is not a required check.

It ends with:

```
🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4
```

Stop here. The merge is the maintainer's; continue with Task 10 Part 1.

---

## PR B — retention, janitor and documentation (branch `build-ordering-retention`, from `iso-v0` after Task 10 Part 1)

### Task 7: One pruner per package set

**Files:**
- Replace: `forge/specs/azoth/retention.sh`
- Create: `system/tests/test_azoth_retention.py`
- Modify: `.github/workflows/kernel-build.yml` (`publish` retention step), `.github/workflows/nvidia-kmod.yml` (`publish` retention step)

**Interfaces:**
- Consumes:
  - `kernel-artifacts.sh digest` and `predicates … modules` (Task 1);
  - the label `io.athanor.azoth-nvidia.digest` (Task 4);
  - system images tagged with the Orchestrator run id (`call-system-image.yml`, `--tag "${RUN_ID}"`).
- Produces:
  - `retention.sh kernel|nvidia [--dry-run]`.
  - `kernel` prunes `azoth` and `azoth-devel` (every release) and `azoth-debuginfo` (two newest).
  - `nvidia` prunes `azoth-nvidia`, keeping:
    - releases whose verified module predicate has a `kernel_digest` among the retained `azoth` releases;
    - the label digest of the newest existing `athanor-system-nvidia:<run id>` and `athanor-system-nvidia-legacy:<run id>` per `headBranch` of `gh run list --workflow athanor-forge-orchestrator.yml --limit 100`.
  - A registry, Rekor or API error exits non-zero before any deletion.

- [ ] **Step 1: Write the failing tests**

`system/tests/test_azoth_retention.py`:

```python
"""Unit tests of forge/specs/azoth/retention.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib
import subprocess
import unittest

from test_kernel_artifacts import KMOD, REG, Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
RETENTION = ROOT / "forge" / "specs" / "azoth" / "retention.sh"


def digest(n):
    return "sha256:" + f"{n:x}" * 64


def version(id_, name, tags, created):
    return {"id": id_, "name": name, "created_at": f"2026-09-{created:02d}T00:00:00Z", "metadata": {"container": {"tags": tags}}}


KERNEL, OLD_KERNEL = digest(1), digest(2)
KEPT, STALE, REFERENCED, OLD_FORM, BUNDLE, SYSTEM = digest(3), digest(4), digest(5), digest(6), digest(7), digest(8)


def attested(kernel):
    return [{"identity": KMOD, "predicate": {"driver": "open", "kernel_digest": kernel}}]


class Retention(Tool):
    def setUp(self):
        super().setUp()
        self.fx = {
            "packages": {
                "azoth": [version(1, KERNEL, ["7.2.5-100.azoth.fc43"], 10), version(2, OLD_KERNEL, [], 1)],
                "azoth-nvidia": [
                    version(10, KEPT, ["7.2.5-100.azoth.fc43-k111111111111-open-610.57.04"], 11),
                    version(11, STALE, ["7.2.5-100.azoth.fc43-k222222222222-open-610.57.04"], 2),
                    version(12, REFERENCED, ["7.2.5-100.azoth.fc43-k222222222222-legacy-580.178.04"], 3),
                    version(13, OLD_FORM, ["7.2.5-100.azoth.fc43-open"], 4),
                    version(14, BUNDLE, [], 11),
                    version(15, digest(9), [f"sha256-{KEPT[7:]}"], 11),
                ],
            },
            "attestations": {
                f"{REG}/azoth-nvidia@{KEPT}": attested(KERNEL),
                f"{REG}/azoth-nvidia@{STALE}": attested(OLD_KERNEL),
                f"{REG}/azoth-nvidia@{REFERENCED}": attested(OLD_KERNEL),
                f"{REG}/azoth-nvidia@{OLD_FORM}": [{"identity": KMOD, "predicate": {"driver": "open"}}],
            },
            "raw": {f"{REG}/azoth-nvidia:sha256-{KEPT[7:]}": {"manifests": [{"digest": BUNDLE}]}},
            "runs": [{"databaseId": 900, "headBranch": "iso-v0"}, {"databaseId": 800, "headBranch": "main"}],
            "tags": {f"{REG}/athanor-system-nvidia-legacy:900": SYSTEM},
            "configs": {f"{REG}/athanor-system-nvidia-legacy@{SYSTEM}": {"io.athanor.azoth-nvidia.digest": REFERENCED}},
            "errors": [],
        }

    def retention(self, *args):
        self.registry(self.fx)
        return subprocess.run(["bash", str(RETENTION), *args], capture_output=True, text=True, env=self.env)

    def deletes(self):
        calls = [json.loads(line) for line in (self.dir / "calls.log").read_text().splitlines()]
        return sorted(c[-1].rsplit("/", 1)[1] for c in calls if c[0] == "gh" and "DELETE" in c)

    def test_nvidia_keeps_current_and_referenced_modules_with_their_bundles(self):
        r = self.retention("nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.deletes(), ["11", "13"])

    def test_dry_run_deletes_nothing(self):
        r = self.retention("nvidia", "--dry-run")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("[dry-run] azoth-nvidia: deleting 11", r.stdout)
        self.assertEqual(self.deletes(), [])

    def test_registry_error_deletes_nothing(self):
        self.fx["errors"].append(f"{REG}/azoth-nvidia:sha256-{KEPT[7:]}")
        r = self.retention("nvidia")
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.deletes(), [])

    def test_rekor_error_deletes_nothing(self):
        self.fx["errors"].append(f"{REG}/azoth-nvidia@{STALE}")
        r = self.retention("nvidia")
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.deletes(), [])

    def test_kernel_keeps_two_debuginfo_releases_and_leaves_modules_alone(self):
        self.fx["packages"]["azoth-debuginfo"] = [version(30 + n, digest(10 + n), [f"v{n}"], n + 1) for n in range(3)]
        r = self.retention("kernel")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.deletes(), ["2", "30"])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to verify they fail**

Run: `python3 -B -m unittest discover -s system/tests -p 'test_azoth_retention.py' -v`

Expected: `FAILED (failures=4)`, measured against the current script:
- `test_nvidia_keeps_current_and_referenced_modules_with_their_bundles` fails with `['2'] != ['11', '13']`: the current script ignores the mode and keeps every `azoth-nvidia` release;
- `test_dry_run_deletes_nothing` fails, because `--dry-run` is not its first argument;
- both error tests fail with `0 == 0`, because a failed `skopeo` inside `< <(…)` is ignored and no `cosign` is called;
- `test_kernel_keeps_two_debuginfo_releases_and_leaves_modules_alone` passes already and guards the refactor.

- [ ] **Step 3: Replace `forge/specs/azoth/retention.sh`**

```bash
#!/usr/bin/env bash
# Retention of the kernel OCI packages on ghcr (docs/architecture/doc_kernel_build.md,
# section 3, step 8; docs/architecture/doc_build_ordering.md, O6). A version (= one manifest)
# survives if it is reachable from a retained release; everything else is deleted.
#
# Release: a manifest carrying a tag that does not start with `sha256-` (the NVR, `latest`,
# the module tags). Reachable from a release with digest sha256:<hex>:
#   - the manifests tagged `sha256-<hex>*`: the referrers index that cosign v3 updates
#     under the fallback tag `sha256-<hex>` (ghcr has no referrers API: GET
#     /v2/<repo>/referrers/<digest> answers 404) and the legacy `.sig`, `.att`, `.sbom`
#     tags;
#   - the members of that index, i.e. the Sigstore bundles of the signature and the
#     attestations, which are untagged manifests: their list comes from the registry,
#     not from the packages API.
# Deleted, therefore: the releases not retained together with their referrers, the
# indexes superseded by every later `cosign sign|attest`, and the manifests left by a
# repeated push of the same tag. Each workflow gate verifies the signatures AFTER the
# retention: should this model stop holding (ghcr with a referrers API, cosign without
# the fallback tag) the run would fail.
#
# One pruner per package set, run by the workflow that publishes it:
#   kernel  Kernel Build: every release of azoth and azoth-devel, the two newest of
#           azoth-debuginfo
#   nvidia  NVIDIA kmod, inside the azoth-nvidia-publish concurrency group: every
#           azoth-nvidia release whose module attestation names the digest of a retained
#           azoth release, and the releases that the newest published athanor-system-nvidia
#           and athanor-system-nvidia-legacy of each branch were built from (their
#           io.athanor.azoth-nvidia.digest label)
#
# Usage: retention.sh kernel|nvidia [--dry-run]. Needs gh with read:packages and
# delete:packages (in CI the GITHUB_TOKEN with packages: write; nvidia also actions: read),
# skopeo authenticated to ghcr (buildah login), and for nvidia cosign.
set -euo pipefail
shopt -s inherit_errexit

usage() { echo "usage: retention.sh kernel|nvidia [--dry-run]" >&2; exit 2; }
MODE=${1:-}
[[ $MODE == kernel || $MODE == nvidia ]] || usage
DRY=''
case ${2:-} in
  '') ;;
  --dry-run) DRY=1 ;;
  *) usage ;;
esac
OWNER=${GITHUB_REPOSITORY_OWNER:-hr-mes}
REGISTRY=${KERNEL_REGISTRY:-ghcr.io/${OWNER,,}}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
ARTIFACTS=$ROOT/system/kernel-artifacts.sh

versions() { # versions PACKAGE: its versions as a JSON array, [] when never published
  local err
  # A package that has never been published (azoth-nvidia before its first NVIDIA build,
  # every package on the first run of a renamed project) has nothing to prune. Any other
  # API error stays fatal.
  if ! err=$(gh api "/users/${OWNER}/packages/container/$1" --silent 2>&1); then
    case $err in
      *"HTTP 404"*) echo "$1: not published yet, nothing to prune" >&2; echo '[]'; return 0 ;;
    esac
    printf '%s\n' "$err" >&2
    return 1
  fi
  gh api --paginate "/users/${OWNER}/packages/container/$1/versions?per_page=100" | jq -s 'add // []'
}

releases() { # releases VERSIONS [KEEP]: the digests of the KEEP newest releases, newest first
  jq -r --argjson keep "${2:-1000000}" '
    [.[] | select(.metadata.container.tags | any(startswith("sha256-") | not))]
    | sort_by(.created_at) | reverse | .[:$keep][].name' <<< "$1"
}

# Every command substitution below is assigned before it is read: bash ignores the exit
# status of one expanded inside a here-string, and a registry error must stop the pruner
# rather than leave a signature bundle unmarked.
nvidia_keep() { # nvidia_keep NVIDIA_VERSIONS KERNEL_RELEASES: the azoth-nvidia digests to keep
  local kernels modules digest predicates runs branches branch pkg ids id image config label
  kernels=$(jq -Rn '[inputs | select(length > 0)]' <<< "$2")
  modules=$(releases "$1")
  while IFS= read -r digest; do
    [[ -n $digest ]] || continue
    predicates=$(bash "$ARTIFACTS" predicates "$REGISTRY/azoth-nvidia@$digest" modules)
    [[ $predicates != unverified ]] || continue
    jq -sr --argjson kernels "$kernels" --arg digest "$digest" \
      'if any(.[]; .kernel_digest as $k | $kernels | index($k)) then $digest else empty end' <<< "$predicates"
  done <<< "$modules"
  # The system images are tagged with the id of the Orchestrator run that built them: the
  # newest run of a branch whose variant image exists is its last publication.
  runs=$(gh run list --repo "${GITHUB_REPOSITORY:-hr-mes/athanor}" --workflow athanor-forge-orchestrator.yml --limit 100 --json databaseId,headBranch)
  branches=$(jq -r '[.[].headBranch] | unique[]' <<< "$runs")
  while IFS= read -r branch; do
    [[ -n $branch ]] || continue
    ids=$(jq -r --arg branch "$branch" '.[] | select(.headBranch == $branch) | .databaseId' <<< "$runs")
    for pkg in athanor-system-nvidia athanor-system-nvidia-legacy; do
      while IFS= read -r id; do
        [[ -n $id ]] || continue
        image=$(bash "$ARTIFACTS" digest "$REGISTRY/$pkg:$id")
        [[ -n $image ]] || continue
        config=$(bash "$ROOT/forge/scripts/retry.sh" skopeo inspect --config "docker://$REGISTRY/$pkg@$image")
        label=$(jq -r '.config.Labels["io.athanor.azoth-nvidia.digest"] // empty' <<< "$config")
        [[ -z $label ]] || echo "$label"
        break
      done <<< "$ids"
    done
  done <<< "$branches"
}

prune() { # prune PACKAGE VERSIONS KEEP_DIGESTS
  local pkg=$1 versions=$2 api digest hex referrers index member id tags listing
  local -A live=()
  api="/users/${OWNER}/packages/container/${pkg}/versions"
  while IFS= read -r digest; do
    [[ -n $digest ]] || continue
    live[$digest]=1
    hex=${digest#sha256:}
    referrers=$(jq -r --arg hex "$hex" '.[] | select(.metadata.container.tags | any(startswith("sha256-" + $hex))) | .name' <<< "$versions")
    if jq -e --arg tag "sha256-${hex}" 'any(.[]; .metadata.container.tags | index($tag))' <<< "$versions" > /dev/null; then
      index=$(bash "$ROOT/forge/scripts/retry.sh" skopeo inspect --raw "docker://${REGISTRY}/${pkg}:sha256-${hex}")
      referrers+=$'\n'$(jq -r '.manifests[].digest' <<< "$index")
    fi
    while IFS= read -r member; do
      [[ -z $member ]] || live[$member]=1
    done <<< "$referrers"
  done <<< "$3"

  echo "${pkg}: ${#live[@]} manifests reachable from a retained release"
  listing=$(jq -r '.[] | "\(.id) \(.name) \(.metadata.container.tags | join(","))"' <<< "$versions")
  while read -r id digest tags; do
    [[ -n $id ]] || continue
    [[ -z ${live[$digest]:-} ]] || continue
    echo "${DRY:+[dry-run] }${pkg}: deleting ${id} ${digest} ${tags:-(untagged)}"
    [[ -n $DRY ]] || gh api --method DELETE "${api}/${id}" > /dev/null
  done <<< "$listing"
}

case $MODE in
  kernel)
    for pkg in azoth azoth-devel azoth-debuginfo; do
      json=$(versions "$pkg")
      keep=1000000
      [[ $pkg != azoth-debuginfo ]] || keep=2
      kept=$(releases "$json" "$keep")
      prune "$pkg" "$json" "$kept"
    done
    ;;
  nvidia)
    kernel_versions=$(versions azoth)
    kernels=$(releases "$kernel_versions")
    json=$(versions azoth-nvidia)
    kept=$(nvidia_keep "$json" "$kernels")
    prune azoth-nvidia "$json" "$kept"
    ;;
esac
```

Run: `python3 -B -m unittest discover -s system/tests -v && shellcheck forge/specs/azoth/retention.sh`

Expected: `Ran 40 tests` `OK`; shellcheck silent.

- [ ] **Step 4: Call each pruner from its own workflow**

- `.github/workflows/kernel-build.yml`, job `publish`, step `Retention (retention.sh)`: replace `run: bash forge/specs/azoth/retention.sh` with `run: bash forge/specs/azoth/retention.sh kernel`, and extend its comment with a new last line: `# azoth-nvidia is pruned by NVIDIA kmod alone (doc_build_ordering.md, O6).`
- `.github/workflows/nvidia-kmod.yml`, job `publish`, step `Retention (retention.sh)`: replace `run: bash forge/specs/azoth/retention.sh` with `run: bash forge/specs/azoth/retention.sh nvidia`.

Run: `actionlint .github/workflows/kernel-build.yml .github/workflows/nvidia-kmod.yml && python3 scripts/verify.py workflows`

Expected: silent; no new failure.

- [ ] **Step 5: Dry run against the registry (read-only, needs a token with `read:packages`)**

Run: `GH_TOKEN=<token with read:packages> bash forge/specs/azoth/retention.sh nvidia --dry-run`

Expected:
- `[dry-run] azoth-nvidia: deleting …` lines for the old `<nvr>-open|legacy` tags and their `sha256-<hex>` referrers;
- no line for a `-k<hex>-` tag whose kernel digest is the current `azoth:<nvr>`, nor for the digests labelled on the newest `athanor-system-nvidia*` images.

The local `gh` token of 2026-09-17 answers HTTP 403 to the packages API. Without a suitable token, skip this step: the first `publish` run shows the same lines, and its `Verify (gate K4)` fails if a kept tag lost its signature.

- [ ] **Step 6: Commit**

```bash
git add forge/specs/azoth/retention.sh system/tests/test_azoth_retention.py .github/workflows/kernel-build.yml .github/workflows/nvidia-kmod.yml
git commit -m "feat(retention): prune azoth-nvidia only from NVIDIA kmod, by kernel digest and image reference" -m "retention.sh kernel runs in Kernel Build for azoth, azoth-devel and azoth-debuginfo; retention.sh nvidia runs in the azoth-nvidia-publish group and keeps the module tags attested for a retained kernel and those the newest system images of each branch were built from. Every registry read is assigned before use, so an error stops the pruner before it deletes (doc_build_ordering.md, O6)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

### Task 8: The janitor leaves the kernel packages alone

**Done in PR A**, ahead of the schedule below: the final whole-branch review of PR A
(`.superpowers/sdd/2026-09-17-build-ordering/final-review-pr-a.md`, finding 2) found that
`forge-ghcr-cleanup.yml`, once repaired, would delete the untagged cosign bundles and the O2
module tags the new chain depends on, turning into a permanent `kernel-missing`. The controller
ruling brought this task forward into PR A rather than waiting for PR B, implemented as written
below against the branch's current `test_kernel_artifacts.Tool` and `fake_registry.py`. PR B
(Tasks 7, 9) is unaffected.

**Files:**
- Replace: `forge/scripts/clean_ghcr.sh`, `.github/workflows/forge-ghcr-cleanup.yml`
- Create: `system/tests/test_clean_ghcr.py`

**Interfaces:**
- Consumes: `test_kernel_artifacts.Tool`; the `user_packages` fixture key of `fake_registry.py` (Task 1).
- Produces:
  - `clean_ghcr.sh OWNER` skips every package matching `azoth*`;
  - for the others it keeps the two newest tagged versions and every version tagged `latest`, `main` or `stable`, and deletes the rest;
  - every API error fails;
  - the workflow calls it in one line.
  - Unchanged: `forge/Justfile` recipe `clean-ghcr` keeps calling `bash scripts/clean_ghcr.sh "{{ owner }}"`.

- [x] **Step 1: Write the failing test**

`system/tests/test_clean_ghcr.py`:

```python
"""Unit tests of forge/scripts/clean_ghcr.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib
import subprocess
import unittest

from test_kernel_artifacts import Tool

JANITOR = pathlib.Path(__file__).resolve().parents[2] / "forge" / "scripts" / "clean_ghcr.sh"


def version(id_, tags, day):
    return {"id": id_, "name": f"sha256:{id_:064x}", "created_at": f"2026-09-{day:02d}T00:00:00Z", "metadata": {"container": {"tags": tags}}}


class Janitor(Tool):
    def test_kernel_packages_are_never_touched(self):
        self.registry({"user_packages": ["azoth", "azoth-nvidia", "athanor-system"], "packages": {
            "azoth": [version(1, [], 1)],
            "azoth-nvidia": [version(2, [], 1)],
            "athanor-system": [version(3, ["101"], 1), version(4, ["102"], 2), version(5, ["103"], 3),
                               version(6, ["latest"], 0), version(7, [], 4)],
        }})
        r = subprocess.run(["bash", str(JANITOR), "hr-mes"], capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = [json.loads(line) for line in (self.dir / "calls.log").read_text().splitlines()]
        deleted = sorted(c[-1] for c in calls if "DELETE" in c)
        self.assertEqual(deleted, ["/users/hr-mes/packages/container/athanor-system/versions/3",
                                   "/users/hr-mes/packages/container/athanor-system/versions/7"])
        self.assertFalse(any("azoth" in " ".join(c) and "versions" in " ".join(c) for c in calls))


if __name__ == "__main__":
    unittest.main()
```

- [x] **Step 2: Run it to verify it fails**

Run: `python3 -B -m unittest discover -s system/tests -p 'test_clean_ghcr.py' -v`

Expected: FAIL with `Lists differ`. The current script also deletes `athanor-system` version 6 (tagged `latest`, but not among the two newest) and the untagged versions of `azoth` and `azoth-nvidia`.

- [x] **Step 3: Replace `forge/scripts/clean_ghcr.sh`** (mode 0755)

```bash
#!/usr/bin/env bash
# Janitor of the container packages on ghcr: for each package, the two newest tagged versions
# and every version tagged latest, main or stable stay; the other tagged versions and the
# untagged ones are deleted. The kernel packages (azoth*) are excluded: they are pruned only by
# forge/specs/azoth/retention.sh, which keeps the signature bundles and the module tags a build
# relies on (docs/architecture/doc_build_ordering.md, O6: one pruner per package).
# Usage: clean_ghcr.sh OWNER. Needs gh with read:packages and delete:packages.
set -euo pipefail
shopt -s inherit_errexit

OWNER=${1:?usage: clean_ghcr.sh OWNER}
packages=$(gh api --paginate "/users/${OWNER}/packages?package_type=container" | jq -rs 'add // [] | .[].name')
while IFS= read -r package; do
  [[ -n $package ]] || continue
  case $package in
    azoth*) echo "${package}: pruned by forge/specs/azoth/retention.sh, skipped"; continue ;;
  esac
  encoded=$(jq -rn --arg name "$package" '$name | @uri')
  api="/users/${OWNER}/packages/container/${encoded}/versions"
  versions=$(gh api --paginate "${api}?per_page=100" | jq -s 'add // []')
  doomed=$(jq -r '
    ([.[] | select(.metadata.container.tags | length > 0)] | sort_by(.created_at) | reverse | .[:2] | map(.id)) as $newest
    | .[]
    | select((.id as $id | $newest | index($id)) | not)
    | select(.metadata.container.tags | any(test("^(latest|main|stable)$")) | not)
    | .id' <<< "$versions")
  while IFS= read -r id; do
    [[ -n $id ]] || continue
    echo "${package}: deleting version ${id}"
    gh api --method DELETE "${api}/${id}" > /dev/null
  done <<< "$doomed"
done <<< "$packages"
```

- [x] **Step 4: Replace `.github/workflows/forge-ghcr-cleanup.yml`**

```yaml
name: 🧹 Forge GHCR Cleanup

# The weekly janitor of the container packages (forge/scripts/clean_ghcr.sh). The kernel
# packages azoth* are excluded: their only pruner is forge/specs/azoth/retention.sh
# (docs/architecture/doc_build_ordering.md, O6).

on:
  schedule:
    - cron: '0 0 * * 0'
  workflow_dispatch:

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

permissions:
  contents: read
  packages: write  # FORGE_PAT has delete:packages scope

jobs:
  cleanup-janitor:
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    container:
      image: ghcr.io/${{ github.repository_owner }}/athanor-builder:${{ vars.BUILDER_STABLE_TAG || 'latest' }}
    steps:
      - name: Checkout Repository
        uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4

      - name: Prune the container packages (clean_ghcr.sh)
        env:
          GH_TOKEN: ${{ secrets.FORGE_PAT }}
          OWNER: ${{ github.repository_owner }}
        run: bash forge/scripts/clean_ghcr.sh "$OWNER"
```

- [x] **Step 5: Validate**

Run: `python3 -B -m unittest discover -s system/tests -v && shellcheck forge/scripts/clean_ghcr.sh && actionlint .github/workflows/forge-ghcr-cleanup.yml && python3 scripts/verify.py workflows`

Expected: `Ran 41 tests` `OK`; the linters silent.

- [x] **Step 6: Commit**

Committed in PR A as `1a7f43dd` ("fix(ghcr-cleanup): exclude the kernel packages and move
the janitor into its script"), ahead of PR B, per the final-review-pr-a.md finding 2 ruling
above.

### Task 9: Documentation owed by the spec

**Files:**
- Modify: `docs/architecture/doc_kernel_build.md` (sections 7, 8, 10), `docs/architecture/doc_system_image.md` (S4, S5, S6, S8), `docs/architecture/doc_naming.md` (section 1 table), `forge/specs/azoth/KERNEL.md` (publication table)

**Interfaces:** none (prose). Italian stays Italian in `doc_kernel_build.md`, `doc_naming.md` and `KERNEL.md`.

- [ ] **Step 1: `doc_kernel_build.md`, section 7, gate 4**

Replace:

```
   580 compilano con `nvidia.sh` contro il `kernel-devel` appena costruito, o
   pubblicato per l'NVR dei pin quando il kernel è riusato, con la toolchain del
   kernel; ogni `.ko` deve portare il vermagic del kernel e i tipi kCFI. Poi, sui
   push, `nvidia-kmod.yml`, avviato da Kernel Build a valle della pubblicazione
   (`workflow_run` vale solo dal branch di default): il job `sign` li firma con la
```

with:

```
   580 compilano con `nvidia.sh` contro il `kernel-devel` appena costruito, o
   pubblicato, per digest, quando il kernel è riusato, con la toolchain del
   kernel; ogni `.ko` deve portare il vermagic del kernel e i tipi kCFI. Poi, sui
   push, `nvidia-kmod.yml`, che l'Athanor Forge Orchestrator chiama quando i moduli
   del kernel dei pin mancano (Kernel Build lo avvia a valle della pubblicazione,
   `docs/architecture/doc_build_ordering.md`): il job `sign` li firma con la
```

and replace:

```
   non dipende dal firmware, UEFI che una MOK arruolata non la estende; `publish`
   verifica firma e attestazione con cosign;
```

with:

```
   non dipende dal firmware, UEFI che una MOK arruolata non la estende; `publish`
   non sovrascrive un tag già firmato e attestato, e verifica firma e attestazione
   con `system/kernel-artifacts.sh require-ready`;
```

- [ ] **Step 2: `doc_kernel_build.md`, section 8**

Replace:

```
Kernel Build, che pubblica il kernel e alla fine avvia `nvidia-kmod.yml` per
firma, boot e pubblicazione dei moduli. Il cambio di release Fedora della rootfs
```

with:

```
Kernel Build, che pubblica il kernel e avvia l'Orchestrator: questo chiama
`nvidia-kmod.yml` per firma, boot e pubblicazione dei moduli, poi costruisce le
immagini di sistema (`doc_build_ordering.md`). Il cambio di release Fedora della rootfs
```

- [ ] **Step 3: `doc_kernel_build.md`, section 10**

Replace:

```
toglierli. NVIDIA, in un workflow proprio (`nvidia-kmod.yml`) che parte dopo il
kernel:
```

with:

```
toglierli. NVIDIA, in un workflow riusabile (`nvidia-kmod.yml`) che l'Orchestrator
chiama dopo il kernel, quando i moduli dei pin mancano (`doc_build_ordering.md`, O1):
```

Replace:

```
Pubblicazione `azoth-nvidia:<kernel-nvr>-<driver>`; le immagini
`athanor-system-nvidia` e `athanor-system-nvidia-legacy` le consumano insieme al firmware e
```

with:

```
Pubblicazione `azoth-nvidia:<nvr>-k<primi 12 hex del digest di azoth:<nvr>>-<driver>-<versione>`:
un kernel ripubblicato con lo stesso NVR riceve tag nuovi, e un tag firmato e attestato
non si sovrascrive mai; l'attestazione registra i digest di `azoth` e `azoth-devel`
usati. Le immagini `athanor-system-nvidia` e `athanor-system-nvidia-legacy` le
consumano per digest, come li verifica `system/kernel-artifacts.sh`, insieme al firmware e
```

Replace:

```
`modinfo`. Il workflow `nvidia-kmod.yml`: `build` (matrice dei due rami, runner
self-hosted, kernel-devel dall'immagine pubblicata per l'NVR di `nvr.sh`),
```

with:

```
`modinfo`. Il workflow `nvidia-kmod.yml`: `artifacts` (`system/kernel-artifacts.sh`;
con i moduli già pubblicati, o senza il kernel dei pin, il run finisce con un avviso e
senza approvazione), `build` (matrice dei due rami, runner self-hosted, kernel-devel
dall'immagine pubblicata, per digest),
```

Replace:

```
cosign, SBOM dei moduli,
attestazione dei pin `NVIDIA_*`, retention, gate di verifica). Le patch
```

with:

```
`nvidia-publish.sh`: cosign, SBOM dei moduli, attestazione dei pin `NVIDIA_*` e dei
digest del kernel; retention di `azoth-nvidia`, di cui NVIDIA kmod è l'unico potatore;
gate di verifica). `sign`, `boot` e `publish` condividono il gruppo di concorrenza
`azoth-nvidia-publish`, mai cancellato. Le patch
```

- [ ] **Step 4: `doc_system_image.md`**

- Line 9: replace `` (`system/Containerfile` copies `azoth-nvidia:<nvr>-open`) `` with `` (`system/Containerfile` copies the open `azoth-nvidia` tag of the kernel digest, by digest) ``.
- S4: replace ``- **The signed open modules** from `azoth-nvidia:<kernel-nvr>-open`, as today.`` with ``- **The signed open modules** from the open `azoth-nvidia` tag of the kernel digest (`doc_build_ordering.md`, O2), copied by digest.``
- S5: replace ``- the signed legacy modules come from `azoth-nvidia:<kernel-nvr>-legacy`;`` with ``- the signed legacy modules come from the legacy `azoth-nvidia` tag of the kernel digest, copied by digest;``
- S6: after the line `- the pin in `forge/specs/azoth/pins.env`.`, add ``- the modules themselves are the ones `system/kernel-artifacts.sh` verified: `system/build-image.sh` copies them by the digest in its file, and labels the image with it (`io.athanor.azoth-nvidia.digest`).``
- S8, under **Build and publication:**, replace ``  - `call-system-image.yml` builds the default image, then the two variants from the shared stages;`` with:

  ```
    - `call-system-image.yml` builds the default image, then the two variants from the shared stages, from the kernel and module digests of the file `system/kernel-artifacts.sh` wrote in the Orchestrator's `kernel-artifacts-final` job (`doc_build_ordering.md`, O4); tier 0 pulls `azoth` by the same digest;
  ```

- [ ] **Step 5: `doc_naming.md` and `KERNEL.md`**

- `doc_naming.md` line 19: replace `` `azoth[-devel\|-debuginfo]`, `azoth-nvidia` |`` with `` `azoth[-devel\|-debuginfo]`, `azoth-nvidia` (tag `<nvr>-k<digest>-open\|legacy-<versione>`) |``, and re-pad that table's separator and header rows so the column stays aligned (the table is padded to its widest cell).
- `forge/specs/azoth/KERNEL.md` line 157: replace ``tag `<nvr>-open` e `<nvr>-legacy` (workflow `nvidia-kmod.yml`)`` with ``tag `<nvr>-k<12 hex del digest del kernel>-open-<versione>` e `-legacy-<versione>` (workflow `nvidia-kmod.yml`, chiamato dall'Orchestrator)``.

- [ ] **Step 6: Validate and commit**

Run: `python3 scripts/verify.py docs && grep -rn 'azoth-nvidia:<nvr>-open\|azoth-nvidia:<kernel-nvr>' docs/architecture/doc_kernel_build.md docs/architecture/doc_system_image.md forge/specs/azoth/KERNEL.md`

Expected: `verify.py docs` reports no new failure; grep prints nothing. `doc_build_ordering.md` itself keeps the old tag form in its context section, where it describes the past.

```bash
git add docs/architecture/doc_kernel_build.md docs/architecture/doc_system_image.md docs/architecture/doc_naming.md forge/specs/azoth/KERNEL.md
git commit -m "docs(architecture): amend the kernel, system image and naming documents for the build ordering" -m "NVIDIA kmod is a reusable workflow called by the Orchestrator, module tags carry the kernel digest, images copy the modules by verified digest, and NVIDIA kmod is the only pruner of azoth-nvidia (doc_build_ordering.md, section 5)." -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EUnNXqv8jNWDVMA83G7eZ4"
```

- [ ] **Step 7: Push and open PR B (with the maintainer's consent)**

Check that no Orchestrator run is in progress, then push `build-ordering-retention` and open the PR into `iso-v0`.

The body, in English, states:
- the `nvidia` pruner deletes the pre-O2 `<nvr>-open|legacy` tags on its first run; images built before keep working because they copied their modules at build time;
- the merge touches `kernel-build.yml`, `retention.sh` and `nvidia-kmod.yml`. Kernel Build reuses the kernel and dispatches nothing (artifacts ready). The Orchestrator push run builds the images with `nvidia-kmod` skipped: one approval.

It ends with the Claude Code footer of Task 6 Step 4. Stop for the merge, then Task 10 Part 2.

---

## Task 10: Bootstrap (O10) and acceptance (spec section 6)

**Files:** none. Merges and approvals are the maintainer's; every step here observes GitHub and the registry.

Useful commands:
- `gh run list --workflow <file> --branch iso-v0 --limit 5 --json databaseId,event,status,conclusion,headSha,createdAt`
- `gh run view <id> --json jobs --jq '.jobs[] | [.name, .status, .conclusion] | @tsv'`
- `gh run view <id> --log | grep -E 'kernel-artifacts:|##\[notice\]|##\[warning\]'`

### Part 1: after PR A is merged

- [ ] **Step 1: Kernel Build dispatches the Orchestrator.** On the merge commit, the Kernel Build run (`event=push`) shows:
  - `inputs` success, with `kernel … image published from identical inputs, no build` in its summary;
  - `build` and `publish` skipped;
  - `boot`, `kmod` and `Kernel gate` success;
  - `orchestrator` success, with `kernel-artifacts: state=modules-missing` and the `Dispatch the Orchestrator` step executed.
- [ ] **Step 2: The push run of the Orchestrator defers.** The Orchestrator run with `event=push` on the same `headSha` shows:
  - `kernel-artifacts` success, with `kernel-artifacts: state=modules-missing`, then `cycle=defer` and the notice `… this push starts Kernel Build …`;
  - `nvidia-kmod`, `kernel-artifacts-final` and `system-image` skipped;
  - conclusion `success`.
- [ ] **Step 3: The dispatched run builds everything (acceptance 2, bootstrap half).** The Orchestrator run with `event=workflow_dispatch` waits for the push run in its concurrency group, then shows:
  - `kernel-artifacts` with `state=modules-missing` and `cycle=build`;
  - `nvidia-kmod / artifacts`, `build`, `sign` (first `signing` approval), `boot` and `publish` success. The publish summary lists both tags as `published as …`;
  - `kernel-artifacts-final` success;
  - `system-image / build-repo` and `dag-system-image` (second approval) success, with the ISO published.
  - No run of the three workflows is red, and nobody reran anything.

  Then verify locally (sandbox disabled):

  ```bash
  KERNEL_ARTIFACTS_DIR=$SCRATCH/ka bash system/kernel-artifacts.sh require-ready && cat $SCRATCH/ka/kernel-artifacts.env
  skopeo list-tags docker://ghcr.io/hr-mes/azoth-nvidia | jq -r '.Tags[]' | grep -- '-k'
  for n in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do skopeo inspect --config docker://ghcr.io/hr-mes/$n:<dispatched run id> | jq -c --arg n $n '{($n): (.config.Labels | with_entries(select(.key | startswith("io.athanor.azoth"))))}'; done
  ```

  Expected:
  - `require-ready` exits 0;
  - two `-k<12 hex>-` tags exist;
  - every image's `io.athanor.azoth.digest` equals `kernel_digest`;
  - `athanor-system-nvidia` carries `io.athanor.azoth-nvidia.digest` = `nvidia_open_digest`, and `-legacy` carries `nvidia_legacy_digest`.
- [ ] **Step 4: Acceptance 6 (only during Step 3).** While the dispatched run's `nvidia-kmod / publish` or `boot` is in progress, run `gh workflow run nvidia-kmod.yml --ref iso-v0`.
  - Its `artifacts` job answers `modules-missing` (not yet published), so it builds.
  - Its `sign` job stays queued until the Orchestrator's `publish` ends, and asks for its approval only then.
  - After approval, its `publish` log shows both tags `already published, signed and attested, not overwritten`, and `Verify (gate K4)` passes.
  - Neither run leaves a tag that `cosign verify` rejects. Check it with `bash system/kernel-artifacts.sh require-ready` (exit 0), and confirm the digests are those of Step 3.
  - If the maintainer prefers not to spend the approval, reject it: the manual run goes red at `sign`, the Orchestrator run is unaffected, and acceptance 6 waits for the next pin bump.
- [ ] **Step 5: Acceptance 3.** After Step 3 (and 4), record `skopeo inspect --format '{{.Digest}}'` of both module tags, then run `gh workflow run nvidia-kmod.yml --ref iso-v0`.
  - Only `artifacts` runs, with the notice `state ready: … no build and no approval`; the others are skipped.
  - No approval is requested, and both digests are unchanged.
- [ ] **Step 6: Acceptance 5.** In the `lint` job of any run after the merge, the step `Build ordering scripts (unit tests against an offline registry)` shows `Ran 35 tests` … `OK`.
- [ ] **Step 7: Acceptance 1.** On the next push to `system/**` with unchanged pins, one Orchestrator run:
  - `kernel-artifacts` shows `state=ready cycle=build`;
  - `nvidia-kmod` is skipped;
  - `system-image` succeeds with one approval;
  - no Kernel Build run exists for that push.

Then start PR B (Tasks 7–9).

### Part 2: after PR B is merged

- [ ] **Step 8: One pruner.**
  - The Orchestrator push run of the merge builds the images with `nvidia-kmod` skipped.
  - The Kernel Build run, if the merge touched its paths, shows `retention.sh kernel` output without any `azoth-nvidia` line, and `orchestrator` without a dispatch (state `ready`, nothing published).
  - The next `forge-ghcr-cleanup.yml` run (Sunday or `gh workflow run forge-ghcr-cleanup.yml`) logs `azoth: pruned by forge/specs/azoth/retention.sh, skipped` for each `azoth*` package. It may still fail on `FORGE_PAT`, which is outside this plan.
- [ ] **Step 9: Acceptance 2, kernel pin bump half.** On the next merged kernel bump PR:
  - its System Image Check was green with the warning `azoth:<nvr> is not published yet …` and no build;
  - Kernel Build builds, publishes and dispatches;
  - the Orchestrator push run does not exist (only `forge/specs/azoth/**` changed);
  - the dispatched run calls NVIDIA kmod once, whose `publish` log shows `retention.sh nvidia` deleting the pre-O2 tags on its first run, and publishes the three images;
  - no run is red.
- [ ] **Step 10: Acceptance 4.** When a kernel is republished with the same NVR (a bump that moves `CACHYOS_*` or the builder base without moving `FEDORA_KERNEL_NVR`), right after that cycle's NVIDIA kmod publication:
  - `skopeo list-tags` shows new `-k<new 12 hex>-` tags;
  - the previous `-k<old 12 hex>-` tags still resolve to their earlier digests, kept because the last published system images reference them.

  A later publication may delete them once no retained kernel or newest image references them (O6).
- [ ] **Step 11: Record.** Update the project memory with the run ids of Steps 1–3 and the outcome of each acceptance test.

---

## Self-Review

1. **Spec coverage:**
   - O1 → Task 3 (kmod `workflow_call`), Task 4 (Orchestrator permissions, kmod call without `secrets: inherit`), Task 5 (triggers, Kernel Build dispatch with `sha` and `force_image`);
   - O2 → Task 1 (tag form, predicate check), Task 3 (`nvidia-publish.sh`: predicate digests, no overwrite);
   - O3 → Tasks 1–2; the two-line steps in Tasks 3–6;
   - O4 → Task 2 (`cycle`), Task 4 (jobs, `force_image`, tier 0 and build arguments by digest);
   - O5 → Task 3 (start check, `--expect-kernel-digest`, three pulls by digest);
   - O6 → Task 3 (no workflow group, `azoth-nvidia-publish`), Task 4 (`cancel-in-progress: false`), Task 7 (pruners), Task 8 (janitor);
   - O7 → Task 2 (`check-plan`), Task 6;
   - O8 → no change needed (recorded in Global Constraints);
   - O9 → Task 4 (`KERNEL_REGISTRY` ARG and script variable), scripts everywhere;
   - O10 and section 6 → Task 10;
   - section 5 → Task 9.
2. **Placeholder scan:**
   - `<token with read:packages>` in Task 7 Step 5 and `<dispatched run id>` in Task 10 Step 3 are values known only at execution.
   - No step defers a design choice.
3. **Type consistency:**
   - file keys `state nvr registry kernel_digest devel_digest nvidia_{open,legacy}_{version,tag,digest} cycle check_gpus check_delta` are used with these exact names in `build-image.sh`, `fetch_repo_rpms.sh`, `nvidia-publish.sh`, `retention.sh` and every workflow;
   - artifact names: `kernel-artifacts` (Orchestrator, System Image Check) and `nvidia-kernel-artifacts` (kmod). They are distinct because kmod runs inside the Orchestrator run;
   - identities `kernel|modules`, concurrency group `azoth-nvidia-publish`, inputs `kernel_digest`, `devel-digest`, `sha`, `force_image`, labels `io.athanor.azoth.digest` and `io.athanor.azoth-nvidia.digest` match across tasks.
4. **Test counts:** 13 (Task 1), 30 (Task 2), 35 (Task 4), 40 (Task 7), 41 (Task 8), each measured on the prototypes while writing this plan.
5. **Known limits kept out of scope:**
   - `forge-ghcr-cleanup.yml` still deletes the cosign bundles of the non-kernel packages (spec O6 names only `azoth*`);
   - `FORGE_PAT` failures and the two out-of-scope items of spec section 4 are untouched.

## Appendix: final `system/kernel-artifacts.sh`

The script after Task 2, as validated:

```bash
#!/usr/bin/env bash
# The kernel and NVIDIA module artifacts of the current pins, verified in the registry and
# identified by digest (docs/architecture/doc_build_ordering.md, O2-O5, O7). Every workflow of
# the build ordering runs this script and reads its file; none repeats its decisions.
#
#   resolve [--expect-kernel-digest D]  write kernel-artifacts.env: state=ready, modules-missing
#                                       or kernel-missing, then the verified digests. Exit 0 for
#                                       the three states; 1 on a registry, Rekor, network or data
#                                       error, or when azoth:<nvr> is not D (republished since)
#   require-ready                       resolve, then exit 1 unless state=ready
#   cycle --event E [--before B] [--after A] [--sha S] [--head H]
#                                       after resolve, for an Orchestrator run (O4): append
#                                       cycle=build or cycle=defer; exit 1 when the kernel is
#                                       missing and no other cycle owns it
#   check-plan --base REV --head REV    after resolve, for System Image Check (O7): append
#                                       check_gpus and check_delta; exit 1 for a failing row
#   get KEY                             print the value of KEY; exit 1 when the file lacks it
#   has KEY                             exit 0 when KEY has a non-empty value
#   digest REF                          the digest of REF, empty when the tag does not exist
#   signed REF kernel|modules           signed or unsigned, by the workflow that publishes it
#   predicates REF modules              the custom predicates of REF, one JSON per line, or
#                                       unverified
#   probe digest|signed|predicates ...  one attempt of the three above (they retry it)
#
# The file is $KERNEL_ARTIFACTS_DIR/kernel-artifacts.env (default: kernel-artifacts/ at the
# repository root). KERNEL_REGISTRY is the registry and owner (default ghcr.io/ followed by
# GITHUB_REPOSITORY_OWNER, else hr-mes); GITHUB_SERVER_URL and GITHUB_REPOSITORY name the
# workflows whose signatures are trusted. cycle and check-plan run git in the repository
# checkout that is the current directory. Needs skopeo, cosign and jq for the registry.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SELF=$HERE/$(basename "${BASH_SOURCE[0]}")
ROOT=$(dirname "$HERE")
DIR=${KERNEL_ARTIFACTS_DIR:-$ROOT/kernel-artifacts}
FILE=$DIR/kernel-artifacts.env
PINS=$ROOT/forge/specs/azoth/pins.env
owner=${GITHUB_REPOSITORY_OWNER:-hr-mes}
REGISTRY=${KERNEL_REGISTRY:-ghcr.io/${owner,,}}
ISSUER=https://token.actions.githubusercontent.com
workflows="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-hr-mes/athanor}/.github/workflows"
# Owner, repository and host names hold no regex metacharacter other than the dot.
workflows=${workflows//./\\.}
declare -A IDENTITY=(
  [kernel]="^${workflows}/kernel-build\.yml@refs/heads/"
  [modules]="^${workflows}/nvidia-kmod\.yml@refs/heads/"
)
# cosign v3 reports a missing or foreign signature or attestation with these messages. Any
# other failure (registry, Rekor, TUF, network) is an error, never a missing artifact.
UNVERIFIED='no signatures found|no matching signatures|no matching attestations: *$|no matching CertificateIdentity'
# The push paths of .github/workflows/kernel-build.yml (a unit test keeps them equal).
KERNEL_BUILD_PATHS=('forge/specs/azoth/*' '.github/workflows/kernel-build.yml' '.github/workflows/nvidia-build.yml')
# The files .github/workflows/kernel-bump.yml regenerates with the pins, except
# system/Containerfile: a base bump is reviewed with its package delta (O8).
NVIDIA_PIN_FILES=(forge/specs/azoth/pins.env forge/specs/azoth/KERNEL.md forge/specs/azoth/nvidia/sources.sha256 system/nvidia/locks/open.lock system/nvidia/locks/legacy.lock)
KERNEL_PIN_FILES=("${NVIDIA_PIN_FILES[@]}" forge/specs/azoth/SOURCES/sources.sha256 forge/specs/azoth/builder/Containerfile forge/specs/azoth/boot/Containerfile forge/specs/azoth/nvidia/Containerfile)

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

usage() { sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$SELF" >&2; exit 2; }
die() { echo "kernel-artifacts: $*" >&2; exit 1; }
annotate() { # annotate notice|warning MESSAGE
  if [[ ${GITHUB_ACTIONS:-} == true ]]; then echo "::$1 title=Kernel artifacts::$2"; else echo "kernel-artifacts: $1: $2"; fi
}
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }
ask() { retry bash "$SELF" probe "$@"; }
identity() { [[ -n ${IDENTITY[${1:-}]:-} ]] || usage; echo "${IDENTITY[$1]}"; }

probe_digest() {
  local out status=0
  out=$(skopeo inspect --format '{{.Digest}}' "docker://$1" 2> "$TMP/err") || status=$?
  if [[ $status -ne 0 ]]; then
    grep -q 'manifest unknown' "$TMP/err" && return 0
    cat "$TMP/err" >&2
    return 1
  fi
  [[ $out =~ ^sha256:[0-9a-f]{64}$ ]] || die "$1: malformed digest '$out'"
  echo "$out"
}

probe_signed() {
  local status=0
  cosign verify --certificate-identity-regexp "$(identity "$2")" --certificate-oidc-issuer "$ISSUER" "$1" > /dev/null 2> "$TMP/err" || status=$?
  if [[ $status -eq 0 ]]; then
    echo signed
  elif grep -qE "$UNVERIFIED" "$TMP/err"; then
    echo unsigned
  else
    cat "$TMP/err" >&2
    return 1
  fi
}

probe_predicates() {
  local status=0
  cosign verify-attestation --type custom --certificate-identity-regexp "$(identity "$2")" --certificate-oidc-issuer "$ISSUER" "$1" > "$TMP/out" 2> "$TMP/err" || status=$?
  if [[ $status -eq 0 ]]; then
    jq -ce '.payload | @base64d | fromjson | .predicate.Data | fromjson' "$TMP/out" || die "$1: malformed attestation"
  elif grep -qE "$UNVERIFIED" "$TMP/err"; then
    echo unverified
  else
    cat "$TMP/err" >&2
    return 1
  fi
}

get() {
  local line
  [[ -f $FILE ]] || die "$FILE does not exist: run kernel-artifacts.sh resolve first"
  line=$(grep -m1 "^$1=" "$FILE") || die "$FILE has no $1"
  echo "${line#*=}"
}

write() { # write STATE LINE...: replace the file in one step
  mkdir -p "$DIR"
  printf '%s\n' "state=$1" "${@:2}" > "$FILE.tmp"
  mv "$FILE.tmp" "$FILE"
  echo "kernel-artifacts: state=$1"
}

module_verdict() { # module_verdict REF BRANCH KERNEL_DIGEST: verified or unverified
  local signed predicates pins
  signed=$(ask signed "$1" modules)
  [[ $signed == signed ]] || { echo unverified; return 0; }
  predicates=$(ask predicates "$1" modules)
  [[ $predicates != unverified ]] || { echo unverified; return 0; }
  pins=$(sed -n "s/^\(NVIDIA_${2^^}_[A-Z0-9_]*\)=\(.*\)$/\1\t\2/p" "$PINS" | jq -Rn '[inputs | split("\t") | {(.[0]): .[1]}] | add')
  jq -sr --arg branch "$2" --arg kernel "$3" --argjson pins "$pins" '
    if any(.[]; .driver == $branch and .kernel_digest == $kernel and (.pins as $p | $pins | to_entries | all(.value == $p[.key])))
    then "verified" else "unverified" end' <<< "$predicates"
}

resolve() {
  local expect='' nvr kernel devel kernel_signed devel_signed branch version tag digest verdict state=ready
  while [[ $# -gt 0 ]]; do
    case $1 in
      --expect-kernel-digest) [[ $# -ge 2 ]] || usage; expect=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  rm -f "$FILE"
  nvr=$(bash "$ROOT/forge/specs/azoth/nvr.sh")
  local -a lines=("nvr=$nvr" "registry=$REGISTRY")
  kernel=$(ask digest "$REGISTRY/azoth:$nvr")
  devel=$(ask digest "$REGISTRY/azoth-devel:$nvr")
  if [[ -z $kernel || -z $devel ]]; then
    write kernel-missing "${lines[@]}"
    return 0
  fi
  [[ -z $expect || $kernel == "$expect" ]] || die "$REGISTRY/azoth:$nvr is $kernel, the caller resolved $expect: the kernel was republished since"
  kernel_signed=$(ask signed "$REGISTRY/azoth@$kernel" kernel)
  devel_signed=$(ask signed "$REGISTRY/azoth-devel@$devel" kernel)
  if [[ $kernel_signed != signed || $devel_signed != signed ]]; then
    write kernel-missing "${lines[@]}"
    return 0
  fi
  lines+=("kernel_digest=$kernel" "devel_digest=$devel")
  for branch in open legacy; do
    version=$(sed -n "s/^NVIDIA_${branch^^}_VERSION=//p" "$PINS")
    [[ -n $version ]] || die "NVIDIA_${branch^^}_VERSION is not set in $PINS"
    tag="$nvr-k${kernel:7:12}-$branch-$version"
    lines+=("nvidia_${branch}_version=$version" "nvidia_${branch}_tag=$tag")
    digest=$(ask digest "$REGISTRY/azoth-nvidia:$tag")
    verdict=unverified
    [[ -z $digest ]] || verdict=$(module_verdict "$REGISTRY/azoth-nvidia@$digest" "$branch" "$kernel")
    if [[ $verdict == verified ]]; then
      lines+=("nvidia_${branch}_digest=$digest")
    else
      state=modules-missing
    fi
  done
  write "$state" "${lines[@]}"
}

changed_files() { # changed_files BASE HEAD
  git cat-file -e "$1^{commit}" 2> /dev/null || retry git fetch --no-tags --depth=1 origin "$1"
  git diff --name-only "$1" "$2"
}

matches_any() { # matches_any PATH PATTERN...
  local path=$1 pattern
  shift
  for pattern in "$@"; do
    # shellcheck disable=SC2053 # the right-hand side is a glob on purpose
    [[ $path == $pattern ]] && return 0
  done
  return 1
}

kernel_build_touched() { # kernel_build_touched BASE HEAD: yes or no
  local files file
  files=$(changed_files "$1" "$2")
  while IFS= read -r file; do
    if [[ -n $file ]] && matches_any "$file" "${KERNEL_BUILD_PATHS[@]}"; then
      echo yes
      return 0
    fi
  done <<< "$files"
  echo no
}

cycle() {
  local event='' before='' after='' sha='' head='' state nvr decision=build touched
  while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
      --event) event=$2 ;;
      --before) before=$2 ;;
      --after) after=$2 ;;
      --sha) sha=$2 ;;
      --head) head=$2 ;;
      *) usage ;;
    esac
    shift 2
  done
  state=$(get state)
  nvr=$(get nvr)
  if [[ $state != ready ]]; then
    case $event in
      push)
        if [[ -z $before || $before =~ ^0+$ ]]; then
          [[ $state == modules-missing ]] || die "azoth:$nvr is not published, and a push without a previous commit (new branch, force push) cannot tell whether Kernel Build owns it"
        else
          touched=$(kernel_build_touched "$before" "$after")
          if [[ $touched == yes ]]; then
            decision=defer
            annotate notice "$state for azoth:$nvr, and this push starts Kernel Build, which dispatches the Orchestrator for it: no image in this run"
          else
            [[ $state == modules-missing ]] || die "azoth:$nvr is not published and this push does not start Kernel Build"
          fi
        fi
        ;;
      workflow_dispatch)
        if [[ $state == kernel-missing ]]; then
          [[ -n $sha && $sha != "$head" ]] || die "azoth:$nvr is not published"
          decision=defer
          annotate notice "azoth:$nvr is not published and the branch moved from $sha to $head: the newer push has its own cycle"
        fi
        ;;
      schedule)
        [[ $state == modules-missing ]] || die "azoth:$nvr is not published"
        ;;
      *) die "cycle: unknown event '$event'" ;;
    esac
  fi
  echo "cycle=$decision" >> "$FILE"
  echo "kernel-artifacts: cycle=$decision"
}

check_plan() {
  local base='' head='' state nvr files file keys key only_kernel_pins=true only_nvidia_pins=true nvidia_moved=false other_moved=false gpus delta
  while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
      --base) base=$2 ;;
      --head) head=$2 ;;
      *) usage ;;
    esac
    shift 2
  done
  [[ -n $base && -n $head ]] || usage
  state=$(get state)
  nvr=$(get nvr)
  files=$(changed_files "$base" "$head")
  while IFS= read -r file; do
    [[ -n $file ]] || continue
    matches_any "$file" "${KERNEL_PIN_FILES[@]}" || only_kernel_pins=false
    matches_any "$file" "${NVIDIA_PIN_FILES[@]}" || only_nvidia_pins=false
  done <<< "$files"
  keys=$(git diff -U0 "$base" "$head" -- forge/specs/azoth/pins.env | sed -n 's/^[-+]\([A-Z_][A-Z0-9_]*\)=.*/\1/p' | sort -u)
  while IFS= read -r key; do
    [[ -n $key ]] || continue
    if [[ $key == NVIDIA_* ]]; then nvidia_moved=true; else other_moved=true; fi
  done <<< "$keys"
  case $state in
    ready)
      gpus='none nvidia nvidia-legacy' delta=true
      ;;
    kernel-missing)
      [[ $only_kernel_pins == true && -n $keys ]] || die "azoth:$nvr is not published: a pin bump mixed with other changes cannot be checked, move the pins in their own pull request"
      gpus='' delta=false
      annotate warning "azoth:$nvr is not published yet: Kernel Build on this pull request proves the kernel and the modules build and boot; the images are built after the merge"
      ;;
    modules-missing)
      [[ $only_nvidia_pins == true && $nvidia_moved == true && $other_moved == false ]] || die "the NVIDIA modules of azoth:$nvr are not published: with unchanged NVIDIA pins publishing them is the Orchestrator's job (bootstrap or interrupted publication), and NVIDIA pins move in their own pull request"
      gpus=none delta=true
      annotate warning "the NVIDIA modules of the new pins are not published yet: only the default image is built; the variants are built and gated after the merge"
      ;;
    *) die "$FILE: unknown state '$state'" ;;
  esac
  printf '%s\n' "check_gpus=$gpus" "check_delta=$delta" >> "$FILE"
  echo "kernel-artifacts: check_gpus='$gpus' check_delta=$delta"
}

[[ $# -ge 1 ]] || usage
command=$1
shift
case $command in
  resolve) resolve "$@" ;;
  require-ready)
    [[ $# -eq 0 ]] || usage
    resolve
    state=$(get state)
    [[ $state == ready ]] || die "state=$state: the kernel of the pins and both NVIDIA module branches must be published, signed and attested"
    ;;
  cycle) cycle "$@" ;;
  check-plan) check_plan "$@" ;;
  get) [[ $# -eq 1 ]] || usage; get "$1" ;;
  has) [[ $# -eq 1 ]] || usage; [[ -f $FILE ]] && grep -q "^$1=." "$FILE" ;;
  digest) [[ $# -eq 1 ]] || usage; ask digest "$1" ;;
  signed) [[ $# -eq 2 ]] || usage; ask signed "$1" "$2" ;;
  predicates) [[ $# -eq 2 ]] || usage; ask predicates "$1" "$2" ;;
  probe)
    [[ $# -ge 2 ]] || usage
    case $1 in
      digest) [[ $# -eq 2 ]] || usage; probe_digest "$2" ;;
      signed) [[ $# -eq 3 ]] || usage; probe_signed "$2" "$3" ;;
      predicates) [[ $# -eq 3 ]] || usage; probe_predicates "$2" "$3" ;;
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac
```
