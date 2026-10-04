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
# An empty point of the desktop: a click there closes a dock menu (spike Q2).
DESKTOP = (960, 540)
# How long a popover stays open before the bench closes it; its smoothness is measured over
# this time only, so that the close is not taken for a late frame.
OPEN_MS = 700
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
    popover. Escape does not close it (spike Q2 in docs/shell-bench/spikes.md): a click on
    the same button closes a bar popover, a click on the desktop a dock menu."""
    results = {}
    with instrumented(m) as since:
        for t in locate(m, since):
            if not t.popup:
                continue  # an opener starts another program: not a surface of ours
            dock = t.unit == "athanor-dock"
            side = " right" if dock else ""
            close = [f"move {DESKTOP[0]} {DESKTOP[1]}", "click"] if dock else ["click"]
            ops = []
            for _ in range(args.repeat):
                ops += [f"move {t.x} {t.y}", "wait 300", "press" + side, "wait 150",
                        "release" + side, f"wait {OPEN_MS}", *close, "wait 500"]
            injected = inject(m, t.output_w, t.output_h, ops)
            frames, _ = analysis.parse_journal(m.journal(since))
            presses = [ns for op, ns in injected if op.startswith("press")]
            releases = [ns for op, ns in injected if op.startswith("release")]
            in_place = analysis.responses_ms(presses, frames, t.unit, t.surface)
            opening = analysis.responses_ms(releases, frames, t.unit, "popover")
            on_time, worst, measured = analysis.smoothness(
                frames, [(r, r + OPEN_MS * 10**6) for r in releases], t.unit, "popover")
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
            # A measure carries a value and its limit; a scenario its detail, judged by ST6.
            value = node["value"] if "value" in node else node["detail"]
            limit = node.get("limit", "ST6")
            shown = "not measured" if value is None else f"{value:.4g}" if isinstance(value, float) else value
            lines.append(f"| {stage} | {prefix} | {shown} | {limit} | {'yes' if node['pass'] else '**no**'} |")
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
