---
id: ADR-0096
title: "Kernel artefacts are signed with the project key, not the workflow's OIDC identity"
date: 2026-10-08
status: accepted
issues: [141]
areas: [security, pipeline]
---

# 0096. Kernel artefacts are signed with the project key, not the workflow's OIDC identity

## Context

System images are signed with the project key (`system/sign-images.sh`, `doc_update_trust.md`
UT2), whose public halves are `system/keys/athanor-image-*.pub`. The kernel's OCI artefacts
(kernel, devel, debuginfo and the MicroVM guest kernel, `.github/workflows/kernel-build.yml`) are
signed and attested with cosign keyless, under the OIDC identity of the workflow. Verifying them
therefore means trusting GitHub's OIDC issuer. Issue #141 asks for the 1.0 trust root: a project
key held in a hardware security module, or a private Fulcio and Rekor.

## Decision

On 2026-10-08 the maintainer decided:

1. The kernel's OCI artefacts are signed and attested with cosign and the project key, as the
   system images are, through the same signing environment. No verifier in the project depends on
   GitHub's OIDC identity for them.
2. The key stays in the custody of ADR-0084. A hardware security module is not adopted for 1.0; it
   is reconsidered when a second maintainer joins, together with ADR-0084.
3. A private Fulcio and Rekor are not adopted.

## Consequences

- Issue #141 closes when the kernel artefacts verify with `system/keys/athanor-image-*.pub` and
  every consumer (`kernel-build.yml`'s own checks, the image build, `doc_pipeline.md`) verifies them
  that way.
- Each kernel publication waits for the maintainer's approval of the signing environment, as an
  image publication does.
- This record does not concern the Secure Boot and module signing keys, which sign the kernel
  binary and its modules, not the OCI artefacts.
