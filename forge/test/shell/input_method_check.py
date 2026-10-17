#!/usr/bin/python3
"""input_method_check.py - doc_own_stack.md, milestone T1 task 6 (OS9 item 3): athanor-toolkit's
text field takes composed input from an input method. Run as RIG_HOLD beside
athanor-toolkit-field under cosmic-comp. Starts the test input method, which binds
zwp_input_method_v2 unconfined, as every client of the scene; captures the field with the
preedit, then with the commit; and reads the field's text on the accessibility bus at each step:
empty under the preedit, which is not text yet, and the committed text after.
"""

import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, value_of

PREEDIT, COMMIT = "ni", "你"
TIMEOUT = 10


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


def capture(name):
    subprocess.run(["grim", "-o", "WINIT-0", f"/out/{name}.png"], check=True)


def step(method, expected):
    line = method.stdout.readline().strip()
    if line != expected:
        method.kill()
        print(
            f"FAIL the input method said {line!r}, expected {expected!r}",
            file=sys.stderr,
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
    method = subprocess.Popen(
        ["/out/bin/athanor-toolkit-input-method", PREEDIT, COMMIT],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    if not step(method, f"preedit {PREEDIT}"):
        return 1
    # The preedit is drawn but not committed: the field's text stays empty.
    time.sleep(1)
    capture("input-method-preedit")
    preedit_text = value_of(node, Atspi)
    method.stdin.write("commit\n")
    method.stdin.flush()
    if not step(method, f"commit {COMMIT}"):
        return 1
    committed = wait_for_text(Atspi, node, COMMIT)
    capture("input-method-commit")
    failures = []
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
