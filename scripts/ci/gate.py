#!/usr/bin/env python3
"""The verdict of `gate`, the one required check of pr.yml (doc_pipeline.md PL3).

Usage: gate.py NEEDS_JSON

NEEDS_JSON is the `needs` context of the gate job (toJSON(needs)): every other job of
pr.yml, with its result and outputs. The gate runs with `if: always()` and passes when every
job it needs succeeded or was skipped; a failed, cancelled or unknown result fails it.

One rule is stricter than PL3: a job named after an area of changes.json (kernel, specs,
...) that the change selected must have run. A selected build that was skipped, because a
job it needs failed or a condition left it out, would otherwise pass unseen. The change
detection job must therefore succeed and its `changes` output must be the JSON of
changes.json.

Prints one line per job and the verdict, as Markdown, for the job summary.
"""

import json
import pathlib
import sys

PASSING = ("success", "skipped")


def problems(needs):
    """What keeps the gate red: one line per job, empty when the gate passes."""
    found = []
    detection = needs.get("changes", {})
    if detection.get("result") != "success":
        return [f"changes: {detection.get('result') or 'missing'}"]
    try:
        selected = json.loads(detection.get("outputs", {}).get("changes", ""))
    except json.JSONDecodeError:
        return ["changes: output is not the JSON of changes.json"]
    for job, data in sorted(needs.items()):
        result = data.get("result")
        if result not in PASSING:
            found.append(f"{job}: {result if result else repr(result)}")
        elif result == "skipped" and selected.get(job) is True:
            found.append(f"{job}: selected by the change but skipped")
    return found


def main(path):
    needs = json.loads(pathlib.Path(path).read_text())
    print("| Job | Result |\n| --- | --- |")
    for job, data in sorted(needs.items()):
        print(f"| {job} | {data.get('result') or 'unknown'} |")
    found = problems(needs)
    print()
    if found:
        print("**gate: red**\n")
        for line in found:
            print(f"- {line}")
        return 1
    print("**gate: green**")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: gate.py NEEDS_JSON")
    sys.exit(main(sys.argv[1]))
