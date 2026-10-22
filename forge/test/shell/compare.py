#!/usr/bin/python3
"""compare.py <golden-dir> <actual-dir> <tag>...

A case passes when at most TOLERANCE pixels differ from its golden image by more than
a 2 % colour distance. The count is read from the difference mask `magick compare`
writes, not from its AE metric: ImageMagick 7.1.2 prints AE multiplied by the quantum
range (65535 per pixel in Q16), older releases print the plain count. Spike P3 measured 0 between runs on one
machine; llvmpipe chooses its code path from the host CPU, so 64 pixels, 0.003 % of a
1920x1080 frame, is allowed between runner generations. The smallest thing that can move
on a surface, a 12 px badge, is 113 pixels.

A surface whose smallest mark is smaller has its own tolerance, named by its golden
directory: the dock's running indicator is 2 px across a 32 px button at scale 1.0, 64
pixels, so a dock case allows 16.
"""
import re
import subprocess
import sys
from pathlib import Path

TOLERANCE = 64
TOLERANCES = {"dock": 16}


def tolerance(golden_dir):
    return TOLERANCES.get(golden_dir.name, TOLERANCE)


def parse_count(text):
    match = re.match(r"\s*([0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?)", text)
    if not match:
        raise ValueError(f"not a pixel count: {text!r}")
    return int(float(match.group(1)))


def verdict(differing, allowed=TOLERANCE):
    return differing <= allowed


def differing_pixels(golden, actual, diff):
    # The mask is white where the images differ and black elsewhere. compare exits 1 when
    # the images differ: that is an answer, not a failure.
    result = subprocess.run(["magick", "compare", "-fuzz", "2%", "-highlight-color", "white",
                             "-lowlight-color", "black", "-compose", "src", str(golden), str(actual), str(diff)],
                            capture_output=True, text=True, check=False)
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.strip())
    count = subprocess.run(["magick", str(diff), "-format", "%[fx:round(mean*w*h)]", "info:"],
                           capture_output=True, text=True, check=True)
    return parse_count(count.stdout)


def main(argv):
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    golden_dir, actual_dir, tags = Path(argv[0]), Path(argv[1]), argv[2:]
    allowed = tolerance(golden_dir)
    failed = 0
    for tag in tags:
        golden, actual = golden_dir / f"{tag}.png", actual_dir / f"{tag}.png"
        if not golden.exists():
            print(f"FAIL {tag}: no golden image; run rig.sh update-goldens and review it")
            failed += 1
            continue
        differing = differing_pixels(golden, actual, actual_dir / f"{tag}-diff.png")
        ok = verdict(differing, allowed)
        failed += not ok
        print(f"{'ok  ' if ok else 'FAIL'} {tag}: {differing} pixel(s) differ (tolerance {allowed})")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
