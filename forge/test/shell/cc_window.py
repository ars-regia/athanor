#!/usr/bin/python3
"""cc_window.py N - one GTK window with the app id org.athanor.CcWindowN and the title
cc-window-N: a subject the compositor client can find, act on and close.
SIGUSR1 retitles every window with the text of /tmp/cc-window-N.title."""

import signal
import sys
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

number = sys.argv[1]
app = Gtk.Application(application_id=f"org.athanor.CcWindow{number}")


def present(application):
    window = Gtk.ApplicationWindow(application=application, title=f"cc-window-{number}")
    window.set_default_size(320, 200)
    window.present()


def retitle():
    title = Path(f"/tmp/cc-window-{number}.title").read_text(encoding="utf-8")
    for window in app.get_windows():
        window.set_title(title)
    return GLib.SOURCE_CONTINUE


GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, retitle)
app.connect("activate", present)
sys.exit(app.run([]))
