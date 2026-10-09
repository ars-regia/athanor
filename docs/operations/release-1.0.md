# Release 1.0

| Field | Value |
| --- | --- |
| Purpose | What Athanor 1.0 contains, how each item is accepted, what waits for 1.1, and the order of work |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 2, 2026-10-09: scope from [ADR-0103](../decisions/0103-audit-5-decisions.md) (revision 1, D1), order and release points from [ADR-0105](../decisions/0105-releases-before-1-0.md) |

## 1. Purpose and date

1.0 is the first release a person outside the project can install and keep. It is narrow on
purpose (D1): a signed image, its update chain, the installer, recovery, one complete desktop
session and encryption. Everything else waits.

The date is set outside the project. Fedora 43 reaches its end of life on 2026-12-02. Fedora 45
final is due on 2026-10-20; if its image is not green by mid-November 2026, the base moves to
Fedora 44 instead ([ADR-0078](../decisions/0078-fedora-release-target.md)). 1.0 never ships on
Fedora 43.

## 2. In 1.0

1.0 is done when every row holds. An acceptance check written "to add" does not exist yet: the
pull request that builds the row adds it, under that name or a better one.

| Requirement | Owner | Acceptance |
| --- | --- | --- |
| Signed immutable image, three variants: default, `-nvidia`, and `-nvidia-legacy` only on its evidence (D24) | `doc_system_image.md` S1 to S5, `doc_update_trust.md` UT2, UT3 | `system/verify-images.sh --registry REGISTRY/OWNER DIGESTS_FILE`; `python3 -B -m unittest discover -s system/tests`; `python3 scripts/verify.py registry`; to add: 20 boot-matrix runs for `-nvidia-legacy` and #231 closed |
| NVIDIA failures do not block the other images (D24) | `doc_update_delivery.md` UD4 | to add: a pipeline test in which a failed legacy module removes only `-nvidia-legacy` from the run |
| Update chain to `:stable`: the PB5 slice, promotion only on acceptance evidence for the same digests (D15) | `doc_pipeline.md` PB5, `doc_update_delivery.md` UD1 to UD6 | `python3 -B -m unittest discover -s system/tests` (`test_promote.py`); `.github/workflows/promote-stable.yml`; to add: `accept.yml` verdict file, and a `promote.sh` refusal without it |
| One human approval per promotion to `:stable`, in the environment `release`, and `:latest` signed with no approval from step 3 of the image key rotation; agents open, push and merge pull requests as a GitHub App (D2, D25, ADR-0104) | ADR-0103 D2, D25; ADR-0104; `doc_pipeline.md` PL5, PL56, PL60 | `python3 scripts/github-settings/ghsettings.py diff`; to add: `release` in `.github/settings/environments.json` |
| Signed evidence bundle copied off GitHub at promotion (D23) | `doc_pipeline.md` PL42 | to add: the promotion job verifies the off-GitHub copy against its signature |
| Resolved Fedora package set recorded for every build (D16) | `doc_update_delivery.md` UD28 | to add: the resolved package list in the evidence bundle |
| Key hierarchy for the image key; MOK interim key set with not-after and revocation (D5) | `doc_update_trust.md` UT2, [ADR-0084](../decisions/0084-key-custody-model.md) | `scripts/devvm/acceptance/run.sh` (stages `rotate`, `recover`); to add: `cargo test -p athanor-update` cases for an expired and a revoked key |
| Installer path from the ISO to the first session: Anaconda Users and Timezone pages, MOK import in `%post` with a shown one-time password, x86-64-v3 and UEFI check at ISO start (D17) | `doc_platform_experience.md`, `doc_kernel_profile.md` D3, D14 | `forge/test/iso/run_iso_test.sh ISO OUTPUT_DIR`; `.github/workflows/iso-acceptance.yml`; to add: a first-session login after the install, and the ISO test under Secure Boot firmware |
| Greeter: user list, "Other user", PAM conversation by message type (D7) | `doc_lock_and_prompts.md` LP18 step 4 | `cargo test -p athanor-greeter-ui`; to add: a fake-greetd test with an account lockout and an expired password |
| Disk encryption on by default: LUKS2 with a passphrase, TPM and PIN, off only by the person's choice (D6) | `doc_platform_experience.md`, `doc_kernel_profile.md` disk layout | to add: an ISO acceptance case that installs with the default answers and checks LUKS2 |
| Recovery and rollback: text console, GoBack, greenboot | `doc_recovery.md` R1 to R7 | `cargo test -p athanor-update`; `scripts/devvm/acceptance/run.sh` (stage `goback`); `scripts/devvm/switch-acceptance.sh rollback`; to add: acceptance 1 and 7 of `doc_recovery.md` on the dev VM |
| Desktop session tier: lock, polkit agent, SystemPrompter, screenshot and screen share, bar, dock, launcher; cosmic-idle, cosmic-bg, cosmic-workspaces, cosmic-settings coexist (D13) | `doc_shell.md` SH3, `doc_lock_and_prompts.md` LP11, LP18, `doc_portal.md` PT8, PT9, `doc_settings.md` SE23 | `scripts/devvm/bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `shield-acceptance.sh`; `forge/test/shell/rig.sh`; to add: lock, polkit agent, prompter and capture acceptance on the dev VM |
| cosmic-comp carries patches 1, 2, 5 and 6 (D14) | `doc_compositor.md` CO3 | `scripts/devvm/compositor-acceptance.sh`; to add: a containment test for patch 5 |
| Keyring prompter exchange carried by Athanor (D20) | `doc_lock_and_prompts.md` LP12 | to add: SystemPrompter acceptance on the dev VM |
| Application installation: verified Flathub remote, Flatpak update timer, the packaged store (D8) | `doc_software.md` SW15 to SW17 | `python3 scripts/verify.py contacts`; to add: an image check for the remote and the enabled timer |
| Tetragon: step 1, three or four journal-only policies, SELinux CIL type (D9) | `doc_tetragon.md` TG10, TG11 | `python3 scripts/verify.py services`; to add: an image check that asserts the daemon's real state |
| athanor-attestation and the mesh and post-quantum residue deleted (D10) | ADR-0103 D10 | `python3 scripts/verify.py shipped coverage`; to add: a check that the crate directory and the mesh and post-quantum modules of `athanor-bus-api` are absent |
| DNS over TLS opportunistic until the captive-portal probe ships, then strict (D11) | [ADR-0079](../decisions/0079-captive-portals-under-strict-dot.md) | `python3 scripts/verify.py contacts`; to add: an image check of the resolver mode |
| USBGuard class allowlist; confirmation for new external devices once the notice exists (D12) | `doc_disks.md` DK21 | to add: a rule test over the admitted and blocked classes |
| Existing machines with `sshd` enabled keep it; off on new installs (D18) | [ADR-0089](../decisions/0089-defaults-that-contact-or-listen.md) items 2, 3 | to add: a test of both paths, `sshd` enabled and not enabled |
| SearXNG feature (D19) | `doc_software.md` (section to write) | `python3 scripts/verify.py contacts`; to add: a feature test once the section exists |
| "Snapshots of your files": restore from the command line, visible failure (D21, first step) | [ADR-0101](../decisions/0101-backup-one-mechanism.md) | `cargo test -p athanor-backup`; to add: a case for a home that cannot be snapshotted |
| Update Apply: `auth_admin` with other sessions, `allow_active` with one; `blocked()` fails closed (D22) | `doc_update_trust.md` UT6, `doc_threat_model.md` TM9 | `python3 scripts/verify.py polkit polkit-model`; `cargo test -p athanor-update`; to add: a case for a failing inhibitor query |
| Services keep least privilege; PAM rules hold | `doc_threat_model.md` TM8, [ADR-0090](../decisions/0090-account-lockout-faillock.md) | `python3 scripts/verify.py services pam` |

## 3. Moved to 1.1

- Machines stages V1 and V2; Nix for every user; the rich notification features (D1).
- The Tetragon relay and notices (D9).
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

Four releases lead to 1.0 ([ADR-0105](../decisions/0105-releases-before-1-0.md)): the work
under the hood comes first and the graphical work last. The dates are estimates. Each 0.x
release is a signed tag on `iso-v0` plus a `:stable` promotion of that build on evidence
(BRN4 of `branching.md`), with the `release` approval. 1.0 is 0.9 with no blocking defect,
before 2026-12-02.

**Freeze (D3), in force.** No new specification revision and no non-critical shell work until
P2 of the update-chain plan (the kernel build profile and boot matrix, #122) and spike S1
(#124) are green. They run in parallel inside 0.3, within the machine's CPU budget; S1 is
time-boxed. The Fedora 45 rebase does not wait for P3 (D26); the Fedora 44 fallback date
holds. In the release half of the pipeline the PB5 slice comes first (D15), then the evidence
copy (D23); the key hierarchy (D5) waits for the maintainer's LUKS2 key backup, which is done
in the week of 2026-10-09.

### 4.1 Releases

| Release | Target (estimate) | Contents: rows of section 2 and work outside them | Exit |
| --- | --- | --- | --- |
| 0.3 "chain" | about 2026-10-20 | rows 1 (image, three variants, without the `-nvidia-legacy` evidence), 2, 3, 4, 5, 6; `release.yml` (PB13, [ADR-0104](https://github.com/ars-regia/athanor/pull/354), proposed in PR #354); P2 (#122); S1 (#124) | The first `:stable` promotion on evidence. Existing: `python3 -B -m unittest discover -s system/tests` (`test_promote.py`). To add: a check that the digest behind `:stable` equals the digest of the `accept.yml` verdict |
| 0.5 "base" | about 2026-11-07 | rows 7, 8, 10; the Fedora 45 rebase, or Fedora 44 per [ADR-0078](../decisions/0078-fedora-release-target.md); P4b (#126), the bootc two-step update chain; the start of the image key rotation to key 2 (its close, PB5b, needs a key-2 release on `:stable` and one more release, so it lands in 0.7); the ISO acceptance under Secure Boot (#252) | An ISO install, encrypted and under Secure Boot, updates itself to the next `:stable`. Existing: `forge/test/iso/run_iso_test.sh ISO OUTPUT_DIR`. To add: that test under Secure Boot firmware, ending in an update to a newer `:stable` |
| 0.7 "defences" | about 2026-11-20 | rows 9, 11 (acceptance 1 and 7 of `doc_recovery.md` on the dev VM), 12 (lock, polkit agent, SystemPrompter), 14 to 24; the close of the image key rotation (PB5b) | Every row placed here holds. Existing: `python3 scripts/verify.py services pam polkit polkit-model contacts shipped coverage` and the unit tests the rows name. To add: the image checks and acceptance cases the rows mark "to add" |
| 0.9 "surface" | about 2026-11-27 | rows 12 (screenshot and screen share, bar, dock, launcher, coexistence), 13, and the hardware work below; the visual language and accessibility; the maintainer's aesthetic sign-offs (ST8, VL12) | The release candidate. Existing: `scripts/devvm/bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `compositor-acceptance.sh`. To add: the hardware matrix as a recorded run |
| 1.0 | before 2026-12-02 | 0.9 with no blocking defect | To add: a query for open blocking issues that must return none |

Row numbers count the rows of section 2 from the top: 1 signed immutable image; 2 NVIDIA
failures; 3 update chain to `:stable`; 4 one human approval per release; 5 evidence bundle;
6 resolved package set; 7 key hierarchy and MOK key set; 8 installer path; 9 greeter; 10 disk
encryption; 11 recovery and rollback; 12 desktop session tier; 13 cosmic-comp patches;
14 keyring prompter; 15 application installation; 16 Tetragon; 17 attestation deleted; 18 DNS
over TLS; 19 USBGuard; 20 `sshd`; 21 SearXNG; 22 snapshots; 23 update Apply; 24 least
privilege and PAM. Every row is in exactly one release, except the three split below.

### 4.2 Rows that span releases

- **Row 12, desktop session tier.** Lock, polkit agent and SystemPrompter are functions with
  security consequences and go to 0.7 with the prompter and the greeter, which share
  `doc_lock_and_prompts.md`. Screenshot and screen share, bar, dock, launcher and the
  coexistence of cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings are the
  graphical surface and go to 0.9.
- **Row 1, signed immutable image.** The three variants build in 0.3, and the default and `-nvidia` images promote; the
  `-nvidia-legacy` evidence (20 boot-matrix runs, #231 closed) needs the maintainer's NVIDIA
  hardware and goes to 0.9. Until then `-nvidia-legacy` is not promoted (D24).
- **Row 11, recovery and rollback.** The dev VM acceptance is 0.7; `switch-verified.sh` on the
  desktop and the laptop belongs to the hardware work of 0.9.

### 4.3 Hardware work at 0.9

One session, not one per release (ADR-0105): the maintainer's NVIDIA desktop, the
`-nvidia-legacy` evidence and #231, `switch-verified.sh` on the desktop and the laptop, Wi-Fi
and Bluetooth on hardware, and the final hardware matrix. Releases 0.3 to 0.7 are accepted on
the dev VM and in CI; what they leave unproven on hardware is exactly this list.

## 5. How this file changes

Only a decision record changes the scope of 1.0, and only a decision record changes its order
or its release points: [ADR-0105](../decisions/0105-releases-before-1-0.md) set the four
releases of section 4. A pull request that adds, removes or moves a
row of sections 2 or 3 names the record that decided it; a pull request that only turns a
"to add" into an existing check, or corrects a path, needs none.
