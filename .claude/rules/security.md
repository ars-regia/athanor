---
paths:
  - "**/polkit*.rs"
  - "**/*.policy"
  - "**/*attestation*/**"
  - "**/*bus-api*/**"
  - "**/*mesh*/**"
  - "**/*cloud-rs/**"
  - "system/confidential_computing/**"
---

# Security-critical paths

<!-- The globs name the function (`*attestation*`, `*bus-api*`, `*mesh*`), not the product.
     The Ermete -> Athanor rename of 2026-09-05 broke five of the six globs written with the
     old prefix: the rule stayed in the repository, looked configured, and loaded for none
     of the files it protects. Do not reintroduce fixed prefixes. -->

You are working on a security component. `system/athanor-bus-api/src/polkit.rs` and
`system/confidential_computing/` are stop-and-ask paths (`AGENTS.md`): confirm with the
contributor before editing them.

## No security theatre

Cryptography, token validation, hashes and attestation are **real**. A placeholder, a
hard-coded value or a `return Ok(true)` in an authorisation path **is not a draft to finish
later, it is a vulnerability**. If you cannot implement the real thing now, do not write the
fake one: stop and say so.

- Key encapsulation: `ml-kem`. Do not use `pqc_kyber` in new code: `deny.toml` ignores its
  timing advisory RUSTSEC-2023-0079 (KyberSlash) without a recorded reason.
- Do not add new uses of `pqc_dilithium`, a pre-standard Dilithium rather than FIPS 204
  ML-DSA. No ML-DSA crate is a workspace dependency yet: propose one in a decision record.
- Hash with `sha2`, wipe secret material with `zeroize`, compare secrets in constant time
  with `subtle`. All three are workspace dependencies.

## Zero-trust

- No daemon or application outside a compartment or a MicroVM.
- The IPE policy, Landlock confinement and the compartments are never bypassed. If a path
  requires skipping them, the path is wrong.
- Never `chmod 777`, never widen permissions "to make it work".

## Polkit

The polkit subject identifies **the caller**, not the bus connection. This defect was fixed
once: do not reintroduce it (`python3 scripts/verify.py polkit-subject`).

Every polkit action the code checks is declared in an installed `.policy` file:
`python3 scripts/verify.py polkit`.

## Secrets

Keys never enter the repository: `*.key` and `*.pem` are git-ignored except the public
certificates under `forge/specs/azoth/keys/`, and `.claude/settings.json` denies reading
them. If a value is needed, ask for the environment variable's name, never its content.

## Before delivering

A change here needs a second reviewer who checks it against a concrete failure scenario,
not an opinion (`docs/operations/contributing.md`, CT6).
