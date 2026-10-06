---
id: W2-SETTINGS
title: "doc_settings decisions (wave 2)"
date: 2026-10-05
status: accepted
issues: []
areas: [shell, settings, security]
---

# 0010. doc_settings decisions (wave 2)

## Context

Recorded in the maintainer decision log, section 'Wave 2 decisions (maintainer, 2026-10-05) — all as recommended'.

## Decision

- 1 resident from first Show until logout
- 2 main Wayland socket in release 1
- 3 change system disk passphrase now via udisks ChangePassphrase (auth_admin_keep), keyslots wait for P4b
- 4 athanor-passwd@ confined root service with PAM chauthtok (auth module: implementation plan needs maintainer approval before code), interim text points to passwd
- 5 admins create/delete accounts via AccountsService, yescrypt, no parental controls
- 6 Desktop page absorbs athanor-layout-chooser
- 7 firmware page reads fwupd, applies via os.athanor.Lvfs
- 8 SE24 exclusions as written, printers excluded

## Consequences

Elaborated in `docs/architecture/doc_settings.md` (on shell-specs).
