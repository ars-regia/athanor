"""appearance_probe.py: run on the host by bar-acceptance.sh (stages scheme and accent).

Reads a screenshot of the guest's session. `luma PNG` prints the mean luma (0 to 255) of
the top 60 rows, where the bar stands, and of the bottom 120, where the dock stands, in the
factory float preset. `near PNG #rrggbb` prints, for the same two bands, how many pixels
are within 24 of the colour in each channel: the accent of the active window's running
indicator, which the bar and the dock draw from the same row (athanor-apps).
"""

import sys

from PIL import Image, ImageStat

BAR_ROWS = 60
DOCK_ROWS = 120
TOLERANCE = 24


def bands(image):
    width, height = image.size
    return (
        image.crop((0, 0, width, BAR_ROWS)),
        image.crop((0, height - DOCK_ROWS, width, height)),
    )


def luma(path):
    top, bottom = bands(Image.open(path).convert("L"))
    print(f"{ImageStat.Stat(top).mean[0]:.0f} {ImageStat.Stat(bottom).mean[0]:.0f}")


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
    if len(sys.argv) == 3 and sys.argv[1] == "luma":
        luma(sys.argv[2])
    elif len(sys.argv) == 4 and sys.argv[1] == "near":
        near(sys.argv[2], sys.argv[3])
    else:
        sys.exit("usage: appearance_probe.py luma PNG | near PNG #rrggbb")


if __name__ == "__main__":
    main()
