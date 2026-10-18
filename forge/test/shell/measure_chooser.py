#!/usr/bin/python3
"""measure_chooser.py: the costs of a layout chooser (doc_own_stack.md, milestone T1, task 7).

  measure_chooser.py run <binary> <out.json> [runs]
      Inside a scene (rig.sh measure-chooser runs it as RIG_HOLD): the stripped size of
      <binary>, its start-up time to the first frame and its window's buffer over [runs] traced
      launches (5), and its memory one minute after one more launch, untraced. For
      athanor-toolkit's chooser, ATHANOR_TOOLKIT_RENDERER (cpu or gpu) picks the renderer, and
      the record names it and the adapter wgpu picked.
  measure_chooser.py lines <toolkit-dir>
      The toolkit's lines, split by area, counted as tokei counts code, comments and blanks.
  measure_chooser.py report <toolkit-dir> <out.json>...
      The records and the lines as Markdown tables, for docs/spikes/t1-own-toolkit.md.

Start-up time to the first frame: from the exec of the binary to the first `wl_surface.commit`
of the window's surface (the one given to `xdg_wm_base.get_xdg_surface`) after an `attach` of a
non-null buffer, as the client's own WAYLAND_DEBUG=1 trace sends it: libwayland prints it for
both choosers, as the toolkit speaks Wayland through libwayland-client. The clock is this
script's monotonic clock when the line arrives on the client's stderr. Tracing slows both
clients by a similar amount, which is not measured; the memory is read in a launch without it.
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
RENDERER = "ATHANOR_TOOLKIT_RENDERER"
# The line athanor-toolkit prints on stderr once wgpu has picked an adapter.
ADAPTER = "athanor-toolkit: vello_gpu on "

# A request in either trace: libwayland's "[ts] {queue} -> iface#id.req(args)" (iface@id before
# 1.24) or wayland-backend's "[ts] -> iface@id.req(args)".
REQUEST = re.compile(r" -> (\w+)[@#](\d+)\.(\w+)\((.*)\)\s*$")
OBJECT = re.compile(r"(\w+)[@#](\d+)")
# The requests that make a wl_buffer, and where its width and height sit among the arguments
# that follow the new buffer: wl_shm_pool's (offset, width, height, stride, format) and
# linux-dmabuf's (width, height, format, flags).
BUFFER_SIZE = {"create_buffer": 1, "create_immed": 0}

# Each source file of the toolkit belongs to one area; a new file must be placed here.
AREAS = {
    "src/driver.rs": "driver",
    "src/render.rs": "rendering (vello_cpu, and the scene vello_gpu shares)",
    "src/gpu.rs": "rendering (vello_gpu)",
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
    "examples/layer.rs": "test clients (examples)",
}


class FirstFrame:
    """Feed it trace lines; `done` turns true on the window's first commit with a buffer."""

    def __init__(self):
        self.window = None  # the wl_surface id of the first xdg_surface
        self.buffers = {}  # wl_buffer id: (width, height)
        self.attached = {}  # wl_surface id: the wl_buffer attached, not yet committed
        self.size = None  # the window's buffer at its first frame, (width, height)
        self.done = False

    def feed(self, line):
        match = REQUEST.search(line)
        # libwayland marks the messages of a destroyed proxy "[discarded]": never sent.
        if not match or self.done or "[discarded]" in line:
            return self.done
        interface, oid, request, args = match.groups()
        objects = OBJECT.findall(args)
        if interface == "xdg_wm_base" and request == "get_xdg_surface":
            surfaces = [o for i, o in objects if i == "wl_surface"]
            if surfaces and self.window is None:
                self.window = surfaces[0]
        elif request in BUFFER_SIZE and objects and objects[0][0] == "wl_buffer":
            after = args.split(objects[0][1], 1)[1].split(",")[1:]
            first = BUFFER_SIZE[request]
            size = [a.strip() for a in after[first : first + 2]]
            # Anything else is a trace this parser does not know: the size stays unknown.
            if len(size) == 2 and all(a.isdigit() for a in size):
                self.buffers[objects[0][1]] = (int(size[0]), int(size[1]))
        elif interface == "wl_surface" and request == "attach":
            buffer = OBJECT.match(args.strip())
            if buffer and buffer.group(1) == "wl_buffer" and buffer.group(2) != "0":
                self.attached[oid] = buffer.group(2)
            else:
                self.attached.pop(oid, None)
        elif interface == "wl_surface" and request == "commit":
            buffer = self.attached.pop(oid, None)
            self.done = oid == self.window and buffer is not None
            if self.done:
                self.size = self.buffers.get(buffer)
        return self.done


def stripped_size(binary):
    with tempfile.TemporaryDirectory() as scratch:
        target = Path(scratch) / "stripped"
        subprocess.run(["strip", "-o", str(target), str(binary)], check=True)
        return target.stat().st_size


def kilobytes(path, field):
    text = Path(path).read_text(encoding="utf-8")
    match = re.search(rf"^{field}:\s+(\d+) kB", text, re.MULTILINE)
    if not match:
        raise RuntimeError(f"no {field} in {path}")
    return int(match.group(1))


def shared_objects(maps):
    """The distinct shared objects in a /proc/<pid>/maps text, one per file even when mapped
    under two paths, as {(st_dev, st_ino): size}. A file deleted since it was mapped is left
    out: its size on disk is gone."""
    found = {}
    for line in maps.splitlines():
        fields = line.split(maxsplit=5)
        if (
            len(fields) < 6
            or fields[5].endswith(" (deleted)")
            or ".so" not in Path(fields[5]).name
        ):
            continue
        try:
            st = os.stat(fields[5])
        except FileNotFoundError:
            continue
        found[(st.st_dev, st.st_ino)] = st.st_size
    return found


def memory(pid):
    objects = shared_objects(Path(f"/proc/{pid}/maps").read_text(encoding="utf-8"))
    return {
        "rss_kb": kilobytes(f"/proc/{pid}/status", "VmRSS"),
        "pss_kb": kilobytes(f"/proc/{pid}/smaps_rollup", "Pss"),
        "shared_objects": len(objects),
        "shared_objects_bytes": sum(objects.values()),
    }


def spawn(argv, env):
    return subprocess.Popen(
        argv,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        errors="replace",
    )


def first_frame(argv, env=None, timeout=FIRST_FRAME_TIMEOUT_S):
    """Run argv traced until its first frame, then stop it. Returns (seconds, the first-frame
    request, the window's buffer size, the GPU adapter the client named or None). Raises
    RuntimeError when the client exits, or draws nothing within `timeout`, first."""
    env = dict(os.environ if env is None else env, WAYLAND_DEBUG="1")
    start = time.monotonic()
    proc = spawn(argv, env)
    try:
        ended = threading.Event()  # the first frame, or the end of stderr
        frame, result = FirstFrame(), {}

        def drain():  # read to the end, so the client never blocks on a full pipe
            for line in proc.stderr:
                if line.startswith(ADAPTER):
                    result["adapter"] = line[len(ADAPTER) :].strip()
                if not ended.is_set() and frame.feed(line):
                    result["seconds"] = time.monotonic() - start
                    result["line"] = line.strip()
                    ended.set()
            ended.set()

        threading.Thread(target=drain, daemon=True).start()
        ended.wait(timeout)
        if "seconds" in result:
            return result["seconds"], result["line"], frame.size, result.get("adapter")
        if ended.is_set():
            raise RuntimeError(
                f"{argv[0]} exited with {proc.wait(10)} before its first frame"
            )
        raise RuntimeError(f"{argv[0]} drew no frame in {timeout} s")
    finally:
        stop(proc)


def stop(proc, grace=10):
    """Terminate the client, and kill it if it is still running after `grace` seconds."""
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    if proc.stderr:
        proc.stderr.close()


def resident(argv, after=MEMORY_AFTER_S):
    """Run argv untraced for `after` seconds, then read its memory and stop it."""
    proc = spawn(argv, os.environ)
    try:
        # Keep reading stderr, so a chatty client never blocks on a full pipe.
        threading.Thread(target=proc.stderr.read, daemon=True).start()
        try:
            code = proc.wait(timeout=after)
        except subprocess.TimeoutExpired:
            return memory(proc.pid)
        raise RuntimeError(f"{argv[0]} exited with {code} before the memory reading")
    finally:
        stop(proc)


def run(binary, out, runs):
    startups = []
    for index in range(runs):
        try:
            seconds, line, size, adapter = first_frame([binary])
        except RuntimeError as error:
            sys.exit(f"measure_chooser: {error} (run {index + 1})")
        # A binary that ignores the variable would otherwise be reported as vello_gpu.
        if os.environ.get(RENDERER) == "gpu" and adapter is None:
            sys.exit(
                f"measure_chooser: {binary} named no GPU adapter with {RENDERER}=gpu "
                f"(run {index + 1})"
            )
        startups.append(seconds)
    try:
        mem = resident([binary])
    except (RuntimeError, OSError) as error:
        sys.exit(f"measure_chooser: {error}")
    cpu = re.search(
        r"^model name\s*:\s*(.+)$",
        Path("/proc/cpuinfo").read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    record = {
        "binary": Path(binary).name,
        "renderer": os.environ.get(RENDERER) or None,
        "adapter": adapter,
        "stripped_bytes": stripped_size(binary),
        "startup_s": startups,
        "first_frame_request": line,
        "window_buffer": size,
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


def label(record):
    renderer = record.get("renderer")
    return f"`{record['binary']}`" + (f" ({RENDERER}={renderer})" if renderer else "")


def report(toolkit, outs):
    print(
        "| Chooser | Stripped binary | Start-up to first frame, median (min-max, runs) "
        "| Window buffer | RSS after 1 min | PSS after 1 min "
        "| Shared objects mapped, their on-disk size |"
    )
    print("| --- | --- | --- | --- | --- | --- | --- |")
    records = [json.loads(Path(out).read_text(encoding="utf-8")) for out in outs]
    for r in records:
        ms = [s * 1000 for s in r["startup_s"]]
        size = r.get("window_buffer")
        print(
            f"| {label(r)} | {r['stripped_bytes'] / 1048576:.2f} MiB "
            f"| {statistics.median(ms):.0f} ms ({min(ms):.0f}-{max(ms):.0f}, {len(ms)}) "
            f"| {f'{size[0]}x{size[1]}' if size else 'unknown'} "
            f"| {r['rss_kb'] / 1024:.1f} MiB | {r['pss_kb'] / 1024:.1f} MiB "
            f"| {r['shared_objects']}, {r['shared_objects_bytes'] / 1048576:.1f} MiB |"
        )
    print()
    print("The request each start-up time ends on, from the last launch:")
    print()
    for r in records:
        print(f"- {label(r)}: `{r['first_frame_request']}`")
    gpu = [r for r in records if r.get("adapter")]
    if gpu:
        print()
        print("The adapter wgpu picked:")
        print()
        for r in gpu:
            print(f"- {label(r)}: {r['adapter']}")
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
