---
id: A2-27
title: "Signing approvals and MOK enrolment"
date: 2026-10-06
status: amended by A2-35
issues: [131, 145]
areas: [signing, installer]
---

# 0064. Signing approvals and MOK enrolment

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md. Log annotation: signing #131/#145.

## Decision

At most two approvals per release cycle: the NVIDIA kmod `sign` job, which holds the Secure Boot and module keys and signs each new kernel's vmlinuz and modules, then sign-system-images, which holds the cosign key. A cycle without a kernel or NVIDIA change asks only for sign-system-images. No job that builds or runs a third-party action holds a key; `verify.py workflows` enforces this. MOK enrolment offered on a first-run page that prepares it with a system-generated one-time password shown to the user (CLI stays). athanor-tpm-luks-seal disabled until 1.1 (UKI + signed PCR 11 policy); 1.0 unlocks with the passphrase.

## Consequences

`docs/architecture/doc_ci.md` cites this record for the signing environments of CI5 and CI6, as do `docs/operations/secrets.md` and `docs/operations/github-settings.md`; it takes effect through the work it describes.

A2-35 amends the placement of the MOK enrolment page: it moves from a first-run page to the last page of the installer. The rest of this decision stands.

Amended 2026-10-07 by the maintainer: the first sentence of the decision, which read "two approvals per release cycle stay (sign-kernel, then sign-system-images).", now states the approvals as signed off on that date. The two approvals are those of two GitHub environments, each holding only its own keys: `signing-kernel` (`SECUREBOOT_SIGNING_KEY`, `MODULE_SIGNING_KEY`) for the `sign` job of `nvidia-kmod.yml`, and `signing-images` (`COSIGN_PRIVATE_KEY`, `COSIGN_PASSWORD`) for `sign-system-images` of `call-system-image.yml`. `.github/settings/environments.json` records which environment holds which key, `docs/operations/github-settings.md` section 7 their protection, and `doc_ci.md` the jobs. The rest of the decision is unchanged.
