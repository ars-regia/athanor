#!/usr/bin/python3
"""input_method_check.py - doc_own_stack.md, milestone T1 task 6 (OS9 item 3): athanor-toolkit's
text field takes composed input from an input method. Run as RIG_HOLD beside
athanor-toolkit-field. Starts the test input method; captures the field with the preedit, then
with the commit; and reads the field's text on the accessibility bus at each step: empty under
the preedit, which is not text yet, and the committed text after.

Under cosmic-comp every client of the scene binds zwp_input_method_v2, and the input method and
grim run directly. Under RIG_COMPOSITOR=athanor the protocol and the capture are privileged
(doc_own_stack.md OS4): the input method runs as rig-input-method.service and grim as
rig-grim.service, through in-unit.sh, as rig-privileged-clients grants them. The capture reads
RIG_CAPTURE_OUTPUT, which scene.sh sets to athanor-compositor's first headless output.
"""

import os
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, value_of

PREEDIT, COMMIT = "ni", "你"
METHOD = "/out/bin/athanor-toolkit-input-method"
IN_UNIT = "/repo/forge/test/shell/in-unit.sh"
TIMEOUT = 10
# Between two captures of a frame that may still be on its way.
CAPTURE_INTERVAL_S = 0.1


def entry(Atspi):
    app = find_application(Atspi, "athanor-toolkit-field")
    stack = [app] if app is not None else []
    while stack:
        node = stack.pop()
        if node.get_editable_text_iface() is not None:
            return node
        stack.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    return None


def wait_for_text(Atspi, node, expected):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if value_of(node, Atspi) == expected:
            return True
        time.sleep(0.25)
    return False


def under_athanor():
    return os.environ.get("RIG_COMPOSITOR") == "athanor"


def capture_command(name):
    grim = [IN_UNIT, "rig-grim.service", "/usr/bin/grim"] if under_athanor() else ["grim"]
    return grim + ["-o", os.environ.get("RIG_CAPTURE_OUTPUT", "WINIT-0"), f"/out/{name}.png"]


def method_command():
    unit = [IN_UNIT, "rig-input-method.service"] if under_athanor() else []
    return unit + [METHOD, PREEDIT, COMMIT]


def capture(name):
    subprocess.run(capture_command(name), check=True)
    return Path(f"/out/{name}.png").read_bytes()


def capture_after(name, before):
    """Captures the output as /out/<name>.png once it differs from `before`, the PNG of an
    earlier capture, and two captures in a row agree: the toolkit updates its accessibility tree
    at once and draws at the compositor's next frame callback, so the tree can be ahead of the
    pixels. Returns the PNG, or None when the output does not settle on a change in TIMEOUT."""
    deadline = time.monotonic() + TIMEOUT
    last = None
    while time.monotonic() < deadline:
        shot = capture(name)
        if shot != before and shot == last:
            return shot
        last = shot
        time.sleep(CAPTURE_INTERVAL_S)
    return None


def reader(method):
    """The input method's lines, read on a thread so that a wait for one can time out; None
    when its output ends."""
    lines = queue.Queue()

    def read():
        for line in method.stdout:
            lines.put(line.strip())
        lines.put(None)

    threading.Thread(target=read, daemon=True).start()
    return lines


def step(method, lines, expected):
    """Waits up to TIMEOUT for the input method's next line; kills it unless that is expected."""
    try:
        line = lines.get(timeout=TIMEOUT)
    except queue.Empty:
        line = f"nothing in {TIMEOUT} s"
    else:
        line = "nothing more" if line is None else repr(line)
    if line != repr(expected):
        method.kill()
        print(
            f"FAIL the input method said {line}, expected {expected!r}", file=sys.stderr
        )
        return False
    return True


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    node = entry(Atspi)
    if node is None:
        print(
            "FAIL no editable text field of athanor-toolkit-field on the accessibility bus",
            file=sys.stderr,
        )
        return 1
    before = capture("input-method-before")
    method = subprocess.Popen(
        method_command(),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    lines = reader(method)
    if not step(method, lines, f"preedit {PREEDIT}"):
        return 1
    # The preedit is drawn but not committed: the field's text stays empty.
    preedit = capture_after("input-method-preedit", before)
    preedit_text = value_of(node, Atspi)
    method.stdin.write("commit\n")
    method.stdin.flush()
    if not step(method, lines, f"commit {COMMIT}"):
        return 1
    committed = wait_for_text(Atspi, node, COMMIT)
    failures = []
    if preedit is None:
        failures.append(f"the preedit changed no pixel of the output in {TIMEOUT} s")
    elif capture_after("input-method-commit", preedit) is None:
        failures.append(f"the commit changed no pixel of the output in {TIMEOUT} s")
    if preedit_text != "":
        failures.append(
            f"under the preedit the field's text is {preedit_text!r}, expected ''"
        )
    if not committed:
        failures.append(
            f"the field's text is {value_of(node, Atspi)!r}, expected {COMMIT!r}"
        )
    if method.wait(TIMEOUT) != 0:
        failures.append(f"the input method exited with {method.returncode}")
    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    print(
        f"preedit {PREEDIT!r}: text {preedit_text!r}; commit {COMMIT!r}: text {value_of(node, Atspi)!r}"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
