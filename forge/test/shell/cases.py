#!/usr/bin/python3
"""The cases of doc_shell.md, SH13.

Surfaces: scale {1.0, 1.5} x theme {light, dark} x text {English, German for length, a
right-to-left pseudo-locale}. Layouts: the 27 cases of SH13 over the 14 layouts of SH7.

    cases.py <surface>              tag, variant, scale, locale, catalog (tab-separated)
    cases.py layout --outputs N     tag, preset, panel, dock, scale, width, height (tab-separated)
"""
import sys
from collections import namedtuple
from itertools import product

Case = namedtuple("Case", "tag variant scale locale catalog")
LayoutCase = namedtuple("LayoutCase", "tag preset panel dock outputs scale width height")

# short name -> (LC_ALL, catalog handed to ATHANOR_I18N_CATALOG). Our own strings come
# from the catalog file, read by athanor-i18n, whatever the process locale is; LC_ALL
# gives the case its date and GTK's own strings. English is the message ids: no catalog.
# German and the right-to-left pseudo-language are catalogs of the test; the product
# ships it and en. The pseudo-language declares "Language: ar", which is what makes the
# greeter mirror.
LOCALES = {"en": ("en_US.UTF-8", "-"), "de": ("de_DE.UTF-8", "de.mo"), "rtl": ("ar_EG.UTF-8", "rtl.mo")}
SURFACES = {
    "greeter": {"variants": ("light", "dark"), "scales": ("1.0", "1.5")},
    "chooser": {"variants": ("light", "dark"), "scales": ("1.0", "1.5")},
}

# SH7: factory knobs per preset; "-" is "no dock knob" (the bar).
FACTORY = {"float": ("top", "visible"), "bar": ("bottom", "-"), "minimal": ("top", "none")}
PANELS = ("top", "bottom")
DOCKS = ("visible", "auto-hide", "none")
LANDSCAPE = (1920, 1080)
PORTRAIT = (1080, 1920)


def surface_cases(surface):
    spec = SURFACES[surface]
    return [Case(f"{surface}-{variant}-{scale}-{short}", variant, scale, locale, catalog)
            for variant, scale, (short, (locale, catalog)) in product(spec["variants"], spec["scales"], LOCALES.items())]


def all_layouts():
    """The 14 layouts of SH7, as (preset, panel, dock)."""
    return [(preset, panel, dock) for preset in FACTORY for panel in PANELS
            for dock in (("-",) if FACTORY[preset][1] == "-" else DOCKS)]


def _case(preset, panel, dock, outputs, scale, size):
    shape = "port" if size[1] > size[0] else "land"
    knobs = f"{panel}" if dock == "-" else f"{panel}-{dock}"
    return LayoutCase(f"layout-{preset}-{knobs}-{outputs}o-{scale}-{shape}", preset, panel, dock, outputs, scale, *size)


def layout_cases():
    factory = [(preset, *FACTORY[preset]) for preset in FACTORY]
    found = [_case(*layout, outputs, scale, LANDSCAPE)
             for layout, outputs, scale in product(factory, (1, 2), ("1.0", "1.5"))]
    found += [_case(*layout, 1, "1.0", LANDSCAPE) for layout in all_layouts() if layout not in factory]
    found += [_case(*layout, 1, "1.0", PORTRAIT) for layout in factory + [("float", "bottom", "visible")]]
    return found


if __name__ == "__main__":
    args = sys.argv[1:]
    if len(args) == 3 and args[0] == "layout" and args[1] == "--outputs" and args[2] in ("1", "2"):
        for case in layout_cases():
            if case.outputs == int(args[2]):
                print("\t".join([case.tag, case.preset, case.panel, case.dock, case.scale, str(case.width), str(case.height)]))
        sys.exit(0)
    if len(args) != 1 or args[0] not in SURFACES:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    for case in surface_cases(args[0]):
        print("\t".join(case))
