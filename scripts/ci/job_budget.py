#!/usr/bin/env python3
"""A duration budget for a CI job, kept below the job's timeout so a slowdown is seen on a run
that still passes (issue #790): `start` stamps the time in a file, `check` fails when more than
BUDGET minutes have passed since, and prints the overrun to the job summary.

Usage: job_budget.py start STAMP_FILE
       job_budget.py check STAMP_FILE BUDGET_MINUTES
"""

import pathlib
import sys
import time


def start(stamp: pathlib.Path, now: float) -> None:
    stamp.write_text(f"{now:.0f}\n")


def check(stamp: pathlib.Path, budget_minutes: float, now: float) -> tuple[bool, str]:
    """Whether the job is within budget, and the line that says so."""
    elapsed = now - float(stamp.read_text())
    used = f"{int(elapsed // 60)}:{int(elapsed % 60):02d} of {budget_minutes:g} minutes"
    if elapsed > budget_minutes * 60:
        return False, f"The job took {used}: over its budget, which sits below its timeout."
    return True, f"The job took {used}."


def main(argv: list[str]) -> int:
    if len(argv) == 2 and argv[0] == "start":
        start(pathlib.Path(argv[1]), time.time())
        return 0
    if len(argv) == 3 and argv[0] == "check":
        ok, line = check(pathlib.Path(argv[1]), float(argv[2]), time.time())
        print(line)
        return 0 if ok else 1
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
