#!/usr/bin/python3
"""atspi_compare.py <reference.json> <transcript.json>

doc_own_stack.md, OS8: a surface moves from GTK4 to athanor-toolkit only when what a screen
reader is told about it holds every name, role, state and value of the GTK version. Both files
are atspi_check.py's ATSPI_TRANSCRIPT; the reference is the GTK chooser's, recorded. Prints one
line per element of the reference with its verdict, and fails when the transcript lacks an
element, a role, a state or a value of the reference. A state said in other words, as a toggle
button's "pressed" for "checked", is a note, not a failure.
"""

import json
import sys
from pathlib import Path

from atspi_check import INTERACTIVE

# States that tell when the tree was read, not what the widget is: which widget holds the focus
# and whether the window is the active one depend on the compositor's timing.
MOMENTARY = {"focused", "active"}
# A state and the words that say the same to a screen reader.
REWORDED = {
    "pressed": {"checked"},
    "checked": {"pressed"},
    "sensitive": {"enabled"},
    "enabled": {"sensitive"},
}
# Roles that a screen reader speaks alike. AccessKit 0.25 has no way to say "grouping": its
# Role::Group reaches AT-SPI as a panel. The maintainer decided on 2026-10-17 that AccessKit's
# "panel" equals GTK's "grouping".
SAME_ROLE = [{"button", "push button"}, {"frame", "window"}, {"grouping", "panel"}]
# GTK puts a label inside each button. The maintainer decided on 2026-10-17 that the label that
# repeats its button's name counts as part of the button; a label under any other parent is
# its own element, as the title under the window.
BUTTONS = {"button", "push button", "toggle button"}


def significant(node):
    """What a screen reader speaks: a node with a name or a value, or a control. The
    application's own node is named after the binary, so it is left out."""
    return node["role"] != "application" and bool(
        node["name"] or node["value"] or node["role"] in INTERACTIVE
    )


def spoken(nodes):
    """The significant nodes, less the labels that repeat their button's name: a screen reader
    speaks the two once, as the button's name (BUTTONS). Any other child, an image for one,
    stays an element of its own."""
    parents, out = [], []
    for node in nodes:
        del parents[node["depth"]:]
        parent = parents[-1] if parents else None
        repeats = (
            node["role"] == "label"
            and parent
            and parent["role"] in BUTTONS
            and node["name"] == parent["name"]
        )
        if significant(node) and not (node["name"] and repeats):
            out.append(node)
        parents.append(node)
    return out


def same_role(a, b):
    return a == b or any(a in group and b in group for group in SAME_ROLE)


def compare(reference, transcript):
    """Returns (lines, failures): a verdict per element of the reference, and the failures."""
    candidates = spoken(transcript)
    lines, failures = [], []
    for ref in spoken(reference):
        named = [node for node in candidates if node["name"] == ref["name"]]
        match = next(
            (node for node in named if node["role"] == ref["role"]), None
        ) or next(iter(named), None)
        label = f"{ref['role']} {ref['name']!r}"
        if match is None:
            failures.append(f"{label}: missing")
            lines.append(
                f"{label}: reference {ref['states']} {ref['value']!r}; missing; FAIL"
            )
            continue
        candidates.remove(match)
        problems, notes = [], []
        if not same_role(match["role"], ref["role"]):
            problems.append(f"role {match['role']!r}")
        elif match["role"] != ref["role"]:
            notes.append(f"role {match['role']!r}")
        for state in sorted(set(ref["states"]) - MOMENTARY - set(match["states"])):
            reworded = REWORDED.get(state, set()) & set(match["states"])
            if reworded:
                notes.append(f"{state!r} said as {min(reworded)!r}")
            else:
                problems.append(f"state {state!r} missing")
        if ref["value"] and match["value"] != ref["value"]:
            problems.append(f"value {match['value']!r}, expected {ref['value']!r}")
        failures.extend(f"{label}: {problem}" for problem in problems)
        verdict = "FAIL" if problems else "note" if notes else "same"
        detail = "; ".join(problems + notes)
        lines.append(
            f"{label}: reference {ref['states']} {ref['value']!r}; "
            f"transcript {match['role']!r} {match['states']} {match['value']!r}; "
            f"{verdict}{': ' + detail if detail else ''}"
        )
    return lines, failures


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    reference, transcript = (
        json.loads(Path(path).read_text(encoding="utf-8")) for path in argv
    )
    lines, failures = compare(reference, transcript)
    for line in lines:
        print(line)
    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
