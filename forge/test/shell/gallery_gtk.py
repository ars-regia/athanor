#!/usr/bin/python3
"""gallery_gtk.py [--lang en|it] [--rtl] [--page <name>]

doc_own_stack.md, OS10: the GTK 4 twin of athanor-toolkit's gallery
(system/athanor-toolkit/gallery). It shows the same pages with GTK widgets, from the same
strings.tsv, and takes the same command line. What a screen reader is told about it, and how it
draws, are the references the toolkit's gallery is checked against (rig.sh orca gallery, atspi
gallery and surface gtk-gallery). The page toggle of the page shown holds the focus at the start.
"""

import argparse
import sys
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

from cases import GALLERY_PAGES  # noqa: E402

STRINGS = (
    Path(__file__).resolve().parents[3]
    / "system/athanor-toolkit/gallery/strings.tsv"
)
LANGUAGES = ("en", "it")


def strings(lang):
    """The text of each key of strings.tsv in `lang`."""
    column = LANGUAGES.index(lang) + 1
    rows = (line.split("\t") for line in STRINGS.read_text(encoding="utf-8").splitlines())
    return {row[0]: row[column] for row in rows}


def toggle_row(labels, active):
    """A row of toggle buttons of one width, one of them active, as radio buttons behave."""
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8, homogeneous=True)
    buttons = []
    for label in labels:
        button = Gtk.ToggleButton(label=label)
        if buttons:
            button.set_group(buttons[0])
        row.append(button)
        buttons.append(button)
    buttons[active].set_active(True)
    return row, buttons


def group(title):
    """A vertical group named by its heading, as the GTK chooser's."""
    container = Gtk.Box(
        orientation=Gtk.Orientation.VERTICAL,
        spacing=8,
        accessible_role=Gtk.AccessibleRole.GROUP,
    )
    heading = Gtk.Label(label=title, xalign=0)
    container.update_relation([Gtk.AccessibleRelation.LABELLED_BY], [[heading]])
    container.append(heading)
    return container


def fill_page(page, container, text):
    """The widgets of `page`, as system/athanor-toolkit/gallery/src/main.rs adds them."""
    if page == "buttons":
        container.append(Gtk.Label(label=text["label"], xalign=0))
        container.append(Gtk.Button(label=text["button"]))
        row, _ = toggle_row([text["one"], text["two"], text["three"]], 0)
        container.append(row)
        field = Gtk.Entry()
        field.update_property([Gtk.AccessibleProperty.LABEL], [text["field"]])
        container.append(field)
    elif page == "choices":
        container.append(Gtk.CheckButton(label=text["check1"], active=True))
        container.append(Gtk.CheckButton(label=text["check2"]))
        # A switch has no label of its own: the label beside it names it, and the toolkit's
        # switch draws both as one widget.
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        label = Gtk.Label(label=text["switch"], xalign=0, hexpand=True)
        switch = Gtk.Switch(active=True, valign=Gtk.Align.CENTER)
        switch.update_property([Gtk.AccessibleProperty.LABEL], [text["switch"]])
        row.append(label)
        row.append(switch)
        container.append(row)
    else:
        raise ValueError(f"no page {page!r}")


def present(application, options, text):
    window = Gtk.ApplicationWindow(
        application=application,
        title=text["title"],
        default_width=420,
        resizable=False,
    )
    content = Gtk.Box(
        orientation=Gtk.Orientation.VERTICAL,
        spacing=18,
        margin_top=24,
        margin_bottom=24,
        margin_start=24,
        margin_end=24,
    )
    pages = list(GALLERY_PAGES)
    selector = group(text["pages"])
    row, toggles = toggle_row(
        [text[f"page.{page}"] for page in pages], pages.index(options.page)
    )
    selector.append(row)
    content.append(selector)
    shown = []
    for page in pages:
        container = group(text[f"page.{page}"])
        fill_page(page, container, text)
        container.set_visible(page == options.page)
        content.append(container)
        shown.append(container)

    def select(button):
        # The toggles are one group: GTK keeps one of them active, as the toolkit's radio toggles do.
        if button.get_active():
            for toggle, container in zip(toggles, shown):
                container.set_visible(toggle is button)

    for toggle in toggles:
        toggle.connect("toggled", select)
    window.set_child(content)
    window.set_focus(toggles[pages.index(options.page)])
    window.present()


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--lang", choices=LANGUAGES, default="en")
    parser.add_argument("--rtl", action="store_true")
    parser.add_argument("--page", choices=list(GALLERY_PAGES), default=next(iter(GALLERY_PAGES)))
    options = parser.parse_args(argv[1:])
    # The application's name on the accessibility bus is the program's: the file's name.
    GLib.set_prgname(Path(argv[0]).name)
    if options.rtl:
        Gtk.Widget.set_default_direction(Gtk.TextDirection.RTL)
    text = strings(options.lang)
    application = Gtk.Application(application_id="org.athanor.GalleryGtk")
    application.connect("activate", lambda app: present(app, options, text))
    return application.run([argv[0]])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
