# Audits

| Field | Value |
| --- | --- |
| Status | rules in force, moved unchanged from the root `AGENTS.md` on 2026-10-09 |
| Defines | how defects found by audits become checks, when audits run, what they re-examine |

Automate the class, audit the delta, probe the runtime.

- **Every class of defect an audit finds becomes an automatic check** (`verify.py`, a CI
  step, the VM acceptance) in the pull request that fixes it, or an issue that names the
  check to add. A finding without one is expected to come back.
- **Delta audits, triggered by events.** Starting from the previous audit's report and
  covering only what changed since, one runs before every milestone (public ISO, `:stable`,
  1.0), after a Fedora major release bump, and after a batch of merges in a sensitive area
  (updates, signing, PAM, SELinux, the Gatekeeper). No full-repository audit on a calendar.
- **A short runtime audit every month** on the booted signed image, even without events:
  exposed services, `systemd-analyze security`, SELinux denials, PAM, the update and trust
  state.
- **Every audit re-examines every decision within its scope**: a full audit all of them, a
  delta audit every decision that bears on what changed, listed in its report first. The
  maintainer's and the agents' alike, none excluded: decision records, answers to review
  batches, defaults, simplifications and deferrals are subjects of the audit, not premises.
  Where an easy option and a harder one that pays off more exist, the audit recommends the
  harder one and states its cost. A reversal still needs an amending decision record and
  the maintainer's approval *(maintainer decisions, 2026-10-09)*.
- **Reports stay outside the public repository.** Issues and pull requests name the fix,
  not how to exploit what it fixes.
