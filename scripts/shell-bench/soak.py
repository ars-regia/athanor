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
    # No file of the user's means COSMIC's default; the soak's own file is removed again then.
    theme = m.run(f"if [ -f {THEME} ]; then cat {THEME}; fi").decode().strip()
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
                dock = t.unit == "athanor-dock"
                side = " right" if dock else ""
                # Escape closes neither (spike Q2): the bar's button again, the desktop for the dock.
                close = [f"move {bench.DESKTOP[0]} {bench.DESKTOP[1]}", "click"] if dock else ["click"]
                ops += [f"move {t.x} {t.y}", "wait 200", "press" + side, "release" + side, "wait 600",
                        *close, "wait 300"]
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
            m.run(f"echo {theme} > {THEME}" if theme else f"rm -f {THEME}")
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
