#!/usr/bin/python3
"""orca_compare.py <reference> <transcript>

doc_own_stack.md, OS8: Orca's speech transcript of a surface "holds every name, role, state
and value of the GTK transcript". Both files hold one utterance per line, as orca_walk.py
writes them; the reference is the GTK surface's. Every utterance of the reference must be
spoken in the transcript, in any order: the two walks may start on different controls.
Prints a verdict per utterance and exits 1 when one is missing.
"""

import sys
from itertools import pairwise
from pathlib import Path

# Orca says a node's role after its name. AccessKit 0.25 has no "grouping": its groups reach
# AT-SPI as panels. The maintainer decided on 2026-10-17 that AccessKit's "panel" equals GTK's
# "grouping" (atspi_compare.py), so a grouping of the reference may be heard as a panel, in
# English and in Italian. Only the reference's role is rewritten: a label that ends in
# "panel", such as "Control panel", is matched as it is said.
#
# The same goes for a single-line field: AccessKit's Role::TextInput reaches AT-SPI as an entry
# and has no way to say "text", the role of GTK's GtkText, so the line of a field may be heard
# as an entry's. Only the line of a field of the gallery (the `field` and `notes` keys of its
# strings.tsv) is rewritten: Orca 51 ends no utterance with a full stop, so nothing else tells a
# field's line from a label's, and a label such as "Sample text" is matched as it is said.
#
# A multi-line field too: GTK's GtkTextView reaches AT-SPI as a text, AccessKit's
# Role::MultilineTextInput as a multi-line entry, and Orca says the line of the caret after a
# text's line but in the same utterance as an entry's. So the line of a text area of the gallery
# (the `notes` key of its strings.tsv) and the utterance that follows it in the reference may be
# heard together, as an entry's line that ends in that utterance; neither alone, nor a line after
# anything else, a single-line field's line included.
#
# And a switch: GTK 4.24's GtkSwitch reaches AT-SPI as a switch, AccessKit's Role::Switch as a
# toggle button (maintainer, 2026-10-21). Only a line that names a switch of the gallery (the
# `switch` key of its strings.tsv) with the state and the role words together is rewritten: a
# check box, whatever its label, spoken as a toggle button still fails.


def gallery_labels(key):
    """The texts of `key` in the gallery's strings.tsv, one per language."""
    return [
        line.split("\t")[1:]
        for line in (
            Path(__file__).resolve().parents[3] / "system/athanor-toolkit/gallery/strings.tsv"
        )
        .read_text(encoding="utf-8")
        .splitlines()
        if line.startswith(f"{key}\t")
    ][0]


SWITCH_LABELS = gallery_labels("switch")
AREA_LABELS = gallery_labels("notes")
FIELD_LABELS = gallery_labels("field") + AREA_LABELS
# Orca 51's words, from its catalogue: the state, then the role.
SWITCH_SPEECH = {
    "on switch": "toggle button pressed",
    "off switch": "toggle button not pressed",
    "attivo selettore": "pulsante di commutazione premuto",
    "non attivo selettore": "pulsante di commutazione non premuto",
}
SAME_ROLE = {"grouping": "panel", "raggruppamento": "pannello"}
SAME_FIELD_ROLE = {"text": "entry", "testo": "campo inserimento"}


def heard_as(utterance):
    """The ways the transcript may say a reference utterance: as it is, and, when it ends in
    a grouping's role, with a panel's, or a field's line, with an entry's."""
    yield utterance
    for role, said in SAME_ROLE.items():
        if utterance.endswith(f" {role}"):
            yield f"{utterance[: -len(role)]}{said}"
    for label in FIELD_LABELS:
        for role, said in SAME_FIELD_ROLE.items():
            if utterance == f"{label} {role}":
                yield f"{label} {said}"
    for label in SWITCH_LABELS:
        for gtk, said in SWITCH_SPEECH.items():
            if utterance == f"{label} {gtk}":
                yield f"{label} {said}"


def compare(reference, transcript):
    """Returns (lines, failures) for two lists of utterances."""
    spoken = set(transcript)
    lines, failures = [], []
    if not reference:
        # A reference that holds nothing would let any transcript pass.
        return lines, ["the reference holds no utterance"]
    together = {}
    for field, after in pairwise(reference):
        for label in AREA_LABELS:
            for role, said in SAME_FIELD_ROLE.items():
                if field != f"{label} {role}":
                    continue
                joined = f"{label} {said} {after}"
                if joined in spoken:
                    together.setdefault(field, joined)
                    together.setdefault(after, joined)
    for utterance in dict.fromkeys(reference):
        said = next((form for form in heard_as(utterance) if form in spoken), None)
        said = said or together.get(utterance)
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
