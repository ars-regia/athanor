---
id: PHONE
title: "No phone integration in release 1"
date: 2026-10-05
status: accepted
issues: []
areas: [fleet, product]
---

# 0013. No phone integration in release 1

## Context

Recorded in the maintainer decision log, section 'Phone (maintainer, 2026-10-05)'.

## Decision

- Release 1 has NO phone integration. The KDE Connect companion draft is archived as archived-doc_phone-kdeconnect.md (its research stays reusable: KDE Connect v8 facts, pairing-code weakness, firewalld helper pattern).
- Vision, in the maintainer's words: "athanor dovrebbe connettere tutti i pc dello stesso proprietario, sarebbe bella un app dedicata al telefono, per poter controllare i pc da telefono".
- Order: doc_fleet first (owner identity over Cloudflare Zero Trust, the owner's PCs recognising each other), then doc_phone as a client of the fleet. Construction of both after 1.0.
- The phone app must cover: status and actions (on, locked, updated; lock, suspend, restart, wake), remote desktop, notifications and files, approvals (phone as a second factor for a login, an admin action, a USB device). It should be expandable, and every capability is a switch the user turns on or off.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
