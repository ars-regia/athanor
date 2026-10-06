---
id: W1-LANGUAGES
title: "doc_languages decisions (wave 1)"
date: 2026-10-05
status: accepted
issues: []
areas: [shell, i18n]
---

# 0005. doc_languages decisions (wave 1)

## Context

Recorded in the maintainer decision log, section 'Wave 1 maintainer decisions', under the heading doc_languages. The options are lettered as in the specification's own question list; the specification lists the alternatives.

## Decision

- 1 B gettext-rs in apps, athanor-i18n in shell+greeter
- 2 A PRs on .po for release 1
- 3 C all glibc locales, marked complete/partial
- 4 A cosmic-comp xkb_config via athanor-compositor-client
- 5 A IBus, spike S1 confirms or reverses
- 6 A 40-athanor-locale1.rules AUTH_ADMIN_KEEP

## Consequences

Elaborated in `docs/architecture/doc_languages.md`.
