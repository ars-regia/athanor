# Audit 3 charter (shared by every review)

## Mandate

The maintainer wants Athanor to meet the standard of a major vendor OS (macOS, Windows) or a mature
Linux distribution (Fedora, NixOS, GNOME), and to be easy to maintain for a team that works with coding
agents (Claude Code, Codex and similar). Every choice is open to challenge, the maintainer's included.
The best result wins over the easiest one. The audit reports; it never edits.

Snapshot: commit `91aefb9f78201c8425edbf1b942d798bfd8f4f7f` of `iso-v0`. The retired Ermete-era tree
is the ref `origin/main`, read with `git show origin/main:<path>` and `git ls-tree -r --name-only
origin/main <dir>`. Every crate and spec on main is named `ermete-*` where iso-v0 says `athanor-*`.

## Ground rules

- Read only. Never edit, commit, push, open PRs or issues, or post anything.
- Never open `docs/architecture/graph-vaults/`, `.codegraph/`, `graphify-out/`, lockfiles (except to
  count entries with `grep -c`), `target/`, `node_modules/`.
- Never read secrets: `.env*`, private keys, runner environment files, credentials. A public certificate or `cosign.pub` may be read.
- No builds, no `cargo build|test|check|clippy`, no podman, no network. `cargo metadata --offline
--no-deps --format-version 1` is allowed. `python3 scripts/verify.py <check>` is allowed.
- Use `rg`/`grep`/`find` to locate, then read only the line ranges you need.
- Everything you write in the findings file is in English.

## Standards to measure against

Cite the standard a finding breaks, by name.

- Requirements and specifications: RFC 2119/8174 keywords; ISO/IEC/IEEE 29148 qualities (each
  requirement unambiguous, verifiable, consistent, traceable, feasible, singular); a spec states scope,
  non-goals, rationale, interfaces, failure behaviour and acceptance criteria.
- Architecture: arc42 sections, C4 levels; decisions as ADRs (Nygard/MADR); one source of truth per fact.
- Documentation: Diátaxis (tutorial, how-to, reference, explanation kept apart); docs describe the
  tree as it is; no reference to files, branches or tools that do not exist.
- Rust: Rust API Guidelines; workspace-level `[lints]`; `unsafe` justified with `// SAFETY:`; errors
  propagated, no `unwrap`/`expect`/`panic!` in daemons (the workspace has `panic = "abort"`);
  `cargo-deny` licences/advisories; one async runtime per process; no dead code shipped.
- Packaging and system integration: Fedora Packaging Guidelines; systemd unit hardening
  (`systemd-analyze security` exposure, `DynamicUser`, `ProtectSystem=strict`, `NoNewPrivileges`,
  capability bounding); polkit actions declared, least privilege, `auth_admin` for destructive actions;
  D-Bus policy files least privilege; bootc/ostree image conventions.
- Security: no placeholder in a security path (fake crypto, fake signature or token check, hashes that
  are not hashes, TODO in an auth check); OWASP ASVS principles; threat model coverage.
- Supply chain and CI: SLSA build levels; OpenSSF Scorecard checks (pinned actions by SHA,
  least-privilege `permissions:`, no `pull_request_target` with checkout of PR code, branch protection,
  signed releases, dependency update tool, SAST, fuzzing); reproducible builds.
- Repository and governance: OpenSSF Best Practices criteria (licence, SECURITY.md, CONTRIBUTING,
  tests in CI, release notes); CODEOWNERS; templates.
- Agent maintainability: an `AGENTS.md` (agents.md convention, read by Codex and others) and/or
  `CLAUDE.md` that is true, short and points to deeper docs; commands documented and deterministic;
  checks that fail loudly; small focused files; no tribal knowledge that lives only outside the repo.

## Severity

- critical: a security hole, a fake security mechanism, data loss, or the shipped image broken.
- high: wrong behaviour, or a document that would lead a reader or an agent to act wrongly.
- medium: maintainability, redundancy, inconsistency without immediate harm.
- low: polish.

## Findings file format

Write your findings to the path your brief names, in this exact shape:

```
# <dimension title>

Snapshot: 91aefb9f. Scope: <what you covered, and what you did not>.

## Summary
<5-10 lines: the state of this dimension and the three to five things that matter most>

## Findings

### <DIM>-01 <short title>
- Severity: critical|high|medium|low
- Category: <contradiction|stale|redundancy|fake-implementation|vulnerability|missing|quality|naming|process|other>
- Where: <path:line>[, <path:line>...]
- Evidence: <what the files say, quoted briefly; verified at the snapshot>
- Standard: <the standard or rule it breaks>
- Recommendation: <the best fix, concretely; name what to delete, write or change>
- Needs a decision: no | yes: <the question, with the options and your recommended one>
```

Only report what you verified in the files. If something is a suspicion you could not verify, say
"Suspected" at the start of Evidence. Do not report the same root cause twice: one finding, several
Where entries. Prefer fewer, deeper findings over many shallow ones.
