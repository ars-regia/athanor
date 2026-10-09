---
id: ADR-0085
title: "Keyring prompter: the secret exchange comes from upstream oo7"
date: 2026-10-07
status: amended by ADR-0103
issues: [151]
areas: [security, shell]
---

# 0085. Keyring prompter: the secret exchange comes from upstream oo7

## Context

A2-7 (ADR 0042) asks that the keyring prompter of `doc_lock_and_prompts.md` (LP12, D11) reuse
oo7's secret exchange `sx-aes-1` and write no cryptography of its own. oo7's public API does
not offer the client side of that exchange: it is private to oo7-daemon, and the `oo7` crate
exposes only AES behind its `unstable` feature, with Diffie-Hellman and key derivation kept
`pub(crate)`. The specification listed four options as open decision L12: (a) propose
upstream that oo7 expose the client side of the secret exchange; (b) depend on `oo7` with the
`unstable` feature, pinned, which gives AES only; (c) another audited library; (d) copy
oo7-daemon's code into the prompter.

## Decision

Option (a). Athanor proposes upstream that oo7 expose the client side of the secret exchange
(`sx-aes-1`). The prompter's implementation plan (LP12) waits for it. No cryptography of our
own is written in the meantime.

## Consequences

`docs/architecture/doc_lock_and_prompts.md` records L12 as decided (LP12, L12, D11). The
prompter stays deferred to the Fedora 45 milestone (A2-31) and gcr3's `gcr-prompter` stays
until then. The upstream proposal is not yet made.
