---
id: W1-ACCESSIBILITY
title: "doc_accessibility decisions (wave 1)"
date: 2026-10-05
status: accepted
issues: []
areas: [accessibility, shell]
---

# 0001. doc_accessibility decisions (wave 1)

## Context

Recorded in the maintainer decision log, section 'Wave 1 maintainer decisions', under the heading doc_accessibility. The options are lettered as in the specification's own question list; the specification lists the alternatives.

## Decision

- 1 A GNOME keys
- 2 A athanor-a11y
- 3 A greeter now, persisted
- 4 B cosmic-comp patch after spike
- 5 A athanor-osk
- 6 B cosmic-comp patch (orca cgroup)
- 7 whole-bus AT-SPI proxy (readers gated by cgroup: orca.service + tests), spike on Orca latency first; risk recorded until then. ALSO: research GNOME Newton status and evaluate building our own compositor-mediated a11y. Newton research done (newton-research.md): parked since mid-2024, build proxy (a) with a single replaceable reader gate (SO_PEERCRED + pidfd cgroup), do not build own Newton (b); revisit on wayland-protocols !493 dbus_annotation or AT-SPI3
- 8 A Ctrl+Alt+Tab, Super+Alt+A

## Consequences

Elaborated in `docs/architecture/doc_accessibility.md` (on shell-specs).
