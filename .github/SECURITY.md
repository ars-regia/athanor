# Security Policy

## Reporting a vulnerability

Report privately through GitHub security advisories:
<https://github.com/hr-mes/athanor/security/advisories/new>.
Do not open a public issue or pull request for a vulnerability. Include the
affected component and image version, the steps to reproduce, and the impact you
expect.

## Response times

Proposed targets, pending maintainer confirmation. They are not a commitment until the maintainer confirms them.

| Step | Target |
| --- | --- |
| Acknowledge the report | 7 days |
| First assessment (accepted, needs information, or out of scope) | 14 days |
| Fix or mitigation in a signed `:stable` image, for an accepted report | 90 days; sooner for a key compromise or remote code execution |
| Public advisory | with the fix, or at 90 days at the latest, in agreement with the reporter |

## Scope

In scope: code, configuration and pipelines in this repository, and what they
ship in the image:

- the update and trust chain (`athanor-update`, the container signature policy,
  `:stable` promotion, the Secure Boot, module and image signing keys' use);
- the Azoth kernel configuration and patches carried by Athanor;
- the confinement, authorisation (polkit) and attestation code;
- the build and release workflows.

Out of scope, and better reported to the upstream project (tell us too if
Athanor's configuration makes it worse):

- vulnerabilities in unmodified Fedora, COSMIC, Linux, NVIDIA or Flatpak
  packages;
- code the user runs outside confinement: the threat model treats it as the user
  (maintainer decision A2-9, #151);
- machines outside the supported audience (see `README.md`).

## Supported versions

Athanor has no numbered release yet. The supported version is the current
`:stable` image. Security updates are provided while Fedora supports the base
release; see "Audience and support window" in `README.md`.
