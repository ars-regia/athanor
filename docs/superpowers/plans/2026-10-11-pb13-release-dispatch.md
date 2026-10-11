# PB13 second pull request: the build dispatches its release

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the build's `request-release` job dispatches `release.yml` on `iso-v0`, the Orchestrator's `sign-system-images`, `verify-system-images` and `tag-system-images` jobs go, and `verify.py workflows` gains the rules of PL54, PL56, PL57 and PL60 that PB13's gate names.

**Architecture:** one new step at the end of `request-release` (`gh workflow run release.yml` with `GITHUB_TOKEN`, `actions: write`, on `iso-v0` only); three jobs deleted; `release_problems()` in `scripts/verify.py` grows from the two rules of the first pull request to the full list; one summary line in `release.yml`; one characterisation test of `system/tag-images.sh`; docs follow.

**Tech Stack:** GitHub Actions YAML, Python 3 stdlib plus PyYAML (already used by `verify.py`), bash.

**Spec:** `docs/architecture/doc_pipeline.md` revision 3 (#407), PL54 to PL60 and the PB13 row of the build plan; ADR-0104; ADR-0111; the first pull request's plan, `docs/superpowers/plans/2026-10-11-pb13-release-workflow.md`.

## Global Constraints

- Base: `origin/iso-v0` at `9a25e16f`, worktree `/var/tmp/athanor-wt/pb13-pr2`, branch `feat/pb13-release-dispatch`.
- PB13 row: "The second, after that run is green, removes the Orchestrator's `sign-system-images`, `verify-system-images` and `tag-system-images` jobs, turns the dispatch on and adds the `verify.py workflows` rules". The run was green: release run 38161770027.
- PL54: "On `iso-v0`, the default branch, it then dispatches `release.yml` on that ref with one input, `build_run_id`, after a push and after the nightly schedule (04:00 UTC) alike (PQ18). On `main` it dispatches nothing". "That job and `request-acceptance` (PL56) alone have `actions: write`". "A failed dispatch fails the build."
- PL56: "`verify.py workflows` rejects a job of `release.yml` that does not check that the run's ref is `refs/heads/iso-v0` (PL54), a job in `signing` or `signing-images` outside `release.yml`, a second writer of `:latest`, and a job that runs `system/promote.sh` or writes `:stable` outside the environment `release` (PL60)."
- PL57: `build.yml` (today the Orchestrator) keeps `${{ github.workflow }}-${{ github.ref }}`, `release.yml` keeps `${{ github.workflow }}`, both with `cancel-in-progress: false`; "`verify.py workflows` checks both declarations".
- `main` is untouched: its own copy of the Orchestrator keeps the old jobs, the only path of a hand release of `main` and of the ISO's `:latest` (PL54). `.github/settings/environments.json` does not change: `signing` still serves `release.yml` and `main`'s old jobs.
- **Stop and ask (AGENTS.md):** Task 1 deletes `sign-system-images`, a job of the `signing` environment, and Task 2 adds a lint rule over the signing environments. The maintainer's approval of this plan covers exactly the text below. `release.yml`'s `sign-images` job is not touched.
- No `|| true`, no `2>/dev/null` that hides an error, English in everything committed, Conventional Commits, no attribution.
- `scripts/verify.py` and Markdown under `docs/` are edited with exact-replacement Python scripts written in the scratchpad, then `git diff --numstat` shows no stray deletions.
- Tests run through `node /home/hr-mes/.claude/bin/cc-test.mjs -- <command>`, except `python3 -B` unittest runs, which cc-test refuses and which run directly.

## Review Focus

1. **The artifacts of a build run still `in_progress`.** From this pull request on, `candidate` downloads `release-request`, `image-digests` and `kernel-digests` while `request-release` may still be running. Artifacts v4 are listed by the API once uploaded; this is measured before the merge (Task 5, step 1) on the nightly build, by listing the artifacts of the run while it is `in_progress`. If they are not visible, the release run fails in `candidate` and `:latest` stays where it is; the plan stops and comes back to the maintainer.
2. **A dispatch made with `GITHUB_TOKEN` starts a run.** Documented by GitHub as the exception to the rule that the token's events start nothing. Observed on the first push after the merge (post-merge gate).
3. **A build re-run.** A re-run of the failed jobs of a build runs `request-release` again and dispatches again with the same run id; the request then carries the new run attempt, which `candidate` compares with the API (first pull request, `test_release_candidate.py`). Nothing new to test.
4. **A dispatch off `iso-v0`.** A push to `main` runs `main`'s own file. A `workflow_dispatch` of the Orchestrator from a feature branch must dispatch nothing: the step's `if` is pinned by the lint (Task 2, `test_a_dispatch_off_iso_v0_fails`), and `release.yml` itself fails on any other ref.
5. **A `tag-latest` stopped after its first repository.** Its re-run must complete the others (PB13 gate). Pinned in Task 4.

---

### Task 1: `request-release` dispatches, the three old jobs go

**Files:**
- Modify: `.github/workflows/athanor-forge-orchestrator.yml` (delete lines 332-474; `request-release` at 475-529)
- Modify: `.github/workflows/release.yml:2-7` (header comment)
- Modify: `scripts/tests/test_verify_release.py` (the needs fixture)

**Interfaces:**
- Produces: in `request-release`, a last step whose `if` is exactly `github.ref == 'refs/heads/iso-v0'` and whose `run` is exactly `gh workflow run release.yml --ref iso-v0 -f build_run_id="${GITHUB_RUN_ID}"`. Task 2 pins both strings.

- [ ] **Step 1:** delete the jobs `sign-system-images`, `verify-system-images` and `tag-system-images` with their comments.
- [ ] **Step 2:** in `request-release`, replace the comment line pair "PB13's first pull request: it dispatches nothing yet, and the maintainer dispatches / release.yml by hand; the second turns the dispatch on and removes the three jobs above." with:

```yaml
    # On iso-v0 it then dispatches release.yml with this run's id (PL54); on any other ref it
    # dispatches nothing: main is frozen, and release.yml fails on any ref but iso-v0.
```

  shrink `needs` to `[lint, orchestrator-brain, build-builder, kernel-artifacts, nvidia-kmod, nvidia-kmod-sign, nvidia-kmod-publish, kernel-artifacts-final, kernel-digests, dag-compile, system-image]` and drop the three `contains(...)` lines of the removed jobs from `if`.
- [ ] **Step 3:** give `request-release` the dispatch permission and the step:

```yaml
    permissions:
      contents: read
      # The dispatch of release.yml (PL54). It could dispatch any workflow of the repository;
      # the candidate check of PL55 bounds what a dispatch gets signed.
      actions: write
```

```yaml
      - name: 🚀 Dispatch the release of this build (release.yml)
        if: github.ref == 'refs/heads/iso-v0'
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
        run: gh workflow run release.yml --ref iso-v0 -f build_run_id="${GITHUB_RUN_ID}"
```

  It comes after the upload of `release-request`, so the artifact exists before the release starts. `GITHUB_RUN_ID` comes from the runner's environment, never from an expression in `run:`.
- [ ] **Step 4:** in `release.yml`, replace header lines 3-5 with: "The build's request-release job dispatches it on iso-v0 with the build run id; the maintainer can release a build again by hand: / `gh workflow run release.yml --ref iso-v0 -f build_run_id=<run id>`".
- [ ] **Step 5:** in `test_verify_release.py`, `test_request_release_without_a_need_fails` mutates `"dag-compile, system-image]"` into `"system-image]"` and expects `["athanor-forge-orchestrator.yml: request-release does not need dag-compile"]`.
- [ ] **Step 6:** `actionlint .github/workflows/athanor-forge-orchestrator.yml .github/workflows/release.yml`; `python3 scripts/verify.py workflows ci`; `python3 -B -m unittest discover -s scripts/tests -p 'test_verify_*.py'`. All green.
- [ ] **Step 7:** commit `feat(ci): the build dispatches its release, and the Orchestrator signs no image (PB13)`.

### Task 2: the `verify.py workflows` rules of PL54, PL56, PL57 and PL60

**Files:**
- Modify: `scripts/verify.py` (`release_problems`, constants beside `RELEASE_YML`)
- Test: `scripts/tests/test_verify_release.py`

**Interfaces:**
- Consumes: `triggers(doc)`, `job_environment(job)`, `read`, `yaml` of `verify.py`; Task 1's step strings.
- Produces: `RELEASE_DISPATCH`, `ON_ISO_V0`, `LATEST_WRITER`, `STABLE_WRITER`; `release_problems(root) -> list[str]`, same signature.

- [ ] **Step 1: the failing tests.** Each mutates the copies that `tree()` makes and asserts the exact list:

| Test | Mutation | Expected problem |
| --- | --- | --- |
| `test_a_trigger_beside_workflow_dispatch_fails` | release: `"on:\n  workflow_dispatch:\n"` → `"on:\n  push:\n  workflow_dispatch:\n"` | `release.yml: starts on ['push', 'workflow_dispatch'], not on workflow_dispatch alone (PL54)` |
| `test_an_input_interpolated_into_run_fails` | release: `--out "${RUNNER_TEMP}/out" \|` → `--out "${RUNNER_TEMP}/out" --run ${{ inputs.build_run_id }} \|` | `release.yml: job candidate interpolates an input into run:, which only env may carry (PL54)` |
| `test_another_release_group_fails` | release: `"  group: ${{ github.workflow }}\n"` → `"  group: ${{ github.workflow }}-${{ github.ref }}\n"` | `release.yml: concurrency is not the group ${{ github.workflow }} without cancel-in-progress (PL57)` |
| `test_another_build_group_fails` | orchestrator: `"cancel-in-progress: false"` → `"cancel-in-progress: true"` (first) | `athanor-forge-orchestrator.yml: concurrency is not the group ${{ github.workflow }}-${{ github.ref }} without cancel-in-progress (PL57)` |
| `test_a_job_level_group_fails` | release: `"  candidate:\n"` → `"  candidate:\n    concurrency: other\n"` | `release.yml: job candidate declares a concurrency group of its own (PL57)` |
| `test_a_dispatch_off_iso_v0_fails` | orchestrator: `"if: github.ref == 'refs/heads/iso-v0'"` → `"if: always()"` | `athanor-forge-orchestrator.yml: request-release does not dispatch release.yml once, on iso-v0 alone, with its run id (PL54)` |
| `test_a_signing_job_outside_release_fails` (subtests `signing`, `Signing-Images`) | orchestrator: `"environment: signing-kernel"` → `"environment: <name>"` | `athanor-forge-orchestrator.yml: job nvidia-kmod-sign runs in <name lowercased>, which only release.yml may use (PL56)` |
| `test_a_second_writer_of_latest_fails` | orchestrator: append `"      - run: bash system/tag-images.sh --registry r --tag latest d.txt\n"` | `athanor-forge-orchestrator.yml: job request-release moves :latest of the system images, which only tag-latest of release.yml writes (PL35, PL56)` |
| `test_stable_outside_release_fails` | orchestrator: append `"      - run: bash system/promote.sh 412\n"` | `athanor-forge-orchestrator.yml: job request-release writes :stable outside the environment release (PL60)` |

- [ ] **Step 2:** run `python3 -B -m unittest scripts/tests/test_verify_release.py`; the nine new tests fail, the old ones pass.
- [ ] **Step 3: the rules.** Beside `ORCHESTRATOR_YML`:

```python
RELEASE_GROUP = "${{ github.workflow }}"
BUILD_GROUP = "${{ github.workflow }}-${{ github.ref }}"
ON_ISO_V0 = "github.ref == 'refs/heads/iso-v0'"
RELEASE_DISPATCH = 'gh workflow run release.yml --ref iso-v0 -f build_run_id="${GITHUB_RUN_ID}"'
INPUT_IN_RUN = re.compile(r"\$\{\{[^}]*inputs\.")
# system/tag-images.sh is the one script that moves a tag of the system images (PL35); a raw
# skopeo copy onto :latest is not recognised, and ADR-0111 item 5 accepts that the registry
# itself binds no workflow.
LATEST_WRITER = re.compile(r"system/tag-images\.sh\b[^\n]*--tag latest\b")
STABLE_WRITER = re.compile(r"system/promote\.sh\b|system/tag-images\.sh\b[^\n]*--tag stable")
```

  In `release_problems`, after the ref-check loop, with `release` the parsed `release.yml`, `orchestrator` the parsed Orchestrator (whose `jobs` the existing code already reads) and `request` its `request-release` job:

```python
    def group_problem(name, doc, group):
        if doc.get("concurrency") != {"group": group, "cancel-in-progress": False}:
            return [f"{name}: concurrency is not the group {group} without cancel-in-progress (PL57)"]
        return []

    if triggers(release) != {"workflow_dispatch"}:
        problems.append(f"release.yml: starts on {sorted(triggers(release))}, not on workflow_dispatch alone (PL54)")
    problems += group_problem("release.yml", release, RELEASE_GROUP)
    for job_id, job in release["jobs"].items():
        if "concurrency" in job:
            problems.append(f"release.yml: job {job_id} declares a concurrency group of its own (PL57)")
        if any(INPUT_IN_RUN.search(str(step.get("run", ""))) for step in job.get("steps", [])):
            problems.append(f"release.yml: job {job_id} interpolates an input into run:, which only env may carry (PL54)")
    problems += group_problem(Path(ORCHESTRATOR_YML).name, orchestrator, BUILD_GROUP)
    dispatches = [(step.get("if"), step.get("run")) for step in request.get("steps", [])
                  if "release.yml" in str(step.get("run", ""))]
    if dispatches != [(ON_ISO_V0, RELEASE_DISPATCH)]:
        problems.append(f"{Path(ORCHESTRATOR_YML).name}: request-release does not dispatch release.yml once, "
                        "on iso-v0 alone, with its run id (PL54)")
    for wf in sorted((root / ".github/workflows").glob("*.y*ml")):
        try:
            doc = yaml.safe_load(read(wf))
        except yaml.YAMLError:
            continue  # signing_problems reports a workflow it cannot read
        for job_id, job in (doc.get("jobs") or {}).items():
            environment = (job_environment(job) or "").lower()
            if environment in ("signing", "signing-images") and wf.name != "release.yml":
                problems.append(f"{wf.name}: job {job_id} runs in {environment}, which only release.yml may use (PL56)")
            for step in job.get("steps", []):
                run = str(step.get("run", ""))
                if LATEST_WRITER.search(run) and (wf.name, job_id) != ("release.yml", "tag-latest"):
                    problems.append(f"{wf.name}: job {job_id} moves :latest of the system images, "
                                    "which only tag-latest of release.yml writes (PL35, PL56)")
                if STABLE_WRITER.search(run) and environment != "release":
                    problems.append(f"{wf.name}: job {job_id} writes :stable outside the environment release (PL60)")
```

  The order of the appended problems matches the order the tests expect; the docstring of `release_problems` names the new rules.
- [ ] **Step 4:** the tests pass, and `test_the_repository_passes` still does on the real tree (`promote-stable.yml`'s `promote` runs in `release`).
- [ ] **Step 5:** commit `feat(verify): release.yml alone signs and tags the system images (PL54, PL56, PL57, PL60)`.

### Task 3: the release summary names who dispatched it

**Files:** Modify `.github/workflows/release.yml` (job `candidate`, after the ref check).

- [ ] **Step 1:** add, right after `🌿 Release only from iso-v0`:

```yaml
      - name: 👤 Who dispatched this release
        run: echo "Dispatched by ${GITHUB_TRIGGERING_ACTOR}." >> "${GITHUB_STEP_SUMMARY}"
```

  The ref check stays the first step after the checkout, which `release_problems` checks.
- [ ] **Step 2:** `actionlint`, `python3 scripts/verify.py workflows`; commit `feat(release): the summary names who dispatched the release (PB13)`.

### Task 4: a re-run of a partial `tag-latest` completes the others

**Files:** Test `system/tests/test_sign_images.py` (after `test_a_tag_the_registry_serves_late_is_read_again`).

A characterisation test: `tag-images.sh` copies by digest and is idempotent, so it passes on the first run; it pins the PB13 gate item.

- [ ] **Step 1:**

```python
    def test_a_rerun_after_a_partial_move_completes_the_others(self):
        """tag-latest stopped after its first repository: the re-run moves the others to the
        recorded digests and leaves the first where it is (PB13 gate)."""
        self.digests()
        recorded = {f"{REG}/{name}": self.tags[f"{REG}/{name}:412"] for name in NAMES}
        first, *rest = sorted(recorded)
        self.tags[f"{first}:latest"] = recorded[first]
        self.tags.update({f"{repository}:latest": "sha256:" + "0" * 64 for repository in rest})
        (self.state / "tags.json").write_text(json.dumps(self.tags))
        r = self.tag()
        self.assertEqual(r.returncode, 0, r.stderr)
        tags = json.loads((self.state / "tags.json").read_text())
        self.assertEqual({repository: tags[f"{repository}:latest"] for repository in recorded}, recorded)
```

- [ ] **Step 2:** `python3 -B -m unittest discover -s system/tests -p test_sign_images.py`; commit `test(release): a re-run of a partial tag-latest completes the move (PB13)`.

### Task 5: measure, docs, gate

- [ ] **Step 1: measure Review Focus 1.** While the nightly Orchestrator run of 2026-10-12 (04:00 UTC) is `in_progress` and after its `system-image` job ended: `gh api repos/ars-regia/athanor/actions/runs/<id>/artifacts --jq '.artifacts[].name'` lists `image-digests`, and `gh run download <id> -R ars-regia/athanor -n image-digests -D <scratch>` succeeds. If not, stop and report.
- [ ] **Step 2: docs**, by exact-replacement script, describing the state after this pull request:
  - `doc_ci.md`: the CI1 tree (lines 31-33 go, `request-release` dispatches); the image rows of the registry table (64-65: `tag-latest` of CI33 writes `latest`; the ISO's `latest` on `main` only, through `main`'s old jobs); 73, 104-106, 136, 139 (signing in CI33 `sign-images`); the environment table 422-423 (`signing` and `signing-images`: CI33 `sign-images`); 431 (the takeover is done).
  - `doc_update_delivery.md` 181-185 (UD25 steps name `release.yml`'s jobs), `doc_update_trust.md` 44, `doc_system_image.md` 186, `doc_build_ordering.md` 150.
  - `docs/operations/secrets.md` 25, 64, 66, 171-174, 233, 246, 253 and `docs/operations/github-settings.md` 124-125: `release.yml` `sign-images` in place of the Orchestrator job; `main`'s old jobs named where they still matter.
  - ADRs and past plans keep their text.
- [ ] **Step 3:** `git diff --numstat` shows no stray deletions in the docs; `python3 scripts/verify.py docs decisions`.
- [ ] **Step 4:** `just check` in the background with `CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/var/tmp/athanor-pb13-target nice -n 10`; green.
- [ ] **Step 5:** commit `docs(ci): the release path runs through release.yml alone (PB13)`; auditor at the head; pull request; card.

## After the merge (maintainer)

The merge is itself a push to `iso-v0` that touches the Orchestrator, so its build is the first automatic release: approve the `signing` deployment when the release run asks. Then, from the PB13 gate:

- `gh run view <build run> -R ars-regia/athanor --json jobs --jq '.jobs[].name'` lists no image signing, verification or tagging job, and the release run's summary names its triggering actor;
- each `:latest` of the build's `image-digests.txt` equals its digest, and `system/fetch-evidence.sh <build run>` finds the `signature` evidence;
- the nightly build of 2026-10-13 dispatches a release run as well.

Gate items left to later runs, recorded on the PB13 issue: a second build completing while a release waits for its approval; a dispatch of an older build failing in `candidate`; a rejected signing approval leaving `:latest` unchanged.
