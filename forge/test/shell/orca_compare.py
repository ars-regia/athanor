#!/usr/bin/python3
"""orca_compare.py <reference> <transcript>

doc_own_stack.md, OS8: Orca's speech transcript of a surface "holds every name, role, state
and value of the GTK transcript". Both files hold one utterance per line, as orca_walk.py
writes them; the reference is the GTK surface's. Every utterance of the reference must be
spoken in the transcript, in any order: the two walks may start on different controls.
Prints a verdict per utterance and exits 1 when one is missing.
"""

import sys
from pathlib import Path

# Orca says a node's role after its name. AccessKit 0.25 has no "grouping": its groups reach
# AT-SPI as panels. The maintainer decided on 2026-10-17 that AccessKit's "panel" equals GTK's
# "grouping" (atspi_compare.py), so a panel is heard as a grouping, in English and in Italian.
SAME_ROLE = {"panel": "grouping", "pannello": "raggruppamento"}


def heard(utterance):
    """The utterance with a panel's role said as a grouping's."""
    for said, meant in SAME_ROLE.items():
        if utterance.endswith(f" {said}"):
            return f"{utterance[: -len(said)]}{meant}"
    return utterance


def compare(reference, transcript):
    """Returns (lines, failures) for two lists of utterances."""
    spoken = {heard(utterance): utterance for utterance in transcript}
    lines, failures = [], []
    for utterance in dict.fromkeys(reference):
        if utterance in spoken:
            said = spoken[utterance]
            lines.append(
                f"{utterance!r}: spoken"
                + ("" if said == utterance else f" as {said!r}")
            )
        else:
            lines.append(f"{utterance!r}: missing; FAIL")
            failures.append(f"{utterance!r}: missing")
    return lines, failures


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    reference, transcript = (Path(path).read_text().splitlines() for path in argv)
    lines, failures = compare(reference, transcript)
    print("\n".join(lines))
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
