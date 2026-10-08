---
id: ADR-0090
title: "Account lockout through authselect's with-faillock"
date: 2026-10-08
status: accepted
issues: []
areas: [security]
---

# 0090. Account lockout through authselect's with-faillock

## Context

Audit 4 (finding RT-A1) found no account lockout in the image: `pam_faillock` was absent from
the authselect `system-auth` and `password-auth` stacks and `/etc/security/faillock.conf` held
no active setting. Guessing a password at login, sudo or polkit was bounded only by
`pam_faildelay`. Fedora's authselect default enables `with-faillock`.

## Decision

The image enables authselect's `with-faillock` feature and sets, in `/etc/security/faillock.conf`:

- `deny = 5`: five failures lock the account;
- `fail_interval = 900`: failures count within fifteen minutes;
- `unlock_time = 600`: the lock lasts ten minutes.

root is not locked out (`even_deny_root` stays off), so the recovery console remains usable. This
replaces the `deny = 10`, `unlock_time = 300` values and the timing of `doc_lock_and_prompts.md`
D3, which tied the feature to the lock screen's switch.

## Consequences

- `system/Containerfile` runs `authselect enable-feature with-faillock` next to `without-nullok`
  (ADR-0060) and appends the three settings, and the build fails if the module or the settings are
  missing. `scripts/verify.py pam` checks the same lines in the source.
- An upgraded machine receives the change as it received `without-nullok`: authselect writes
  `/etc/authselect/*` and `/etc/security/faillock.conf` into the image's `/usr/etc`, and the
  `/etc` three-way merge replaces every file the administrator did not modify.
- The tally is in `/run/faillock` (tmpfs): a reboot clears it, which keeps a locked-out
  administrator one reboot away from the recovery console.
- Limit: At the cosmic-greeter lock screen failures are counted only once the user's tally file exists: root-side PAM (greetd, sudo, polkit) creates it after an earlier failure and `fchown`s it to the user with mode 0660 (`faillock.c:80, 89-91`), and the user-side locker can then read and write it, so the lockout is enforced there too (`pam_faillock.c:199-217`, `:496-503`). With no file yet, the locker's check succeeds (`:204-205`) and its failed create is ignored (`:303-305`), so a fresh boot's lock screen does not count failures. This holds until `athanor-unlockd` (root-side) replaces the locker. Read from source, not run (doc_lock_and_prompts.md, spike L10).
