---
id: ADR-0143
title: "Pre-1.0 audit decisions of 2026-11-20: the confined class, one build policy, the kernel and alerting gates, the firewall acceptance and the recovery console"
date: 2026-11-20
status: accepted
issues: [1066, 1347, 1350, 1353, 1355, 1357, 835]
areas: [security, build, process, kernel, shell, docs]
---

# 0143. Pre-1.0 audit decisions of 2026-11-20: the confined class, one build policy, the kernel and alerting gates, the firewall acceptance and the recovery console

**Accepted** by the maintainer with the pull request that records it. On 2026-11-20 the maintainer
answered, as selectable options, the questions that the broad pre-1.0 audit of that day raised, and
the decisions below follow those answers. Approving this record approves the decisions below.

## Context

`docs/operations/audits.md` requires every audit to re-examine the decisions it meets, and a
reversal needs a record that amends the old one. The audit's report stays outside the public
repository; the issues named under each decision carry what the repository needs. Each decision
below reverses or completes one recorded earlier.

- The confined application class (`doc_session_daemons.md` SD8, SD9 and SD22 steps 6 and 7; tier 2
  of `doc_threat_model.md`, TM2 and TM3) has no step in 1.0. The "Release 1.0" paragraph of
  `doc_session_daemons.md` says that `athanor-broker` and SD22 steps 4 to 7 "are not placed by
  ADR-0138", and ADR-0138 decision 3 brings forward only the owners of the roles of
  cosmic-settings-daemon. Until the broker runs, the limits that `doc_files.md` FM3 and FM4
  declare stand.
- [ADR-0075](0075-engineering-gates.md) decision 3 says that compiler, linker and lint policy live
  in the workspace `Cargo.toml`, that release builds use thin LTO, and that "RPM builds inherit
  that policy instead of overriding it". Its Consequences say that `forge/config/rpmmacros` "stops
  setting `RUSTFLAGS` that contradict the workspace". `forge/config/rpmmacros` still sets
  `RUSTFLAGS` (`%rustflags`, with `codegen-units=16`) and `CARGO_PROFILE_RELEASE_LTO="thin"` for
  every build through `%__spec_build_pre`, while the `[profile.release]` of `Cargo.toml` says
  `lto = true` and `codegen-units = 1`.
- The weekly reproducibility build of the kernel (`repro` in `kernel-weekly.yml`) blocks nothing:
  the header of that workflow says "Nothing is blocked by it", and `doc_ci.md` CI10 says "Blocks
  nothing". [ADR-0110](0110-kernel-artifact-promotion.md) point 5 keeps it as "the independent
  audit of promoted builds". The job is red today (#1066).
- `doc_pipeline.md` PL51 says that every scheduled workflow ends with an alert job, and that
  "the other scheduled workflows gain it with PB2" (section 12). Two of the eleven workflows with
  an `on.schedule` trigger carry that job today, `kernel-weekly.yml` and `maintenance.yml`.
- `scripts/devvm/acceptance/expectations.toml` sets `[firewall] required = false`, from the pull
  request that added the runtime audit probes to the VM acceptance (#423). With it, a missing
  `firewall-cmd` is not a failure in `scripts/devvm/acceptance/probes.py`.
- [ADR-0141](0141-everything-own-in-1-0.md) decision 5 ports the recovery console to Sal.
  `doc_recovery.md` (revision 2, approved) specifies a text console (R1), and R4 retires the kiosk
  and deletes its source, `forge/specs/athanor-recovery/athanor-recovery-1.0.0`, and the legacy
  glass theme. That source is still in the tree, outside the workspace, and `athanor-recovery` has
  a line in `forge/config/own-allow.txt`.

## Decision

1. **The confined application class is placed in 1.0** (tracked by #1347), as an explicit
   placement under [ADR-0135](0135-faster-road-to-1-0.md) decision 2 (c). In this order:
   1. the unit properties of the launch path (`doc_session_daemons.md` SD8), carried by the `Unit`
      type of `athanor-sulphur-client`;
   2. the `xdg-dbus-proxy` filter, one per confined application (SD9).

   Both go into the launch path that exists today: the `Unit` of `athanor-sulphur-client`, which
   every launch of the shell already starts as a transient unit of the user manager (BR2.3). They
   do not wait for the broker. The broker (SD7, SD22 steps 6 and 7) is placed in 1.0 with them,
   keeps the order of SD22, with S1, S2, S3 and S5 of its table first, and takes over the launch
   path, the properties and the filter included, when it lands. This completes ADR-0138, which
   places no broker step in 1.0. Until
   the two steps land, the limits declared in `doc_files.md` FM3 and FM4 stand, and tier 2 of the
   threat model (TM2, TM3) is not met for the `confined` class.

2. **`Cargo.toml` is the single source of the build policy of the shipped Rust binaries** (#1350).
   The overrides in `forge/config/rpmmacros` (`%rustflags` and the `CARGO_PROFILE_RELEASE_*` exports of
   `%__spec_build_pre`) are removed from the build of the workspace's packages, and so is any spec that overrides
   `RUSTFLAGS` or the profile for one. This executes ADR-0075 decision 3, and amends its sentence
   "release builds use thin LTO": the release profile is the one `Cargo.toml` states, today
   `lto = true` and `codegen-units = 1`. The flags of `%rustflags` that are not profile settings
   stay, in the workspace's `.cargo/config.toml`: `target-cpu=x86-64-v3`, which the hardware
   audience of [ADR-0054](0054-audience-and-support-window.md) needs, and the mold linker. With the
   `RUSTFLAGS` override gone, the hardening flags that `.cargo/config.toml` already sets (full
   RELRO, PIC, frame pointers, `-D unsafe_code`) apply to the RPM builds too.
   If the build time proves too high, `Cargo.toml` itself changes, by a record that amends this
   one, and the macros do not return. The macros also apply to upstream Rust packages that the
   forge builds; which of them keep a flag is decided in the pull request that removes the
   override, and a flag that an upstream package needs moves to its spec.

3. **Kernel reproducibility blocks the 1.0 release** once the current failure is fixed (#1066).
   Until then the `repro` job stays advisory and #1066 tracks it. From the fix, a green `repro` of
   the pin that 1.0 ships is a condition of the release (`docs/operations/release-1.0.md`), and a
   red one is a release blocker, not an issue to open. The job stays weekly and stays out of the
   pull request gate. This amends ADR-0110 point 5, which makes `repro` an audit that gates
   nothing, and the sentences of `kernel-weekly.yml` and `doc_ci.md` CI10 that say it blocks
   nothing. `bench` stays advisory.

4. **Every scheduled workflow carries the alert job of PL51 now, not at PB2** (#1353), and
   `scripts/verify.py` gets a rule that requires one in every workflow with an `on.schedule`
   trigger. The rule joins the workflow checks and fails a pull request that adds a scheduled
   workflow without it. This amends `doc_pipeline.md` PL51 and section 12, where the other
   workflows "gain it with PB2".

5. **The firewall acceptance is required** (#1357). `[firewall] required` becomes `true` in
   `scripts/devvm/acceptance/expectations.toml`, and the image build asserts that the firewall is
   present, so that a missing firewall fails at the build and not
   only in the acceptance. This reverses the `required = false` of #423.

6. **ADR-0141 decision 5 is amended for the recovery console** (#1355). The recovery console stays
   the text console that `doc_recovery.md` specifies (R1), which is Athanor's own and needs no
   port to Sal. The dead GTK kiosk source, `forge/specs/athanor-recovery/athanor-recovery-1.0.0`,
   the legacy glass theme of `athanor-style` (`system/athanor-style/src/glass.rs`) and the
   `athanor-recovery` line of `forge/config/own-allow.txt` are removed, as R4 and R10 decided on
   2026-10-08. A graphical recovery, if one is ever built, is written from scratch in
   Sal (R4) and is not a 1.0 item.

## Tracking corrections, not reversals

- The bootc half of P6 belongs in 1.0. [ADR-0103](0103-audit-5-decisions.md) D26 moves only "the
  dm-verity part of P6" to 1.1, so what remains of P6 under bootc stays in 1.0. Issue #128 is
  milestoned "After 1.0" and is corrected to 1.0. No decision changes.
- `doc_overview.md` is revised for [ADR-0141](0141-everything-own-in-1-0.md) decision 4, which
  returns the overview to 1.0 from 1.1 (#835). Its revision 3 still says that the overview "stays
  in release 1.1 (ADR-0135)". No decision changes.

## Alternatives rejected

- **Keep the confined class after 1.0.** It leaves tier 2 of the threat model unmet at the
  release, with the declared limits of `doc_files.md` FM3 and FM4 as the only answer.
- **Keep the macro overrides for build speed.** Two build policies for the same binaries diverge
  silently, which is what ADR-0075 decision 3 was written to stop. A measured build time is the
  reason to change `Cargo.toml`, in the open.
- **Alert at PB2.** PB2 is a pipeline pull request with other gates, and until it lands a failed
  scheduled run raises no issue. The rule costs one job per workflow.
- **Port the recovery console to Sal.** The text console is the console of a failed session, and
  it should depend on as little of the session as it can.

## Consequences

- Specifications and documents to revise, each under [ADR-0074](0074-agent-and-contributor-model.md):
  - `doc_session_daemons.md`: the "Release 1.0" paragraph, and SD22 steps 6 and 7;
  - `doc_files.md` FM3 and FM4, which state the limits that end with the broker;
  - `doc_pipeline.md` PL51 and the PB2 row of section 12; `doc_ci.md` CI10;
  - `release-1.0.md`: the kernel reproducibility condition, and the confined class;
  - `doc_overview.md`, for ADR-0141 decision 4;
  - `forge/README.md`, section "Compiler flags", for decision 2;
  - `doc_recovery.md` needs no revision: it already says what decision 6 keeps.
- The first work of decision 1 is the unit properties of the launch path. The broker's
  implementation touches process launch and confinement and is stop-and-ask under `AGENTS.md`
  where it meets authentication or polkit.
- Amended:
  - [ADR-0075](0075-engineering-gates.md) decision 3: the release profile is the one of
    `Cargo.toml`, not thin LTO;
  - [ADR-0138](0138-own-components-in-1-0.md): the placements of 1.0, to which decision 1 adds the
    confined class;
  - [ADR-0110](0110-kernel-artifact-promotion.md) point 5: the weekly `repro` is a release
    condition from the fix of #1066;
  - [ADR-0141](0141-everything-own-in-1-0.md) decision 5: the recovery console is not ported.
- The status lines and the index name this record for ADR-0075, ADR-0110, ADR-0138 and ADR-0141.
