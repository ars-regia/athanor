# Release 1.0

| Field | Value |
| --- | --- |
| Purpose | What Athanor 1.0 contains, how each item is accepted, what waits for 1.1, and the order of work |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 3, 2026-10-14: scope from [ADR-0103](../decisions/0103-audit-5-decisions.md) (revision 1, D1), order and release points from [ADR-0105](../decisions/0105-releases-before-1-0.md), amended by [ADR-0115](../decisions/0115-zero-nine-in-one-week.md) (two releases, 0.3 and 0.9, and five rows moved to 1.1) |

## 1. Purpose and date

1.0 is the first release a person outside the project can install and keep. It is narrow on
purpose (D1): a signed image, its update chain, the installer, recovery, one complete desktop
session and encryption. Everything else waits.

The date is set outside the project. Fedora 43 reaches its end of life on 2026-12-02. Fedora 45
final is due on 2026-10-20; if its image is not green by mid-November 2026, the base moves to
Fedora 44 instead ([ADR-0078](../decisions/0078-fedora-release-target.md)). Release 0.9 ships on
Fedora 43; the rebase happens between 0.9 and 1.0 ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
1.0 never ships on Fedora 43.

## 2. In 1.0

1.0 is done when every row holds. An acceptance check written "to add" does not exist yet: the
pull request that builds the row adds it, under that name or a better one.

| Requirement | Owner | Acceptance |
| --- | --- | --- |
| Signed immutable image, three variants: default, `-nvidia`, and `-nvidia-legacy` only on its evidence (D24; the evidence is in 1.1, so the variant stays unpromoted, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md)) | `doc_system_image.md` S1 to S5, `doc_update_trust.md` UT2, UT3 | `system/verify-images.sh --registry REGISTRY/OWNER DIGESTS_FILE`; `python3 -B -m unittest discover -s system/tests`; `python3 scripts/verify.py registry` |
| NVIDIA failures do not block the other images (D24) | `doc_update_delivery.md` UD4 | `system/tests/test_build_variants.py` (image stage); `system/tests/test_kernel_artifacts.py` and `system/tests/test_optional_branch.py` (kernel-module stage) |
| Update chain to `:stable`: the PB5 slice, promotion only on acceptance evidence for the same digests (D15) | `doc_pipeline.md` PB5, `doc_update_delivery.md` UD1 to UD6 | `python3 -B -m unittest discover -s system/tests` (`test_promote.py`: missing evidence, a fail verdict, another digest or another run are refused); `scripts/ci/evidence.py`; `.github/workflows/accept.yml` writes the verdict file; `.github/workflows/promote-stable.yml` |
| One human approval per promotion to `:stable`, in the environment `release`, and `:latest` signed with no approval from step 3 of the image key rotation; agents open, push and merge pull requests as a GitHub App (D2, D25, ADR-0104) | ADR-0103 D2, D25; ADR-0104; `doc_pipeline.md` PL5, PL56, PL60 | `python3 scripts/github-settings/ghsettings.py diff`; `release` in `.github/settings/environments.json`; `.github/workflows/promote-stable.yml` runs `system/promote.sh` only in its job `promote`, in `release` |
| Signed evidence bundle copied off GitHub at promotion (D23) | `doc_pipeline.md` PL42 | `system/evidence-bundle.sh verify`, run by both jobs of `.github/workflows/promote-stable.yml`; `system/tests/test_evidence_bundle.py` |
| Resolved Fedora package set recorded for every build (D16) | `doc_update_delivery.md` UD28 | `system/package-set.sh`, artifact `package-sets`; `system/promote.sh` refuses a promoted image without its set (`test_promote.py` `test_a_promoted_image_without_its_package_set_is_refused`); the set is in the evidence bundle |
| Key hierarchy for the image key; MOK interim key set with not-after and revocation (D5) | `doc_update_trust.md` UT2, [ADR-0084](../decisions/0084-key-custody-model.md) | `scripts/devvm/acceptance/run.sh` (stages `rotate`, `recover`); to add: `cargo test -p athanor-update` cases for an expired and a revoked key |
| Installer path from the ISO to the first session: Anaconda Users and Timezone pages, MOK import in `%post` with a shown one-time password, x86-64-v3 and UEFI check at ISO start (D17) | `doc_platform_experience.md`, `doc_kernel_profile.md` D3, D14 | `forge/test/iso/run_iso_test.sh ISO OUTPUT_DIR`; `.github/workflows/iso-acceptance.yml`; to add: a first-session login after the install, and the ISO test under Secure Boot firmware |
| Greeter: user list, "Other user", PAM conversation by message type (D7) | `doc_lock_and_prompts.md` LP18 step 4 | `cargo test -p athanor-greeter-ui`; to add: a fake-greetd test with an account lockout and an expired password |
| Disk encryption on by default: LUKS2 with a passphrase, TPM and PIN, off only by the person's choice (D6) | `doc_platform_experience.md`, `doc_kernel_profile.md` disk layout | to add: an ISO acceptance case that installs with the default answers and checks LUKS2 |
| Recovery and rollback: text console, GoBack, greenboot | `doc_recovery.md` R1 to R7 | `cargo test -p athanor-update`; `scripts/devvm/acceptance/run.sh` (stage `goback`); `scripts/devvm/switch-acceptance.sh rollback`; to add: acceptance 1 and 7 of `doc_recovery.md` on the dev VM |
| Desktop session tier: session lock and polkit agent as upstream components packaged in Fedora, chosen in the pull request that ships them, confined and checked; screenshot and screen share, bar, dock, launcher; cosmic-idle, cosmic-bg, cosmic-workspaces, cosmic-settings coexist (D13, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md)) | `doc_shell.md` SH3, `doc_lock_and_prompts.md` LP11, `doc_portal.md` PT8, PT9, `doc_settings.md` SE23 | `scripts/devvm/bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `shield-acceptance.sh`; `forge/test/shell/rig.sh`; to add: lock, polkit agent and capture acceptance on the dev VM |
| cosmic-comp carries patches 1 and 2 (D14, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md)) | `doc_compositor.md` CO3 | `scripts/devvm/compositor-acceptance.sh` |
| Application installation: verified Flathub remote, Flatpak update timer, the packaged store (D8) | `doc_software.md` SW15 to SW17 | `python3 scripts/verify.py contacts`; to add: an image check for the remote and the enabled timer |
| athanor-attestation and the mesh and post-quantum residue deleted (D10) | ADR-0103 D10 | `python3 scripts/verify.py shipped coverage`; to add: a check that the crate directory and the mesh and post-quantum modules of `athanor-bus-api` are absent |
| DNS over TLS opportunistic until the captive-portal probe ships, then strict (D11) | [ADR-0079](../decisions/0079-captive-portals-under-strict-dot.md) | `python3 scripts/verify.py contacts`; the image check of the resolver mode (`system/check-image-contacts.sh`) |
| USBGuard class allowlist; confirmation for new external devices once the notice exists (D12) | `doc_disks.md` DK21 | to add: a rule test over the admitted and blocked classes |
| Existing machines with `sshd` enabled keep it; off on new installs (D18) | [ADR-0089](../decisions/0089-defaults-that-contact-or-listen.md) items 2, 3 | to add: a test of both paths, `sshd` enabled and not enabled |
| "Snapshots of your files": restore from the command line, visible failure (D21, first step) | [ADR-0101](../decisions/0101-backup-one-mechanism.md) | `cargo test -p athanor-backup`; to add: a case for a home that cannot be snapshotted |
| Update Apply: `auth_admin` with other sessions, `allow_active` with one; `blocked()` fails closed (D22) | `doc_update_trust.md` UT6, `doc_threat_model.md` TM9 | `python3 scripts/verify.py polkit polkit-model`; `cargo test -p athanor-update`; to add: a case for a failing inhibitor query |
| Services keep least privilege; PAM rules hold | `doc_threat_model.md` TM8, [ADR-0090](../decisions/0090-account-lockout-faillock.md) | `python3 scripts/verify.py services pam` |

## 3. Moved to 1.1

- Machines stages V1 and V2; Nix for every user; the rich notification features (D1).
- Tetragon, step 1 included: the policies and the SELinux CIL type (row 16), and the relay and
  notices (D9) ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- Athanor's own session lock, polkit agent and SystemPrompter (`doc_lock_and_prompts.md` LP18),
  with cosmic-comp patch 5 (the compositor trusted path) and patch 6 (rows 12 and 13), and the
  keyring prompter exchange carried by Athanor (row 14, D20); until then GNOME Keyring keeps
  its own gcr prompter ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- The SearXNG feature (row 21, D19), whose section of `doc_software.md` is not written
  ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- The `-nvidia-legacy` evidence: 20 boot-matrix runs and #231 closed (row 1). D24 keeps the
  variant unpromoted without it ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- The retirement of cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings, and the
  programs that replace them (D13).
- A snapshot and lock of the Fedora input (D16).
- Kernel roles, IPE and AutoFDO (D26).

Later, with no release named: compositor patches 3, 4 and 7 to 10 after a measured rebase drill
(D14); Bazaar after the Fedora 45 rebase (D8); strict DNS over TLS after the probe (D11); the
first-run account helper (D17); the external-disk backup and its restore interface (D21); SBOM
and provenance as promotion conditions (D15). The Bridge (UD45 to UD50) is dropped, not moved
(D4).

## 4. Order of work

Two releases lead to 1.0 ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md), which cut the
four of [ADR-0105](../decisions/0105-releases-before-1-0.md)): 0.3, then 0.9 within one week. The
dates are estimates. Each 0.x release is a signed tag on `iso-v0` plus a `:stable` promotion of
that build on evidence (BRN4 of `branching.md`), with the `release` approval. 1.0 is 0.9 on
Fedora 45 (or 44), rebased, with no blocking defect, before 2026-12-02.

**Freeze (D3), lifted on 2026-10-14** ([ADR-0112](../decisions/0112-pace-to-0-9.md)). P2 of
the update-chain plan (#122) is green; spike S1 (#124) still gates P4b, and both now fall
between 0.9 and 1.0 with the rebase (ADR-0115). Specification revisions and shell work run in
parallel with it, within the machine's CPU budget. The Fedora 45 rebase does not wait for P3
(D26); the Fedora 44 fallback date holds. In the release half of the pipeline the PB5 slice
comes first (D15), then the evidence copy (D23); the key hierarchy (D5) waits for the
maintainer's LUKS2 key backup, which is done in the week of 2026-10-09.

### 4.1 Releases

| Release | Target (estimate) | Contents: rows of section 2 and work outside them | Exit |
| --- | --- | --- | --- |
| 0.3 "chain" | its exit was met on 2026-10-14; tagged when its remaining issues close | rows 1 (image, three variants, without the `-nvidia-legacy` evidence), 2, 3, 4, 5, 6; `release.yml` (PB13, [ADR-0104](../decisions/0104-release-workflow-and-stable-gate.md), merged in #354); P2 (#122) | The first `:stable` promotion on evidence. Existing: `python3 -B -m unittest discover -s system/tests` (`test_promote.py`). `system/promote.sh` reads `:stable` back and refuses unless it is the evidence digest (`test_promote.py` `test_stable_is_read_back`) |
| 0.9 "surface" | about 2026-10-21 (one week) | rows 7, 8, 9, 10, 11, 12, 13 (patches 1 and 2), 15, 17, 18, 19, 20, 22, 23, 24; the start of the image key rotation to key 2; the ISO acceptance under Secure Boot (#252); the hardware work of 4.3; the visual language and accessibility; the maintainer's aesthetic sign-offs (ST8, VL12). It ships on Fedora 43 | The release candidate: every row placed here holds and the hardware matrix is a recorded run. Existing: `scripts/devvm/bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `compositor-acceptance.sh`, `python3 scripts/verify.py services pam polkit polkit-model contacts shipped coverage`, `forge/test/iso/run_iso_test.sh ISO OUTPUT_DIR`, and the unit tests the rows name. To add: the image checks and acceptance cases the rows mark "to add", the ISO test under Secure Boot firmware ending in an update to a newer `:stable`, and the hardware matrix |
| 1.0 | before 2026-12-02 | 0.9 rebased onto Fedora 45, or Fedora 44 per [ADR-0078](../decisions/0078-fedora-release-target.md), with S1 (#124) and P4b (#126), the bootc two-step update chain; the close of the image key rotation (PB5b), which needs a key-2 release on `:stable` and one more release; no blocking defect | To add: a query for open blocking issues that must return none |

Row numbers are those of revision 2 of this file, so that issues and records that cite them
stay valid: 1 signed immutable image; 2 NVIDIA failures; 3 update chain to `:stable`; 4 one
human approval per promotion to `:stable`; 5 evidence bundle; 6 resolved package set; 7 key
hierarchy and MOK key set; 8 installer path; 9 greeter; 10 disk encryption; 11 recovery and
rollback; 12 desktop session tier; 13 cosmic-comp patches; 14 keyring prompter; 15 application
installation; 16 Tetragon; 17 attestation deleted; 18 DNS over TLS; 19 USBGuard; 20 `sshd`; 21
SearXNG; 22 snapshots; 23 update Apply; 24 least privilege and PAM. Rows 14, 16 and 21 are in
section 3 and no longer in section 2. Every other row is in exactly one release, except the
three split below.

### 4.2 Rows that span releases

- **Row 12, desktop session tier.** For 0.9 the session lock and the polkit agent are upstream
  components packaged in Fedora, confined and checked; the pull request that ships them
  chooses the components. Screenshot and screen share, bar, dock, launcher and the coexistence
  of cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings are the graphical surface.
  All of this is 0.9. Athanor's own lock, polkit agent and SystemPrompter
  (`doc_lock_and_prompts.md` LP18) are in 1.1 (section 3).
- **Row 13, cosmic-comp patches.** Patches 1 and 2 are 0.9. Patches 5 and 6 are in 1.1, so 1.0
  has no compositor trusted path for the lock and the prompts (ADR-0115).
- **Row 1, signed immutable image.** The three variants build in 0.3, and the default and
  `-nvidia` images promote; the `-nvidia-legacy` evidence (20 boot-matrix runs, #231 closed) is
  in 1.1. Until then `-nvidia-legacy` is not promoted (D24).
- **Row 11, recovery and rollback.** The dev VM acceptance and `switch-verified.sh` on the
  desktop and the laptop are both 0.9; the second belongs to the hardware work of 4.3.

### 4.3 Hardware work at 0.9

One session, not one per release (ADR-0105), and it needs the maintainer in the week of 0.9:
`switch-verified.sh` on the desktop and the laptop, the NVIDIA desktop for the `-nvidia`
image, Wi-Fi and Bluetooth on hardware, and the final hardware matrix. The `-nvidia-legacy`
evidence and #231 are not part of it; they are in 1.1. Release 0.3 is accepted on the dev VM
and in CI; what it leaves unproven on hardware is exactly this list.

## 5. How this file changes

Only a decision record changes the scope of 1.0, and only a decision record changes its order
or its release points: [ADR-0105](../decisions/0105-releases-before-1-0.md) set four
releases and [ADR-0115](../decisions/0115-zero-nine-in-one-week.md) cut them to the two of
section 4. A pull request that adds, removes or moves a
row of sections 2 or 3 names the record that decided it; a pull request that only turns a
"to add" into an existing check, or corrects a path, needs none.
