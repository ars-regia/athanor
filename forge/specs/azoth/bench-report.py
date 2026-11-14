#!/usr/bin/env python3
"""Trend report of the benchmark (bench.sh; spec, section 7, gate 5).

    bench-report.py DIR...

Every DIR is a weekly run, in chronological order, with one or more results.json
(<label>/results.json). Prints Markdown for the job summary: the table of the latest
run, the comparison between the kernels measured in the same run (for instance the
published kernel against the o3 variant: the comparison that decides, because GitHub
runners change CPU from one run to the next) and one Mermaid chart per metric with the
trend across runs of the reference kernel (the first of every run).
"""

import json
import sys
from pathlib import Path

# Direction of "better" for every metric: gives the sign of the A/B comparison.
HIGHER_IS_BETTER = {
    "hackbench_s": False,
    "schbench_wakeup_p99_us": False,
    "schbench_rps_p50": True,
    "fio_null_read_iops": True,
    "netperf_tcp_stream_mbps": True,
    "netperf_tcp_rr_tps": True,
}


def load(run_dir):
    """[results] of a run: the first is the reference kernel (published)."""
    results = [json.loads(p.read_text()) for p in sorted(run_dir.rglob("results.json"))]
    results.sort(
        key=lambda r: (
            r["label"] != "published" and not r["label"].startswith("7"),
            r["label"],
        )
    )
    return results


def fmt(v):
    return f"{v:,.0f}" if abs(v) >= 100 else f"{v:.2f}"


def main():
    runs = [(Path(d), load(Path(d))) for d in sys.argv[1:]]
    runs = [(d, r) for d, r in runs if r]
    if not runs:
        sys.exit("no results.json")
    latest_dir, latest = runs[-1]
    ref = latest[0]
    out = [
        f"## Benchmark {ref['kver']} ({ref['date']})",
        "",
        f"QEMU/KVM {ref['vcpus']} vCPU, {ref['mem_mib']} MiB, {ref['seconds_per_test']} s per test, host `{ref['host_cpu']}`",
        "",
    ]
    # Latest run: one column per measured kernel; with several kernels, the delta towards the first.
    labels = [r["label"] for r in latest]
    out.append(
        "| metric | "
        + " | ".join(f"`{ln}`" for ln in labels)
        + (" | delta | " if len(latest) > 1 else " |")
    )
    out.append(
        "| --- |" + " --- |" * len(labels) + (" --- |" if len(latest) > 1 else "")
    )
    for metric in ref["metrics"]:
        row = [metric] + [
            fmt(r["metrics"][metric]["value"]) + " " + r["metrics"][metric]["unit"]
            if metric in r["metrics"]
            else "n/a"
            for r in latest
        ]
        if len(latest) > 1:
            deltas = []
            for r in latest[1:]:
                if metric in r["metrics"] and ref["metrics"][metric]["value"]:
                    pct = (
                        r["metrics"][metric]["value"] / ref["metrics"][metric]["value"]
                        - 1
                    ) * 100
                    better = (pct > 0) == HIGHER_IS_BETTER.get(metric, True)
                    deltas.append(f"{pct:+.1f}% ({'better' if better else 'worse'})")
            row.append(", ".join(deltas) or "n/a")
        out.append("| " + " | ".join(row) + " |")
    if len(latest) > 1:
        out += [
            "",
            f"The delta is of the variant against `{ref['label']}`, in the same run and on the same machine.",
        ]
    # Trend of the reference kernel across runs: one chart per metric.
    history = [r[0] for _, r in runs]
    if len(history) > 1:
        out += ["", f"### Trend over {len(history)} runs", ""]
        dates = [h["date"] for h in history]
        for metric, info in ref["metrics"].items():
            values = [
                h["metrics"][metric]["value"] if metric in h["metrics"] else 0
                for h in history
            ]
            direction = (
                "higher is better"
                if HIGHER_IS_BETTER.get(metric, True)
                else "lower is better"
            )
            out += [
                "```mermaid",
                "xychart-beta",
                f'    title "{metric} ({info["unit"]}, {direction})"',
                "    x-axis [" + ", ".join(f'"{d}"' for d in dates) + "]",
                f'    y-axis "{info["unit"]}"',
                "    line [" + ", ".join(f"{v:g}" for v in values) + "]",
                "```",
                "",
            ]
        out.append(
            "GitHub runners change CPU from one run to the next (`host_cpu` in results.json): the trend is indicative, the decision lies in the A/B comparison of the same run."
        )
    print("\n".join(out))


if __name__ == "__main__":
    main()
