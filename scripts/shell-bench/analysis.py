"""Pure functions of the shell bench (doc_shell_standard.md, ST5): what the machine reports,
turned into numbers and verdicts. No input or output here: bench.py feeds it."""

import json
import math
import re
from typing import NamedTuple

# ST5, verbatim. Changing one is a revision of the standard approved by the maintainer (ST10).
THRESHOLDS = {
    "in_place_ms": 50,
    "opening_ms": 100,
    "on_time": 0.99,
    "late_intervals": 2,
    "start_s": 1.5,
    "idle_cpu": 0.001,
    "pss_mb": {"athanor-bar": 64, "athanor-dock": 48, "athanor-shelld": 16},
    "growth": 0.10,
    "recovery_s": 1.0,
}
REPETITIONS = 50
NO_FRAME_S = 2.0
# A window with fewer frames is a single change, not an animation.
ANIMATION_FRAMES = 5
# A frame is on time when it follows the previous one by at most this many refresh intervals.
ON_TIME_INTERVALS = 1.5
# AT-SPI role names of a button: GTK 4 on the reference machine reports "button" (spike Q2),
# older at-spi2 releases "push button".
BUTTON_ROLES = ("button", "push button", "toggle button")


class Frame(NamedTuple):
    unit: str
    surface: str
    pid: int
    frame: int
    presented_ns: int
    refresh_ns: int


class Placement(NamedTuple):
    unit: str
    surface: str
    x: int
    y: int
    w: int
    h: int
    output_w: int
    output_h: int


class Target(NamedTuple):
    unit: str
    surface: str
    name: str
    popup: bool
    x: int
    y: int
    output_w: int
    output_h: int


def parse_journal(entries):
    """Frames with a presentation time, and placements, from `journalctl -o json` entries."""
    frames, placements = [], []
    for entry in entries:
        message = entry.get("MESSAGE")
        start = message.find('{"bench":') if isinstance(message, str) else -1
        if start < 0:
            continue
        try:
            record = json.loads(message[start:])
        except json.JSONDecodeError:
            continue
        unit = entry.get("_SYSTEMD_USER_UNIT", "").removesuffix(".service")
        if record["bench"] == "frame" and record["presented_us"] > 0:
            frames.append(Frame(unit, record["surface"], int(entry.get("_PID", 0)), record["frame"],
                                record["presented_us"] * 1000, record["refresh_us"] * 1000))
        elif record["bench"] == "surface":
            placements.append(Placement(unit, record["surface"], record["x"], record["y"], record["w"],
                                        record["h"], record["output_w"], record["output_h"]))
    return frames, placements


def targets(placement, nodes):
    """The buttons of the window of `nodes` (tree.py's lines) that is `placement`'s surface,
    aimed at their centre in output coordinates."""
    windows = [n["toplevel"] for n in nodes if "toplevel" in n and (n["w"], n["h"]) == (placement.w, placement.h)]
    if len(windows) != 1:
        raise RuntimeError(f"{len(windows)} windows of {placement.unit} match its {placement.surface} "
                           f"surface of {placement.w}x{placement.h}: the bench needs exactly one")
    return [
        Target(placement.unit, placement.surface, n["name"], n["popup"],
               placement.x + n["x"] + n["w"] // 2, placement.y + n["y"] + n["h"] // 2,
               placement.output_w, placement.output_h)
        for n in nodes
        if n.get("window") == windows[0] and n["role"] in BUTTON_ROLES and n["w"] > 0
    ]


def _of(frames, unit, surface):
    return sorted((f for f in frames if f.unit == unit and f.surface == surface), key=lambda f: f.presented_ns)


def responses_ms(times_ns, frames, unit, surface):
    """Per input time, milliseconds to the first frame of the surface presented after it;
    None when none came within NO_FRAME_S."""
    shown = _of(frames, unit, surface)
    out = []
    for t in times_ns:
        first = next((f.presented_ns for f in shown if f.presented_ns > t), None)
        out.append(None if first is None or first - t > NO_FRAME_S * 1e9 else (first - t) / 1e6)
    return out


def percentile(values, p):
    """Nearest-rank percentile; a miss (None) counts as infinitely slow."""
    ranked = sorted(math.inf if v is None else v for v in values)
    if not ranked:
        raise ValueError("no samples")
    return ranked[max(0, math.ceil(p / 100 * len(ranked)) - 1)]


def smoothness(frames, windows_ns, unit, surface):
    """(share of frames on time, worst gap in refresh intervals, windows measured) over the
    windows (start, end) that hold an animation; (None, 0.0, 0) when none does."""
    shown = _of(frames, unit, surface)
    on_time = total = measured = 0
    worst = 0.0
    for start, end in windows_ns:
        inside = [f for f in shown if start <= f.presented_ns <= end]
        if len(inside) < ANIMATION_FRAMES:
            continue
        measured += 1
        for a, b in zip(inside, inside[1:]):
            ratio = (b.presented_ns - a.presented_ns) / b.refresh_ns
            total += 1
            on_time += ratio <= ON_TIME_INTERVALS
            worst = max(worst, ratio)
    return (on_time / total if total else None), worst, measured


def settled(times_ns, frames, unit, surface, limit_ns, horizon_ns):
    """Per input time t, whether the surface's last frame in (t, t + horizon] came by t + limit:
    the surface reached its final state within the limit and stayed there."""
    shown = _of(frames, unit, surface)
    out = []
    for t in times_ns:
        last = [f.presented_ns for f in shown if t < f.presented_ns <= t + horizon_ns]
        out.append(bool(last) and last[-1] <= t + limit_ns)
    return out


def cpu_ticks(stat):
    """utime + stime of a /proc/<pid>/stat line (fields 14 and 15 of proc(5))."""
    fields = stat[stat.rindex(")") + 2:].split()
    return int(fields[11]) + int(fields[12])


def pss_kb(smaps_rollup):
    for line in smaps_rollup.splitlines():
        if line.startswith("Pss:"):
            return int(line.split()[1])
    raise ValueError("no Pss line in smaps_rollup")


_PPM = re.compile(rb"P6\s+(\d+)\s+(\d+)\s+255\s")


def read_ppm(data):
    """(width, height, pixels) of a binary PPM as `grim -t ppm` writes it."""
    match = _PPM.match(data)
    if match is None:
        raise ValueError("not a binary PPM with maxval 255")
    return int(match[1]), int(match[2]), data[match.end():]


def changed(a, b, rows, tolerance=24):
    """How many pixels in rows [start, stop) differ between two PPM images by more than
    `tolerance` in any channel, and their bounding box (x0, y0, x1, y1), inclusive."""
    w, h, pa = read_ppm(a)
    if read_ppm(b)[:2] != (w, h):
        raise ValueError("the images differ in size")
    pb = read_ppm(b)[2]
    count, box = 0, None
    for y in range(*rows):
        base = y * w * 3
        for x in range(w):
            i = base + x * 3
            if max(abs(pa[i + c] - pb[i + c]) for c in range(3)) > tolerance:
                count += 1
                box = (x, y, x, y) if box is None else (
                    min(box[0], x), min(box[1], y), max(box[2], x), max(box[3], y))
    return count, box


def normalise(names):
    """(role, name) pairs with digits masked: a clock must not read as a lost state."""
    return [(role, re.sub(r"\d+", "#", name)) for role, name in names]


def verdict(value, limit, *, at_least=False):
    passed = value is not None and (value >= limit if at_least else value <= limit)
    return {"value": value, "limit": limit, "pass": passed}
