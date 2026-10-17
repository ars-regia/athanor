"""Colour arithmetic for the Calmo tokens: HSL to sRGB, compositing, WCAG contrast."""

import colorsys


def from_hex(text):
    """'#rrggbb' -> (r, g, b, 1.0), channels in 0..1."""
    text = text.lstrip("#")
    if len(text) != 6:
        raise ValueError(f"not a #rrggbb colour: {text!r}")
    return tuple(int(text[i:i + 2], 16) / 255 for i in (0, 2, 4)) + (1.0,)


def resolve(spec, hue, saturation):
    """A token colour (hex string or table) -> (r, g, b, a)."""
    if isinstance(spec, str):
        return from_hex(spec)
    alpha = float(spec.get("a", 1.0))
    if "hex" in spec:
        return from_hex(spec["hex"])[:3] + (alpha,)
    sat = spec["s"]
    if isinstance(sat, str):
        if not sat.startswith("accent"):
            raise ValueError(f"saturation must be a number or accent[+N]: {sat!r}")
        sat = min(100.0, saturation + float(sat[len("accent"):] or 0))
    h = (hue + spec.get("dh", 0)) % 360
    r, g, b = colorsys.hls_to_rgb(h / 360, spec["l"] / 100, sat / 100)
    return (r, g, b, alpha)


def over(top, bottom):
    """Source-over compositing of `top` on an opaque `bottom`."""
    a = top[3]
    return tuple(t * a + b * (1 - a) for t, b in zip(top[:3], bottom[:3])) + (1.0,)


def to_hex(color):
    return "#%02x%02x%02x" % tuple(round(c * 255) for c in color[:3])


def luminance(color):
    def linear(u):
        return u / 12.92 if u <= 0.04045 else ((u + 0.055) / 1.055) ** 2.4

    r, g, b = (linear(c) for c in color[:3])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    """WCAG 2.1 contrast ratio of two opaque colours, 1.0 to 21.0."""
    hi, lo = sorted((luminance(a), luminance(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def _linear(u):
    return u / 12.92 if u <= 0.04045 else ((u + 0.055) / 1.055) ** 2.4


def _gamma(u):
    u = min(1.0, max(0.0, u))
    return 12.92 * u if u <= 0.0031308 else 1.055 * u ** (1 / 2.4) - 0.055


def to_oklab(color):
    """Opaque sRGB -> OKLab (L, a, b), after Björn Ottosson's reference."""
    r, g, b = (_linear(c) for c in color[:3])
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )


def from_oklab(lab):
    """OKLab -> opaque sRGB, each channel clipped to the gamut."""
    lightness, a, b = lab
    l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (lightness - 0.0894841775 * a - 1.2914855480 * b) ** 3
    return (
        _gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        _gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        _gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
        1.0,
    )


# The step of the foreground search in OKLab lightness, and the contrast it seeks (WCAG 2.1,
# 1.4.11, for lines, rings, indicators and thin fills).
FOREGROUND_STEP = 0.005
FOREGROUND_MIN = 3.0


def foreground(accent, surfaces, dark):
    """The accent drawn as a line, a ring, an indicator or a thin fill (#571): `accent` with
    its OKLab lightness raised in a dark variant and lowered in a light one, in steps, until it
    reaches 3:1 against every surface; the accent itself when it already does. Each candidate
    is rounded to 8 bits a channel, as CSS carries it. appearance.rs `foreground` is the same
    rule; its test checks it against generated/accent.rs."""
    lightness, a, b = to_oklab(accent)
    sign = 1 if dark else -1
    for step in range(round(1 / FOREGROUND_STEP) + 1):
        candidate = from_hex(
            to_hex(from_oklab((lightness + sign * step * FOREGROUND_STEP, a, b)))
        )
        if min(contrast(candidate, s) for s in surfaces) >= FOREGROUND_MIN:
            return candidate
    raise ValueError(
        f"no lightness of {to_hex(accent)} reaches {FOREGROUND_MIN}:1 on every surface"
    )
