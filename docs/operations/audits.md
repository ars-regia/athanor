# Audits

| Field | Value |
| --- | --- |
| Status | rules in force; calendar and audit types decided by the maintainer on 2026-10-09 ([ADR-0102](../decisions/0102-audit-calendar.md)) |
| Defines | how defects found by audits become checks, when audits run, what they re-examine |

Automate the class, audit the delta, probe the runtime.

## Rules

- **Every class of defect an audit finds becomes an automatic check** (`verify.py`, a CI
  step, the VM acceptance) in the pull request that fixes it, or an issue that names the
  check to add. A finding without one is expected to come back. Agents spend judgement;
  what a script can repeat, a script repeats.
- **Delta audits** start from the previous audit's report and cover only what changed
  since. They run on the calendar below and, in addition, before every milestone (public
  ISO, `:stable`, 1.0), after a Fedora major release bump, and after a batch of merges in
  a sensitive area (updates, signing, PAM, SELinux, the Gatekeeper). No full-repository
  audit runs on a calendar: the broad audit belongs to milestones.
- **Every audit re-examines every decision within its scope**: a full audit all of them, a
  delta audit every decision that bears on what changed, listed in its report first. The
  maintainer's and the agents' alike, none excluded: decision records, answers to review
  batches, defaults, simplifications and deferrals are subjects of the audit, not premises.
  Where an easy option and a harder one that pays off more exist, the audit recommends the
  harder one and states its cost. A reversal still needs an amending decision record and
  the maintainer's approval *(maintainer decisions, 2026-10-09)*.
- **Reports stay outside the public repository.** Issues and pull requests name the fix,
  not how to exploit what it fixes.

## Calendar

| Audit | When | Who runs it |
| --- | --- | --- |
| Reviewer on the pull request | every pull request, before merge | an agent (the reviewer) |
| Delta audit: security, pipeline, packages, decisions | weekly, or every 15 to 20 merged pull requests, whichever comes first | agents |
| Runtime audit: the documented promises checked on a booted VM | every image that is a promotion candidate, and at least monthly on the booted signed image (exposed services, `systemd-analyze security`, SELinux denials, PAM, update and trust state) | agents; the mechanical part moves into the ISO acceptance |
| Coherence of open pull requests | when two or more open pull requests touch the same area | an agent |
| Documents against the tree: cited lines, versions, workflows | monthly | an agent; cited-line checks move into `verify.py` |
| CI review | quarterly, and after a large pipeline change | an agent; `actionlint` covers the mechanical part, a workflow linter such as `zizmor` is to add |
| Specifications against decision records | quarterly, and before 1.0 | an agent |
| Broad audit: comparison with other projects, ecosystem, trust model | every milestone: before 1.0, then every major release | agents |
| Agent rules and the permission gate | monthly | an agent, adversarial |

Specification reviews run when a specification asks for approval, the 1.0 inventory on
request, and research when a question needs it: none of them is periodic.

## Audit types

Decided on 2026-10-09; none of them exists yet. Each lands with its own pull request, and
until it does an issue names it.

| Type | When | Who runs it |
| --- | --- | --- |
| Vulnerabilities of the published image: a scan of its SBOM (osv-scanner or grype), plus an end-of-life calendar for the Azoth kernel base, the Flatpak runtimes and the Fedora release | nightly scan; the calendar is reviewed with the monthly documents audit | CI |
| Update and rollback drill on real hardware: upgrade from each of the two previous images, roll back, migrate through the bridge while it is open | every release | the maintainer, on the laptop, with an agent's checklist |
| Key recovery drill: restore from the LUKS2 backup, a simulated rotation, a revocation | quarterly | the maintainer |
| Confinement escape attempts from a confined application: Landlock, bwrap, the portals, the filtered bus | quarterly | an agent, on a VM |
| Build reproducibility: build one commit twice and compare the images | monthly | CI |
| Accessibility: an Orca walk through the greeter, the bar and Settings, and a diff of the AT-SPI tree | every release | the maintainer for the Orca walk; CI for the tree diff |
| Resource regressions: boot time, bar start-up, memory against the per-service limits of `doc_shell_standard.md` (ST5) | every image, kept as a trend | CI |
| Licences of the image | every release | CI, reviewed by an agent |

Fuzzing is not on the calendar because CI already repeats it: 15 `cargo-fuzz` targets run
weekly in `fuzz.yml`, with the corpus replayed in the gate (PR #310).
