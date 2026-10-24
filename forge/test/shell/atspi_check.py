#!/usr/bin/python3
"""atspi_check.py <application-name> <minimum-interactive>

doc_shell.md, SH13: "every interactive widget exposes a role and a name in the AT-SPI
tree". Walks the tree of one application on the accessibility bus and fails when a
showing interactive widget has no name, or when fewer interactive widgets than expected
are found, which is what an empty or missing tree looks like. With ATSPI_REQUIRED_NAME
set, a showing node must carry that name: the greeter's seal says its badge so (section 7,
item 3). An environment variable, because scene.sh splits RIG_HOLD on spaces.
With ATSPI_TRANSCRIPT set, the tree is also written there as JSON, every node with its role,
name, states and value, for atspi_compare.py (doc_own_stack.md, OS8).
AX17 (doc_accessibility.md): every showing interactive widget has extents inside its window; a
password field gives out only the invisible character over Text.GetText and
Text.GetStringAtOffset (AX4); with ATSPI_RING set, the focused control's ring has a contrast of
3:1 (AX8, ring_check.py), and ATSPI_HIGH_CONTRAST=1 reads it 3 px wide. The variants of
`rig.sh atspi <surface> --matrix` set the appearance and the text scale, not this script.
"""
import json
import os
import sys
import time

# at-spi2-core 2.58 (the rig's) names ROLE_BUTTON "button"; older releases said "push button".
INTERACTIVE = {"button", "push button", "toggle button", "check box", "radio button", "password text", "entry",
               "text", "combo box", "slider", "spin button", "link", "menu item", "check menu item",
               "radio menu item", "switch"}


INVISIBLE = "\u25cf"


def password_problems(nodes):
    """nodes: [(role, name, texts)], `texts` what Text.GetText and Text.GetStringAtOffset returned.
    A password field gives out only the invisible character (AX4); the text is never quoted."""
    return [f"the password field {name!r} returns more than the invisible character over AT-SPI"
            for role, name, texts in nodes
            if role == "password text" and any(set(text) - {INVISIBLE} for text in texts)]


def problems(nodes, expected, required=(), window=None):
    """nodes: [(role, name, showing)], or (role, name, showing, (x, y, width, height)) with the
    extents in the window's coordinates. With `window`, (x, y, width, height), a showing
    interactive node must have extents inside it (doc_accessibility.md, AX17)."""
    extents = {index: node[3] for index, node in enumerate(nodes) if len(node) > 3}
    nodes = [node[:3] for node in nodes]
    found = [f"no showing node is named {name!r}" for name in required
             if not any(showing and node_name == name for _, node_name, showing in nodes)]
    interactive = [(role, name) for role, name, showing in nodes if showing and role in INTERACTIVE]
    for role, name in interactive:
        if not name.strip():
            found.append(f"a showing '{role}' has no accessible name")
    if window is not None:
        wx, wy, ww, wh = window
        for index, (role, name, showing) in enumerate(nodes):
            if not (showing and role in INTERACTIVE and index in extents):
                continue
            x, y, w, h = extents[index]
            if w <= 0 or h <= 0:
                found.append(f"the showing '{role}' {name!r} has no extents")
            elif x < wx or y < wy or x + w > wx + ww or y + h > wy + wh:
                found.append(f"the showing '{role}' {name!r} at {(x, y, w, h)} is outside its window {window}")
    if len(interactive) < expected:
        found.append(f"{len(interactive)} interactive widget(s) in the tree, expected at least {expected}: "
                     f"the accessibility tree is missing or incomplete")
    return found


def walk(accessible, Atspi, depth=0, out=None):
    out = [] if out is None else out
    states = accessible.get_state_set()
    out.append((accessible.get_role_name(), accessible.get_name() or "", states.contains(Atspi.StateType.SHOWING), depth))
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        if child is not None:
            walk(child, Atspi, depth + 1, out)
    return out


def value_of(accessible, Atspi):
    """What a screen reader reads as the node's value: a number, or the text of an entry."""
    if accessible.get_value_iface() is not None:
        return str(Atspi.Value.get_current_value(accessible))
    if accessible.get_editable_text_iface() is not None:
        return Atspi.Text.get_text(accessible, 0, Atspi.Text.get_character_count(accessible))
    return ""


def transcript(accessible, Atspi, depth=0, out=None):
    """Every node, depth first: its role, name, sorted state names and value."""
    out = [] if out is None else out
    states = sorted(state.value_nick for state in accessible.get_state_set().get_states())
    out.append({"depth": depth, "role": accessible.get_role_name(), "name": accessible.get_name() or "",
                "states": states, "value": value_of(accessible, Atspi)})
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        if child is not None:
            transcript(child, Atspi, depth + 1, out)
    return out


def extents_of(accessible, Atspi):
    """(x, y, width, height) in the window's coordinates, or None for a node that is no component."""
    if accessible.get_component_iface() is None:
        return None
    rect = Atspi.Component.get_extents(accessible, Atspi.CoordType.WINDOW)
    return (rect.x, rect.y, rect.width, rect.height)


def with_extents(accessible, Atspi, out=None):
    """[(role, name, showing, extents)] depth first, as walk() orders them."""
    out = [] if out is None else out
    showing = accessible.get_state_set().contains(Atspi.StateType.SHOWING)
    out.append((accessible.get_role_name(), accessible.get_name() or "", showing,
                extents_of(accessible, Atspi) or (0, 0, 0, 0)))
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        if child is not None:
            with_extents(child, Atspi, out)
    return out


def window_of(app, Atspi):
    """The largest top-level window of the application, at the origin of its own coordinates."""
    sizes = [extents_of(app.get_child_at_index(index), Atspi) or (0, 0, 0, 0)
             for index in range(app.get_child_count())]
    width, height = max(((w, h) for _, _, w, h in sizes), default=(0, 0))
    # ponytail: the largest window bounds every control; a popup larger than its parent would hide an overflow
    return (0, 0, width, height)


def password_texts(accessible, Atspi):
    """What a reader gets from every password field: GetText and GetStringAtOffset, after a probe
    is set as its content, so that an empty field does not pass for lack of anything to leak."""
    nodes = []
    if accessible.get_role_name() == "password text":
        editable = accessible.get_editable_text_iface() is not None
        if editable:
            Atspi.EditableText.set_text_contents(accessible, "probe-not-a-secret")
        count = Atspi.Text.get_character_count(accessible)
        texts = [Atspi.Text.get_text(accessible, 0, count)]
        for granularity in (Atspi.TextGranularity.CHAR, Atspi.TextGranularity.WORD, Atspi.TextGranularity.LINE):
            texts.append(Atspi.Text.get_string_at_offset(accessible, 0, granularity).content)
        nodes.append((accessible.get_role_name(), accessible.get_name() or "", texts))
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        if child is not None:
            nodes.extend(password_texts(child, Atspi))
    return nodes


def focused_extents(accessible, Atspi):
    """The focused node's extents in screen coordinates, the capture's, or None."""
    if accessible.get_state_set().contains(Atspi.StateType.FOCUSED) and accessible.get_component_iface() is not None:
        rect = Atspi.Component.get_extents(accessible, Atspi.CoordType.SCREEN)
        return (rect.x, rect.y, rect.width, rect.height)
    for index in range(accessible.get_child_count()):
        child = accessible.get_child_at_index(index)
        box = child and focused_extents(child, Atspi)
        if box:
            return box
    return None


def find_application(Atspi, name, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            if app is not None and app.get_name() == name:
                return app
        time.sleep(0.5)
    return None


def ring_problems(app, Atspi):
    """AX8: capture the output and check the ring around the focused control. A scene with no
    focused control has no ring to check; it says so, and the walk of a surface that takes focus
    on a key (the launcher's entry) is where this runs."""
    import ring_check
    from input_method_check import capture

    box = focused_extents(app, Atspi)
    if box is None:
        print("ring: no control holds the focus, nothing to measure")
        return []
    return ring_check.problems(capture("atspi-ring"), box, os.environ.get("ATSPI_HIGH_CONTRAST") == "1")


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    app = find_application(Atspi, argv[0])
    if app is None:
        print(f"no application named {argv[0]!r} on the accessibility bus", file=sys.stderr)
        return 1
    tree = walk(app, Atspi)
    for role, name, showing, depth in tree:
        print(f"{'  ' * depth}{role}: {name!r}{'' if showing else ' (hidden)'}")
    found = problems(with_extents(app, Atspi), int(argv[1]),
                     [name for name in [os.environ.get("ATSPI_REQUIRED_NAME", "")] if name],
                     window_of(app, Atspi))
    path = os.environ.get("ATSPI_TRANSCRIPT", "")
    if path:
        with open(path, "w", encoding="utf-8") as file:
            json.dump(transcript(app, Atspi), file, ensure_ascii=False, indent=1)
            file.write("\n")
    # Last: the password check sets a probe in the field, which the transcript must not hold.
    found += password_problems(password_texts(app, Atspi))
    if os.environ.get("ATSPI_RING"):
        found += ring_problems(app, Atspi)
    for line in found:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
