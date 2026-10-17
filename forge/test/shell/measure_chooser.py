#!/usr/bin/python3
"""measure_chooser.py: the costs of a layout chooser (doc_own_stack.md, milestone T1, task 7).

  measure_chooser.py run <binary> <out.json> [runs]
      Inside a scene (rig.sh measure-chooser runs it as RIG_HOLD): the stripped size of
      <binary>, its start-up time to the first frame over [runs] launches (5), and its
      resident memory one minute after the last launch.
  measure_chooser.py lines <toolkit-dir>
      The toolkit's lines, split by area, counted as tokei counts code, comments and blanks.
  measure_chooser.py report <toolkit-dir> <out.json>...
      Both as Markdown tables, for docs/spikes/t1-own-toolkit.md.

Start-up time to the first frame: from the exec of the binary to the first `wl_surface.commit`
of the window's surface (the one given to `xdg_wm_base.get_xdg_surface`) after an `attach` of a
non-null buffer, as the client's own WAYLAND_DEBUG=1 trace sends it: libwayland prints it for
GTK, wayland-backend for the toolkit. The clock is this script's monotonic clock when the line
arrives on the client's stderr. Both clients pay the same tracing cost.
"""

import json
import os
import re
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

RUNS = 5
MEMORY_AFTER_S = 60
FIRST_FRAME_TIMEOUT_S = 30

# A request in either trace: libwayland's "[ts] {queue} -> iface#id.req(args)" (iface@id before
# 1.24) or wayland-backend's "[ts] -> iface@id.req(args)".
REQUEST = re.compile(r" -> (\w+)[@#](\d+)\.(\w+)\((.*)\)\s*$")
OBJECT = re.compile(r"(\w+)[@#](\d+)")

# Each source file of the toolkit belongs to one area; a new file must be placed here.
AREAS = {
    "src/driver.rs": "driver",
    "src/render.rs": "rendering (vello_cpu)",
    "src/widget.rs": "widgets",
    "src/focus.rs": "widgets",
    "src/theme.rs": "widgets",
    "src/lib.rs": "widgets",
    "src/a11y.rs": "accessibility",
    "src/text.rs": "text",
    "examples/chooser/main.rs": "chooser (example)",
    "examples/chooser/tree.rs": "chooser (example)",
    "examples/field.rs": "test clients (examples)",
    "examples/input_method.rs": "test clients (examples)",
}


class FirstFrame:
    """Feed it trace lines; `done` turns true on the window's first commit with a buffer."""

    def __init__(self):
        self.window = None  # the wl_surface id of the xdg_surface
        self.attached = (
            set()
        )  # wl_surface ids with a non-null buffer attached, not yet committed
        self.done = False

    def feed(self, line):
        match = REQUEST.search(line)
        if not match or self.done:
            return self.done
        interface, oid, request, args = match.groups()
        if interface == "xdg_wm_base" and request == "get_xdg_surface":
            surfaces = [o for i, o in OBJECT.findall(args) if i == "wl_surface"]
            if surfaces:
                self.window = surfaces[0]
        elif interface == "wl_surface" and request == "attach":
            buffer = OBJECT.match(args.strip())
            if buffer and buffer.group(1) == "wl_buffer" and buffer.group(2) != "0":
                self.attached.add(oid)
            else:
                self.attached.discard(oid)
        elif interface == "wl_surface" and request == "commit":
            self.done = oid == self.window and oid in self.attached
            self.attached.discard(oid)
        return self.done


def stripped_size(binary):
    with tempfile.TemporaryDirectory() as scratch:
        target = Path(scratch) / "stripped"
        subprocess.run(["strip", "-o", str(target), str(binary)], check=True)
        return target.stat().st_size


def memory(pid):
    status = Path(f"/proc/{pid}/status").read_text(encoding="utf-8")
    rss = int(re.search(r"^VmRSS:\s+(\d+) kB", status, re.MULTILINE).group(1))
    rollup = Path(f"/proc/{pid}/smaps_rollup").read_text(encoding="utf-8")
    pss = int(re.search(r"^Pss:\s+(\d+) kB", rollup, re.MULTILINE).group(1))
    objects = set()
    for line in Path(f"/proc/{pid}/maps").read_text(encoding="utf-8").splitlines():
        fields = line.split(maxsplit=5)
        if len(fields) == 6 and ".so" in Path(fields[5]).name:
            objects.add(fields[5])
    return {
        "rss_kb": rss,
        "pss_kb": pss,
        "shared_objects": len(objects),
        "shared_objects_bytes": sum(os.stat(o).st_size for o in objects),
    }


def launch(binary):
    """Start the binary traced; return (process, seconds to first frame or None, first-frame line)."""
    env = dict(os.environ, WAYLAND_DEBUG="1")
    start = time.monotonic()
    proc = subprocess.Popen(
        [binary],
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        errors="replace",
    )
    seen = threading.Event()
    result = {}

    def drain():  # read to the end, so the client never blocks on a full pipe
        frame = FirstFrame()
        for line in proc.stderr:
            if not seen.is_set() and frame.feed(line):
                result["seconds"] = time.monotonic() - start
                result["line"] = line.strip()
                seen.set()

    threading.Thread(target=drain, daemon=True).start()
    seen.wait(FIRST_FRAME_TIMEOUT_S)
    return proc, start, result


def stop(proc):
    proc.terminate()
    proc.wait(timeout=10)


def run(binary, out, runs):
    startups = []
    line = None
    for index in range(runs):
        proc, start, result = launch(binary)
        if "seconds" not in result:
            stop(proc)
            sys.exit(
                f"measure_chooser: {binary} drew no frame in {FIRST_FRAME_TIMEOUT_S} s (run {index + 1})"
            )
        startups.append(result["seconds"])
        line = result["line"]
        if index < runs - 1:
            stop(proc)
    time.sleep(max(0.0, MEMORY_AFTER_S - (time.monotonic() - start)))
    if proc.poll() is not None:
        sys.exit(
            f"measure_chooser: {binary} exited before the memory reading ({proc.returncode})"
        )
    mem = memory(proc.pid)
    stop(proc)
    cpu = re.search(
        r"^model name\s*:\s*(.+)$",
        Path("/proc/cpuinfo").read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    record = {
        "binary": Path(binary).name,
        "stripped_bytes": stripped_size(binary),
        "startup_s": startups,
        "first_frame_request": line,
        "memory_after_s": MEMORY_AFTER_S,
        **mem,
        "cpu": cpu.group(1) if cpu else "unknown",
        "kernel": os.uname().release,
        "gsk_renderer": os.environ.get("GSK_RENDERER", "default"),
    }
    Path(out).write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(
        f"measure_chooser: {record['binary']}: first frame median "
        f"{statistics.median(startups) * 1000:.0f} ms, RSS {mem['rss_kb']} kB"
    )


def count(text):
    """tokei's three counts for Rust source: (code, comments, blanks), plus the code lines from
    the first top-level #[cfg(test)] to the end of the file, the convention of this crate."""
    code = comments = blanks = tests = 0
    in_block = in_tests = False
    for raw in text.splitlines():
        line = raw.strip()
        if raw.startswith("#[cfg(test)]"):
            in_tests = True
        if in_block:
            comments += 1
            in_block = "*/" not in line
        elif not line:
            blanks += 1
        elif line.startswith("//"):
            comments += 1
        elif line.startswith("/*"):
            comments += 1
            in_block = "*/" not in line[2:]
        else:
            code += 1
            tests += in_tests
    return code, comments, blanks, tests


def lines(toolkit):
    root = Path(toolkit)
    totals = {}
    found = sorted(str(p.relative_to(root)) for p in root.rglob("*.rs"))
    unplaced = [f for f in found if f not in AREAS]
    if unplaced:
        sys.exit(
            f"measure_chooser: no area for {', '.join(unplaced)}; add them to AREAS"
        )
    for name in found:
        area = totals.setdefault(AREAS[name], [0, 0, 0, 0])
        for i, value in enumerate(count((root / name).read_text(encoding="utf-8"))):
            area[i] += value
    return totals


def report(toolkit, outs):
    print(
        "| Chooser | Stripped binary | Start-up to first frame, median (min-max, runs) "
        "| RSS after 1 min | PSS after 1 min | Shared objects mapped |"
    )
    print("| --- | --- | --- | --- | --- | --- |")
    for out in outs:
        r = json.loads(Path(out).read_text(encoding="utf-8"))
        ms = [s * 1000 for s in r["startup_s"]]
        print(
            f"| `{r['binary']}` | {r['stripped_bytes'] / 1048576:.2f} MiB "
            f"| {statistics.median(ms):.0f} ms ({min(ms):.0f}-{max(ms):.0f}, {len(ms)}) "
            f"| {r['rss_kb'] / 1024:.1f} MiB | {r['pss_kb'] / 1024:.1f} MiB "
            f"| {r['shared_objects']}, {r['shared_objects_bytes'] / 1048576:.1f} MiB |"
        )
    print()
    print("| Area | Code | Of which tests | Comments | Blanks |")
    print("| --- | --- | --- | --- | --- |")
    totals = lines(toolkit)
    for area, (code, comments, blanks, tests) in totals.items():
        print(f"| {area} | {code} | {tests} | {comments} | {blanks} |")
    code, comments, blanks, tests = (
        sum(v[i] for v in totals.values()) for i in range(4)
    )
    print(f"| total | {code} | {tests} | {comments} | {blanks} |")


def main(argv):
    if len(argv) in (3, 4) and argv[0] == "run":
        run(argv[1], argv[2], int(argv[3]) if len(argv) == 4 else RUNS)
    elif len(argv) == 2 and argv[0] == "lines":
        for area, values in lines(argv[1]).items():
            print(area, *values, sep="\t")
    elif len(argv) >= 3 and argv[0] == "report":
        report(argv[1], argv[2:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
