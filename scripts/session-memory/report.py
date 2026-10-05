#!/usr/bin/env python3
"""Turn the guest's `PSS_REPORT` line into a JSON report, and compare it with the budget.

Usage: report.py CONSOLE_LOG OUT_JSON [BUDGET_JSON]

CONSOLE_LOG is the serial log of the ISO acceptance (forge/test/iso/console.py), which holds
the line pss.py printed in the guest. OUT_JSON gets the report. BUDGET_JSON
(default: budget.json beside this script) holds `max_pss_mb`, the whole-session budget of
doc_shell_standard.md, ST5; the maintainer sets it. Without that figure the script reports
the sum, says that nothing was compared, and exits 0. Exit 1 only when a budget is set and
the sum exceeds it. A log with no report is not a failure here: the acceptance verdict
already fails a run that never reached a session.
"""

import json
import pathlib
import re
import sys

LINE = re.compile(r"^PSS_REPORT (\{.*\})\s*$", re.M)


def parse(log_text):
    """The last report in the console log, or None."""
    found = LINE.findall(log_text.replace("\r", ""))
    return json.loads(found[-1]) if found else None


def budget_mb(path):
    """The budget in MB, or None when the file or the figure is absent."""
    path = pathlib.Path(path)
    if not path.is_file():
        return None
    value = json.loads(path.read_text()).get("max_pss_mb")
    return None if value is None else float(value)


def build(log_text, budget):
    data = parse(log_text)
    if data is None:
        return {"measured": False, "budget_mb": budget, "verdict": "no-measurement"}
    total_mb = data["pss_kb"] / 1024
    if budget is None:
        verdict = "no-budget"
    else:
        verdict = "over" if total_mb > budget else "within"
    top = sorted(data["processes"], key=lambda p: -p["pss_kb"])
    return {
        "measured": True,
        "pss_mb": round(total_mb, 1),
        "processes_counted": len(data["processes"]),
        "unreadable": data["unreadable"],
        "budget_mb": budget,
        "verdict": verdict,
        "processes": top,
    }


def summary(report):
    if not report["measured"]:
        return "session memory: no measurement (the run never produced a PSS_REPORT line); nothing compared"
    head = f"session memory: {report['pss_mb']} MB PSS over {report['processes_counted']} processes"
    if report["unreadable"]:
        head += f" ({report['unreadable']} unreadable)"
    if report["verdict"] == "no-budget":
        return (
            head
            + "; no budget is set in budget.json, so this reports and does not fail"
        )
    return head + f"; budget {report['budget_mb']} MB: {report['verdict']}"


def main(argv):
    if len(argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    here = pathlib.Path(__file__).resolve().parent
    log = pathlib.Path(argv[1]).read_text(errors="replace")
    report = build(log, budget_mb(argv[3] if len(argv) == 4 else here / "budget.json"))
    pathlib.Path(argv[2]).write_text(json.dumps(report, indent=2) + "\n")
    print(summary(report))
    return 1 if report["verdict"] == "over" else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
