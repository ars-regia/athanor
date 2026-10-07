# Security Policy

This is the coordinated vulnerability disclosure (CVD) policy of Athanor. It follows the
structure of ISO/IEC 29147 and is part of the project's cybersecurity posture described in
[ADR-0081](../docs/decisions/0081-cra-compliance-posture.md) and
[`docs/compliance/`](../docs/compliance/README.md). It is a statement of intent by the
maintainer, not a legal commitment.

## Supported versions and support period

Athanor has no release yet. Until the first release, security fixes land on the development
branch `iso-v0`; no other branch receives them.

Once released, the support period is **five years for the product line**, counted from the
date the line is placed on the market, with the Fedora base rebased forward within the line
(ADR-0081). Only the latest release of the line is supported; moving to it is a free,
in-place update.

- Support end date: **2031-12-31 (provisional until the first release date is fixed)**.
- The date is published here, in the release notes of each release, and as `SUPPORT_END` in
  `/usr/lib/os-release` of every image
  (`forge/specs/athanor-base-config/SOURCES/usr/lib/os-release`, the single source).

## Reporting a vulnerability

Do not open a public issue or pull request for a vulnerability.

1. Use GitHub private vulnerability reporting: open the repository's **Security** tab and
   choose **Report a vulnerability**.
2. E-mail channel: `<SECURITY E-MAIL ADDRESS: to be set by the maintainer>`.

Please include the affected component and version (image variant and `VERSION` from
`/usr/lib/os-release`), a description of the impact, and the steps to reproduce. Do not
include data belonging to other people.

## What happens next

These are the response times the maintainer aims for. Athanor is maintained by one person,
so they are targets, not guarantees.

| Step                                     | Target                           |
| ---------------------------------------- | -------------------------------- |
| Acknowledgement of the report            | 7 days                           |
| First assessment (accepted, or why not)  | 14 days                          |
| Coordinated public disclosure            | 90 days after the report, sooner when a fix is available |

A fix is published as a security update (separate from feature updates, free of charge) and
described in a GitHub security advisory with a CSAF 2.0 document, naming the affected
versions, the impact, the severity and the remediation. Reporters are credited unless they
ask not to be. When a report concerns a component we integrate, we report it to its
upstream and share the fix.

A vulnerability that is being actively exploited, or a severe incident, is also handled
under the reporting runbook in
[`docs/compliance/reporting-runbook.md`](../docs/compliance/reporting-runbook.md).

## Scope

In scope are the parts that ship in the system image: the kernel (Azoth) and its signing
keys, the image build, signing and promotion pipeline, the update path (`athanor-update`),
the desktop portal backend, the shell and the other components built from `forge/specs/`.
Crates listed in `experimental/EXEMPT` are not part of the image and are out of scope.

Out of scope: vulnerabilities in upstream Fedora or third-party packages that are not
specific to how Athanor integrates them (report those upstream; tell us if Athanor is
affected), social engineering, denial of service by resource exhaustion on hardware you do
not own, and physical attacks outside the threat model in `docs/architecture/`.

## Safe harbour

We will not pursue or support legal action against anyone who, in good faith, researches
and reports a vulnerability under this policy: you test only against systems you own or
have permission to use, you avoid privacy violations and service disruption, you do not
exfiltrate or keep data beyond what is needed to demonstrate the issue, and you give us the
disclosure time above. This is a statement of the maintainer's intent; it cannot bind third
parties such as upstream projects or hosting providers.
