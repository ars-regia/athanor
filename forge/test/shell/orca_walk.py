#!/usr/bin/python3
"""orca_walk.py <transcript> <key>...

doc_own_stack.md, OS8: Orca's speech transcript for a scripted walk through a surface. Run as
RIG_HOLD beside the surface. Starts Orca with a debug file, waits until it has presented the
focused window, presses each <key> through wtype's virtual keyboard, quits Orca through its
D-Bus service (Orca writes its debug file out when it exits), and writes every utterance, one
per line, to <transcript>. The debug file stays beside it as <transcript>.log.

A key is an XKB key name, with modifiers before it joined by "+" ("shift+F10"), and "*N"
after it presses it N times ("Tab*11").
"""

import re
import subprocess
import sys
import time
from pathlib import Path

SERVICE = "org.gnome.Orca.Service"
# Orca logs each utterance quoted, then the voice it speaks with: SPEECH OUTPUT: 'text' {voice}.
SPEECH = re.compile(r"SPEECH OUTPUT: '(.*?)'(?: \{|$)", re.MULTILINE)
STARTUP, PAUSE = 30, 1.5


def owned():
    return (
        subprocess.run(
            ["busctl", "--user", "status", SERVICE], capture_output=True, check=False
        ).returncode
        == 0
    )


def utterances(log):
    return [match.group(1) for match in SPEECH.finditer(log)]


def run(command):
    """Runs a command; returns its error output when it fails, else None."""
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    return (
        None
        if result.returncode == 0
        else result.stderr.strip() or f"exit {result.returncode}"
    )


def presses(keys):
    """wtype's arguments for each press of `keys`, in order."""
    for key in keys:
        key, _, times = key.partition("*")
        *modifiers, name = key.split("+")
        press = [arg for modifier in modifiers for arg in ("-M", modifier)] + ["-k", name]
        press += [arg for modifier in reversed(modifiers) for arg in ("-m", modifier)]
        for _ in range(int(times or 1)):
            yield key, press


def walk(orca, keys):
    """Waits for Orca, walks the surface and quits Orca; returns why it failed, or None."""
    deadline = time.monotonic() + STARTUP
    while not owned():
        if time.monotonic() > deadline or orca.poll() is not None:
            return f"Orca did not take {SERVICE}"
        time.sleep(0.5)
    time.sleep(3 * PAUSE)
    for key, press in presses(keys):
        if error := run(["wtype", *press]):
            return f"wtype did not press {key}: {error}"
        time.sleep(PAUSE)
    quit_orca = [
        "busctl",
        "--user",
        "call",
        SERVICE,
        "/org/gnome/Orca/Service",
        SERVICE,
        "Quit",
    ]
    if error := run(quit_orca):
        return f"Orca did not quit through {SERVICE}: {error}"
    try:
        orca.wait(timeout=STARTUP)
    except subprocess.TimeoutExpired:
        return f"Orca did not exit within {STARTUP} s of Quit"
    return None


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    transcript, keys = Path(argv[0]), argv[1:]
    log = transcript.with_suffix(transcript.suffix + ".log")
    log.unlink(missing_ok=True)
    with open(transcript.with_suffix(transcript.suffix + ".stderr"), "w") as stderr:
        orca = subprocess.Popen(
            ["orca", "--replace", f"--debug-file={log}"], stdout=stderr, stderr=stderr
        )
        # Orca never outlives the walk, whichever way the walk ends.
        try:
            error = walk(orca, keys)
        finally:
            if orca.poll() is None:
                orca.kill()
                orca.wait()
    if error:
        print(f"FAIL {error}", file=sys.stderr)
        return 1
    spoken = utterances(log.read_text(errors="replace"))
    transcript.write_text("".join(f"{line}\n" for line in spoken))
    print(f"Orca spoke {len(spoken)} utterance(s) into {transcript}")
    for line in spoken:
        print(f"  {line}")
    if not spoken:
        print("FAIL Orca spoke nothing", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
