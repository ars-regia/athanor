#!/usr/bin/python3
"""orca_compare.py <reference> <transcript> [--own-compositor <preset|-> <dock|-> <lang>]

doc_own_stack.md, OS8: Orca's speech transcript of a surface "holds every name, role, state
and value of the GTK transcript". Both files hold one utterance per line, as orca_walk.py
writes them; the reference is the GTK surface's. Every utterance of the reference must be
spoken in the transcript, in any order: the two walks may start on different controls.
Prints a verdict per utterance and exits 1 when one is missing.

--own-compositor names the scene's bar preset and dock setting, as rig.sh seeds them: the line
of each button decision 28 of doc_own_stack.md hides there is exempt, naming the decision and
the control's issue (golden_report.exemptions), and so is the bar's tiling button, which waits
on a protocol rather than a decision (own_exemptions). An exempt line the transcript speaks
fails, and so does an exemption that names no utterance of the reference.
"""

import sys
from itertools import pairwise
from pathlib import Path

from golden_report import exemptions

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
#
# A multi-line field too: GTK's GtkTextView reaches AT-SPI as a text, AccessKit's
# Role::MultilineTextInput as a multi-line entry, and Orca says the line of the caret after a
# text's line but in the same utterance as an entry's. So the line of a text area of the gallery
# (the `notes` key of its strings.tsv) and the utterance that follows it in the reference may be
# heard together, as an entry's line that ends in that utterance; neither alone, nor a line after
# anything else, a single-line field's line included.
#
# And a switch: GTK's GtkSwitch reaches AT-SPI as a check box, AccessKit's Role::Switch as a toggle
# button (maintainer, 2026-10-21). Only a line that names a switch of the gallery (the `switch` key
# of its strings.tsv) with the role and the state words together is rewritten: a real check box,
# whatever its label, spoken as a toggle button still fails.


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
SWITCH_SPEECH = {
    "check box checked.": "toggle button pressed.",
    "check box not checked.": "toggle button not pressed.",
    "casella di spunta spuntata.": "pulsante di commutazione premuto.",
    "casella di spunta non spuntata.": "pulsante di commutazione non premuto.",
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
    for role, said in SAME_FIELD_ROLE.items():
        if utterance.endswith(f" {role}."):
            yield f"{utterance[: -len(role) - 1]}{said}."
    for label in SWITCH_LABELS:
        for gtk, said in SWITCH_SPEECH.items():
            if utterance == f"{label} {gtk}":
                yield f"{label} {said}"


# The bar's tiling button under athanor-compositor: not a choice of decision 28 but a protocol to
# come, so atspi_compare.py, which reads the whole tree, still reports it. Orca says it collapsed.
TILING = {"en": "Tiling collapsed", "it": "Affiancamento contratto"}
NO_TILING = ("the bar lacks the tiling button, which needs athanor_workspace_tiling_v1", 739)


def own_exemptions(preset, dock, lang):
    """golden_report.exemptions, and the tiling button a bar running `preset` lacks."""
    exempt = exemptions(preset, dock, lang)
    if preset is not None:
        exempt[TILING[lang]] = NO_TILING
    return exempt


def compare(reference, transcript, exempt=None):
    """Returns (lines, failures) for two lists of utterances. `exempt` maps the name of a button
    decision 28 hides to its (cause, issue); Orca says such a button as "<name> button."."""
    exempt = {f"{name} button.": why for name, why in (exempt or {}).items()}
    spoken = set(transcript)
    lines, failures = [], []
    if not reference:
        # A reference that holds nothing would let any transcript pass.
        return lines, ["the reference holds no utterance"]
    failures += [
        f"exemption {line!r} names no utterance of the reference"
        for line in sorted(set(exempt) - set(reference))
    ]
    together = {}
    for field, after in pairwise(reference):
        for label in AREA_LABELS:
            for role, said in SAME_FIELD_ROLE.items():
                if field != f"{label} {role}.":
                    continue
                joined = f"{label} {said} {after}"
                if joined in spoken:
                    together.setdefault(field, joined)
                    together.setdefault(after, joined)
    for utterance in dict.fromkeys(reference):
        said = next((form for form in heard_as(utterance) if form in spoken), None)
        said = said or together.get(utterance)
        if utterance in exempt:
            cause, issue = exempt[utterance]
            if said is None:
                lines.append(f"{utterance!r}: hidden; {cause} (#{issue})")
            else:
                lines.append(f"{utterance!r}: spoken, but exempt; FAIL")
                failures.append(f"{utterance!r}: spoken, but exempt: {cause} (#{issue})")
        elif said is None:
            lines.append(f"{utterance!r}: missing; FAIL")
            failures.append(f"{utterance!r}: missing")
        else:
            lines.append(
                f"{utterance!r}: spoken"
                + ("" if said == utterance else f" as {said!r}")
            )
    return lines, failures


def main(argv):
    own = argv[2:]
    if len(argv) not in (2, 6) or (own and own[0] != "--own-compositor"):
        print(__doc__, file=sys.stderr)
        return 2
    reference, transcript = (Path(path).read_text().splitlines() for path in argv[:2])
    exempt = {}
    if own:
        preset, dock, lang = (None if v == "-" else v for v in own[1:])
        exempt = own_exemptions(preset, dock, lang)
    lines, failures = compare(reference, transcript, exempt)
    print("\n".join(lines))
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
