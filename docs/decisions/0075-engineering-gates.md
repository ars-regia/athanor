---
id: ADR-0075
title: "Engineering gates"
date: 2026-10-06
status: accepted
issues: [223]
areas: [build, security]
---

# 0075. Engineering gates

## Context

Audit 3 (`docs/reviews/2026-10-06-audit-3/engineering.md`) found that the project's
engineering controls exist but do not bind:

- The Rust security workflow triggers only on branches that are no longer developed, and it
  suppresses its own failures.
- No pull request runs the full verifier or the crate tests, and most crate tests run
  nowhere. Since #217 the Kernel gate runs the shared lint and eight verifier checks only.
- The cargo-vet audit is fabricated, and `deny.toml` is the template with advisories ignored.
- Compiler flags and lint policy are split across four places, and the RPM macros override
  the workspace's.
- Half of the workspace ships nothing.
- D-Bus names use two prefixes.

## Decision

1. **`just check` is the pull request gate, and CI runs exactly that recipe** in a `pr.yml`
   workflow, as a required check. The recipe runs lint, the full verifier, and the tests of
   every crate and Python suite that has them.

   Current verifier failures are allow-listed in the verifier, one issue each. The gate is
   required from the start, so nothing regresses while the list empties.

2. **Supply chain.** cargo-deny is the dependency policy, made strict: licences, advisories
   with no blanket ignores, bans and sources. Each ignored advisory carries a reason and a
   review date. cargo-vet is dropped until a reviewer pool exists. A fabricated audit is never
   recorded.
3. **One place for build policy.**
   - Compiler, linker and lint policy live in the workspace `Cargo.toml` (`[workspace.lints]`
     and the profiles).
   - Release builds use thin LTO.
   - RPM builds inherit that policy instead of overriding it.
4. **No dead code in the workspace.**
   - A binary that ships nothing and has no approved specification item is deleted (see
     ADR-0073).
   - Kani and bare-metal eBPF CI jobs go with the code they target. A Kani job returns only
     with a real harness.
5. **Conventions.**
   - The D-Bus prefix is `os.athanor`, with versioned interface names (`Name1`).
   - Libraries use `thiserror`; `anyhow` is used only at the `main` of binaries.
   - Logging uses `tracing`.
   - There is one async runtime per process.

## Consequences

- **Delivery.** Workstreams W4 and W9 of the Audit 3 program apply this record.
- **Changes.**
  - `rust-security-audit.yml` runs on the default branch and on pull requests.
  - `supply-chain/` and `.trivyignore` are deleted.
  - `forge/config/rpmmacros` stops setting `RUSTFLAGS` that contradict the workspace.
- **Remaining `org.athanor` names.** The names in Rust sources are renamed in the pull
  request that next touches each crate. A verifier check keeps new ones out.
