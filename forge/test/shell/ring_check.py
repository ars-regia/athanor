#!/usr/bin/python3
"""ring_check.py <capture.png> <x> <y> <width> <height> [--high-contrast]

doc_accessibility.md, AX8 and AX17: the focus ring of the focused control, whose extents are
the box, has a contrast of at least 3:1 against the surface beside it. The ring is 2 px wide,
3 px in high contrast, and sits 2 px outside the box (`outline-offset: 2px`). The ring band and
the surface band of the same width just beyond it are each averaged; the WCAG ratio of the two
is the contrast. Reads 8-bit RGB and RGBA PNGs, the formats grim writes, with the standard
library only.
"""

import struct
import sys
import zlib

MINIMUM = 3.0
OFFSET = 2


def ring_width(high_contrast):
    return 3 if high_contrast else 2


def read_png(data):
    """(width, height, rows of (r, g, b)) of a non-interlaced 8-bit RGB or RGBA PNG."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    position, idat, header = 8, [], None
    while position < len(data):
        length, kind = struct.unpack(">I4s", data[position : position + 8])
        body = data[position + 8 : position + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        position += 12 + length
    if header is None:
        raise ValueError("the PNG has no IHDR chunk")
    width, height, depth, colour_type, _, _, interlace = header
    if depth != 8 or colour_type not in (2, 6) or interlace:
        raise ValueError("only non-interlaced 8-bit RGB and RGBA PNGs are read")
    step = 3 if colour_type == 2 else 4
    raw, stride, rows, previous = (
        zlib.decompress(b"".join(idat)),
        width * step,
        [],
        bytearray(width * step),
    )
    for y in range(height):
        kind, line = (
            raw[y * (stride + 1)],
            bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)]),
        )
        for i in range(stride):
            left = line[i - step] if i >= step else 0
            up = previous[i]
            upper_left = previous[i - step] if i >= step else 0
            if kind == 1:
                line[i] = (line[i] + left) & 255
            elif kind == 2:
                line[i] = (line[i] + up) & 255
            elif kind == 3:
                line[i] = (line[i] + (left + up) // 2) & 255
            elif kind == 4:
                estimate = left + up - upper_left
                nearest = min(
                    (abs(estimate - c), n, c)
                    for n, c in enumerate((left, up, upper_left))
                )[2]
                line[i] = (line[i] + nearest) & 255
            elif kind:
                raise ValueError(f"unknown PNG filter {kind}")
        rows.append([tuple(line[x * step : x * step + 3]) for x in range(width)])
        previous = line
    return width, height, rows


def luminance(colour):
    def linear(channel):
        value = channel / 255
        return value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4

    red, green, blue = (linear(channel) for channel in colour)
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def band_colour(rows, width, height, box, inner, outer):
    """Mean colour of the pixels between the box grown by `inner` and by `outer` px."""
    x, y, w, h = box
    total, count = [0, 0, 0], 0
    for py in range(max(y - outer, 0), min(y + h + outer, height)):
        for px in range(max(x - outer, 0), min(x + w + outer, width)):
            distance = max(x - px, 0, px - (x + w - 1), y - py, py - (y + h - 1))
            if inner < distance <= outer:
                for channel in range(3):
                    total[channel] += rows[py][px][channel]
                count += 1
    if not count:
        raise ValueError(f"the box {box} leaves no pixel for the ring in the capture")
    return tuple(channel / count for channel in total)


def contrast(png, box, width=2):
    """WCAG contrast of the ring of `width` px around `box` (x, y, w, h) against the surface beyond it."""
    image_width, image_height, rows = read_png(png)
    ring = band_colour(rows, image_width, image_height, box, OFFSET, OFFSET + width)
    surface = band_colour(
        rows, image_width, image_height, box, OFFSET + width, OFFSET + 2 * width
    )
    light, dark = sorted((luminance(ring), luminance(surface)), reverse=True)
    return (light + 0.05) / (dark + 0.05)


def problems(png, box, high_contrast=False):
    ratio = contrast(png, box, ring_width(high_contrast))
    if ratio < MINIMUM:
        return [
            f"the focus ring has a contrast of {ratio:.2f}:1 against the surface, under {MINIMUM:g}:1"
        ]
    return []


def main(argv):
    flags = [arg for arg in argv if arg.startswith("--")]
    args = [arg for arg in argv if not arg.startswith("--")]
    if len(args) != 5 or set(flags) - {"--high-contrast"}:
        print(__doc__, file=sys.stderr)
        return 2
    with open(args[0], "rb") as file:
        found = problems(
            file.read(), tuple(int(arg) for arg in args[1:]), "--high-contrast" in flags
        )
    for line in found:
        print(f"FAIL {line}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
