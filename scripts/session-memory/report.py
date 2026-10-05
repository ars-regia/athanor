#!/usr/bin/env python3
"""Turn the guest's framed PSS report into a JSON report, and compare it with the budget.

Usage: report.py CONSOLE_LOG OUT_JSON [BUDGET_JSON]

CONSOLE_LOG is the serial log of the ISO acceptance (forge/test/iso/console.py), which holds
the block pss.py printed in the guest (its format is described there). The block is checked
against its byte count and checksum; the first intact copy is used. OUT_JSON gets the
report. BUDGET_JSON (default: budget.json beside this script) holds `max_pss_mb`, the
whole-session budget of doc_shell_standard.md, ST5; the maintainer sets it.

Exit status:
  - no budget figure: 0 always; the script reports the sum, or that there is none, and says
    that nothing was compared;
  - a budget figure: 1 when the sum exceeds it, and also 1 when there is no intact report
    (`no-measurement`: no block; `corrupt`: blocks found, none intact), so that a lost line
    cannot bypass a budget that is set.
"""

import base64
import binascii
import json
import pathlib
import re
import sys
import zlib

BEGIN = re.compile(r"^PSS_BEGIN (\d+) ([0-9a-f]{8})$")
CHUNK = re.compile(r"^PSS_D (\d+) ([A-Za-z0-9+/=]+)$")


def blocks(log_text):
    """Every block that starts in the log as (declared bytes, crc, {index: chunk})."""
    found, current = [], None
    for line in log_text.replace("\r", "").splitlines():
        if m := BEGIN.match(line):
            current = (int(m.group(1)), m.group(2), {})
            found.append(current)
        elif current is not None and (m := CHUNK.match(line)):
            current[2][int(m.group(1))] = m.group(2)
        elif line == "PSS_END":
            current = None
    return found


def decode(declared, crc, chunks):
    """The report of one block, or None when any check fails."""
    try:
        if sorted(chunks) != list(range(len(chunks))):
            return None
        raw = base64.b64decode(
            "".join(chunks[i] for i in sorted(chunks)), validate=True
        )
        if len(raw) != declared or f"{zlib.crc32(raw):08x}" != crc:
            return None
        data = json.loads(raw)
    except (binascii.Error, ValueError):
        return None
    ok = (
        isinstance(data, dict)
        and isinstance(data.get("pss_kb"), int)
        and isinstance(data.get("processes"), list)
    )
    return data if ok else None


def parse(log_text):
    """(report or None, 'ok' | 'none' | 'corrupt')."""
    found = blocks(log_text)
    for block in found:
        data = decode(*block)
        if data is not None:
            return data, "ok"
    # A start marker that no intact block follows (cut, garbled or wrongly summed) is damage.
    return None, ("corrupt" if found or "PSS_BEGIN" in log_text else "none")


def budget_mb(path):
    """The budget in MB, or None when the file or the figure is absent."""
    path = pathlib.Path(path)
    if not path.is_file():
        return None
    value = json.loads(path.read_text()).get("max_pss_mb")
    return None if value is None else float(value)


def build(log_text, budget):
    data, state = parse(log_text)
    if data is None:
        return {
            "measured": False,
            "budget_mb": budget,
            "verdict": "no-measurement" if state == "none" else "corrupt",
        }
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
        "unreadable": data.get("unreadable", 0),
        "budget_mb": budget,
        "verdict": verdict,
        "processes": top,
    }


def summary(report):
    if not report["measured"]:
        what = (
            "no intact report in the console log"
            if report["verdict"] == "corrupt"
            else "no report in the console log"
        )
        tail = (
            "no budget is set, so nothing is compared"
            if report["budget_mb"] is None
            else f"a budget of {report['budget_mb']} MB is set, so this fails"
        )
        return f"session memory: {what}; {tail}"
    head = f"session memory: {report['pss_mb']} MB PSS over {report['processes_counted']} processes"
    if report["unreadable"]:
        head += f" ({report['unreadable']} unreadable)"
    if report["verdict"] == "no-budget":
        return (
            head
            + "; no budget is set in budget.json, so this reports and does not fail"
        )
    return head + f"; budget {report['budget_mb']} MB: {report['verdict']}"


def failed(report):
    if report["verdict"] == "over":
        return True
    return not report["measured"] and report["budget_mb"] is not None


def main(argv):
    if len(argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    here = pathlib.Path(__file__).resolve().parent
    log = pathlib.Path(argv[1]).read_text(errors="replace")
    report = build(log, budget_mb(argv[3] if len(argv) == 4 else here / "budget.json"))
    pathlib.Path(argv[2]).write_text(json.dumps(report, indent=2) + "\n")
    print(summary(report))
    return 1 if failed(report) else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
