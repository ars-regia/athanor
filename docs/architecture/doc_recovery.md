# Athanor recovery: a text console, and the way back

Status: **revision 1, 2026-09-30, awaiting the maintainer's approval.** The maintainer chose option A of section 3 on 2026-09-30, and option B for later. The console it describes is implemented on the branch of this revision; what was and was not run is in section 5.

## 1. Context

When the desktop does not start, greetd fails and systemd stops restarting it after three failures in a minute (`StartLimitBurst=3`, `StartLimitIntervalSec=60s`). The package `athanor-recovery` hooks that failure with `OnFailure=athanor-recovery.target`. Until 2026-09-30 the target started a graphical kiosk, `athanor-recovery-ui`, on cosmic-comp, whose button ran `rpm-ostree rollback`.

The kiosk could not do what it offered, and read in the code (not run) it failed in six ways:

- It ran as the user `athanor-recovery` with `NoNewPrivileges=yes`, so the rollback it ran could not do privileged work.
- A rollback outside `athanor-update` is undone by the update timer: only `GoBack()` records the digest it leaves as held (`doc_update_trust.md`, UT6).
- Its fallback wrote a bcachefs snapshot and reported success. A snapshot is not a rollback, and the image does not use bcachefs.
- Its diagnostics were fixed text ("Critical Failure / Integrity Tamper Detected", "Bcachefs"), the same whatever failed.
- It opened a LUKS volume at a device and with a key file written into the code.
- It authenticated nobody: its PAM stack has only `account` and `session`.

## 2. Decisions

**R1. The recovery is a text console.** The same trigger now starts `athanor-recovery.target`, which wants a login on `tty1` (`getty@tty1.service`) and a unit that writes a message. The root account is locked (`rootpw --lock`), so the login is an administrator's own.

**R2. The way back is `GoBack()`, from `sudo athanor-update go-back`.** The command is a client of `os.athanor.Update1`. The service asks polkit for `os.athanor.update.rollback`, which is `auth_admin` from every kind of session (SH12), runs `bootc rollback`, holds the digest it left and restarts. Run through `sudo` the caller is root, and polkit authorises a root subject without an agent, so nothing has to authenticate the person a second time on a console that has no agent. Any other user is told to use `sudo`.

**R3. The message is in place before the login prints its banner, and only after a recovery.** It is in English and Italian and names `sudo athanor-update go-back` and `journalctl -b -u greetd`. agetty prints `/etc/issue.d/*.issue` and does not read `/run/issue.d` while `/etc/issue` exists (measured with util-linux 2.39). So `tmpfiles.d` declares `/etc/issue.d/50-athanor-recovery.issue` as a link to `/run/athanor-recovery/recovery.issue`, and `athanor-recovery-notice.service` writes that file. At every boot but a recovery the link points at nothing, and agetty passes over it: the banner is the plain one.

**R4. The kiosk is frozen, not deleted.** Its source stays in `forge/specs/athanor-recovery/athanor-recovery-1.0.0`, out of the workspace (`exclude`, as `athanor-settings-rs` and the others) and out of the image. The package ships no binary and no `athanor-recovery` user; a machine that already has the user keeps it. `athanor-style` still carries a legacy glass theme "kept only because athanor-recovery still loads it"; with the kiosk out of the workspace nothing loads it, and it can go.

## 3. Options that were weighed

| | Option | Outcome |
|---|---|---|
| A | Remove the kiosk; a text console and `athanor-update go-back` | **Chosen for now.** Small, honest, and it keeps `auth_admin` from every session. |
| B | A graphical kiosk with a polkit agent of its own: it authenticates an administrator with the greeter's PAM code, then calls `GoBack()` | **Later,** with the polkit agent of stage 4 (`doc_shell.md`, SH1). The agent is the trusted path and needs hardware to test; the kiosk's `NoNewPrivileges` also stops polkit's setuid helper. |
| C | A rule that lets the kiosk call `GoBack()` without a password | **Not taken.** It contradicts "administrator authentication from every kind of session". A person at the machine could already choose the previous entry in the boot menu (D41 states that residual risk), but the decision is the maintainer's to change, not this document's. |

## 4. Tests

`forge/specs/athanor-recovery/tests/test_units.py` (14 tests) covers the trigger, the target, the notice unit, its exposure (1.6 of 10 by `systemd-analyze security`), the message and the package. `athanor-update` has 6 tests of the client on a private bus, with a fake service answering in the service's own error type.

## 5. What was run and what was not

Run: the tests above; `agetty --show-issue` in a pseudo-terminal with the shipped link and message, and with the link dangling (the banner is unchanged); `systemd-tmpfiles --create` on the shipped file; and, in the source of polkit, the special case that authorises uid 0.

Not run, because it needs a machine: greetd failing three times and the target starting the getty (greetd declares `Conflicts=getty@tty1.service`, and the order of the two stops and starts was reasoned, not seen); `sudo athanor-update go-back` against the real service, `bootc rollback` and the restart; that a member of `wheel` can read `journalctl -b -u greetd`; the appearance of the message on a real console.

## 6. Acceptance, on the dev VM

1. Make the greeter fail three times in a minute. `tty1` shows a login and, above it, the message in both languages.
2. As an administrator, `sudo athanor-update go-back` restarts on the previous version, and the next three update checks do not download the digest that was left.
3. With no previous deployment it answers "there is no previous version to go back to" and changes nothing.
4. `athanor-update go-back` as an ordinary user, without `sudo`, is refused and says to use `sudo`.
5. A normal boot shows the plain banner and no message, and `/run/athanor-recovery` does not exist.
