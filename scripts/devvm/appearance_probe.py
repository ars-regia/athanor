"""appearance_probe.py: run on the host by bar-acceptance.sh (stages scheme and accent).

Reads screenshots of the guest's session, in two bands: the top 60 rows, where the bar
stands, and the bottom 120, where the dock stands, in the factory float preset.
`drop FROM TO` prints, for each band, the mean luma drop (0 to 255) from FROM to TO over the
pixels whose luma changed by more than 8, and 0 when none did: the wallpaper around the
islands does not change, so the measure does not depend on the screen's width. `near PNG
#rrggbb` prints, for the same two bands, how many pixels
are within 24 of the colour in each channel: the accent of the active window's running
indicator, which the bar and the dock draw from the same row (athanor-apps).
"""

import sys

from PIL import Image

BAR_ROWS = 60
DOCK_ROWS = 120
TOLERANCE = 24
CHANGED = 8


def bands(image):
    width, height = image.size
    return (
        image.crop((0, 0, width, BAR_ROWS)),
        image.crop((0, height - DOCK_ROWS, width, height)),
    )


def drop(before, after):
    def mean_drop(old, new):
        changed = [
            a - b for a, b in zip(old.getdata(), new.getdata()) if abs(a - b) > CHANGED
        ]
        return sum(changed) / len(changed) if changed else 0

    old = bands(Image.open(before).convert("L"))
    new = bands(Image.open(after).convert("L"))
    print(" ".join(f"{mean_drop(a, b):.0f}" for a, b in zip(old, new)))


def near(path, colour):
    target = [int(colour[index : index + 2], 16) for index in (1, 3, 5)]

    def count(band):
        return sum(
            1
            for pixel in band.getdata()
            if all(
                abs(channel - want) <= TOLERANCE for channel, want in zip(pixel, target)
            )
        )

    top, bottom = bands(Image.open(path).convert("RGB"))
    print(f"{count(top)} {count(bottom)}")


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "drop":
        drop(sys.argv[2], sys.argv[3])
    elif len(sys.argv) == 4 and sys.argv[1] == "near":
        near(sys.argv[2], sys.argv[3])
    else:
        sys.exit("usage: appearance_probe.py drop FROM TO | near PNG #rrggbb")


if __name__ == "__main__":
    main()
