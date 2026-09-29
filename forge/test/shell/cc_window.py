#!/usr/bin/python3
"""cc_window.py N - one GTK window with the app id org.athanor.CcWindowN and the title
cc-window-N: a subject the compositor client can find, act on and close."""

import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk  # noqa: E402

number = sys.argv[1]
app = Gtk.Application(application_id=f"org.athanor.CcWindow{number}")


def present(application):
    window = Gtk.ApplicationWindow(application=application, title=f"cc-window-{number}")
    window.set_default_size(320, 200)
    window.present()


app.connect("activate", present)
sys.exit(app.run([]))
