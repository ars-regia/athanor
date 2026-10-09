# Release 1.0

| Field | Value |
| --- | --- |
| Purpose | What Athanor 1.0 contains, how each item is accepted, what waits for 1.1, and the order of work |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 1, 2026-10-09 ([ADR-0103](../decisions/0103-audit-5-decisions.md), D1) |

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
| One human approval per release, in the environment `release`; agents open, push and merge pull requests as a GitHub App (D2, D25) | ADR-0103 D2, D25; `doc_pipeline.md` PL5 | `python3 scripts/github-settings/ghsettings.py diff`; to add: `release` in `.github/settings/environments.json` |
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
and provenance as promotion conditions (D15). The Bridge (UD45 to UD51) is dropped, not moved
(D4).

## 4. Order of work

1. **Freeze (D3).** No new specification revision and no non-critical shell work until P2 of
   the update-chain plan (the kernel build profile and boot matrix, #122) and spike S1 (#124)
   are green.
2. **P2 and S1 first, in parallel**, within the machine's CPU budget; S1 is time-boxed.
3. **The Fedora 45 rebase does not wait for P3** (D26); the Fedora 44 fallback date holds.
4. **In the release half of the pipeline, the PB5 slice comes first** (D15), then the evidence
   copy (D23) and the key hierarchy (D5), which waits for the maintainer's LUKS2 key backup.
5. The rows of section 2 in any order the dependencies allow; each lands with its check.

## 5. How this file changes

Only a decision record changes the scope of 1.0. A pull request that adds, removes or moves a
row of sections 2 or 3 names the record that decided it; a pull request that only turns a
"to add" into an existing check, or corrects a path, needs none.
