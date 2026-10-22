# Image Key Rotation and Key Hierarchy (Row 7, Releases 0.10 and 1.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task; the tasks are sequential and most stop at a maintainer checkpoint. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Row 7 of `docs/operations/release-1.0.md`, split by the maintainer's decision of 2026-10-22 into two parts. **Part A, release 0.10 "base":** 0.10 carries key 2 as a plain key, by the rotation of `doc_update_trust.md` UT3 from key 1 to key 2 (`docs/operations/secrets.md` section 4.1 steps 2 and 3; key 2 is already in `signing-images`), with the LUKS2 key backup it depends on. **Part B, release 1.0:** the key hierarchy of ADR-0103 D5 (the offline root, short-lived online keys, their not-after date and revocation), the not-after date and revocation of the MOK interim key set, and the close of the rotation (`doc_pipeline.md` PB5b, step 4 of section 4.1). The row's check, `cargo test -p athanor-update` cases for an expired and a revoked key, exists when part B is done.

**Why the split** (maintainer, 2026-10-22): the containers/image `policy.json` that UT3 uses has no field for a not-after date or a revocation, so the hierarchy needs an approved amendment of UT2, new verification code and a key ceremony, which do not fit before the Fedora 44 decision point of 2026-11-16.

**Architecture:** Nothing in this plan designs cryptography. Part A takes the rotation of UT3 and `secrets.md` section 4.1 as written: key n+1 ships in the policy of an image signed with key n, and key n leaves one release after the first image signed with n+1; no verification code changes. Part B: ADR-0103 D5 and UT2 state what the hierarchy is: an offline root, kept in the custody kit of ADR-0084, signs a short-lived online signing key with a not-after date and a timestamp; machines pin the root, not the online key; the root revokes an online key, and a machine stops trusting one at its not-after date at the latest; for the Machine Owner Key the interim is a signed key set with a not-after date and revocation; the out-of-band recovery of UT2 (`forge/specs/athanor-update/RECOVERY.md`) stays the path for a compromised root. UT2 leaves the mechanism to "the revision that builds it", which specifies how the policy of UT3 and the service read it, with acceptance cases for an expired and a revoked key (ADR-0113, ADR-0115 item 6). Task B1 writes that revision; every later task of part B takes its formats and states from it.

**Tech Stack:** Rust 2021 (`athanor-update`: p256 ECDSA, serde, chrono), Bash for `system/` and the dev-VM harness, cosign and skopeo for signing, containers/image policy files, GitHub environments.

**Spec:** `docs/architecture/doc_update_trust.md` revision 4 (UT2, UT3, UT5, the key succession of section 4, acceptance items 4, 6 and 10); `docs/decisions/0103-audit-5-decisions.md` D5 and D25; `docs/decisions/0084-key-custody-model.md`; `docs/decisions/0113-update-trust-software-approval.md`; `docs/decisions/0115-zero-nine-in-one-week.md` item 6; `docs/decisions/0123-rebase-release-0-10-rows-7-and-25.md` items 2 and 4; `docs/operations/release-1.0.md` sections 4.2 and 5; `docs/architecture/doc_pipeline.md` PB5b and PL18; `docs/operations/secrets.md` sections 1.4, 2 (KC1 to KC8), 3 (RL5, RL8) and 4.1; `docs/architecture/doc_kernel_profile.md` D41 and its key lifecycle table (MOK).

## Global Constraints

- English for everything committed, enterprise tone, Conventional Commits; no attribution, no model names, no co-author trailers.
- Base every worktree on `origin/iso-v0` and report its commit; never push to `iso-v0` or `main`; one logical change per commit.
- Stop-and-ask areas (`AGENTS.md`): signing, keys, Secure Boot and MOK (`forge/specs/azoth/keys/`, `system/keys/`, `system/sign-images.sh`, `system/promote.sh`, `forge/scripts/sign_attest.sh`, jobs that use `signing-kernel`, `signing-images` or `signing`), and the verification path of `athanor-update`. Every task that touches one of them starts with a **maintainer checkpoint** step and does not go past it until the maintainer approves in the conversation.
- The agent never sees, lists or reads key material: no file under `forge/specs/azoth/keys/` or `system/keys/` is opened, no private key, no `.env*`, no credential store. Key generation, the backup and `gh secret set` are the maintainer's, offline (`doc_update_trust.md` D3, `secrets.md` KC3). Tests use throwaway keys derived inside the test, as `check.rs` `key_lifecycle` does, or created and deleted by `tests/vectors/make.sh`.
- No fake security: a not-after date or a revocation is checked by real signature verification over the root's statement, or the key is not trusted. Never an unconditional `Ok`.
- No `|| true`, no `continue-on-error`, no `2>/dev/null` that hides an error; no new `unwrap()`, `expect()` or `panic!` outside tests (`verify.py panics`).
- Docs under `docs/`, `scripts/verify.py` and `forge/config/packages.json` change only through an exact-match replacement script, then `git diff --numstat`.
- Builds at most 4 CPU threads (`taskset -c 0-3 cargo test -j 4`), cargo targets under `/var/tmp`, and only with at least 6 GiB of `MemAvailable`; one VM at a time.
- **Part A is ready** (maintainer's decision of 2026-10-22) when Tasks A1 to A4 are done, the release steps of Tasks A3 and A4 are merged, the dev-VM acceptance of Task A5 is green on the 0.10 candidate, and the 0.10 release run signs with key 2. The rebase to Fedora 45 does not wait for it (ADR-0123 item 4): if part A is not ready at the Fedora 44 decision point of ADR-0078, 2026-11-16 (`docs/superpowers/plans/2026-10-18-fedora-45-rebase.md`, M5), 0.10 ships without it and a pull request naming ADR-0123 moves it to the release that follows; key 2 moves with it (`release-1.0.md` section 4.2).

## State on 2026-10-22

- `cargo test -p athanor-update key_lifecycle` (commit `test(update): image key rotation and removal from the policy (row 7)`), with throwaway keys derived in the test:
  - a key the policy names at the check verifies the booted image (`verified`, reason `signature`) and its release attestation (security class): UT2, UT5;
  - during a rotation either key in the policy verifies: UT3;
  - a key removed from the policy verifies nothing it signed before (`verified` false, reason `key-not-in-policy`; the attestation reads as feature class): UT2, UT3. This is the only withdrawal of trust UT2 defines while the hierarchy does not exist, and the one part A relies on.
- Not written: a key past its not-after date, and a key revoked by the root (part B). See "Gap in UT2".
- Key 2 exists: it was generated into the KC2 backup, `signing-images` holds it as SEC3 and SEC4, and its public half is committed as `system/keys/athanor-image-2.pub` (`docs/operations/secrets.md` section 4.1 step 1; `git ls-files system/keys`). It is used as it is and never regenerated.
- The LUKS2 key backup of KC2 is not made yet (maintainer, 2026-10-22). Task A2 makes it.

## Gap in UT2 (part B, Task B1)

UT2 does not define enough to test an expired or a revoked key without guessing:

1. **Where the not-after date is read.** UT2 says the root signs the online key "with a not-after date, and a timestamp". It names no file, format or field. The policy of UT3 lists keys as `sigstoreSigned` requirements with `keyPaths`, and that requirement carries no not-after date and no revocation list; `athanor-update` reads keys from the same `keyPaths` (`policy.rs` `scopes`, `check.rs` `policy_keys`).
2. **Where the revocation is read**, and how a machine receives a revocation the root signed after its image was built: UT2 says only "the root revokes an online key".
3. **Which clock decides.** "Stops trusting one at its not-after date at the latest" leaves open whether the date is compared with the check time (`Context::now`), the image's build time, or the timestamp the root signed, and what a machine whose clock is wrong does.
4. **What the result is.** No reason of the closed list of the trust state (`athanor-trust-state` `Reason`) and no `ErrorCode` names an expired or a revoked key. Today a key outside the policy gives `key-not-in-policy` for the booted image and `refused` with the error code `policy` for a download (acceptance item 4); UT2 does not say whether an expired or revoked key gives the same states or new ones.
5. **Who enforces it.** The download gate is containers/image, through `bootc --enforce-container-sigpolicy` and the policy of UT3; `athanor-update` recomputes "verified" (UT5) and the class of the attestation (UT13) afterwards. UT2 does not say whether an expired or revoked key must already be refused by the download gate, or only by `athanor-update`.
6. **The MOK interim key set.** "A signed key set with a not-after date and revocation" names no signer, format, reader (shim, the kernel, a userspace check) or result.

Task B1 closes these points; Task B3 turns each answer into a test before any code.

## Review Focus

1. **The rotation order** (secrets.md section 4.1): a machine that skipped the transitional release is moved with `athanor-update recover-key`, and the dev-VM stages `rotate` and `recover` prove it on the 0.10 candidate. Test in Task A5.
2. **A machine whose clock is wrong** (RTC reset to 1970, set years ahead). Expected: whatever Task B1 decides, a clock set back never makes an expired key trusted again without the root's statement, and the result is a named state, not a silent pass. Test in Task B3.
3. **A revocation that reaches the machine after the image it revokes was staged.** Expected per Task B1; the staged deployment is not applied on a key the machine knows is revoked. Test in Task B3.

---

## Part A: release 0.10, the rotation to key 2

### Task A1: Split row 7 in the release plan (maintainer checkpoint: approved document, release points)

**Files:**
- Modify (exact-match script): `docs/operations/release-1.0.md` (row 7 of section 2, the 0.10 row of section 4.1, the row 7 entry of section 4.2)
- Create, if open question 3 asks for it: a decision record under `docs/decisions/` that records the maintainer's decision of 2026-10-22. `release-1.0.md` section 5 lets only a decision record change a release point, and the split amends ADR-0123 item 2 and may touch ADR-0103 D25.

- [ ] **Step 1: Maintainer checkpoint.** Draft the exact text in this task, not before: the 0.10 row is the key-2 rotation under UT3 (check: the dev-VM stages `rotate` and `recover`, `system/verify-images.sh` with key 2 alone); the 1.0 row is the hierarchy, the MOK key set's not-after date and revocation, and the PB5b close (check: "to add: `cargo test -p athanor-update` cases for an expired and a revoked key"). Show the draft and stop until the maintainer approves it.
- [ ] **Step 2: Write** the approved text with an exact-match replacement script under `.scratch/`; the pull request names ADR-0123.
- [ ] **Step 3: Check.** Run `python3 scripts/verify.py docs decisions` and `git diff --numstat`. Expected: both pass; only the replaced lines changed.
- [ ] **Step 4: Commit** `docs(release): split row 7, key 2 in 0.10 and the key hierarchy in 1.0 (ADR-0123)`. The pull request needs the maintainer's approval.

### Task A2: Make and prove the LUKS2 key backup (maintainer only)

UT2 and ADR-0103 D5: the hierarchy depends on the LUKS2 backup of the keys, and the rotation needs it verified (maintainer, 2026-10-22). It comes before any ceremony and before Task A3.

- [ ] **Step 1: Maintainer, offline.** Make both drives of `secrets.md` KC2: each a LUKS2 volume holding the private keys and passphrases of section 1.4 that KC2 lists, key 2 and its password among them, and image key 1 archived as section 4.1 step 1 describes, with the KC7 fingerprint inventory. If a key KC2 lists cannot be written to the drives, stop and tell the agent which, by name only.
- [ ] **Step 2: Maintainer.** Open each drive on another machine than the one that wrote it, with the passphrase typed, and compare each file by SHA-256 with the inventory (KC3, KC7).
- [ ] **Step 3: Maintainer.** Write the KC7 inventory and record the date in the private custody log; place the drives as KC2 says and write the sealed letter of KC8. Nothing is committed.
- **Check:** the maintainer states in the conversation that both drives opened on another machine and matched the inventory. No later task starts without it.

### Task A3: PB5b step 2, the transitional release (maintainer: `signing`, `release`)

`secrets.md` section 4.1 step 2: a release that carries key 2 in its policy, signed with key 1, is promoted to `:stable`, and the machines meant to keep updating without recovery boot it.

- [ ] **Step 1: Maintainer checkpoint.** Confirm which release is the transitional one (open question 2) and that Tasks A1 and A2 are done.
- [ ] **Step 2: Maintainer.** Approve `sign-images` in `signing` and the promotion in `release`: `gh run list -R ars-regia/athanor --workflow release.yml --limit 3`, then the approval in the run's page, and `gh workflow run promote-stable.yml -R ars-regia/athanor` per `promote-stable.yml`'s inputs.
- [ ] **Step 3: Maintainer decides** that the machines meant to keep updating have booted it (`promote.sh` cannot tell, section 4.1 step 2).
- **Check:** `system/verify-images.sh --registry REGISTRY/OWNER DIGESTS_FILE` on the promoted digests.

### Task A4: PB5b step 3, sign with key 2 (maintainer checkpoint: signing)

ADR-0123 item 2: 0.10 is the key-2 release on `:stable`.

- [ ] **Step 1: Maintainer checkpoint.** Name the edits: `sign-images` moves to `signing-images` in `.github/workflows/release.yml`; `system/sign-images.sh` verifies against key 2 alone; `.github/settings/environments.json`. Stop until approved.
- [ ] **Step 2: Write the failing tests** in `system/tests/test_sign_images.py` (an image signed with key 1 fails `sign-images.sh`'s own verification) and `scripts/tests/test_verify_signing.py`.
- [ ] **Step 3: Implement, run** `python3 -B -m unittest discover -s system/tests`, `python3 scripts/verify.py workflows`, `python3 scripts/github-settings/ghsettings.py diff`. Expected: PASS, and the diff shows only the environment change.
- [ ] **Step 4: Commit** `ci(release): sign the system images with key 2 in signing-images (secrets.md 4.1 step 3)`.

### Task A5: Acceptance on the 0.10 candidate and the 0.10 release run

- [ ] **Step 1: Run** `scripts/devvm/acceptance/run.sh` (stages `rotate` and `recover`: acceptance items 6 and 10 of `doc_update_trust.md`) on the 0.10 candidate, one VM, no runner build beside it, and `CARGO_TARGET_DIR=/var/tmp/athanor-wt/target-row7 taskset -c 0-3 cargo test -j 4 -p athanor-update key_lifecycle`. Expected: every stage prints `PASS`, and the three cases pass.
- [ ] **Step 2: Maintainer.** The 0.10 release run signs in `signing-images` with key 2 and no reviewer (KC1); the promotion of 0.10 to `:stable` takes the `release` approval.
- **Check:** `system/verify-images.sh` with key 2 alone on the 0.10 digests; the evidence of UD4 on the same digests.

## Part B: release 1.0, the key hierarchy

### Task B1: Amend UT2 with the mechanism of the hierarchy (maintainer checkpoint: approved document, signing, keys)

**Files:**
- Modify (exact-match script): `docs/architecture/doc_update_trust.md` (UT2, UT3, section 6), `docs/operations/secrets.md` (sections 1.4, 2, 4.1), `docs/architecture/doc_kernel_profile.md` (the MOK rows of the key lifecycle table, D41) if the MOK key set is specified there
- Create: a decision record under `docs/decisions/` if the amendment changes a decision

**Decided input** (maintainer, 2026-10-22): an online key lives 180 days; the maintainer renews it with the offline root at 90 days.

- [ ] **Step 1: Maintainer checkpoint.** Present the six points of "Gap in UT2" and open question 1. Write nothing until the maintainer has answered each in the conversation. The amendment takes the answers and the decided input verbatim; it does not choose a format for the maintainer.
- [ ] **Step 2: Write the amendment** with an exact-match replacement script under `.scratch/`: the online key's lifetime and renewal, the role of key 2, where the not-after date and the revocation are read, which clock decides, the reason and error code of each outcome, who enforces it, the MOK key set, and acceptance items for an expired key, a revoked key and a key valid at the check time.
- [ ] **Step 3: Check.** Run `python3 scripts/verify.py docs decisions` and `git diff --numstat`. Expected: both pass; only the replaced lines changed.
- [ ] **Step 4: Commit** `docs(update): specify the image key hierarchy and the MOK key set (UT2)`. The pull request needs the maintainer's approval as any approved-document change does; no later task of part B starts before it merges (ADR-0115 item 6).

### Task B2: Key ceremony for the offline root and the first online key (maintainer only: keys, signing)

Key 2 is used as it is and never regenerated (`secrets.md` section 4.1 step 1). What is new is what Task B1 and open question 1 decide: expected, the offline root and the first online key. Needs the backup of Task A2.

- [ ] **Step 1: Maintainer checkpoint.** Name what is generated, from Task B1's text and the answer to open question 1, and the role key 2 takes. Stop until approved.
- [ ] **Step 2: Maintainer, offline.** Generate on tmpfs with the preamble of `secrets.md` section 1.4 and follow KC3 in order: write each key to both drives, close, unplug, reopen with the passphrase, compare by SHA-256; derive each public key (`cosign public-key --key <file>` with `COSIGN_PASSWORD`) and compare it with the file to be committed; sign and verify a test blob. Only then, and only for a new key that Task B1 says signs in CI, `gh secret set COSIGN_PRIVATE_KEY --env signing-images --repo ars-regia/athanor < <file>` and `gh secret set COSIGN_PASSWORD --env signing-images --repo ars-regia/athanor`; then shred the plaintext. If key 2 stays the signing key, nothing is uploaded. The root's private half is never uploaded (UT2: offline root).
- [ ] **Step 3: Maintainer.** Record the fingerprints in the KC7 inventory, and renew the sealed letter of KC8 if a drive moved. Put the renewal of the online key, at 90 days with the offline root, in the maintainer's calendar; each renewal repeats Steps 2 and 3 for the new online key.
- [ ] **Step 4: Commit** the public halves only, in the place Task B1 names (`system/keys/` today), as `feat(keys): add the image key root (UT2)`, with the names Task B1 gives. The maintainer commits or names the files; the agent does not open them.
- **Check:** `gh api repos/ars-regia/athanor/environments/signing-images/secrets --jq '.secrets[].name'` lists `COSIGN_PRIVATE_KEY` and `COSIGN_PASSWORD`; `python3 -B -m unittest discover -s forge/specs/athanor-update/tests` (`test_render_policy.py`) and `python3 scripts/verify.py shipped` pass with the new public files; key 2's fingerprint in the KC7 inventory is unchanged.

### Task B3: Read the not-after date and the revocation in `athanor-update` (maintainer checkpoint: verification path)

**Files:** from Task B1; expected `forge/specs/athanor-update/athanor-update-1.0.0/src/policy.rs`, `check.rs`, `sigobj.rs`, `attestation.rs`, and `athanor-trust-state` if a reason is added.

- [ ] **Step 1: Maintainer checkpoint.** Name the files and the rules from Task B1. Stop until approved: this is the verification path.
- [ ] **Step 2: Write the failing tests** in `check.rs` `key_lifecycle`, beside the three of 2026-10-22, with keys derived in the test: an image and an attestation signed with an online key past its not-after date; one signed with a revoked key; the boundary, a key whose not-after date is the check time and one second after it (the inclusive side as Task B1 states); a root statement that does not verify; and each clock case of Review Focus 2. Each test name cites the UT2 item; each asserts the reason and error code Task B1 names.
- [ ] **Step 3: Run** `CARGO_TARGET_DIR=/var/tmp/athanor-wt/target-row7 taskset -c 0-3 cargo test -j 4 -p athanor-update key_lifecycle`. Expected: the new cases FAIL, the three existing ones PASS.
- [ ] **Step 4: Implement** exactly the rules of Task B1.
- [ ] **Step 5: Run** the Step 3 command, `cargo test -j 4 -p athanor-update -p athanor-trust-state`, `cargo clippy -p athanor-update --all-targets -- -D warnings`, `python3 scripts/verify.py panics`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(update): stop trusting an expired or revoked image key (UT2)`. Then update the check column of the 1.0 row of `release-1.0.md` that Task A1 writes: the cases exist, "to add" goes.

### Task B4: Sign under the hierarchy (maintainer checkpoint: signing)

Only if Task B1 adds something the release must publish (a root-signed statement of the online key, a revocation list).

- [ ] **Step 1: Maintainer checkpoint.** Name `system/sign-images.sh`, `system/verify-images.sh`, the `sign-images` job of `.github/workflows/release.yml`, and the D43 lint of `scripts/verify.py`. Stop until approved. No step uses a key; the first real signature happens in a release run the maintainer dispatches.
- [ ] **Step 2: Write the failing tests** in `system/tests/test_sign_images.py` and `scripts/tests/test_verify_signing.py` for what Task B1 requires.
- [ ] **Step 3: Implement**, keeping UT2's rule that the key is alone in its job.
- [ ] **Step 4: Run** `python3 -B -m unittest discover -s system/tests`, `python3 -B -m unittest discover -s scripts/tests`, `python3 scripts/verify.py workflows`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(release): publish the online key statement of the image key hierarchy (UT2)`.

### Task B5: The MOK interim key set (maintainer checkpoint: Secure Boot, MOK, keys)

- [ ] **Step 1: Maintainer checkpoint.** Name the files Task B1 points at (expected under `forge/specs/azoth/` and `doc_kernel_profile.md` D41). Stop until approved. The agent does not open `forge/specs/azoth/keys/`.
- [ ] **Step 2: Write the failing tests** for an expired and a revoked entry of the key set, with throwaway certificates generated in the test.
- [ ] **Step 3: Implement** exactly the rules of Task B1.
- [ ] **Step 4: Run** the checks `forge/specs/azoth/AGENTS.md` names for the area. Expected: PASS.
- [ ] **Step 5: Commit** `feat(kernel): MOK key set with a not-after date and revocation (D5)`.

### Task B6: Dev-VM acceptance for an expired and a revoked key

- [ ] **Step 1: Add stages** to `scripts/devvm/acceptance/run.sh` for the acceptance items Task B1 adds (an expired key, a revoked key), with a throwaway key pair made in the harness as stages `rotate` and `recover` do today.
- [ ] **Step 2: Run** `scripts/devvm/acceptance/run.sh` (stages `rotate`, `recover` and the new ones) on the 1.0 candidate, one VM, no runner build beside it. Expected: every stage prints `PASS`.
- [ ] **Step 3: Commit** `test(update): dev-VM acceptance for an expired and a revoked image key (UT2)`.

### Task B7: PB5b step 4 and the close, with 1.0

`doc_pipeline.md` PB5b: a release signed with key 2 alone was promoted to `:stable` and one more release followed; `athanor-image-1.pub` is gone from `system/keys/`; the live environments and `environments.json` list `signing-kernel`, `signing-images` and `release` only; `MOK_PRIVATE_KEY` is absent. ADR-0123 item 2: PB5b closes with 1.0.

- [ ] **Step 1: Maintainer checkpoint.** Removing key 1 and deleting `signing` are signing and destructive changes. Stop until approved.
- [ ] **Step 2: Remove** key 1 from the policy (the maintainer removes `system/keys/athanor-image-1.pub` or names the commit), drop the `signing` alias of `scripts/verify.py`, and update `environments.json`; commit `chore(keys): retire image key 1 (secrets.md 4.1 step 4, PB5b)`.
- [ ] **Step 3: Maintainer.** Delete the environment: `gh api -X DELETE repos/ars-regia/athanor/environments/signing`.
- **Check:** `python3 scripts/github-settings/ghsettings.py diff` reports no drift; `gh api repos/ars-regia/athanor/environments --jq '.environments[].name'` prints `signing-kernel`, `signing-images`, `release` and the non-signing environments only; `gh secret list -R ars-regia/athanor --env signing-kernel` has no `MOK_PRIVATE_KEY`; `python3 scripts/verify.py shipped` and the `rotate` stage pass with key 2 alone.

## Open questions for the maintainer

1. **What role does key 2 take in the 1.0 hierarchy?** (Task B1.) Key 2 exists and is not regenerated: `secrets.md` 4.1 makes it a plain cosign key in `signing-images`, listed in `keyPaths`. UT2 has machines pin an offline root and trust short-lived online keys under it. Is key 2 the root, the first online key, or a plain key that the hierarchy replaces?
2. **Which release is the transitional one for part A** (Task A3): signed with key 1, carrying key 2 in its policy, promoted to `:stable` and booted before 0.10 is signed with key 2. Is it 0.9, if its policy already names key 2, or a release cut for the purpose before 0.10?
3. **ADR-0103 D25 and the record for the split** (Task A1). D25 says "No transitional release signed with key 2 is cut before the key hierarchy of D5 exists", and `doc_pipeline.md` repeats it; part A signs 0.10 with key 2 before the hierarchy. `release-1.0.md` section 5 lets only a decision record change a release point. Does Task A1 carry a record that amends D25 and ADR-0123 item 2, or is D25 read otherwise?
4. **The six points of "Gap in UT2"** (Task B1): where the not-after date and the revocation are read, which clock decides, the reason and error code of each outcome, whether the containers/image download gate or only `athanor-update` enforces them, and the MOK key set's signer, format and reader.
