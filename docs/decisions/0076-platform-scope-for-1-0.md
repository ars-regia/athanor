---
id: ADR-0076
title: "Platform scope for 1.0"
date: 2026-10-06
status: accepted
issues: [223]
areas: [platform, security, build, docs]
---

# 0076. Platform scope for 1.0

## Context

Audit 3 (`docs/reviews/2026-10-06-audit-3/platform-specs.md` and `forge-packages.md`) left
several platform questions open. None of them involves signing keys:

- **Execution control.** A2-8 gave up IPE for 1.0 and moved execution control to fs-verity
  and signed images. That left 1.0 without a stated control, and the specifications
  still name IPE.
- **The DNF channel.** The workflow still defines a GitHub Pages DNF channel, whose deploy
  step is gated on the retired `main`; images consume the tier repositories as OCI images.
- **Factory reset.** It has no scope (D36).
- **Local AI.** doc_local_ai is stale, and its scope for 1.0 is undecided.
- **The release package.** `athanor-base-config` obsoletes `fedora-release` and is installed
  with `--nodeps`.
- **The builder.** forge builds binary RPMs in place, without SRPMs, in its own Nix builder
  container, while Fedora's standard isolated builder is mock.
- **Specification structure.** The specifications have no common structure.

## Decision

1. **Execution control for 1.0:**
   - ostree's composefs backend for `/usr`, with fs-verity where the filesystem supports it;
   - `noexec` on the system-writable temporary mounts.

   User code stays the user's (A2-9). doc_kernel_profile states the mount list and the
   residual risk. The sealed composefs of 1.1 (A2-8) remains the target.

2. **The GitHub Pages DNF channel is removed.** The tier repositories are consumed as OCI
   images only.
3. **Factory reset in 1.0 keeps the user's home.** It performs a three-way reset of `/etc`
   and removes layered state. A full wipe is out of scope for 1.0.
4. **Local AI is out of the 1.0 scope.** The spikes stay. doc_local_ai is marked deferred.
5. **Athanor carries its own release package**, the Fedora Remix route, in place of
   obsoleting `fedora-release` and installing with `--nodeps`.
6. **forge builds SRPMs in the current builder container**, then binary RPMs from them.
   mock is adopted when aarch64 builds become real.
7. **One specification template.** It has fixed headings: scope, non-goals, rationale,
   interfaces, failure behaviour and acceptance criteria. Each specification adopts it at its
   next revision, and `verify.py docs` enforces it on revised documents.

## Consequences

- **Record status.** This record amends A2-8 (execution control for 1.0).
- **Delivery.** Workstreams W2 (SRPM builds), W3 (release package), W4 (removal of the Pages
  channel), W6 (template and the IPE text) and W7 (recovery and reset) of the Audit 3
  program apply it.
- **Specification changes.**
  - doc_kernel_profile and doc_update_trust replace their IPE text.
  - doc_local_ai is marked deferred.
