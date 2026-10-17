#!/usr/bin/python3
"""orca_walk.py <transcript> <tabs>

doc_own_stack.md, OS8: Orca's speech transcript for a scripted walk through a surface. Run as
RIG_HOLD beside the surface. Starts Orca with a debug file, waits until it has presented the
focused window, presses Tab <tabs> times through wtype's virtual keyboard, quits Orca through
its D-Bus service (Orca writes its debug file out when it exits), and writes every utterance,
one per line, to <transcript>. The debug file stays beside it as <transcript>.log.
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


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    transcript, tabs = Path(argv[0]), int(argv[1])
    log = transcript.with_suffix(transcript.suffix + ".log")
    log.unlink(missing_ok=True)
    with open(transcript.with_suffix(transcript.suffix + ".stderr"), "w") as stderr:
        orca = subprocess.Popen(
            ["orca", "--replace", f"--debug-file={log}"], stdout=stderr, stderr=stderr
        )
        deadline = time.monotonic() + STARTUP
        while not owned():
            if time.monotonic() > deadline or orca.poll() is not None:
                orca.kill()
                print(f"FAIL Orca did not take {SERVICE}", file=sys.stderr)
                return 1
            time.sleep(0.5)
        time.sleep(3 * PAUSE)
        for _ in range(tabs):
            subprocess.run(["wtype", "-k", "Tab"], check=True)
            time.sleep(PAUSE)
        subprocess.run(
            [
                "busctl",
                "--user",
                "call",
                SERVICE,
                "/org/gnome/Orca/Service",
                SERVICE,
                "Quit",
            ],
            check=True,
            capture_output=True,
        )
        orca.wait(timeout=STARTUP)
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
