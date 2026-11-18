#!/usr/bin/python3
"""Fail when the system image holds an RPM that ADR-0141 decision 7 refuses and
forge/config/own-allow.txt does not list, or when the list names an RPM the image no longer holds.

Usage: rpm -qa --qf '%{NAME}\\n' | check_image_own.py ALLOW_LIST
system/check-image-own.sh runs it inside a built image. scripts/verify.py own loads it for
parse_allow_list and checks the crate entries of the same list.
"""

import re
import sys

# The RPMs decision 7 refuses in the image.
REFUSED_RPM = re.compile(
    r"cosmic-.+|pop-launcher|gtk-layer-shell|gtk4-layer-shell|xdg-user-dirs-gtk"
)
ENTRY = re.compile(r"(rpm|crate)\s+(\S+)\s+(#\d+)")


def parse_allow_list(text):
    """[(kind, name, issue)] from the text of the list; raises ValueError on a malformed line,
    on a name listed twice and on an rpm entry decision 7 does not refuse."""
    entries, seen = [], set()
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        match = ENTRY.fullmatch(line)
        if not match:
            raise ValueError(f"line {number}: expected `rpm|crate NAME #ISSUE`: {line}")
        kind, name, issue = match.groups()
        if (kind, name) in seen:
            raise ValueError(f"line {number}: {kind} {name} is listed twice")
        if kind == "rpm" and not REFUSED_RPM.fullmatch(name):
            raise ValueError(
                f"line {number}: decision 7 does not refuse the RPM {name}"
            )
        seen.add((kind, name))
        entries.append((kind, name, issue))
    return entries


def rpm_problems(installed, entries):
    """What is wrong between the RPM names INSTALLED in the image and the rpm ENTRIES."""
    listed = {name for kind, name, _ in entries if kind == "rpm"}
    refused = {name for name in installed if REFUSED_RPM.fullmatch(name)}
    return [
        f"{name}: in the image, refused by ADR-0141 decision 7 and not in the allow-list"
        for name in sorted(refused - listed)
    ] + [
        f"{name}: listed in the allow-list but not in the image; remove its entry"
        for name in sorted(listed - refused)
    ]


def main(argv):
    if len(argv) != 2:
        print("usage: check_image_own.py ALLOW_LIST", file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as f:
        entries = parse_allow_list(f.read())
    problems = rpm_problems(
        {line.strip() for line in sys.stdin if line.strip()}, entries
    )
    for problem in problems:
        print(f"check_image_own: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
