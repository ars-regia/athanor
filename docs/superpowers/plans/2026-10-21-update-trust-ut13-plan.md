# Security-Class Updates (UT13, Row 25 of Release 0.10) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A security-class system image, recognised from a key-signed release attestation, downloads even on a metered connection and applies at the next shutdown the user starts, with one bounded postpone per lineage, a per-machine opt-out in the trust shield, and the hold of a digest greenboot returned from; issue #543, row 25 of `docs/operations/release-1.0.md`.

**Architecture:** The release pipeline derives each repository's class and advisory ids in `candidate` (PL55), writes an in-toto statement per image, and `sign-images` attaches it, key-signed, as the cosign attestation `<repo>:sha256-<hex>.att`. `athanor-update` fetches that attestation under the attachments policy, verifies its DSSE signature with the keys of the policy in force, and classifies the candidate; the check service unlocks a downloaded security digest for the next shutdown unless the opt-out is on or the lineage is postponed, keeps the lineage under `/var/lib/athanor-update/`, and publishes it in `athanor-trust-state` schema 2. Two new requests on `os.athanor.Update1` carry the postpone and the opt-out; the notifier and the bar's shield sheet are their only callers.

**Tech Stack:** Rust 2021 (zbus, serde, p256 ECDSA, GTK4 for the bar), Python 3 standard library for `scripts/ci/`, Bash for `system/` and the dev-VM harness, systemd units, polkit, cosign (attestation signing only, after the D43 amendment of Task 1).

**Spec:** `docs/architecture/doc_update_trust.md` revision 4 (UT6, UT7 schema 2, UT11, UT12, UT13, acceptance items 16 to 19); `docs/decisions/0094-update-control-settled-points.md`; `docs/decisions/0120-security-class-updates-in-1-0.md` as amended by `docs/decisions/0123-*.md` (row 25 ships in 0.10 "base"); `docs/architecture/doc_pipeline.md` PL31, PL55, PB4b and PB10; `docs/architecture/doc_bar.md` BR6 items 6 and 7; `docs/architecture/doc_shell.md` section 7, item 4; issue #543.

## Global Constraints

- English for everything committed, enterprise tone, Conventional Commits; no attribution, no model names, no co-author trailers.
- Base every worktree on `origin/iso-v0` and report its commit; never push to `iso-v0` or `main`; one logical change per commit.
- No `|| true`, no `continue-on-error`, no `2>/dev/null` that hides an error; no new `unwrap()`, `expect()` or `panic!` outside tests (`verify.py panics` budget only goes down).
- No fake security: the attestation is verified with real ECDSA P-256 over the DSSE pre-authentication encoding, or the image is of the feature class. Never an unconditional `Ok`.
- The plan never uses a project key. Tests sign with throwaway keys created and deleted by a vector script, as `tests/vectors/make.sh` does; only public material is committed.
- Stop-and-ask areas (`AGENTS.md`): every task that touches signing, attestation code, polkit or on-disk state starts with a **maintainer checkpoint** step and does not go past it until the maintainer approves in the conversation.
- Constants from the spec, verbatim: postpone length `7 * 86_400` seconds from `first_notice_at`; `boots_remaining` starts at `7`; metered means NetworkManager `Metered` `1` or `3`; error names `os.athanor.Update1.Error.AlreadyPostponed` and `os.athanor.Update1.Error.NotSecurityClass`; polkit `os.athanor.update.set-automatic` = `auth_admin` for any, inactive and active, never kept; `os.athanor.update.postpone` = `auth_admin` / `auth_admin` / `yes`; state at schema `2` adds `automatic_security_updates: bool` and `postpone` (object or `null`: `anchor`, `first_notice_at`, `boots_remaining`, `postponed`).
- Docs under `docs/`, `scripts/verify.py` and `forge/config/packages.json` change only through an exact-match replacement script, never Edit or Write (the format hook rewrites the whole file); check `git diff --numstat` after.
- Builds and tests at most 4 CPU threads (`cargo -j 4`, podman `--cpus 4`); cargo targets under `/var/tmp`; one VM at a time.
- Crates that link GTK test through `forge/test/shell/rig.sh cargo test -p <crate>` after `rig.sh build-image`.
- Nothing new is born in GTK (2026-10-18): Task 10 adds rows and a button to the existing shield sheet, the surface BR6 already specifies; no new GTK surface.

## Review Focus

1. **The wall clock jumps during a lineage** (RTC reset to 1970, NTP correcting a clock set years ahead). Expected: a clock set back never moves `first_notice_at` and never grants more than seven boots; a clock set ahead expires the postpone, which errs toward applying the fix. Test in Task 5.
2. **The lineage or opt-out file is unreadable or malformed** (power cut during a hand edit, a zero-length file). Expected: no silent default; the check publishes `last_error = storage`, unlocks nothing it would not otherwise unlock, and `Postpone()` refuses with `Failed`. Test in Task 5 and Task 6.
3. **An attestation image with several layers that disagree** (one DSSE envelope with `class = security` signed by an unknown key beside a policy-signed `feature` one, or an oversized or non-DSSE layer). Expected: only envelopes signed by a policy key whose subject is the candidate digest count; `security` needs at least one such envelope saying so and none saying otherwise; oversized layers are skipped unread past the cap. Test in Task 4.
4. **`Postpone()` when nothing will apply at the next shutdown** (opt-out on, nothing staged, the same digest already postponed, a signed copy of UT5 unlocked). Expected: a refusal that names why, nothing re-locked twice, and the lineage untouched. Test in Task 8.
5. **The shutdown evaluation cannot take the store lock** (a check still running when the user shuts down). Expected: the shutdown is never delayed past the unit's stop timeout, the deployment stays as it is, and the next boot's evaluation unlocks an expired postpone for the following shutdown. Test in Task 7.

---

### Task 1: Settle the open points and amend the specifications (maintainer checkpoint)

**Files:**
- Modify (exact-match script): `docs/architecture/doc_update_trust.md` (UT1, UT6, UT7 schema 2, UT11, UT13)
- Modify (exact-match script): `docs/architecture/doc_pipeline.md` (PB4b row, PL55, section 3.3 contract) and `docs/architecture/doc_kernel_profile.md` (D43: `cosign attest` in `sign-images`)
- Modify (exact-match script): `docs/architecture/doc_bar.md` (BR6 items 6 and 7: warning texts)
- Create: `docs/decisions/0127-release-attestation-and-ut13-details.md` (0126 is the toolkit's; confirm the number against `docs/decisions/README.md` when it is written)

**Interfaces:**
- Consumes: nothing.
- Produces: the decided values every later task cites: `PREDICATE_TYPE`, the `release-class.toml` path and format, the attestation tag and layer media type, the cosign flags of `sign-images`, the texts of the notice, the switch warning and the persistent warning (EN and IT), the refusal of `Postpone()` per state, the nullability of `postpone`, the shutdown hook.

- [x] **Step 1: Maintainer checkpoint.** *Answered 2026-10-21: every default below accepted, point 1 as amended; task 10 adds rows to the existing GTK shield sheet.* Show the maintainer the points below with the proposed default of each, and stop until each is answered in the conversation. They touch an approved document's text, signing (D43) and on-disk state.

| # | Point | Proposed default |
|---|---|---|
| 1 | Predicate type URI of the release attestation | `https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b` (a URL the project controls that leads to the specification) |
| 2 | Envelope and transport | in-toto Statement v1 in a DSSE envelope, attached by `cosign attest --key … --type <URI> --tlog-upload=false --new-bundle-format=false` as `<repo>:sha256-<hex>.att`, layer media type `application/vnd.dsse.envelope.v1+json` (retention already keeps `.att`, UT10) |
| 3 | Predicate fields | `class` (`security`\|`feature`), `advisories` (sorted, unique ids), `class_reason`, `build_time` (RFC 3339), `revision` (40-hex commit), `build_run_id` (decimal string), `updateinfo_timestamp` (integer or `null`); subject `[{name: <repository>, digest: {sha256: <hex>}}]` |
| 4 | Signing key | the image key the policy in force trusts for the repository (key 1 today, key 2 after PB5b, row 7); the client uses the same `keyPaths` as for image signatures |
| 5 | D43 | `sign-images` may run `cosign attest` beside the key, from `sigstore/cosign-installer` pinned by full SHA; `system/sign-images.sh` still starts no container |
| 6 | `release-class.toml` | one file per motivating change at `release/class/<YYYY-MM-DD>-<slug>.toml`, `class = "security"`, `advisories = ["CVE-…", "FEDORA-…"]`, optional `repositories = [...]` (absent = every repository); a file added or modified in `<base>..<candidate>` counts |
| 7 | Fedora half of PL55 | count only advisories of `type="security"`; updateinfo from the `updates` repository of the candidate's `VERSION_ID`, reached through the Fedora metalink and checked against the repomd checksum |
| 8 | Opt-out wording | UT6 and the last sentence of the UT13 opt-out bullet are read with "it" = the opt-out: `SetAutomaticSecurityUpdates(false)` re-locks an unlocked security digest; `SetAutomaticSecurityUpdates(true)` unlocks a waiting one and starts the lineage clock if it has not started. Both sentences are rewritten in terms of `enabled` |
| 9 | `postpone` nullability | `null` while no lineage is open (no security digest has reached `will-apply-at-next-shutdown` since the anchor); `anchor` is persisted under `/var/lib` either way; `first_notice_at` is never `null` inside the object |
| 10 | What counts as applied | the booted digest verifies as security class, differs from the anchor, and `rollbackQueued` is false; a boot greenboot returns from is not applied. *Amended on 2026-10-22 by the maintainer:* it was also built after the anchor, whose build time `lineage.json` keeps (a missing label never counts), so `GoBack()` to an earlier security digest does not reset the budget |
| 11 | What counts as a boot | a new `/proc/sys/kernel/random/boot_id` seen by `check --offline`; a soft reboot keeps the boot id and is not counted |
| 12 | Item 19 "three checks" | UT6's hold stands: the digest stays held until a newer one is offered; acceptance runs three checks and asserts none downloads it |
| 13 | Shutdown evaluation | `athanor-update-state.service` gains `RemainAfterExit=yes`, `ExecStop=/usr/bin/athanor-update shutdown`, `After=ostree-finalize-staged.service` (stopped before it), `TimeoutStopSec=30`; UT1's unit count is corrected |
| 14 | `Postpone()` refusals | not `will-apply-at-next-shutdown` and nothing postponed: `NothingDownloaded`; staged digest of the feature class or a signed copy: `NotSecurityClass`; budget used or expired: `AlreadyPostponed`; opt-out on: `NothingDownloaded` (nothing applies by itself, so there is nothing to postpone) |
| 15 | Logging of the switch | journal at notice with `MESSAGE_ID` `os.athanor.update.automatic-changed`, the caller's uid and the new value (UT6 already says it logs both; ADR-0094 leaves it open) |
| 16 | Texts | the notice (EN/IT), the "Postpone" action label, the switch's warning before it turns off, the persistent warning row, the `postponed-until <time>` row; drafted from UT13 and written into UT13 and BR6 |
| 17 | Scope cut of PB4b | markers, `mark-current`, the janitor's marker rule and the re-run rule stay with the rest of PB4b (ADR-0120); until markers exist, an unlabelled `:latest` is `unlabelled-baseline` |

- [ ] **Step 2: Write the decision record and the amendments** with an exact-match replacement script under `.scratch/`, from the answers of Step 1 only.
- [ ] **Step 3: Check.** Run: `python3 scripts/verify.py docs decisions` and `git diff --numstat`. Expected: both checks pass; only the replaced lines changed.
- [ ] **Step 4: Commit** with `docs(update): settle the release attestation and the UT13 details (ADR-0127)`. The pull request needs the maintainer's approval as any approved-document change does; no later task starts before it merges.

---

### Task 2: Derive the class and advisories in `candidate` (PL55)

**Files:**
- Create: `scripts/ci/release_class.py`
- Modify: `scripts/ci/release_candidate.py` (`check` writes `classes`), `scripts/ci/schemas/release-candidate.schema.json` (`schema_version` 2, `classes` array)
- Modify: `.github/workflows/release.yml` (`candidate` job: `fetch-depth: 0`, podman available)
- Test: `scripts/ci/tests/test_release_class.py`, `scripts/ci/tests/test_release_candidate.py`, fixtures under `scripts/ci/tests/fixtures/release_class/`

**Interfaces:**
- Consumes: Task 1 points 6, 7, 17.
- Produces:
  - `Advisory = NamedTuple("Advisory", [("id", str), ("type", str), ("fixed", dict[str, str])])` (package name to fixed NEVRA)
  - `parse_updateinfo(xml: bytes) -> tuple[int, list[Advisory]]` (metadata timestamp, advisories)
  - `fedora_half(old: dict[str, str], new: dict[str, str], advisories: list[Advisory]) -> list[str]`
  - `git_half(base: str, candidate: str, repo_root: Path, repository: str) -> list[str] | None` (`None` = no security file in range; `[]` never means security)
  - `derive(repository: str, baseline: Baseline | None, candidate: Image, sources: Sources) -> ClassRecord`
  - `ClassRecord` dict keys, verbatim from PL55: `repository`, `baseline_digest`, `candidate_digest`, `class`, `advisories`, `class_reason`, `updateinfo_timestamp`

- [ ] **Step 1: Write the failing tests** (`test_release_class.py`), each from the PB4b test list:
  - `test_no_baseline_is_feature`: `derive(repo, None, cand, src)` gives `class == "feature"`, `advisories == []`, `class_reason == "no-baseline"`.
  - `test_unlabelled_baseline_runs_only_the_fedora_half`: a `:latest` without `org.opencontainers.image.revision` and a security `release-class.toml` in history gives `class_reason == "unlabelled-baseline"`, `class == "feature"` with no Fedora advisory, and `"security"` with `["FEDORA-2026-aaaa"]` when the Fedora half finds one.
  - `test_security_release_class_toml_in_range_sets_the_class`: a commit in `<base>..<candidate>` adding `release/class/2026-10-21-x.toml` with `class = "security"`, `advisories = ["CVE-2026-0001"]` gives `security`, `["CVE-2026-0001"]`, `class_reason == ""`.
  - `test_a_file_outside_the_range_is_not_claimed`: the same file committed before `<base>` gives `feature`.
  - `test_a_file_scoped_to_another_repository_is_not_claimed`: `repositories = ["athanor-system-nvidia"]` does not count for `athanor-system`.
  - `test_fedora_advisory_between_latest_and_candidate_is_claimed`: old `openssl-3.5.1-1.fc45`, new `openssl-3.5.1-3.fc45`, advisory fixed at `-2` gives `["FEDORA-2026-aaaa"]`; fixed at `-1` (at or before old) or `-4` (after new) gives `[]`.
  - `test_only_security_advisories_count`: a `type="bugfix"` advisory in range is not claimed (Task 1 point 7).
  - `test_label_commit_absent_fails` and `test_unavailable_updateinfo_with_a_labelled_baseline_fails`: `derive` raises `ClassError` naming the repository.
  - `test_non_ancestor_label_uses_the_range`: a label commit on a side branch still yields `git log <label>..<candidate>`.
  - `test_empty_updateinfo_result_is_no_fedora_advisory`.
  - `test_advisories_are_the_sorted_union`: git and Fedora halves give `["CVE-2026-0001", "FEDORA-2026-aaaa"]` once each.
  - `test_malformed_release_class_toml_fails` (unknown `class` value, non-list `advisories`): `ClassError`.
  - In `test_release_candidate.py`: `test_check_writes_one_class_record_per_repository`, asserting the record validates against the schema at `schema_version == 2`.
- [ ] **Step 2: Run** `python3 -B -m unittest discover -s scripts/ci/tests -p 'test_release_c*.py'`. Expected: FAIL, `ModuleNotFoundError: release_class`.
- [ ] **Step 3: Implement** the interfaces above in `scripts/ci/release_class.py`: NEVRA comparison with `rpm.labelCompare` semantics written in the standard library (epoch, then `rpmvercmp` on version and release); RPM databases read without running any code of the candidate, since this job decides which release is signed as security: `podman create` (never started), `podman cp <container>:/usr/lib/sysimage/rpm <dir>`, `podman rm`, then the runner's own rpm, `rpm --dbpath <dir> -qa --qf '%{NAME} %|EPOCH?{%{EPOCH}}:{0}|:%{VERSION}-%{RELEASE}\n'` (an absent epoch prints `0`, never `(none)`); updateinfo via the metalink, repomd checksum checked, `.zst`/`.xz`/`.gz` decompressed with the standard library or `zstd` on the runner. `release_candidate.py check` calls `derive` per repository of `image-digests.txt` and writes `classes` into `release-candidate.json`; it prints `class_reason` into the job summary when non-empty.
- [ ] **Step 4: Run** the Step 2 command and `python3 scripts/verify.py workflows`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(release): derive the security class and advisories in candidate (PL55)`.

---

### Task 3: Produce and sign the release attestation (maintainer checkpoint: signing)

**Files:**
- Create: `scripts/ci/release_attestation.py`, `scripts/ci/tests/test_release_attestation.py`
- Modify: `system/sign-images.sh` (new `--attestations DIR`), `system/verify-images.sh` (verifies each `.att`)
- Modify: `.github/workflows/release.yml` (`candidate` writes statements; `sign-images` installs cosign and passes `--attestations`; `verify-images` verifies)
- Modify (exact-match script): `scripts/verify.py` (`SIGN_JOB_COMMANDS`, `SIGN_SCRIPTS`, the action allow-list, the `IMAGE_SIGN_SCRIPT` rule)
- Modify tests: `scripts/tests/test_verify_signing.py`, `system/tests/test_sign_images.py` (stubbed `skopeo` and `cosign`)

**Interfaces:**
- Consumes: Task 2 `ClassRecord`; Task 1 points 1 to 5.
- Produces: `statement(record: dict, build_time: str, revision: str, build_run_id: str) -> dict`; `write_statements(candidate_json: Path, out: Path) -> list[Path]` writing `out/<repository-basename>/<hex>.json`; the registry tag `<repo>:sha256-<hex>.att` that Task 4 reads.

- [ ] **Step 1: Maintainer checkpoint.** Name the files: `system/sign-images.sh`, the `sign-images` job of `.github/workflows/release.yml` (environment `signing`), the D43 lint in `scripts/verify.py`, `system/verify-images.sh`. Stop until the maintainer approves the D43 amendment of Task 1 point 5 and these edits. No step of this task uses a key; the first real signature happens in a release run the maintainer dispatches.
- [ ] **Step 2: Write the failing tests:**
  - `test_statement_shape`: `_type == "https://in-toto.io/Statement/v1"`, `predicateType == PREDICATE_TYPE`, one subject whose `digest.sha256` is the candidate hex, predicate keys exactly the seven of Task 1 point 3.
  - `test_feature_record_keeps_empty_advisories`: `advisories == []`, `class == "feature"`.
  - `test_write_statements_one_file_per_image` over a `release-candidate.json` fixture with two repositories.
  - `test_sign_images_attests_every_statement` (stub `cosign` records argv): one `cosign attest` per statement with `--key`, `--type <PREDICATE_TYPE>`, `--tlog-upload=false`, `--new-bundle-format=false`, `--predicate <file>` and the reference `<registry>/<repo>@sha256:<hex>`; the key file lives on tmpfs as the skopeo key already does.
  - `test_sign_images_refuses_a_statement_for_an_unlisted_digest`: a statement whose subject is not in `image-digests.txt` exits non-zero before any `cosign` call.
  - In `test_verify_signing.py`: `test_sign_job_may_run_the_attesting_command` (the amended command passes) and `test_sign_job_rejects_another_cosign_subcommand` (`cosign sign` in the step fails the lint).
- [ ] **Step 3: Run** `python3 -B -m unittest discover -s scripts/ci/tests -p 'test_release_attestation.py'`, `python3 -B -m unittest discover -s system/tests -p 'test_sign_images.py'`, `python3 -B -m unittest discover -s scripts/tests -p 'test_verify_signing.py'`. Expected: FAIL.
- [ ] **Step 4: Implement.** `release_attestation.py` (standard library); `sign-images.sh --attestations DIR` signs the images with skopeo as today, then attests; `verify-images.sh` runs `cosign verify-attestation --key <policy key> --type <PREDICATE_TYPE> --insecure-ignore-tlog=true --new-bundle-format=false` per digest and checks the payload's subject digest; the `candidate` job uploads the statements in `release-candidate`.
- [ ] **Step 5: Run** the Step 3 commands and `python3 scripts/verify.py workflows ci` (the D43 lint runs in `workflows`). Expected: PASS.
- [ ] **Step 6: Commit** `feat(release): sign a release attestation with the class and advisories (PB4b)`.

---

### Task 4: Verify the attestation on the machine (maintainer checkpoint: attestation code)

**Files:**
- Create: `forge/specs/athanor-update/athanor-update-1.0.0/src/attestation.rs`
- Modify: `src/tools.rs` (`fetch_attestation`), `src/store.rs` (`attestation_dir`), `src/main.rs` (`mod attestation`)
- Modify: `tests/vectors/make.sh` (writes `tests/vectors/made/att-*`)
- Test: unit tests in `src/attestation.rs`

**Interfaces:**
- Consumes: Task 1 points 1 to 4; `sigobj::load_key`, `sigobj::repository_of`; the keys `booted_claim` already gathers from `InForce.scopes`.
- Produces:
  - `pub enum Class { Security { advisories: Vec<String> }, Feature }`
  - `pub fn class_of(dir: &Path, keys: &[VerifyingKey], repository: &str, digest: &str) -> Class`
  - `Tools::fetch_attestation(&self, repository: &str, digest: &str, dest: &Path) -> Result<(), Failure>` (`skopeo copy --policy ATTACHMENTS_POLICY docker://<repo>:sha256-<hex>.att dir:<dest>`)
  - `Store::attestation_dir(&self, digest: &str) -> Option<PathBuf>` (`/var/lib/athanor-update/attestations/<hex>`, `None` unless `sha256:` and 64 hex)
  - `pub const PREDICATE_TYPE: &str` (Task 1 point 1)

- [ ] **Step 1: Maintainer checkpoint.** `AGENTS.md` asks before any new attestation code (ADR-0103 D10). Name the files above and the verification rule (DSSE PAE `"DSSEv1" SP len(type) SP type SP len(body) SP body`, ECDSA P-256 SHA-256 DER, payload type `application/vnd.in-toto+json`). Stop until approved.
- [ ] **Step 2: Extend `make.sh`** to write, with its throwaway keys `a` and `b`: `att-security` (security, `["CVE-2026-0001"]`, key `a`), `att-feature`, `att-no-class` (predicate without `class`), `att-bad-signature` (one byte of the signature flipped), `att-other-digest` (subject of another digest), `att-other-repo`, `att-wrong-type` (other `predicateType`), `att-unknown-key` (signed by `b` only), `att-mixed` (a security envelope signed by `b` and a feature envelope signed by `a`), `att-oversized` (a layer over the cap). Run `bash forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/make.sh`; commit only public material.
- [ ] **Step 3: Write the failing tests** in `attestation.rs`, all with `keys = [a]`:
  - `a_signed_security_statement_is_security`: `class_of(att-security) == Class::Security { advisories: vec!["CVE-2026-0001".into()] }`.
  - `an_invalid_signature_is_feature` (item 18).
  - `a_statement_without_the_class_field_is_feature` (item 18, ADR-0094 point 5).
  - `a_statement_for_another_digest_or_repository_is_feature`.
  - `another_predicate_type_is_feature`.
  - `a_key_outside_the_policy_is_feature`.
  - `a_missing_directory_is_feature`.
  - Review Focus 3: `envelopes_that_disagree_are_feature` (`att-mixed` with `keys = [a, b]` gives `Feature`; with `keys = [a]` gives `Feature`) and `an_oversized_layer_is_skipped_unread` (`att-oversized` gives `Feature`, read capped at `MAX_PAYLOAD`).
- [ ] **Step 4: Run** `cargo test -j 4 -p athanor-update attestation`. Expected: FAIL, unresolved `class_of`.
- [ ] **Step 5: Implement** the Produces list. Security only when at least one envelope verifies with a policy key, its subject is `(repository, digest)`, its predicate type is `PREDICATE_TYPE`, its `class` is `"security"`, and no verified envelope for that subject says otherwise. `class_of` re-verifies on every call; no verdict is cached on disk.
- [ ] **Step 6: Run** the Step 4 command and `python3 scripts/verify.py panics`. Expected: PASS.
- [ ] **Step 7: Commit** `feat(update): verify the release attestation and read its class (UT13)`.

---

### Task 5: Trust state schema 2 and the lineage store (maintainer checkpoint: on-disk state)

**Files:**
- Modify: `system/athanor-trust-state/src/lib.rs` (`SCHEMA = 2`, `Postpone`, two fields, consistency checks, `postponed_until`)
- Create: `forge/specs/athanor-update/athanor-update-1.0.0/src/lineage.rs`
- Modify: `src/store.rs` (lineage and opt-out files), `src/check.rs` (`run` publishes the two fields), `src/main.rs`
- Modify every schema-1 reader and fixture in the same commit: `forge/specs/athanor-update/athanor-update-notify-1.0.0/src/notices.rs` (test states), `forge/specs/athanor-bar/athanor-bar-1.0.0/src/shield.rs` (test states), `forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/src/seal.rs` (test states), `system/athanor-fuzz-entries/src/layout.rs`, `fuzz/corpus/trust_state/verified`, `forge/specs/athanor-update/athanor-update-1.0.0/tests/state-verified.json`, `forge/test/shell/trust_state.py`, `forge/test/shell/tests/test_trust_state.py`, `scripts/devvm/shield-acceptance.sh`, `scripts/devvm/acceptance/probes.py`, `scripts/tests/test_runtime_probes.py`

**Interfaces:**
- Consumes: Task 1 points 9 to 11.
- Produces:
  - In `athanor-trust-state`: `pub struct Postpone { pub anchor: Option<String>, pub first_notice_at: i64, pub boots_remaining: u32, pub postponed: bool }`; `State` gains `pub automatic_security_updates: bool` and `pub postpone: Option<Postpone>`; `pub const POSTPONE_SECS: i64 = 7 * 86_400`; `pub const POSTPONE_BOOTS: u32 = 7`; `pub fn postponed_until(state: &State) -> Option<i64>` (`Some(first_notice_at + POSTPONE_SECS)` only when `postponed`).
  - In `lineage.rs`: `pub struct Lineage { pub anchor: Option<String>, pub open: Option<Open> }`, `pub struct Open { pub first_notice_at: i64, pub boots_remaining: u32, pub postponed: bool, pub last_boot_id: String }`; `impl Lineage { pub fn notice(&mut self, now: i64, boot_id: &str); pub fn boot(&mut self, boot_id: &str); pub fn applied(&mut self, digest: &str); pub fn expired(&self, now: i64) -> bool; pub fn postpone(&mut self, now: i64) -> Result<i64, Budget>; pub fn published(&self) -> Option<Postpone> }`; `pub enum Budget { Used, Expired, NotOpen }`.
  - In `Store`: `lineage(&self) -> io::Result<Lineage>` / `set_lineage(&self, &Lineage) -> io::Result<()>` (`/var/lib/athanor-update/lineage.json`, 0644, through `Store::replace`); `automatic_security_updates(&self) -> io::Result<bool>` / `set_automatic_security_updates(&self, bool) -> io::Result<()>` (`/var/lib/athanor-update/automatic-security-updates`, `on` or `off`, absent = `on`).

- [ ] **Step 1: Maintainer checkpoint.** On-disk state: the published file changes schema (every reader rejects schema 1 after this commit, and the bar, notifier and greeter of an older deployment reject schema 2 after a `GoBack()` until the service rewrites the file at that boot) and two new persistent files appear under `/var/lib/athanor-update/`. Name the files above and stop until approved.
- [ ] **Step 2: Write the failing tests.**
  - trust-state: `schema_1_is_malformed`; `schema_2_round_trips` with `postpone: null` and with an object; `boots_remaining_above_seven_is_malformed`; `an_unknown_postpone_field_is_malformed`; `postponed_until_is_first_notice_plus_seven_days` (`first_notice_at = 1_790_000_000`, `postponed = true` gives `Some(1_790_604_800)`; `postponed = false` gives `None`).
  - lineage: `notice_opens_once_and_never_moves_first_notice_at` (second `notice` at a later `now` keeps the first); `postpone_once_then_used` (`Ok(first + 604_800)`, then `Err(Budget::Used)`); `a_newer_digest_inherits_the_budget` (the lineage is per machine, so `notice` after a new digest changes nothing); `seven_boots_expire` (`boot` with seven new ids sets `boots_remaining = 0` and `expired`); `the_same_boot_id_counts_once`; `applied_resets_and_records_the_anchor`; `go_back_does_not_reset` (no method is called on `GoBack`; asserted in Task 8).
  - Review Focus 1: `a_clock_set_back_neither_moves_nor_extends` (`notice(1_790_000_000)`, then `expired(1_000_000_000) == false` and `first_notice_at` unchanged; seven boots still expire it); `a_clock_set_ahead_expires`.
  - Review Focus 2: `a_malformed_lineage_file_is_a_storage_error` (`Store::lineage` on `b""` and on `b"{"` returns `Err`); `a_malformed_opt_out_file_is_a_storage_error` (`"maybe\n"` returns `Err`, absent returns `Ok(true)`).
  - Python fixtures: `test_trust_state.py` asserts the fixture writes `"schema": 2` with both fields.
- [ ] **Step 3: Run** `cargo test -j 4 -p athanor-trust-state -p athanor-update` and `python3 -B -m unittest discover -s forge/test/shell/tests`. Expected: FAIL.
- [ ] **Step 4: Implement** the Produces list; `check::run` publishes `automatic_security_updates` from the store and `postpone: lineage.published()`; every fixture and literal listed under Files moves to schema 2.
- [ ] **Step 5: Run** the Step 3 commands, `cargo test -j 4 -p athanor-update-notify -p athanor-fuzz-entries`, `forge/test/shell/rig.sh cargo test -p athanor-bar -p athanor-greeter-ui`, `python3 -B -m unittest discover -s scripts/tests`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(trust-state): schema 2 with the opt-out and the postpone lineage (UT7)`.

---

### Task 6: Classify the candidate, download on metered, unlock a security digest

**Files:**
- Modify: `forge/specs/athanor-update/athanor-update-1.0.0/src/check.rs` (`online_update`, `run`, the test `Fake`)

**Interfaces:**
- Consumes: Task 4 `class_of`, `Tools::fetch_attestation`, `Store::attestation_dir`; Task 5 `Lineage`, `Store::{lineage, set_lineage, automatic_security_updates}`.
- Produces: `fn candidate_class<T: Tools>(ctx: &Context<'_, T>, policy: &InForce, repository: &str, digest: &str) -> Class` (fetch, then `class_of`; a failed fetch is `Feature`, logged, not a check failure); `fn settle_security<T: Tools>(ctx: &Context<'_, T>, status: &Status, lineage: &mut Lineage, automatic: bool) -> Result<UpdateState, Failure>` (unlock or keep locked; opens the lineage on the first unlock); `Fake` gains `attestation(digest, Class)` and records `"fetch_attestation"`, `"apply_downloaded"`.
- The decision reads `Store::automatic_security_updates()` and treats `Err` as a failed check (`Failure`, `Storage`), never the published `automatic_security_updates`, which is `false` for an unreadable file too.

- [ ] **Step 1: Write the failing tests** in `check.rs`:
  - `a_security_digest_downloads_and_unlocks` (item 17): state `WillApplyAtNextShutdown`, calls include `download` then `apply_downloaded`, no reboot call, `postpone.first_notice_at == NOW`, `boots_remaining == 7`, `postponed == false`.
  - `a_feature_digest_stays_downloaded` (item 18): `Downloaded`, no `apply_downloaded`.
  - `a_digest_without_an_attestation_is_feature`: fetch fails, `Downloaded`, `last_error == None`.
  - `a_staged_digest_attested_later_is_settled_as_security`: the first check fails to fetch the attestation and stages the digest as feature (`Downloaded`); the second check, same candidate, fetches a security attestation: `settle_security` runs and the state reads `WillApplyAtNextShutdown`.
  - `a_security_digest_downloads_on_a_metered_connection` and `a_feature_digest_waits_on_a_metered_connection` (UT12 kept): `Available`, no `download`.
  - `with_the_opt_out_on_nothing_downloads_on_metered_and_nothing_unlocks` (ADR-0094 point 7, item 17): metered gives `Available`; unmetered gives `Downloaded` with no `apply_downloaded`; `automatic_security_updates == false` published.
  - `a_postponed_lineage_keeps_a_newer_security_digest_locked`: lineage `postponed`, not expired: `Downloaded`, no `apply_downloaded`, `first_notice_at` unchanged.
  - `an_expired_postpone_unlocks_at_the_next_check`.
  - `a_held_security_digest_is_not_downloaded_three_times` (item 19): with the digest in `held`, three `run(ctx, false)` calls make no `download` call and publish `Held`.
  - `a_greenboot_return_does_not_count_as_applied`: booted security digest with `rollback_queued = true` leaves the lineage open and the anchor unchanged.
  - `booting_a_security_digest_resets_the_lineage`: booted digest verifies as security, not queued: `postpone == None`, anchor = booted digest.
  - `a_signed_copy_opens_no_lineage`: the UT5 move unlocks with `postpone == None`.
  - `check_offline_counts_one_boot_per_boot_id`.
  - Review Focus 2: `an_unreadable_opt_out_file_unlocks_nothing`: `last_error == Storage`, no `apply_downloaded`.
- [ ] **Step 2: Run** `cargo test -j 4 -p athanor-update check::`. Expected: FAIL.
- [ ] **Step 3: Implement.** In `online_update`, classify after `offerable` returns `Available` and before the metered test, and also on the early return for a candidate already staged: a staged digest not yet known as security is classified again on each check, so an attestation that failed to fetch once does not leave a security digest waiting for `Apply()`; the metered test becomes `ctx.tools.metered() && !(class is Security && automatic)`; after a staged download-only deployment of the candidate, call `settle_security`. In `run`, before `online_update`: read the boot id from `/proc/sys/kernel/random/boot_id` (a `Context` field so tests set it), call `lineage.boot`, detect "applied" (Task 1 point 10) and call `lineage.applied`; in offline mode also unlock an expired postpone. Persist the lineage once per run, under the store lock already taken.
- [ ] **Step 4: Run** the Step 2 command, then `cargo test -j 4 -p athanor-update`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(update): apply security-class digests at the next shutdown (UT13)`.

---

### Task 7: Evaluate the expiry at the shutdown that would apply

**Files:**
- Modify: `forge/specs/athanor-update/athanor-update-1.0.0/src/main.rs` (`Command::Shutdown`), `src/check.rs` (`pub fn at_shutdown`)
- Modify: `forge/specs/athanor-update/SOURCES/usr/lib/systemd/system/athanor-update-state.service`
- Test: `forge/specs/athanor-update/tests/test_units.py`, unit tests in `check.rs`

**Interfaces:**
- Consumes: Task 5 `Lineage::expired`; Task 6 `settle_security`.
- Produces: `pub fn at_shutdown<T: Tools>(ctx: &Context<'_, T>) -> Result<(), Failure>` (non-blocking `Store::try_lock`; unlocks a staged security digest whose postpone expired; nothing else); CLI `athanor-update shutdown`.

- [ ] **Step 1: Write the failing tests:**
  - `shutdown_unlocks_an_expired_postpone`: lineage postponed, `now = first + 604_800`: one `apply_downloaded`.
  - `shutdown_leaves_an_unexpired_postpone_locked`.
  - `shutdown_never_touches_a_feature_digest`.
  - Review Focus 5: `shutdown_with_the_store_busy_changes_nothing` (lock held by the test: returns `Err` with `ErrorCode::Storage`, no tool call).
  - `test_units.py`: `test_state_unit_evaluates_at_shutdown` asserts `RemainAfterExit=yes`, `ExecStop=/usr/bin/athanor-update shutdown`, `After=` contains `ostree-finalize-staged.service`, `TimeoutStopSec=30`, and the hardening lines unchanged.
- [ ] **Step 2: Run** `cargo test -j 4 -p athanor-update shutdown` and `python3 -B -m unittest discover -s forge/specs/athanor-update/tests`. Expected: FAIL.
- [ ] **Step 3: Implement** `at_shutdown` and the unit change of Task 1 point 13. `PrivateNetwork=yes` stays: the evaluation needs no network.
- [ ] **Step 4: Run** the Step 2 commands and `python3 scripts/verify.py services`. Expected: PASS. The stop ordering against `ostree-finalize-staged.service` is proved on the dev VM in Task 11 (`stage_postpone`).
- [ ] **Step 5: Commit** `feat(update): unlock an expired postpone at shutdown (UT13)`.

---

### Task 8: `Postpone()` and `SetAutomaticSecurityUpdates()` (maintainer checkpoint: polkit)

**Files:**
- Modify: `forge/specs/athanor-update/athanor-update-1.0.0/src/requests.rs`, `src/serve.rs`
- Modify: `forge/specs/athanor-update/SOURCES/usr/share/polkit-1/actions/os.athanor.update.policy` (two actions, EN and IT messages), `forge/specs/athanor-update/SOURCES/usr/share/dbus-1/system.d/os.athanor.Update1.conf` (`send_member` `Postpone` and `SetAutomaticSecurityUpdates`)
- Modify (exact-match script): `docs/architecture/doc_threat_model.md` (TM9 rows for the two actions)
- Test: unit tests in `requests.rs`, `scripts/tests/test_verify_polkit_model.py`

**Interfaces:**
- Consumes: Task 5 `Lineage::postpone`, `Store::set_automatic_security_updates`; Task 6 `candidate_class` (class of the staged digest via `class_of`); `Tools::relock`, `Tools::apply_downloaded`.
- Produces:
  - `pub const ACTION_POSTPONE: &str = "os.athanor.update.postpone"`, `pub const ACTION_SET_AUTOMATIC: &str = "os.athanor.update.set-automatic"`
  - `Refusal::{AlreadyPostponed, NotSecurityClass}` mapped in `serve::Error` to `os.athanor.Update1.Error.AlreadyPostponed` and `.NotSecurityClass`
  - `pub async fn postpone<T: Tools, C: Caller>(ctx: &Context<'_, T>, caller: &C) -> Result<i64, Refusal>` (returns the expiry)
  - `pub async fn set_automatic<T: Tools, C: Caller>(ctx: &Context<'_, T>, caller: &C, enabled: bool) -> Result<(), Refusal>`
  - D-Bus `Postpone()` and `SetAutomaticSecurityUpdates(b enabled)`, both logging the caller's uid at notice (`uid_of`), the switch with `MESSAGE_ID` of Task 1 point 15.

- [ ] **Step 1: Maintainer checkpoint.** Name `os.athanor.update.policy`, `os.athanor.Update1.conf`, the TM9 rows and the two request functions; `system/athanor-bus-api/src/polkit.rs` is not edited. The actions and defaults were approved on 2026-10-08 (ADR-0094); confirm the refusal table of Task 1 point 14 and stop until approved.
- [ ] **Step 2: Write the failing tests** in `requests.rs` (with `FakeCaller` grants):
  - `postpone_relocks_and_records` (item 17): from `WillApplyAtNextShutdown` on a security digest: asks `ACTION_POSTPONE` once, calls `relock`, returns `Ok(first + 604_800)`, the republished state has `postpone.postponed == true` and `update == Downloaded`.
  - `a_second_postpone_is_already_postponed` and `a_newer_digest_of_the_lineage_is_already_postponed` (item 17): `Err(Refusal::AlreadyPostponed)`, `first_notice_at` unchanged, no second `relock`.
  - `an_expired_budget_is_already_postponed`.
  - `a_feature_digest_is_not_security_class`.
  - Review Focus 4: `postpone_with_nothing_staged_is_nothing_downloaded`, `postpone_with_the_opt_out_on_is_nothing_downloaded`, `postpone_of_a_signed_copy_is_not_security_class`; each leaves the lineage file byte-identical and calls no tool.
  - `postpone_refused_by_polkit_changes_nothing`.
  - `postpone_with_an_unreadable_lineage_is_failed` (Review Focus 2).
  - `turning_automatic_off_relocks_an_unlocked_security_digest` and `turning_it_on_unlocks_a_waiting_one_and_starts_the_clock` (Task 1 point 8): `set_automatic(false)` calls `relock`, publishes `automatic_security_updates == false`; `set_automatic(true)` calls `apply_downloaded`, `first_notice_at == NOW` when no lineage was open.
  - `set_automatic_asks_its_action_every_time` (no `_keep`: two calls ask twice).
  - `go_back_does_not_reset_the_lineage`.
  - `test_verify_polkit_model.py`: `test_update_policy_actions_have_tm9_rows` for both new ids.
- [ ] **Step 3: Run** `cargo test -j 4 -p athanor-update requests::` and `python3 -B -m unittest discover -s scripts/tests -p 'test_verify_polkit_model.py'`. Expected: FAIL.
- [ ] **Step 4: Implement** the Produces list; the `.policy` defaults verbatim from the Global Constraints; the D-Bus policy allows the two `send_member` values only with `send_interface=os.athanor.Update1`.
- [ ] **Step 5: Run** the Step 3 commands and `python3 scripts/verify.py polkit polkit-subject polkit-model services`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(update): Postpone and SetAutomaticSecurityUpdates requests (UT6)`.

---

### Task 9: The notifier's security notice and its "Postpone" action

**Files:**
- Modify: `forge/specs/athanor-update/athanor-update-notify-1.0.0/src/notices.rs`, `src/text.rs`, `src/main.rs`

**Interfaces:**
- Consumes: Task 5 `State.postpone`, `postponed_until`; Task 8 D-Bus `Postpone()`; Task 1 point 16 texts.
- Produces: `Notice::Security { digest: String, until: Option<i64> }`; `Request::Postpone` (method `"Postpone"`); `pub const ACTION_POSTPONE: &str = "postpone"`; `Texts` gains `security_title`, `security_body`, `postpone`, `postponed_until` (EN and IT, verbatim from Task 1).

- [ ] **Step 1: Write the failing tests:**
  - `a_security_digest_is_announced_once_with_postpone`: state `WillApplyAtNextShutdown`, `postpone` open and not postponed: `due` gives `Notice::Security`, the actions are `[ACTION_POSTPONE]` with no `ACTION_LATER` (item 17, "no open-ended Later"); a second `due` in the same or a later session gives `None`.
  - `a_postponed_digest_offers_no_postpone`: `postponed == true` gives a notice without `ACTION_POSTPONE` that carries `until`.
  - `with_the_opt_out_on_the_notice_still_arrives` (item 17): state `Downloaded`, `automatic_security_updates == false`, security digest: the UT11 notice with "Restart to update" and "Later".
  - `a_signed_copy_is_not_announced_as_security` (`postpone == None`).
  - `postpone_is_invoked_only_from_the_notification_owner`: `invoked` from another sender or id gives `None`; from the owner gives `Some(Request::Postpone)`.
  - `italian_texts_exist_for_every_security_string`.
- [ ] **Step 2: Run** `cargo test -j 4 -p athanor-update-notify`. Expected: FAIL.
- [ ] **Step 3: Implement.** A security notice is a `will-apply-at-next-shutdown` state with `postpone` not `null`; the Landlock ruleset is unchanged; `main.rs` maps `Request::Postpone` to the `Postpone` call and shows its refusal.
- [ ] **Step 4: Run** the Step 2 command. Expected: PASS.
- [ ] **Step 5: Commit** `feat(update-notify): announce security updates with one postpone (UT11, UT13)`.

---

### Task 10: The shield sheet: switch, warnings and the Postpone button (BR6 items 6 and 7)

**Files:**
- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/shield.rs`, `src/ui/shield.rs`, `po/athanor-bar.pot`, `po/en.po`, `po/it.po`

**Interfaces:**
- Consumes: Task 5 state fields; Task 8 D-Bus requests and error names; Task 1 point 16 texts.
- Produces: `Rows` gains `automatic_security_updates: bool`, `postponed_until: Option<i64>`, `postpone_offered: bool`; `Request::{Postpone, SetAutomatic(bool)}` with `method()` `"Postpone"` and `"SetAutomaticSecurityUpdates"`; `Refusal::{AlreadyPostponed, NotSecurityClass}`; `pub fn postpone_offered(read: &Result<State, ReadError>) -> bool`.

- [ ] **Step 1: Write the failing tests** in `shield.rs`:
  - `the_switch_reflects_the_state` and `the_persistent_warning_shows_while_automatic_is_off` (item 17): `Rows.automatic_security_updates == false` for a state with the opt-out on.
  - `postpone_is_offered_only_for_an_unpostponed_security_digest`.
  - `the_postponed_row_carries_the_expiry` (`postponed_until == Some(first + 604_800)`).
  - `every_update1_error_has_its_refusal` extended: `"AlreadyPostponed"` and `"NotSecurityClass"` map to the new variants.
  - `requests_name_their_method` extended: `SetAutomatic(false).method() == "SetAutomaticSecurityUpdates"`.
  - `a_withdrawn_postpone_says_already_postponed` when the state stopped offering it.
- [ ] **Step 2: Run** `forge/test/shell/rig.sh cargo test -p athanor-bar shield`. Expected: FAIL.
- [ ] **Step 3: Implement.** In `ui/shield.rs`: the switch row; turning it off opens the warning of Task 1 point 16 first and calls `SetAutomaticSecurityUpdates(false)` only on confirmation; the persistent warning row while off; the Postpone button beside "Restart to update"; refusals through `refusal_words`. Strings go through gettext and the `po/` files.
- [ ] **Step 4: Run** the Step 2 command and `bash forge/specs/athanor-bar/athanor-bar-1.0.0/po/update.sh`. Expected: tests PASS; `git diff po/` shows the new msgids with EN and IT translations and no other change.
- [ ] **Step 5: Commit** `feat(bar): automatic security updates switch and postpone in the shield (BR6)`.

---

### Task 11: Dev-VM acceptance for items 16 to 19 and doc_shell section 7, item 4

**Files:**
- Modify: `scripts/devvm/acceptance/images.sh` (attest `vsec1`, `vsec2` as security and `v8` as feature with the throwaway key `acc-1`; `vsec-bad` with a broken signature; `vsec-noclass`)
- Modify: `scripts/devvm/acceptance/run.sh` (`STAGES` gains `security postpone optout class securitygreenboot` after `apply`)
- Modify: `scripts/devvm/acceptance/probes.py` and its expectations (schema 2 fields), `scripts/devvm/shield-acceptance.sh` (switch, warning, Postpone button)
- Modify (exact-match script): `docs/operations/release-1.0.md` row 25 status, `docs/architecture/doc_update_trust.md` acceptance items 16 to 19 marked with the run id

**Interfaces:**
- Consumes: everything above, as installed in the image built by the harness.
- Produces: the run log under `.scratch/devvm/acceptance/` that closes #543.

- [ ] **Step 1: Write the stages**, each asserting on the state file and on `bootc status --format json` in the guest:
  - `stage_security` (items 16, 17, doc_shell item 4): on a metered connection, `vsec1` downloads and reads `will-apply-at-next-shutdown`; the notice was sent with "Postpone" and without "Later"; nothing reboots for 10 minutes; a user-started reboot boots `vsec1`; one `GoBack()` returns; then a soft reboot variant (UD37) also applies.
  - `stage_postpone` (item 17): `Postpone()` as the active user without a password; state `downloaded`, `postpone.postponed == true`; a shutdown keeps the old version; a second `Postpone()` and one after `vsec2` is published are refused `AlreadyPostponed` and `first_notice_at` is unchanged; the guest clock moved 7 days ahead, then a shutdown, boots `vsec2` (the shutdown hook of Task 7); after it, a new lineage starts.
  - `stage_optout` (item 17): `SetAutomaticSecurityUpdates(false)` asks for an administrator; a newly published security image stays downloaded and locked; the shield shows the persistent warning; the notice still arrives; on a metered connection nothing downloads; `SetAutomaticSecurityUpdates(true)` unlocks the waiting digest.
  - `stage_class` (item 18): `vsec-bad` and `vsec-noclass` stay `downloaded` and wait for `Apply()`.
  - `stage_securitygreenboot` (item 19): a security image that fails greenboot is returned from; three checks do not download it.
- [ ] **Step 2: Run** `bash scripts/devvm/acceptance/run.sh security postpone optout class securitygreenboot` with one VM, `--cpus 4`. Expected: each stage prints `PASS`. Then run the whole `STAGES` list once. Expected: no stage regresses.
- [ ] **Step 3: Update** row 25 and the acceptance items with the run id, through an exact-match script; `git diff --numstat`.
- [ ] **Step 4: Commit** `test(update): dev-VM acceptance of security-class updates (UT13 items 16-19)`.

---

## Effort (estimate)

| Task | Estimate |
|---|---|
| 1 | 0.5 day of drafting, plus the maintainer's answers |
| 2 | 2 days |
| 3 | 1.5 days, plus one release run the maintainer dispatches |
| 4 | 1.5 days |
| 5 | 1.5 days (many readers) |
| 6 | 2 days |
| 7 | 0.5 day |
| 8 | 1.5 days |
| 9 | 1 day |
| 10 | 1.5 days |
| 11 | 2 to 3 days of dev-VM time |
