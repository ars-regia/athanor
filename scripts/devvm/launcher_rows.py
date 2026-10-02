"""launcher_rows.py: run in the guest session by launcher-acceptance.sh, as
`python3 - < launcher_rows.py`. Prints, for the shown launcher, four sections separated
by marker lines:

  the accessible name of each row of the list (LIST_ITEM), then "--";
  every label's text (the preview's facts among them), then "==";
  the labels below a Region (GTK's role of the preview, "filler" in AT-SPI), then "##";
  the name of each list (LIST) and each Region, and the size of each image that is showing
  ("image: WIDTHxHEIGHT": the preview's picture is large, its icon is not).

Prints nothing when the launcher is hidden or absent."""

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def walk(node, out, in_region=False):
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is None:
            continue
        role = child.get_role()
        below = in_region or role == Atspi.Role.FILLER
        if role == Atspi.Role.LIST_ITEM:
            out["rows"].append(child.get_name())
        elif role == Atspi.Role.LABEL:
            out["labels"].append(child.get_name())
            if in_region:
                out["region"].append(child.get_name())
        elif role in (Atspi.Role.LIST, Atspi.Role.FILLER):
            out["lists"].append(f"{role.value_nick}: {child.get_name()}")
        elif role == Atspi.Role.IMAGE and child.get_state_set().contains(Atspi.StateType.SHOWING):
            size = child.get_extents(Atspi.CoordType.SCREEN)
            out["lists"].append(f"image: {size.width}x{size.height}")
        walk(child, out, below)


def main():
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app is not None and app.get_name() == "athanor-launcher":
            out = {"rows": [], "labels": [], "region": [], "lists": []}
            walk(app, out)
            if out["rows"]:
                for key, marker in (("rows", "--"), ("labels", "=="), ("region", "##"), ("lists", None)):
                    print("\n".join(out[key]))
                    if marker:
                        print(marker)
            return


if __name__ == "__main__":
    main()
