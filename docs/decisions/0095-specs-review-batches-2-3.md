---
id: ADR-0095
title: "Specifications, review batches 2 and 3, approved"
date: 2026-10-08
status: accepted
issues: []
areas: [kernel, session, security, storage, desktop]
---

# 0095. Specifications, review batches 2 and 3, approved

## Context

Review batches 2 and 3 of the 1.0 specification review cover the remaining approved-pending
specifications, together with the four kernel profile decisions that P4b needs and the case of
`doc_session.md` SN8 in which the notification service itself fails. Each question was presented
with options and a recommendation. On 2026-10-08 the maintainer approved every specification
"with the indicated changes" and accepted every recommendation.

## Decision

1. `doc_kernel_profile.md`: D39 closed on bootc's per-deployment `/etc` with its three-way merge,
   spike S1 verifying users and groups, the machine id, NetworkManager connections and the SELinux
   policy store; D48 rewritten as an IPE goal not applicable under bootc, the unrefused external
   firmware stated as residual risk for 1.0 and 1.1, LOADPIN an optional P6 spike; the 2 GiB ESP
   or XBOOTLDR of D17 applies from 1.1, 1.0 keeping the Fedora layout and reusing an existing
   Windows ESP; D41 split, the signed manifest for 1.0, SBAT and PCR 14 for 1.1; P4b and S1 open
   on the amendment.
2. `doc_session.md` SN8: a notice that cannot be delivered because `athanor-shelld` reached its
   start limit is kept and raised at the next session start.
3. `doc_virtualization.md` (PR #323): the owner-scoped polkit rule first accepted for VZ3 was
   found root-equivalent by the audit of the PR, and so was a session libvirt socket reachable by
   a confined application (the domain XML can name the program QEMU runs); the maintainer decided
   on 2026-10-08 that in 1.0 Machines runs libvirt's embedded QEMU driver inside its own
   confinement, with QEMU, swtpm, passt and virtiofsd as its children, no root daemon, no polkit
   rule and no libvirt socket reachable by any other application; the guest reaches the local network but not the host (passt with `--no-map-gw`); FreeRDP runs as a
   child of Machines in its `confined` class, the password on a pipe; stages V0 to V2 in 1.0, V3 and V4 after it, gated on spike
   S2; the Windows licence accepted by the person, the product key optional, and a download and
   checksum spike before V1; the shared clipboard on and declared, switchable per machine, the
   microphone off, both listed on the Privacy page; whole-disk boot (VZ13) and GPU passthrough
   after 1.0; spike S4 proves the model before V1 and compares GNOME Boxes and Athanor's Windows
   layer; the Windows agent added to
   the "why not upstream" section; the name Machines, disks excluded from backup, a first
   catalogue of Windows 11, Fedora Workstation, Ubuntu LTS and Debian.
4. `doc_first_run.md`: the resolver shown as information, changed in Settings; the
   `athanor-first-run` package installs `contacts.toml`; a test case for a dead-key password from
   first run to the greeter; the F3 warning kept; BeaconDB as written.
5. `doc_languages.md`: spike S4 runs first; the greeter follows the system layout and the lock
   screen and prompts follow the session layout, a divergence stated in LN10 and LP14 and hinted at
   in first run; LN3 and LN9 updated.
6. `doc_files.md`: approved as a preserved design, only section 7 and `athanor-files-view`
   operative in 1.0; FM14 points to GNOME Disks; Nautilus unconfined until the broker launches it
   confined.
7. `doc_disks.md`: DK3 and the usbguard module kept, the maintainer to name the owner, with a build step before SD22
   steps 4 and 5 (ADR-0007); DK20 B `noexec` decided (ADR-0025); no exec switch in 1.0; the
   udisksd hardening after spike S5; USBGuard with S4 and S6 as gates; withdrawn items marked and
   section 5 rewritten.
8. `doc_recovery.md`: greenboot's package, greetd check, configuration and preset owned by the
   specification; the VM verification declared debt before 1.0; the frozen recovery kiosk source
   and the glass theme retired (ADR-0073), removed by a follow-up change.
9. `doc_local_ai.md`: deferred out of 1.0 (ADR-0076 point 4), direction approved; the Gatekeeper
   reference removed (ADR-0034); AI11 recorded as done and the remaining `athanor-semantic-db`
   directory removed by a follow-up change; the microphone shown in the bar and listed on the
   Privacy page without a per-application permission; A0 (c) and A0 (d) moved to their own issues.
10. `doc_tetragon.md`: approved with sessiond's subscription recorded in
    `doc_session_daemons.md`; two persistence policy files; the export file readable by root and
    the relay only; network events limited to executables from temporary directories;
    notifications only for sensitive-file opens by another user or a tier 2 application and for
    ptrace of another user's process.
11. `doc_threat_model.md`: a named Settings exception in TM2; Software removed from TM1; Settings
    the only authorised writer of the three TM3 paths it writes; the five specifications that owe a
    threat model reference listed for their next revision. The project instructions already carry
    TM8 (`AGENTS.md`), so no instruction file changes.

## Consequences

Each specification carries its decisions in its decisions section and status line; this record is
the index. Follow-ups named by the decisions: the two A0 issues, the removal of the recovery kiosk
source and of the `athanor-semantic-db` directory, the "Security events" row owed by
`doc_bar.md` BR6, and spike S4 of `doc_languages.md` as the first spike.
