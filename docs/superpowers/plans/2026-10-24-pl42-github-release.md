# GitHub Release of Each Promotion (PL42) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Requires ADR-0131.** The maintainer's decisions of 2026-10-24 on this plan's open
> questions are recorded in ADR-0131 (a separate pull request); no task starts before it is
> merged. This plan adds a job with `contents: write` to `promote-stable.yml`, whose jobs
> `promote` and `iso` run in the `release` environment, and changes the promotion record that
> `system/promote.sh` compares against the signed plan. AGENTS.md lists `system/promote.sh`
> under "Stop and ask before editing", so Tasks 1, 2 and 4 still show their diff before the push.
> Task 3 changes repository settings, which the maintainer applies (**[SETTINGS]**).
> Every step that touches signing, an environment, a permission or the trust chain is marked
> **[TRUST]**.

**Goal:** Each promotion to `:stable` publishes a GitHub Release named after the UT9 version of
the promoted build. The Release carries the signed evidence bundle and the keyless signature of
its `bundle.sha256`, and a person can verify it with `cosign verify-blob` against the identity of
`promote-stable.yml`. This is the GitHub half of PL42, which ADR-0106 item 7 placed in 0.5 and
ADR-0115 item 2 moved into 0.9. A promotion whose commit has the maintainer's signed `v0.x.y`
tag becomes that release, with its notes taken from `docs/releases/`. For 0.9 the notes say
that screenshot and screen share are absent (ADR-0129 item 3).

**Architecture:** The `plan` job of `promote-stable.yml` already builds the bundle, signs its
`bundle.sha256` without a key (`system/evidence-bundle.sh create`, `id-token: write`) and keeps it
as the artifact `promotion-<run_id>`. That signature is the one PL42 asks for. It is made before
the approval, and the approval covers it, so the Release job does not sign again. Three changes
are needed:

- the promotion record gains the UT9 version and the commit of the default image (Task 1);
- the `plan` job, with read access only, runs a new script, `system/github-release.sh plan`. It
  chooses the Release's tag, title and `latest` flag, and builds its notes. It fails the plan on
  a missing or wrong notes file or on a bad tag. It writes the choice and the notes' SHA-256 into
  `release.json`, which `evidence-bundle.sh create` adds to the bundle. The bundle's signed
  `bundle.sha256` therefore covers it, and the summary the `release` approval covers shows it.
  `promotion.json` does not change for this;
- a new job, `github-release`, runs after `iso`. Its first step, without the job token, verifies
  the downloaded bundle against the workflow identity (`evidence-bundle.sh verify-dir`). Its
  second step runs `system/github-release.sh publish` with the token and nothing else. That
  step:
  - checks that the bundle is this run's;
  - recomputes the choice and refuses on any difference from `release.json`;
  - packs the bundle into one deterministic tar;
  - publishes the Release as a draft first and then for real.

  It never overwrites an asset.

The YAML only checks the ref, checks out the repository, downloads the artifact and calls the
scripts.

**Tech Stack:** bash (`set -euo pipefail`, `shopt -s inherit_errexit`), jq, GNU tar, cosign (verify only), `gh` CLI (`gh release`, `gh api`), Python 3 `unittest` with the offline fakes of `system/tests/fake_registry.py`, GitHub Actions YAML as glue.

**Spec:**
- `docs/architecture/doc_pipeline.md`. Its status line reads revision 4, approved 2026-10-15 (ADR-0118). PL42 kept its revision 3 text, approved on 2026-10-11 (ADR-0111). The parts used here:
  - section 2 item 3;
  - the retention row "evidence bundle per promoted release";
  - PL42;
  - the PB9 row of section 12. Its acceptance reads: "the first promotion has a GitHub Release with the bundle and a verifying `bundle.sha256` signature";
  - PL56 and PL60, which say which jobs belong to the `release` environment;
  - PL35, which defines the immutable `promoted-<run_id>` tag;
  - PL54, which limits runs to the `iso-v0` ref.
- ADR-0106 items 4 and 7.
- ADR-0115 item 2 and its consequences, which say to read "0.9" wherever these texts say "0.5".
- ADR-0131, the maintainer's decisions on this plan (section "Decisions (ADR-0131)" below); it amends ADR-0115 item 2 for the VSA.
- ADR-0129 item 3.
- ADR-0104 item 3 and ADR-0105 item 2. A 0.x release is a signed tag plus a `:stable` promotion.
- `docs/architecture/doc_update_trust.md` UT9. The version label is `<base major>.<UTC build date>.<serial>` and is never used to order releases.
- `docs/operations/branching.md` BRN4.
- `docs/operations/release-1.0.md` row 40 and the 0.9 row of section 4.1, which names #600 and #604 as known limits for its release notes.

## Decisions this plan takes

Each decision below follows from an approved text or from ADR-0131, whose items are listed
at the end.

1. **The job.** A new job, `github-release`, in `promote-stable.yml` (`promote.yml` after PB5's
   rename) with `needs: [iso]`. It publishes a Release only for a promotion whose system images
   and ISO have both moved, so a Release never announces something that did not happen. "Re-run
   failed jobs" retries the Release alone, as the `iso` job's comment already describes for the
   ISO.
2. **Its permissions: `contents: write` and nothing else** **[TRUST]**. It gets no `id-token`:
   the keyless signature PL42 asks for already exists, made by the `plan` job under the
   workflow identity `…/promote-stable.yml@refs/heads/iso-v0`. A second signature, made after
   the approval, would sign the same `bundle.sha256` again and would give a job that holds
   `contents: write` an OIDC token as well. It gets no `packages` permission and no registry
   login: it reads only the signed artifact. It runs in no environment (ADR-0131 decision 1).
3. **The version** (UT9) comes from the `org.opencontainers.image.version` label of the
   `athanor-system` digest in the run's `image-digests.txt`. That digest is always listed. The
   label is recorded in `promotion.json` as `version`, with `revision`, the
   `org.opencontainers.image.revision` commit. Both are read by digest, so the plan and the apply
   compute the same record, the job summary the approval covers shows them, and the bundle
   signature binds them. The Release title is the version. The tag is `promoted-<build run id>`,
   created on `revision`, unless decision 4 applies (ADR-0131 decision 2).
4. **0.x releases.** A tag matching `v<major>.<minor>.<patch>` that points at `revision` counts
   only if it meets decision 3 of ADR-0131: an annotated tag whose `verification.verified` is
   true and whose tagger email equals the repository variable `MAINTAINER_TAG_EMAIL`. If
   exactly one such tag exists, the promotion is that 0.x release:
   - the Release uses that tag, so the API creates no tag, and it is marked `--latest`;
   - the title is the tag's subject followed by the version, for example
     `Athanor 0.9 "surface" (43.20261024.5123)`;
   - the notes are `docs/releases/<major>.<minor>.<patch>.md`, from the checkout of the commit
     the promotion was dispatched on (`github.sha`, which every job of the run checks out), then
     the text of decision 5. The notes file's first line must be a level-1 heading that names
     `<major>.<minor>.<patch>`.

   `system/github-release.sh plan` fails the `plan` job, before the approval and before any
   bundle is signed, in each of these cases. The error names the tag.
   - `MAINTAINER_TAG_EMAIL` is unset;
   - `revision` is not in the history of `iso-v0`, checked through the `branches` and `compare`
     endpoints as `system/trusted-run.sh` checks a run. ADR-0105 item 2 defines a 0.x release
     as a signed tag on `iso-v0`, so a tag elsewhere is no release;
   - a `v*` tag on the commit is lightweight, unverified, or tagged by another email
     (ADR-0131 decision 7);
   - more than one `v*` tag points at the commit;
   - the notes file is missing or its first heading names another version.
5. **Notes of a promotion without a 0.x tag.** The text the `release` approval covered
   (`system/evidence-bundle.sh summary`), followed by the commands that verify the bundle. No
   notes file is needed, so an hourly promotion (UD5) never waits for one. Its Release is marked
   `--latest=false`.
6. **One asset per promotion run:** `evidence-<run_id>-<promotion run id>.tar`. Both ids come
   from the record: `.run_id`, and the last path segment of `.trigger`. The name follows the
   off-GitHub copy `athanor-evidence:<run_id>-<github.run_id>`. The tar holds the bundle
   directory as it is, including `release.json`, `bundle.sha256` and
   `bundle.sha256.sigstore.json`, so `evidence-bundle.sh verify-dir` works on the extracted
   tree. The archive is deterministic:
   `tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu`. A re-run
   produces the same bytes.
7. **Idempotence and second promotions** (ADR-0131 decision 6). `publish` handles each state as
   follows:

   | State found | What `publish` does |
   | --- | --- |
   | No Release | Creates a draft with the asset, then publishes it with its `latest` flag |
   | A draft without this asset | Uploads the asset, then publishes |
   | A draft with this asset, same SHA-256 | Publishes |
   | A published Release without this asset (a second promotion of the same build) | Uploads the asset |
   | A published Release with this asset, same SHA-256 (a re-run) | Does nothing and succeeds |
   | This asset with a different SHA-256 | Stops; it never uses `--clobber` |
   | The tag already points at another commit | Stops |

   A second build run of a commit that carries a `v0.x` tag, such as the nightly rebuild of
   PQ18, does not join the `v*` Release. A `v*` Release takes assets only from the build run of
   its first asset. `plan` names `promoted-<run_id>` for any other build, with the UT9 version as
   title and `--latest=false`.
8. **Fail closed.**
   - Before packing, `publish` checks that `.run_id` equals `RUN_ID` and that `.trigger` equals
     `$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID`. `verify-dir` alone
     proves the bundle is signed, not that it is this run's; `system/promote-iso.sh` checks
     `run_id` the same way.
   - The tag, title, `latest` flag and notes SHA-256 it recomputes must equal `release.json`.
   - Any `gh` error other than "release not found" stops the script; it is never read as
     "absent".
   - After it publishes, `publish` reads the Release back. It must not be a draft, it must hold
     the asset with the local SHA-256, and its tag must resolve to `revision`.

   A failed `github-release` job leaves `:stable` where `promote` put it, because the Release
   follows the promotion and does not gate it. Everything that could make every re-run fail (the
   notes file, the tag) is checked in `plan`, before the approval. The run shows red, and the
   alert of PL51 fires once PB2 adds it. The off-GitHub copy, checked before `:stable` moved,
   stays the store that fails closed (ADR-0106 item 4).
9. **Testing without GitHub.** `fake_registry.py` already stands in for `gh` on `PATH`. It gains:
   - `gh release view|create|upload|edit|download`;
   - `gh api` paths for git tag refs and tag objects, the `branches` endpoint and the `compare`
     endpoint, all called with a leading `/` as the script calls them, since the fake matches
     only paths that start with `/`.

   Its state is kept in the fixture file. `system/tests/test_github_release.py` drives both
   subcommands through it.

## Global Constraints

- English everywhere; Conventional Commits as `git log` shows (`feat(release): …`, `test(release): …`, `ci(release): …`, `docs(release): …`); no attribution of any kind.
- **No key, no new signature.** No task reads `system/keys/` beyond what `promote.sh` already reads, and none signs anything. The only cosign call added is `verify-blob`, run through the existing `verify-dir`.
- **Stop and ask.** Task 1 edits `scripts/ci/promotion-record.sh`, the record library that `system/promote.sh` sources. Task 4 edits a workflow whose jobs run in `release`. Both start with "show the diff to the maintainer" and are not pushed without the maintainer's agreement in the conversation.
- **YAML is glue** (`.github/workflows/AGENTS.md`). Logic lives in `system/github-release.sh`. The job's `run:` lines only call it, and values reach it through `env`, never interpolated into `run:` (PL54).
- No `|| true`, no `continue-on-error`, no `2>/dev/null` that hides an error. A `gh` call whose failure is inspected captures stderr to a file and matches the not-found message, as `tag_of` in `promotion-record.sh` does.
- Existing Markdown under `docs/` is changed only through an exact-match Python replacement that asserts the old text occurs once, then `git diff --numstat`. New files may be written whole.
- No `cd`; use `git -C`, repository-relative paths, `python3 -B -m unittest discover -s system/tests`.
- Checks before each commit: `bash -n` on changed scripts, `shellcheck`, `python3 -B -m unittest discover -s system/tests`; for workflow changes also `actionlint` and `python3 scripts/verify.py workflows ci docs registry`; `just check` before the pull request.
- Base: `origin/iso-v0` at `a1086a7a` (after #763, ADR-0130).
- **[SETTINGS]** marks a change to `.github/settings/` that the maintainer applies with `ghsettings.py` (`docs/operations/github-settings.md`).
- **Unverified:** marks a claim this plan could not check; the step that relies on it says how to check it.

## Review Focus

- **[TRUST]** Task 4: the job's `permissions:` block (`contents: write` only), its lack of an environment (ADR-0131 decision 1), that no step of it has `id-token` or a registry login, and that the step holding the token runs no nix and no cosign.
- **[TRUST]** Task 4: the step `plan` gains, in a job that holds `id-token: write`. It holds `GH_TOKEN` and runs no nix.
- **[TRUST]** Task 1: `version` and `revision` added to `promotion.json`. The apply compares the recomputed record with the signed plan, so a plan made before the merge and applied after it fails with "plan again". That is fail-closed, and acceptable once.
- **[TRUST]** Task 2: `release.json` enters the signed bundle and the approved summary, and `publish` refuses any difference from it. The bundle identity is unchanged (`promote-stable.yml@refs/heads/${RELEASE_BRANCH:-iso-v0}`).
- **[TRUST]** Task 2: the 0.x tag check (ADR-0131 decisions 3 and 7). A lightweight or unverified `v*` tag, or one tagged by another email, fails `plan` and names the tag; it is never treated as "no tag".
- **[SETTINGS]** Task 3: the tag rulesets and their single bypass actor.
- Every place where `gh` can fail must stop the script. "Not found" is the only state read as absence.
- With a `contents: write` token, the Releases API creates `promoted-<run_id>` on `revision`, and the `promotion-tags` ruleset then keeps it from moving. Check that nothing in the repository treats tags as trusted refs: `KERNEL_TRUSTED_REFS` lists branches (BRN6), and `promoted-*` cannot shadow `iso-v0` (PL55).

## Tasks

### Task 1: Record the UT9 version and the commit in the promotion record **[TRUST]**

**Files:**
- Modify: `scripts/ci/promotion-record.sh` (`promotion_record`)
- Modify: `system/evidence-bundle.sh` (`summary`)
- Modify: `system/tests/test_promote.py`, `system/tests/test_evidence_bundle.py`

**Interfaces:**
- `promotion.json` gains two top-level fields:
  - `version`: the `org.opencontainers.image.version` label of `athanor-system@<recorded digest>`, which must match `^[0-9]+\.[0-9]{8}\.[0-9]+$`;
  - `revision`: the `org.opencontainers.image.revision` label, which must match `^[0-9a-f]{40}$`.

  Both are read with one `skopeo inspect --config` of that digest, beside `created()`. A missing
  or malformed label stops the plan with `die`.
- `summary` prints `Version: <version> (commit <revision>)` under its heading. A missing field is an error, as for the other fields.

- [ ] **Step 1: Show the diff to the maintainer before the push** (stop-and-ask: `promote.sh` sources this library).
- [ ] **Step 2: Failing tests:**
  - `test_promote.py`: `test_the_record_names_the_version_and_commit_of_the_default_image`, `test_a_default_image_without_a_version_or_revision_label_is_refused`, `test_the_version_is_the_default_images_even_when_it_is_already_stable`;
  - `test_evidence_bundle.py`: `test_summary_names_the_version_and_commit`, `test_summary_refuses_a_plan_without_a_version`.

  The fixture's `configs` gain the two labels.
- [ ] **Step 3: Implement.** Run `python3 -B -m unittest discover -s system/tests`. The existing record tests pass after their fixtures gain the labels.
- [ ] **Step 4: Commit** `feat(release): record the UT9 version and commit of the promoted build`.

### Task 2: `system/github-release.sh` and its offline test **[TRUST]**

**Files:**
- Create: `system/github-release.sh`
- Modify: `system/evidence-bundle.sh`. `create` copies `release.json` and fails without it. `summary` prints the Release's tag, title, `latest` flag and notes SHA-256 from it.
- Modify: `system/tests/fake_registry.py`. Add `gh release` and the `gh api` paths of decision 9, with the state kept in the fixture. Document two new fixture keys in its docstring:
  - `releases`: `{tag: {"draft": bool, "latest": bool, "target": sha, "title": str, "body": str, "assets": {name: sha256}}}`;
  - `git_tags`: `{name: {"sha": object sha, "type": "commit"|"tag", "commit": sha, "message": str, "tagger_email": str, "verified": bool}}`. The existing key `tags` already holds registry tags.
- Modify: `system/tests/test_evidence_bundle.py`
- Create: `system/tests/test_github_release.py`, built on `Tool` from `test_kernel_artifacts.py`.

**Interfaces:**
- `github-release.sh plan ARTIFACTS_DIR` writes `ARTIFACTS_DIR/release.json`: `{"tag", "title", "latest", "notes_sha256"}`.
  - Environment: `GH_TOKEN` (read access), `GITHUB_REPOSITORY`, `MAINTAINER_TAG_EMAIL`, `RELEASE_BRANCH` (default `iso-v0`), `RELEASE_NOTES_DIR` (default `docs/releases` under the repository root).
- `github-release.sh publish BUNDLE_DIR` publishes the bundle, already verified by `verify-dir` in the step before.
  - Environment: `GH_TOKEN` (`contents: write`), `GITHUB_REPOSITORY`, `GITHUB_SERVER_URL`, `GITHUB_RUN_ID`, `RUN_ID`, `MAINTAINER_TAG_EMAIL`, `RELEASE_NOTES_DIR`.
  - Output: on standard output, the Markdown line with the Release URL, for `$GITHUB_STEP_SUMMARY`.
- Exit status of both subcommands: 0 done or already done, 1 refused, 2 usage.
- Internal functions, one job each:
  - `release_of` returns tag, title, `latest` and the notes file built from the record, the tags at `revision` and the existing `v*` Release (decisions 4, 5 and 7). `plan` and `publish` both call it;
  - `on_branch` runs the `branches` and `compare` check of decision 4;
  - `run_check` performs the check of decision 8;
  - `pack` writes the tar of decision 6;
  - `publish_state` holds the table of decision 7;
  - `read_back` performs the last check of decision 8.
- `gh` calls used, each with a leading `/`:
  - `gh api /repos/$GITHUB_REPOSITORY/git/matching-refs/tags/v`;
  - `gh api /repos/$GITHUB_REPOSITORY/git/tags/<sha>` for the type, `object.sha`, `tagger.email`, `message` and `verification.verified`;
  - `gh api /repos/$GITHUB_REPOSITORY/git/ref/tags/<tag>`;
  - `gh api /repos/$GITHUB_REPOSITORY/branches/$RELEASE_BRANCH --jq .commit.sha` and `gh api /repos/$GITHUB_REPOSITORY/compare/<revision>...<branch sha> --jq .status`;
  - the `gh release` calls: `view <tag> --json isDraft,tagName,assets,url`, `create <tag> <asset> --draft --target <revision> --title … --notes-file …`, `upload <tag> <asset>`, `edit <tag> --draft=false --latest|--latest=false`, and `download <tag> -p <asset> -D <dir>` to read back an asset that already exists.

- [ ] **Step 1: Failing tests.** Every "refuses" test asserts exit status 1 and the expected stderr message. So does every test that "publishes nothing", which also asserts that the call log holds no `release create`, `upload` or `edit`. None of them can pass before the script exists.
  - `test_evidence_bundle.py`:
    - `test_create_refuses_a_plan_without_release_json`
    - `test_summary_names_the_release_tag_title_and_notes_hash`
  - `test_github_release.py`, `plan`:
    - `test_plan_names_the_promotion_tag_and_the_version_without_a_version_tag`
    - `test_plan_names_a_maintainer_version_tag_with_its_notes_and_latest`
    - `test_plan_refuses_without_maintainer_tag_email`
    - `test_plan_refuses_a_revision_not_on_the_release_branch`
    - `test_plan_refuses_a_lightweight_version_tag_and_names_it`
    - `test_plan_refuses_an_unverified_version_tag_and_names_it`
    - `test_plan_refuses_a_version_tag_of_another_tagger_and_names_it`
    - `test_plan_refuses_two_version_tags_at_the_commit`
    - `test_plan_ignores_a_version_tag_at_another_commit`
    - `test_plan_refuses_a_version_tag_without_its_notes_file`
    - `test_plan_refuses_notes_whose_first_heading_names_another_version`
    - `test_plan_sends_another_build_of_a_tagged_commit_to_its_promotion_tag`
    - `test_a_gh_error_other_than_not_found_is_not_taken_for_absent`
  - `test_github_release.py`, `publish`:
    - `test_publish_refuses_a_bundle_of_another_build_run`
    - `test_publish_refuses_a_bundle_of_another_promotion_run`
    - `test_publish_refuses_when_the_recomputed_release_differs_from_release_json`
    - `test_the_release_is_created_as_a_draft_then_published_with_its_latest_flag`
    - `test_a_promotion_release_is_published_with_latest_false`
    - `test_a_draft_without_the_asset_is_completed`
    - `test_a_draft_holding_the_same_asset_is_published`
    - `test_a_rerun_with_the_same_asset_changes_nothing`
    - `test_an_existing_asset_with_other_bytes_is_refused_and_not_replaced`
    - `test_a_second_promotion_of_the_same_build_adds_its_own_asset`
    - `test_a_promotion_tag_at_another_commit_publishes_nothing`
    - `test_the_tar_is_byte_identical_across_runs`
    - `test_the_published_release_is_read_back`
    - `test_no_call_uses_clobber`. It asserts that the log holds a `release create` or `upload` and no `--clobber`.
- [ ] **Step 2: Implement** the script, the `evidence-bundle.sh` change and the fake. Two claims are unverified; check both with the real `gh` on a scratch repository before the pull request:
  - **Unverified:** `gh release view <tag>` finds a draft whose tag does not exist yet. gh resolves a draft by listing releases when the tags endpoint answers 404. If it does not, `release_of` looks the draft up with `gh api /repos/…/releases --paginate` and matches `tag_name`.
  - **Unverified:** a `contents: read` token, which is what the `plan` job holds, sees draft Releases. If it does not, a draft left by a failed `github-release` job is invisible to `plan`. Its recomputation in `publish` then differs from `release.json`, and `publish` refuses, which is fail-closed. The fix is to re-run from `plan`.
- [ ] **Step 3: Checks:** run `bash -n` and `shellcheck` on both scripts, then `python3 -B -m unittest discover -s system/tests`.
- [ ] **Step 4: Commit** `feat(release): name and publish the GitHub Release of a promotion (PL42)`.

### Task 3: The tag ruleset and the maintainer's tag email **[SETTINGS]**

This task comes before the job is wired (ADR-0131 decision 8). The maintainer applies the
settings; the agent only edits the files.

**Files:**
- Modify: `.github/settings/rulesets.json`. Add two rulesets with `"target": "tag"` and `"enforcement": "active"`:
  - `version-tags`: `refs/tags/v*`, rules `creation`, `update` and `deletion`. Its only bypass actor is the repository admin role: `{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}`.
  - `promotion-tags`: `refs/tags/promoted-*`, rules `update` and `deletion`, and no bypass actor.
  - The App is never a bypass actor: no `Integration` actor in either ruleset.
- Modify: `.github/settings/actions.json` `variables`. Add `MAINTAINER_TAG_EMAIL`.
- Modify (exact-match script): `docs/operations/secrets.md`. Add a row `VAR11 | MAINTAINER_TAG_EMAIL | none; set by the maintainer | promote-stable.yml plan and github-release, through system/github-release.sh: the tagger email a v0.x tag must carry (ADR-0131 decision 3)`.
- Modify (exact-match script): `docs/architecture/doc_ci.md`, the table of secrets and variables (the rows near `EVIDENCE_REGISTRY`, around line 411). Add a row: `MAINTAINER_TAG_EMAIL` | variable, no default: unset, `plan` fails closed (ADR-0131 decision 3) | repository, set by the maintainer | CI11.
- Modify (exact-match script): `docs/operations/github-settings.md`, the "Desired state ahead of GitHub" table (around line 228). The `rulesets.json` row names `version-tags` and `promotion-tags` with their rules and bypass actor; the `actions.json` row names the variable `MAINTAINER_TAG_EMAIL`, with "not set" in its live column until the maintainer sets it.

- [ ] **Step 1: Edit the files.** **Unverified:** that `actor_id` 5 is the admin role, and that `ghsettings.py` round-trips a `tag` target. Check both with the plan that `apply` prints (Step 3) and with `python3 -B -m unittest discover -s scripts/tests` (`test_ghsettings.py`).
- [ ] **Step 2: Commit** `chore(settings): protect the tags a GitHub Release names (ADR-0131)`.
- [ ] **Step 3: [M] The maintainer applies the settings** after the merge, from their own machine. The commands are the ones `docs/operations/github-settings.md` gives:
  - `python3 scripts/github-settings/ghsettings.py apply`, then read the plan: two `POST …/rulesets` and no `DESTRUCTIVE` line;
  - `python3 scripts/github-settings/ghsettings.py apply --yes`;
  - `python3 scripts/github-settings/ghsettings.py diff`, which must print no drift for `rulesets`;
  - `gh variable set MAINTAINER_TAG_EMAIL -R ars-regia/athanor`, which reads the value from the prompt.

### Task 4: The `github-release` job, and the Release choice in `plan` **[TRUST]**

**Files:**
- Modify: `.github/workflows/promote-stable.yml`. Add one step to the `plan` job and the new job, and name `system/github-release.sh` in the header comment.
- Modify (exact-match script): `docs/architecture/doc_ci.md`, CI11 entry:
  - Purpose: "then publishes the GitHub Release of PL42";
  - Environment line: "`release` (jobs `promote` and `iso`); `github-release` has none and holds `contents: write` only";
  - variables: add `MAINTAINER_TAG_EMAIL`.

**Interfaces:**
- **[TRUST]** The `plan` job gains one step between "Plan the promotion" and "Bundle and sign the evidence". It runs `bash system/github-release.sh plan artifacts`, with `env: GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}` and `MAINTAINER_TAG_EMAIL: ${{ vars.MAINTAINER_TAG_EMAIL }}`. The job's permissions do not change: `contents: read` covers the tag, branch and compare reads.
- The new job:
  - `github-release: needs: [iso]`, `runs-on: ubuntu-24.04`, `timeout-minutes: 15`;
  - `permissions: { contents: write }` **[TRUST]**;
  - no `environment` (ADR-0131 decision 1) **[TRUST]**.
- Its steps:
  - the existing ref check step, copied verbatim (PL54, PL60);
  - `actions/checkout`, pinned at the same SHA, `persist-credentials: false`;
  - `cachix/install-nix-action`, pinned at the same SHA;
  - `actions/download-artifact`, pinned at the same SHA, `name: promotion-${{ inputs.run_id }}`, `path: approved`;
  - **[TRUST]** "Verify the approved bundle": `run: nix shell --inputs-from . nixpkgs#cosign -c bash system/evidence-bundle.sh verify-dir approved approved`, with no token in its `env`;
  - "Publish the Release": `run: bash system/github-release.sh publish approved | tee -a "${GITHUB_STEP_SUMMARY}"`, with `env: GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}` and `MAINTAINER_TAG_EMAIL: ${{ vars.MAINTAINER_TAG_EMAIL }}`, and no nix. The job token is set only where `gh` runs, as the comment at `promote-stable.yml` line 80 says of `fetch-evidence.sh`.

  `RUN_ID` already comes from the workflow `env`. bash runs with `-eo pipefail`, as the file's `defaults` comment states.

- [ ] **Step 1: Show the diff to the maintainer before the push** (a workflow with jobs in `release`, and a step added to the job that signs the bundle).
- [ ] **Step 2: Implement.** Run `actionlint` and `python3 scripts/verify.py workflows ci docs registry`. `verify.py workflows` must stay green: no job runs `promote.sh` or writes `:stable` (PL60 rule, `STABLE_WRITER`).
- [ ] **Step 3: Commit** `ci(release): publish the GitHub Release after the promotion (PL42)`.

### Task 5: Release notes of 0.9 and the row of release-1.0.md

**Files:**
- Create: `docs/releases/0.9.0.md`. Its first line is `# Athanor 0.9.0 "surface"`, the heading `plan` checks (decision 4). It must name:
  - screenshot and screen share as absent (ADR-0129 item 3);
  - the known limits #600 and #604 (the 0.9 row of `release-1.0.md` section 4.1);
  - the items of PL42 the bundle does not carry yet: provenance (PL19 to PL21), SBOM (PL24), scan report (PL25), VEX (PL26) and the VSA (PL29) (ADR-0131 decision 4);
  - Fedora 43 as the base (ADR-0115 item 3).

  The rest of the text is the maintainer's.
- Modify (exact-match script): `docs/operations/release-1.0.md` row 40. Its checks column gains "`system/github-release.sh`, job `github-release` of `promote-stable.yml`; `system/tests/test_github_release.py`".

- [ ] **Step 1: Draft the notes with the heading and the four required items**, mark the rest as the maintainer's, and check them with `python3 scripts/verify.py docs`. **Unverified:** whether `verify.py docs` asks a new directory under `docs/` for an index entry. If it does, add the entry it names.
- [ ] **Step 2: Commit** `docs(release): notes of release 0.9`.

### Task 6: Acceptance on the first promotion after the merge (PB9)

Not code. The maintainer runs these after the next promotion. Each is given as a command to paste:

- [ ] `gh release list -R ars-regia/athanor --limit 3` shows a Release whose title is the promoted version.
- [ ] `gh release download <tag> -R ars-regia/athanor -p 'evidence-*.tar' -D /var/tmp/pl42-check`, then `tar -xf` into an empty directory, then `cosign verify-blob --bundle bundle.sha256.sigstore.json --certificate-identity https://github.com/ars-regia/athanor/.github/workflows/promote-stable.yml@refs/heads/iso-v0 --certificate-oidc-issuer https://token.actions.githubusercontent.com bundle.sha256` and `sha256sum --strict -c bundle.sha256` in that directory. Both pass (the PB9 acceptance).
- [ ] Re-run the `github-release` job (`gh run rerun <promotion run> --job <job id> -R ars-regia/athanor`). It ends green, and `gh release view <tag> -R ars-regia/athanor --json assets` lists the same asset with the same size.
- [ ] `gh api /repos/ars-regia/athanor/rulesets --jq '.[].name'` lists `version-tags` and `promotion-tags` (Task 3).
- [ ] For 0.9: the maintainer's signed `v0.9.0` is on the promoted commit before the promotion is dispatched (ADR-0131 decision 5); the plan summary names `v0.9.0` and the notes hash; the Release shows the notes of Task 5 and is marked latest.

## Decisions (ADR-0131)

Decided by the maintainer on 2026-10-24; ADR-0131 records them and amends ADR-0115 item 2 for the VSA.

1. The `github-release` job runs in no environment and holds `contents: write` only.
2. A promotion without a 0.x tag gets the tag `promoted-<build run id>` on the build's commit, and the UT9 version as its title.
3. A 0.x tag counts when `verification.verified` of the tag object is true and its tagger email is the variable `MAINTAINER_TAG_EMAIL`; there is no allowed-signers file.
4. The Release ships today's bundle; the 0.9 notes name the PL42 items it lacks (provenance PL19 to PL21, SBOM PL24, scan PL25, VEX PL26, VSA PL29); 0.9 does not wait for the VSA.
5. The maintainer pushes the signed `v0.x.y` tag before dispatching the promotion of that build. The order is ADR-0131's own rule; ADR-0105 item 2 only defines a 0.x release as a signed tag plus a `:stable` promotion.
6. A second promotion of a build adds its bundle to the build's Release as an asset of its own, named after the promotion run; a re-run of a promotion run changes nothing.
7. A `v0.x` tag on the promoted commit that fails decision 3 fails the job closed, naming the tag; there is no fallback to `promoted-<build run id>`.
8. The tag rulesets protect `refs/tags/v*` (creation, update, deletion; the admin role is the only bypass) and `refs/tags/promoted-*` (update, deletion) in 0.9; the maintainer applies them, and the App is never a bypass actor.
