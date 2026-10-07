---
id: ADR-0083
title: "One supported desktop; derived images stay open"
date: 2026-10-07
status: accepted
issues: []
areas: [product, shell, update]
---

# 0083. One supported desktop; derived images stay open

## Context

The maintainer asked whether Athanor should bind its users to its own interface, as other
image-based systems do or do not.

- Fedora's Atomic Desktops ship one image per desktop (Silverblue, Kinoite, Sway Atomic,
  Budgie Atomic, COSMIC Atomic). Users move between them with `rpm-ostree rebase`; layering a
  second desktop onto an image is possible and discouraged.
- Opinionated image-based products ship one desktop: Bluefin (GNOME, with Aurora as a
  separate KDE product), openSUSE Aeon (GNOME, with Kalpa for KDE), Vanilla OS, elementary
  OS, Pop!_OS, Endless OS, ChromeOS.
- Much of Athanor's security lives in its shell: the greeter and the lock screen
  (`doc_lock_and_prompts.md`), the SystemPrompter and the trusted path (A2-6, A2-7), the
  launches in a security context and the session daemons (`doc_shell.md`,
  `doc_session_daemons.md`). Another desktop on the same image has none of them.
- bootc lets anyone build an image `FROM` a published one. Packages layered with
  `rpm-ostree` make bootc refuse `upgrade` and `switch` (`doc_update_trust.md`, UT3).

## Decision

1. **Athanor ships and supports one desktop, its own.** The signed system images (S2 of
   `doc_system_image.md`) carry only the Athanor shell. No official KDE or GNOME variant is
   published before 1.0.
2. **Derived images stay open.** Nothing in the image or the update service prevents a user
   from building, signing and following an image `FROM` an Athanor image, with another
   desktop or other changes. The project documents how
   (`docs/operations/derived-images.md`) and says plainly which guarantees such an image
   loses. A derived image is not supported: a problem is reported against Athanor only when
   it reproduces on an unmodified image.

## Consequences

`docs/operations/derived-images.md` describes the derived image: how to build it, how to
make the update service verify it with the builder's own key (UT3, UT5 of
`doc_update_trust.md`), and what it loses. The applications stay free: Flatpak and Nix
(A2-16) install software without touching the image.
