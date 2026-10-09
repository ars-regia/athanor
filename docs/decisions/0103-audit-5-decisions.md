---
id: ADR-0103
title: "Audit 5 decisions and the scope of 1.0"
date: 2026-10-09
status: accepted
issues: [122, 124, 231]
areas: [platform, kernel, build, signing, security, shell, apps, docs]
---

# 0103. Audit 5 decisions and the scope of 1.0

## Context

Audit 5 ran on 2026-10-09 against `iso-v0` at `dbf6e7ea` and reviewed every choice the
project has made, the maintainer's included, across the platform, the pipeline, security,
the desktop, the applications and the process. Nearly every specification is approved;
implementation is the bottleneck. Three facts were verified at the source on the day: no
`:stable` tag exists for `athanor-system`; `maintenance.yml` failed in 9 of its last 10 runs;
the 148 pull requests merged since 2026-10-06 were all authored and merged by one account.

The calendar is external. Fedora 43 reaches its end of life on 2026-12-02; Fedora 45 final is
due on 2026-10-20, with Fedora 44 as the fallback if the Fedora 45 image is not green by
mid-November ([ADR-0078](0078-fedora-release-target.md)). No document defined what 1.0
contains, and the approved specifications, taken together, describe a scope wider than a
first release.

The maintainer took the 27 decisions below on 2026-10-09. Each follows the audit's
recommended option, except D19.

## Decision

### Scope and order

- **D1. A narrow 1.0, written down.** `docs/operations/release-1.0.md` defines 1.0: the
  signed image, the update chain, the installer, recovery, one complete desktop session and
  encryption, each item with its acceptance command, and a cut list to 1.1: Machines stages
  V1 and V2 (amends [ADR-0095](0095-specs-review-batches-2-3.md) item 3 for 1.0), Nix for
  every user ([A2-16](0053-nix-for-every-user.md)), and the rich notification features.
- **D3. Freeze.** New specification revisions and non-critical shell work are frozen until P2
  of the update-chain plan (the kernel build profile and boot matrix, #122) and spike S1
  (#124) are green. S1 is time-boxed. P2 and S1 run in parallel within the CPU budget.
- **D26. The kernel profile's 1.0 gate is re-cut to the narrow 1.0.** The 1.0 row of
  `doc_kernel_profile.md` section 12 ("the immediate items, S1 and P1–P7 green") and the order
  of P2, P3 and P4a are re-cut to the narrow 1.0: roles (P5), IPE (the dm-verity part of P6)
  and AutoFDO (P7) move to 1.1. The new order follows the audit recommendation the
  maintainer accepted: the Fedora 45 rebase (P4a) no longer waits for P3, so
  [ADR-0078](0078-fedora-release-target.md) item 1 is amended in its start condition only;
  its fallback to Fedora 44 stays. The `doc_kernel_profile.md` revision that applies this
  decision writes the order out.

### Release trust and review

- **D2. Agents open pull requests as a GitHub App.** The App is an identity distinct from the
  maintainer ([ADR-0080](0080-pipeline-architecture.md) item 5); the maintainer approves for
  real, and the App has no administrator bypass. A new environment `release`, which holds no
  key, asks the maintainer once per release. [ADR-0098](0098-update-delivery-ci-operations-batch-4.md)
  item 6 is amended: the approval it removes from `signing-images` is replaced by the one of
  `release`, not dropped. PR #345 waits and is rewritten on this model. Follow-up of the same
  day: commits keep the maintainer's git identity as their author, since the maintainer is the
  one using the agent; only the pull request's author, the push and the merge move to the App.
- **D25. Signing does not hold the run.** The release signing is a separate job that does
  not hold the concurrency group of the builds, and it runs behind the `release` environment.
  No transitional release signed with key 2 is cut before the key hierarchy of D5 exists.
- **D23. Evidence leaves GitHub.** At promotion, the signed evidence bundle is copied to a
  location off GitHub (`doc_pipeline.md` PL42). This supersedes the sentence "There is no
  copy outside GitHub for now" of [ADR-0081](0081-cra-compliance-posture.md) item 3.

### Platform and updates

- **D4. The Bridge is dropped.** UD45 to UD51 of `doc_update_delivery.md`, the environment
  `bridge` and its token are removed. The maintainer's desktop and laptop move with
  `scripts/switch-verified.sh`. The owner-pinned policy (UD11 to UD13) is kept. This reverses
  the decision on UD50 and the part of [ADR-0098](0098-update-delivery-ci-operations-batch-4.md)
  item 6 that concerns `bridge`. *(Narrowed by the maintainer on 2026-10-09: UD51, the
  `pinned-build` state, is not part of the Bridge and stays; the Bridge is UD45 to UD50.)*
- **D5. A key hierarchy for the image key before 1.0.** An offline root, kept in the custody
  kit of [ADR-0084](0084-key-custody-model.md), signs a short-lived online signing key and a
  timestamp; clients pin the root. For the Machine Owner Key, the interim is a signed key set
  with a not-after date and revocation. This amends `doc_update_trust.md` UT2, which states
  that the image key has no revocation. D5 depends on the LUKS2 key backup.
- **D15. The PB5 promotion slice comes first**, with acceptance evidence; SBOM and provenance
  (`doc_pipeline.md` PB4, PL19 to PL24) are added later as conditions of the promotion. This
  changes the order of `doc_pipeline.md` section 12, where PB3 and PB4 came before PB5.
- **D16. The resolved Fedora package set is recorded per build for 1.0**; a snapshot and lock
  of the Fedora input (the planned lock of `doc_system_image.md`, `doc_update_delivery.md`
  UD28) follow in 1.1.
- **D24. NVIDIA failures are decoupled from the other images now**: a failure in an NVIDIA
  variant no longer stops the build or promotion of the others.
  `athanor-system-nvidia-legacy` is in 1.0 only if 20 boot-matrix runs show an acceptable flake
  rate and #231 is fixed.

### Security

- **D6. Disk encryption is on by default.** LUKS2 with a passphrase, unlocked with TPM and
  PIN ([A2-27](0064-signing-approvals-and-mok-enrolment.md)); the person can turn it off in
  the installer. This reverses the maintainer decision of 2026-10-08 recorded in
  `doc_platform_experience.md` ("Disk encryption and the TPM") and in the disk layout of
  `doc_kernel_profile.md`, which made encryption a choice and not a default. This record is
  the decision record that decision lacked.
- **D9. Tetragon in a reduced scope.** Construction step 1 of `doc_tetragon.md`, three or four
  policies that write to the journal only, and an SELinux CIL type (TG11). The relay (TG7) and
  the notices (TG8) move to 1.1. An image check asserts the real state of the daemon. This
  amends the 1.0 scope of [A2-10b](0048-tetragon-made-real.md) and
  [A2-32](0069-tetragon-policy-decisions.md).
- **D10. athanor-attestation is deleted**, with its Cargo exclusion and the mesh and
  post-quantum residue in `athanor-bus-api`. This amends
  [ADR-0087](0087-attestation-outside-the-workspace.md), reversing its decision to keep the
  crate in the tree until a Keylime rewrite.
- **D11. DNS over TLS is opportunistic until the captive-portal probe ships, then strict.**
  This amends [ADR-0079](0079-captive-portals-under-strict-dot.md) item 1 and
  [ADR-0089](0089-defaults-that-contact-or-listen.md) item 6 until the probe exists.
- **D12. USBGuard admits by class.** Human interface devices, hubs, and the internal devices
  present at install (webcam, Bluetooth, readers) are allowed; a new external device that is
  not a human interface device needs confirmation once the notice exists (`doc_disks.md` DK21).
  This amends the default-deny baseline of [W1-FOLLOWUP](0008-wave1-follow-up.md).
- **D18. Remote login survives the update.** Machines with `sshd` already enabled keep it
  reachable: the update keeps ssh open in their active firewall zone instead of closing it.
  New installs have it off. This amends [ADR-0089](0089-defaults-that-contact-or-listen.md)
  item 2 and its "Existing installs" consequence for existing machines.
- **D22. Update Apply asks an administrator when other sessions exist.**
  `os.athanor.update.apply` is `auth_admin` when other sessions exist and `allow_active` only
  with one session; `blocked()` fails closed (`doc_update_trust.md` UT6,
  `doc_threat_model.md` TM9).

### Desktop

- **D7. The greeter lists users.** A user list and "Other user"; the PAM conversation is
  structured by message type, with no text matching (`doc_lock_and_prompts.md` LP18 step 4,
  `doc_settings.md` SE20). The login path is an authentication path: its diff is shown to the
  maintainer before it is pushed.
- **D13. The desktop in three tiers.** 1.0 carries the lock, the polkit agent, the
  SystemPrompter, screenshot and screen share (`doc_portal.md` PT8, PT9), and the bar, dock
  and launcher. cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings stay until 1.1,
  coexisting as `doc_settings.md` SE23 describes, and then retire. This amends SH3 of
  `doc_shell.md` ("of COSMIC only cosmic-comp stays") for 1.0.
- **D14. cosmic-comp patches 1, 2, 5 and 6 at 1.0.** The other patches of the register in
  `doc_compositor.md` CO3 follow a measured rebase drill. This amends
  [ADR-0093](0093-desktop-specs-batch-1.md) point 1, which accepted the rebase cost before it
  was measured. Patch 5, the trusted path, stays as [ADR-0092](0092-carry-pr-1441-upstream-first.md)
  decided.
- **D20. The keyring prompter's exchange is carried by Athanor for 1.0** (`doc_lock_and_prompts.md`
  LP12). This reverses, for 1.0, the dependency on upstream oo7 of
  [ADR-0085](0085-keyring-prompter-secret-exchange-source.md), in line with the principle of
  [ADR-0092](0092-carry-pr-1441-upstream-first.md).

### Applications and services

- **D8. Installing applications.** The verified Flathub remote, a Flatpak update timer and the
  store already packaged; Bazaar follows the Fedora 45 rebase
  ([A2-28](0065-bazaar-waits-for-fedora-45.md)). `doc_software.md` SW15 to SW17 and
  sections 9.3 and 9.4 are aligned to this.
- **D17. The installer.** Anaconda's Users and Timezone pages stay; the Machine Owner Key is
  imported in `%post` with a one-time password shown to the person; an x86-64-v3 and UEFI
  check runs at the start of the ISO (`doc_kernel_profile.md` D3, D14); the first-run account
  helper (`doc_first_run.md` FR6) comes later. This amends
  [A2-35](0072-mok-enrolment-in-installer.md) in its mechanism.
- **D19. SearXNG is recorded and planned for 1.0**, against the audit's recommendation of 1.1.
  Its section in `doc_software.md` is written in a later pull request.
- **D21. Backup in two steps.** First, the feature is renamed "Snapshots of your files", has a
  restore from the command line and fails visibly; then an external disk and a restore
  interface. This amends [ADR-0101](0101-backup-one-mechanism.md) in the feature's name and
  scope.

### Pipeline and process

- **D27. One area vocabulary.** The areas are the seven of `docs/operations/ownership.md`
  (`kernel`, `build`, `signing`, `security`, `shell`, `apps`, `docs`) plus `platform`; the
  `build-ci` area of `components.toml` becomes `build`, and its `signing-update` area becomes
  `signing` until the owners of signing and of updates differ. A `verify.py` check aligns the decision
  records, the inventory and the ownership map. This applies item 4 of
  [ADR-0074](0074-agent-and-contributor-model.md).

## Consequences

**Specifications.** The approved specifications this record touches are amended by it from
today: `doc_update_delivery.md` (UD4, UD28, UD45 to UD51), `doc_update_trust.md` (UT2, UT6),
`doc_kernel_profile.md` (the 1.0 gate, the P2, P3 and P4a order, the disk layout),
`doc_platform_experience.md` (encryption), `doc_pipeline.md` (section 12 order, PL42),
`doc_tetragon.md`, `doc_shell.md` (SH3), `doc_compositor.md` (CO3), `doc_lock_and_prompts.md`
(LP12, LP18), `doc_portal.md`, `doc_settings.md` (SE20, SE23), `doc_software.md`, `doc_disks.md`
(DK21), `doc_first_run.md` and `doc_threat_model.md` (TM9). Their text is aligned in later
pull requests, one per area; until then this record prevails where they differ.

**Decision records.** Under the rule of `README.md`, the status lines of the records this one
changes name ADR-0103, in the pull request that carries it: W1-FOLLOWUP, A2-10b, A2-16,
A2-32, A2-35, ADR-0078, ADR-0079, ADR-0081, ADR-0085, ADR-0087, ADR-0089, ADR-0093, ADR-0095,
ADR-0098 and ADR-0101 become `amended by ADR-0103`, each in the part named above. The
encryption decision of 2026-10-08 had no record of its own.

**What moves to 1.1.** Machines V1 and V2, Nix for every user and the rich notification
features (D1); the Tetragon relay and notices (D9); the retirement of cosmic-idle, cosmic-bg,
cosmic-workspaces and cosmic-settings (D13); the Fedora snapshot and lock (D16); roles, IPE
and AutoFDO (D26). Conditional or later, without a release named: compositor patches 3, 4 and
7 to 10 after the rebase drill (D14), Bazaar after the Fedora 45 rebase (D8), strict DNS over
TLS after the probe (D11), the USB confirmation after its notice (D12), the first-run account
helper (D17), the second step of backup (D21), SBOM and provenance as promotion conditions
(D15), and `athanor-system-nvidia-legacy` on its evidence (D24).

**Order of work.** The freeze of D3 holds until P2 and S1 are green. Within the release half
of the pipeline, the PB5 slice comes first (D15).

**Costs.** The key hierarchy (D5), the greeter's user list (D7), the reduced Tetragon (D9), the
carried prompter exchange (D20) and SearXNG (D19) add work to the 1.0 path; the Bridge (D4),
the full Tetragon, the later compositor patches and the 1.1 items leave it. Encryption by
default (D6) adds an installer and acceptance case. The App identity and the `release`
environment (D2, D25) add one approval per release and remove the unreviewed merge path.

**Steps that only the maintainer takes**, since they touch identities, environments or keys:

1. Create the GitHub App for the agents (D2).
2. Create the environment `release`, with the maintainer as required reviewer and no secret
   (D2, D25).
3. Delete the environment `bridge` and the token `ATHANOR_BRIDGE_TOKEN` (D4).
4. Make the LUKS2 backup of the keys, on which the key hierarchy of D5 depends.
5. Delete `MOK_PRIVATE_KEY` from the environment `signing` (`secrets.md` RL6).
