#!/usr/bin/env python3
"""Checks that the acceptance test reports what actually happened.

Run it directly: python3 forge/test/iso/test_verdict.py

Everything here guards against a quiet wrong answer. A virtual machine that will not boot
is loud and needs no test; what does not announce itself is a verdict that calls a broken
run green, a console that types the wrong thing at GRUB and leaves the installer waiting
for a person, a check that mistakes the installed system's own GRUB for a reinstall and
stops the run just as it was about to succeed, or a console that has its answer and keeps
waiting anyway, which is what turned a twelve-minute run into ninety.

So: the cases that decide a pass or a fail, the boot commands console.py sends, the
markers it recognises across chunk boundaries, when it stops, and the PNG conversion,
checked on the pixels rather than on the file existing.
"""

import os
import pathlib
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

HERE = pathlib.Path(__file__).resolve().parent


def verdict(tmp: pathlib.Path, phases, qemu_status=0, *extra):
    out = tmp / "run"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    (out / "phases.txt").write_text("".join(f"{n} {t}\n" for n, t in phases))
    (out / "serial.log").write_bytes(b"console output\n")
    proc = subprocess.run(
        [sys.executable, str(HERE / "verdict.py"), str(out), "900", str(qemu_status), *extra],
        capture_output=True,
        text=True,
    )
    return proc.returncode, (out / "verdict.md").read_text()


def test_pass(tmp: pathlib.Path) -> None:
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("profile-ok", 390),
            ("karg-compress-ok", 395),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-alive", 500),
        ],
    )
    assert code == 0, f"a complete run must pass, got {code}"
    assert "**PASS**" in report, report
    assert "kernel profile holds: profile-ok" in report, report
    assert "btrfs compression karg: karg-compress-ok" in report, report
    assert "first boot to greeter: 300s" in report, report
    assert "greeter to session: 60s" in report, report
    assert "session to settings: 40s" in report, report


COMPLETE_RUN = [
    ("installed", 100),
    ("kickstart-done", 110),
    ("profile-ok", 390),
    ("karg-compress-ok", 395),
    ("greeter-alive", 400),
    ("session-alive", 460),
    ("settings-alive", 500),
]


def test_secure_boot_case_requires_secure_boot_enabled(tmp: pathlib.Path) -> None:
    """The Secure Boot case passes only on the guest's own report that it booted with
    Secure Boot on; the default case reports the state and is not decided by it."""
    code, report = verdict(tmp, COMPLETE_RUN + [("secureboot-enabled", 396)], 0, "secure-boot")
    assert code == 0, report
    assert "secure boot: secureboot-enabled (required: secureboot-enabled)" in report, report
    for answer in ([("secureboot-disabled", 396)], [("secureboot-not-enforced", 396)], []):
        code, report = verdict(tmp, COMPLETE_RUN + answer, 0, "secure-boot")
        assert code == 1, f"{answer}: a Secure Boot case without Secure Boot must fail"
        assert "**FAIL**" in report, report
    code, report = verdict(tmp, COMPLETE_RUN + [("secureboot-disabled", 396)])
    assert code == 0, report
    assert "secure boot: secureboot-disabled" in report, report


def test_secureboot_probe_answers_from_the_guest(tmp: pathlib.Path) -> None:
    """SECUREBOOT_PROBE as the guest's shell runs it, against a mokutil that answers
    either way: the marker comes from printf, never from the echo of the typed line."""
    sys.path.insert(0, str(HERE))
    import console

    work = tmp / "sbprobe"
    shutil.rmtree(work, ignore_errors=True)
    (work / "bin").mkdir(parents=True)
    (work / "bin" / "mokutil").write_text('#!/bin/sh\nprintf "%b\\n" "$FAKE_SB"\n')
    (work / "bin" / "athanor-secureboot-enroll").write_text("#!/bin/sh\necho status\n")
    for tool in ("mokutil", "athanor-secureboot-enroll"):
        (work / "bin" / tool).chmod(0o755)
    (work / "probe.sh").write_bytes(console.SECUREBOOT_PROBE + b"\n")
    for state, expected in (
        ("SecureBoot enabled", "SECUREBOOT_ENABLED"),
        ("SecureBoot disabled", "SECUREBOOT_DISABLED"),
        # Firmware on, shim validation off: not an enforced Secure Boot.
        ("SecureBoot enabled\\nSecureBoot validation is disabled in shim", "SECUREBOOT_NOT_ENFORCED"),
    ):
        proc = subprocess.run(
            ["sh", str(work / "probe.sh")],
            capture_output=True,
            text=True,
            env={**os.environ, "PATH": f"{work / 'bin'}:{os.environ['PATH']}", "FAKE_SB": state},
        )
        assert proc.stdout.splitlines() == [expected, "status"], proc.stdout


def test_session_without_settings_fails(tmp: pathlib.Path) -> None:
    """A desktop that came up is not the whole gate: Settings has to open inside it."""
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-dead", 500),
        ],
    )
    assert code != 0, "a run whose Settings died passed"
    assert "**FAIL**" in report, report
    assert "settings opened: NO" in report, report


def test_profile_drift_fails(tmp: pathlib.Path) -> None:
    """A desktop that works on a kernel profile that does not hold is not a pass: the
    installed system is not the one the profile describes."""
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("profile-drift", 390),
            ("karg-compress-ok", 395),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-alive", 500),
        ],
    )
    assert code != 0, "a run whose kernel profile drifted passed"
    assert "**FAIL**" in report, report
    assert "kernel profile holds: NO" in report, report


def test_profile_unreadable_fails(tmp: pathlib.Path) -> None:
    """A desktop that works when the kernel profile itself could not be read is not a
    pass either: an unreadable profile proves nothing about the installed system."""
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("profile-unreadable", 390),
            ("karg-compress-ok", 395),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-alive", 500),
        ],
    )
    assert code != 0, "a run whose kernel profile could not be read passed"
    assert "**FAIL**" in report, report
    assert "kernel profile holds: NO" in report, report


def test_karg_missing_fails(tmp: pathlib.Path) -> None:
    """A btrfs root that booted without rootflags=compress=zstd:1 is a failed install,
    however well the desktop came up.

    The argument is set by the installer's %post and by nothing else, so its absence
    means that %post did not do its job -- which is exactly what happened in run
    35280318314, where the call aborted the installation outright. A quieter breakage
    that merely skipped the argument would leave a machine mounting its root
    uncompressed for good, and every other check in this gate would still be green.
    """
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("profile-ok", 390),
            ("karg-compress-missing", 395),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-alive", 500),
        ],
    )
    assert code != 0, "a btrfs root without the compression karg passed"
    assert "**FAIL**" in report, report
    assert "btrfs compression karg: NO" in report, report


def test_karg_unreadable_fails(tmp: pathlib.Path) -> None:
    """A guest that could not say what its root filesystem is has not answered the
    question, and an unanswered question is not a pass. /sysroot is the physical root on
    every system this ISO installs; one where it cannot be stat'ed is not that system."""
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("profile-ok", 390),
            ("karg-compress-unreadable", 395),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-alive", 500),
        ],
    )
    assert code != 0, "a run that could not read its root filesystem passed"
    assert "**FAIL**" in report, report
    assert "btrfs compression karg: NO" in report, report


def test_karg_not_applicable_passes(tmp: pathlib.Path) -> None:
    """On a root that is not btrfs the installer sets nothing, on purpose: ext4 and xfs
    refuse the option and would not mount. The run must pass and say so rather than
    report a missing argument that was never meant to be there.

    The acceptance VM installs on btrfs today (collaudo.ks autopart --type=btrfs), so
    this case is the honesty of the check rather than a case it meets: the guest decides
    which answer applies, from its own /sysroot, and the gate cannot be made green by a
    filesystem the test assumed.
    """
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("profile-ok", 390),
            ("karg-compress-not-applicable", 395),
            ("greeter-alive", 400),
            ("session-alive", 460),
            ("settings-alive", 500),
        ],
    )
    assert code == 0, f"a non-btrfs root must pass without the karg, got {code}"
    assert "**PASS**" in report, report
    assert "btrfs compression karg: karg-compress-not-applicable" in report, report


def test_greeter_without_session_fails(tmp: pathlib.Path) -> None:
    """A greeter that takes the password and produces no desktop is the failure the
    login step exists to catch; it is not a pass because the greeter was there."""
    code, report = verdict(
        tmp, [("installed", 100), ("kickstart-done", 110), ("greeter-alive", 400)]
    )
    assert code != 0, "a run that never reached a session passed"
    assert "**FAIL**" in report, report
    assert "greeter reached: greeter-alive" in report, report
    assert "session started: NO" in report, report


def test_greetd_starting_is_not_a_greeter(tmp: pathlib.Path) -> None:
    """The unit starting and the target being reached are not a greeter on screen.

    Run 34384585109 had both on the console while the greeter it started was aborting
    three seconds in, three times, until the start limit. Only the guest's own report
    of a session still alive settles it.
    """
    code, report = verdict(
        tmp,
        [
            ("installed", 100),
            ("kickstart-done", 110),
            ("login-prompt", 300),
            ("greeter-unit", 310),
            ("graphical-target", 311),
        ],
    )
    assert code == 1, "greetd starting is not the greeter being up"
    assert "greeter reached: NO" in report, report


def test_installed_but_no_greeter(tmp: pathlib.Path) -> None:
    # The half that most needs to be told apart: the disk was written, nothing started.
    code, report = verdict(
        tmp, [("installed", 100), ("kickstart-done", 110)], qemu_status=124
    )
    assert code == 1, "a run that never reached a session must fail"
    assert "installer finished: yes" in report, report
    assert "greeter reached: NO" in report, report
    assert "killed by the timeout" in report, report


def test_text_login_is_not_a_greeter(tmp: pathlib.Path) -> None:
    """The gate is the greeter, not a serial getty.

    Run 34259567237 installed correctly, booted, and reached `athanor login:` with no
    trace of greetd anywhere in its console. The verdict called that a pass. A text login
    prompt appears on the way to graphical.target, so it proves the system booted and
    nothing at all about the session.
    """
    code, report = verdict(
        tmp,
        [("installed", 100), ("kickstart-done", 110), ("login-prompt", 400)],
        qemu_status=124,
    )
    assert code == 1, "a boot that only reached a text login must not pass"
    assert "greeter reached: NO" in report, report


def test_panic_fails_despite_markers(tmp: pathlib.Path) -> None:
    # A login prompt after a panic is not a pass: the guest already broke.
    code, report = verdict(
        tmp,
        [("installed", 1), ("kickstart-done", 2), ("login-prompt", 3), ("panic", 4)],
    )
    assert code == 1, "a panic must fail the run"
    assert "guest failures seen: panic" in report, report


def test_reinstall_loop_fails(tmp: pathlib.Path) -> None:
    # The machine booted the ISO again instead of the disk it had just written. Every
    # install marker is present, several times over, and the run still proves nothing
    # about first boot, so it must fail and say which way.
    code, report = verdict(
        tmp,
        [("installed", 100), ("kickstart-done", 110), ("reinstall-loop", 700)],
        qemu_status=124,
    )
    assert code == 1, "a guest that reinstalls in a loop must fail"
    assert "guest failures seen: reinstall-loop" in report, report


def test_nothing_recorded(tmp: pathlib.Path) -> None:
    # No phases file at all: a verdict, not a traceback.
    out = tmp / "empty"
    out.mkdir(parents=True, exist_ok=True)
    proc = subprocess.run(
        [sys.executable, str(HERE / "verdict.py"), str(out), "900", "1"],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 1, proc.stdout + proc.stderr
    assert "**FAIL**" in proc.stdout, proc.stdout


def test_screenshot_keeps_the_pixels(tmp: pathlib.Path) -> None:
    shots = tmp / "shots"
    shots.mkdir(parents=True, exist_ok=True)
    width, height = 4, 3
    pixels = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255] * height)
    ppm = shots / "screen-001.ppm"
    ppm.write_bytes(b"P6\n%d %d\n255\n" % (width, height) + pixels)

    subprocess.run(
        [sys.executable, str(HERE / "screenshots.py"), str(shots)], check=True
    )
    png = (shots / "screen-001.png").read_bytes()
    assert png[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    assert not ppm.exists(), "the PPM should be replaced, not kept"

    idat = png.index(b"IDAT") + 4
    length = struct.unpack(">I", png[idat - 8 : idat - 4])[0]
    raw = zlib.decompress(png[idat : idat + length])
    stride = width * 3
    # Drop the per-scanline filter byte and compare with what went in.
    back = b"".join(
        raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)] for y in range(height)
    )
    assert back == pixels, "the image changed on the way through"


def test_console_boots_our_kickstart(tmp: pathlib.Path) -> None:
    """console.py must name our kickstart at GRUB, in the order GRUB accepts."""
    if not hasattr(socket, "AF_UNIX"):
        # QEMU serves its console on a unix socket, so this check only means something
        # where the test itself runs. Skipping rather than failing keeps the suite usable
        # from a Windows workstation while it still runs in full on the job's runner.
        print("  skip test_console_boots_our_kickstart: no unix sockets here")
        return

    run = tmp / "console"
    shutil.rmtree(run, ignore_errors=True)
    run.mkdir(parents=True)
    sock = str(run / "serial.sock")

    server = socket.socket(socket.AF_UNIX)
    server.bind(sock)
    server.listen(1)
    proc = subprocess.Popen(
        [
            sys.executable,
            str(HERE / "console.py"),
            sock,
            str(run / "serial.log"),
            str(run / "phases.txt"),
            str(run / "monitor.sock"),
        ],
        stdout=subprocess.DEVNULL,
    )
    try:
        conn, _ = server.accept()
        conn.settimeout(2)
        conn.sendall(b"GRUB version 2.12\r\n")

        typed = b""
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                typed += conn.recv(65536)
            except socket.timeout:
                pass
            if typed.rstrip().endswith(b"boot"):
                break
        assert b"search --no-floppy" in typed, typed[:200]
        assert b"inst.ks=hd:LABEL=ATHANORKS:/collaudo.ks" in typed, typed[:400]
        assert b"initrd /images/pxeboot/initrd.img" in typed, (
            "the initrd was never named"
        )
        assert typed.rstrip().endswith(b"boot"), typed[-40:]

        # Markers split across reads, which is how a real console delivers them.
        conn.sendall(b"Install fin")
        time.sleep(0.4)
        conn.sendall(b"ished\r\n")
        time.sleep(0.4)
        conn.sendall(b"Athanor kickstart finished\r\n")
        time.sleep(0.4)
        # As systemd prints it, colour escapes around the unit name included.
        conn.sendall(
            b"[\x1b[0;32m  OK  \x1b[0m] Started \x1b[0;1;39mgreetd.service\x1b[0m"
            b" - Greeter daemon.\r\n"
        )
        time.sleep(0.8)
        conn.close()
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
        server.close()

    phases = dict(
        line.split() for line in (run / "phases.txt").read_text().splitlines()
    )
    for expected in ("grub-booted", "installed", "kickstart-done", "greeter-unit"):
        assert expected in phases, f"{expected} not recorded: {sorted(phases)}"


def test_installed_systems_own_grub_is_not_a_loop(tmp: pathlib.Path) -> None:
    """The GRUB of the installed system must not be mistaken for another install."""
    if not hasattr(socket, "AF_UNIX"):
        print(
            "  skip test_installed_systems_own_grub_is_not_a_loop: no unix sockets here"
        )
        return

    run = tmp / "notloop"
    shutil.rmtree(run, ignore_errors=True)
    run.mkdir(parents=True)
    sock = str(run / "serial.sock")

    server = socket.socket(socket.AF_UNIX)
    server.bind(sock)
    server.listen(1)
    proc = subprocess.Popen(
        [
            sys.executable,
            str(HERE / "console.py"),
            sock,
            str(run / "serial.log"),
            str(run / "phases.txt"),
            str(run / "monitor.sock"),
        ],
        stdout=subprocess.DEVNULL,
    )
    try:
        conn, _ = server.accept()
        conn.settimeout(2)
        conn.sendall(b"GRUB version 2.12\r\n")
        # Let it type the boot commands, then play back the shape of a good run: the
        # install finishes, the machine restarts, and the system it wrote shows its own
        # GRUB before starting. None of that is a reinstall.
        time.sleep(14)
        conn.sendall(b"Install finished\r\n")
        time.sleep(0.5)
        conn.sendall(b"Athanor kickstart finished\r\n")
        time.sleep(0.5)
        conn.sendall(
            b'BdsDxe: starting Boot0004 "Athanor OS" from HD(1,GPT,...)/shimx64.efi\r\n'
        )
        time.sleep(0.5)
        conn.sendall(b"GRUB version 2.12\r\n")
        time.sleep(0.5)
        conn.sendall(
            b"[\x1b[0;32m  OK  \x1b[0m] Started \x1b[0;1;39mgreetd.service\x1b[0m"
            b" - Greeter daemon.\r\n"
        )
        time.sleep(1.0)
        conn.close()
        proc.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
        server.close()

    phases = dict(
        line.split() for line in (run / "phases.txt").read_text().splitlines()
    )
    assert "reinstall-loop" not in phases, (
        "the installed system's own GRUB was called a reinstall: " + str(sorted(phases))
    )
    assert "greeter-unit" in phases, sorted(phases)


def typed_until(conn: socket.socket, needle: bytes, timeout: float) -> bytes:
    """Everything the console types at the guest until `needle` is in it."""
    typed = b""
    deadline = time.time() + timeout
    while time.time() < deadline and needle not in typed:
        try:
            typed += conn.recv(65536)
        except socket.timeout:
            pass
    assert needle in typed, typed[-300:]
    return typed


def play_the_monitor(server: socket.socket, received: list[bytes]) -> None:
    """Answer like QEMU's monitor: a banner on connect, a prompt after every command.
    monitor.py waits on both, and a silent socket would cost its full timeouts."""
    try:
        while True:
            conn, _ = server.accept()
            with conn:
                conn.settimeout(5)
                conn.sendall(b"QEMU 9.0.0 monitor - type 'help' for more information\r\n(qemu) ")
                while True:
                    try:
                        data = conn.recv(65536)
                    except socket.timeout:
                        break
                    if not data:
                        break
                    received.append(data)
                    conn.sendall(b"\r\n(qemu) ")
    except OSError:
        return  # the server socket was closed: the test is over


def test_console_logs_in_opens_settings_and_stops(tmp: pathlib.Path) -> None:
    """A steady greeter gets the password typed on the machine's keyboard, a steady
    session gets Settings started inside it, and Settings still running ends the run at
    once: the wait is what used to cost eighty minutes."""
    if not hasattr(socket, "AF_UNIX"):
        print("  skip test_console_logs_in_opens_settings_and_stops: no unix sockets")
        return

    run = tmp / "prompt"
    shutil.rmtree(run, ignore_errors=True)
    run.mkdir(parents=True)
    sock = str(run / "serial.sock")

    server = socket.socket(socket.AF_UNIX)
    server.bind(sock)
    server.listen(1)
    monitor = socket.socket(socket.AF_UNIX)
    monitor.bind(str(run / "monitor.sock"))
    monitor.listen(2)
    pressed: list[bytes] = []
    threading.Thread(target=play_the_monitor, args=(monitor, pressed), daemon=True).start()
    proc = subprocess.Popen(
        [
            sys.executable,
            str(HERE / "console.py"),
            sock,
            str(run / "serial.log"),
            str(run / "phases.txt"),
            str(run / "monitor.sock"),
        ],
        stdout=subprocess.DEVNULL,
        # The login path waits on the guest in real time; the fake guest answers at once.
        env={**os.environ, "ATHANOR_CONSOLE_PACE": "0.01"},
    )
    try:
        conn, _ = server.accept()
        conn.settimeout(2)
        conn.sendall(b"GRUB version 2.12\r\n")
        time.sleep(14)  # it types the boot commands
        conn.sendall(b"Install finished\r\nAthanor kickstart finished\r\n")
        time.sleep(0.5)

        # The installed system offers its serial login; the console logs in and asks the
        # guest whether the greeter is up. Play the guest: wait for the question, answer.
        conn.sendall(b"athanor login: ")
        typed = typed_until(conn, b"PROFILE_%s", 20)
        conn.sendall(b"PROFILE_OK\r\n")
        if b"KARG_%s" not in typed:
            typed += typed_until(conn, b"KARG_%s", 20)
        conn.sendall(b"KARG_OK btrfs\r\n")
        if b"SECUREBOOT_%s" not in typed:
            typed += typed_until(conn, b"SECUREBOOT_%s", 20)
        conn.sendall(b"SECUREBOOT_ENABLED\r\n")
        if b"GREETER_%s" not in typed:
            typed_until(conn, b"GREETER_%s", 20)
        conn.sendall(b"GREETER_ALIVE c5\r\n")

        # It types the password on the keyboard, one key at a time, then asks whether
        # the session came up.
        typed_until(conn, b"SESSION_%s", 60)
        conn.sendall(b"SESSION_ALIVE c7\r\n")

        # It starts Settings inside that session and asks whether it stayed up.
        typed_until(conn, b"SETTINGS_%s", 60)
        started = time.time()
        conn.sendall(b"SETTINGS_ALIVE name-after:3s\r\n")
        proc.wait(timeout=40)
        waited = time.time() - started
    finally:
        if proc.poll() is None:
            proc.kill()
        server.close()
        monitor.close()

    keys = b"".join(pressed).decode().split()
    assert keys[: 2 * 9] == [
        w for key in list("collaudo") + ["ret"] for w in ("sendkey", key)
    ], keys
    assert keys.count("screendump") == 2, keys
    # The script shuts the machine down when this process returns, so a console that
    # lingers here is a run that lingers: exactly the eighty idle minutes this replaced.
    assert waited < 10, f"the console waited {waited:.1f}s after the answer was in"
    phases = dict(
        line.split() for line in (run / "phases.txt").read_text().splitlines()
    )
    for expected in (
        "profile-ok",
        "karg-compress-ok",
        "secureboot-enabled",
        "greeter-alive",
        "login-sent",
        "session-alive",
        "settings-asked",
        "settings-alive",
        "settings-shot",
    ):
        assert expected in phases, f"{expected} not recorded: {sorted(phases)}"
    assert "idle-timeout" not in phases, "it should not have waited out the idle window"


def test_karg_probe_answers_from_the_guest(tmp: pathlib.Path) -> None:
    """The question the guest is actually asked, run as a shell command.

    Everything above checks what the verdict does with an answer; this checks that the
    answer is the right one. The probe is run verbatim, with the two things it reads
    replaced: a `stat` on PATH that reports whichever root filesystem the case wants,
    and a file bind-mounted over /proc/cmdline in a private mount namespace. That needs
    unprivileged user namespaces, which is what the acceptance runner has; where they
    are unavailable the case is skipped rather than failed, as the socket cases are.

    The substring case is the point of `grep -qxF` over a token list: a command line
    carrying `ignore=rootflags=compress=zstd:1x` contains the argument and does not
    have it, and a probe that answered OK there would make the gate meaningless.
    """
    if shutil.which("unshare") is None:
        print("  skip test_karg_probe_answers_from_the_guest: no unshare here")
        return

    sys.path.insert(0, str(HERE))
    import console

    work = tmp / "probe"
    shutil.rmtree(work, ignore_errors=True)
    (work / "bin").mkdir(parents=True)
    (work / "bin" / "stat").write_text(
        '#!/bin/sh\n[ -n "$FAKE_FSTYPE" ] || exit 1\nprintf "%s\\n" "$FAKE_FSTYPE"\n'
    )
    (work / "bin" / "stat").chmod(0o755)
    (work / "probe.sh").write_bytes(console.KARG_PROBE + b"\n")

    base = "BOOT_IMAGE=/vmlinuz root=UUID=x rootflags=subvol=root rw"
    cases = (
        ("btrfs", base + " " + console.KARG_COMPRESS.decode(), "KARG_OK btrfs"),
        ("btrfs", base, "KARG_MISSING btrfs"),
        ("btrfs", base + " ignore=" + console.KARG_COMPRESS.decode() + "x",
         "KARG_MISSING btrfs"),
        ("xfs", base, "KARG_NOTBTRFS xfs"),
        ("", base, "KARG_UNREADABLE"),
    )
    for fstype, cmdline, expected in cases:
        (work / "cmdline").write_text(cmdline + "\n")
        proc = subprocess.run(
            [
                "unshare",
                "-rm",
                "sh",
                "-c",
                f"mount --bind {work}/cmdline /proc/cmdline &&"
                f' PATH={work}/bin:$PATH FAKE_FSTYPE="{fstype}" sh {work}/probe.sh',
            ],
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0 and not proc.stdout:
            print(
                "  skip test_karg_probe_answers_from_the_guest:"
                f" no unprivileged namespaces here ({proc.stderr.strip()[:60]})"
            )
            return
        answer = proc.stdout.splitlines()[0]
        assert answer == expected, f"{fstype or 'unreadable'}: {answer!r} != {expected!r}"
        if "MISSING" in expected:
            assert cmdline in proc.stdout, "a missing karg must print the command line"


def main() -> int:
    with tempfile.TemporaryDirectory() as name:
        tmp = pathlib.Path(name)
        for test in (
            test_pass,
            test_greeter_without_session_fails,
            test_session_without_settings_fails,
            test_profile_drift_fails,
            test_profile_unreadable_fails,
            test_karg_missing_fails,
            test_karg_unreadable_fails,
            test_karg_not_applicable_passes,
            test_karg_probe_answers_from_the_guest,
            test_secure_boot_case_requires_secure_boot_enabled,
            test_secureboot_probe_answers_from_the_guest,
            test_greetd_starting_is_not_a_greeter,
            test_installed_but_no_greeter,
            test_text_login_is_not_a_greeter,
            test_panic_fails_despite_markers,
            test_reinstall_loop_fails,
            test_nothing_recorded,
            test_screenshot_keeps_the_pixels,
            test_console_boots_our_kickstart,
            test_installed_systems_own_grub_is_not_a_loop,
            test_console_logs_in_opens_settings_and_stops,
        ):
            test(tmp)
            print(f"  ok  {test.__name__}")
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
