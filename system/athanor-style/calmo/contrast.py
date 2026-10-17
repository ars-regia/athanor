"""WCAG AA gate over every declared pair of the tokens, in the four variants."""

import sys

import math

import tokens as tk
from color import contrast, from_oklab, over, to_oklab

THRESHOLD = {"text": 4.5, "large": 3.0, "ui": 3.0}
# VL11's sweep of the hue circle: 15 degree steps at the lowest chroma VL3 allows.
SWEEP_STEP = 15
SWEEP_CHROMA = 0.08


def accents(tokens):
    """[(label, colour)]: the nine presets, then the sweep. The sweep's two lightness limits
    are the darkest and the lightest of the presets in OKLab: the range a fixed accent takes.
    VL3's wallpaper correction (VL12 step 4) will narrow them to the lightness it allows."""
    named = tk.presets(tokens)
    if not named:
        return []
    lightness = [to_oklab(c)[0] for c in named.values()]
    cases = list(named.items())
    for limit in (min(lightness), max(lightness)):
        for hue in range(0, 360, SWEEP_STEP):
            angle = math.radians(hue)
            lab = (limit, SWEEP_CHROMA * math.cos(angle), SWEEP_CHROMA * math.sin(angle))
            cases.append((f"h{hue} L{limit:.2f}", from_oklab(lab)))
    return cases


def expand(pairs):
    """Pairs with each = true become one pair per background."""
    for pair in pairs:
        if pair.get("each"):
            for bg in pair["bg"]:
                yield {**pair, "bg": bg, "each": False}
        else:
            yield pair


def flatten(names, palette):
    """One name, or a bottom-to-top list of names, to an opaque colour."""
    names = [names] if isinstance(names, str) else names
    result = palette[names[0]]
    if result[3] != 1.0:
        raise ValueError(f"the bottom layer must be opaque: {names[0]}")
    for name in names[1:]:
        result = over(palette[name], result)
    return result


def palettes(tokens):
    """[(label, variant, palette)]: each variant with its own accent, then the light and the
    dark variant with each accent of `accents`."""
    cases = [(variant, variant, tk.colors(tokens, variant)) for variant in tk.VARIANTS]
    for variant in ("light", "dark"):
        if "acc" not in cases[tk.VARIANTS.index(variant)][2]:
            continue
        for label, accent in accents(tokens):
            cases.append((f"{variant} {label}", variant, tk.colors(tokens, variant, accent)))
    return cases


def check(tokens):
    """[(case, fg, bg, kind, ratio, needed)] for every pair, failures included."""
    rows = []
    for case, variant, palette in palettes(tokens):
        for pair in expand(tokens["pair"]):
            if "only" in pair and variant not in pair["only"]:
                continue
            bg = flatten(pair["bg"], palette)
            fg = over(palette[pair["fg"]], bg)
            needed = pair.get("min", THRESHOLD[pair["kind"]])
            rows.append(
                (
                    case,
                    pair["fg"],
                    pair["bg"],
                    pair["kind"],
                    contrast(fg, bg),
                    needed,
                )
            )
    return rows


def main():
    failures = 0
    accented = 0
    for case, fg, bg, kind, ratio, needed in check(tk.load()):
        ok = ratio >= needed
        failures += not ok
        # The accent cases print only what fails: they are some five thousand rows.
        if case in tk.VARIANTS or not ok:
            print(
                f"{'ok  ' if ok else 'FAIL'} {case:18s} {fg:13s} on {str(bg):34s} {kind:5s} {ratio:5.2f} (needs {needed})"
            )
        accented += case not in tk.VARIANTS
    print(f"{accented} pair(s) checked with the presets and the hue sweep")
    print(f"{failures} failing pair(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
