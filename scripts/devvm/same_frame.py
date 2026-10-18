"""same_frame.py BEFORE AFTER: run on the host by launcher-acceptance.sh, on two binary
PPM captures of one output (`grim -t ppm`).

Exits 0 when at most 1% of the pixels differ, and 1 otherwise, printing the share. A
closed surface must leave the screen as it found it: a hidden surface that commits no
frame leaves the compositor showing its last one, which only pixels reveal (the
launcher's accessibility tree said it was hidden). The 1% leaves room for the bar's clock.
"""

import sys

LIMIT = 0.01


def pixels(data):
    """The size and the RGB bytes of a binary PPM with a maximum of 255."""
    # The pixel data follows exactly one whitespace byte after the maximum, and may itself
    # start with whitespace bytes, so it is cut by position, not split.
    fields = data.split(maxsplit=3)
    if (
        len(fields) < 4
        or fields[0] != b"P6"
        or fields[3][:3] != b"255"
        or not fields[3][3:4].isspace()
    ):
        sys.exit("same_frame: not a binary PPM with a maximum of 255")
    width, height = int(fields[1]), int(fields[2])
    body = fields[3][4:]
    if len(body) != width * height * 3:
        sys.exit("same_frame: the PPM is truncated")
    return (width, height), body


def changed(before, after):
    """The share of the pixels that differ between two captures of the same size."""
    size, a = pixels(before)
    other, b = pixels(after)
    if size != other:
        sys.exit(f"same_frame: the captures are {size} and {other}")
    differ = sum(1 for i in range(0, len(a), 3) if a[i : i + 3] != b[i : i + 3])
    return differ / (size[0] * size[1])


def main(argv):
    if len(argv) != 3:
        sys.exit("usage: same_frame.py BEFORE AFTER")
    with open(argv[1], "rb") as first, open(argv[2], "rb") as second:
        share = changed(first.read(), second.read())
    print(f"changed: {share:.2%} of the pixels")
    return 0 if share <= LIMIT else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
