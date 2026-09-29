#!/usr/bin/python3
"""float_frame.py <image>...

A float layout keeps the panel and the dock GAP px away from every output edge, so the
frame of that width along the edges of a correct capture holds only the background.
cosmic-panel 1.8.0 sometimes leaves stale, opaque content of its own in the transparent
gap of its surface: about one float capture in 10 to 20 in the rig, at fixed positions
with varying colours, gone once the panel exits and the compositor repaints. The layout
cases capture such a scene again instead of comparing it (rig.sh, capture_layout).

Exits 1 and names each image whose frame holds anything but the background colour,
which is read at the centre of the image, empty in every float layout.
"""

import subprocess
import sys

from compare import parse_count

GAP = 4


def magick(*args):
    return subprocess.run(
        ["magick", *args], capture_output=True, text=True, check=True
    ).stdout


def stale_pixels(image):
    width, height, background = magick(
        str(image), "-format", "%w %h %[pixel:p{w/2,h/2}]", "info:"
    ).split()
    inner = f"rectangle {GAP},{GAP} {int(width) - GAP - 1},{int(height) - GAP - 1}"
    # Paint the inside with the background, then count what differs from it exactly.
    count = magick(
        str(image),
        "-alpha",
        "off",
        "-fill",
        background,
        "-draw",
        inner,
        "-fill",
        "white",
        "+opaque",
        background,
        "-fill",
        "black",
        "-opaque",
        background,
        # The fills bring the alpha channel back, and an opaque alpha would add a quarter
        # of the frame to the mean.
        "-alpha",
        "off",
        "-format",
        "%[fx:round(mean*w*h)]",
        "info:",
    )
    return parse_count(count)


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    stale = 0
    for image in argv:
        found = stale_pixels(image)
        if found:
            print(f"stale {image}: {found} pixel(s) in the {GAP} px float gap")
            stale += 1
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
