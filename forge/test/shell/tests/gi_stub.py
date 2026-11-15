"""A stand-in for PyGObject where it is not installed (the unit job has none): enough for the
e2e scripts to import, with GLib.Error a real exception class. A real gi is left alone."""

import sys
import types
from unittest import mock


def install():
    try:
        from gi.repository import GLib  # noqa: F401
    except ImportError:
        glib = types.SimpleNamespace(Error=type("Error", (Exception,), {}))
        repository = types.ModuleType("gi.repository")
        repository.GLib = glib
        repository.Gio = mock.MagicMock()
        gi = types.ModuleType("gi")
        gi.repository = repository
        sys.modules.update({"gi": gi, "gi.repository": repository})
