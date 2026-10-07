---
id: ADR-0087
title: "Keep athanor-attestation outside the workspace until its Keylime rewrite"
date: 2026-10-07
status: accepted
issues: [151, 201]
areas: [security, build]
---

# 0087. Keep athanor-attestation outside the workspace until its Keylime rewrite

## Context

ADR-0073 item 1 retires `system/confidential_computing` with the other components that have
no product role. Its crate, `athanor-attestation`, verifies AMD SEV-SNP and Intel TDX reports
and releases keys to verified machines, but the verification does not check what it claims
(#201) and its post-quantum labels name parameter sets it does not build (#206). Under the
three-tier threat model (A2-9, `doc_threat_model.md`, TM7) attestation is outside 1.0, and
`doc_kernel_profile.md` places a rewrite on Keylime, with its PCR 11 and PCR 12 policy, before
mesh admission depends on attestation.

## Decision

The maintainer decided on 2026-10-07 to keep the crate in the tree instead of deleting it:

- `system/confidential_computing/athanor-attestation` leaves the workspace members and joins
  `exclude`, so it is neither built nor shipped;
- its source stays where it is, a protected path; the Keylime rewrite starts from an
  approved specification, as ADR-0073 item 6 requires;
- #201 and #206 stay open until that rewrite replaces it.

## Consequences

This amends ADR-0073 only for `system/confidential_computing`: the rest of item 1 is
unchanged. `docs/architecture/components.toml` lists the crate as `out-of-1.0`, issue #151,
and `experimental/EXEMPT` records why it is outside the workspace.
