#!/usr/bin/python3
"""Fail when the units enabled in a system image disagree with forge/config/contacts.toml.

Usage: check_image_contacts.py CONTACTS_TOML [IMAGE_ROOT]
system/check-image-contacts.sh runs it inside a built image (IMAGE_ROOT defaults to /).

A unit is enabled when a *.wants directory of /etc/systemd/system or /usr/lib/systemd/system holds
it and the unit is not masked (a symlink to /dev/null in /etc/systemd/system). Two defects fail:
  - an enabled timer that the list gives as neither a contact, a local timer nor an inert one,
    which is how a Fedora update adds a beacon;
  - a unit the list calls silent that is enabled.
scripts/verify.py contacts makes the same comparison against the presets of the checkout.
"""

import pathlib
import sys
import tomllib


def enabled_units(root):
    """Names of the units some *.wants directory of the image enables, masked ones left out."""
    units = set()
    for base in ("etc/systemd/system", "usr/lib/systemd/system"):
        for wants in (root / base).glob("*.wants"):
            if wants.is_dir():
                units |= {p.name for p in wants.iterdir()}
    return {u for u in units if not is_masked(root, u)}


def is_masked(root, unit):
    path = root / "etc/systemd/system" / unit
    return path.is_symlink() and str(path.readlink()) == "/dev/null"


def problems(contacts, enabled):
    listed = {c["unit"] for c in contacts.get("contact", []) if c.get("unit")}
    for group in ("local", "inert"):
        listed |= {e["unit"] for e in contacts.get(group, [])}
    silent = {e["unit"] for e in contacts.get("silent", [])}
    found = [f"{u} is an enabled timer that contacts.toml does not list" for u in sorted(enabled)
             if u.endswith(".timer") and u not in listed and u not in silent]
    found += [f"{u} is silent in contacts.toml and enabled in the image" for u in sorted(silent & enabled)]
    return found


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    contacts = tomllib.loads(pathlib.Path(argv[1]).read_text())
    root = pathlib.Path(argv[2] if len(argv) == 3 else "/")
    found = problems(contacts, enabled_units(root))
    for line in found:
        print(line, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
