"""launcher_rows.py: run in the guest session by launcher-acceptance.sh, as
`python3 - < launcher_rows.py`. Prints, for the shown launcher, four sections separated
by marker lines:

  the accessible name of each row of the list (LIST_ITEM), then "--";
  every label's text (the preview's facts among them), then "==";
  the labels outside any list or menu: the preview's title, body and facts, and the
  footer's hint (the toolkit's pane is a generic container, which AccessKit leaves out of
  the tree, so the preview has no node of its own), then "##";
  each list box ("list box: Results") and menu ("menu: Actions", named by its popup, as
  the menu node has no name), and "title offset: N", how far in pixels the preview's title
  sits below the top of the results' viewport (a decoded picture is 220 px high, a card's
  icon 96; the toolkit leaves both out of the tree, as images without a description).

Prints nothing when the launcher is hidden or absent."""

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

LISTS = {Atspi.Role.LIST_BOX: "list box", Atspi.Role.MENU: "menu"}


def top(node):
    return node.get_extents(Atspi.CoordType.SCREEN).y


def walk(node, out, window, in_list=False):
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is None:
            continue
        role = child.get_role()
        if role == Atspi.Role.LIST_ITEM:
            out["rows"].append(child.get_name())
        elif role == Atspi.Role.LABEL:
            out["labels"].append(child.get_name())
            if not in_list:
                out["region"].append(child.get_name())
                if out["title"] is None:
                    out["title"] = top(child)
        elif role in LISTS:
            out["lists"].append(f"{LISTS[role]}: {child.get_name() or window}")
            if role == Atspi.Role.LIST_BOX and out["viewport"] is None:
                out["viewport"] = top(node)
        walk(child, out, window, in_list or role in LISTS)


def main():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-launcher":
            out = {
                "rows": [],
                "labels": [],
                "region": [],
                "lists": [],
                "title": None,
                "viewport": None,
            }
            for number in range(app.get_child_count()):
                window = app.get_child_at_index(number)
                if window is not None:
                    walk(window, out, window.get_name())
            if out["rows"]:
                if None not in (out["title"], out["viewport"]):
                    out["lists"].append(
                        f"title offset: {out['title'] - out['viewport']}"
                    )
                for key, marker in (
                    ("rows", "--"),
                    ("labels", "=="),
                    ("region", "##"),
                    ("lists", None),
                ):
                    print("\n".join(out[key]))
                    if marker:
                        print(marker)
            return


if __name__ == "__main__":
    main()
