#!/usr/bin/env python3
"""Compare what CachyOS gives the build with what a person reviewed (doc_kernel_build.md, section 8).

    cachyos_lock.py config LOCK GENERATED      derived.config: the options taken from CachyOS
    cachyos_lock.py protected LOCK GENERATED   cachyos-protected.sha256, cachyos-files.list: VALUE  PATH lines

Prints every entry that is new, changed or gone in GENERATED against LOCK and exits 1 when
there is one, 0 when the two agree. Comment lines and blank lines are ignored.
"""

import re
import sys

CONFIG_LINE = re.compile(r"^(?:(CONFIG_\w+)=.*|# (CONFIG_\w+) is not set)$")


def config_entries(text):
    """Map each symbol of a config fragment to its line."""
    entries = {}
    for line in text.split("\n"):  # build.sh ends lines with \n only; splitlines() splits on more
        match = CONFIG_LINE.match(line)
        if match:
            entries[match.group(1) or match.group(2)] = line
    return entries


def protected_entries(text):
    """Map each path of a `VALUE  PATH` list to its value (mode and hash, `deleted`, or a status)."""
    entries = {}
    for line in text.split("\n"):  # build.sh ends lines with \n only; splitlines() splits on more
        if not line.strip() or line.startswith("#"):
            continue
        digest, sep, path = line.partition("  ")
        if not sep or not path:
            raise ValueError(f"malformed line: {line!r}")
        if path in entries:
            raise ValueError(f"path listed twice: {path!r}")
        entries[path] = digest
    return entries


def differences(lock, generated):
    """The entries of GENERATED that are new, changed or gone against LOCK, in key order."""
    out = []
    for key in sorted(lock.keys() | generated.keys()):
        if key not in lock:
            out.append(f"new {key}: {generated[key]}")
        elif key not in generated:
            out.append(f"gone {key}: {lock[key]}")
        elif lock[key] != generated[key]:
            out.append(f"changed {key}: {lock[key]} -> {generated[key]}")
    return out


def main(argv):
    if len(argv) != 4 or argv[1] not in ("config", "protected"):
        print(__doc__, file=sys.stderr)
        return 2
    parse = config_entries if argv[1] == "config" else protected_entries
    with (
        open(argv[2], encoding="utf-8") as lock,
        open(argv[3], encoding="utf-8") as generated,
    ):
        found = differences(parse(lock.read()), parse(generated.read()))
    for line in found:
        print(f"  {line}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
