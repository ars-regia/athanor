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
A contact that names image_file and image_contains must have that Key=value as the last active
line of the file, and no other resolved.conf.d drop-in may set the key (the resolver mode,
DNSOverTLS=opportunistic, ADR-0103 D11).
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


def last_setting(path, key):
    """The value of the last active `key=` line of a file (comments ignored), or None."""
    value = None
    for line in path.read_text().splitlines():
        name, eq, val = line.partition("=")
        if eq and name.strip() == key:
            value = val.strip()
    return value


def config_problems(contacts, root):
    """A contact with image_file and image_contains ("Key=value") needs the last active Key= line of that
    file to be the value, and no other resolved.conf.d drop-in of the image to set Key at all."""
    found = []
    for c in contacts.get("contact", []):
        if not c.get("image_file"):
            continue
        key, _, want = c.get("image_contains", "").partition("=")
        try:
            got = last_setting(root / c["image_file"], key)
        except OSError as err:
            found.append(f"{c['id']}: cannot read {c['image_file']} in the image ({err})")
            continue
        if got != want:
            found.append(f"{c['id']}: {c['image_file']} in the image sets {key}={got}, not {want}")
        for d in ("etc/systemd/resolved.conf.d", "usr/lib/systemd/resolved.conf.d"):
            for f in sorted((root / d).glob("*.conf")):
                if f != root / c["image_file"] and last_setting(f, key) is not None:
                    found.append(f"{c['id']}: {d}/{f.name} in the image sets {key}")
    return found


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    contacts = tomllib.loads(pathlib.Path(argv[1]).read_text())
    root = pathlib.Path(argv[2] if len(argv) == 3 else "/")
    found = problems(contacts, enabled_units(root)) + config_problems(contacts, root)
    for line in found:
        print(line, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
