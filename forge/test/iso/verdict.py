#!/usr/bin/env python3
"""Turns what the run recorded into a pass or a fail, and into something readable.

Usage: verdict.py OUTPUT_DIR GREETER_WAIT QEMU_STATUS [secure-boot]

Reads phases.txt, written by console.py, and the console log beside it. Prints a report
on stdout, writes the same report to OUTPUT_DIR/verdict.md for the job summary, and
exits non-zero when the run did not reach a desktop session.

Seven things have to be true for a pass, and they are checked separately so a failure
says which part broke:

  installed        the ISO's own kickstart finished, so ostreecontainer wrote the image
  kickstart-done   our additions ran too, so the disk has an account on it
  profile          athanor-profile-check reports the installed system's kernel profile holds
                   (fails on drift, and fails if the profile itself could not be read)
  karg             the compression argument the installer's %post sets on a btrfs root is
                   on the command line the system booted with -- or the root is not btrfs,
                   where the installer sets nothing and there is nothing to find
  greeter          the installed system reports a greeter session alive and steady
  session          after the password is typed at that greeter, the account's own
                   desktop session is alive and steady, panel and wallpaper included
  settings         the settings application, started inside that session, is still
                   running twenty seconds later

With `secure-boot` (the Secure Boot case, run_iso_test.sh with SECURE_BOOT_FIRMWARE) an
eighth: the installed system reports Secure Boot enabled, so shim accepted the signed
Azoth kernel against the Athanor certificate in MokList. Without it the state is reported
and decides nothing.

The greeter is recognised from the console rather than from a screenshot, because a
screenshot cannot tell a drawn greeter from a wallpaper. The screenshots are still kept
and published: an automated check says the session started, a person looking at the
picture says it started *correctly*, and the second question is not one this script can
answer.
"""

import pathlib
import sys

# A serial getty is not a greeter, and neither is greetd starting. The getty comes up on
# the way to graphical.target and says nothing about the session (run 34259567237), and
# "Started greetd.service" was on the console of run 34384585109 while the greeter it had
# started was aborting three seconds in, three times, until the start limit. Both stay
# recorded because they place a failure; neither settles a pass. What does is the guest's
# own report, GREETER_ALIVE in console.py: a greeter-class session still there, with the
# shell inside it, after it has had time to die.
GREETER_SIGNALS = ("greeter-alive",)
# The same, one step on: SESSION_ALIVE is the guest's report of the account's desktop
# session still there, its units all active, after logging in at the greeter.
SESSION_SIGNALS = ("session-alive",)
# And one more: SETTINGS_ALIVE is the guest's report of the settings application, started
# inside that session, still running after it has had time to open or to die.
SETTINGS_SIGNALS = ("settings-alive",)
# And the kernel profile: PROFILE_OK is the guest's report that athanor-profile-check found
# every setting of its role combination's profile in force (doc_kernel_profile.md,
# section 12 item 3). PROFILE_DRIFT fails the run by leaving this unset.
PROFILE_SIGNALS = ("profile-ok",)
# And the btrfs compression argument: KARG_OK is the guest's report that it found
# rootflags=compress=zstd:1 on the command line it booted with, and KARG_NOTBTRFS its
# report that its root is not btrfs, so the installer set nothing and there is nothing to
# find. Both settle the question; KARG_MISSING and KARG_UNREADABLE fail the run by
# leaving this unset. Without this line the gate cannot tell a root mounted with
# compression from one mounted without it -- the desktop comes up either way.
KARG_SIGNALS = ("karg-compress-ok", "karg-compress-not-applicable")
# "reinstall-loop" is not a crash but it is a failure, and a distinctive one: the machine
# booted the installer again instead of the system it had just written, so the run says
# nothing about first boot no matter how long it is left going.
FAILURE_SIGNALS = ("panic", "emergency", "reinstall-loop")
# The guest's answer to SECUREBOOT_PROBE in console.py.
SECUREBOOT_SIGNALS = ("secureboot-enabled", "secureboot-disabled")


def main() -> int:
    out = pathlib.Path(sys.argv[1])
    greeter_wait = int(sys.argv[2])
    qemu_status = int(sys.argv[3])
    if sys.argv[4:] not in ([], ["secure-boot"]):
        print("usage: verdict.py OUTPUT_DIR GREETER_WAIT QEMU_STATUS [secure-boot]", file=sys.stderr)
        return 2
    secure_boot_required = sys.argv[4:] == ["secure-boot"]

    phase_file = out / "phases.txt"
    phases: dict[str, int] = {}
    if phase_file.exists():
        for line in phase_file.read_text().splitlines():
            name, _, epoch = line.partition(" ")
            if name and epoch.isdigit():
                phases.setdefault(name, int(epoch))

    log = out / "serial.log"
    log_bytes = log.stat().st_size if log.exists() else 0
    shots = sorted(out.glob("screen-*.ppm"))

    installed = "installed" in phases
    kickstart_done = "kickstart-done" in phases
    profile = next((s for s in PROFILE_SIGNALS if s in phases), None)
    karg = next((s for s in KARG_SIGNALS if s in phases), None)
    greeter = next((s for s in GREETER_SIGNALS if s in phases), None)
    session = next((s for s in SESSION_SIGNALS if s in phases), None)
    settings = next((s for s in SETTINGS_SIGNALS if s in phases), None)
    secure_boot = next((s for s in SECUREBOOT_SIGNALS if s in phases), None)
    failures = [s for s in FAILURE_SIGNALS if s in phases]

    lines = ["## ISO acceptance test", ""]
    lines.append(f"- installer finished: {'yes' if installed else 'NO'}")
    lines.append(
        f"- unattended kickstart finished: {'yes' if kickstart_done else 'NO'}"
    )
    lines.append(f"- kernel profile holds: {profile if profile else 'NO'}")
    lines.append(f"- btrfs compression karg: {karg if karg else 'NO'}")
    lines.append(f"- greeter reached: {greeter if greeter else 'NO'}")
    lines.append(f"- session started: {session if session else 'NO'}")
    lines.append(f"- settings opened: {settings if settings else 'NO'}")
    lines.append(
        f"- secure boot: {secure_boot if secure_boot else 'NO ANSWER'}"
        f"{' (required: secureboot-enabled)' if secure_boot_required else ''}"
    )
    if failures:
        lines.append(f"- guest failures seen: {', '.join(failures)}")
    lines.append(f"- console log: {log_bytes} bytes")
    lines.append(f"- screenshots: {len(shots)}")
    # A non-zero status from `timeout` is 124, which means the run outlived its budget
    # rather than that QEMU itself broke; both are worth naming.
    if qemu_status == 124:
        lines.append("- qemu: killed by the timeout, the run did not finish on its own")
    elif qemu_status != 0:
        lines.append(f"- qemu: exited {qemu_status}")

    if installed and phases.get("installed") and greeter:
        elapsed = phases[greeter] - phases["installed"]
        lines.append(f"- first boot to greeter: {elapsed}s (budget {greeter_wait}s)")
    if greeter and session:
        lines.append(f"- greeter to session: {phases[session] - phases[greeter]}s")
    if session and settings:
        lines.append(f"- session to settings: {phases[settings] - phases[session]}s")

    ok = (
        installed
        and kickstart_done
        and profile is not None
        and karg is not None
        and greeter is not None
        and session is not None
        and settings is not None
        and not failures
        and (not secure_boot_required or secure_boot == "secureboot-enabled")
    )
    lines.insert(1, f"**{'PASS' if ok else 'FAIL'}**")
    report = "\n".join(lines) + "\n"

    (out / "verdict.md").write_text(report)
    print(report)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
