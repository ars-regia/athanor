"""adw_scheme.py: run in the guest's session by session-acceptance.sh (stage scheme).

A libadwaita window that prints what its style manager reads: whether a colour scheme
reaches it from the system (system-supports-color-schemes: from the Settings portal, or
outside a sandbox from org.gnome.desktop.interface, which the stage hides with
GSETTINGS_BACKEND=memory) and whether it draws dark. One line when the window is shown and
one at every change, `scheme: system=<bool> dark=<bool>`; it runs until it is stopped.
"""

import gi

gi.require_version("Adw", "1")
from gi.repository import Adw  # noqa: E402


def report(manager, *_):
    system = str(manager.get_system_supports_color_schemes()).lower()
    dark = str(manager.get_dark()).lower()
    print(f"scheme: system={system} dark={dark}", flush=True)


def activate(app):
    window = Adw.ApplicationWindow(
        application=app, default_width=480, default_height=320
    )
    window.set_content(Adw.StatusPage(title="Session acceptance: colour scheme"))
    window.present()
    manager = Adw.StyleManager.get_default()
    manager.connect("notify::dark", report)
    report(manager)


app = Adw.Application(application_id="org.athanor.SessionAcceptance.Scheme")
app.connect("activate", activate)
app.run()
