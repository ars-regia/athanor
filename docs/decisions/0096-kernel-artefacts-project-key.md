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

1. The kernel's OCI artefacts are signed with the project key, with the same tool and signature
   format as the system images (`system/sign-images.sh`), in the release's existing signing step
   for system images (ADR-0064), so a release cycle asks for no additional approval. Every
   verifier of a released kernel artefact, outside the pipeline that built it, verifies it
   against `system/keys/athanor-image-*.pub` instead of the workflow's OIDC identity.
2. The keyless signature made when the kernel is published stays as a build record
   (`doc_update_trust.md` UT2, `doc_pipeline.md` PL17). The checks that run inside the pipeline
   before the release signing step (`kernel-build.yml`, `call-kernel.yml`,
   `system/kernel-artifacts.sh`, the image build) keep verifying that record, since they run on
   GitHub and would gain nothing from refusing GitHub's identity; no check after the release
   signing step, and no verifier outside GitHub, relies on it.
3. The key stays in the custody of ADR-0084. A hardware security module is not adopted for 1.0; it
   is reconsidered when a second maintainer joins, together with ADR-0084.
4. A private Fulcio and Rekor are not adopted.

## Consequences

- Issue #141 closes when the kernel artefacts verify with `system/keys/athanor-image-*.pub` and
  every verifier of a released kernel artefact (point 1) verifies them that way.
- ADR-0064's approval budget is unchanged: the kernel artefacts are signed under the approval the
  release already asks for its system images. A kernel published between releases carries only its
  keyless build record until the next release signs it.
- `doc_update_trust.md` UT2, `doc_pipeline.md` and `KERNEL.md` are amended with the
  implementation.
- This record does not concern the Secure Boot and module signing keys, which sign the kernel
  binary and its modules, not the OCI artefacts.
