"""Behaviour scenarios of the shell standard (doc_shell_standard.md, ST6), measured on the
reference machine with real input. Each one: an input, the expected screen, a time limit.
The dock's auto-hide comes first: these are the cases that would have caught its defect.

The desktop must be empty: the screen below the dock is compared with a reference taken
with the dock stopped.
"""

import shlex
import time

import analysis
import bench

LAYOUT = "~/.config/athanor/layout.toml"
DOC = 'schema = 1\n[output."*"]\ndock = "{dock}"\n'
MS = 10**6
# Auto-hide's delays are 0.2 s to reveal and 1 s to hide (athanor_dock::autohide); ST6's
# limits leave 0.1 s and 0.2 s of room on top.
REVEAL_LIMIT, HIDE_LIMIT, HORIZON = 300 * MS, 1200 * MS, 1700 * MS
# Pixels that may differ in the band once hidden: the strip, and noise.
HIDDEN_NOISE = 0.005
CENTRE_TOLERANCE_PX = 2


def _write_layout(m, dock):
    m.run(f"mkdir -p ~/.config/athanor && printf %s {shlex.quote(DOC.format(dock=dock))} > {LAYOUT}")


def run(m):
    original = m.run(f"if [ -f {LAYOUT} ]; then cat {LAYOUT}; fi")
    results = {}
    try:
        with bench.instrumented(m) as since:
            _write_layout(m, "visible")
            m.systemctl("restart", "athanor-dock")
            time.sleep(3)
            # An application button: its context menu is the one the last scenario opens.
            dock = next(t for t in bench.locate(m, since) if t.unit == "athanor-dock" and t.popup)
            _, placements = analysis.parse_journal(m.journal(since))
            island = [p for p in placements if p.unit == "athanor-dock" and p.surface == "dock"][-1]
            band = (island.y, island.output_h - 4)  # the strip's 4 px are allowed to change
            w, h = island.output_w, island.output_h
            centre = (w // 2, h // 2)
            m.systemctl("stop", "athanor-dock")
            bench.inject(m, w, h, [f"move {centre[0]} {centre[1]}", "wait 300"])
            reference = m.screenshot()
            m.systemctl("start", "athanor-dock")
            time.sleep(3)

            def hidden():
                count, _ = analysis.changed(reference, m.screenshot(), band)
                return count <= HIDDEN_NOISE * w * (band[1] - band[0]), f"{count} pixels differ"

            def shown():
                count, box = analysis.changed(reference, m.screenshot(), (island.y, h))
                if box is None:
                    return False, "nothing drawn"
                offset = (box[0] + box[2]) / 2 - w / 2
                wide = box[2] - box[0] + 1 >= 0.9 * island.w
                return abs(offset) <= CENTRE_TOLERANCE_PX and wide, f"box {box}, off centre by {offset:+.1f} px"

            def check(name, ops, marker, limit, state):
                injected = bench.inject(m, w, h, ops + [f"wait {HORIZON // MS}"])
                t = [ns for op, ns in injected if op == marker][-1]
                frames, _ = analysis.parse_journal(m.journal(since))
                in_time = analysis.settled([t], frames, "athanor-dock", "dock", limit, HORIZON)[0]
                ok, detail = state()
                results[f"dock-autohide/{name}"] = {"pass": in_time and ok, "register": None,
                                                    "detail": f"settled in time: {in_time}; {detail}"}

            # Switching from visible to auto-hide while running hides the dock as a start in auto-hide does.
            t0 = int(m.session("python3 -c 'import time; print(time.clock_gettime_ns(time.CLOCK_MONOTONIC))'"))
            _write_layout(m, "auto-hide")
            time.sleep(HORIZON / 1e9 + 0.5)
            frames, _ = analysis.parse_journal(m.journal(since))
            in_time = analysis.settled([t0], frames, "athanor-dock", "dock", HIDE_LIMIT, HORIZON)[0]
            ok, detail = hidden()
            results["dock-autohide/switch"] = {"pass": in_time and ok, "register": None,
                                               "detail": f"settled in time: {in_time}; {detail}"}
            edge = f"move {w // 2} {h - 1}"
            # Resting on the edge reveals the dock at its place, centred, within 0.3 s.
            check("rest", [edge], edge, REVEAL_LIMIT, shown)
            # Leaving it hides it, so that only the strip is drawn, within 1.2 s.
            away = f"move {centre[0]} {centre[1]}"
            check("leave", [away], away, HIDE_LIMIT, hidden)
            # After a context menu opens and closes (a click on the desktop: Escape does not close it,
            # spike Q2) and the pointer leaves, it hides again.
            check("menu", [edge, "wait 600", f"move {dock.x} {dock.y}", "wait 300", "rclick", "wait 500",
                           f"move {bench.DESKTOP[0]} {bench.DESKTOP[1]}", "click", "wait 300", away], away, HIDE_LIMIT, hidden)
    finally:
        if original:
            m.run(f"printf %s {shlex.quote(original.decode())} > {LAYOUT}")
        else:
            m.run(f"rm -f {LAYOUT}")
        m.systemctl("restart", "athanor-dock")
    return results
