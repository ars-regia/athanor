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
`pam_faildelay`. The `local` authselect profile offers `with-faillock` as a feature (it is not
enabled by default on the base image).

## Decision

The image enables authselect's `with-faillock` feature and sets, in `/etc/security/faillock.conf`:

- `deny = 5`: five failures lock the account;
- `fail_interval = 900`: failures count within fifteen minutes;
- `unlock_time = 600`: the lock lasts ten minutes.

root is not locked out (`even_deny_root` and `admin_group` stay unset). That exemption does not
protect recovery: root is locked (`rootpw --lock`) and the recovery console is an administrator's
own login. It stays reachable because the tally lives in `/run/faillock` (tmpfs), so booting into
the recovery entry starts with no failures.

Amends ADR-0003 (D3: `deny = 10`, `unlock_time = 300`, enabled only with the lock screen's switch).

Accepted trade-off: anyone at a password prompt (the greeter, or `su` and `sudo -S` run as another
user) can lock the administrator out for ten minutes, repeatedly. This local denial of service is
accepted for 1.0. The lockout also does not bound a process running as the same user: the tally is
user-owned (mode 0660), that process can truncate it, and the account phase resets it on success;
against it only `pam_faildelay` remains.

## Consequences

- `system/Containerfile` runs `authselect enable-feature with-faillock` next to `without-nullok`
  (ADR-0060) and appends the three settings, and the build fails if the module or the settings are
  missing. `scripts/verify.py pam` checks the same lines in the source.
- An upgraded machine receives the change as it received `without-nullok`: authselect writes
  `/etc/authselect/*` and `/etc/pam.d/*` into the image's `/usr/etc`, the `printf` appends to
  `/etc/security/faillock.conf`, and the `/etc` three-way merge replaces, file by file, every file
  the administrator did not modify.
- Limit: the merge is per file. A machine where the administrator ran authselect locally keeps its
  own `/etc/authselect` and `/etc/pam.d` (no `pam_faillock`) while `faillock.conf` updates, so the
  lockout can be silently absent there.
- Follow-up: a runtime `authselect check` plus a check that `with-faillock` is enabled, surfaced in
  the update trust state and the shield. Not done here.
- The tally is in `/run/faillock` (tmpfs): a reboot clears it, which keeps a locked-out
  administrator one reboot away from the recovery console.
- Limit: At the cosmic-greeter lock screen failures are counted only once the user's tally file exists: root-side PAM (greetd, sudo, polkit) creates it after an earlier failure and `fchown`s it to the user with mode 0660 (`faillock.c:80, 89-91`), and the user-side locker can then read and write it, so the lockout is enforced there too (`pam_faillock.c:199-217`, `:496-503`). With no file yet, the locker's check succeeds (`:204-205`) and its failed create is ignored (`:303-305`), so a fresh boot's lock screen does not count failures. This holds until `athanor-unlockd` (root-side) replaces the locker. Read from source, not run (doc_lock_and_prompts.md, spike L10).
