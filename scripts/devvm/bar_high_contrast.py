"""bar_high_contrast.py: run in the guest's session by bar-acceptance.sh (stage high-contrast).

Switches high contrast on and back off through athanor-bar's accessibility menu over
AT-SPI, as a user would, and checks that each switch reaches the GNOME key high-contrast and
the built themes cosmic-comp reads in both modes (Dark/v2 and Light/v2, spike VL-S1): the
write happens inside the unit's confinement, so a directory or a dconf path the unit cannot
write shows up here as a key that never changes. Prints one line per step and
exits non-zero on the first that fails.
"""

import os
import subprocess
import sys
import time
from pathlib import Path

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402

CONFIG = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
KEYS = [
    CONFIG / "cosmic" / f"com.system76.CosmicTheme.{mode}" / "v2" / "is_high_contrast"
    for mode in ("Dark", "Light")
]


def wait_for(check, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        found = check()
        if found:
            return found
        time.sleep(0.2)
    return check()


def application():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-bar":
            return app
    return None


def showing(accessible, role, label):
    """Showing accessibles of `role` named `label`, directly or through LabelledBy (a GTK
    Switch takes its name from its row's label)."""
    found = []

    def name_of(accessible):
        if accessible.get_name():
            return accessible.get_name()
        for relation in accessible.get_relation_set():
            if relation.get_relation_type() == Atspi.RelationType.LABELLED_BY:
                for index in range(relation.get_n_targets()):
                    target = relation.get_target(index)
                    if target is not None and target.get_name():
                        return target.get_name()
        return ""

    def visit(node):
        if (
            node.get_role_name() == role
            and node.get_state_set().contains(Atspi.StateType.SHOWING)
            and name_of(node) == label
        ):
            found.append(node)
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child is not None:
                visit(child)

    # A popover rebuilds its rows while it opens: a node read in between no longer exists.
    # The walk is then empty, and wait_for walks again.
    try:
        visit(accessible)
    except GLib.Error:
        return []
    return found


def gnome_key():
    return subprocess.run(
        ["gsettings", "get", "org.gnome.desktop.a11y.interface", "high-contrast"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def keys_are(value):
    text = str(value).lower()
    return lambda: (
        gnome_key() == text
        and all(
            key.exists() and key.read_text(encoding="utf-8").strip() == text
            for key in KEYS
        )
    )


def step(name, ok):
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    if not ok:
        sys.exit(1)


def main():
    app = wait_for(application, 20)
    step("athanor-bar is on the accessibility bus", app is not None)
    step(
        "high contrast is off before the run",
        gnome_key() == "false"
        and all(
            not key.exists() or key.read_text(encoding="utf-8").strip() == "false"
            for key in KEYS
        ),
    )
    opener = wait_for(lambda: showing(app, "button", "Accessibility"), 5)
    step("the Accessibility button shows", bool(opener))
    opener[0].do_action(0)
    # GtkSwitch reports as "check box" on at-spi2-core 2.58; older releases say "switch".
    switch = wait_for(
        lambda: (
            showing(app, "check box", "High contrast")
            or showing(app, "switch", "High contrast")
        ),
        5,
    )
    step("the High contrast switch shows", bool(switch))
    switch[0].do_action(0)
    step(
        "high contrast on reaches the GNOME key and both built themes within 2 s",
        wait_for(keys_are(True), 2),
    )
    switch[0].do_action(0)
    step(
        "high contrast off reaches the GNOME key and both built themes within 2 s",
        wait_for(keys_are(False), 2),
    )
    opener[0].do_action(0)


if __name__ == "__main__":
    main()
