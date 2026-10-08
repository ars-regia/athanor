#!/usr/bin/python3
"""Fail when the units enabled in a system image disagree with forge/config/contacts.toml.

Usage: check_image_contacts.py CONTACTS_TOML [IMAGE_ROOT]
system/check-image-contacts.sh runs it inside a built image (IMAGE_ROOT defaults to /).

Scope. A unit is enabled when a *.wants or *.requires directory of /etc/systemd/system,
/usr/lib/systemd/system, /etc/systemd/user or /usr/lib/systemd/user holds it. It is masked, and
left out, when the same name in /etc/systemd/system (or /etc/systemd/user, for a user unit) is a
symlink to /dev/null or an empty file. Two defects fail:
  - an enabled timer that the list gives as neither a contact, a local timer nor an inert one,
    which is how a Fedora update adds a beacon;
  - a unit the list calls silent that is enabled.
The check sees timers and the listed units only. It does not follow Wants=, Requires= or
Upholds= written inside another unit, nor sockets, paths or D-Bus activation, so a beacon pulled
in that way is not seen. scripts/verify.py contacts makes the same comparison against the presets
of the checkout.
"""

import pathlib
import sys
import tomllib


SCOPES = (("etc/systemd/system", "usr/lib/systemd/system"), ("etc/systemd/user", "usr/lib/systemd/user"))


def enabled_units(root):
    """Names of the units some *.wants or *.requires directory of the image enables, masked ones left out."""
    units = set()
    for etc, lib in SCOPES:
        for base in (etc, lib):
            for pattern in ("*.wants", "*.requires"):
                for d in (root / base).glob(pattern):
                    if d.is_dir():
                        units |= {p.name for p in d.iterdir() if not is_masked(root / etc, p.name)}
    return units


def is_masked(etc_dir, unit):
    """systemd masks a unit by a symlink to /dev/null or by an empty file in /etc."""
    path = etc_dir / unit
    if path.is_symlink():
        return str(path.readlink()).endswith("/dev/null")
    return path.is_file() and path.stat().st_size == 0


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
