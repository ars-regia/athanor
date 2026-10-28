# Release 1.0

| Field | Value |
| --- | --- |
| Purpose | What Athanor 1.0 contains, how each item is accepted, what waits for 1.1, and the order of work |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 6, 2026-10-23, with [ADR-0129](../decisions/0129-capture-leaves-0-9.md): release 0.9 ships without screenshot and screen share, which row 12 keeps for the first release after steps 4 and 5 of `doc_portal.md` are ready. Revision 5, 2026-10-22, with [ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md): row 7 and image key 2 move to 1.0, and release 0.11 "anchor", signed with key 1, follows 0.10. Revision 4, 2026-10-19, with [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md): 1.0 waits for Athanor's own compositor ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md), [ADR-0122](../decisions/0122-own-compositor-optional-session-and-desktop-first.md)), and the Fedora rebase ships as release 0.10. Revision 3, 2026-10-14: scope from [ADR-0103](../decisions/0103-audit-5-decisions.md) (revision 1, D1), order and release points from [ADR-0105](../decisions/0105-releases-before-1-0.md), amended by [ADR-0115](../decisions/0115-zero-nine-in-one-week.md) (two releases, 0.3 and 0.9, and rows moved to 1.1) and [ADR-0120](../decisions/0120-security-class-updates-in-1-0.md) (row 25) |

## 1. Purpose and date

1.0 is the first release a person outside the project can install and keep. It is narrow on
purpose (D1): a signed image, its update chain, the installer, recovery, one complete desktop
session on Athanor's own compositor, and encryption offered in the installer as the person's
choice. Everything else waits.

1.0 has no date. It ships when Athanor's own compositor passes its acceptance and replaces
cosmic-comp, whose session then leaves the greeter, with the GTK4 shell on top ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md) decision 5), and once the overview and the window switcher on Alt+Tab have come (`doc_own_stack.md` decision 28). The toolkit
does not gate it: its surfaces replace their GTK4 twins one at a time, under `doc_own_stack.md`
OS8 (section 6 step 5).

The date of the Fedora rebase is set outside the project. Fedora 43 reaches its end of life on
2026-12-02. Fedora 45 final is due on 2026-10-20; if its image is not green by mid-November 2026,
the base moves to Fedora 44 instead ([ADR-0078](../decisions/0078-fedora-release-target.md)). Release 0.9 ships on Fedora 43; the rebase ships as
release 0.10 "base" before 2026-12-02 ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md), [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md)). The Fedora 43 line gets no feature
release after 0.9. Release 0.11 "anchor" follows 0.10 and is not a rebase ([ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md)). 0.11 is the last 0.x release before 1.0: a further Fedora rebase
before 1.0 (`doc_own_stack.md` section 8) changes the release points, and a decision record
places it (section 5).

## 2. In 1.0

1.0 is done when every row holds. An acceptance check written "to add" does not exist yet: the
pull request that builds the row adds it, under that name or a better one.

| Requirement | Owner | Acceptance |
| --- | --- | --- |
| Signed immutable image, three variants: default, `-nvidia`, and `-nvidia-legacy` only on its evidence (D24; the evidence is in 1.1, so the variant stays unpromoted, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md)) | `doc_system_image.md` S1 to S5, `doc_update_trust.md` UT2, UT3 | `system/verify-images.sh --registry REGISTRY/OWNER DIGESTS_FILE`; `python3 -B -m unittest discover -s system/tests`; `python3 scripts/verify.py registry` |
| NVIDIA failures do not block the other images (D24) | `doc_update_delivery.md` UD4 | `system/tests/test_build_variants.py` (image stage); `system/tests/test_kernel_artifacts.py` and `system/tests/test_optional_branch.py` (kernel-module stage) |
| Update chain to `:stable`: the PB5 slice, promotion only on acceptance evidence for the same digests (D15) | `doc_pipeline.md` PB5, `doc_update_delivery.md` UD1 to UD6 | `python3 -B -m unittest discover -s system/tests` (`test_promote.py`: missing evidence, a fail verdict, another digest or another run are refused); `scripts/ci/evidence.py`; `.github/workflows/accept.yml` writes the verdict file; `.github/workflows/promote-stable.yml` |
| One human approval per promotion to `:stable`, in the environment `release`, and `:latest` signed with no approval from step 3 of the image key rotation, the switch of Task B4b of `docs/superpowers/plans/2026-10-22-row-7-key-hierarchy.md`, merged after 0.11 and before 1.0 is signed ([ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md)); agents open, push and merge pull requests as a GitHub App (D2, D25, ADR-0104) | ADR-0103 D2, D25; ADR-0104; `doc_pipeline.md` PL5, PL56, PL60 | `python3 scripts/github-settings/ghsettings.py diff`; `release` in `.github/settings/environments.json`; `.github/workflows/promote-stable.yml` runs `system/promote.sh` only in its job `promote`, in `release` |
| Signed evidence bundle copied off GitHub at promotion (D23) | `doc_pipeline.md` PL42 | `system/evidence-bundle.sh verify`, run by both jobs of `.github/workflows/promote-stable.yml`; `system/tests/test_evidence_bundle.py`; `system/github-release.sh`, job `github-release` of `promote-stable.yml`; `system/tests/test_github_release.py` |
| Resolved Fedora package set recorded for every build (D16) | `doc_update_delivery.md` UD28 | `system/package-set.sh`, artifact `package-sets`; `system/promote.sh` refuses a promoted image without its set (`test_promote.py` `test_a_promoted_image_without_its_package_set_is_refused`); the set is in the evidence bundle |
| Key hierarchy for the image key; MOK interim key set with not-after and revocation (D5; in 1.0, [ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md), which amends items 2 and 4 of [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md), item 4 for row 7 only) | `doc_update_trust.md` UT2, [ADR-0084](../decisions/0084-key-custody-model.md) | `scripts/devvm/acceptance/run.sh` (stages `rotate`, `recover`); to add: `cargo test -p athanor-update` cases for an expired and a revoked key |
| Installer path from the ISO to the first session: Anaconda Users and Timezone pages, MOK import in `%post` with a shown one-time password, x86-64-v3 and UEFI check at ISO start (D17) | `doc_platform_experience.md`, `doc_kernel_profile.md` D3, D14 | `forge/test/iso/run_iso_test.sh ISO OUTPUT_DIR` (the install, MokManager answered with the shown password, the first session and Settings; `SECURE_BOOT=1` on Secure Boot firmware); `.github/workflows/accept.yml`; `python3 scripts/verify.py installer`; `system/tests/test_installer_scripts.py`; the `SECURE_BOOT=1` case, the second install of `accept.yml` (#252) |
| Greeter: user list, "Other user", PAM conversation by message type (D7) | `doc_lock_and_prompts.md` LP19 for 0.9, LP18 step 4 for 1.1 | `doc_lock_and_prompts.md` section 5, items 18 to 24: `cargo test -p athanor-greeter-ui` (items 18 to 22, the lockout and expiry against a fake greetd among them); `scripts/devvm/greeter-acceptance.sh` (items 23 and 24, on the dev VM) |
| Recovery and rollback: text console, GoBack, greenboot | `doc_recovery.md` R1 to R7 | `cargo test -p athanor-update`; `scripts/devvm/acceptance/run.sh` (stage `goback`); `scripts/devvm/switch-acceptance.sh rollback`; `scripts/devvm/acceptance/run.sh` (stages `console` and `greenboot`: acceptance 1 and 7 of `doc_recovery.md`) |
| Desktop session tier: session lock and polkit agent as upstream components packaged in Fedora, chosen in the pull request that ships them, confined and checked; screenshot and screen share, bar, dock, launcher; cosmic-idle, cosmic-bg, cosmic-workspaces, cosmic-settings coexist (D13, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md)); in 1.0 the tier runs on Athanor's own compositor, which provides the four COSMIC components the protocols they use or replaces them ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md)) | `doc_shell.md` SH3, `doc_lock_and_prompts.md` LP11, `doc_portal.md` PT8, PT9, `doc_settings.md` SE23, `doc_own_stack.md` OS5 | `scripts/devvm/bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `shield-acceptance.sh`; `forge/test/shell/rig.sh`; `scripts/devvm/lock-acceptance.sh` (the lock and the polkit agent on the dev VM); to add: capture acceptance on the dev VM; the same acceptance against the own compositor |
| The compositor. Until 1.0, cosmic-comp carries patches 1 and 2 (D14, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md)). Before 1.0 the own compositor may become the default session with cosmic-comp's as the second choice (`doc_own_stack.md` decision 28); in 1.0 it is the only session and passes the acceptance of `doc_own_stack.md` revision 2: OS4 and OS5 tested in the fixture of OS6, the trusted path and the keyboard monitor that answers Orca alone among them ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md) decision 5; [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md) item 5) | `doc_compositor.md` CO3; `doc_own_stack.md` OS3 to OS6 | `scripts/devvm/compositor-acceptance.sh` (cosmic-comp); `cargo test -p athanor-compositor`; to add: the acceptance that `doc_own_stack.md` revision 2 writes |
| Application installation: verified Flathub remote, Flatpak update timer, the packaged store (D8) | `doc_software.md` SW15 to SW17 | `python3 scripts/verify.py contacts`; `system/check-image-contacts.sh` (the `applications` contact: the pinned remote key and the enabled timer, run on the system image in CI) |
| athanor-attestation and the mesh and post-quantum residue deleted (D10) | ADR-0103 D10 | `python3 scripts/verify.py attestation-absent bus-api-residue` |
| DNS over TLS opportunistic until the captive-portal probe ships, then strict (D11) | [ADR-0079](../decisions/0079-captive-portals-under-strict-dot.md) | `python3 scripts/verify.py contacts`; the image check of the resolver mode (`system/check-image-contacts.sh`) |
| USBGuard class allowlist; confirmation for new external devices once the notice exists (D12) | `doc_disks.md` DK21 | `python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests` (`test_usbguard_rules.py`: the admitted and blocked classes); the first-boot snapshot of the internal devices (D12, `athanor-usbguard-internal`) is tested on a fake sysfs; open: the same on real hardware, with SELinux and the boot order, in the 0.9 hardware acceptance |
| Existing machines with `sshd` enabled keep it; off on new installs (D18); the installer's `public` zone loses `ssh` on upgrade ([ADR-0119](../decisions/0119-ssh-closes-in-the-public-zone-on-upgrade.md)) | [ADR-0089](../decisions/0089-defaults-that-contact-or-listen.md) items 2, 3; [ADR-0116](../decisions/0116-migrate-the-installer-firewall-zone.md) | `python3 -B -m unittest discover -s system/tests` (`test_sshd_default.py`: the install kickstarts disable `sshd`, nothing shipped disables it on an existing machine); `python3 -B -m unittest discover -s forge/specs/athanor-base-config/tests` (`test_firewall_zone_migrate.py`) |
| "Snapshots of your files": restore from the command line, visible failure (D21, first step) | [ADR-0101](../decisions/0101-backup-one-mechanism.md) | `cargo test -p athanor-backup` (includes the home that cannot be snapshotted) |
| Update Apply: `auth_admin` with other sessions, `allow_active` with one; `blocked()` fails closed (D22) | `doc_update_trust.md` UT6, `doc_threat_model.md` TM9 | `python3 scripts/verify.py polkit polkit-model`; `cargo test -p athanor-update`, a failing inhibitor query included (`a_failing_inhibitor_query_refuses_as_failed`, `an_unanswered_inhibitor_query_refuses_as_failed_and_unlocks_nothing`) |
| Services keep least privilege; PAM rules hold | `doc_threat_model.md` TM8, [ADR-0090](../decisions/0090-account-lockout-faillock.md) | `python3 scripts/verify.py services pam` |
| Security-class updates: applied at the next shutdown the person starts, one postpone, the opt-out, the class read from the signed release attestation ([ADR-0120](../decisions/0120-security-class-updates-in-1-0.md); in 0.10, [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md)) | `doc_update_trust.md` UT13, `doc_pipeline.md` PB4b and PB10, [ADR-0082](../decisions/0082-update-control.md) | to add: acceptance items 16 to 19 of `doc_update_trust.md` section 6 on the dev VM (`scripts/devvm/acceptance/run.sh`); #543 |

## 3. Moved to 1.1

- Machines stages V1 and V2; Nix for every user; the rich notification features (D1).
- Tetragon, step 1 included: the policies and the SELinux CIL type (row 16), and the relay and
  notices (D9) ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- Athanor's own session lock, polkit agent and SystemPrompter (`doc_lock_and_prompts.md` LP18)
  (row 12), and the keyring prompter exchange carried by Athanor (row 14, D20); until then GNOME
  Keyring keeps its own gcr prompter ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)). cosmic-comp patches 5 and 6, which ADR-0115 placed
  here, are not written: their function, the compositor trusted path and the keyboard monitor
  that answers Orca alone (AX4), is a requirement of the own compositor in 1.0 (row 13,
  `doc_own_stack.md` OS4; [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md) item 5).
- LUKS2 on by default (row 10, D6). The record's wording of the reason: Anaconda on Fedora 43 has
  no setting that makes encryption default-on; the only patch-free route, an encrypted autopart in the kickstart, preselects the
  disks and stops asking for the disk, and the maintainer rejected it. Until 1.1 encryption stays
  the person's choice in the installer (`doc_platform_experience.md` section 1)
  ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- The SearXNG feature (row 21, D19), whose section of `doc_software.md` is not written
  ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- The `-nvidia-legacy` evidence: 20 boot-matrix runs and #231 closed (row 1). D24 keeps the
  variant unpromoted without it ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md)).
- The retirement of cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings, and the
  programs that replace them (D13). Until then they run on the own compositor through the
  protocols it provides them, unless one is replaced before 1.0 (`doc_own_stack.md` OS5,
  ADR-0121).
- A snapshot and lock of the Fedora input (D16).
- Kernel roles, IPE and AutoFDO (D26).

Later, with no release named: cosmic-comp patches 3, 4 and 7 to 10 after a measured rebase drill
(D14), while cosmic-comp is the default, the own compositor carrying their behaviour as
`doc_own_stack.md` OS4 says; Bazaar after the Fedora 45 rebase (D8); strict DNS over TLS after the probe (D11); the
first-run account helper (D17); the external-disk backup and its restore interface (D21); SBOM
and provenance as promotion conditions (D15). The Bridge (UD45 to UD50) is dropped, not moved
(D4).

## 4. Order of work

Four releases lead to 1.0: 0.3, then 0.9 within about one week ([ADR-0115](../decisions/0115-zero-nine-in-one-week.md), which cut the four of
[ADR-0105](../decisions/0105-releases-before-1-0.md)), then 0.10 "base", the Fedora rebase, before 2026-12-02 ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md), [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md)), then 0.11 "anchor", the transitional release, signed with key 1 and carrying the new trust anchor ([ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md)). The
dates are estimates. Each 0.x release is a signed tag on `iso-v0` plus a `:stable` promotion of
that build on evidence (BRN4 of `branching.md`), with the `release` approval. 1.0 is the last
0.x release with Athanor's own compositor in place of cosmic-comp, its
acceptance passed, and no blocking defect; it has no date ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md) decision 5).

**Freeze (D3), lifted on 2026-10-14** ([ADR-0112](../decisions/0112-pace-to-0-9.md)). P2 of
the update-chain plan (#122) is green; spike S1 (#124) still gates P4b. Specification
revisions and shell work run in parallel with it, within the machine's CPU budget. The
placement of S1 and P4b after 0.9, with the rebase in 0.10, is a proposal of ADR-0115. The Fedora 45
rebase does not wait for P3 (D26); the Fedora 44 fallback date holds. In the release half of the
pipeline the PB5 slice comes first (D15), then the evidence copy (D23); the key hierarchy (D5)
waits for the maintainer's LUKS2 key backup, which is not made yet (maintainer, 2026-10-22) and is Task A2 of `docs/superpowers/plans/2026-10-22-row-7-key-hierarchy.md`.

### 4.1 Releases

| Release | Target (estimate) | Contents: rows of section 2 and work outside them | Exit |
| --- | --- | --- | --- |
| 0.3 "chain" | its exit was met on 2026-10-14; tagged when its remaining issues close | rows 1 (image, three variants, without the `-nvidia-legacy` evidence), 2, 3, 4, 5, 6; `release.yml` (PB13, [ADR-0104](../decisions/0104-release-workflow-and-stable-gate.md), merged in #354); P2 (#122) | The first `:stable` promotion on evidence. Existing: `python3 -B -m unittest discover -s system/tests` (`test_promote.py`). `system/promote.sh` reads `:stable` back and refuses unless it is the evidence digest (`test_promote.py` `test_stable_is_read_back`) |
| 0.9 "surface" | about 2026-10-21 (one week), some days later by [ADR-0122](../decisions/0122-own-compositor-optional-session-and-desktop-first.md) | rows 8, 9, 11, 12 (without screenshot and screen share, [ADR-0129](../decisions/0129-capture-leaves-0-9.md)), 13 (patches 1 and 2), 15 (the image ships the pinned Flathub remote and the daily update timer, `athanor-flatpak-update.timer`; the `*.preinstall` reader of decision 7 and the packaged store follow), 17, 18, 19, 20, 22, 23, 24; the ISO acceptance under Secure Boot (#252); the hardware work of 4.3; the visual language and accessibility, without the keyboard monitor that answers Orca alone (AX4, a 1.0 requirement of the own compositor by ADR-0123 item 5); the maintainer's aesthetic sign-offs (ST8, VL12). It ships on Fedora 43. Being finished on 2026-10-18: the shell stage 1 and stage 2 acceptance (#133, #134) and the hardware session (#538) are open; ST8 and VL12 were signed with reservations on #538, and the aesthetic targets they set apply to the surfaces rebuilt on the toolkit (#616); #600 (doubled shell surfaces after an output is disabled and enabled in software, milestone 1.0) and #604 (the NVIDIA desktop's GPU falls off the bus on resume or unplug, milestone After 1.0) are known limits in its release notes | The release candidate: every row placed here holds and the hardware matrix is a recorded run. Existing: `scripts/devvm/bar-acceptance.sh`, `dock-acceptance.sh`, `launcher-acceptance.sh`, `compositor-acceptance.sh`, `python3 scripts/verify.py services pam polkit polkit-model contacts shipped coverage`, `forge/test/iso/run_iso_test.sh ISO OUTPUT_DIR`, and the unit tests the rows name. To add: the image checks and acceptance cases the rows mark "to add", except the capture acceptance of row 12 (ADR-0129), the ISO test under Secure Boot firmware ending in an update to a newer `:stable`, and the hardware matrix. A matrix row that fails on a known limit names its issue (#600, #604) |
| 0.10 "base" | before 2026-12-02 | 0.9 rebased onto Fedora 45 (P4a, #125), or Fedora 44 per [ADR-0078](../decisions/0078-fedora-release-target.md); S1 (#124) and P4b (#126), the bootc two-step update chain (proposed by ADR-0115); row 25, the security-class updates of UT13 (#543). The rebase does not wait for row 25: a row not ready by the Fedora 44 decision point moves to the next release ([ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md)) | A `:stable` promotion of the rebased build on evidence before 2026-12-02; the checks the rebase moves (kernel, package set, compositor) run again on it (ADR-0115). Existing: `test_promote.py`, `scripts/devvm/acceptance/run.sh`. To add: the gate of P4a (#125) and the cases row 25 marks "to add" |
| 0.11 "anchor" | after 0.10 and before 1.0; no date | The new trust anchor of the key hierarchy in its policy, signed with key 1 ([ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md)); nothing else is assigned to it, and fixes that land meanwhile ship in it as usual | A `:stable` promotion of the build on evidence, signed with key 1, with the `release` approval; its policy names the new trust anchor. Existing: `test_promote.py`. To add: the check that the policy of the build names the new trust anchor |
| 1.0 | no date: when the own compositor passes its acceptance ([ADR-0121](../decisions/0121-own-compositor-and-toolkit.md) decision 5) and the overview and the window switcher on Alt+Tab have come (`doc_own_stack.md` decision 28) | row 7 (key hierarchy, not-after and revocation) with image key 2, 1.0 being the first release signed under the hierarchy; rows 12 and 13 on Athanor's own compositor, the default session in place of cosmic-comp, with the GTK4 shell on top; PB5b closes at the first release after 1.0 (section 4.2); no blocking defect | The acceptance of `doc_own_stack.md` revision 2 (section 6 step 4): every surface 1.0 carries drawn in the rig against the own compositor with its goldens matched; OS4 and OS5 tested in the fixture of OS6; AX17's scenarios passing and Orca's transcripts no poorer than on cosmic-comp; the hardware matrix of 4.3, NVIDIA's driver included, a recorded run on the 1.0 build, where a failing row names its issue and only an issue of milestone 1.0 or earlier blocks; the overview's acceptance (`doc_overview.md`, section 5) recorded under the own compositor after the change of OV19 step 5, which shows it has come; the window switcher's acceptance, from the revision of `doc_overview.md` that designs it. Every row of section 2 holds. To add: the checks revision 2 names, and a query for open blocking issues that must return none |

Row numbers are those of revision 2 of this file, so that issues and records that cite them
stay valid: 1 signed immutable image; 2 NVIDIA failures; 3 update chain to `:stable`; 4 one
human approval per promotion to `:stable`; 5 evidence bundle; 6 resolved package set; 7 key
hierarchy and MOK key set; 8 installer path; 9 greeter; 10 disk encryption; 11 recovery and
rollback; 12 desktop session tier; 13 compositor (cosmic-comp patches until 1.0); 14 keyring prompter; 15 application
installation; 16 Tetragon; 17 attestation deleted; 18 DNS over TLS; 19 USBGuard; 20 `sshd`; 21
SearXNG; 22 snapshots; 23 update Apply; 24 least privilege and PAM; and, added later, 25 security-class updates. Rows 10, 14, 16 and 21 are in
section 3 and no longer in section 2. Every other row is in exactly one release, except the
rows narrowed below.

### 4.2 Rows split or narrowed

- **Row 7, key hierarchy.** It leaves 0.9 for 1.0, with the PB5b rotation, its key ceremony
  and key 2 ([ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md), which amends items 2 and 4 of [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md), item 4 for row 7 only; it was in 0.10).
  0.10 is signed with key 1 and changes nothing of the keys: its policy already trusts the
  public half of key 2, committed by `secrets.md` section 4.1 step 1, as every image since. Release 0.11 "anchor" is signed with key 1 and
  carries the new trust anchor in its policy: the hierarchy's root, which machines pin
  (`doc_update_trust.md` UT2), in the form the amendment of UT2 defines; key 2 arrives with the hierarchy in 1.0, the
  first release signed under it. Key 1 is retired, and PB5b closes, at the first release
  after 1.0. [ADR-0103](../decisions/0103-audit-5-decisions.md) D25 stands: no release is
  signed with key 2 before the hierarchy exists. `doc_update_trust.md` UT2 already says
  "before 1.0", read as: the hierarchy exists before 1.0 is signed; the mechanism is not written, and the approved text defers it to the revision
  that builds it. The next step is an amendment of UT2 that defines the mechanism, for approval.
- **Row 9, greeter.** For 0.9, D7's acceptance is met on the greeter's own conversation code.
  LP18 step 4, moving it into `athanor-auth`, waits for Athanor's lock and moves to 1.1 with
  step 3.
- **Row 12, desktop session tier.** For 0.9 the session lock and the polkit agent are upstream
  components packaged in Fedora, confined and checked; the pull request that ships them
  chooses the components. Screenshot and screen share, bar, dock, launcher and the coexistence
  of cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings are the graphical surface.
  All of this is 0.9 except screenshot and screen share, which ship in the first release after
  steps 4 and 5 of `doc_portal.md` are ready
  ([ADR-0129](../decisions/0129-capture-leaves-0-9.md)). Athanor's own lock, polkit agent and
  SystemPrompter (`doc_lock_and_prompts.md` LP18) are in 1.1 (section 3). In 1.0 the same tier runs on the own
  compositor, which provides cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings the
  protocols they use or replaces them (`doc_own_stack.md` OS5, ADR-0121).
- **Row 13, the compositor.** cosmic-comp with patches 1 and 2 is the default session of 0.9 and of
  each release before the switch of `doc_own_stack.md` decision 28, and the second choice of each
  release after it. In 1.0 the own compositor replaces it, with the overview and the window
  switcher on Alt+Tab that decision leaves after the switch, and the rows of the register of
  `doc_compositor.md` CO3 become its requirements or leave with cosmic-comp, as
  `doc_own_stack.md` OS4 says. Its acceptance holds OS4 in full, so 1.0 has the compositor
  trusted path for the lock and the prompts and the keyboard monitor that answers Orca alone
  (AX4), which ADR-0115 left to cosmic-comp patches 5 and 6 in 1.1 ([ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md) item 5).
  Before 1.0 the image may offer the own compositor as an optional session beside cosmic-comp's,
  under the conditions of [ADR-0122](../decisions/0122-own-compositor-optional-session-and-desktop-first.md), and then make it the default session with
  cosmic-comp's at the greeter as the second choice (`doc_own_stack.md` decision 28); neither
  step is a release point.
- **Row 1, signed immutable image.** The three variants build in 0.3, and the default and
  `-nvidia` images promote; the `-nvidia-legacy` evidence (20 boot-matrix runs, #231 closed) is
  in 1.1. Until then `-nvidia-legacy` is not promoted (D24).
- **Row 19, USBGuard.** For 0.9 the acceptance is the rule test over the shipped rules. The
  lock-aware `athanor-usbguard-policy` service of LP18 step 10 depends on Athanor's lock and
  moves with it to 1.1.

### 4.3 Hardware work at 0.9

One session, not one per release (ADR-0105), and it needs the maintainer in the week of 0.9:
`switch-verified.sh` on the desktop and the laptop, the NVIDIA desktop for the `-nvidia`
image, Wi-Fi and Bluetooth on hardware, and the final hardware matrix. The `-nvidia-legacy`
evidence and #231 are not part of it; they are in 1.1. Release 0.3 is accepted on the dev VM
and in CI, and what it leaves unproven on hardware is exactly this list. What 0.9 leaves
unproven is the `-nvidia-legacy` evidence, which moved to 1.1. The session is #538.

At 1.0 the hardware matrix runs again, on the own compositor, as its acceptance requires
(`doc_own_stack.md` section 6 step 4).

## 5. How this file changes

Only a decision record changes the scope of 1.0, and only a decision record changes its order
or its release points: [ADR-0105](../decisions/0105-releases-before-1-0.md) set four
releases, [ADR-0115](../decisions/0115-zero-nine-in-one-week.md) cut them to two, and [ADR-0121](../decisions/0121-own-compositor-and-toolkit.md) and [ADR-0123](../decisions/0123-rebase-release-0-10-rows-7-and-25.md) added 0.10, the third
of section 4, and [ADR-0128](../decisions/0128-row-7-and-key-2-in-1-0.md) added 0.11, the fourth. A pull request that adds, removes or moves a
row of sections 2 or 3 names the record that decided it; a pull request that only turns a
"to add" into an existing check, or corrects a path, needs none.
