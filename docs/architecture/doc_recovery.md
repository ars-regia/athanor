# Athanor recovery: a text console, and the way back

Status: **revision 1, 2026-09-30, awaiting the maintainer's approval.** The maintainer chose option A of section 3 on 2026-09-30, and option B for later. The console it describes is implemented on the branch of this revision; what was and was not run is in section 5.

**Amendment of 2026-10-06** (maintainer decision A2-8 of 2026-10-05, specification audit 2, #150): greenboot is the one automatic fallback, owned by this document, enabled by preset and ordered before R1 (R5). Revision 1's text is otherwise unchanged.

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

**R1. The recovery is a text console.** The same trigger now starts `athanor-recovery.target`, which wants a login on `tty1` (`getty@tty1.service`) and a unit that writes a message. The root account is locked (`rootpw --lock`), so the login is an administrator's own. That login goes through the same PAM stacks as every other, so the account lockout of ADR-0089 applies to it: five wrong passwords within fifteen minutes lock the account for ten minutes, or until the next reboot, which clears the tally in `/run/faillock`.

**R2. The way back is `GoBack()`, from `sudo athanor-update go-back`.** The command is a client of `os.athanor.Update1`. The service asks polkit for `os.athanor.update.rollback`, which is `auth_admin` from every kind of session (SH12), runs `bootc rollback`, holds the digest it left and restarts. Run through `sudo` the caller is root, and polkit authorises a root subject without an agent, so nothing has to authenticate the person a second time on a console that has no agent. Any other user is told to use `sudo`.

**R3. The message is in place before the login prints its banner, and only after a recovery.** It is in English and Italian and names `sudo athanor-update go-back` and `journalctl -b -u greetd`. agetty prints `/etc/issue.d/*.issue` and does not read `/run/issue.d` while `/etc/issue` exists (measured with util-linux 2.39). So `tmpfiles.d` declares `/etc/issue.d/50-athanor-recovery.issue` as a link to `/run/athanor-recovery/recovery.issue`, and `athanor-recovery-notice.service` writes that file. At every boot but a recovery the link points at nothing, and agetty passes over it: the banner is the plain one.

**R4. The kiosk is frozen, not deleted.** Its source stays in `forge/specs/athanor-recovery/athanor-recovery-1.0.0`, out of the workspace (`exclude`, as `athanor-settings-rs` and the others) and out of the image. The package ships no binary and no `athanor-recovery` user; a machine that already has the user keeps it. `athanor-style` still carries a legacy glass theme "kept only because athanor-recovery still loads it"; with the kiosk out of the workspace nothing loads it, and it can go.

**R5. Greenboot is the automatic fallback, owned here, and marks the deployment before R1** (amendment of 2026-10-06, maintainer decision A2-8 of 2026-10-05, #150). Release 1.0 boots bootc on the ostree backend through GRUB (`doc_kernel_profile.md`, D6 as amended on 2026-10-06), and greenboot, with GRUB's boot counter, is its one automatic fallback: a deployment whose required health checks fail is marked bad, and the next boot returns to the previous deployment. Nothing reboots by itself, so that next boot is one the person starts (A2-26, #150; `doc_kernel_profile.md`, D31: a change of the base takes effect at a full reboot the user chooses, never forced). Before this amendment three mechanisms touched the same failure and none owned it: the systemd-boot counting of `doc_kernel_profile.md` section 8, which does not exist on this path; R1; and greenboot itself, shipped in `upstream_core` (`forge/config/packages.json:105-106`) with the required check `/etc/greenboot/check/required.d/10-greetd-running.sh` of `athanor-system-config`, and named by no specification.

- **This document owns greenboot** (#150: "greenboot, owned by `doc_recovery.md`"). The scope of that ownership, its packages, its required checks and its configuration, is proposed here and awaits the maintainer.
- **Enabled by preset** (#150: "enabled by preset"). The image enables greenboot through a preset file it ships, instead of relying on Fedora's presets: `80-athanor-recovery.preset` of `athanor-recovery` enables `greenboot-healthcheck.service` and `greenboot-set-rollback-trigger.service`.
- **Ordered before R1** (#150: "ordered before R1"; A2-26, #150). Greenboot's automatic reboot is off, so greenboot and R1 act in the same failing boot without working against each other: greenboot marks the deployment and reboots nothing, and R1 shows its console when the greeter does not start. The order is that the mark comes before R1's message: `athanor-recovery-notice.service` is ordered after greenboot's health check, so the message of R3 can say whether the booted deployment is marked.
- **The notice, in a session** (A2-26, #150). The update service records the mark in its state file (`doc_update_trust.md`, UT7) and holds the digest of the marked deployment (UT6), whatever its class. The notifier `athanor-update-notify` (UT11) shows each user of that boot the notice: this version failed its health check, and the next restart returns to the previous version. The notice has no action that reboots by itself; the person restarts from the session's power menu when ready.
- **Without a session** (A2-26, #150). When the greeter does not start there is no session and no notifier: the person sees R1's console on `tty1`. R3's message then also says, in English and Italian, that this version failed its health check and that the next restart returns to the previous version, and names `sudo systemctl reboot`; `sudo athanor-update go-back` (R2) stays for a deployment that is not marked. The console reboots nothing by itself, as D31 of `doc_kernel_profile.md` requires: the person chooses when the machine restarts.
- A return made by greenboot does not go through `GoBack()`, so nothing holds the digest it left (`doc_update_trust.md`, UT6); the update service learns of it from the mark on the bad deployment and holds the digest (A2-26, #150, below).

Questions on R5 raised by the audit of this amendment (2026-10-06); the second and third are decided by maintainer decision A2-26 (#150), which also settles the first, except for the mark it now relies on:

- **Nothing yet makes the order hold.** R1 fires during the failing boot, on greetd's third failure within a minute (`StartLimitBurst=3`, `StartLimitIntervalSec=60s`); greenboot spends a try of GRUB's counter only through a reboot after its required checks fail. On a deployment whose greeter does not start, R1's console can therefore appear before greenboot has acted. Which mechanism makes "ordered before R1" true is not specified. *Settled by A2-26 (#150):* greenboot no longer reboots, so nothing has to win a race: R1's console appears in the failing boot, and its message reads greenboot's mark (above). **The mark** (2026-10-07). The image carries greenboot rebuilt with a `GREENBOOT_AUTO_REBOOT` setting (`forge/specs/greenboot-rs`), which `athanor-system-config` sets to false in `greenboot.conf`. When a required check fails on a new deployment, greenboot runs its red scripts and makes the previous deployment the default at once (`bootc rollback`), and reboots nothing; it logs "Rollback successful", the line R3's message reads. The next boot, whenever the person starts it, is the previous deployment, and greenboot on it recognises the return from the previous boot's journal without rebooting. GRUB's boot counter is not used: Fedora's `grub-boot-success.timer` sets `boot_success=1` after two minutes of any user session, which would cancel the counter's fallback before the person restarts. The update service reads the mark from `bootc status` (`rollbackQueued`): while it is set, every check holds the booted digest and downloads nothing, and `GoBack()` does not roll back a second time (`doc_update_trust.md`, UT6, 2026-10-07).
- **Greenboot reboots by itself.** As shipped, greenboot reboots the machine when its required checks fail, and returns to the previous deployment when the tries are spent, without the user. That conflicts with the rule that nothing reboots by itself (`doc_shell.md` SH11; `doc_update_trust.md` UT13). **Decided (A2-26, #150): greenboot's automatic reboot is turned off.** A failed health check marks the deployment bad and nothing reboots; the session shows a notice that the next boot returns to the previous version, and the user restarts when ready. D31 stands.
- **A rolled-back digest is downloaded again.** Since nothing holds the digest greenboot left, the next update check downloads it again, and a security-class digest would apply again at the next shutdown (UT13). **Decided (A2-26, #150): the update service holds the bad digest,** as `GoBack()` does (`doc_update_trust.md`, UT6).

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

Not run for R5 (added on 2026-10-06): nothing. Greenboot's activation on the shipped image, its order against R1, and a return to the previous deployment are all unverified.

## 6. Acceptance, on the dev VM

1. Make the greeter fail three times in a minute. `tty1` shows a login and, above it, the message in both languages.
2. As an administrator, `sudo athanor-update go-back` restarts on the previous version, and the next three update checks do not download the digest that was left.
3. With no previous deployment it answers "there is no previous version to go back to" and changes nothing.
4. `athanor-update go-back` as an ordinary user, without `sudo`, is refused and says to use `sudo`.
5. A normal boot shows the plain banner and no message, and `/run/athanor-recovery` does not exist.
6. Added on 2026-10-06 (R5): greenboot is enabled by the image's own preset file.
7. Added on 2026-10-06 (R5), rewritten for A2-26 (#150): stage and boot a new deployment whose greeter does not start (a test image whose `greetd.service` runs `/usr/bin/false`). In that boot, greenboot's required check `10-greetd-running.sh` fails and the deployment is marked; `tty1` shows the recovery console, whose message says in both languages that this version failed its health check and that the next restart returns to the previous version; ten minutes later the machine has not rebooted (`bootc status` still shows the marked deployment booted, and `journalctl --list-boots` shows no new boot). After `sudo systemctl reboot` from the console, `bootc status` shows the previous deployment booted, and the next three update checks do not download the marked digest.
8. Added on 2026-10-06 (R5, A2-26): with a deployment whose required health check fails, nothing reboots by itself and the deployment is marked bad.
9. Added on 2026-10-06 (R5, A2-26): the session of that boot shows the notice that the next boot returns to the previous version; after the user restarts, the previous version runs.
10. Added on 2026-10-06 (R5, A2-26): the next three update checks do not download the digest of the bad deployment.
