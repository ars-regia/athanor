"""session_window.py: run in the guest's session by default-session-acceptance.sh, as
`python3 - COMMAND ARG < session_window.py` (or from a file, under systemd-run).

  window APP-ID    a GTK4 window of the application APP-ID holding one text field, focused;
                   prints the field's whole text, one line per change, to stdout
  inhibit APP-ID   the same window, holding an idle inhibitor while it is mapped
                   (GtkApplication's IDLE inhibit, which GTK sends on Wayland as
                   zwp_idle_inhibit_manager_v1's create_inhibitor on the window's surface)
  active APP-ID    reads the application APP-ID over AT-SPI; exits 0 when its window is the
                   active one, 1 when it is not, 2 when the application is not found
  focused APP      reads the application APP over AT-SPI (athanor-dock, for one); prints the
                   name of its focused node and exits 0, exits 1 when no node has the focus,
                   2 when the application is not found
"""

import sys

import gi


def window(app_id, inhibit=False):
    gi.require_version("Gtk", "4.0")
    from gi.repository import GLib, Gtk

    # The name AT-SPI gives the application, which `active` looks for.
    GLib.set_prgname(app_id)
    GLib.set_application_name(app_id)
    app = Gtk.Application(application_id=app_id)

    def activate(app):
        entry = Gtk.Entry()
        entry.connect("changed", lambda e: print(e.get_text(), flush=True))
        win = Gtk.ApplicationWindow(application=app, title=app_id, child=entry)
        win.set_default_size(480, 120)
        if inhibit:
            # The inhibitor needs the window's surface, which exists once it is mapped.
            win.connect(
                "map",
                lambda w: app.inhibit(
                    w, Gtk.ApplicationInhibitFlags.IDLE, "default-session-acceptance"
                ),
            )
        win.present()
        entry.grab_focus()

    app.connect("activate", activate)
    return app.run([])


def application(name):
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == name:
            return app, Atspi
    return None, Atspi


def active(app_id):
    app, atspi = application(app_id)
    if app is None:
        return 2
    for child in range(app.get_child_count()):
        node = app.get_child_at_index(child)
        if node is not None and node.get_state_set().contains(atspi.StateType.ACTIVE):
            return 0
    return 1


def focused(name):
    app, atspi = application(name)
    if app is None:
        return 2
    pending = [app]
    while pending:
        node = pending.pop()
        if node is None:
            continue
        if node.get_state_set().contains(atspi.StateType.FOCUSED):
            print(node.get_name())
            return 0
        pending.extend(
            node.get_child_at_index(i) for i in range(node.get_child_count())
        )
    return 1


if __name__ == "__main__":
    command, arg = sys.argv[1:3]
    sys.exit(
        {
            "window": window,
            "inhibit": lambda a: window(a, inhibit=True),
            "active": active,
            "focused": focused,
        }[command](arg)
    )
