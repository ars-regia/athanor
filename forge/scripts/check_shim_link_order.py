#!/usr/bin/env python3
"""check_shim_link_order.py <elf>

gtk4-layer-shell works by interposing libwayland-client's symbols, so the dynamic linker
must load it before libwayland-client, and upstream asks for "before GTK as well". When
it does not, nothing fails: the surface silently becomes an ordinary window with a title
bar. The order of DT_NEEDED is the load order, and today it is right only by the accident
of how cargo orders its -l flags (spike P2). This check turns the accident into a
contract: it runs in %check and fails the package.
"""

import os
import re
import subprocess
import sys

SHIM = "libgtk4-layer-shell.so"
AFTER = ("libgtk-4.so", "libwayland-client.so")


def needed(readelf_output):
    return re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", readelf_output)


def problems(libraries):
    names = [name for name in libraries if name.startswith(SHIM)]
    if not names:
        return [
            f"{SHIM} is not a direct dependency: the surface cannot be a layer surface"
        ]
    shim = libraries.index(names[0])
    return [
        f"{name} is loaded before {names[0]}: the shim would not interpose libwayland"
        for name in libraries[:shim]
        if name.startswith(AFTER)
    ]


def main(argv):
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    output = subprocess.run(
        ["readelf", "-d", argv[0]],
        check=True,
        capture_output=True,
        text=True,
        env={**os.environ, "LC_ALL": "C"},  # needed() matches the English labels
    ).stdout
    found = problems(needed(output))
    for line in found:
        print(f"{argv[0]}: {line}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
