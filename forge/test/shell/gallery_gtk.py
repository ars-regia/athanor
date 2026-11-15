#!/usr/bin/python3
"""gallery_gtk.py [--lang en|it] [--rtl] [--page <name>]

doc_own_stack.md, OS10: the GTK 4 twin of athanor-sal's gallery
(system/athanor-sal/gallery). It shows the same pages with GTK widgets, from the same
strings.tsv, and takes the same command line. What a screen reader is told about it, and how it
draws, are the references the toolkit's gallery is checked against (rig.sh orca gallery, atspi
gallery and surface gtk-gallery). The page toggle of the page shown holds the focus at the start;
the dialogs page starts with its dialog open, the focus on the dialog's button, and the menus page
with its menu open.
"""

import argparse
import sys
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Pango", "1.0")
from gi.repository import Gdk, Gio, GLib, Gtk, Pango  # noqa: E402

from cases import GALLERY_PAGES  # noqa: E402

STRINGS = (
    Path(__file__).resolve().parents[3]
    / "system/athanor-sal/gallery/strings.tsv"
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


# The rows of the lists page, a heading among them every ROWS_PER_HEADING, as the toolkit's.
ROWS = 50
ROWS_PER_HEADING = 25


def fill_rows(rows, text, first):
    """The rows numbered from first + 1 into the list box `rows`, each holding its name and a
    second line and named by the first. A heading opens each ROWS_PER_HEADING rows past the
    first ones: a row that is neither selectable, activatable nor focusable, as the toolkit's
    list_heading, so that Tab enters the list at its first row."""
    for n in range(first + 1, first + ROWS + 1):
        if n > first + 1 and (n - first - 1) % ROWS_PER_HEADING == 0:
            last = min(n + ROWS_PER_HEADING - 1, first + ROWS)
            label = Gtk.Label(label=f"{text['rows']} {n}\u2013{last}", xalign=0)
            rows.append(Gtk.ListBoxRow(child=label, selectable=False, activatable=False,
                                       focusable=False))
        name = f"{text['row']} {n}"
        column = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        column.append(Gtk.Label(label=name, xalign=0))
        column.append(Gtk.Label(label=text["row.detail"], xalign=0))
        row = Gtk.ListBoxRow(child=column)
        row.update_property([Gtk.AccessibleProperty.LABEL], [name])
        rows.append(row)


def menu_model(text):
    """The menu of the menus page, as the toolkit's: an item and a check, two radios, then an item
    that takes no input and a submenu of one item, each group a section."""
    first, radios, last, more = Gio.Menu(), Gio.Menu(), Gio.Menu(), Gio.Menu()
    first.append(text["menu.item"], "menu.item")
    first.append(text["menu.check"], "menu.check")
    radios.append(text["menu.radio1"], "menu.sort::name")
    radios.append(text["menu.radio2"], "menu.sort::date")
    last.append(text["menu.off"], "menu.off")
    more.append(text["menu.about"], "menu.item")
    last.append_submenu(text["menu.more"], more)
    model = Gio.Menu()
    for section in (first, radios, last):
        model.append_section(None, section)
    return model


def menu_actions():
    """The menu's actions: GLib's default handlers toggle the check and select a radio."""
    group = Gio.SimpleActionGroup()
    group.add_action(Gio.SimpleAction.new("item", None))
    group.add_action(Gio.SimpleAction.new_stateful("check", None, GLib.Variant.new_boolean(True)))
    group.add_action(
        Gio.SimpleAction.new_stateful(
            "sort", GLib.VariantType.new("s"), GLib.Variant.new_string("name")
        )
    )
    off = Gio.SimpleAction.new("off", None)
    off.set_enabled(False)
    group.add_action(off)
    return group


MENU_ROLES = (
    Gtk.AccessibleRole.MENU_ITEM,
    Gtk.AccessibleRole.MENU_ITEM_CHECKBOX,
    Gtk.AccessibleRole.MENU_ITEM_RADIO,
)


def label_text(widget):
    """The first non-empty label under `widget`: the entry's own label comes before its accelerator."""
    child = widget.get_first_child()
    while child is not None:
        if isinstance(child, Gtk.Label) and child.get_text():
            return child.get_text()
        text = label_text(child)
        if text:
            return text
        child = child.get_next_sibling()
    return None


def name_items(widget):
    """Names every menu entry after its label, as the GTK bar's menu did: GTK 4.20 names a model
    button only through a LabelledBy relation to a label whose accessible name is empty, and the
    relation outranks the Label property, so it goes."""
    child = widget.get_first_child()
    while child is not None:
        if child.get_accessible_role() in MENU_ROLES:
            text = label_text(child)
            if text:
                child.reset_relation(Gtk.AccessibleRelation.LABELLED_BY)
                child.update_property([Gtk.AccessibleProperty.LABEL], [text])
        else:
            name_items(child)
        child = child.get_next_sibling()


def fill_page(page, container, text):
    """The widgets of `page`, as system/athanor-sal/gallery/src/main.rs adds them."""
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
    elif page == "values":
        scale = Gtk.Scale(
            orientation=Gtk.Orientation.HORIZONTAL,
            adjustment=Gtk.Adjustment(value=50, lower=0, upper=100, step_increment=1, page_increment=10),
        )
        scale.set_draw_value(False)
        scale.update_property([Gtk.AccessibleProperty.LABEL], [text["slider"]])
        container.append(scale)
    elif page == "lists":
        # The button comes first: Tab steps through the list's rows, not past them.
        replace = Gtk.Button(label=text["replace"])
        container.append(replace)
        scroll = Gtk.ScrolledWindow(
            min_content_height=200,
            max_content_height=200,
            hscrollbar_policy=Gtk.PolicyType.NEVER,
        )
        rows = Gtk.ListBox()
        rows.update_property([Gtk.AccessibleProperty.LABEL], [text["list"]])
        first = [0]
        fill_rows(rows, text, first[0])

        def again(_):
            first[0] = ROWS - first[0]
            rows.remove_all()
            fill_rows(rows, text, first[0])

        replace.connect("clicked", again)
        scroll.set_child(rows)
        container.append(scroll)
    elif page == "text":
        container.append(
            Gtk.Label(
                label=text["wrap"],
                xalign=0,
                wrap=True,
                max_width_chars=40,
                lines=3,
                ellipsize=Pango.EllipsizeMode.END,
            )
        )
        view = Gtk.TextView(wrap_mode=Gtk.WrapMode.WORD_CHAR, accepts_tab=False)
        view.get_buffer().set_text(text["notes.text"].replace("\\n", "\n"))
        view.update_property([Gtk.AccessibleProperty.LABEL], [text["notes"]])
        # Four lines tall, as the toolkit's text area.
        _, line = view.create_pango_layout("X").get_pixel_size()
        scroll = Gtk.ScrolledWindow(
            min_content_height=4 * line,
            max_content_height=4 * line,
            hscrollbar_policy=Gtk.PolicyType.NEVER,
            has_frame=True,
        )
        scroll.set_child(view)
        container.append(scroll)
    elif page == "states":
        # The button takes input once the switch is on.
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        label = Gtk.Label(label=text["enable"], xalign=0, hexpand=True)
        switch = Gtk.Switch(active=False, valign=Gtk.Align.CENTER)
        switch.update_property([Gtk.AccessibleProperty.LABEL], [text["enable"]])
        row.append(label)
        row.append(switch)
        container.append(row)
        button = Gtk.Button(label=text["waiting"], sensitive=False)
        switch.connect("notify::active", lambda s, _: button.set_sensitive(s.get_active()))
        container.append(button)
    elif page == "dialogs":
        button = Gtk.Button(label=text["dialog.open"])
        button.connect("clicked", lambda b: open_dialog(b.get_root(), text))
        container.append(button)
    elif page == "menus":
        button = Gtk.Button(label=text["menu.open"])
        button.insert_action_group("menu", menu_actions())
        popover = Gtk.PopoverMenu.new_from_model(menu_model(text))
        popover.set_parent(button)
        # The toolkit's button says it has a popup and whether it is open, as a GtkMenuButton
        # does for its own toggle.
        button.update_property([Gtk.AccessibleProperty.HAS_POPUP], [True])
        button.update_state([Gtk.AccessibleState.EXPANDED], [False])
        popover.connect("show", lambda _: button.update_state([Gtk.AccessibleState.EXPANDED], [True]))
        popover.connect("closed", lambda _: button.update_state([Gtk.AccessibleState.EXPANDED], [False]))
        # The tracker adds an entry once its action resolves, so the entries are named when shown.
        popover.connect("map", name_items)
        button.connect("clicked", lambda _: popover.popup())
        container.append(button)
        return popover
    else:
        raise ValueError(f"no page {page!r}")


def open_dialog(parent, text):
    """The dialog of the dialogs page, as the toolkit's Shell::open_dialog opens one: a modal
    window of `parent` that reads as a dialog, with a sentence that wraps and a button that
    closes it, which holds the focus. Escape closes it, as GTK's dialogs."""
    dialog = Gtk.Window(
        transient_for=parent,
        modal=True,
        title=text["dialog.title"],
        default_width=360,
        resizable=False,
        accessible_role=Gtk.AccessibleRole.DIALOG,
    )
    content = Gtk.Box(
        orientation=Gtk.Orientation.VERTICAL,
        spacing=18,
        margin_top=24,
        margin_bottom=24,
        margin_start=24,
        margin_end=24,
    )
    content.append(Gtk.Label(label=text["dialog.text"], xalign=0, wrap=True))
    close = Gtk.Button(label=text["dialog.close"])
    close.connect("clicked", lambda _: dialog.close())
    content.append(close)
    escape = Gtk.ShortcutController()
    escape.add_shortcut(
        Gtk.Shortcut(
            trigger=Gtk.KeyvalTrigger(keyval=Gdk.KEY_Escape, modifiers=0),
            action=Gtk.CallbackAction.new(lambda *_: dialog.close() or True),
        )
    )
    dialog.add_controller(escape)
    dialog.set_child(content)
    dialog.set_focus(close)
    dialog.present()


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
    menu = None
    for page in pages:
        container = group(text[f"page.{page}"])
        menu = fill_page(page, container, text) or menu
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
    if options.page == "dialogs":
        open_dialog(window, text)
    elif options.page == "menus":
        # Once the window is mapped: a popover opens over a mapped parent.
        GLib.idle_add(menu.popup)


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
