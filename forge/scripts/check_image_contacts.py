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
A contact that names remote_file, remote_url, remote_fingerprint and remote_key_sha256 needs that
.flatpakrepo in the image to carry, in [Flatpak Repo], the Url and a GPGKey that is exactly the pinned
blob, with the pinned fingerprint and an expiry more than 90 days away, and no Filter or Subset
line (the Flathub remote, doc_software.md decision 6). A contact with must_enable = true needs its
unit enabled.
"""

import base64
import configparser
import hashlib
import pathlib
import re
import sys
import tomllib
from datetime import datetime, timedelta, timezone


EXPIRY_WARNING_DAYS = 90
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


def packets(data):
    """(tag, body) of each OpenPGP packet of data (RFC 9580 section 4), or ValueError on a malformed one."""
    pos = 0
    while pos < len(data):
        head = data[pos]
        if not head & 0x80:
            raise ValueError("not an OpenPGP packet")
        if head & 0x40:
            tag, pos = head & 0x3F, pos + 1
            first = data[pos]
            if first < 192:
                size, pos = first, pos + 1
            elif first < 224:
                size, pos = ((first - 192) << 8) + data[pos + 1] + 192, pos + 2
            elif first == 255:
                size, pos = int.from_bytes(data[pos + 1:pos + 5], "big"), pos + 5
            else:
                raise ValueError("partial packet length")
        else:
            tag, kind = (head >> 2) & 0x0F, head & 3
            width = (1, 2, 4)[kind] if kind < 3 else 0
            if not width:
                raise ValueError("indeterminate packet length")
            size, pos = int.from_bytes(data[pos + 1:pos + 1 + width], "big"), pos + 1 + width
        if pos + size > len(data):
            raise ValueError("truncated packet")
        yield tag, data[pos:pos + size]
        pos += size


def key_fingerprint(b64):
    """Fingerprint of the primary key of an OpenPGP public key given as base64 (a flatpakrepo GPGKey=),
    or None when the first packet is not a version 4 public key."""
    data = base64.b64decode(b64, validate=True)
    for tag, body in packets(data):
        if tag != 6 or body[0] != 4:
            return None
        return hashlib.sha1(b"\x99" + len(body).to_bytes(2, "big") + body).hexdigest().upper()
    return None


def key_expiry(data):
    """Earliest expiry (datetime, UTC) among the primary key and its subkeys, from the key expiration
    time of their version 4 self-signatures, or None when none of them expires."""
    created, expiries = None, []
    for tag, body in packets(data):
        if tag in (6, 14):
            created = int.from_bytes(body[1:5], "big")
        elif tag == 2 and body[0] == 4 and body[1] in (0x10, 0x11, 0x12, 0x13, 0x18) and created is not None:
            hashed, pos = body[6:6 + int.from_bytes(body[4:6], "big")], 0
            while pos < len(hashed):
                first = hashed[pos]
                if first < 192:
                    size, pos = first, pos + 1
                elif first < 255:
                    size, pos = ((first - 192) << 8) + hashed[pos + 1] + 192, pos + 2
                else:
                    size, pos = int.from_bytes(hashed[pos + 1:pos + 5], "big"), pos + 5
                if hashed[pos] & 0x7F == 9 and size == 5:
                    expiries.append(created + int.from_bytes(hashed[pos + 1:pos + 5], "big"))
                pos += size
    return datetime.fromtimestamp(min(expiries), timezone.utc) if expiries else None


def remote_problems(contacts, root, enabled, now=None):
    """Contacts that name a .flatpakrepo (remote_file, remote_url, remote_fingerprint, remote_key_sha256)
    and contacts with must_enable = true."""
    now = now or datetime.now(timezone.utc)
    found = []
    for c in contacts.get("contact", []):
        if c.get("must_enable") and c.get("unit") not in enabled:
            found.append(f"{c['id']}: {c.get('unit')} is not enabled in the image")
        if not c.get("remote_file"):
            continue
        name = c["remote_file"]
        try:
            text = (root / name).read_text()
        except OSError as err:
            found.append(f"{c['id']}: cannot read {name} in the image ({err})")
            continue
        if re.search(r"^\s*(filter|subset)\s*=", text, re.I | re.M):
            found.append(f"{c['id']}: {name} sets Filter or Subset, and the remote is unfiltered (doc_software.md, decision 6)")
        ini = configparser.ConfigParser(delimiters=("=",), interpolation=None, strict=True)
        try:
            ini.read_string(text)
            repo = ini["Flatpak Repo"]
            url, key = repo.get("Url"), repo.get("GPGKey", "")
            blob = base64.b64decode(key, validate=True)
            got = key_fingerprint(key)
            expiry = key_expiry(blob)
        except (configparser.Error, KeyError, ValueError) as err:
            found.append(f"{c['id']}: {name} is not a readable flatpakrepo with a GPGKey in [Flatpak Repo] ({err})")
            continue
        if url != c["remote_url"]:
            found.append(f"{c['id']}: {name} has Url={url}, not {c['remote_url']}")
        if got != c["remote_fingerprint"].replace(" ", ""):
            found.append(f"{c['id']}: the GPGKey of {name} has fingerprint {got}, not {c['remote_fingerprint']}")
        if hashlib.sha256(blob).hexdigest() != c["remote_key_sha256"]:
            found.append(f"{c['id']}: the GPGKey of {name} is not the pinned blob (sha256 {c['remote_key_sha256']})")
        if expiry is not None and expiry - now < timedelta(days=EXPIRY_WARNING_DAYS):
            found.append(f"{c['id']}: the GPGKey of {name} expires on {expiry:%Y-%m-%d}, within {EXPIRY_WARNING_DAYS} days: rotate it")
    return found


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    contacts = tomllib.loads(pathlib.Path(argv[1]).read_text())
    root = pathlib.Path(argv[2] if len(argv) == 3 else "/")
    enabled = enabled_units(root)
    found = problems(contacts, enabled) + config_problems(contacts, root) + remote_problems(contacts, root, enabled)
    for line in found:
        print(line, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
