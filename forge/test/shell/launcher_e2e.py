#!/usr/bin/python3
"""launcher_e2e.py <stage> - athanor-launcher end to end in the rig, as scene.sh's RIG_HOLD,
with the launcher started by bar_session.py --client athanor-launcher over the float layout
and shown at start by ATHANOR_LAUNCHER_SHOW. Stages:

- e2e: READY=1 reaches NOTIFY_SOCKET (Type=notify); the launcher owns os.athanor.Launcher1;
  "2+2*3" answers 8 in a Calculator group (§5 item 3, with the real qalc); Show hides and
  shows again with the query kept (LA5); every show logs "shown" with its time, under
  150 ms (§5 item 1); at rest it stays within 80 MB PSS (§5 item 10, open doubt 4).
- frozen-provider: with a provider that never answers, the Applications group is listed
  within the 1 s deadline of a query, and the provider costs one "did not answer in time"
  warning per query, nothing more (Review Focus 2).
- hostile: a desktop entry named with U+202E and markup shows its text as plain text,
  markup literal and the override stripped by text::line, in the row's accessible name; no
  accessible name carries U+202E (Review Focus 3, §5 item 8).
- no-localsearch: localsearch is absent from the rig's session: no Files group, no
  "Indexing files…" row, and no error in the log (Review Focus 5, §5 item 7).
- window-preview: hide and show, so the window that came after the start is listed, and
  wait for its row (the capture follows in scene.sh).
"""

import os
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application  # noqa: E402
from bar_e2e import check, failures, pss_kb, wait_for  # noqa: E402
from bar_session import READY_FILE  # noqa: E402

PSS_LIMIT_KB = 80 * 1024
SHOW_LIMIT_MS = 150
MISSED = "the search provider did not answer in time"  # the warning the launcher logs per query
NAME = "os.athanor.Launcher1"
LOG = Path(f"/out/{os.environ.get('RIG_TAG', 'launcher')}-athanor-launcher.log")


def show():
    subprocess.run(
        ["gdbus", "call", "--session", "--dest", NAME, "--object-path", "/os/athanor/Launcher1",
         "--method", f"{NAME}.Show"],
        check=True, capture_output=True,
    )


def labels(Atspi):
    """The accessible names of the launcher's list rows, in order."""
    app = find_application(Atspi, "athanor-launcher", 1)
    if app is None:
        return []
    found = []

    def walk(node):
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child.get_role() == Atspi.Role.LIST_ITEM:
                found.append(child.get_name())
            walk(child)

    walk(app)
    return found


def launcher_pid():
    """The launcher's pid, from the owner of its bus name (PID_FILE is the bar's)."""
    status = subprocess.run(["busctl", "--user", "status", NAME], capture_output=True, check=False, text=True)
    for line in status.stdout.splitlines():
        if line.startswith("PID="):
            return int(line[4:])
    return None


def log_text():
    return LOG.read_text(encoding="utf-8", errors="replace") if LOG.exists() else ""


def show_times():
    return [int(ms) for ms in re.findall(r"shown ms=(\d+)", log_text())]


def stage_e2e(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    owner = subprocess.run(["busctl", "--user", "status", NAME], capture_output=True, check=False)
    check("bus name", owner.returncode == 0, owner.stderr.decode(errors="replace"))
    check("calculator", wait_for(lambda: "8, Calculation" in labels(Atspi), 5), str(labels(Atspi)))
    show()  # hides
    check("hidden", wait_for(lambda: not labels(Atspi), 3), str(labels(Atspi)))
    show()  # shows again, with "2+2*3" remembered
    check("query kept", wait_for(lambda: "8, Calculation" in labels(Atspi), 5), str(labels(Atspi)))
    times = show_times()
    check("show time", len(times) >= 2 and max(times) <= SHOW_LIMIT_MS, f"shown ms: {times}")
    time.sleep(5)  # at rest
    pid = launcher_pid()
    check("launcher pid", pid is not None, "no PID= in busctl status")
    pss = pss_kb(pid) if pid is not None else None
    check("memory", pss is not None and pss <= PSS_LIMIT_KB, f"{pss} kB PSS, limit {PSS_LIMIT_KB}")


def stage_frozen_provider(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    check("applications", wait_for(lambda: "CC Window, Application" in labels(Atspi), 5), str(labels(Atspi)))
    time.sleep(2)  # past the deadline
    warnings = log_text().count(MISSED)
    check("one warning", warnings == 1, f"{warnings} warnings")
    # Timed: hide, then show again, which runs the remembered query anew (LA5). The other
    # groups must not wait for the frozen provider.
    show()
    check("hidden", wait_for(lambda: not labels(Atspi), 3), str(labels(Atspi)))
    started = time.monotonic()
    show()
    check(
        "applications within the deadline",
        wait_for(lambda: "CC Window, Application" in labels(Atspi), 1),
        f"{time.monotonic() - started:.2f} s: {labels(Atspi)}",
    )
    time.sleep(2)
    warnings = log_text().count(MISSED)
    check("one more warning", warnings == 2, f"{warnings} warnings")
    pid = launcher_pid()
    check("still running", pid is not None and Path(f"/proc/{pid}").exists(), "the launcher is gone")


def stage_hostile(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    # text::line strips the bidi controls and the markup is shown literally (F13).
    expected = "gnp.exe <b>bold</b>, Application"
    check("plain text", wait_for(lambda: expected in labels(Atspi), 5), repr(labels(Atspi)))
    check("no bidi override", not any("\u202e" in row for row in labels(Atspi)), repr(labels(Atspi)))


def stage_no_localsearch(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    check("rows", wait_for(lambda: "CC Window, Application" in labels(Atspi), 5), str(labels(Atspi)))
    time.sleep(2)
    rows = labels(Atspi)
    check("no file group", not any(row.endswith(", File") or row == "Files" for row in rows), str(rows))
    check("no indexing row", "Indexing files…" not in rows, str(rows))
    # The journal format prefixes the priority: <3> is an error.
    errors = [line for line in log_text().splitlines() if line.startswith("<3>")]
    check("no error", not errors, "\n".join(errors))


def stage_window_preview(Atspi):
    check("READY=1", wait_for(lambda: READY_FILE.exists(), 10), "no READY=1 on NOTIFY_SOCKET")
    time.sleep(2)  # bar_session.py opens the window at READY
    show()
    show()
    check("window row", wait_for(lambda: any(row.endswith(", Window") for row in labels(Atspi)), 5), str(labels(Atspi)))
    time.sleep(2)  # the preview's delay and the capture


STAGES = {
    "e2e": stage_e2e,
    "frozen-provider": stage_frozen_provider,
    "hostile": stage_hostile,
    "no-localsearch": stage_no_localsearch,
    "window-preview": stage_window_preview,
}


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    STAGES[sys.argv[1]](Atspi)
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
