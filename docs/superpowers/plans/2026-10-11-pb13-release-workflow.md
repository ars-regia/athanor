# PB13 Release Workflow, First Pull Request: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `release.yml` and the Orchestrator's `request-release` job, so that one build of `iso-v0` can be released to `:latest` by a workflow that checks the build against the API before it signs, verifies and tags. This is the first of PB13's two pull requests: `request-release` writes the request but does not dispatch, and the old jobs stay.

**Architecture:** The build's last job writes `release-request.json` (run id, attempt, workflow path, ref, commit, builder hash, and the SHA-256 of the three digest files). `release.yml` runs on `workflow_dispatch` with one input, `build_run_id`. Its `candidate` job downloads the build's artifacts, checks them against the API's record of the run and checks that `:latest` would move forwards. Its later jobs then reuse the scripts the Orchestrator's sign, verify and tag jobs already call, on the files of `release-candidate`.

**Tech Stack:** GitHub Actions YAML, bash, Python 3 (stdlib only), `gh`, `skopeo`, `cosign`, and the offline fakes in `system/tests/fake_registry.py`.

**Spec:** `docs/architecture/doc_pipeline.md` revision 3 (approved in #407), section 4.10 (PL54 to PL60) and the PB13 row of the build plan; ADR-0104; ADR-0111.

## Global Constraints

- Base: `origin/iso-v0` at `14807f70`, worktree `/var/tmp/athanor-wt/pb13`, branch `feat/pb13-release-workflow`.
- Scope is PB13's first pull request only. `request-release` writes and uploads the request and **does not dispatch**. `sign-system-images`, `verify-system-images` and `tag-system-images` stay. The `verify.py workflows` rules of PL56 (ref check present, no second writer of `:latest`, signing only in `release.yml`) belong to the second pull request.
- PL54: "every job checks that the run's ref is `refs/heads/iso-v0` and fails, never skips, on any other ref". "The input `build_run_id` must match `^[0-9]+$`, and every job passes it to its scripts through `env`, never interpolated into a `run:` line".
- PL57: `release.yml` concurrency group `${{ github.workflow }}`, `cancel-in-progress: false`.
- D43: the signing job uses only checkout, download-artifact and upload-artifact pinned by SHA; it runs only commands in `SIGN_JOB_COMMANDS`; it sets env only from `SIGN_JOB_ENV` plus its environment's secrets; it has no `defaults` beyond `{"run": {"shell": "bash"}}`; and `release.yml` sets no workflow-level `env` or `defaults`.
- **Stop and ask (AGENTS.md):** the `sign-images` job (environment `signing`) and the new `SIGN_JOB_COMMANDS` entry. The maintainer's approval of this plan covers exactly the text given in Task 6. Any other change to these needs a new question.
- `sign-images` runs in `signing` until step 3 of the image key rotation, as `sign-system-images` does today.
- Stdlib-only Python. `jsonschema` is not a repository dependency, so the schemas are checked by a small validator of the subset they use (Task 3).
- No `|| true`, no `2>/dev/null` that hides an error, no new `unwrap`. English in everything committed, Conventional Commits, no attribution.
- Edit Markdown under `docs/` with exact-replacement Python scripts written in the scratchpad, never with Edit or Write, and check `git diff --numstat` afterwards. `scripts/verify.py` is edited the same way.
- Run tests through `node /home/hr-mes/.claude/bin/cc-test.mjs -- <command>`.

## Review Focus

1. **A build run still `in_progress` when `candidate` reads it.** This is the normal case from PR 2 on, because `request-release` dispatches from inside the build. `trusted-run.sh --in-progress` must trust an `in_progress` run with conclusion `null`, and still refuse `queued`, `waiting`, and a completed run that failed. Pinned in Task 2.
2. **A job list longer than one API page.** The package matrix can exceed 100 jobs. `candidate` reads the jobs with `--paginate --slurp` and must see a failed job on the second page. Pinned in Task 4 (`test_a_failed_job_on_a_later_page_refuses`).
3. **`:latest` written by `main`'s old jobs, or with no revision label.** PL58 orders it by build time alone, and a label commit absent from the repository fails. Pinned in Task 4.
4. **A registry error that is not "manifest unknown".** It must fail the order check, never read as "no `:latest`". Pinned in Task 4.
5. **A re-run of `tag-latest` after another release moved `:latest`.** `order` runs again on the files of its own run and must refuse an older candidate. Pinned in Task 4 (`order` on an existing `release-candidate` directory) and wired in Task 6.

Known ceiling, not tested: in PR 2, `gh run download` must see the artifacts of a build run that is still `in_progress`. Artifacts v4 are listed as soon as they are uploaded, but this is verified only by the first automatic release run after PR 2.

---

### Task 1: Every image names the commit it was built from

**Files:**
- Modify: `system/build-image.sh` (the `args+=(--label "org.opencontainers.image.version=...` block, about line 128)
- Create: `system/check-image-revision.sh`
- Modify: `.github/workflows/call-system-image.yml` (new step after "🐳 Build OS Images")
- Test: `system/tests/test_build_image.py`, `system/tests/test_check_image_revision.py`

**Interfaces:**
- Produces: the label `org.opencontainers.image.revision=<40-hex commit>` on every non-system image; `check-image-revision.sh REVISION IMAGE...` (exit 0 when all match, 1 on a mismatch, 2 on usage). Task 4 reads the label.

- [ ] **Step 1: Write the failing tests**

In `system/tests/test_build_image.py`, after `test_every_image_carries_its_own_version_and_build_time`:

```python
    def test_every_image_names_the_commit_it_was_built_from(self):
        self.artifacts_file()
        self.env["GITHUB_SHA"] = "e" * 40
        r = subprocess.run(["bash", str(BUILD), "--gpu", "none", "--registry", "localhost", "--tag", "check"],
                           capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("org.opencontainers.image.revision=" + "e" * 40, self.calls()[-1])

    def test_a_revision_that_is_not_a_commit_builds_nothing(self):
        self.artifacts_file()
        self.env["GITHUB_SHA"] = "iso-v0"
        r = subprocess.run(["bash", str(BUILD), "--gpu", "none", "--registry", "localhost", "--tag", "check"],
                           capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 2)
        self.assertIn("not a commit", r.stderr)
```

Create `system/tests/test_check_image_revision.py`:

```python
"""system/check-image-revision.sh with a podman stub that answers the label of each image."""

import os
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "check-image-revision.sh"
REV = "a" * 40


class CheckImageRevision(unittest.TestCase):
    def run_with(self, labels, *args):
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = pathlib.Path(tmp)
            # The stub prints the label of the image named last, or "<no value>" as podman does.
            table = "\n".join(f'  "{image}") echo "{label}" ;;' for image, label in labels.items())
            (bin_dir / "podman").write_text(textwrap.dedent("""\
                #!/usr/bin/env bash
                case "${@: -1}" in
                """) + table + '\n  *) echo "<no value>" ;;\nesac\n')
            (bin_dir / "podman").chmod(0o755)
            env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}"}
            return subprocess.run(["bash", str(SCRIPT), *args], capture_output=True, text=True, env=env)

    def test_images_that_name_the_commit_pass(self):
        r = self.run_with({"r/a:1": REV, "r/b:1": REV}, REV, "r/a:1", "r/b:1")
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_an_image_without_the_label_fails(self):
        r = self.run_with({"r/a:1": REV}, REV, "r/a:1", "r/b:1")
        self.assertEqual(r.returncode, 1)
        self.assertIn("r/b:1", r.stderr)

    def test_an_image_of_another_commit_fails(self):
        r = self.run_with({"r/a:1": "b" * 40}, REV, "r/a:1")
        self.assertEqual(r.returncode, 1)

    def test_a_revision_that_is_not_a_commit_is_a_usage_error(self):
        self.assertEqual(self.run_with({}, "iso-v0", "r/a:1").returncode, 2)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s system/tests -p 'test_*image*.py' -v`
Expected: the two new `test_build_image` cases and every `test_check_image_revision` case FAIL.

- [ ] **Step 3: Implement**

In `system/build-image.sh`, replace

```bash
args+=(--label "org.opencontainers.image.version=$fedora.$(date -u -d "@$now" +%Y%m%d).$SERIAL"
  --label "org.opencontainers.image.created=$(date -u -d "@$now" +%Y-%m-%dT%H:%M:%SZ)")
```

with

```bash
# The commit the image was built from (doc_pipeline.md PL58): a release compares it with the
# commit of :latest. GITHUB_SHA in the pipeline, HEAD in a local build.
revision=${GITHUB_SHA:-$(git -C "$ROOT" rev-parse HEAD)}
[[ $revision =~ ^[0-9a-f]{40}$ ]] || { echo "${0##*/}: revision '$revision' is not a commit" >&2; exit 2; }
args+=(--label "org.opencontainers.image.version=$fedora.$(date -u -d "@$now" +%Y%m%d).$SERIAL"
  --label "org.opencontainers.image.created=$(date -u -d "@$now" +%Y-%m-%dT%H:%M:%SZ)"
  --label "org.opencontainers.image.revision=$revision")
```

Create `system/check-image-revision.sh` (mode 755):

```bash
#!/usr/bin/env bash
# The image check of the revision label (docs/architecture/doc_pipeline.md, PL55 and PL58):
# every image the run built names the commit it was built from, so the order check of a later
# release can compare commits. An image without the label fails its own build.
# Usage: check-image-revision.sh REVISION IMAGE...
set -euo pipefail

[[ $# -ge 2 && $1 =~ ^[0-9a-f]{40}$ ]] || {
    echo "usage: ${0##*/} REVISION IMAGE..." >&2
    exit 2
}
revision=$1
shift
for image in "$@"; do
    label=$(podman image inspect --format '{{index .Labels "org.opencontainers.image.revision"}}' "$image")
    [[ $label == "$revision" ]] || {
        echo "${0##*/}: $image: revision label '$label', expected $revision" >&2
        exit 1
    }
    echo "$image: org.opencontainers.image.revision=$label"
done
```

In `.github/workflows/call-system-image.yml`, insert after the "🐳 Build OS Images" step:

```yaml
      - name: 🔖 Check that every image names its commit (system/check-image-revision.sh)
        # The order check of a release compares the commit of :latest with the candidate's
        # (doc_pipeline.md PL58): an image without the label fails here, before the push.
        env:
          RUN_ID: ${{ github.run_id }}
        run: |
          set -euo pipefail
          mapfile -t names < artifacts/variants.txt
          images=()
          for name in "${names[@]}"; do images+=("${IMAGE_REGISTRY}/${name}:${RUN_ID}"); done
          bash system/check-image-revision.sh "${GITHUB_SHA}" "${images[@]}" | tee -a "${GITHUB_STEP_SUMMARY}"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s system/tests -v`
Expected: PASS, with no other test changed.
Then: `shellcheck system/check-image-revision.sh system/build-image.sh && actionlint .github/workflows/call-system-image.yml`

- [ ] **Step 5: Commit**

```bash
git add system/build-image.sh system/check-image-revision.sh system/tests/test_build_image.py \
  system/tests/test_check_image_revision.py .github/workflows/call-system-image.yml
git commit -m "feat(image): label every image with the commit it was built from (PB13)"
```

---

### Task 2: `trusted-run.sh --in-progress`

**Files:**
- Modify: `system/trusted-run.sh`
- Test: `system/tests/test_accept_target.py`

**Interfaces:**
- Produces: `trusted-run.sh --in-progress RUN_ID WORKFLOW_FILE...`. It trusts a run whose `status` is `in_progress` with `conclusion` `null`, or that is `completed` with `success`. Every other check is unchanged. Exit 0 when trusted, 10 when not. Task 4 calls it.

- [ ] **Step 1: Write the failing tests**

Add to class `AcceptTarget` in `system/tests/test_accept_target.py`:

```python
    def test_in_progress_trusts_a_build_still_running_its_last_job(self):
        self.github(run_object(status="in_progress", conclusion=None))
        self.assertEqual(self.trusted("--in-progress").returncode, 0)
        self.assertEqual(self.trusted().returncode, 10)

    def test_in_progress_trusts_a_completed_successful_run(self):
        self.github()
        self.assertEqual(self.trusted("--in-progress").returncode, 0)

    def test_in_progress_refuses_a_failed_or_waiting_run(self):
        for over in ({"conclusion": "failure"}, {"status": "queued", "conclusion": None},
                     {"status": "waiting", "conclusion": None}, {"status": "in_progress", "conclusion": "failure"}):
            with self.subTest(**over):
                self.github(run_object(**over))
                r = self.trusted("--in-progress")
                self.assertEqual(r.returncode, 10, r.stderr)

    def test_in_progress_still_requires_the_commit_on_the_branch(self):
        self.github(run_object(status="in_progress", conclusion=None), compare="diverged")
        self.assertEqual(self.trusted("--in-progress").returncode, 10)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s system/tests -p test_accept_target.py -v`
Expected: the first two FAIL, because `--in-progress` is a usage error (exit 2).

- [ ] **Step 3: Implement**

In `system/trusted-run.sh`:

1. Header comment: after the `--completed` sentence add "With --in-progress a run still `in_progress` is trusted too: `release.yml` checks a build whose `request-release` job is still running (doc_pipeline.md PL55)." Change the usage line to `[--completed | --in-progress]`, in the comment and in the usage message.
2. Replace the flag parsing with:

```bash
conclusions='["success"]'
in_progress=false
case ${1:-} in
--completed)
    conclusions='["success", "failure"]'
    shift
    ;;
--in-progress)
    in_progress=true
    shift
    ;;
esac
```

3. Add `--argjson inprog "$in_progress"` to the `jq` call, and replace the status and conclusion lines with:

```
    (if .status == "completed" or ($inprog and .status == "in_progress") then empty else "status \(.status)" end),
    (if (.conclusion | IN($conclusions[])) or ($inprog and .status == "in_progress" and .conclusion == null) then empty else "conclusion \(.conclusion)" end)
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s system/tests -p 'test_*.py' -v` and `shellcheck system/trusted-run.sh`
Expected: PASS, including `test_fetch_evidence.py`, unchanged.

- [ ] **Step 5: Commit**

```bash
git add system/trusted-run.sh system/tests/test_accept_target.py
git commit -m "feat(release): trust a build run still in progress for the release request (PB13)"
```

---

### Task 3: The release request and its schema

**Files:**
- Create: `scripts/ci/schema.py`, `scripts/ci/schemas/release-request.schema.json`, `scripts/ci/release_request.py`
- Test: `scripts/ci/tests/test_release_request.py`

**Interfaces:**
- Produces:
  - `schema.validate(doc, schema)`, which raises `ValueError` listing every problem; `schema.load(name)` returns `scripts/ci/schemas/<name>.schema.json` as a dict.
  - `release_request.py --image-digests FILE --kernel-digests DIR --out DIR`, which writes `DIR/release-request.json`.
  - The request fields: `schema_version` (1), `build_run_id` (int), `build_run_attempt` (int), `workflow_path`, `ref`, `commit`, `builder_content_hash`, and `sha256` = {`image-digests.txt`, `kernel-digests.txt`, `kernel-unsigned.txt`: 64-hex}.

- [ ] **Step 1: Write the failing tests**

`scripts/ci/tests/test_release_request.py`:

```python
"""scripts/ci/release_request.py and the schema subset of scripts/ci/schema.py (doc_pipeline.md PL55)."""

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

CI = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CI))
import schema  # noqa: E402

SCRIPT = CI / "release_request.py"
SHA = "c" * 40
BUILDER = "f" * 64
ENV = {"GITHUB_RUN_ID": "412", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_REPOSITORY": "ars-regia/athanor",
       "GITHUB_WORKFLOW_REF": "ars-regia/athanor/.github/workflows/athanor-forge-orchestrator.yml@refs/heads/iso-v0",
       "GITHUB_REF": "refs/heads/iso-v0", "GITHUB_SHA": SHA, "BUILDER_HASH": BUILDER}


class ReleaseRequest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        (self.dir / "image-digests.txt").write_text("ghcr.io/ars-regia/athanor-system 412 sha256:" + "1" * 64 + "\n")
        (self.dir / "kernel").mkdir()
        (self.dir / "kernel/kernel-digests.txt").write_text("k\n")
        (self.dir / "kernel/kernel-unsigned.txt").write_text("")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, **env):
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT), "--image-digests", str(self.dir / "image-digests.txt"),
             "--kernel-digests", str(self.dir / "kernel"), "--out", str(self.dir / "out")],
            capture_output=True, text=True, env={**os.environ, **ENV, **env})

    def test_the_request_names_the_run_and_hashes_the_three_files(self):
        r = self.write()
        self.assertEqual(r.returncode, 0, r.stderr)
        request = json.loads((self.dir / "out/release-request.json").read_text())
        self.assertEqual(request["build_run_id"], 412)
        self.assertEqual(request["build_run_attempt"], 2)
        self.assertEqual(request["workflow_path"], ".github/workflows/athanor-forge-orchestrator.yml")
        self.assertEqual(request["commit"], SHA)
        self.assertEqual(request["sha256"]["kernel-unsigned.txt"], hashlib.sha256(b"").hexdigest())
        schema.validate(request, schema.load("release-request"))

    def test_a_malformed_value_writes_nothing(self):
        for env in ({"GITHUB_SHA": "iso-v0"}, {"BUILDER_HASH": "x"}, {"GITHUB_RUN_ID": "12a"}):
            with self.subTest(**env):
                r = self.write(**env)
                self.assertEqual(r.returncode, 1)
                self.assertFalse((self.dir / "out/release-request.json").exists())

    def test_a_missing_kernel_file_writes_nothing(self):
        (self.dir / "kernel/kernel-unsigned.txt").unlink()
        self.assertEqual(self.write().returncode, 1)


class Schema(unittest.TestCase):
    S = {"type": "object", "required": ["a"], "additionalProperties": False,
         "properties": {"a": {"type": "integer", "minimum": 1}, "b": {"type": ["string", "null"], "pattern": "^x+$"},
                        "c": {"type": "array", "items": {"enum": ["p", "q"]}}}}

    def test_a_valid_document_passes(self):
        schema.validate({"a": 1, "b": None, "c": ["p"]}, self.S)

    def test_each_violation_is_named(self):
        for doc, word in (({}, "a"), ({"a": 0}, "minimum"), ({"a": True}, "integer"), ({"a": 1, "z": 1}, "z"),
                          ({"a": 1, "b": "y"}, "pattern"), ({"a": 1, "c": ["r"]}, "enum")):
            with self.subTest(doc=doc):
                with self.assertRaisesRegex(ValueError, word):
                    schema.validate(doc, self.S)

    def test_a_keyword_outside_the_subset_fails_instead_of_passing(self):
        with self.assertRaisesRegex(ValueError, "maxLength"):
            schema.validate("x", {"type": "string", "maxLength": 3})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/ci/tests -p test_release_request.py -v`
Expected: ERROR, "No module named 'schema'".

- [ ] **Step 3: Implement**

`scripts/ci/schema.py`:

```python
"""The subset of JSON Schema that the schemas of scripts/ci/schemas use, checked with the
standard library: jsonschema is not a dependency of the pipeline. A keyword outside the subset
fails, so a schema never appears to say more than is checked."""

import json
import pathlib
import re

SCHEMAS = pathlib.Path(__file__).resolve().parent / "schemas"
KEYWORDS = {"$schema", "$id", "title", "description", "type", "required", "properties",
            "additionalProperties", "items", "pattern", "const", "enum", "minimum"}
TYPES = {"object": dict, "array": list, "string": str, "integer": int, "null": type(None)}


def load(name):
    return json.loads((SCHEMAS / f"{name}.schema.json").read_text())


def errors(doc, schema, at="$"):
    unknown = sorted(set(schema) - KEYWORDS)
    if unknown or schema.get("additionalProperties", False) is not False:
        return [f"{at}: schema uses {unknown or ['additionalProperties other than false']}, outside the subset"]
    kinds = schema.get("type", [])
    kinds = [kinds] if isinstance(kinds, str) else kinds
    if kinds and not any(isinstance(doc, TYPES[k]) and not isinstance(doc, bool) for k in kinds):
        return [f"{at}: not {' or '.join(kinds)}"]
    out = []
    if "const" in schema and doc != schema["const"]:
        out.append(f"{at}: not {schema['const']!r}")
    if "enum" in schema and doc not in schema["enum"]:
        out.append(f"{at}: not in enum {schema['enum']}")
    if "pattern" in schema and isinstance(doc, str) and not re.search(schema["pattern"], doc):
        out.append(f"{at}: does not match pattern {schema['pattern']}")
    if "minimum" in schema and isinstance(doc, int) and doc < schema["minimum"]:
        out.append(f"{at}: below minimum {schema['minimum']}")
    if isinstance(doc, dict):
        props = schema.get("properties", {})
        out += [f"{at}.{k}: missing" for k in schema.get("required", []) if k not in doc]
        if "additionalProperties" in schema:
            out += [f"{at}.{k}: not allowed" for k in doc if k not in props]
        for k, v in doc.items():
            if k in props:
                out += errors(v, props[k], f"{at}.{k}")
    if isinstance(doc, list) and "items" in schema:
        for i, v in enumerate(doc):
            out += errors(v, schema["items"], f"{at}[{i}]")
    return out


def validate(doc, schema):
    problems = errors(doc, schema)
    if problems:
        raise ValueError("; ".join(problems))
```

`scripts/ci/schemas/release-request.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "release-request.json (docs/architecture/doc_pipeline.md, PL55)",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema_version", "build_run_id", "build_run_attempt", "workflow_path", "ref", "commit", "builder_content_hash", "sha256"],
  "properties": {
    "schema_version": {"const": 1},
    "build_run_id": {"type": "integer", "minimum": 1},
    "build_run_attempt": {"type": "integer", "minimum": 1},
    "workflow_path": {"type": "string", "pattern": "^\\.github/workflows/[a-z0-9-]+\\.ya?ml$"},
    "ref": {"type": "string", "pattern": "^refs/(heads|tags)/[A-Za-z0-9._/-]+$"},
    "commit": {"type": "string", "pattern": "^[0-9a-f]{40}$"},
    "builder_content_hash": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
    "sha256": {
      "type": "object",
      "additionalProperties": false,
      "required": ["image-digests.txt", "kernel-digests.txt", "kernel-unsigned.txt"],
      "properties": {
        "image-digests.txt": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
        "kernel-digests.txt": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
        "kernel-unsigned.txt": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
      }
    }
  }
}
```

`scripts/ci/release_request.py`:

```python
#!/usr/bin/env python3
"""Writes release-request.json, what the build asks release.yml to release
(docs/architecture/doc_pipeline.md, PL54 and PL55). release.yml's candidate job checks every
field against the API's record of this run and the hashes against the files it downloads.

  release_request.py --image-digests FILE --kernel-digests DIR --out DIR

Environment: GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT, GITHUB_REPOSITORY, GITHUB_WORKFLOW_REF,
GITHUB_REF, GITHUB_SHA, BUILDER_HASH (the content hash of this run's builder image).
"""

import argparse
import hashlib
import json
import os
import pathlib
import sys

import schema


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--image-digests", required=True, type=pathlib.Path)
    p.add_argument("--kernel-digests", required=True, type=pathlib.Path)
    p.add_argument("--out", required=True, type=pathlib.Path)
    a = p.parse_args(argv)
    env = os.environ
    try:
        files = {"image-digests.txt": a.image_digests,
                 "kernel-digests.txt": a.kernel_digests / "kernel-digests.txt",
                 "kernel-unsigned.txt": a.kernel_digests / "kernel-unsigned.txt"}
        workflow = env["GITHUB_WORKFLOW_REF"].removeprefix(env["GITHUB_REPOSITORY"] + "/").split("@", 1)[0]
        request = {
            "schema_version": 1,
            "build_run_id": int(env["GITHUB_RUN_ID"]),
            "build_run_attempt": int(env["GITHUB_RUN_ATTEMPT"]),
            "workflow_path": workflow,
            "ref": env["GITHUB_REF"],
            "commit": env["GITHUB_SHA"],
            "builder_content_hash": env["BUILDER_HASH"],
            "sha256": {name: sha256(path) for name, path in files.items()},
        }
        schema.validate(request, schema.load("release-request"))
    except (KeyError, ValueError, OSError) as error:
        print(f"release_request.py: no request written: {error!r}", file=sys.stderr)
        return 1
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "release-request.json").write_text(json.dumps(request, indent=2) + "\n")
    print(json.dumps(request, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/ci/tests -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/ci/schema.py scripts/ci/schemas/release-request.schema.json scripts/ci/release_request.py \
  scripts/ci/tests/test_release_request.py
git commit -m "feat(release): write the release request of a build (PB13, PL55)"
```

---

### Task 4: The candidate check and the order check (the PB13 gate)

**Files:**
- Create: `scripts/ci/release_candidate.py`, `scripts/ci/schemas/release-candidate.schema.json`
- Test: `scripts/ci/tests/test_release_candidate.py`

**Interfaces:**
- Consumes: `schema.validate`/`schema.load` (Task 3); `system/trusted-run.sh --in-progress` (Task 2); the revision label (Task 1); the request fields (Task 3).
- Produces:
  - `release_candidate.py check --out DIR`, with env `BUILD_RUN_ID`, `GITHUB_REPOSITORY`, `GITHUB_REF`, and `gh` authenticated. It writes `DIR/release-candidate.json`, `DIR/image-digests.txt`, `DIR/kernel-digests.txt` and `DIR/kernel-unsigned.txt`. It exits 0 when the check passes, 1 when refused.
  - `release_candidate.py order --dir DIR`, which re-reads those files and repeats PL58. It exits 0 or 1.
  - `release-candidate.json` = {`schema_version`: 1, `request`: the request, `order`: [{`repository`, `candidate_digest`, `latest_digest` (str or null), `latest_created` (str or null), `verdict`: `absent`, `equal` or `newer`}], `superseded`: [run ids]}.

- [ ] **Step 1: Write the failing tests**

`scripts/ci/tests/test_release_candidate.py`. It reuses the `Tool` harness and `fake_registry.py` of `system/tests`: `gh api` answers from `api`, `gh run download` from `run_artifacts`, and `skopeo inspect` from `tags` and `configs`.

```python
"""scripts/ci/release_candidate.py against an offline GitHub and registry: the gate of PB13
(doc_pipeline.md PL55, PL58). Every refusal below is a release that must not be signed."""

import hashlib
import json
import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "system/tests"))
from test_kernel_artifacts import Tool  # noqa: E402

SCRIPT = ROOT / "scripts/ci/release_candidate.py"
R = "ars-regia/athanor"
REG = "ghcr.io/ars-regia"
ORCH = ".github/workflows/athanor-forge-orchestrator.yml"
SHA, TIP, OLD = "c" * 40, "d" * 40, "b" * 40
IMAGE_JOB = "system-image / 💿 System Image & Disk Build"
NAMES = ["athanor-system", "athanor-system-nvidia"]


def digest(n):
    return "sha256:" + f"{n:x}" * 64


DIGESTS = "".join(f"{REG}/{name} 412 {digest(i + 1)}\n" for i, name in enumerate(NAMES))
KERNEL, UNSIGNED = f"{REG}/azoth {digest(7)}\n", ""
SUPERSEDED = f"/repos/{R}/actions/workflows/athanor-forge-orchestrator.yml/runs?branch=iso-v0&status=success&created=>=2026-10-10T00:00:00Z&per_page=100"


def h(text):
    return hashlib.sha256(text.encode()).hexdigest()


def request(**over):
    return {"schema_version": 1, "build_run_id": 412, "build_run_attempt": 1, "workflow_path": ORCH,
            "ref": "refs/heads/iso-v0", "commit": SHA, "builder_content_hash": "f" * 64,
            "sha256": {"image-digests.txt": h(DIGESTS), "kernel-digests.txt": h(KERNEL), "kernel-unsigned.txt": h(UNSIGNED)},
            **over}


def run_object(**over):
    return {"id": 412, "run_attempt": 1, "head_sha": SHA, "path": ORCH, "head_branch": "iso-v0", "event": "push",
            "status": "in_progress", "conclusion": None, "created_at": "2026-10-11T04:00:00Z",
            "head_repository": {"full_name": R}, **over}


def jobs(*extra, image="success"):
    base = [{"name": "lint", "conclusion": "success"}, {"name": "nvidia-kmod-sign", "conclusion": "skipped"},
            {"name": IMAGE_JOB, "conclusion": image}, {"name": "request-release", "conclusion": None}]
    return [{"jobs": base + list(extra)}]


class Candidate(Tool):
    def github(self, req=None, run=None, pages=None, files=None, latest=True, labels=None, compare=None, extra=None):
        """A build run 412 of iso-v0 and a :latest built on 2026-10-10 from OLD."""
        files = {"image-digests.txt": DIGESTS, "kernel-digests.txt": KERNEL, "kernel-unsigned.txt": UNSIGNED, **(files or {})}
        api = {
            f"/repos/{R}/actions/runs/412": run or run_object(),
            f"/repos/{R}/actions/runs/412/attempts/1/jobs?per_page=100": pages or jobs(),
            f"/repos/{R}/branches/iso-v0": {"commit": {"sha": TIP}},
            f"/repos/{R}/compare/{SHA}...{TIP}": {"status": "ahead"},
            f"/repos/{R}/compare/{OLD}...{TIP}": {"status": "ahead"},
            f"/repos/{R}/compare/{OLD}...{SHA}": {"status": "ahead"},
            SUPERSEDED: [{"workflow_runs": [
                {"id": 400, "created_at": "2026-10-10T12:00:00Z", "head_branch": "iso-v0", "event": "push"},
                {"id": 412, "created_at": "2026-10-11T04:00:00Z", "head_branch": "iso-v0", "event": "push"},
                {"id": 413, "created_at": "2026-10-11T05:00:00Z", "head_branch": "iso-v0", "event": "push"}]}],
            **{f"/repos/{R}/compare/{k}": {"status": v} for k, v in (compare or {}).items()},
        }
        tags, configs = {}, {}
        for i, name in enumerate(NAMES):
            configs[f"{REG}/{name}@{digest(i + 1)}"] = {"org.opencontainers.image.created": "2026-10-11T04:30:00Z",
                                                       "org.opencontainers.image.revision": SHA}
            if latest:
                tags[f"{REG}/{name}:latest"] = digest(i + 8)
                configs[f"{REG}/{name}@{digest(i + 8)}"] = {"org.opencontainers.image.created": "2026-10-10T00:00:00Z",
                                                           "org.opencontainers.image.revision": OLD, **(labels or {})}
        self.registry({
            "api": api, "tags": tags, "configs": configs, **(extra or {}),
            "run_artifacts": {"412": {
                "release-request": {"release-request.json": json.dumps(req or request())},
                "image-digests": {"image-digests.txt": files["image-digests.txt"]},
                "kernel-digests": {"kernel-digests.txt": files["kernel-digests.txt"], "kernel-unsigned.txt": files["kernel-unsigned.txt"]},
            }},
        })

    def check(self, run_id="412", ref="refs/heads/iso-v0"):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), "check", "--out", str(self.dir / "out")],
                              capture_output=True, text=True, cwd=ROOT,
                              env={**self.env, "BUILD_RUN_ID": run_id, "GITHUB_REF": ref})

    def order(self):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), "order", "--dir", str(self.dir / "out")],
                              capture_output=True, text=True, cwd=ROOT, env={**self.env, "GITHUB_REF": "refs/heads/iso-v0"})

    def refused(self, word, **kw):
        r = self.check(**kw)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn(word, r.stderr)
        self.assertFalse((self.dir / "out/release-candidate.json").exists())

    # The request check (PL55)

    def test_a_successful_build_of_iso_v0_is_a_candidate(self):
        self.github()
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        out = json.loads((self.dir / "out/release-candidate.json").read_text())
        self.assertEqual(out["request"]["build_run_id"], 412)
        self.assertEqual([o["verdict"] for o in out["order"]], ["newer", "newer"])
        self.assertEqual(out["superseded"], [400])
        self.assertEqual((self.dir / "out/image-digests.txt").read_text(), DIGESTS)

    def test_a_completed_build_is_a_candidate_too(self):
        self.github(run=run_object(status="completed", conclusion="success"))
        self.assertEqual(self.check().returncode, 0)

    def test_a_run_id_that_is_not_a_number_refuses(self):
        self.github()
        self.refused("build_run_id", run_id="412; echo")

    def test_a_release_run_on_another_ref_refuses(self):
        self.github()
        self.refused("refs/heads/main", ref="refs/heads/main")

    def test_a_request_from_another_workflow_refuses(self):
        self.github(req=request(workflow_path=".github/workflows/other.yml"))
        self.refused("workflow")

    def test_a_run_of_another_workflow_refuses(self):
        self.github(run=run_object(path=".github/workflows/other.yml"))
        self.refused("path")

    def test_a_request_of_another_attempt_refuses(self):
        self.github(req=request(build_run_attempt=2))
        self.refused("attempt")

    def test_a_request_of_another_run_refuses(self):
        self.github(req=request(build_run_id=411))
        self.refused("run id")

    def test_a_request_of_another_commit_refuses(self):
        self.github(req=request(commit="e" * 40))
        self.refused("commit")

    def test_a_request_of_another_ref_refuses(self):
        self.github(req=request(ref="refs/heads/main"))
        self.refused("ref")

    def test_a_build_of_another_branch_refuses(self):
        self.github(run=run_object(head_branch="main"))
        self.refused("branch")

    def test_a_tag_named_like_the_branch_refuses(self):
        self.github(compare={f"{SHA}...{TIP}": "diverged"})
        self.refused("not on iso-v0")

    def test_a_fork_refuses(self):
        self.github(run=run_object(head_repository={"full_name": "someone/athanor"}))
        self.refused("repository")

    def test_a_pull_request_event_refuses(self):
        self.github(run=run_object(event="pull_request"))
        self.refused("event")

    def test_a_failed_job_refuses(self):
        self.github(pages=jobs({"name": "verify-system-images", "conclusion": "failure"}))
        self.refused("verify-system-images")

    def test_a_failed_job_on_a_later_page_refuses(self):
        self.github(pages=jobs() + [{"jobs": [{"name": "dag-compile (x)", "conclusion": "cancelled"}]}])
        self.refused("dag-compile (x)")

    def test_a_skipped_image_job_refuses(self):
        self.github(pages=jobs(image="skipped"))
        self.refused("image job")

    def test_a_changed_file_refuses(self):
        for name in ("image-digests.txt", "kernel-digests.txt", "kernel-unsigned.txt"):
            with self.subTest(name=name):
                self.github(files={name: "tampered\n"})
                self.refused(name)

    def test_a_malformed_request_refuses(self):
        self.github(req={**request(), "extra": 1})
        self.refused("extra")

    def test_a_missing_artifact_refuses(self):
        self.github()
        fx = json.loads((self.dir / "registry.json").read_text())
        del fx["run_artifacts"]["412"]["kernel-digests"]
        self.registry(fx)
        self.refused("kernel-digests")

    # The order check (PL58)

    def test_no_latest_passes(self):
        self.github(latest=False)
        self.assertEqual(self.check().returncode, 0)
        out = json.loads((self.dir / "out/release-candidate.json").read_text())
        self.assertEqual([o["verdict"] for o in out["order"]], ["absent", "absent"])
        self.assertEqual(out["superseded"], [])

    def test_a_registry_error_is_not_a_missing_latest(self):
        self.github(latest=False, extra={"errors": [f"{REG}/athanor-system:latest"]})
        self.refused("athanor-system")

    def test_latest_at_the_candidate_is_a_no_op(self):
        self.github()
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["tags"][f"{REG}/athanor-system:latest"] = digest(1)
        self.registry(fx)
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        out = json.loads((self.dir / "out/release-candidate.json").read_text())
        self.assertEqual(out["order"][0]["verdict"], "equal")

    def test_an_older_candidate_refuses(self):
        self.github(labels={"org.opencontainers.image.created": "2026-10-12T00:00:00Z"})
        self.refused("created")

    def test_a_candidate_of_the_same_second_refuses(self):
        self.github(labels={"org.opencontainers.image.created": "2026-10-11T04:30:00Z"})
        self.refused("created")

    def test_a_latest_commit_on_iso_v0_but_not_an_ancestor_refuses(self):
        self.github(compare={f"{OLD}...{SHA}": "diverged"})
        self.refused("ancestor")

    def test_a_latest_commit_absent_from_the_repository_refuses(self):
        self.github()
        fx = json.loads((self.dir / "registry.json").read_text())
        del fx["api"][f"/repos/{R}/compare/{OLD}...{TIP}"]
        self.registry(fx)
        self.refused(OLD)

    def test_a_latest_built_from_main_is_ordered_by_time_alone(self):
        self.github(compare={f"{OLD}...{TIP}": "diverged", f"{OLD}...{SHA}": "diverged"})
        self.assertEqual(self.check().returncode, 0)

    def test_an_unlabelled_latest_is_ordered_by_time_alone(self):
        self.github(labels={"org.opencontainers.image.revision": ""})
        self.assertEqual(self.check().returncode, 0)

    def test_order_again_refuses_once_a_newer_release_moved_latest(self):
        self.github()
        self.assertEqual(self.check().returncode, 0)
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["tags"][f"{REG}/athanor-system:latest"] = digest(14)
        fx["configs"][f"{REG}/athanor-system@{digest(14)}"] = {"org.opencontainers.image.created": "2026-10-12T00:00:00Z",
                                                               "org.opencontainers.image.revision": SHA}
        fx["api"][f"/repos/{R}/compare/{SHA}...{SHA}"] = {"status": "identical"}
        self.registry(fx)
        r = self.order()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("created", r.stderr)

    def test_order_again_passes_when_latest_is_already_the_candidate(self):
        self.github()
        self.assertEqual(self.check().returncode, 0)
        fx = json.loads((self.dir / "registry.json").read_text())
        for i, name in enumerate(NAMES):
            fx["tags"][f"{REG}/{name}:latest"] = digest(i + 1)
        self.registry(fx)
        self.assertEqual(self.order().returncode, 0)
```

Before Step 2, check two fake behaviours this test relies on, and widen `system/tests/fake_registry.py` only if one is missing:
- `skopeo inspect --format '{{.Digest}}' docker://X:latest` prints `tags[X:latest]`, or fails with "manifest unknown" when absent. It does, at line 115.
- `gh api ... --paginate --slurp /path` prints `json.dumps(fx["api"][path])`. It does, because only `--jq` changes the output.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/ci/tests -p test_release_candidate.py -v`
Expected: every test FAILs, because the script does not exist.

- [ ] **Step 3: Implement**

`scripts/ci/schemas/release-candidate.schema.json` embeds the request schema by value. The subset has no `$ref`, so the `request` property repeats the object of `release-request.schema.json`. Write it as:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "release-candidate.json (docs/architecture/doc_pipeline.md, PL55 and PL58); request is release-request.schema.json",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema_version", "request", "order", "superseded"],
  "properties": {
    "schema_version": {"const": 1},
    "request": {"type": "object"},
    "order": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["repository", "candidate_digest", "latest_digest", "latest_created", "verdict"],
        "properties": {
          "repository": {"type": "string", "pattern": "^[a-z0-9.-]+(/[a-z0-9._-]+)+$"},
          "candidate_digest": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"},
          "latest_digest": {"type": ["string", "null"], "pattern": "^sha256:[0-9a-f]{64}$"},
          "latest_created": {"type": ["string", "null"], "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$"},
          "verdict": {"enum": ["absent", "equal", "newer"]}
        }
      }
    },
    "superseded": {"type": "array", "items": {"type": "integer", "minimum": 1}}
  }
}
```

`release_candidate.py` validates `request` against `release-request` separately, so `{"type": "object"}` is enough there.

`scripts/ci/release_candidate.py`:

```python
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

import schema

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD_WORKFLOW = ".github/workflows/athanor-forge-orchestrator.yml"
BRANCH = "iso-v0"
RELEASE_REF = f"refs/heads/{BRANCH}"
REQUEST_JOB = "request-release"
# The job of call-system-image.yml that builds and pushes the images (PB12 renames it).
IMAGE_JOB = "system-image / 💿 System Image & Disk Build"
ARTIFACTS = {"release-request": ["release-request.json"], "image-digests": ["image-digests.txt"],
             "kernel-digests": ["kernel-digests.txt", "kernel-unsigned.txt"]}
FILES = ["image-digests.txt", "kernel-digests.txt", "kernel-unsigned.txt"]
COMMIT = re.compile(r"^[0-9a-f]{40}$")
CREATED = "org.opencontainers.image.created"
REVISION = "org.opencontainers.image.revision"


class Refused(Exception):
    pass


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def tool(*args):
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode:
        raise Refused(f"{' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def api(path, *flags):
    return json.loads(tool("gh", "api", *flags, path))


def compare(repo, base, head):
    """identical or ahead: base is an ancestor of head. Commit ids only, so no tag can stand in."""
    return tool("gh", "api", f"/repos/{repo}/compare/{base}...{head}", "--jq", ".status").strip()


def labels(ref):
    return json.loads(tool("skopeo", "inspect", "--config", f"docker://{ref}"))["config"].get("Labels") or {}


def when(text, what):
    if not isinstance(text, str) or not text:
        raise Refused(f"{what}: no {CREATED} label")
    return datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))


def order(repo, digests, commit):
    """PL58, per repository of the digests file. Every problem is collected, then refused."""
    tip = tool("gh", "api", f"/repos/{repo}/branches/{BRANCH}", "--jq", ".commit.sha").strip()
    if not COMMIT.match(tip):
        raise Refused(f"branch {BRANCH} has no commit id ('{tip}')")
    verdicts, why = [], []
    for line in digests.read_text().splitlines():
        repository, _tag, candidate = line.split()
        entry = {"repository": repository, "candidate_digest": candidate, "latest_digest": None,
                 "latest_created": None, "verdict": "absent"}
        verdicts.append(entry)
        r = subprocess.run(["skopeo", "inspect", "--format", "{{.Digest}}", f"docker://{repository}:latest"],
                           capture_output=True, text=True)
        if r.returncode:
            if "manifest unknown" not in r.stderr:
                why.append(f"{repository}:latest cannot be read: {r.stderr.strip()}")
            continue
        entry["latest_digest"] = latest = r.stdout.strip()
        if latest == candidate:
            entry["verdict"] = "equal"
            continue
        try:
            old, new = labels(f"{repository}@{latest}"), labels(f"{repository}@{candidate}")
            entry["latest_created"] = old.get(CREATED)
            if not when(new.get(CREATED), candidate) > when(old.get(CREATED), latest):
                why.append(f"{repository}: candidate created {new.get(CREATED)} is not later than :latest {old.get(CREATED)}")
            revision = old.get(REVISION) or ""
            if revision:
                if not COMMIT.match(revision):
                    why.append(f"{repository}: :latest names revision '{revision}', not a commit")
                elif compare(repo, revision, tip) in ("identical", "ahead") \
                        and compare(repo, revision, commit) not in ("identical", "ahead"):
                    why.append(f"{repository}: :latest commit {revision} is not an ancestor of {commit}")
            # A :latest from main, or unlabelled, is ordered by build time alone (PL58).
        except Refused as error:
            why.append(f"{repository}: {error}")
        entry["verdict"] = "newer"
    if why:
        raise Refused("; ".join(why))
    return verdicts


def superseded(repo, run, verdicts):
    """Successful builds of iso-v0 between :latest and the candidate (PL55), for the record.
    ponytail: ordered by time from the oldest :latest a release moves; a build that main's old
    jobs or PB13's interim old jobs released also shows. Exact once markers exist (PB4b)."""
    times = sorted(v["latest_created"] for v in verdicts if v["verdict"] == "newer" and v["latest_created"])
    if not times:
        return []
    path = (f"/repos/{repo}/actions/workflows/{pathlib.Path(BUILD_WORKFLOW).name}/runs"
            f"?branch={BRANCH}&status=success&created=>={times[0]}&per_page=100")
    runs = [r for page in api(path, "--paginate", "--slurp") for r in page["workflow_runs"]]
    return sorted(r["id"] for r in runs if r["id"] != run["id"] and r["created_at"] < run["created_at"])


def check(out):
    run_id = os.environ.get("BUILD_RUN_ID", "")
    if not re.fullmatch(r"[0-9]+", run_id):
        raise Refused(f"build_run_id '{run_id}' is not a run id")
    ref = os.environ.get("GITHUB_REF", "")
    if ref != RELEASE_REF:
        raise Refused(f"release.yml releases only from {RELEASE_REF}, not {ref}")
    repo = os.environ["GITHUB_REPOSITORY"]
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        for name in ARTIFACTS:
            tool("gh", "run", "download", run_id, "-n", name, "-D", str(tmp))
        request = json.loads((tmp / "release-request.json").read_text())
        try:
            schema.validate(request, schema.load("release-request"))
        except ValueError as error:
            raise Refused(f"release-request.json: {error}") from None
        run = api(f"/repos/{repo}/actions/runs/{run_id}")
        why = []
        if request["build_run_id"] != int(run_id):
            why.append(f"request names run id {request['build_run_id']}, not {run_id}")
        if request["workflow_path"] != BUILD_WORKFLOW:
            why.append(f"request names workflow {request['workflow_path']}")
        if request["build_run_attempt"] != run["run_attempt"]:
            why.append(f"request names attempt {request['build_run_attempt']}, the run is at {run['run_attempt']}")
        if request["ref"] != RELEASE_REF:
            why.append(f"request names ref {request['ref']}")
        if request["commit"] != run["head_sha"]:
            why.append(f"request names commit {request['commit']}, the run built {run['head_sha']}")
        for name in FILES:
            if sha256(tmp / name) != request["sha256"][name]:
                why.append(f"{name} differs from the hash of the request")
        pages = api(f"/repos/{repo}/actions/runs/{run_id}/attempts/{run['run_attempt']}/jobs?per_page=100",
                    "--paginate", "--slurp")
        jobs = [j for page in pages for j in page["jobs"]]
        why += [f"job {j['name']} {j['conclusion']}" for j in jobs
                if j["name"] != REQUEST_JOB and j["conclusion"] not in ("success", "skipped")]
        if [j["conclusion"] for j in jobs if j["name"] == IMAGE_JOB] != ["success"]:
            why.append(f"image job '{IMAGE_JOB}' did not succeed once")
        trusted = subprocess.run(["bash", str(ROOT / "system/trusted-run.sh"), "--in-progress", run_id, BUILD_WORKFLOW],
                                 capture_output=True, text=True, env={**os.environ, "RELEASE_BRANCH": BRANCH})
        if trusted.returncode:
            why.append(trusted.stderr.strip() or f"trusted-run.sh exited {trusted.returncode}")
        if why:
            raise Refused("; ".join(why))
        out.mkdir(parents=True, exist_ok=True)
        for name in FILES:
            shutil.copy(tmp / name, out / name)
    verdicts = order(repo, out / "image-digests.txt", request["commit"])
    candidate = {"schema_version": 1, "request": request, "order": verdicts,
                 "superseded": superseded(repo, run, verdicts)}
    schema.validate(candidate, schema.load("release-candidate"))
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
                raise Refused(f"release.yml releases only from {RELEASE_REF}")
            candidate = json.loads((a.dir / "release-candidate.json").read_text())
            result = order(os.environ["GITHUB_REPOSITORY"], a.dir / "image-digests.txt", candidate["request"]["commit"])
    except Refused as error:
        print(f"release_candidate.py {a.cmd}: refused: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

A failed `check` must leave no `release-candidate.json`. The early `raise` paths write nothing, and the file is written last.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/ci/tests -v`
Expected: PASS. Then run the PB13 gate on its own: `python3 -B -m unittest discover -s scripts/ci/tests -p test_release_candidate.py -v`.

- [ ] **Step 5: Commit**

```bash
git add scripts/ci/release_candidate.py scripts/ci/schemas/release-candidate.schema.json \
  scripts/ci/tests/test_release_candidate.py
git commit -m "feat(release): check the release candidate against the build run and :latest (PB13, PL55, PL58)"
```

---

### Task 5: `request-release` in the Orchestrator (no dispatch)

**Files:**
- Modify: `.github/workflows/athanor-forge-orchestrator.yml` (append after `tag-system-images`)
- Test: `python3 scripts/verify.py workflows`, `actionlint`

**Interfaces:**
- Consumes: `release_request.py` (Task 3); the artifacts `image-digests` and `kernel-digests`; `needs.build-builder.outputs.content_hash`.
- Produces: the artifact `release-request` of every build run whose other jobs all succeeded or were skipped and whose `system-image` succeeded.

- [ ] **Step 1: Confirm the job list**

Run: `grep -n -E '^  [a-z-]+:$' .github/workflows/athanor-forge-orchestrator.yml`
Expected ids: lint, orchestrator-brain, build-builder, kernel-artifacts, nvidia-kmod, nvidia-kmod-sign, nvidia-kmod-publish, kernel-artifacts-final, kernel-digests, dag-compile, system-image, sign-system-images, verify-system-images, tag-system-images. If the list differs, the `needs` and `if` below follow the file.

- [ ] **Step 2: Add the job**

```yaml
  request-release:
    # The last job of the build (doc_pipeline.md PL54, PL55): writes release-request.json, what
    # release.yml releases. It runs only when every other job succeeded or was skipped and the
    # image job succeeded, an explicit list: !cancelled() alone is true after a failure too.
    # PB13's first pull request: it dispatches nothing yet, and the maintainer dispatches
    # release.yml by hand; the second turns the dispatch on and removes the three jobs above.
    needs: [lint, orchestrator-brain, build-builder, kernel-artifacts, nvidia-kmod, nvidia-kmod-sign,
      nvidia-kmod-publish, kernel-artifacts-final, kernel-digests, dag-compile, system-image,
      sign-system-images, verify-system-images, tag-system-images]
    if: >-
      ${{ !cancelled() && needs.system-image.result == 'success'
      && contains(fromJSON('["success", "skipped"]'), needs.lint.result)
      && contains(fromJSON('["success", "skipped"]'), needs.orchestrator-brain.result)
      && contains(fromJSON('["success", "skipped"]'), needs.build-builder.result)
      && contains(fromJSON('["success", "skipped"]'), needs.kernel-artifacts.result)
      && contains(fromJSON('["success", "skipped"]'), needs.nvidia-kmod.result)
      && contains(fromJSON('["success", "skipped"]'), needs.nvidia-kmod-sign.result)
      && contains(fromJSON('["success", "skipped"]'), needs.nvidia-kmod-publish.result)
      && contains(fromJSON('["success", "skipped"]'), needs.kernel-artifacts-final.result)
      && contains(fromJSON('["success", "skipped"]'), needs.kernel-digests.result)
      && contains(fromJSON('["success", "skipped"]'), needs.dag-compile.result)
      && contains(fromJSON('["success", "skipped"]'), needs.sign-system-images.result)
      && contains(fromJSON('["success", "skipped"]'), needs.verify-system-images.result)
      && contains(fromJSON('["success", "skipped"]'), needs.tag-system-images.result) }}
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
        with:
          persist-credentials: false
      - name: 📎 Digests recorded by the build job
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: image-digests
          path: artifacts
      - name: 📎 Kernel artefacts of the release
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: kernel-digests
          path: kernel
      - name: 📨 Write the release request (scripts/ci/release_request.py)
        env:
          BUILDER_HASH: ${{ needs.build-builder.outputs.content_hash }}
        run: python3 -B scripts/ci/release_request.py --image-digests artifacts/image-digests.txt --kernel-digests kernel --out "${RUNNER_TEMP}/out"
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        with:
          name: release-request
          path: ${{ runner.temp }}/out/release-request.json
          if-no-files-found: error
```

- [ ] **Step 3: Check**

Run: `actionlint .github/workflows/athanor-forge-orchestrator.yml && python3 scripts/verify.py workflows`
Expected: both pass. The job holds no environment, so D43 does not apply to it.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/athanor-forge-orchestrator.yml
git commit -m "feat(release): the build writes its release request (PB13, PL54)"
```

---

### Task 6: `release.yml` (STOP-AND-ASK: signing job and D43 allow-list)

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `scripts/verify.py` (the `SIGN_JOB_COMMANDS` block, about line 198), with an exact-replacement script
- Test: `scripts/tests/test_verify_signing.py`

**Interfaces:**
- Consumes: `release_candidate.py check|order` (Task 4); `system/sign-images.sh`, `system/verify-images.sh`, `system/kernel-artifacts.sh verify-key`, `system/tag-images.sh` unchanged; `accept.yml` input `run_id`.
- Produces: `RELEASE_REF_CHECK` in `scripts/verify.py`, which PR 2's `verify.py workflows` rule will require in every job of `release.yml`.

- [ ] **Step 1: Write the failing test**

Add to `scripts/tests/test_verify_signing.py`, class `SigningTest`:

```python
    def test_the_release_ref_check_may_run_in_a_signing_job(self):
        job = SIGN_JOB.replace("      - uses: " + DOWNLOAD,
                               "      - run: " + json.dumps(verify.RELEASE_REF_CHECK) + "\n      - uses: " + DOWNLOAD, 1)
        self.assertEqual(self.problems({"k.yml": workflow(job)}), [])

    def test_a_ref_check_with_anything_appended_fails_in_a_signing_job(self):
        job = SIGN_JOB.replace("      - uses: " + DOWNLOAD,
                               "      - run: " + json.dumps(verify.RELEASE_REF_CHECK + "; curl x") + "\n      - uses: " + DOWNLOAD, 1)
        self.assert_one({"k.yml": workflow(job)}, "run")
```

`json.dumps` gives a double-quoted YAML scalar whose escapes YAML reads back unchanged.

- [ ] **Step 2: Run the test to verify it fails**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p test_verify_signing.py -v`
Expected: ERROR, because `verify` has no attribute `RELEASE_REF_CHECK`.

- [ ] **Step 3: Add the constant to `scripts/verify.py`**

With an exact-replacement script in the scratchpad, insert before `SIGN_JOB_COMMANDS = {`:

```python
# The ref check of every job of release.yml (doc_pipeline.md PL54): a release run on any ref but
# iso-v0 fails, never skips. A signing job may run it: it reads no secret and changes nothing.
RELEASE_REF_CHECK = ('[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || '
                     '{ echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }')
```

and add `    RELEASE_REF_CHECK,` as the last member of `SIGN_JOB_COMMANDS`. Check `git diff --numstat scripts/verify.py`: 5 lines added, 0 deleted.

- [ ] **Step 4: Write `.github/workflows/release.yml`**

The ref check is the same text in every job. In YAML it is written as a plain `run:` scalar whose value equals `RELEASE_REF_CHECK` exactly.

```yaml
name: Release
# Releases one build of iso-v0 to :latest (docs/architecture/doc_pipeline.md section 4.10,
# PL54 to PL59; ADR-0104). The build's request-release job will dispatch it with the build run
# id (PB13's second pull request); until then the maintainer dispatches it by hand:
#   gh workflow run release.yml --ref iso-v0 -f build_run_id=<run id>
# Every job checks the ref and fails on any other: release.yml releases only from iso-v0.
# build_run_id reaches scripts through env only, and candidate refuses one that is not a number.

on:
  workflow_dispatch:
    inputs:
      build_run_id:
        description: Run id of the athanor-forge-orchestrator.yml build to release
        required: true
        type: string

permissions: {}

# Releases queue behind each other and none is cancelled by a newer one (PL57).
concurrency:
  group: ${{ github.workflow }}
  cancel-in-progress: false

jobs:
  candidate:
    name: 🧾 Check the build and the order of :latest
    runs-on: ubuntu-24.04
    defaults:
      run:
        shell: bash
    timeout-minutes: 15
    permissions:
      contents: read
      actions: read
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
        with:
          persist-credentials: false
      - name: 🌿 Release only from iso-v0
        run: '[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || { echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }'
      - name: 🧾 Check the request against the build run (scripts/ci/release_candidate.py)
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
          BUILD_RUN_ID: ${{ inputs.build_run_id }}
        run: python3 -B scripts/ci/release_candidate.py check --out "${RUNNER_TEMP}/out" | tee -a "${GITHUB_STEP_SUMMARY}"
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        with:
          name: release-candidate
          path: ${{ runner.temp }}/out/
          if-no-files-found: error

  sign-images:
    name: 🔏 Key-based signature of the candidate
    # The only job of release.yml that receives COSIGN_* (D43). It runs the steps of the
    # Orchestrator's sign-system-images on the files of release-candidate, which candidate
    # checked against the build run: the images and the kernel artefacts without the project
    # key signature (ADR-0096). It builds nothing and verifies nothing: verify-images does.
    needs: [candidate]
    runs-on: ubuntu-24.04
    # Image key rotation (docs/operations/secrets.md section 4.1): `signing` until step 3,
    # then `signing-images` (PL56).
    environment: signing
    defaults:
      run:
        shell: bash
    timeout-minutes: 60
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
      - name: 🌿 Release only from iso-v0
        run: '[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || { echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }'
      - name: 📎 The checked candidate
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: release-candidate
          path: artifacts
      - name: 🔑 Login to the registry
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          GITHUB_ACTOR: ${{ github.actor }}
        run: echo "${GITHUB_TOKEN}" | skopeo login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
      - name: 🔏 Sign by digest (system/sign-images.sh)
        env:
          COSIGN_PRIVATE_KEY: ${{ secrets.COSIGN_PRIVATE_KEY }}
          COSIGN_PASSWORD: ${{ secrets.COSIGN_PASSWORD }}
          KERNEL_REGISTRY: ${{ vars.KERNEL_REGISTRY }}
        run: bash system/sign-images.sh --registry "ghcr.io/${GITHUB_REPOSITORY_OWNER,,}" --kernel-digests artifacts/kernel-unsigned.txt artifacts/image-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"

  verify-images:
    name: 🔎 Verify the signatures as a machine does
    # No environment, no secret, no login (PL16, D43). It runs also when sign-images was
    # skipped or failed: an unsigned image fails the verification, so the run ends red.
    needs: [candidate, sign-images]
    if: ${{ !cancelled() && needs.candidate.result == 'success' }}
    runs-on: ubuntu-24.04
    defaults:
      run:
        shell: bash
    timeout-minutes: 60
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
        with:
          persist-credentials: false
      - name: 🌿 Release only from iso-v0
        run: '[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || { echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }'
      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6
      - name: 📎 The checked candidate
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: release-candidate
          path: artifacts
      - name: 🔎 Pull each image through the shipped policy (system/verify-images.sh)
        env:
          BUILD_RUN_ID: ${{ inputs.build_run_id }}
        run: |
          set -euo pipefail
          builder=$(jq -r .request.builder_content_hash artifacts/release-candidate.json)
          bash system/verify-images.sh --registry "ghcr.io/${GITHUB_REPOSITORY_OWNER,,}" --builder "${builder}" \
            --evidence evidence --build-run-id "${BUILD_RUN_ID}" artifacts/image-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"
      - name: 🔎 Kernel artefacts carry the project key signature (ADR-0096)
        run: bash system/kernel-artifacts.sh verify-key artifacts/kernel-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"
      - name: 📎 Hand the signature evidence to the promotion, named by the build run (PL56)
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        with:
          name: evidence-${{ inputs.build_run_id }}-signature
          path: evidence/
          if-no-files-found: error

  tag-latest:
    name: 🏷️ Move :latest to the verified candidate
    # PL58 again, just before the tag moves: the run may have waited for its approval, or be a
    # re-run after a newer release moved :latest. No key; the token writes the tags.
    needs: [verify-images]
    if: ${{ !cancelled() && needs.verify-images.result == 'success' }}
    runs-on: ubuntu-24.04
    defaults:
      run:
        shell: bash
    timeout-minutes: 30
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
        with:
          persist-credentials: false
      - name: 🌿 Release only from iso-v0
        run: '[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || { echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }'
      - name: 📎 The checked candidate
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: release-candidate
          path: artifacts
      - name: ⏱️ :latest never moves backwards (scripts/ci/release_candidate.py order)
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
        run: python3 -B scripts/ci/release_candidate.py order --dir artifacts | tee -a "${GITHUB_STEP_SUMMARY}"
      - name: 🔑 Login to the registry
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          GITHUB_ACTOR: ${{ github.actor }}
        run: echo "${GITHUB_TOKEN}" | skopeo login ghcr.io -u "${GITHUB_ACTOR}" --password-stdin
      - name: 🏷️ Point :latest at the verified digests (system/tag-images.sh)
        run: bash system/tag-images.sh --registry "ghcr.io/${GITHUB_REPOSITORY_OWNER,,}" --tag latest artifacts/image-digests.txt | tee -a "${GITHUB_STEP_SUMMARY}"

  request-acceptance:
    name: 🧪 Dispatch the acceptance of the build
    # accept.yml runs outside the release group, and its evidence feeds UD4 (PL56, UD17, UD18).
    needs: [tag-latest]
    runs-on: ubuntu-24.04
    defaults:
      run:
        shell: bash
    timeout-minutes: 5
    permissions:
      actions: write
    steps:
      - name: 🌿 Release only from iso-v0
        run: '[[ "${GITHUB_REF}" == refs/heads/iso-v0 ]] || { echo "::error::release.yml releases only from refs/heads/iso-v0, not ${GITHUB_REF}"; exit 1; }'
      - name: 🧪 Dispatch accept.yml with the build run id
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
          BUILD_RUN_ID: ${{ inputs.build_run_id }}
        run: gh workflow run accept.yml --ref iso-v0 -f run_id="${BUILD_RUN_ID}"
```

Before the commit, confirm three facts at the source:
- `accept.yml` declares `run_id` as its only required input: `grep -n -A6 'workflow_dispatch' .github/workflows/accept.yml`.
- `system/fetch-evidence.sh` already trusts `release.yml` for the `signature` gate: `grep -n 'release.yml' system/fetch-evidence.sh`.
- `system/tag-images.sh` accepts `GITHUB_REF=refs/heads/iso-v0`: lines 25 and 45.

- [ ] **Step 5: Check**

Run:
```
node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p 'test_verify_*.py' -v
actionlint .github/workflows/release.yml
python3 scripts/verify.py workflows
python3 -c "import yaml,sys; sys.path.insert(0,'scripts'); import importlib.util as u; s=u.spec_from_file_location('v','scripts/verify.py'); v=u.module_from_spec(s); s.loader.exec_module(v); d=yaml.safe_load(open('.github/workflows/release.yml')); runs=[st['run'] for j in d['jobs'].values() for st in j['steps'] if 'run' in st and 'GITHUB_REF' in st['run']]; assert len(runs)==5 and all(r==v.RELEASE_REF_CHECK for r in runs), runs; print('ref check in 5 jobs, identical to RELEASE_REF_CHECK')"
```
Expected: all pass. The D43 lint reads `release.yml` and finds no problem in `sign-images`.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/release.yml scripts/verify.py scripts/tests/test_verify_signing.py
git commit -m "feat(release): release.yml signs, verifies and tags a checked build (PB13, PL56)"
```

---

### Task 7: The documents follow the code

**Files** (each changed with an exact-replacement script in the scratchpad; check `git diff --numstat` shows 0 lines deleted unless a line is replaced):
- `docs/architecture/doc_ci.md`: the CI1 tree, the CI1 secrets and environment line, a new entry `### CI33 Release` after CI32, the release path line.
- `docs/architecture/doc_update_delivery.md`: a note at UD25.
- `docs/operations/secrets.md`: SEC3 and ENV6 "used by" cells.

- [ ] **Step 1: Write the replacements**

`doc_ci.md`:
- In the CI1 tree, the last line ` `- tag-system-images ...` becomes ` |- tag-system-images ...` (same text after the marker), followed by the new last line:
  ` `- request-release ................. scripts/ci/release_request.py -> artifact release-request (PB13; dispatches CI33 from PB13's second pull request)`
- Release path line: replace `CI1 publishes :<run_id> and :latest -> CI12 accept.yml (dispatched with the run id)` with `CI1 publishes :<run_id> and :latest (until PB13's second pull request) and writes release-request -> CI33 release.yml (by hand until then) signs, verifies and moves :latest -> CI12 accept.yml (dispatched with the run id)`.
- After the CI32 block, before `## 3. Known broken workflows`, add:

```markdown
### CI33 Release

- **File:** `release.yml`. **Purpose:** releases one build of `iso-v0` to `:latest` (doc_pipeline.md section 4.10, PL54 to PL59): checks the build's `release-request` against the API's record of the run and the order of `:latest`, signs the images and the kernel artefacts with the project key, verifies them as a machine does, moves `:latest`, and dispatches CI12. Every job fails on a ref other than `refs/heads/iso-v0`.
- **Jobs:** `candidate`, `sign-images` (environment `signing`, the alias of `signing-images` during the image key rotation), `verify-images`, `tag-latest`, `request-acceptance`. **Triggers:** dispatch (`build_run_id`), by hand until PB13's second pull request turns on the dispatch of CI1's `request-release`. **Output:** artifacts `release-candidate` and `evidence-<build_run_id>-signature`, the tag `latest` of the system images, a run of CI12.
- **Secrets, variables:** `GITHUB_TOKEN`, `COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD`, `KERNEL_REGISTRY`. **Environment:** `signing` (job `sign-images`). **Runner:** hosted. **Concurrency:** `Release`, no cancel (PL57).
- **Scripts:** `scripts/ci/release_candidate.py`, `scripts/ci/schema.py`, `system/trusted-run.sh`, `system/sign-images.sh`, `system/verify-images.sh`, `system/kernel-artifacts.sh`, `system/tag-images.sh`.
- **Health:** `gh run list --workflow release.yml --branch iso-v0 --limit 5`.
```

`doc_update_delivery.md`: after the UD25 line `**UD25. Sign, verify, then tag.** _(Landed, #299.)_ The order of a release run becomes:`, add one paragraph before item 1:
`PB13 moves these steps into `release.yml` (doc_pipeline.md PL56: `sign-images`, `verify-images`, `tag-latest`), in the same order, after a `candidate` job that checks the build; the Orchestrator's jobs below leave with PB13's second pull request.`

`secrets.md`: in the SEC3 row, append `, `release.yml` `sign-images` (PB13)` to the "used by" cell after the orchestrator mention. Do the same in the ENV6 row's "used by" cell. Read both rows before writing the replacement, and replace the exact cell text.

- [ ] **Step 2: Check**

Run: `python3 scripts/verify.py docs ci decisions` and `git diff --numstat -- docs/`
Expected: 3/3 pass, and only the intended lines change.

- [ ] **Step 3: Commit**

```bash
git add docs/architecture/doc_ci.md docs/architecture/doc_update_delivery.md docs/operations/secrets.md
git commit -m "docs(ci): catalogue release.yml and the release request (PB13)"
```

---

### Task 8: Whole-branch check and pull request

- [ ] `just check`. Every check must be green, or red only on a finding listed in `scripts/ci/known-red.txt`.
- [ ] Run the `auditor` subagent on the branch diff against `origin/iso-v0`. Fix MINOR and BLOCKING findings and audit again.
- [ ] Push and open the pull request as `athanor-agent` against `iso-v0`. The body covers what changes, why (PB13 PR 1 of doc_pipeline.md rev 3), how it was verified, and the maintainer's step after the merge:
  - wait for the first Orchestrator run on `iso-v0` after the merge to finish green (its old jobs move `:latest`);
  - then run `gh workflow run release.yml -R ars-regia/athanor --ref iso-v0 -f build_run_id=<that run id>`;
  - approve `sign-images` in `signing`;
  - expect `tag-latest` to report `equal` for every repository, and one more run of `accept.yml`.
- [ ] Merge only by the CLAUDE.md merge rule (card, `hr-mes` approval at the card sha).

## Self-Review

- **Spec coverage.**
  - PL54: ref check in every job (Task 6), the numeric input (Task 4), passed through `env` (Task 6), `request-release` with an explicit list (Task 5), no dispatch (PR 1 scope).
  - PL55: request fields and schema (Task 3); candidate checks, the in-progress mode, hashes, jobs and `superseded` (Tasks 2 and 4); `release-candidate` with the three files (Tasks 4 and 6).
  - PL56: the five PB13 jobs (Task 6); `mark-current` is PB4b.
  - PL57: the concurrency group (Task 6).
  - PL58: the order check in `candidate` and `tag-latest` (Tasks 4 and 6); the revision label and its image check (Task 1).
  - PB13 scope: environments.json needs no change, since `signing` already allows `iso-v0` and the dispatch needs no environment. doc_ci, UD25 and secrets.md are in Task 7.
  - Left to PR 2: the dispatch, the removal of the old jobs, and the `verify.py workflows` rules.
- **Placeholder scan.** None. Task 7's `secrets.md` cells are read before replacement because the row text is long. The edit is exact.
- **Type consistency.**
  - `build_run_id` and `build_run_attempt` are ints in both schemas and in the code.
  - Verdicts are `absent`, `equal` and `newer` everywhere.
  - `RELEASE_REF_CHECK` is the same text in verify.py and release.yml, and Task 6 Step 5 asserts it.
- **Review Focus.** All five are pinned by tests, in Tasks 2 and 4.
