"""preview_window.py: run in the guest's session by preview-session-acceptance.sh, as
`python3 - COMMAND APP-ID < preview_window.py` (or from a file, under systemd-run).

  window APP-ID   a GTK4 window of the application APP-ID holding one text field, focused;
                  prints the field's whole text, one line per change, to stdout
  active APP-ID   reads the application APP-ID over AT-SPI; exits 0 when its window is the
                  active one, 1 when it is not, 2 when the application is not found
"""

import sys

import gi


def window(app_id):
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
        win.present()
        entry.grab_focus()

    app.connect("activate", activate)
    return app.run([])


def active(app_id):
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is None or app.get_name() != app_id:
            continue
        for child in range(app.get_child_count()):
            node = app.get_child_at_index(child)
            if node is not None and node.get_state_set().contains(
                Atspi.StateType.ACTIVE
            ):
                return 0
        return 1
    return 2


if __name__ == "__main__":
    command, app_id = sys.argv[1:3]
    sys.exit({"window": window, "active": active}[command](app_id))
