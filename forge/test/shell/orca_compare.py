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
# "grouping" (atspi_compare.py), so a grouping of the reference may be heard as a panel, in
# English and in Italian. Only the reference's role is rewritten: a label that ends in
# "panel", such as "Control panel", is matched as it is said.
#
# The same goes for a single-line field: AccessKit's Role::TextInput reaches AT-SPI as an entry
# and has no way to say "text", the role of GTK's GtkText, so the line of a field may be heard
# as an entry's. Only that line is rewritten: Orca ends a field's line with a full stop and a
# label's without, so a label such as "Sample text" is matched as it is said.
SAME_ROLE = {"grouping": "panel", "raggruppamento": "pannello"}
SAME_FIELD_ROLE = {"text": "entry", "testo": "campo inserimento"}


def heard_as(utterance):
    """The ways the transcript may say a reference utterance: as it is, and, when it ends in
    a grouping's role, with a panel's, or a field's line, with an entry's."""
    yield utterance
    for role, said in SAME_ROLE.items():
        if utterance.endswith(f" {role}"):
            yield f"{utterance[: -len(role)]}{said}"
    for role, said in SAME_FIELD_ROLE.items():
        if utterance.endswith(f" {role}."):
            yield f"{utterance[: -len(role) - 1]}{said}."


def compare(reference, transcript):
    """Returns (lines, failures) for two lists of utterances."""
    spoken = set(transcript)
    lines, failures = [], []
    if not reference:
        # A reference that holds nothing would let any transcript pass.
        return lines, ["the reference holds no utterance"]
    for utterance in dict.fromkeys(reference):
        said = next((form for form in heard_as(utterance) if form in spoken), None)
        if said is None:
            lines.append(f"{utterance!r}: missing; FAIL")
            failures.append(f"{utterance!r}: missing")
        else:
            lines.append(
                f"{utterance!r}: spoken"
                + ("" if said == utterance else f" as {said!r}")
            )
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
