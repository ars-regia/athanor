"""Loads tokens.toml and resolves each variant to concrete colours."""
import tomllib
from pathlib import Path

from color import FOREGROUND_STEP, contrast, foreground, from_hex, from_oklab, resolve, to_hex, to_oklab

HERE = Path(__file__).resolve().parent
VARIANTS = ("light", "dark", "light-hc", "dark-hc")
# The surfaces the accent's foreground is drawn on, and must reach 3:1 against (#571).
SURFACES = ("surf", "surf2", "bar", "chip")
WHITE = from_hex("#ffffff")


def load(path=HERE / "tokens.toml"):
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def on_accent(tokens, accent):
    """The text on `accent` (VL3): white when it reaches 4.5:1, otherwise the dark variant's
    `acc_ink`, as appearance.rs `on_accent` chooses it at run time."""
    if contrast(accent, WHITE) >= 4.5:
        return WHITE
    return colors(tokens, "dark")["acc_ink"]


def fill(tokens, accent):
    """The accent as the fill of a control, under its text: the accent itself, unless neither
    ink reaches 4.5:1 on it; then its OKLab lightness is lowered in steps until white does.
    Of libadwaita's nine presets only red (#e62d42) is darkened, to #e2283f (maintainer
    decision, 2026-10-17, #571). appearance.rs `fill` is the same rule."""
    dark_ink = colors(tokens, "dark")["acc_ink"]
    if max(contrast(accent, WHITE), contrast(accent, dark_ink)) >= 4.5:
        return accent
    lightness, a, b = to_oklab(accent)
    for step in range(1, round(1 / FOREGROUND_STEP) + 1):
        candidate = from_hex(to_hex(from_oklab((lightness - step * FOREGROUND_STEP, a, b))))
        if contrast(candidate, WHITE) >= 4.5:
            return candidate
    raise ValueError(f"no lightness of {to_hex(accent)} carries white text at 4.5:1")


def colors(tokens, variant, accent=None):
    """{name: (r, g, b, a)} for one variant, with `inherits` applied. `accent`, an opaque
    colour, replaces the variant's `acc` (through `fill`) and `acc_ink`, as a preset does at
    run time (appearance.rs `accent_css`). `acc_fg`, the accent drawn as a line, a ring, an
    indicator or a thin fill, is derived from `acc` (color.foreground)."""
    section = tokens["variant"][variant]
    specs = {}
    if "inherits" in section:
        specs.update(tokens["variant"][section["inherits"]]["color"])
    specs.update(section.get("color", {}))
    hue, saturation = tokens["accent"]["hue"], tokens["accent"]["saturation"]
    palette = {name: resolve(spec, hue, saturation) for name, spec in specs.items()}
    if accent is not None:
        palette["acc"] = fill(tokens, accent)
        palette["acc_ink"] = on_accent(tokens, palette["acc"])
    if "acc" in palette:
        dark = variant.startswith("dark")
        palette["acc_fg"] = foreground(palette["acc"], [palette[n] for n in SURFACES], dark)
    return palette


def presets(tokens):
    """{name: colour} of libadwaita's nine accent presets."""
    return {name: from_hex(spec) for name, spec in tokens["accent"].get("presets", {}).items()}
