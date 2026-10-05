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

import json
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
sys.path.insert(0, str(HERE))
from evidence import CHECKS as EVIDENCE_CHECKS  # noqa: E402


def verdict(tmp: pathlib.Path, phases, qemu_status=0):
    out = tmp / "run"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    (out / "phases.txt").write_text("".join(f"{n} {t}\n" for n, t in phases))
    (out / "serial.log").write_bytes(b"console output\n")
    proc = subprocess.run(
        [sys.executable, str(HERE / "verdict.py"), str(out), "900", str(qemu_status)],
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


LABELS = {
    "io.athanor.run-id": "412",
    "org.opencontainers.image.revision": "c" * 40,
    "io.athanor.image-digest.athanor-system": "sha256:" + "a" * 64,
    "io.athanor.image-digest.athanor-system-nvidia": "sha256:" + "b" * 64,
    "io.athanor.image-digest.athanor-system-nvidia-legacy": "sha256:" + "d" * 64,
}
ACTIONS = {
    "GITHUB_REPOSITORY": "hr-mes/athanor",
    "GITHUB_RUN_ID": "900",
    "GITHUB_RUN_ATTEMPT": "1",
    "GITHUB_REF": "refs/heads/iso-v0",
    "GITHUB_EVENT_NAME": "schedule",
    "GITHUB_SHA": "e" * 40,
    "GITHUB_WORKFLOW_REF": "hr-mes/athanor/.github/workflows/athanor-forge-orchestrator.yml@refs/heads/iso-v0",
}


def evidence(tmp: pathlib.Path, labels, verdict_dir, actions=ACTIONS):
    labels_file = tmp / "labels.json"
    labels_file.write_text(json.dumps(labels))
    out = tmp / "evidence"
    shutil.rmtree(out, ignore_errors=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith("GITHUB_")}
    proc = subprocess.run(
        [sys.executable, str(HERE / "evidence.py"), str(verdict_dir), str(labels_file), str(out)],
        capture_output=True,
        text=True,
        env={**env, **actions},
    )
    path = out / "acceptance-412.json"
    return proc.returncode, json.loads(path.read_text()) if path.exists() else None


def rewrite_verdict(tmp: pathlib.Path, change) -> pathlib.Path:
    """A passing verdict.json, edited: what a check that broke or a partial run would leave."""
    test_pass(tmp)
    path = tmp / "run" / "verdict.json"
    doc = json.loads(path.read_text())
    change(doc)
    path.write_text(json.dumps(doc))
    return tmp / "run"


def test_evidence_binds_the_digests_the_commit_and_the_run(tmp: pathlib.Path) -> None:
    test_pass(tmp)
    code, doc = evidence(tmp, LABELS, tmp / "run")
    assert code == 0, "a labelled ISO must yield evidence"
    assert (doc["schema"], doc["kind"], doc["result"], doc["run_id"]) == (1, "athanor-acceptance", "pass", "412")
    assert doc["revision"] == "c" * 40 and doc["tested_image"] == "athanor-system"
    assert doc["images"] == {
        "athanor-system": "sha256:" + "a" * 64,
        "athanor-system-nvidia": "sha256:" + "b" * 64,
        "athanor-system-nvidia-legacy": "sha256:" + "d" * 64,
    }
    assert set(doc["tests"]) == set(EVIDENCE_CHECKS) and all(doc["tests"].values())
    assert doc["acceptance"] == {
        "repository": "hr-mes/athanor", "run_id": "900", "run_attempt": "1", "ref": "refs/heads/iso-v0",
        "event": "schedule", "sha": "e" * 40,
        "workflow_ref": "hr-mes/athanor/.github/workflows/athanor-forge-orchestrator.yml@refs/heads/iso-v0",
    }


def test_evidence_checks_are_the_verdict_checks(tmp: pathlib.Path) -> None:
    test_pass(tmp)
    written = json.loads((tmp / "run" / "verdict.json").read_text())["checks"]
    assert tuple(written) == EVIDENCE_CHECKS, "evidence.py must require exactly the checks verdict.py writes"


def test_evidence_of_a_failed_run_says_fail(tmp: pathlib.Path) -> None:
    verdict(tmp, [("installed", 100), ("greeter-alive", 400)])
    code, doc = evidence(tmp, LABELS, tmp / "run")
    assert code == 0 and doc["result"] == "fail", "a failed verdict must be recorded as a failure"
    assert doc["tests"]["session"] is False


def test_evidence_without_a_verdict_says_fail(tmp: pathlib.Path) -> None:
    empty = tmp / "never-started"
    empty.mkdir(exist_ok=True)
    code, doc = evidence(tmp, LABELS, empty)
    assert code == 0 and doc["result"] == "fail" and doc["tests"] == {}, "a run that left no verdict is a failure"


def test_evidence_of_a_partial_pass_says_fail(tmp: pathlib.Path) -> None:
    # "pass" with a check missing, a check false, a check that is truthy but not true, or an
    # extra check nobody asked for: none of them may attest more than was verified.
    def drop(doc):
        del doc["checks"]["settings"]

    def falsify(doc):
        doc["checks"]["karg"] = False

    def truthy(doc):
        doc["checks"]["greeter"] = "yes"

    def extra(doc):
        doc["checks"]["something-else"] = True

    for change in (drop, falsify, truthy, extra):
        code, doc = evidence(tmp, LABELS, rewrite_verdict(tmp, change))
        assert code == 0 and doc["result"] == "fail", f"{change.__name__}: a partial pass must be a fail"
        assert "something-else" not in doc["tests"], "the evidence attests only the known checks"


def test_evidence_needs_every_label_well_formed(tmp: pathlib.Path) -> None:
    test_pass(tmp)
    cases = {
        "no labels": {"org.opencontainers.image.version": "412"},
        "a tag for a run": {**LABELS, "io.athanor.run-id": "latest"},
        "a short commit": {**LABELS, "org.opencontainers.image.revision": "c" * 12},
        "a tag for a digest": {**LABELS, "io.athanor.image-digest.athanor-system": "latest"},
        "an uppercase digest": {**LABELS, "io.athanor.image-digest.athanor-system": "sha256:" + "A" * 64},
        "a missing variant": {k: v for k, v in LABELS.items() if not k.endswith("nvidia-legacy")},
        "an unknown image": {**LABELS, "io.athanor.image-digest.athanor-other": "sha256:" + "f" * 64},
    }
    for name, labels in cases.items():
        code, doc = evidence(tmp, labels, tmp / "run")
        assert code == 1 and doc is None, f"{name}: evidence must not be written"


def test_evidence_needs_the_actions_context(tmp: pathlib.Path) -> None:
    test_pass(tmp)
    for variable in ACTIONS:
        code, doc = evidence(tmp, LABELS, tmp / "run", {k: v for k, v in ACTIONS.items() if k != variable})
        assert code == 1 and doc is None, f"without {variable} the evidence cannot say where it came from"
    code, doc = evidence(tmp, LABELS, tmp / "run", {**ACTIONS, "GITHUB_SHA": "not-a-commit"})
    assert code == 1 and doc is None, "a malformed commit is not a commit"


def attest(tmp: pathlib.Path, evidence_file: pathlib.Path):
    """attest.sh with a cosign that records its arguments."""
    bin_dir = tmp / "bin"
    bin_dir.mkdir(exist_ok=True)
    log = tmp / "cosign.log"
    log.unlink(missing_ok=True)
    fake = bin_dir / "cosign"
    fake.write_text(f'#!/bin/sh\necho "$*" >> {log}\n')
    fake.chmod(0o755)
    env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}", "REGISTRY": "registry.example/owner", "RETRY_ATTEMPTS": "1"}
    proc = subprocess.run(["bash", str(HERE / "attest.sh"), str(evidence_file)], capture_output=True, text=True, env=env)
    return proc.returncode, log.read_text().splitlines() if log.exists() else []


def test_attest_signs_every_digest_of_passing_evidence_only(tmp: pathlib.Path) -> None:
    test_pass(tmp)
    code, _ = evidence(tmp, LABELS, tmp / "run")
    path = tmp / "evidence" / "acceptance-412.json"
    code, calls = attest(tmp, path)
    assert code == 0, "passing evidence must be attested"
    expected = {f"registry.example/owner/{k.removeprefix('io.athanor.image-digest.')}@{v}"
                for k, v in LABELS.items() if k.startswith("io.athanor.image-digest.")}
    assert {c.split()[-1] for c in calls} == expected, calls
    assert all(c.startswith(f"attest --yes --type custom --predicate {path} ") for c in calls), calls
    for change in ({"result": "fail"}, {"images": {"athanor-system": "latest"}}):
        forged = tmp / "forged.json"
        forged.write_text(json.dumps({**json.loads(path.read_text()), **change}))
        code, calls = attest(tmp, forged)
        assert code == 1 and calls == [], f"{change}: must not be attested"


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
            test_evidence_binds_the_digests_the_commit_and_the_run,
            test_evidence_checks_are_the_verdict_checks,
            test_evidence_of_a_failed_run_says_fail,
            test_evidence_without_a_verdict_says_fail,
            test_evidence_of_a_partial_pass_says_fail,
            test_evidence_needs_every_label_well_formed,
            test_evidence_needs_the_actions_context,
            test_attest_signs_every_digest_of_passing_evidence_only,
        ):
            test(tmp)
            print(f"  ok  {test.__name__}")
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
