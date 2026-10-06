# Security Policy

## Supported versions

Athanor has no numbered release yet. The supported version is the current system image
built from the default branch `iso-v0`, and no other branch or older image receives
security fixes. Decision A2-4 (`docs/decisions/0039-delivery-repairs-before-1-0.md`)
creates an evidence-gated `:stable` tag before 1.0; once it is published, `:stable` is the
supported channel. Security updates are provided while Fedora supports the base
release; see "Audience and support window" in `README.md`.

## Reporting a vulnerability

Do not open a public issue or pull request for a vulnerability. Report it privately through
GitHub's private vulnerability reporting ("Report a vulnerability" under the repository's
Security tab, <https://github.com/hr-mes/athanor/security/advisories/new>). Include the
affected component and image version, the steps to reproduce, and the impact you expect.

## Response times

Targets confirmed by the maintainer (decision A2-25, `docs/decisions/0062-governance-targets-confirmed.md`).
Athanor is maintained by one person, so they are targets, not guarantees.

| Step | Target |
| --- | --- |
| Acknowledge the report | 7 days |
| First assessment (accepted, needs information, or out of scope) | 14 days |
| Fix or mitigation in the image built from `iso-v0`, for an accepted report | 90 days |

## Scope

In scope are the parts that ship in the system image: the kernel and its signing keys, the
image build and signing pipeline, the update path (`athanor-update`), the desktop portal
backend and the shell, and the confinement, authorisation (polkit) and attestation code.
Crates listed in `experimental/EXEMPT` are not part of the image.

Out of scope, and better reported to the upstream project (tell us too if Athanor's
configuration makes it worse):

- vulnerabilities in unmodified Fedora, COSMIC, Linux, NVIDIA or Flatpak packages;
- code the user runs outside confinement: the threat model treats it as the user
  (maintainer decision A2-9, #151);
- machines outside the supported audience (see `README.md`).
