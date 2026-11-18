#!/usr/bin/python3
"""atspi_compare.py <reference.json> <transcript.json> [--own-compositor <preset|-> <dock|-> <lang>]
atspi_compare.py --gallery <reference dir> <transcript dir> <en|it>

doc_own_stack.md, OS8: a surface moves from GTK4 to athanor-sal only when what a screen
reader is told about it holds every name, role, state and value of the GTK version. Both files
are atspi_check.py's ATSPI_TRANSCRIPT; the reference is the GTK chooser's, recorded. Prints one
line per element of the reference with its verdict, and fails when the transcript lacks an
element, a role, a state or a value of the reference. A state said in other words, as a toggle
button's "pressed" for "checked", is a note, not a failure.

--own-compositor names the scene's bar preset and dock setting, as rig.sh seeds them: the
elements of the controls decision 28 of doc_own_stack.md hides there are exempt, each line
naming the decision and the control's issue (golden_report.exemptions). An exempt element the
transcript holds fails, and so does an exemption that names no element of the reference.

--gallery compares the gallery's trees, one per page of cases.py gallery-pages, each in
atspi-gallery-<page>[-it].json of both directories; a page with no reference or no transcript
fails (gallery_problems).

ATSPI_VARIANT is the variant of AX17's matrix the transcript was read in, as rig.sh's
RIG_ATSPI_VARIANT (<appearance>@<scale>); in a dark one the reference reads as for_variant says.
"""

import json
import os
import sys
from pathlib import Path

from atspi_check import INTERACTIVE
from cases import GALLERY_PAGES
from golden_report import exemptions
from orca_compare import GTK_ARROWS, GTK_ARROWS_NOTE, switch_labels, toolkit_arrows

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
# "panel" equals GTK's "grouping". The same goes for a single-line field: AccessKit's
# Role::TextInput reaches AT-SPI as an entry and has no way to say "text", the role of GTK's
# GtkText, and Orca speaks both as a text field. And a list: the toolkit's list, of which the user
# chooses one row, is ARIA's listbox, which W3C Core-AAM 1.2 maps to AT-SPI's list box, where GTK's
# ListBox says list; Orca says "list" for both. The maintainer decided on 2026-11-05 (#982) that the
# toolkit keeps the role Core-AAM prescribes (doc_own_stack.md, OS8, decision 25).
SAME_ROLE = [
    {"button", "push button"},
    {"frame", "window"},
    {"grouping", "panel"},
    {"text", "entry"},
    {"list", "list box"},
]
# The control center's Dark mode tile says the colour scheme the scene is seeded with, and the
# reference is the GTK panel's in the light scheme: in a dark variant of AX17's matrix that tile,
# and nothing else, is expected on, pressed and with "On" as its second line (maintainer decision,
# 2026-11-14). The matrix runs in English.
DARK_VARIANTS = {"dark", "hc-dark"}
DARK_TILE = ("toggle button", "Dark mode")
# GTK puts a label inside each button. The maintainer decided on 2026-10-17 that the label that
# repeats its button's name counts as part of the button, and extended the rule to check boxes on
# 2026-11-05 (#648): GTK's GtkCheckButton holds its label as a child too, and to a label at any
# depth inside the button on 2026-11-14: the GTK panel's tile holds its title in a box in a box,
# and to radio buttons on 2026-11-14: a GtkCheckButton in a group is a radio button that holds its
# label the same way. A label under any other parent is its own element, as the title under the
# window.
BUTTONS = {"button", "push button", "toggle button", "check box", "radio button"}
# A switch: GTK's GtkSwitch reaches AT-SPI as a check box, AccessKit's Role::Switch as a toggle
# button, as orca_compare.py hears it (maintainer, 2026-10-21). GTK draws a label beside the
# switch, a sibling that repeats its name, where the toolkit's switch draws both as one widget:
# that label counts as part of the switch. Both rules hold only for a check box that names a
# switch of the gallery (the `switch` and `enable` keys of its strings.tsv) or of the control
# center (orca_compare.CONTROL_CENTER_SWITCHES and CONTROL_CENTER_FIXTURE_SWITCHES; maintainer,
# 2026-11-14) or of the bar (orca_compare.BAR_SWITCHES), whose toggle button is the
# role W3C Core-AAM 1.2 maps ARIA's switch to: a real check box, whatever its label, said as a
# toggle button still fails. Nor does AccessKit give a toggle button
# the `checkable` state (accesskit_atspi_common 0.21.0, node.rs:336, which adds it only to a
# toggled node of another role): for a gallery's switch its absence is a note (maintainer,
# 2026-11-05, #648; docs/spikes/t1-own-toolkit.md, "AccessKit gaps").
SWITCH = ("check box", "toggle button")
# GTK 4.20's PopoverMenu says each entry of its menu a menu item, a check or a radio entry with
# the `checkable` state, where W3C Core-AAM 1.2 maps ARIA's menuitemcheckbox and menuitemradio to
# AT-SPI's check menu item and radio menu item. The maintainer decided on 2026-11-15 that a
# checkable menu item of the reference equals a check menu item and a radio menu item of the
# transcript, and that the toolkit keeps the roles Core-AAM prescribes; a menu item that is not
# checkable is still neither.
CHECKABLE_MENU_ITEM = {"check menu item", "radio menu item"}
# A disabled control takes no focus (doc_own_stack.md, lines 506-508: "it takes neither the focus
# nor the pointer"), where GTK keeps an insensitive widget focusable. A control disabled in both
# trees, neither sensitive in the reference nor enabled or sensitive in the transcript, may lack
# `focusable` (maintainer, 2026-11-05, #648).
ENABLED = {"enabled", "sensitive"}
# The gallery's pages whose tree is known to differ from GTK's, each with its issue: still compared
# and reported, its differences are notes, and the page fails once it holds no difference, so that
# the entry is removed with the fix (maintainer, 2026-11-05, #648).
EXPECTED_DIFFERENCES = {}
# GTK's ScrolledWindow holds a scroll bar, a node with a value and a child panel with one. The
# toolkit's scroll draws only an indicator, which takes no input, and publishes no node for it; its
# scroll view says how far it is scrolled. The maintainer decided on 2026-11-05 (#982) that a scroll
# bar of the reference, with its subtree, has no counterpart in a transcript that holds no scroll bar.
# The rule holds only for the gallery's pages it names, so that a scroll bar the toolkit should
# publish elsewhere still fails as missing.
INDICATOR = "the toolkit draws a scroll indicator, not a scroll bar (maintainer, 2026-11-05, #982)"
INDICATOR_PAGES = {"lists"}
# GTK 4.20's calendar leaves its four arrow buttons unnamed, which SH13 fails; the toolkit's
# calendar names them from its CalendarText. The maintainer decided on 2026-11-13 (#1122) that
# an unnamed button of the bar's calendar references matches the transcript's named button at the
# same position: the k-th unnamed button of the reference, the k-th named button of the
# transcript that no element of the reference names. The rule holds only for the references it
# names, and for the gallery's calendar page, which shows the same two calendars, so that an
# unnamed button anywhere else still fails as missing. On the same references and pages, a button
# that GTK 4.24 names in English (orca_compare.py's GTK_ARROWS) matches the toolkit's same arrow,
# by its text in the gallery's strings.tsv (maintainer, 2026-11-18, #1210).
ARROWS = "GTK leaves the calendar's arrows unnamed, the toolkit names them (maintainer, 2026-11-13, #1122)"
ARROWS_REFERENCES = {"atspi-bar-calendar", "atspi-bar-calendar-it"}
ARROWS_PAGES = {"calendar"}
# A row of a list.
ROW = "list item"
# The states that make a row a thing to choose, not a heading.
CHOOSABLE = {"selectable", "focusable"}


def in_scroll_bar(nodes):
    """The ids of the nodes of `nodes` that are a scroll bar or lie inside one."""
    out, bar = set(), None
    for node in nodes:
        if bar is not None and node["depth"] <= bar:
            bar = None
        if bar is None and node["role"] == "scroll bar":
            bar = node["depth"]
        if bar is not None:
            out.add(id(node))
    return out


def significant(node):
    """What a screen reader speaks: a node with a name or a value, or a control, or a list row
    that can be chosen. A row with no name of its own is compared by its states when it is
    selectable or focusable, since a toolkit row that lost either would otherwise pass unseen; a
    heading row that holds only a label (GTK's, with the states sensitive, showing and visible) is
    no element, so a toolkit that draws it differently, or not as a row, is not reported as
    missing. The application's own node is named after the binary, so it is left out."""
    return node["role"] != "application" and bool(
        node["name"]
        or node["value"]
        or node["role"] in INTERACTIVE
        or (node["role"] == ROW and CHOOSABLE & set(node["states"]))
    )


def spoken(nodes):
    """The significant nodes, less the labels that repeat the name of a button they are inside,
    at any depth: a screen reader speaks the two once, as the button's name (BUTTONS). Any other
    child, an image for one, stays an element of its own. So are the labels beside a gallery's
    switch (SWITCH)."""
    parents, out, parent_of = [], [], []
    for node in nodes:
        del parents[node["depth"]:]
        parent = parents[-1] if parents else None
        parent_of.append(id(parent))
        repeats = node["role"] == "label" and any(
            above["role"] in BUTTONS and node["name"] == above["name"] for above in parents
        )
        if significant(node) and not (node["name"] and repeats):
            out.append(node)
        parents.append(node)
    switches = {
        (parent, node["name"])
        for node, parent in zip(nodes, parent_of)
        if node["role"] == SWITCH[0] and node["name"] and node["name"] in switch_labels()
    }
    beside = {
        id(node)
        for node, parent in zip(nodes, parent_of)
        if node["role"] == "label" and (parent, node["name"]) in switches
    }
    return [node for node in out if id(node) not in beside]


def for_variant(reference, variant):
    """The reference as the matrix's `variant` (<appearance>@<scale>, or empty outside the
    matrix) reads it: in a dark variant the Dark mode tile is pressed and its "Off" says "On"
    (DARK_VARIANTS); any other variant reads the reference as it is."""
    if variant.split("@")[0] not in DARK_VARIANTS:
        return reference
    out, tile = [], None
    for node in reference:
        if tile is not None and node["depth"] <= tile:
            tile = None
        if (node["role"], node["name"]) == DARK_TILE:
            node = dict(node, states=sorted(set(node["states"]) | {"pressed"}))
            tile = node["depth"]
        elif tile is not None and (node["role"], node["name"]) == ("label", "Off"):
            node = dict(node, name="On")
        out.append(node)
    return out


def gallery_switch(reference_role, role, name):
    """A check box of the reference that names a gallery's switch, said as a toggle button."""
    return (reference_role, role) == SWITCH and name in switch_labels()


def known_absence(state, ref, node):
    """Why the transcript may lack `state` of the reference, or None: a known difference."""
    if state == "checkable" and gallery_switch(ref["role"], node["role"], ref["name"]):
        return "AccessKit's toggle button is not checkable (node.rs:336)"
    disabled = "sensitive" not in ref["states"] and not ENABLED & set(node["states"])
    if state == "focusable" and disabled:
        return "a disabled control takes no focus (doc_own_stack.md, lines 506-508)"
    return None


def checkable_menu_item(ref, node):
    """A checkable menu item of the reference said as a check or radio menu item."""
    return (
        ref["role"] == "menu item"
        and "checkable" in ref["states"]
        and node["role"] in CHECKABLE_MENU_ITEM
    )


def same_role(a, b):
    return a == b or any(a in group and b in group for group in SAME_ROLE)


def arrow_counterparts(reference, candidates):
    """The ARROWS rule: the id of each unnamed button of the reference, mapped to the named
    button of `candidates` at its position among those no element of the reference names, and of
    each button GTK 4.24 names in English, mapped to the toolkit's same arrow."""
    names = {ref["name"] for ref in reference}
    unnamed = [ref for ref in reference if same_role(ref["role"], "button") and not ref["name"]]
    named = [node for node in candidates
             if same_role(node["role"], "button") and node["name"] and node["name"] not in names]
    counterparts = {id(ref): node for ref, node in zip(unnamed, named)}
    # GTK 4.24's English arrows, each the toolkit's same arrow, in order, one node for one arrow.
    taken = {id(node) for node in counterparts.values()}
    for ref in reference:
        if same_role(ref["role"], "button") and ref["name"] in GTK_ARROWS:
            texts = toolkit_arrows(ref["name"])
            arrow = next((node for node in candidates if id(node) not in taken
                          and same_role(node["role"], "button") and node["name"] in texts), None)
            if arrow is not None:
                counterparts[id(ref)] = arrow
                taken.add(id(arrow))
    return counterparts


def compare(reference, transcript, exempt=None, indicator=False, arrows=False):
    """Returns (lines, failures): a verdict per element of the reference, and the failures.
    `exempt` maps the name of an element decision 28 hides to its (cause, issue); `indicator`
    applies the scroll-indicator rule of INDICATOR_PAGES, `arrows` the ARROWS rule."""
    exempt = exempt or {}
    candidates = spoken(transcript)
    lines, failures = [], []
    if not spoken(reference):
        # A reference that holds nothing would let any transcript pass.
        return lines, ["the reference holds no element"]
    failures += [
        f"exemption {name!r} names no element of the reference"
        for name in sorted(set(exempt) - {ref["name"] for ref in spoken(reference)})
    ]
    counterparts = arrow_counterparts(spoken(reference), candidates) if arrows else {}
    bars = set()
    if indicator and not any(node["role"] == "scroll bar" for node in transcript):
        bars = in_scroll_bar(reference)
    for ref in spoken(reference):
        if id(ref) in bars:
            lines.append(f"{ref['role']} {ref['name']!r}: no counterpart; note: {INDICATOR}")
            continue
        # Among the nodes of its name, the one of its role, then one of a role said alike (a
        # toolkit's frame and its panel may share the name of GTK's grouping), then a switch's
        # toggle button (SWITCH), so that a popover's frame named as its switch does not take
        # its place, then any (maintainer decision, 2026-11-14).
        named = [node for node in candidates if node["name"] == ref["name"]]
        match = (
            next((node for node in named if node["role"] == ref["role"]), None)
            or next((node for node in named if same_role(node["role"], ref["role"])), None)
            or next(
                (node for node in named if gallery_switch(ref["role"], node["role"], ref["name"])),
                None,
            )
            or next(iter(named), None)
        )
        match = counterparts.get(id(ref), match)
        label = f"{ref['role']} {ref['name']!r}"
        if ref["name"] in exempt:
            cause, issue = exempt[ref["name"]]
            if match is None:
                lines.append(f"{label}: hidden; {cause} (#{issue})")
            else:
                failures.append(f"{label}: present, but exempt: {cause} (#{issue})")
                lines.append(f"{label}: present, but exempt; FAIL")
            continue
        if match is None:
            failures.append(f"{label}: missing")
            lines.append(
                f"{label}: reference {ref['states']} {ref['value']!r}; missing; FAIL"
            )
            continue
        candidates.remove(match)
        problems, notes = [], []
        if id(ref) in counterparts:
            notes.append(f"named {match['name']!r}: {GTK_ARROWS_NOTE if ref['name'] else ARROWS}")
        if not (
            same_role(match["role"], ref["role"])
            or gallery_switch(ref["role"], match["role"], ref["name"])
            or checkable_menu_item(ref, match)
        ):
            problems.append(f"role {match['role']!r}")
        elif match["role"] != ref["role"]:
            notes.append(f"role {match['role']!r}")
        for state in sorted(set(ref["states"]) - MOMENTARY - set(match["states"])):
            reworded = REWORDED.get(state, set()) & set(match["states"])
            absence = known_absence(state, ref, match)
            if reworded:
                notes.append(f"{state!r} said as {min(reworded)!r}")
            elif absence:
                notes.append(f"{state!r} absent: {absence}")
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


def gallery_problems(references, transcripts, lang):
    """Returns (lines, failures) for the gallery's pages, each compared on its own and its lines
    prefixed with the page; a page with no reference or no transcript is a failure. A page of
    EXPECTED_DIFFERENCES reports its failures as expected, and fails when it has none; a key of
    EXPECTED_DIFFERENCES that names no page fails."""
    suffix = "" if lang == "en" else f"-{lang}"
    lines = []
    failures = [
        f"{page}: expected difference (#{issue}) names no page of cases.py"
        for page, issue in EXPECTED_DIFFERENCES.items()
        if page not in GALLERY_PAGES
    ]
    for page in GALLERY_PAGES:
        name = f"atspi-gallery-{page}{suffix}.json"
        paths = {"reference": Path(references) / name, "transcript": Path(transcripts) / name}
        absent = [f"{page}: no {what} {path}" for what, path in paths.items() if not path.is_file()]
        if absent:
            failures += absent
            continue
        reference, transcript = (
            json.loads(path.read_text(encoding="utf-8")) for path in paths.values()
        )
        found, failed = compare(
            reference, transcript, indicator=page in INDICATOR_PAGES, arrows=page in ARROWS_PAGES
        )
        lines += [f"{page}: {line}" for line in found]
        issue = EXPECTED_DIFFERENCES.get(page)
        if issue is None:
            failures += [f"{page}: {line}" for line in failed]
        elif failed:
            lines += [f"{page}: expected difference (#{issue}): {line}" for line in failed]
        else:
            failures.append(f"{page}: no difference left; remove its expected difference (#{issue})")
    return lines, failures


def main(argv):
    own = argv[2:]
    if argv[:1] == ["--gallery"]:
        if len(argv) != 4 or argv[3] not in ("en", "it"):
            print(__doc__, file=sys.stderr)
            return 2
        lines, failures = gallery_problems(*argv[1:])
    elif len(argv) not in (2, 6) or (own and own[0] != "--own-compositor"):
        print(__doc__, file=sys.stderr)
        return 2
    else:
        reference, transcript = (
            json.loads(Path(path).read_text(encoding="utf-8")) for path in argv[:2]
        )
        reference = for_variant(reference, os.environ.get("ATSPI_VARIANT", ""))
        exempt = {}
        if own:
            preset, dock, lang = (None if v == "-" else v for v in own[1:])
            exempt = exemptions(preset, dock, lang)
        arrows = Path(argv[0]).stem in ARROWS_REFERENCES
        lines, failures = compare(reference, transcript, exempt, arrows=arrows)
    for line in lines:
        print(line)
    for line in failures:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
