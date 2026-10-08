---
paths:
  - "**/polkit*.rs"
  - "**/*.policy"
  - "**/*attestation*/**"
  - "**/*bus-api*/**"
  - "**/*trust-state*/**"
  - "system/confidential_computing/**"
  - "forge/specs/polkit/**"
  - "forge/specs/athanor-selinux/**"
  - "forge/specs/athanor-keylime/**"
  - "forge/specs/athanor-update/**"
---

# Security-critical paths

<!-- The rename of 2026-09-05 broke five globs written with the old prefix: the rule looked
     configured and loaded for none of its files. `verify.py agent-docs` now fails a glob
     that matches no tracked file. -->

You are working on a security component. `system/athanor-bus-api/src/polkit.rs` and
`system/confidential_computing/` are stop-and-ask paths (`AGENTS.md`): confirm with the
contributor before editing them.

## No security theatre

Cryptography, token validation, hashes and attestation are **real**. A placeholder, a
hard-coded value or a `return Ok(true)` in an authorisation path **is not a draft to finish
later, it is a vulnerability**. If you cannot implement the real thing now, do not write the
fake one: stop and say so.

- No post-quantum crate is a workspace dependency. `pqc_dilithium`, a pre-standard
  Dilithium rather than FIPS 204 ML-DSA, survives only in `athanor-attestation`, outside the
  workspace (ADR-0087): add no new use. Propose a KEM or signature crate in a decision record.
- Hash with `sha2`, wipe secret material with `zeroize`, compare secrets in constant time
  with `subtle`. All three are workspace dependencies.

## Least privilege, polkit, keys

- The least-privilege rule of the root `AGENTS.md` holds. If a path requires skipping the IPE
  policy or Landlock, the path is wrong; never widen permissions "to make it work".
- The polkit subject identifies **the caller**, not the bus connection
  (`verify.py polkit-subject`), and every action the code checks is declared in an installed
  `.policy` file (`verify.py polkit`).
- Keys never enter the repository: `*.key` and `*.pem` are git-ignored except the public
  certificates under `forge/specs/azoth/keys/`, and `.claude/settings.json` denies reading
  them.

## Before delivering

A change here needs a second reviewer who checks it against a concrete failure scenario,
not an opinion (`docs/operations/contributing.md`, CT6).
