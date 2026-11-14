#!/usr/bin/python3
"""orca_compare.py <reference> <transcript> [--own-compositor <preset|-> <dock|-> <lang>]
orca_compare.py --walks <transcript>

doc_own_stack.md, OS8: Orca's speech transcript of a surface "holds every name, role, state
and value of the GTK transcript". Both files hold one utterance per line, as orca_walk.py
writes them; the reference is the GTK surface's. Every utterance of the reference must be
spoken in the transcript, in any order: the two walks may start on different controls.
Prints a verdict per utterance and exits 1 when one is missing.

--own-compositor names the scene's bar preset and dock setting, as rig.sh seeds them: the line
of each button decision 28 of doc_own_stack.md hides there is exempt, naming the decision and
the control's issue (golden_report.exemptions). An exempt line the transcript speaks fails, and
so does an exemption that names no utterance of the reference.

--walks checks the gallery's transcript, one walk per page, on its own (walk_problems).
"""

import sys
from functools import cache
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
# But for the query the launcher's Orca recipe types itself (rig.sh, `orca launcher`): a field
# holding it is an entry, and Orca says an entry's text in the same utterance as its role, where
# GTK's GtkText is a text whose line Orca says after it (maintainer decision, 2026-11-13, a
# Core-AAM role). So a field's line followed by that query, and only by it, may be heard as one
# entry utterance that ends in it.
#
# And a switch: GTK's GtkSwitch reaches AT-SPI as a check box, AccessKit's Role::Switch as a toggle
# button (maintainer, 2026-10-21). Only a line that names a switch of the gallery (the `switch` and
# `enable` keys of its strings.tsv) with the role and the state words together is rewritten: a real check box,
# whatever its label, spoken as a toggle button still fails.


STRINGS = Path(__file__).resolve().parents[3] / "system/athanor-sal/gallery/strings.tsv"


@cache
def gallery_labels(*keys):
    """The texts of `keys` in the gallery's strings.tsv, every language. Read on first use, by
    a reference that holds a gallery's switch, text area or frame title, so that the walk of
    another surface does not need the file; a missing key is an error that names it."""
    rows = {
        row[0]: row[1:]
        for row in (line.split("\t") for line in STRINGS.read_text(encoding="utf-8").splitlines())
    }
    missing = [key for key in keys if key not in rows]
    if missing:
        raise LookupError(f"{STRINGS} has no key {', '.join(missing)}")
    return [text for key in keys for text in rows[key]]


def switch_labels():
    return gallery_labels("switch", "enable")


SWITCH_SPEECH = {
    "check box checked.": "toggle button pressed.",
    "check box not checked.": "toggle button not pressed.",
    "casella di spunta spuntata.": "pulsante di commutazione premuto.",
    "casella di spunta non spuntata.": "pulsante di commutazione non premuto.",
}
SAME_ROLE = {"grouping": "panel", "raggruppamento": "pannello"}
SAME_FIELD_ROLE = {"text": "entry", "testo": "campo inserimento"}
TYPED = "cc window."

# Focus that returns from a dialog. When a dialog closes, GTK moves the focus to the window's
# control, then makes the window active, then sends window:activate, and Orca says the window's
# title with the frame's role ("Galleria cornice"; in English it says the title alone). AccessKit
# reports the window's activation before the focus (focus_moved of accesskit_atspi_common), so
# Orca hears the title without the role. The maintainer decided on 2026-11-13 that GTK's
# "<window title> <frame role word>" is matched by "<window title>" in the transcript, until
# AccessKit can emit the focus before the activation (#1123). The rule holds only for a line of
# the reference that says the gallery's `title` with the frame's role word of the same language,
# and that directly follows, past Orca's key echoes, the line of the gallery dialog's `dialog.close`
# button in that language; every occurrence of the line must follow it. The transcript must say
# the title at the same place, right after that close button past the key echoes: the gallery
# says its title at the start of every walk too. Any other missing frame role, the same line
# outside a dialog's return included, still fails.
FOCUS_RETURN = "AccessKit activates the window before it moves the focus (maintainer, 2026-11-13, #1123)"
# The frame's role word in the columns of the gallery's strings.tsv: English, Italian.
FRAME_ROLE = ("frame", "cornice")
# Orca's echo of the keys a gallery walk presses (rig.sh: space, then Tab), in English and in
# Italian. An echo is not speech about the focused object, and it may land before or after the
# focus's last utterance (#999), so walk_problems drops it.
KEY_ECHOES = {"space", "tab", "spazio", "tabulazione"}


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
    for gtk, said in SWITCH_SPEECH.items():
        label = utterance.removesuffix(f" {gtk}")
        if label != utterance and label in switch_labels():
            yield f"{label} {said}"


def dialog_returns(reference):
    """The lines of the reference that FOCUS_RETURN covers, each mapped to the close button's
    line the transcript must say first and the window title it may say in the line's place."""
    if not any(line.endswith(f" {word}") for line in reference for word in FRAME_ROLE):
        return {}
    forms = {
        f"{title} {word}": (f"{close} button.", title)
        for title, close, word in zip(
            gallery_labels("title"), gallery_labels("dialog.close"), FRAME_ROLE, strict=True
        )
    }
    lines = [line for line in reference if line not in KEY_ECHOES]
    returns, elsewhere = {}, set()
    for before, line in zip([None, *lines], lines):
        if line not in forms:
            continue
        if before == forms[line][0]:
            returns[line] = forms[line]
        else:
            elsewhere.add(line)
    return {line: heard for line, heard in returns.items() if line not in elsewhere}


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
        for role, said in SAME_FIELD_ROLE.items():
            label = field.removesuffix(f" {role}.")
            if label == field or not (after == TYPED or label in gallery_labels("notes")):
                continue
            joined = f"{label} {said} {after}"
            if joined in spoken:
                together.setdefault(field, joined)
                together.setdefault(after, joined)
    returns = dialog_returns(reference)
    # Each line the transcript says with the line before it, past the key echoes.
    follows = set(pairwise(line for line in transcript if line not in KEY_ECHOES))
    for utterance in dict.fromkeys(reference):
        said = next((form for form in heard_as(utterance) if form in spoken), None)
        said = said or together.get(utterance)
        if said is None and returns.get(utterance) in follows:
            said = returns[utterance][1]
            lines.append(f"{utterance!r}: spoken as {said!r}; note: {FOCUS_RETURN}")
            continue
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


def walk_problems(transcript):
    """The problems of a transcript of the gallery's walks, one per page, each opened by Orca's
    first utterance and closed by its last ("Screen reader on." and "Screen reader off." in
    English). Each walk ends with one Tab past the page's controls, which leaves the page for
    the stop after it, the same for every page (cases.gallery_walks); a walk that ends anywhere
    else tabbed a number of times its page does not have. Orca's key echoes are dropped first."""
    if len(transcript) < 2:
        return ["the transcript holds no walk"]
    on, off = transcript[0], transcript[-1]
    walks, walk = [], None
    for line in transcript:
        if line in KEY_ECHOES:
            continue
        if line == on:
            walk = []
        elif line == off and walk is not None:
            walks.append(walk)
            walk = None
        elif walk is not None:
            walk.append(line)
    ends = [walk[-1] if walk else None for walk in walks]
    if len(ends) < 2:
        return [f"{len(ends)} walk: the check needs one per page"]
    if None in ends:
        return [f"the walks end on {ends}: a walk spoke nothing"]
    if len(set(ends)) > 1:
        return [f"the walks end on {ends}, not on one stop: a page's count of Tabs is wrong"]
    return []


def main(argv):
    if argv[:1] == ["--walks"] and len(argv) == 2:
        problems = walk_problems(Path(argv[1]).read_text().splitlines())
        for problem in problems:
            print(f"FAIL {problem}", file=sys.stderr)
        return 1 if problems else 0
    own = argv[2:]
    if len(argv) not in (2, 6) or (own and own[0] != "--own-compositor"):
        print(__doc__, file=sys.stderr)
        return 2
    reference, transcript = (Path(path).read_text().splitlines() for path in argv[:2])
    exempt = {}
    if own:
        preset, dock, lang = (None if v == "-" else v for v in own[1:])
        exempt = exemptions(preset, dock, lang)
    lines, failures = compare(reference, transcript, exempt)
    print("\n".join(lines))
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
