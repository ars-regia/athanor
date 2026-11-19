#!/usr/bin/python3
"""Fails when GTK's own CSS parser reports anything about a stylesheet.

    css_parse_gate.py [--self-test] <file.css>...

GTK never fails a load: it reports through the provider's `parsing-error` signal and
carries on, which is how a sheet with 27 errors reached the old greeter. The provider
needs no display and no Gtk.init(). --self-test first proves that the gate can fail.
"""
import sys
import tempfile

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gio, Gtk  # noqa: E402

KNOWN_BAD = ".x { no-such-property: 1px; color: red !important; }\n"


def reports(path):
    found = []

    def on_error(_provider, section, error):
        start = section.get_start_location()
        found.append(f"{path}:{start.lines + 1}:{start.line_chars + 1}: {error.message}")

    provider = Gtk.CssProvider()
    provider.connect("parsing-error", on_error)
    provider.load_from_file(Gio.File.new_for_path(path))
    return found


def self_test():
    with tempfile.NamedTemporaryFile("w", suffix=".css") as bad:
        bad.write(KNOWN_BAD)
        bad.flush()
        found = reports(bad.name)
    if len(found) != 2:
        print(f"self-test: expected 2 reports on the known-bad sheet, got {found}", file=sys.stderr)
        return False
    return True


def main(argv):
    if argv and argv[0] == "--self-test":
        if not self_test():
            return 2
        argv = argv[1:]
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    found = [line for path in argv for line in reports(path)]
    for line in found:
        print(line)
    print(f"{len(argv)} stylesheet(s), {len(found)} GTK parsing report(s), GTK {Gtk.get_major_version()}.{Gtk.get_minor_version()}.{Gtk.get_micro_version()}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
