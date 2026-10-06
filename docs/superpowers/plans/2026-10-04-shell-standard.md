# Shell standard: bench, register v1, first measurement and dock repair — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `doc_shell_standard.md` stand (its section 7): a bench that measures the bar and the dock on the reference laptop against ST5, register version 1, the first results, the dock's auto-hide repaired against its ST6 scenarios, the restart delay repaired, and the section 5 document changes.

**Architecture:** The bench is a host-side Python program in `scripts/shell-bench/` that drives the reference machine over SSH. Its helpers are streamed to the machine's `python3` the way `scripts/devvm/` does in the VM, so nothing is installed there. Input comes from throwaway `uinput` devices; screenshots from `grim -t ppm`; positions from AT-SPI plus a geometry line the programs log. Time comes from inside the programs: with `ATHANOR_SHELL_BENCH` set, `athanor_apps::timing` logs every presented frame of every bar and dock surface and popover as a JSON line in the journal, carrying GTK's presentation time on `CLOCK_MONOTONIC`, the same clock the input helper stamps its events with.

**Tech Stack:** Rust (gtk4 0.11, gdk4 frame clock, gtk4-layer-shell 0.8), Python 3 stdlib on host and machine, PyGObject Atspi on the machine (already used by `scripts/devvm/dock_press.py`), `grim`, `journalctl -o json`, `bootc usr-overlay`, `just rpms`.

**Spec:** `docs/architecture/doc_shell_standard.md` (revision 1, branch `shell-standard-spec`). Read it first; this plan argues from it.

## Global Constraints

- English in the repository (code, comments, docs, commit messages), enterprise tone; Italian with the maintainer in chat.
- ST5 thresholds, verbatim: in place 50 ms p95; opening 100 ms p95; at least 99% of animation frames at their target refresh and none later than two refresh intervals; start 1.5 s; idle under 0.1% of one CPU over 10 minutes; `athanor-bar` 64 MB PSS, `athanor-dock` 48 MB, `athanor-shelld` 16 MB; growth at most 10% over the soak; soak 24 hours; recovery within 1 s after `SIGKILL`, same state.
- A percentile is taken over at least 50 repetitions (ST5).
- A failing measurement is never relaxed inside the change that fails it (ST2); thresholds change only by an approved revision (ST10).
- The reference machine runs the image as shipped, with no layered package and no persistent change for the bench (ST9). Every change the bench makes (environment, layout file, units stopped) is undone in a `finally`.
- A gate is measured on a published image, never on an overlay (ST9). Overlays are for changes not yet merged.
- A spike that fails stops the plan: report to the maintainer, revise ST9, wait for approval (spec section 3).
- No `cd` in commands; run tests through `node /home/hr-mes/.claude/bin/cc-test.mjs -- <command>`.
- `.md` files under `docs/architecture/` are edited with a Python replace from Bash, never with Edit/Write (the formatter rewrites the whole file); check `git diff --numstat` shows only the intended lines.
- Git writes and `gh` run unsandboxed (sandbox traps); never push `forge/**` while an Orchestrator run is in progress.
- No attribution anywhere.

## Review Focus

1. **A popover that does not close on Escape** turns the next release into a close: no opening frame follows it, so it is a miss, and the stage reports the misses per button instead of measuring a wrong time; Task 2 settles beforehand whether Escape closes the bar's and the dock's popovers.
2. **The bench dying halfway** (SSH dropped, a helper raising) must not leave `ATHANOR_SHELL_BENCH` set, the layout file rewritten or a unit stopped on the reference machine: every stage that changes something restores it in `finally`, and Task 6 has a test that a raising stage still runs the restore.
3. **Frames whose presentation feedback never arrives** (a surface destroyed, a compositor that sends `discarded`) must not loop for ever in the program: `timing` drops a frame after eight flushes, with a unit test on the bookkeeping.
4. **Two surfaces of the same size** (a second output) make the AT-SPI-to-surface match ambiguous: the bench refuses with a clear error rather than clicking the wrong place; tested in Task 5.
5. **A miss counts as a failure, not as a missing sample:** an action that never presents a frame within 2 s is infinitely slow in the percentile; tested in Task 5.

---

## File Structure

| Path                                                                      | Responsibility                                            |
| ------------------------------------------------------------------------- | --------------------------------------------------------- |
| `system/athanor-apps/src/timing.rs`                                       | frame and geometry log lines behind `ATHANOR_SHELL_BENCH` |
| `system/athanor-apps/src/menu.rs`                                         | every popover watched                                     |
| `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs`, `ui/popups.rs` | bar and notification windows watched                      |
| `forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/surface.rs`           | dock window watched; auto-hide repair                     |
| `forge/specs/athanor-{bar,dock,shelld}/*/data/*.service`                  | restart delay                                             |
| `scripts/shell-bench/analysis.py`                                         | pure parsing, statistics and verdicts                     |
| `scripts/shell-bench/machine.py`                                          | the reference machine over SSH                            |
| `scripts/shell-bench/bench.py`                                            | stages and the results writer                             |
| `scripts/shell-bench/scenarios.py`                                        | ST6 scenarios, dock auto-hide first                       |
| `scripts/shell-bench/soak.py`                                             | the 24 h soak                                             |
| `scripts/shell-bench/machine/inject.py`, `machine/tree.py`                | helpers streamed to the machine                           |
| `scripts/shell-bench/allow.txt`                                           | soak allow list, empty                                    |
| `scripts/shell-bench/README.md`                                           | how to prepare the machine and run the bench              |
| `scripts/tests/test_shell_bench.py`                                       | tests of the pure parts                                   |
| `docs/shell-bench/machine.md`, `docs/shell-bench/spikes.md`               | machine facts, spike answers                              |
| `docs/shell-bench/<date>-<commit>/`                                       | results                                                   |
| `docs/architecture/shell-features.md`                                     | register version 1                                        |

---

### Task 1: Reference machine prepared, connection and facts

**Files:**

- Create: `scripts/shell-bench/machine.py`, `scripts/shell-bench/README.md`, `docs/shell-bench/machine.md`

**Interfaces:**

- Produces: `machine.Machine(host, display="wayland-1")` with `run(command, *, stdin=None, check=True, timeout=600) -> bytes`, `session(command, **kw) -> bytes`, `helper(path, *args, root=False, session=False, **kw) -> bytes`, `wall() -> int`, `journal(since, units=UNITS) -> list[dict]`, `screenshot() -> bytes` (PPM), `systemctl(*args) -> bytes`, `main_pid(unit) -> int`; constant `UNITS = ("athanor-bar", "athanor-dock", "athanor-shelld")`; `HERE`.

- [ ] **Step 1: Maintainer checkpoint.** Ask the maintainer (in Italian) to prepare the laptop, and wait for confirmation of each item:
  - Athanor installed from the published default image (not `-nvidia`), one user, logged in to the desktop, no windows open, display blanking and suspend on idle off for the duration of the bench.
  - SSH key-only access from the desktop; a host alias in `~/.ssh/config` (this plan uses `athanor-ref`).
  - Passwordless `sudo` for that user (the bench creates `uinput` devices, reads `bootc status` and suspends in the soak). It is a test machine (ST9).
  - The laptop on mains power.

- [ ] **Step 2: Write `scripts/shell-bench/machine.py`**

```python
"""The reference machine of the shell standard over SSH (doc_shell_standard.md, ST9).

Commands run through the user's SSH configuration (a host alias, key only). Helpers are
streamed to the machine's own python3 on standard input, as scripts/devvm does in the VM,
so nothing is installed there.
"""

import json
import pathlib
import shlex
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
UNITS = ("athanor-bar", "athanor-dock", "athanor-shelld")


class Machine:
    def __init__(self, host, display="wayland-1"):
        self.host = host
        self.display = display

    def run(self, command, *, stdin=None, check=True, timeout=600):
        result = subprocess.run(
            ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", self.host, command],
            input=stdin,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
        if check and result.returncode != 0:
            raise RuntimeError(
                f"{command!r} failed on {self.host} with {result.returncode}: "
                f"{result.stderr.decode(errors='replace').strip()}"
            )
        return result.stdout

    def session(self, command, **kwargs):
        """Runs COMMAND as the session user, with the session's compositor and bus."""
        prefix = (
            "export XDG_RUNTIME_DIR=/run/user/$(id -u) "
            f"WAYLAND_DISPLAY={shlex.quote(self.display)} "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus; "
        )
        return self.run(prefix + command, **kwargs)

    def helper(self, path, *args, root=False, session=False, **kwargs):
        command = ("sudo -n " if root else "") + "python3 - " + " ".join(
            shlex.quote(str(arg)) for arg in args
        )
        runner = self.session if session else self.run
        return runner(command, stdin=pathlib.Path(path).read_bytes(), **kwargs)

    def wall(self):
        return int(self.run("date +%s"))

    def journal(self, since, units=UNITS):
        flags = " ".join(f"-u {unit}" for unit in units)
        out = self.session(f"journalctl --user {flags} --since @{since} -o json --no-pager")
        return [json.loads(line) for line in out.splitlines() if line.strip()]

    def screenshot(self):
        return self.session("grim -t ppm -")

    def systemctl(self, *args):
        return self.session("systemctl --user " + " ".join(shlex.quote(a) for a in args))

    def main_pid(self, unit):
        return int(self.systemctl("show", "-p", "MainPID", "--value", unit))
```

- [ ] **Step 3: Check the connection**

Run: `python3 -c "import sys; sys.path.insert(0, 'scripts/shell-bench'); import machine; m = machine.Machine('athanor-ref'); print(m.run('id -un; sudo -n true && echo sudo-ok').decode()); print(m.systemctl('is-active', *machine.UNITS).decode())"`
Expected: the user name, `sudo-ok`, and `active` three times.

- [ ] **Step 4: Read the facts that resolve open doubt 1**

Run:

```bash
ssh athanor-ref 'grep -m1 "model name" /proc/cpuinfo; grep MemTotal /proc/meminfo; lspci -nn | grep -E "VGA|3D"; uname -r; sudo -n bootc status --json | python3 -c "import json,sys; b=json.load(sys.stdin)[\"status\"][\"booted\"][\"image\"]; print(b[\"image\"][\"image\"], b[\"imageDigest\"])"; for f in /sys/class/drm/card*-eDP-*/modes; do echo "$f: $(head -1 $f)"; done'
```

Expected: i7-8550U, about 8 GB, the UHD 620 line (and an NVIDIA MX150 line if this unit has one), the default image (`athanor-system`, not `-nvidia`), a 1920x1080 mode. The refresh rate is read in Task 4 from the frame timings (`refresh_us`).

- [ ] **Step 5: Write `docs/shell-bench/machine.md`** with those values, one line each (CPU, memory, GPUs and which one drives the panel, panel mode, image and digest, kernel, date), and a line "Refresh rate: see Task 4". If a value contradicts ST4 (no x86-64-v3, the `-nvidia` image, more than 8 GB), stop and report to the maintainer.

- [ ] **Step 6: Write `scripts/shell-bench/README.md`**: the prerequisites of Step 1, the host alias, and the commands of Tasks 6-8 (`bench.py`, `soak.py`) with their flags. Plain English, no more than 60 lines.

- [ ] **Step 7: Commit**

```bash
git add scripts/shell-bench/machine.py scripts/shell-bench/README.md docs/shell-bench/machine.md
git commit -m "feat(shell-bench): reach the reference machine and record its facts"
```

---

### Task 2: Spikes Q1 (capture) and Q2 (input)

**Files:**

- Create: `scripts/shell-bench/machine/inject.py`, `scripts/shell-bench/machine/tree.py`, `docs/shell-bench/spikes.md`

**Interfaces:**

- Consumes: `Machine` (Task 1).
- Produces: `inject.py`, run as `sudo -n python3 - WIDTH HEIGHT OP... < inject.py`, OPs `move X Y`, `press [right]`, `release [right]`, `click`, `rclick`, `key CODE`, `wait MS`; prints one JSON line `{"op": OP, "t_ns": T}` per input OP, T on `CLOCK_MONOTONIC`. `tree.py`, run as `python3 - APP < tree.py` in the session; prints JSON lines `{"toplevel": I, "name", "w", "h"}` for each window of APP and `{"window": I, "name", "role", "popup", "x", "y", "w", "h"}` for each showing node, extents in window coordinates.

- [ ] **Step 1: Q1, capture.** Run: `ssh athanor-ref 'export XDG_RUNTIME_DIR=/run/user/$(id -u); ls $XDG_RUNTIME_DIR | grep -E "^wayland-[0-9]+$"; WAYLAND_DISPLAY=wayland-1 grim -t ppm - | head -c 20 | od -c | head -2'`
      Expected: the session's socket name, and a PPM header `P 6 \n 1 9 2 0 ...`. If the socket is not `wayland-1`, record it: `Machine(display=...)` takes it. If grim answers "compositor doesn't support the screen capture protocol", run `WAYLAND_DEBUG=1 wayland-info 2>/dev/null | grep -E "image_copy|screencopy|image_capture_source"` (or `grep interface` on the `WAYLAND_DEBUG` output of `grim`) to list what cosmic-comp offers to a client on that socket, write it in `spikes.md`, and **stop the plan**: Q1 failed, ST9 needs a revision.

- [ ] **Step 2: Write `scripts/shell-bench/machine/inject.py`**

```python
"""inject.py: run as root on the reference machine by the shell bench, as
`sudo -n python3 - WIDTH HEIGHT OP... < inject.py`.

Gives input through two throwaway uinput devices, an absolute pointer and a keyboard, which
the compositor reads through libinput like real ones (doc_shell_standard.md, spike Q2). The
pointer's range is WIDTH x HEIGHT, mapped onto the one output. Each OP is one argument:
`move X Y`, `press`, `press right`, `release`, `release right`, `click`, `rclick`,
`key CODE` (an evdev key code: 1 is Escape), `wait MS`. After each OP that sends input it
prints {"op": OP, "t_ns": T}, T being CLOCK_MONOTONIC right after the event's SYN_REPORT:
the clock of GTK's frame timings (spike Q3). Modelled on scripts/devvm/pointer_click.py.
"""

import fcntl
import json
import os
import struct
import sys
import time

EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
SYN_REPORT, BTN_LEFT, BTN_RIGHT, ABS_X, ABS_Y = 0, 0x110, 0x111, 0, 1
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT = 0x40045564, 0x40045565, 0x40045567
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
ABS_CNT, BUS_VIRTUAL = 64, 0x06
# KEY_ESC to KEY_MICMUTE: enough keys for libinput to class the device as a keyboard.
KEYS = range(1, 249)
# How long the compositor takes to add a new input device.
SETTLE_S = 1.5


def device(name, keys, width=0, height=0):
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    for kind in (EV_SYN, EV_KEY, EV_ABS) if width else (EV_SYN, EV_KEY):
        fcntl.ioctl(fd, UI_SET_EVBIT, kind)
    for key in keys:
        fcntl.ioctl(fd, UI_SET_KEYBIT, key)
    absmax = [0] * ABS_CNT
    if width:
        for axis in (ABS_X, ABS_Y):
            fcntl.ioctl(fd, UI_SET_ABSBIT, axis)
        absmax[ABS_X], absmax[ABS_Y] = width - 1, height - 1
    zeros = [0] * ABS_CNT
    # struct uinput_user_dev: name, input_id, ff_effects_max, absmax, absmin, absfuzz, absflat
    os.write(
        fd,
        struct.pack(
            f"80sHHHHi{4 * ABS_CNT}i", name, BUS_VIRTUAL, 0, 0, 1, 0, *absmax, *zeros, *zeros, *zeros
        ),
    )
    fcntl.ioctl(fd, UI_DEV_CREATE)
    return fd


def send(fd, *events):
    for kind, code, value in (*events, (EV_SYN, SYN_REPORT, 0)):
        os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


def main():
    width, height = int(sys.argv[1]), int(sys.argv[2])
    ops = [arg.split() for arg in sys.argv[3:]]
    pointer = device(b"athanor-bench-pointer", (BTN_LEFT, BTN_RIGHT), width, height)
    keyboard = device(b"athanor-bench-keyboard", KEYS)
    try:
        time.sleep(SETTLE_S)
        for op in ops:
            t_ns = None
            match op:
                case ["move", x, y]:
                    if not (0 <= int(x) < width and 0 <= int(y) < height):
                        raise SystemExit(f"{x},{y} lies outside the {width}x{height} output")
                    t_ns = send(pointer, (EV_ABS, ABS_X, int(x)), (EV_ABS, ABS_Y, int(y)))
                case ["press" | "release" as what, *side] if side in ([], ["right"]):
                    button = BTN_RIGHT if side else BTN_LEFT
                    t_ns = send(pointer, (EV_KEY, button, int(what == "press")))
                case ["click" | "rclick" as what]:
                    button = BTN_LEFT if what == "click" else BTN_RIGHT
                    t_ns = send(pointer, (EV_KEY, button, 1))
                    time.sleep(0.05)
                    send(pointer, (EV_KEY, button, 0))
                case ["key", code]:
                    t_ns = send(keyboard, (EV_KEY, int(code), 1))
                    time.sleep(0.03)
                    send(keyboard, (EV_KEY, int(code), 0))
                case ["wait", ms]:
                    time.sleep(int(ms) / 1000)
                case _:
                    raise SystemExit(f"unknown op {' '.join(op)!r}")
            if t_ns is not None:
                print(json.dumps({"op": " ".join(op), "t_ns": t_ns}), flush=True)
    finally:
        for fd in (pointer, keyboard):
            fcntl.ioctl(fd, UI_DEV_DESTROY)
            os.close(fd)
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 3: Write `scripts/shell-bench/machine/tree.py`**

```python
"""tree.py: run in the session on the reference machine by the shell bench, as
`python3 - APP < tree.py`.

Prints APP's accessibility tree as JSON lines: one {"toplevel": I, "name", "w", "h"} per
window, then one {"window": I, "name", "role", "popup", "x", "y", "w", "h"} per showing
node, extents in the coordinates of window I. Wayland gives a client no screen
coordinates; the bench adds the surface's origin, which the program logs (timing.rs).
Modelled on scripts/devvm/dock_press.py. Exits 1 when APP is not on the bus.
"""

import json
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402


def main():
    desktop = Atspi.get_desktop(0)
    apps = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    app = next((a for a in apps if a is not None and a.get_name() == sys.argv[1]), None)
    if app is None:
        print(f"no application {sys.argv[1]!r} on the accessibility bus", file=sys.stderr)
        return 1
    for index in range(app.get_child_count()):
        window = app.get_child_at_index(index)
        if window is None:
            continue
        extents = window.get_extents(Atspi.CoordType.WINDOW)
        print(json.dumps({"toplevel": index, "name": window.get_name(),
                          "w": extents.width, "h": extents.height}))
        pending = [window.get_child_at_index(i) for i in range(window.get_child_count())]
        while pending:
            node = pending.pop()
            if node is None:
                continue
            try:
                states = node.get_state_set()
                if states.contains(Atspi.StateType.SHOWING):
                    box = node.get_extents(Atspi.CoordType.WINDOW)
                    print(json.dumps({
                        "window": index, "name": node.get_name(), "role": node.get_role_name(),
                        "popup": states.contains(Atspi.StateType.HAS_POPUP),
                        "x": box.x, "y": box.y, "w": box.width, "h": box.height,
                    }))
                pending.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
            except GLib.Error:
                # Destroyed while walked: a rebuild replaced it.
                continue
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Q2, input.** Run, with the desktop empty:

```bash
python3 - <<'EOF'
import sys; sys.path.insert(0, "scripts/shell-bench")
import machine
m = machine.Machine("athanor-ref")
print(m.helper("scripts/shell-bench/machine/inject.py", 1920, 1080, "move 960 540", "wait 500", "key 125", "wait 1500", "key 1", "wait 500", root=True).decode())
print(m.helper("scripts/shell-bench/machine/tree.py", "athanor-bar", session=True).decode()[:2000])
print(m.helper("scripts/shell-bench/machine/tree.py", "athanor-dock", session=True).decode()[:2000])
EOF
```

Expected: three JSON lines with increasing `t_ns`; the maintainer confirms (or a `grim` shot taken between the two keys shows) that Super opened the launcher and Escape closed it; both trees list the bar's buttons with `"popup": true` on the status modules and the dock's application buttons. If the application names on the bus are not the unit names, record the real names in `spikes.md` and use them in Task 6. If the pointer does not move or the keys do nothing, **stop the plan**: Q2 failed.

- [ ] **Step 5: Escape closes popovers.** Using the tree's extents and, for now, the surface geometry measured by hand from a screenshot, click one bar status button and right-click one dock application button, then send `key 1`; take a screenshot after each. Record in `spikes.md` whether Escape closes each popover. If it does not, the response stage closes by clicking the same button again (bar) and by clicking an empty point of the desktop (dock): write which in `spikes.md`, Task 6 reads it.

- [ ] **Step 6: Write `docs/shell-bench/spikes.md`** with a section per spike: question (copied from the spec's table), what was run, the answer, and "passed" or "failed". Q3 and Q4 come in Task 4.

- [ ] **Step 7: Commit**

```bash
git add scripts/shell-bench/machine docs/shell-bench/spikes.md
git commit -m "feat(shell-bench): give input and read the accessibility tree on the reference machine"
```

---

### Task 3: Timing instrumentation in the bar and the dock

**Files:**

- Create: `system/athanor-apps/src/timing.rs`
- Modify: `system/athanor-apps/src/lib.rs` (add `pub mod timing;`), `system/athanor-apps/Cargo.toml` (add `gtk4-layer-shell = { workspace = true }`), `system/athanor-apps/src/menu.rs` (in `attach_popover`), `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs:457`, `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popups.rs:39`, `forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/surface.rs:113`

**Interfaces:**

- Produces: `athanor_apps::timing::{VARIABLE, enabled() -> bool, watch(native: &impl IsA<gtk4::Widget>, surface: &'static str), watch_layer(window: &gtk4::ApplicationWindow, surface: &'static str)}`. Journal lines (the message contains):
  - `{"bench":"frame","surface":S,"frame":N,"presented_us":P,"predicted_us":Q,"refresh_us":R}` — P is 0 when the compositor gave no presentation time;
  - `{"bench":"surface","surface":S,"x":X,"y":Y,"w":W,"h":H,"output_w":OW,"output_h":OH}` — logical pixels, origin relative to the output, logged when it changes.
- Surface names: `"bar"`, `"bar-popups"`, `"dock"`, `"popover"`.

- [ ] **Step 1: Write the failing tests** at the bottom of the new `system/athanor-apps/src/timing.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_one_line_of_json() {
        assert_eq!(
            frame_line("dock", 42, 1_000_016, 1_000_000, 16_666),
            r#"{"bench":"frame","surface":"dock","frame":42,"presented_us":1000016,"predicted_us":1000000,"refresh_us":16666}"#
        );
    }

    #[test]
    fn a_placement_is_one_line_of_json() {
        assert_eq!(
            surface_line("bar", [0, 1032, 1920, 48, 1920, 1080]),
            r#"{"bench":"surface","surface":"bar","x":0,"y":1032,"w":1920,"h":48,"output_w":1920,"output_h":1080}"#
        );
    }

    #[test]
    fn the_origin_follows_the_anchors() {
        // Anchored at the start: its margin. At the end only: from the far side.
        assert_eq!(origin(Some(8), None, 1920, 600), 8);
        assert_eq!(origin(None, Some(8), 1080, 64), 1080 - 64 - 8);
        // Both: stretched, so the start margin. Neither: centred.
        assert_eq!(origin(Some(0), Some(0), 1920, 1920), 0);
        assert_eq!(origin(None, None, 1920, 600), 660);
    }

    #[test]
    fn a_frame_is_dropped_after_its_last_flush() {
        let mut pending = vec![(1, 0), (2, FLUSHES - 1)];
        // Neither is complete: the first waits, the second has had its last chance.
        settle(&mut pending, 0, |_| None);
        assert_eq!(pending, vec![(1, 1)]);
        // Complete frames leave and are returned; frames older than the history are lost.
        let mut pending = vec![(1, 0), (5, 0), (6, 0)];
        let done = settle(&mut pending, 2, |frame| (frame == 5).then_some(frame));
        assert_eq!(done, vec![5]);
        assert_eq!(pending, vec![(6, 1)]);
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-apps timing`
Expected: compile errors, `frame_line`, `surface_line`, `origin`, `settle` not found. (athanor-apps builds against GTK: if the host lacks the GTK development files, run it through `forge/test/shell/rig.sh cargo test -p athanor-apps timing`, as the other GTK crates do.)

- [ ] **Step 3: Write the module** above the tests:

```rust
//! Frame timing for the shell bench (doc_shell_standard.md, ST9). With `ATHANOR_SHELL_BENCH`
//! set to a non-empty value, every frame a watched surface presents is logged as one line
//! of JSON carrying the presentation time GTK's frame clock got from the compositor, in
//! microseconds of `CLOCK_MONOTONIC`, the clock the bench stamps its input with. A layer
//! surface also logs where it lies on its output, which Wayland never tells a client and
//! the bench needs to aim its pointer. Without the variable nothing is connected.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, LayerShell};

pub const VARIABLE: &str = "ATHANOR_SHELL_BENCH";
/// Presentation feedback arrives after the frame is painted: pending frames are looked at
/// this often, and dropped after `FLUSHES` looks (the compositor discarded them, or the
/// surface went away).
const FLUSH: Duration = Duration::from_millis(250);
const FLUSHES: u8 = 8;

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os(VARIABLE).is_some_and(|value| !value.is_empty()))
}

/// Logs the frames `native` presents, under the name `surface`. `native` is a widget with
/// its own surface (a window or a popover).
pub fn watch(native: &impl IsA<gtk4::Widget>, surface: &'static str) {
    if enabled() {
        attach(native.upcast_ref(), surface, None);
    }
}

/// As [`watch`], and logs where the layer surface `window` lies on its output.
pub fn watch_layer(window: &gtk4::ApplicationWindow, surface: &'static str) {
    if enabled() {
        attach(window.upcast_ref(), surface, Some(window.downgrade()));
    }
}

type Pending = Rc<RefCell<Vec<(i64, u8)>>>;

/// Follows the frame clock of each realization of `widget`: a popover is realized again
/// every time it is shown, and must not count its frames twice.
fn attach(widget: &gtk4::Widget, surface: &'static str, layer: Option<glib::WeakRef<gtk4::ApplicationWindow>>) {
    let current: Rc<RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>> = Rc::default();
    let slot = current.clone();
    widget.connect_realize(move |widget| {
        if let Some(clock) = widget.frame_clock() {
            let id = follow(&clock, surface, layer.clone());
            if let Some((old, old_id)) = slot.replace(Some((clock, id))) {
                old.disconnect(old_id);
            }
        }
    });
    widget.connect_unrealize(move |_| {
        if let Some((clock, id)) = current.take() {
            clock.disconnect(id);
        }
    });
}

fn follow(
    clock: &gdk::FrameClock,
    surface: &'static str,
    layer: Option<glib::WeakRef<gtk4::ApplicationWindow>>,
) -> glib::SignalHandlerId {
    let pending: Pending = Rc::default();
    let scheduled = Rc::new(Cell::new(false));
    let placed = Cell::new(None::<[i32; 6]>);
    clock.connect_after_paint(move |clock| {
        pending.borrow_mut().push((clock.frame_counter(), 0));
        if let Some(rect) = layer.as_ref().and_then(glib::WeakRef::upgrade).and_then(|w| geometry(&w)) {
            if placed.replace(Some(rect)) != Some(rect) {
                tracing::info!("{}", surface_line(surface, rect));
            }
        }
        schedule(clock, surface, &pending, &scheduled);
    })
}

fn schedule(clock: &gdk::FrameClock, surface: &'static str, pending: &Pending, scheduled: &Rc<Cell<bool>>) {
    if scheduled.replace(true) {
        return;
    }
    let (clock, pending, scheduled) = (clock.clone(), pending.clone(), scheduled.clone());
    glib::timeout_add_local_once(FLUSH, move || {
        scheduled.set(false);
        let done = settle(&mut pending.borrow_mut(), clock.history_start(), |frame| {
            clock.timings(frame).filter(gdk::FrameTimings::is_complete)
        });
        for timings in done {
            tracing::info!(
                "{}",
                frame_line(
                    surface,
                    timings.frame_counter(),
                    timings.presentation_time(),
                    timings.predicted_presentation_time(),
                    timings.refresh_interval(),
                )
            );
        }
        if !pending.borrow().is_empty() {
            schedule(&clock, surface, &pending, &scheduled);
        }
    });
}

/// Takes the complete frames out of `pending`, oldest first; drops the frames GDK no longer
/// remembers and those that had their last flush; counts a flush on the rest.
fn settle<T>(pending: &mut Vec<(i64, u8)>, history_start: i64, complete: impl Fn(i64) -> Option<T>) -> Vec<T> {
    let mut done = Vec::new();
    pending.retain_mut(|(frame, flushes)| {
        if *frame < history_start {
            return false;
        }
        if let Some(timings) = complete(*frame) {
            done.push(timings);
            return false;
        }
        *flushes += 1;
        *flushes < FLUSHES
    });
    done
}

/// Where `window` lies on its output: x, y, width, height, output width, output height.
fn geometry(window: &gtk4::ApplicationWindow) -> Option<[i32; 6]> {
    let output = LayerShell::monitor(window)?.geometry();
    let (w, h) = (window.width(), window.height());
    let anchored = |edge| window.is_anchor(edge).then(|| window.margin(edge));
    Some([
        origin(anchored(Edge::Left), anchored(Edge::Right), output.width(), w),
        origin(anchored(Edge::Top), anchored(Edge::Bottom), output.height(), h),
        w,
        h,
        output.width(),
        output.height(),
    ])
}

/// The origin of a layer surface along one axis: `start` and `end` are the margins of the
/// edges it is anchored to.
fn origin(start: Option<i32>, end: Option<i32>, output: i32, len: i32) -> i32 {
    match (start, end) {
        (Some(margin), _) => margin,
        (None, Some(margin)) => output - len - margin,
        (None, None) => (output - len) / 2,
    }
}

fn frame_line(surface: &str, frame: i64, presented: i64, predicted: i64, refresh: i64) -> String {
    format!(
        r#"{{"bench":"frame","surface":"{surface}","frame":{frame},"presented_us":{presented},"predicted_us":{predicted},"refresh_us":{refresh}}}"#
    )
}

fn surface_line(surface: &str, [x, y, w, h, ow, oh]: [i32; 6]) -> String {
    format!(
        r#"{{"bench":"surface","surface":"{surface}","x":{x},"y":{y},"w":{w},"h":{h},"output_w":{ow},"output_h":{oh}}}"#
    )
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-apps timing`
Expected: 4 passed.

- [ ] **Step 5: Hook the surfaces.**
  - `system/athanor-apps/src/menu.rs`, in `attach_popover`, after `popover.set_position(position);`: `crate::timing::watch(popover, "popover");`
  - `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/mod.rs`, right after `let window = gtk4::ApplicationWindow::new(&self.app);` (line 457): `athanor_apps::timing::watch_layer(&window, "bar");`
  - `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/popups.rs`, after `window.init_layer_shell();` (line 40): `athanor_apps::timing::watch_layer(&window, "bar-popups");`
  - `forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/surface.rs`, right after `let window = gtk4::ApplicationWindow::new(&dock.app);` (line 113): `athanor_apps::timing::watch_layer(&window, "dock");`

- [ ] **Step 6: Build and run every test of the three crates**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-apps -p athanor-bar -p athanor-dock` (through `forge/test/shell/rig.sh cargo ...` if the host lacks GTK), then `just lint`.
Expected: all pass, no new clippy warning.

- [ ] **Step 7: Commit**

```bash
git add system/athanor-apps forge/specs/athanor-bar forge/specs/athanor-dock
git commit -m "feat(shell): log presented frames and surface placement for the shell bench"
```

---

### Task 4: Spikes Q3 (presentation time) and Q4 (overlay)

**Files:**

- Modify: `docs/shell-bench/spikes.md`, `docs/shell-bench/machine.md`

**Interfaces:**

- Consumes: Tasks 1-3.
- Produces: the bar and dock packages of this branch on the machine's transient overlay, for Tasks 6-7 and 11-12; the answers Q3 and Q4.

- [ ] **Step 1: Build the packages of this branch**

Run: `just rpms "athanor-bar athanor-dock"` (ask the maintainer first if it is the first build of the day: it is long). Expected: two RPMs under the forge output directory; note their paths.

- [ ] **Step 2: Q4, install them on the overlay**

Run:

```bash
scp <bar.rpm> <dock.rpm> athanor-ref:/var/tmp/
ssh athanor-ref 'sudo -n bootc usr-overlay && sudo -n rpm -Uvh --replacepkgs /var/tmp/athanor-bar-*.rpm /var/tmp/athanor-dock-*.rpm && systemctl --user restart athanor-bar athanor-dock && sleep 3 && systemctl --user is-active athanor-bar athanor-dock && journalctl --user -u athanor-bar -u athanor-dock --since -1min -p warning --no-pager && sudo -n ausearch -m AVC,INTEGRITY_DATA,INTEGRITY_RULE -ts recent 2>&1 | tail -20'
```

Expected: `active` twice, no warning lines from the units, `<no matches>` from ausearch. A denial or an IMA appraisal failure means Q4 failed: record it and **stop the plan**.

- [ ] **Step 3: Q3, presentation time.** Run:

```bash
ssh athanor-ref 'systemctl --user set-environment ATHANOR_SHELL_BENCH=1 WAYLAND_DEBUG=1 && systemctl --user restart athanor-dock && sleep 3 && systemctl --user unset-environment WAYLAND_DEBUG && journalctl --user -u athanor-dock --since -1min --no-pager -o cat | grep -m3 -E "wp_presentation.*clock_id|\"bench\":\"(frame|surface)\""'
```

Then the response check: with `inject.py`, right-click the dock's first application button 10 times with Escape between (positions from Task 2), and read the frame lines of surface `popover`.
Expected: `wp_presentation` announces `clock_id 1` (`CLOCK_MONOTONIC`); frame lines carry `presented_us` non-zero and different from `predicted_us`; each right-click's `t_ns / 1000` is smaller than the next popover frame's `presented_us` by a plausible 5-200 ms. `refresh_us` gives the panel's refresh rate: write it in `machine.md`. If `presented_us` is always 0 or the clock is not monotonic, **stop the plan**: Q3 failed.

- [ ] **Step 4: Q4, the reboot discards the overlay.** Run: `ssh athanor-ref 'systemctl --user unset-environment ATHANOR_SHELL_BENCH; sudo -n systemctl reboot'`, wait for SSH, then `ssh athanor-ref 'rpm -q athanor-bar athanor-dock; findmnt /usr -o OPTIONS -n'`.
      Expected: the image's versions again and `/usr` read-only. Record in `spikes.md`.

- [ ] **Step 5: Commit**

```bash
git add docs/shell-bench
git commit -m "docs(shell-bench): record spikes Q3 and Q4 and the panel's refresh rate"
```

---

### Task 5: Analysis core

**Files:**

- Create: `scripts/shell-bench/analysis.py`, `scripts/tests/test_shell_bench.py`

**Interfaces:**

- Produces: `THRESHOLDS`, `REPETITIONS = 50`, `NO_FRAME_S = 2.0`; `Frame(unit, surface, pid, frame, presented_ns, refresh_ns)`; `Placement(unit, surface, x, y, w, h, output_w, output_h)`; `Target(unit, surface, name, popup, x, y, output_w, output_h)`; `parse_journal(entries) -> (list[Frame], list[Placement])`; `targets(placement, nodes) -> list[Target]`; `responses_ms(times_ns, frames, unit, surface) -> list[float | None]`; `percentile(values, p) -> float`; `smoothness(frames, windows_ns, unit, surface) -> (float | None, float, int)`; `settled(times_ns, frames, unit, surface, limit_ns, horizon_ns) -> list[bool]`; `cpu_ticks(stat) -> int`; `pss_kb(smaps_rollup) -> int`; `read_ppm(data) -> (w, h, bytes)`; `changed(a, b, rows, tolerance=24) -> (int, tuple | None)`; `normalise(names) -> list`; `verdict(value, limit, *, at_least=False) -> dict`.

- [ ] **Step 1: Write the failing tests** in `scripts/tests/test_shell_bench.py`:

```python
"""Unit tests of scripts/shell-bench/analysis.py (python3 -B -m unittest discover -s scripts/tests -v)."""

import json
import math
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "shell-bench"))
import analysis  # noqa: E402

MS = 1_000_000


def entry(unit, record, pid=7):
    return {"_SYSTEMD_USER_UNIT": f"{unit}.service", "_PID": str(pid),
            "MESSAGE": "INFO athanor_apps::timing: " + json.dumps(record)}


def frame(unit, surface, presented_ms, refresh_ms=16.666):
    return analysis.Frame(unit, surface, 7, 0, int(presented_ms * MS), int(refresh_ms * MS))


class Parse(unittest.TestCase):
    def test_frames_and_placements_come_out_of_the_journal(self):
        frames, placements = analysis.parse_journal([
            entry("athanor-dock", {"bench": "frame", "surface": "dock", "frame": 3,
                                   "presented_us": 2000, "predicted_us": 1990, "refresh_us": 16666}),
            entry("athanor-dock", {"bench": "surface", "surface": "dock", "x": 660, "y": 1016,
                                   "w": 600, "h": 64, "output_w": 1920, "output_h": 1080}),
            {"_SYSTEMD_USER_UNIT": "athanor-bar.service", "MESSAGE": "something else"},
            {"_SYSTEMD_USER_UNIT": "athanor-bar.service", "MESSAGE": [255, 0]},
        ])
        self.assertEqual(frames, [analysis.Frame("athanor-dock", "dock", 7, 3, 2_000_000, 16_666_000)])
        self.assertEqual(placements, [analysis.Placement("athanor-dock", "dock", 660, 1016, 600, 64, 1920, 1080)])

    def test_a_frame_without_presentation_time_is_left_out(self):
        frames, _ = analysis.parse_journal([entry("athanor-bar", {
            "bench": "frame", "surface": "bar", "frame": 1, "presented_us": 0,
            "predicted_us": 5, "refresh_us": 16666})])
        self.assertEqual(frames, [])


class Targets(unittest.TestCase):
    PLACE = analysis.Placement("athanor-dock", "dock", 660, 1016, 600, 64, 1920, 1080)

    def test_buttons_of_the_matching_window_are_aimed_at_their_centre(self):
        nodes = [
            {"toplevel": 0, "name": "", "w": 600, "h": 64},
            {"window": 0, "name": "Files", "role": "push button", "popup": True, "x": 10, "y": 8, "w": 48, "h": 48},
            {"window": 0, "name": "", "role": "filler", "popup": False, "x": 0, "y": 0, "w": 600, "h": 64},
        ]
        self.assertEqual(analysis.targets(self.PLACE, nodes), [
            analysis.Target("athanor-dock", "dock", "Files", True, 660 + 34, 1016 + 32, 1920, 1080)])

    def test_two_windows_of_the_same_size_are_refused(self):
        nodes = [{"toplevel": 0, "name": "", "w": 600, "h": 64}, {"toplevel": 1, "name": "", "w": 600, "h": 64}]
        with self.assertRaisesRegex(RuntimeError, "2 windows"):
            analysis.targets(self.PLACE, nodes)


class Response(unittest.TestCase):
    def test_the_first_frame_of_the_surface_after_the_input_counts(self):
        frames = [frame("athanor-bar", "popover", 90), frame("athanor-bar", "bar", 105),
                  frame("athanor-bar", "popover", 130), frame("athanor-bar", "popover", 147)]
        self.assertEqual(analysis.responses_ms([100 * MS], frames, "athanor-bar", "popover"), [30.0])

    def test_no_frame_within_two_seconds_is_a_miss(self):
        frames = [frame("athanor-bar", "popover", 2200)]
        self.assertEqual(analysis.responses_ms([100 * MS], frames, "athanor-bar", "popover"), [None])

    def test_a_miss_is_infinitely_slow_in_the_percentile(self):
        self.assertEqual(analysis.percentile([10.0] * 49 + [None], 95), 10.0)
        self.assertEqual(analysis.percentile([10.0] * 45 + [None] * 5, 95), math.inf)

    def test_percentile_is_nearest_rank(self):
        self.assertEqual(analysis.percentile(list(range(1, 101)), 95), 95)


class Smoothness(unittest.TestCase):
    def test_late_frames_inside_an_animation_are_counted(self):
        times = [0, 16.7, 33.3, 50, 100, 116.7]  # one gap of three refresh intervals
        frames = [frame("athanor-dock", "dock", t) for t in times]
        on_time, worst, measured = analysis.smoothness(frames, [(0, 200 * MS)], "athanor-dock", "dock")
        self.assertEqual(measured, 1)
        self.assertAlmostEqual(on_time, 4 / 5)
        self.assertAlmostEqual(worst, 3.0, places=2)

    def test_a_window_with_few_frames_is_no_animation(self):
        frames = [frame("athanor-dock", "dock", 10), frame("athanor-dock", "dock", 400)]
        self.assertEqual(analysis.smoothness(frames, [(0, 1000 * MS)], "athanor-dock", "dock"), (None, 0.0, 0))


class Settled(unittest.TestCase):
    def test_the_last_frame_before_the_horizon_must_come_within_the_limit(self):
        frames = [frame("athanor-dock", "dock", t) for t in (1010, 2005, 4300)]
        ok = analysis.settled([0, 2000 * MS, 3000 * MS], frames, "athanor-dock", "dock",
                              limit_ns=1200 * MS, horizon_ns=1700 * MS)
        # 0: last frame 1010 within 1200. 2000: last 2005, fine. 3000: last 4300 is past 4200.
        self.assertEqual(ok, [True, True, False])


class Kernel(unittest.TestCase):
    def test_cpu_ticks_survive_a_command_with_spaces(self):
        stat = "4242 (athanor bar) S 1 2 3 4 5 6 7 8 9 10 120 30 0 0 20 0 1 0 100"
        self.assertEqual(analysis.cpu_ticks(stat), 150)

    def test_pss(self):
        self.assertEqual(analysis.pss_kb("Rss:  9000 kB\nPss:  4096 kB\n"), 4096)


class Pixels(unittest.TestCase):
    @staticmethod
    def ppm(w, h, pixels):
        return f"P6\n{w} {h}\n255\n".encode() + bytes(pixels)

    def test_a_leading_whitespace_byte_is_pixel_data(self):
        w, h, data = analysis.read_ppm(self.ppm(1, 1, [32, 10, 9]))
        self.assertEqual((w, h, data), (1, 1, bytes([32, 10, 9])))

    def test_changed_pixels_and_their_box(self):
        a = self.ppm(4, 2, [0] * 24)
        b = self.ppm(4, 2, [0] * 3 + [200, 0, 0] + [0] * 12 + [0, 0, 100] + [0] * 3)
        self.assertEqual(analysis.changed(a, b, (0, 2)), (2, (1, 0, 2, 1)))
        self.assertEqual(analysis.changed(a, b, (0, 1)), (1, (1, 0, 1, 0)))

    def test_names_ignore_digits(self):
        self.assertEqual(analysis.normalise([("label", "10:41"), ("push button", "Files")]),
                         [("label", "#:#"), ("push button", "Files")])


class Verdict(unittest.TestCase):
    def test_not_measured_fails(self):
        self.assertEqual(analysis.verdict(None, 50), {"value": None, "limit": 50, "pass": False})

    def test_limits(self):
        self.assertTrue(analysis.verdict(49.9, 50)["pass"])
        self.assertFalse(analysis.verdict(0.98, 0.99, at_least=True)["pass"])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p 'test_shell_bench.py' -v`
Expected: `ModuleNotFoundError: No module named 'analysis'`.

- [ ] **Step 3: Write `scripts/shell-bench/analysis.py`**

```python
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
        if n.get("window") == windows[0] and n["role"] in ("push button", "toggle button") and n["w"] > 0
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
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p 'test_shell_bench.py' -v`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/shell-bench/analysis.py scripts/tests/test_shell_bench.py
git commit -m "feat(shell-bench): parse frame timings and judge them against the thresholds"
```

---

### Task 6: Bench stages and results

**Files:**

- Create: `scripts/shell-bench/bench.py`
- Modify: `scripts/tests/test_shell_bench.py` (one test of the restore)

**Interfaces:**

- Consumes: `Machine`, `analysis.*`, `inject.py`, `tree.py`.
- Produces: `bench.py --host H [--stages a,b] [--repeat N] [--commit C] [--out DIR]`; stages `facts`, `response`, `idle`, `memory`, `recovery`, `start`, `scenarios`; `bench.instrumented(machine)` context manager; `bench.inject(machine, width, height, ops) -> list[(op, t_ns)]`; `bench.locate(machine, since) -> list[Target]`; `results.json` and `report.md` in `docs/shell-bench/<date>-<commit>/`.

- [ ] **Step 1: Write the failing test** of the restore (append to `scripts/tests/test_shell_bench.py`):

```python
import bench  # noqa: E402


class Restore(unittest.TestCase):
    def test_a_raising_stage_still_unsets_the_variable(self):
        calls = []

        class Fake:
            def systemctl(self, *args):
                calls.append(args)
                return b""

            def wall(self):
                return 0

        with self.assertRaises(ZeroDivisionError):
            with bench.instrumented(Fake(), settle_s=0):
                1 / 0
        self.assertEqual(calls[-2:], [("unset-environment", "ATHANOR_SHELL_BENCH"),
                                      ("restart", "athanor-bar", "athanor-dock")])
```

- [ ] **Step 2: Run it to see it fail**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p 'test_shell_bench.py' -v`
Expected: `ModuleNotFoundError: No module named 'bench'`.

- [ ] **Step 3: Write `scripts/shell-bench/bench.py`**

```python
#!/usr/bin/env python3
"""The shell bench (doc_shell_standard.md, ST9): measures the bar and the dock on the
reference machine against ST5 and writes the results under docs/shell-bench/.

    python3 scripts/shell-bench/bench.py --host athanor-ref [--stages response,idle] [--repeat 50]

Every stage restores what it changed on the machine, even when it fails.
"""

import argparse
import contextlib
import datetime
import json
import pathlib
import re
import subprocess
import sys
import time

import analysis
import machine
from machine import HERE, UNITS

ROOT = HERE.parents[1]
INJECT = HERE / "machine" / "inject.py"
TREE = HERE / "machine" / "tree.py"
GTK_UNITS = ("athanor-bar", "athanor-dock")
SURFACES = {"athanor-bar": "bar", "athanor-dock": "dock"}
ESCAPE = 1
STAGES = ("facts", "response", "idle", "memory", "recovery", "start", "scenarios")


@contextlib.contextmanager
def instrumented(m, settle_s=3):
    """The bar and the dock restarted with frame timing on; off again on the way out."""
    since = m.wall()
    m.systemctl("set-environment", "ATHANOR_SHELL_BENCH=1")
    try:
        m.systemctl("restart", *GTK_UNITS)
        time.sleep(settle_s)
        yield since
    finally:
        m.systemctl("unset-environment", "ATHANOR_SHELL_BENCH")
        m.systemctl("restart", *GTK_UNITS)


def inject(m, width, height, ops):
    out = m.helper(INJECT, width, height, *ops, root=True, timeout=3600)
    return [(r["op"], r["t_ns"]) for r in map(json.loads, out.splitlines())]


def locate(m, since):
    """The buttons of the bar's and the dock's surfaces, in output coordinates."""
    _, placements = analysis.parse_journal(m.journal(since))
    latest = {(p.unit, p.surface): p for p in placements}
    found = []
    for unit, surface in SURFACES.items():
        if (unit, surface) not in latest:
            raise RuntimeError(f"{unit} logged no placement of its {surface} surface")
        nodes = [json.loads(line) for line in m.helper(TREE, unit, session=True).splitlines()]
        found += analysis.targets(latest[(unit, surface)], nodes)
    return found


def stage_facts(m, args):
    status = json.loads(m.run("sudo -n bootc status --json"))["status"]["booted"]["image"]
    return {
        "cpu": m.run("grep -m1 'model name' /proc/cpuinfo").decode().split(":", 1)[1].strip(),
        "memory_kb": int(m.run("grep MemTotal /proc/meminfo").split()[1]),
        "gpus": [l for l in m.run("lspci -nn").decode().splitlines() if re.search(r"VGA|3D", l)],
        "kernel": m.run("uname -r").decode().strip(),
        "image": status["image"]["image"],
        "digest": status["imageDigest"],
    }


def stage_response(m, args):
    """Per popup button: the press redraws the button in place, the release opens the
    popover. Escape closes it (spike record in docs/shell-bench/spikes.md)."""
    results = {}
    with instrumented(m) as since:
        for t in locate(m, since):
            if not t.popup:
                continue  # an opener starts another program: not a surface of ours
            side = " right" if t.unit == "athanor-dock" else ""
            ops = []
            for _ in range(args.repeat):
                ops += [f"move {t.x} {t.y}", "wait 300", "press" + side, "wait 150",
                        "release" + side, "wait 700", f"key {ESCAPE}", "wait 500"]
            injected = inject(m, t.output_w, t.output_h, ops)
            frames, _ = analysis.parse_journal(m.journal(since))
            presses = [ns for op, ns in injected if op.startswith("press")]
            releases = [ns for op, ns in injected if op.startswith("release")]
            in_place = analysis.responses_ms(presses, frames, t.unit, t.surface)
            opening = analysis.responses_ms(releases, frames, t.unit, "popover")
            on_time, worst, measured = analysis.smoothness(
                frames, [(r, r + 10**9) for r in releases], t.unit, "popover")
            results[f"{t.unit}/{t.name}"] = {
                "in_place_p95_ms": analysis.verdict(analysis.percentile(in_place, 95),
                                                     analysis.THRESHOLDS["in_place_ms"]),
                "opening_p95_ms": analysis.verdict(analysis.percentile(opening, 95),
                                                    analysis.THRESHOLDS["opening_ms"]),
                # Review focus 1: a popover left open makes the next release close it, a miss.
                "misses": opening.count(None),
                "smoothness": {
                    "animations": measured,
                    "on_time": analysis.verdict(on_time, analysis.THRESHOLDS["on_time"], at_least=True),
                    "worst_intervals": analysis.verdict(worst, analysis.THRESHOLDS["late_intervals"]),
                },
            }
    return results


def _ticks(m):
    pids = [m.main_pid(unit) for unit in UNITS]
    return sum(analysis.cpu_ticks(m.run(f"cat /proc/{pid}/stat").decode()) for pid in pids), pids


def stage_idle(m, args):
    """Ten minutes with no input and the screen on (the maintainer turned blanking off)."""
    hz = int(m.run("getconf CLK_TCK"))
    before, pids = _ticks(m)
    time.sleep(600)
    after, pids_after = _ticks(m)
    if pids != pids_after:
        return {"cpu": analysis.verdict(None, analysis.THRESHOLDS["idle_cpu"]), "error": "a unit restarted"}
    share = (after - before) / hz / 600
    return {"cpu": analysis.verdict(share, analysis.THRESHOLDS["idle_cpu"])}


def stage_memory(m, args):
    out = {}
    for unit in UNITS:
        kb = analysis.pss_kb(m.run(f"cat /proc/{m.main_pid(unit)}/smaps_rollup").decode())
        out[unit] = analysis.verdict(kb / 1024, analysis.THRESHOLDS["pss_mb"][unit])
    return out


def _names(m, unit):
    nodes = [json.loads(line) for line in m.helper(TREE, unit, session=True).splitlines()]
    return analysis.normalise(sorted((n["role"], n["name"]) for n in nodes if "window" in n))


def stage_recovery(m, args):
    """Three kills per unit, under the five of SH8's give-up."""
    out = {}
    with instrumented(m) as since:
        for unit, surface in SURFACES.items():
            before = _names(m, unit)
            delays = []
            for _ in range(3):
                old = m.main_pid(unit)
                killed = int(m.session(
                    "python3 -c 'import os, sys, time; print(time.clock_gettime_ns(time.CLOCK_MONOTONIC), flush=True); "
                    f"os.execvp(\"systemctl\", [\"systemctl\", \"--user\", \"kill\", \"--kill-whom=main\", \"-s\", \"SIGKILL\", \"{unit}\"])'"
                ).split()[0])
                deadline = time.monotonic() + 10
                while m.main_pid(unit) in (0, old) and time.monotonic() < deadline:
                    time.sleep(0.2)
                time.sleep(3)
                frames, _ = analysis.parse_journal(m.journal(since))
                new = m.main_pid(unit)
                first = [f.presented_ns for f in frames if f.pid == new and f.surface == surface and f.presented_ns > killed]
                delays.append((min(first) - killed) / 1e9 if first else None)
            worst = None if None in delays else max(delays)
            out[unit] = {"recovery_s": analysis.verdict(worst, analysis.THRESHOLDS["recovery_s"]),
                         "same_state": _names(m, unit) == before}
    return out


def stage_start(m, args):
    """From cosmic-comp's start to the first frame of the bar and the dock, at a real login.
    The variable reaches the session through environment.d, removed again at the end."""
    conf = "~/.config/environment.d/90-athanor-shell-bench.conf"
    m.run(f"mkdir -p ~/.config/environment.d && echo ATHANOR_SHELL_BENCH=1 > {conf}")
    try:
        input("Reboot the reference machine, log in on it, then press Enter here. ")
        time.sleep(10)
        hz = int(m.run("getconf CLK_TCK"))
        pid = int(m.run("pgrep -u $(id -u) -x cosmic-comp").split()[0])
        start_ticks = int(m.run(f"cat /proc/{pid}/stat").decode().rsplit(")", 1)[1].split()[19])
        boot, mono = (int(v) for v in m.run(
            "python3 -c 'import time; print(time.clock_gettime_ns(time.CLOCK_BOOTTIME), time.clock_gettime_ns(time.CLOCK_MONOTONIC))'").split())
        started = start_ticks * 10**9 // hz - (boot - mono)
        frames, _ = analysis.parse_journal(m.journal(0))
        out = {}
        for unit, surface in SURFACES.items():
            first = [f.presented_ns for f in frames if f.unit == unit and f.surface == surface and f.presented_ns > started]
            out[unit] = analysis.verdict((min(first) - started) / 1e9 if first else None, analysis.THRESHOLDS["start_s"])
        return out
    finally:
        m.run(f"rm -f {conf}")


def stage_scenarios(m, args):
    import scenarios
    return scenarios.run(m)


def report(results):
    lines = [f"# Shell bench, {results['date']}, {results['commit']}", "",
             f"Image `{results.get('facts', {}).get('image', '?')}` at `{results.get('facts', {}).get('digest', '?')}`.", "",
             "| Stage | Measure | Value | Limit | Pass |", "|---|---|---|---|---|"]

    def walk(stage, prefix, node):
        if isinstance(node, dict) and "pass" in node:
            value = node["value"]
            shown = "not measured" if value is None else f"{value:.4g}" if isinstance(value, float) else value
            lines.append(f"| {stage} | {prefix} | {shown} | {node['limit']} | {'yes' if node['pass'] else '**no**'} |")
        elif isinstance(node, dict):
            for key, child in node.items():
                walk(stage, f"{prefix}/{key}" if prefix else key, child)
        elif isinstance(node, bool):
            lines.append(f"| {stage} | {prefix} | {node} | true | {'yes' if node else '**no**'} |")

    for stage in STAGES[1:]:
        if stage in results:
            walk(stage, "", results[stage])
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--host", required=True)
    parser.add_argument("--display", default="wayland-1")
    parser.add_argument("--stages", default=",".join(STAGES))
    parser.add_argument("--repeat", type=int, default=analysis.REPETITIONS)
    parser.add_argument("--commit", default=subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True, check=True).stdout.strip())
    parser.add_argument("--out", type=pathlib.Path)
    args = parser.parse_args()
    if args.repeat < analysis.REPETITIONS:
        print(f"note: fewer than {analysis.REPETITIONS} repetitions: not a measurement of ST5", file=sys.stderr)
    m = machine.Machine(args.host, args.display)
    date = datetime.date.today().isoformat()
    out = args.out or ROOT / "docs" / "shell-bench" / f"{date}-{args.commit}"
    results = {"date": date, "commit": args.commit, "repeat": args.repeat}
    for stage in args.stages.split(","):
        print(f"stage {stage}", file=sys.stderr)
        results[stage] = globals()[f"stage_{stage}"](m, args)
    out.mkdir(parents=True, exist_ok=True)
    (out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    (out / "report.md").write_text(report(results))
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p 'test_shell_bench.py' -v`
Expected: all pass.

- [ ] **Step 5: Smoke run on the machine** with the overlay of Task 4 reinstalled (the reboot discarded it; repeat Task 4 Step 2): `python3 scripts/shell-bench/bench.py --host athanor-ref --stages facts,response,memory --repeat 3 --out /var/tmp/shell-bench-smoke`
      Expected: a `results.json` with one entry per bar status module and per dock application, numbers in milliseconds, `misses` 0 (if not, and Task 2 found Escape does not close, change the close op to what `spikes.md` says and run again). Then `ssh athanor-ref 'systemctl --user show-environment | grep -c ATHANOR'` prints `0`.

- [ ] **Step 6: Commit**

```bash
git add scripts/shell-bench/bench.py scripts/tests/test_shell_bench.py
git commit -m "feat(shell-bench): measure response, idle, memory, recovery and start on the reference machine"
```

---

### Task 7: Dock auto-hide scenarios (ST6)

**Files:**

- Create: `scripts/shell-bench/scenarios.py`

**Interfaces:**

- Consumes: `bench.instrumented`, `bench.inject`, `bench.locate`, `analysis.*`, `Machine`.
- Produces: `scenarios.run(m) -> dict` with `{"dock-autohide/<name>": {"pass": bool, "detail": str}}` for `switch`, `rest`, `leave`, `menu`; register entry named in each (filled after Task 9; until then `"register": null`).

- [ ] **Step 1: Write `scripts/shell-bench/scenarios.py`**

```python
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
            # After a context menu opens and closes and the pointer leaves, it hides again.
            check("menu", [edge, "wait 600", f"move {dock.x} {dock.y}", "wait 300", "rclick", "wait 500",
                           f"key {bench.ESCAPE}", "wait 300", away], away, HIDE_LIMIT, hidden)
    finally:
        if original:
            m.run(f"printf %s {shlex.quote(original.decode())} > {LAYOUT}")
        else:
            m.run(f"rm -f {LAYOUT}")
        m.systemctl("restart", "athanor-dock")
    return results
```

Under auto-hide the revealed dock is centred at the same place as the visible one, so the coordinates found under `visible` hold; if `rest` fails, `menu` fails too, which is correct.

- [ ] **Step 2: Run the scenarios against today's dock** (overlay of Task 4 installed, so the dock logs frames):

Run: `python3 scripts/shell-bench/bench.py --host athanor-ref --stages scenarios --out /var/tmp/shell-bench-scenarios`
Expected: at least `rest` and `switch` fail, with details matching the maintainer's report (nothing drawn on the edge; the band changed after the switch). If all four pass, the defect does not reproduce on the reference machine: record that and ask the maintainer whether it reproduces on the desktop at that moment, because the scenarios then miss something.

- [ ] **Step 3: Commit**

```bash
git add scripts/shell-bench/scenarios.py
git commit -m "feat(shell-bench): add the dock auto-hide scenarios of ST6"
```

---

### Task 8: Soak

**Files:**

- Create: `scripts/shell-bench/soak.py`, `scripts/shell-bench/allow.txt` (empty, one comment line saying entries are `<unit> <regex>`, each needing a reason approved by the maintainer, ST10)

**Interfaces:**

- Consumes: `bench.*`, `analysis.*`, `Machine`.
- Produces: `soak.py --host H [--hours 24] --out DIR` writing `soak.json`, merged into the results by Task 10.

- [ ] **Step 1: Confirm the theme switch path on the machine.** Run: `ssh athanor-ref 'cat ~/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark'`. Expected: `true` or `false`. If the file does not exist, switch the theme once in COSMIC Settings on the machine and look again; if it is elsewhere, use that path below.

- [ ] **Step 2: Write `scripts/shell-bench/soak.py`**

```python
#!/usr/bin/env python3
"""The shell standard's soak (doc_shell_standard.md, ST5, stability): for HOURS, open and
close every popover of the bar and the dock, send bursts of notifications, switch theme,
and suspend and resume once an hour. Passes with no restart, no err line outside
allow.txt, and no process growing by more than 10%.

    python3 scripts/shell-bench/soak.py --host athanor-ref [--hours 24] --out DIR
"""

import argparse
import json
import pathlib
import re
import sys
import time

import analysis
import bench
import machine

THEME = "~/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"
NOTIFY = ("gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications "
          "--method org.freedesktop.Notifications.Notify \"'shell-bench'\" 0 \"''\" \"'soak {n}'\" \"''\" "
          "'[]' '{{}}' 5000 > /dev/null")
WARMUP_S = 1800  # memory is compared from the first sample after half an hour


def allowed(path):
    rules = []
    for line in path.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            unit, pattern = line.split(None, 1)
            rules.append((unit, re.compile(pattern)))
    return rules


def pss(m):
    return {unit: analysis.pss_kb(m.run(f"cat /proc/{m.main_pid(unit)}/smaps_rollup").decode())
            for unit in machine.UNITS}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--host", required=True)
    parser.add_argument("--hours", type=float, default=24)
    parser.add_argument("--suspend-every", type=float, default=3600, help="seconds between suspends")
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()
    m = machine.Machine(args.host)
    theme = m.run(f"cat {THEME}").decode().strip()
    samples, cycle = [], 0
    rules = allowed(bench.HERE / "allow.txt")
    # Instrumented for the positions of the buttons; the restarts it makes come before
    # `restarts` is read and after `after` is.
    with bench.instrumented(m) as since:
        try:
            start = last_suspend = time.monotonic()
            restarts = {u: m.systemctl("show", "-p", "NRestarts", "--value", u).strip() for u in machine.UNITS}
            targets = [t for t in bench.locate(m, since) if t.popup]
            ops = []
            for t in targets:
                side = " right" if t.unit == "athanor-dock" else ""
                ops += [f"move {t.x} {t.y}", "wait 200", "press" + side, "release" + side, "wait 600",
                        f"key {bench.ESCAPE}", "wait 300"]
            while time.monotonic() - start < args.hours * 3600:
                cycle += 1
                bench.inject(m, targets[0].output_w, targets[0].output_h, ops)
                m.session("; ".join(NOTIFY.format(n=f"{cycle}.{i}") for i in range(20)))
                m.run(f"echo {'false' if cycle % 2 else 'true'} > {THEME}")
                if time.monotonic() - start > WARMUP_S:
                    samples.append(pss(m))
                if time.monotonic() - last_suspend > args.suspend_every:
                    m.run("sudo -n systemd-run --on-active=2 rtcwake -m mem -s 60")
                    time.sleep(120)
                    last_suspend = time.monotonic()
        finally:
            m.run(f"echo {theme} > {THEME}")
        after = {u: m.systemctl("show", "-p", "NRestarts", "--value", u).strip() for u in machine.UNITS}
        errors = [
            (e.get("_SYSTEMD_USER_UNIT", ""), e.get("MESSAGE", ""))
            for e in m.journal(since)
            if int(e.get("PRIORITY", 7)) <= 3
            and not any(e.get("_SYSTEMD_USER_UNIT", "").startswith(u) and r.search(str(e.get("MESSAGE", ""))) for u, r in rules)
        ]
    growth = {u: analysis.verdict(analysis.growth(samples[0][u], samples[-1][u]) if samples else None,
                                  analysis.THRESHOLDS["growth"]) for u in machine.UNITS}
    result = {"hours": args.hours, "cycles": cycle, "restarts_unchanged": restarts == after,
              "err_lines": errors[:50], "no_err_lines": not errors, "growth": growth}
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "soak.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k != "err_lines"}))
    return 0 if result["restarts_unchanged"] and not errors and all(g["pass"] for g in growth.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
```

Add `growth` to `analysis.py` with its test in `scripts/tests/test_shell_bench.py`:

```python
def growth(first, last):
    """Relative growth from `first` to `last`."""
    return (last - first) / first
```

```python
class Growth(unittest.TestCase):
    def test_growth_is_relative(self):
        self.assertAlmostEqual(analysis.growth(1000, 1100), 0.10)
```

- [ ] **Step 3: Run the tests, then a 20-minute soak to see one suspend cycle work**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -p 'test_shell_bench.py' -v`, then `python3 scripts/shell-bench/soak.py --host athanor-ref --hours 0.34 --suspend-every 600 --out /var/tmp/soak-smoke`.
Expected: tests pass; the soak prints its summary; the machine suspended once and came back (the maintainer can confirm on the screen); the theme file holds its original value afterwards.

- [ ] **Step 4: Commit**

```bash
git add scripts/shell-bench/soak.py scripts/shell-bench/allow.txt scripts/shell-bench/analysis.py scripts/tests/test_shell_bench.py
git commit -m "feat(shell-bench): add the 24-hour soak of the shell standard"
```

---

### Task 9: Register version 1

**Files:**

- Create: `docs/architecture/shell-features.md`

**Interfaces:**

- Produces: entries `F-<surface>-<nn>` (for example `F-dock-03`), each with surface, feature, sources (reference + link or version), our status (`have`, `missing`, `partial`, `excluded`), and for `excluded` the maintainer's reason. Scenarios name these ids.

- [ ] **Step 1: Research.** Dispatch one research agent (model `sonnet`) per pair of references: macOS + Windows 11, GNOME + KDE Plasma, COSMIC + DankMaterialShell, Noctalia + Caelestia + end-4's illogical-impulse. Each returns, for every surface of ST1 (bar, dock, control center, notifications and calendar, launcher and application library, on-screen display, session lock and authentication dialogs, Settings, workspace overview, greeter), the features offered **in a default installation**, each with a source link or version. Give each agent the start of `/var/tmp/shell-benchmark/REPORT.md` as prior art and ask it for facts with links only, not opinions.

- [ ] **Step 2: Merge into the register.** One table per surface, union of all sources, duplicates merged with all their sources listed. Our status from the code on `iso-v0` and the benchmark report. Mark `excluded` only where the expected reasons of ST3 apply, as **proposals** for the maintainer, never as decided.

- [ ] **Step 3: Header.** Version 1, date, status "draft, awaiting the maintainer's approval", the rule of ST3 on versions and exclusions, a link to the standard.

- [ ] **Step 4: Maintainer checkpoint.** Present the register in Italian (counts per surface, missing entries of the bar and the dock, proposed exclusions) and wait for approval. On approval, set the status to "version 1, frozen on <date>". Then fill the `register` field of the four scenarios of Task 7 with the auto-hide entry's id.

- [ ] **Step 5: Commit**

```bash
git add docs/architecture/shell-features.md scripts/shell-bench/scenarios.py
git commit -m "docs(shell): add the shell feature register, version 1"
```

---

### Task 10: First measurement of today's bar and dock

**Files:**

- Create: `docs/shell-bench/<date>-<commit>/results.json`, `report.md`, `soak.json`, `defects.md`

**Interfaces:**

- Consumes: everything above. The machine runs the **published** image that includes Task 3's instrumentation, so this task waits for this branch's instrumentation to be merged and an image published, or measures on the overlay and says so in the report (ST9: a gate is measured on a published image; this first measurement is not a gate).

- [ ] **Step 1: Run the bench end to end**

Run: `python3 scripts/shell-bench/bench.py --host athanor-ref --commit <image commit>` (the `start` stage asks for a reboot and a login on the machine: the maintainer does it).
Expected: `docs/shell-bench/<date>-<commit>/results.json` and `report.md`, every stage present, 50 repetitions.

- [ ] **Step 2: Run the soak**

Run: `python3 scripts/shell-bench/soak.py --host athanor-ref --out docs/shell-bench/<date>-<commit>` (24 hours; launch it after every approval-gated step is done, see the memory on night runs).
Expected: `soak.json`.

- [ ] **Step 3: Write `defects.md`**: every failing measure and scenario, one line each, with its number and limit, ordered as ST2 orders them (defects of enabled surfaces first). The dock's auto-hide and the restart delay are expected; add what else failed. Do not relax any number (ST2).

- [ ] **Step 4: Commit**

```bash
git add docs/shell-bench
git commit -m "docs(shell-bench): first measurement of the bar and the dock on the reference machine"
```

---

### Task 11: Repair of the dock's auto-hide

**Files:**

- Modify: `forge/specs/athanor-dock/athanor-dock-1.0.0/src/ui/surface.rs` (`Hider::show`, `place`)

**Interfaces:**

- Consumes: the scenarios (Task 7), the overlay procedure (Task 4).

Use superpowers:systematic-debugging. The leading hypothesis, from reading the code: `Hider::show` changes the surface's anchors in place, so its size changes with every hide and reveal; if GTK does not commit a buffer of the new size, cosmic-comp keeps showing the old island buffer at the surface's new origin, which is the start of the edge ("the dock slides to its start"), and the input region stays the old buffer, so the pointer on the edge finds nothing ("it does not come back").

- [ ] **Step 1: Evidence.** With the overlay of the current branch installed and the dock under auto-hide:

```bash
ssh athanor-ref 'systemctl --user set-environment WAYLAND_DEBUG=1 && systemctl --user restart athanor-dock && sleep 4 && systemctl --user unset-environment WAYLAND_DEBUG'
```

Move the pointer onto the dock and away with `inject.py`, then read `journalctl --user -u athanor-dock --since -2min -o cat | grep -E "zwlr_layer_surface_v1.*(set_anchor|set_size|configure|ack_configure)|wl_surface.*(attach|commit|set_input_region)"`.
Expected for the hypothesis: after a `set_anchor` that widens the surface, a `configure` with the full width, an `ack_configure`, and **no** `attach` of a buffer that wide (or one only much later). Write what was seen in the commit message. If the protocol trace shows a buffer of the right size attached, the hypothesis is wrong: stop and report to the maintainer with the trace.

- [ ] **Step 2: Fix by never changing the surface's shape.** Under auto-hide the surface keeps one geometry for its whole life: anchored to its edge and to both ends of it, as deep as the island. The island is centred inside it; the strip lies along the edge. What changes between hidden and shown is the visible child and the input region, never the anchors. This removes the in-place anchor change the defect lives in, and the reveal no longer waits for a configure round trip.

In `place()`, replace the anchor loop with:

```rust
        for anchor in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            // Under auto-hide the surface spans its whole edge for its whole life, so the
            // strip is found anywhere along it and nothing is resized when it hides or shows.
            let across = placement.auto_hide && across(edge).contains(&anchor);
            self.window.set_anchor(anchor, anchor == edge || across);
        }
```

After the island is built, centre it and lay the strip on the edge:

```rust
        if vertical {
            island.set_valign(gtk4::Align::Center);
            strip.set_halign(if edge == Edge::Left { gtk4::Align::Start } else { gtk4::Align::End });
        } else {
            island.set_halign(gtk4::Align::Center);
            strip.set_valign(gtk4::Align::End);
        }
```

and make the stack homogeneous in both directions, so it is as deep as the island in both states:

```rust
        stack.set_hhomogeneous(true);
        stack.set_vhomogeneous(true);
```

Replace `Hider::show` and its comment with:

```rust
    /// Hidden, the strip is drawn and only the strip takes input; shown, the island is
    /// drawn and only the island takes input. The surface keeps its anchors and its size:
    /// changing them in place left cosmic-comp showing the old buffer at the new origin.
    fn show(&self, shown: bool) {
        self.stack
            .set_visible_child_name(if shown { "island" } else { "strip" });
        self.limit_input();
    }

    /// Limits the input region to the visible child, so the transparent rest of the edge
    /// lets the pointer through to the windows below. Called again after every layout,
    /// because the island's bounds change as applications come and go.
    fn limit_input(&self) {
        let (Some(child), Some(surface)) = (self.stack.visible_child(), self.window.surface()) else {
            return;
        };
        let Some(bounds) = child.compute_bounds(&self.window) else {
            return;
        };
        let rect = gtk4::cairo::RectangleInt::new(
            bounds.x().floor() as i32,
            bounds.y().floor() as i32,
            bounds.width().ceil() as i32,
            bounds.height().ceil() as i32,
        );
        surface.set_input_region(&gtk4::cairo::Region::create_rectangle(&rect));
    }
```

In `Surface::new`, before the `Surface { .. }` literal, follow the layouts of the window's frame clock once for the surface's whole life (the window is realized once; `place` only swaps the hider):

```rust
        let layout = hider.clone();
        window.connect_realize(move |window| {
            if let Some(clock) = window.frame_clock() {
                let layout = layout.clone();
                clock.connect_layout(move |_| {
                    let current = layout.borrow().clone();
                    if let Some(hider) = current {
                        hider.limit_input();
                    }
                });
            }
        });
```

In `place()`, right after the anchor loop, give a surface without auto-hide its whole area back for input (a region larger than the surface is clipped to it by the compositor; an empty region would take no input at all):

```rust
        if !placement.auto_hide {
            if let Some(surface) = self.window.surface() {
                let all = gtk4::cairo::RectangleInt::new(0, 0, i32::MAX / 2, i32::MAX / 2);
                surface.set_input_region(&gtk4::cairo::Region::create_rectangle(&all));
            }
        }
```

`gtk4::cairo` is gtk4's re-export of cairo-rs; no new dependency.

The stylesheet must leave the window transparent under auto-hide, so the island-deep surface shows nothing outside `.dock-island` and the strip: check the `window.athanor-dock` rules in `system/athanor-style/calmo/templates/surfaces.css.in` (line 223 on). If one paints the window, change that template rule only, regenerate with `python3 system/athanor-style/calmo/generate.py css` (then `generate.py --check` passes), and commit template and `generated/` together.

- [ ] **Step 3: Unit tests stay green**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- cargo test -p athanor-dock` (through `rig.sh` if needed), `just lint`.
Expected: pass.

- [ ] **Step 4: The scenarios pass on the reference machine.** Build the packages (`just rpms "athanor-bar athanor-dock"`), install them on the overlay (Task 4, Step 2), run `python3 scripts/shell-bench/bench.py --host athanor-ref --stages scenarios --out /var/tmp/shell-bench-dock-fix`.
      Expected: all four `dock-autohide/*` pass. Also check the visible dock and the vertical docks in `scripts/devvm/dock-acceptance.sh` and the dock's surface cases (`forge/test/shell/rig.sh` cases with `dock`), which must not change.

- [ ] **Step 5: Commit**

```bash
git add forge/specs/athanor-dock system/athanor-style/calmo
git commit -m "fix(dock): keep the auto-hide surface's shape and move only its content and input region"
```

---

### Task 12: Restart delay

**Files:**

- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service:20`, `forge/specs/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service:18`, `forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service:18`

- [ ] **Step 1: Change `RestartSec=1s` to `RestartSec=100ms`** in the three units, with this comment above it in each:

```ini
# The first restart comes at once, so a crash costs under a second (doc_shell_standard.md,
# ST5, recovery); RestartSteps stretches the delay towards RestartMaxDelaySec in a crash loop.
RestartSec=100ms
```

- [ ] **Step 2: Verify the crash-loop behaviour still holds** in the dev VM: `scripts/devvm/dock-acceptance.sh` (stage `crash-loop`) and `scripts/devvm/shelld-acceptance.sh`, against packages of this branch deployed with `scripts/devvm/deploy.sh`.
      Expected: PASS: five kills still give up (SH8) and the unit stays active.

- [ ] **Step 3: Recovery passes on the reference machine.** Overlay with the new packages, `python3 scripts/shell-bench/bench.py --host athanor-ref --stages recovery --out /var/tmp/shell-bench-recovery`.
      Expected: `recovery_s` under 1.0 for bar and dock, `same_state` true. If recovery still fails with the delay gone, the cost is the start of the program: record it in `defects.md` as its own defect; do not relax the threshold.

- [ ] **Step 4: Commit**

```bash
git add forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service forge/specs/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service
git commit -m "fix(shell): restart the bar, the dock and shelld at once after a crash"
```

---

### Task 13: Document changes and acceptance

**Files:**

- Modify: `docs/architecture/doc_shell.md` (SH1, line 43; section 3), `docs/architecture/doc_bar.md` (BR7; section 5 item 17), `docs/architecture/doc_shell_standard.md` (status)

- [ ] **Step 1: Edit with a Python replace from Bash** (formatter trap), each `assert old in s` before replacing:
  - `doc_shell.md` SH1: `ours is usable by an average user and better than COSMIC's at the moment of the switch` → `ours passes \`doc_shell_standard.md\``.
  - `doc_shell.md` section 3: before `### Stage 3` (or at the end of the stage 2 subsection if stage 3 has no heading yet), add a paragraph: "**The control center** comes before the rest of stage 3 (`doc_shell_standard.md`, section 4, step 5): its own specification, built on the services the bar already holds, then its implementation and its gate."
  - `doc_bar.md` BR7, after the auto-hide bullet: " - Auto-hide is specified by the scenarios of `doc_shell_standard.md`, ST6, which the shell bench runs (`scripts/shell-bench/scenarios.py`)."
  - `doc_bar.md` section 5, item 17: append " These are the memory budgets of `doc_shell_standard.md`, ST5."
  - `doc_shell_standard.md` status line: leave "awaiting the maintainer's approval" unless the maintainer has approved; if so, write "approved on <date>".
    Check: `git diff --numstat docs/architecture` shows only small counts on those files.

- [ ] **Step 2: Acceptance (spec section 7)**, each with its evidence in the PR body:
  1. the bench ran end to end on the reference machine and wrote `docs/shell-bench/<date>-<commit>/` (Task 10);
  2. register version 1 approved and frozen (Task 9);
  3. the dock auto-hide scenarios pass on the reference machine after the repair (Task 11, Step 4);
  4. the section 5 changes are made (Step 1).

- [ ] **Step 3: Full checks**

Run: `node /home/hr-mes/.claude/bin/cc-test.mjs -- python3 -B -m unittest discover -s scripts/tests -v`, `just lint`, `python3 scripts/verify.py`.
Expected: pass (verify.py's known baseline failures excepted, named in the PR).

- [ ] **Step 4: Commit and open the PR** against `iso-v0`, after asking the maintainer:

```bash
git add docs/architecture/doc_shell.md docs/architecture/doc_bar.md docs/architecture/doc_shell_standard.md
git commit -m "docs(shell): make the shell standard the rule for replacing a surface"
git push -u origin shell-standard-spec
gh pr create --base iso-v0 --title "Shell standard: bench, register v1, dock auto-hide repair" --body-file <body>
```
