# GitHub Release of Each Promotion (PL42) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Maintainer review before any code.** This plan adds a job with `contents: write` to
> `promote-stable.yml`, which runs the `release` environment, and changes the promotion
> record that `system/promote.sh` compares against the signed plan. AGENTS.md lists
> `system/promote.sh` under "Stop and ask before editing". No task starts before the
> maintainer has approved the plan and answered the open questions. Every step that touches
> signing, an environment, a permission or the trust chain is marked **[TRUST]**.

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
the approval, and the approval covers it, so the Release job does not sign again. Two changes
are needed:

- the promotion record gains the UT9 version and the commit of the default image, so the signed
  plan names the Release;
- a new job, `github-release`, runs after `iso`. It downloads that artifact and passes it to a
  new script, `system/github-release.sh`. The script verifies the bundle against the workflow
  identity, works out the tag, title and notes, packs the bundle into one deterministic tar, and
  publishes the Release as a draft first and then for real. If the Release already holds the
  same asset, the script does nothing. Any other state stops the job, and it never overwrites an
  asset.

The YAML only checks the ref, checks out the repository, downloads the artifact and calls the
script.

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
- ADR-0129 item 3.
- ADR-0104 item 3 and ADR-0105 item 2. A 0.x release is a signed tag plus a `:stable` promotion.
- `docs/architecture/doc_update_trust.md` UT9. The version label is `<base major>.<UTC build date>.<serial>` and is never used to order releases.
- `docs/operations/branching.md` BRN4.
- `docs/operations/release-1.0.md` row 40 and the 0.9 row of section 4.1, which names #600 and #604 as known limits for its release notes.

## Decisions this plan takes

Each decision below follows from an approved text. The open questions at the end list the
decisions that remain the maintainer's.

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
   login: it reads only the signed artifact. It gets no environment under open question 1, which
   recommends that.
3. **The version** (UT9) comes from the `org.opencontainers.image.version` label of the
   `athanor-system` digest in the run's `image-digests.txt`. That digest is always listed. The
   label is recorded in `promotion.json` as `version`, with `revision`, the
   `org.opencontainers.image.revision` commit. Both are read by digest, so the plan and the apply
   compute the same record, the job summary the approval covers shows them, and the bundle
   signature binds them. The Release title is the version. The tag is decided by open
   question 2.
4. **0.x releases.** If exactly one tag matching `v<major>.<minor>.<patch>` points at
   `revision`, and it is an annotated tag that is signed under open question 3, the promotion is
   that 0.x release. In that case:
   - the Release uses that tag, so the API creates no tag;
   - the title is the tag's subject followed by the version, for example
     `Athanor 0.9 "surface" (43.20261024.5123)`;
   - the notes are `docs/releases/<major>.<minor>.<patch>.md` from the checked-out tip of
     `iso-v0`.

   The job stops before it creates anything in any of these cases:
   - the notes file is missing;
   - more than one `v*` tag points at the commit;
   - a `v*` tag points at the commit but is lightweight or unsigned. BRN4 says a release is a
     signed tag, so such a tag is a defect, not a fallback.
5. **Notes of a promotion without a 0.x tag.** The text the `release` approval covered
   (`system/evidence-bundle.sh summary`), followed by the commands that verify the bundle. No
   notes file is needed, so an hourly promotion (UD5) never waits for one.
6. **One asset per promotion run:** `evidence-<build run id>-<promotion run id>.tar`, named like
   the off-GitHub copy `athanor-evidence:<run_id>-<github.run_id>`. The tar holds the bundle
   directory as it is, including `bundle.sha256` and `bundle.sha256.sigstore.json`, so
   `evidence-bundle.sh verify-dir` works on the extracted tree. The archive is deterministic:
   `tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu`. A re-run
   produces the same bytes.
7. **Idempotence.** The script handles each state as follows:

   | State found | What the script does |
   | --- | --- |
   | No Release | Creates a draft with the asset, then publishes it |
   | A draft, or a published Release without this asset | Uploads the asset, then publishes |
   | A Release with this asset, same SHA-256 | Does nothing and succeeds |
   | A Release with this asset, different SHA-256 | Stops; it never uses `--clobber` |
   | The tag already points at another commit | Stops |

   If a second promotion of the same build run promotes a variant left out the first time, its
   asset joins the same Release.
8. **Fail closed.** `system/github-release.sh` first runs `evidence-bundle.sh verify-dir`, which
   checks the signature, the sums and that no unlisted file is present. If that fails, nothing
   is published. Any `gh` error other than "release not found" stops the script; it is never
   read as "absent". After it publishes, the script reads the Release back: it must not be a
   draft, it must hold the asset with the local SHA-256, and its tag must resolve to `revision`.
   A failed job leaves `:stable` where `promote` put it, because the Release follows the
   promotion and does not gate it. The run shows red, and the alert of PL59 fires once PB2
   lands. The off-GitHub copy, checked before `:stable` moved, stays the store that fails closed
   (ADR-0106 item 4).
9. **Testing without GitHub.** `fake_registry.py` already stands in for `gh` on `PATH`. It gains
   `gh release view|create|upload|edit|download`, plus `gh api` for tag refs and tag objects,
   with its state kept in the fixture file. `system/tests/test_github_release.py` drives the
   script through it.

## Global Constraints

- English everywhere; Conventional Commits as `git log` shows (`feat(release): …`, `test(release): …`, `ci(release): …`, `docs(release): …`); no attribution of any kind.
- **No key, no new signature.** No task reads `system/keys/` beyond what `promote.sh` already reads, and none signs anything. The only cosign call added is `verify-blob`, run through the existing `verify-dir`.
- **Stop and ask.** Task 1 edits `scripts/ci/promotion-record.sh`, the record library that `system/promote.sh` sources. Task 3 edits a workflow whose jobs run in `release`. Both start with "show the diff to the maintainer" and are not pushed without the maintainer's agreement in the conversation.
- **YAML is glue** (`.github/workflows/AGENTS.md`). Logic lives in `system/github-release.sh`. The job's `run:` lines only call it, and values reach it through `env`, never interpolated into `run:` (PL54).
- No `|| true`, no `continue-on-error`, no `2>/dev/null` that hides an error. A `gh` call whose failure is inspected captures stderr to a file and matches the not-found message, as `tag_of` in `promotion-record.sh` does.
- Existing Markdown under `docs/` is changed only through an exact-match Python replacement that asserts the old text occurs once, then `git diff --numstat`. New files may be written whole.
- No `cd`; use `git -C`, repository-relative paths, `python3 -B -m unittest discover -s system/tests`.
- Checks before each commit: `bash -n` on changed scripts, `shellcheck`, `python3 -B -m unittest discover -s system/tests`; for workflow changes also `actionlint` and `python3 scripts/verify.py workflows ci docs registry`; `just check` before the pull request.
- Base: `origin/iso-v0` at `bc66aff4` (after #761, ADR-0129).
- **Unverified:** marks a claim this plan could not check; the step that relies on it says how to check it.

## Review Focus

- **[TRUST]** Task 3: the job's `permissions:` block (`contents: write` only), its lack of an environment (open question 1), and that no step of it has `id-token` or a registry login.
- **[TRUST]** Task 1: `version` and `revision` added to `promotion.json`. The apply compares the recomputed record with the signed plan, so a plan made before the merge and applied after it fails with "plan again". That is fail-closed, and acceptable once.
- **[TRUST]** Task 2: the bundle is verified before anything is published, and the identity is the one in `evidence-bundle.sh` (`promote-stable.yml@refs/heads/${RELEASE_BRANCH:-iso-v0}`), unchanged.
- **[TRUST]** Task 2: the 0.x tag check (open question 3). A lightweight or unverified `v*` tag at the commit stops the job; it is never treated as "no tag".
- Every place where `gh` can fail must stop the script. "Not found" is the only state read as absence.
- With a `contents: write` token, the Releases API creates the tag of a promotion Release on `revision` (open question 2). Check that nothing in the repository treats tags as trusted refs. `KERNEL_TRUSTED_REFS` lists branches (BRN6), and `promoted-*` cannot shadow `iso-v0` (PL55).

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
- Modify: `system/tests/fake_registry.py`. Add `gh release` and the tag API, with the state kept in the fixture, and add fixture keys to its docstring: `releases` (`{tag: {"draft": bool, "target": sha, "title": str, "body": str, "assets": {name: sha256}}}`) and `tags` (`{name: {"sha": object sha, "type": "commit"|"tag", "commit": sha, "message": str, "verified": bool}}`).
- Create: `system/tests/test_github_release.py`, built on `Tool` from `test_kernel_artifacts.py`.

**Interfaces:**
- `github-release.sh BUNDLE_DIR`
  - Environment: `GH_TOKEN`, `GITHUB_REPOSITORY`, `GITHUB_SERVER_URL`, `GITHUB_RUN_ID` (the promotion run), `RELEASE_NOTES_DIR` (default `docs/releases` under the repository root).
  - Output: on standard output, the Markdown line with the Release URL, for `$GITHUB_STEP_SUMMARY`. Exit status: 0 published or already published, 1 refused, 2 usage.
- Internal functions, one job each:
  - `bundle_check` runs `evidence-bundle.sh verify-dir BUNDLE BUNDLE`;
  - `release_of` returns `tag title notes_file`, from the record and the tags at `revision` (decision 4, open questions 2 and 3);
  - `pack` writes the deterministic tar of decision 6;
  - `publish` holds the state machine of decision 7;
  - `read_back` performs the check of decision 8.
- `gh` calls used:
  - `gh api repos/$GITHUB_REPOSITORY/git/matching-refs/tags/v` to find tags;
  - `gh api repos/$GITHUB_REPOSITORY/git/tags/<sha>` for the tag type, `object.sha`, `message` and `verification.verified`;
  - `gh release view <tag> --json isDraft,tagName,assets,url`, `gh release create <tag> <asset> --draft --target <revision> --title … --notes-file …`, `gh release upload <tag> <asset>`, `gh release edit <tag> --draft=false`, and `gh release download <tag> -p <asset> -D <dir>`, which reads back an asset that already exists.

- [ ] **Step 1: Failing tests** in `test_github_release.py`:
  - `test_a_bundle_whose_signature_does_not_verify_publishes_nothing`
  - `test_a_bundle_with_an_unlisted_file_publishes_nothing`
  - `test_a_promotion_without_a_version_tag_publishes_under_the_promotion_tag_with_the_version_as_title`
  - `test_the_promotion_notes_are_the_approved_summary_and_the_verify_commands`
  - `test_a_signed_version_tag_at_the_commit_uses_that_tag_and_its_notes_file`
  - `test_a_version_tag_without_its_notes_file_publishes_nothing`
  - `test_a_lightweight_or_unverified_version_tag_publishes_nothing`
  - `test_two_version_tags_at_the_commit_publish_nothing`
  - `test_a_version_tag_at_another_commit_is_ignored`
  - `test_a_promotion_tag_at_another_commit_publishes_nothing`
  - `test_the_release_is_created_as_a_draft_then_published`
  - `test_a_draft_left_by_a_failed_run_is_completed`
  - `test_a_rerun_with_the_same_asset_changes_nothing`
  - `test_an_existing_asset_with_other_bytes_is_refused_and_not_replaced`
  - `test_a_second_promotion_of_the_same_build_adds_its_asset`
  - `test_a_gh_error_other_than_not_found_is_not_taken_for_absent`
  - `test_the_tar_is_byte_identical_across_runs`
  - `test_the_published_release_is_read_back`
  - `test_no_call_uses_clobber`

  The last test inspects the `FAKE_LOG` call log.
- [ ] **Step 2: Implement** the script and the fake. **Unverified:** `gh release view <tag>` finds a draft whose tag does not exist yet. gh resolves a draft by listing releases when the tags endpoint answers 404. Check with the real `gh` on a scratch repository before the pull request. If it does not, `release_of` looks the draft up with `gh api repos/…/releases --paginate` and matches `tag_name`.
- [ ] **Step 3: Checks:** run `bash -n system/github-release.sh`, `shellcheck system/github-release.sh` and `python3 -B -m unittest discover -s system/tests`.
- [ ] **Step 4: Commit** `feat(release): publish the evidence bundle of a promotion as a GitHub Release (PL42)`.

### Task 3: The `github-release` job of `promote-stable.yml` **[TRUST]**

**Files:**
- Modify: `.github/workflows/promote-stable.yml`. Add the new job and name `system/github-release.sh` in the header comment.
- Modify (exact-match script): `docs/architecture/doc_ci.md`, CI11 entry. Purpose: "then publishes the GitHub Release of PL42". Environment line: "`release` (jobs `promote` and `iso`); `github-release` has none and holds `contents: write` only".

**Interfaces:**
- The job:
  - `github-release: needs: [iso]`, `runs-on: ubuntu-24.04`, `timeout-minutes: 15`;
  - `permissions: { contents: write }` **[TRUST]**;
  - no `environment` (open question 1) **[TRUST]**.
- Its steps:
  - the existing ref check step, copied verbatim (PL54, PL60);
  - `actions/checkout` pinned at the same SHA, `persist-credentials: false`;
  - `cachix/install-nix-action` pinned at the same SHA;
  - `actions/download-artifact` pinned at the same SHA, `name: promotion-${{ inputs.run_id }}`, `path: approved`;
  - `run: nix shell --inputs-from . nixpkgs#cosign -c bash system/github-release.sh approved | tee -a "${GITHUB_STEP_SUMMARY}"`, with `env: GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`.

  `RUN_ID` already comes from the workflow `env`. bash runs with `-eo pipefail`, as the file's `defaults` comment states.

- [ ] **Step 1: Show the diff to the maintainer before the push** (a workflow with jobs in `release`).
- [ ] **Step 2: Implement.** Run `actionlint` and `python3 scripts/verify.py workflows ci docs registry`. `verify.py workflows` must stay green: the job runs neither `promote.sh` nor any write of `:stable` (PL60 rule, `STABLE_WRITER`).
- [ ] **Step 3: Commit** `ci(release): publish the GitHub Release after the promotion (PL42)`.

### Task 4: Release notes of 0.9 and the row of release-1.0.md

**Files:**
- Create: `docs/releases/0.9.0.md`. It must name:
  - screenshot and screen share as absent (ADR-0129 item 3);
  - the known limits #600 and #604 (the 0.9 row of `release-1.0.md` section 4.1);
  - the items of PL42 the bundle does not carry yet (open question 4);
  - Fedora 43 as the base (ADR-0115 item 3).

  The rest of the text is the maintainer's.
- Modify (exact-match script): `docs/operations/release-1.0.md` row 40. Its checks column gains "`system/github-release.sh`, job `github-release` of `promote-stable.yml`; `system/tests/test_github_release.py`".

- [ ] **Step 1: Draft the notes with the four required items**, mark the rest as the maintainer's, and check them with `python3 scripts/verify.py docs`. **Unverified:** whether `verify.py docs` asks a new directory under `docs/` for an index entry. If it does, add the entry it names.
- [ ] **Step 2: Commit** `docs(release): notes of release 0.9`.

### Task 5: Acceptance on the first promotion after the merge (PB9)

Not code. The maintainer runs these after the next promotion. Each is given as a command to paste:

- [ ] `gh release list -R ars-regia/athanor --limit 3` shows a Release whose title is the promoted version.
- [ ] `gh release download <tag> -R ars-regia/athanor -p 'evidence-*.tar' -D /var/tmp/pl42-check`, then `tar -xf` into an empty directory, then `cosign verify-blob --bundle bundle.sha256.sigstore.json --certificate-identity https://github.com/ars-regia/athanor/.github/workflows/promote-stable.yml@refs/heads/iso-v0 --certificate-oidc-issuer https://token.actions.githubusercontent.com bundle.sha256` and `sha256sum --strict -c bundle.sha256` in that directory. Both pass (the PB9 acceptance).
- [ ] Re-run the `github-release` job (`gh run rerun <promotion run> --job <job id> -R ars-regia/athanor`). It ends green, and `gh release view <tag> --json assets` lists the same asset with the same size.
- [ ] For 0.9: the maintainer's signed `v0.9.0` is on the promoted commit before the promotion is dispatched (open question 5), and the Release shows the notes of Task 4.

## Open questions for the maintainer

1. **The environment of `github-release`.**
   - Recommended: **none**, with `contents: write` only. PL60 and PL56 require `release` only for a job that runs `system/promote.sh` or writes `:stable`. The job runs only after `promote` and `iso`, which were approved in the same run. A re-run needs no new approval.
   - Alternative: `release`, which adds an approval click to every promotion.
2. **The tag of a promotion with no 0.x tag.**
   - Recommended: **`promoted-<build run id>`**, a lightweight tag that the Releases API creates on `revision`. It mirrors the immutable registry tag of PL35. The title stays the UT9 version, as PL42's "named after the version" requires.
   - Alternative: the tag `<UT9 version>`. Two builds of one commit on one UTC day carry the same version, because UT9's serial is the commit count, so the second Release would collide. UT9 also says the version string never orders releases.
   - A third alternative, no Release for such promotions, contradicts PL42's "each promotion publishes".
3. **How the 0.x tag's signature is checked.**
   - Recommended: **GitHub's verification of the tag object** (`verification.verified` from `git/tags/<sha>`), recorded in the job summary. It checks against the signing keys registered on the maintainer's account.
   - Alternative: an allowed-signers file in the repository read by `git verify-tag`. That is key material, so it is stop-and-ask.
   - BRN4 and ADR-0105 item 2 require a signed tag but do not say how CI checks one.
4. **The PL42 contents not in today's bundle.** PL42 lists SBOMs, provenance, the VSA, the scan report and VEX. Today's bundle holds `promotion.json`, `image-digests.txt`, the acceptance and signature evidence, and the package sets. The SBOM exists only as an attestation on each image.
   - Recommended: **the 0.9 Release carries the bundle as it is**, and its notes list the missing items. Each item joins the bundle, and so the Release, with the block that produces it (PL24 to PL26, PL29).
   - ADR-0115's consequences also move the VSA (PL29) into 0.9 by ADR-0106 item 7, which this plan does not build. The maintainer decides whether 0.9 waits for it.
5. **The order of the signed tag and the promotion.**
   - Recommended: **the maintainer pushes `v0.9.0` before dispatching the promotion of that build.** If the tag comes later, re-running `github-release` creates the `v0.9.0` Release beside the `promoted-<run>` one, and nothing is lost.
   - ADR-0105 item 2 defines a release as the pair but not the order of its two parts.
