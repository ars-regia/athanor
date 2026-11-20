#!/usr/bin/env python3
"""Decode the kernel log carried by a drm_panic QR code.

When Azoth panics, drm_panic paints the tail of the kernel log on screen as a QR
code pointing at https://drm-panic.fedoraproject.org/. That page decodes it in the
browser; this does the same offline, so a photographed panic can go into a bug
report as text.

The payload is the `z` query parameter: decimal digits, packed the way the QR
numeric mode packs them. Every 17 digits are one little-endian 7-byte group; a
shorter trailing group carries len * 2 // 5 bytes. The bytes that come out are a
zlib stream, and that stream is the log.

Usage:
    scripts/decode-drm-panic.py 'https://drm-panic.fedoraproject.org/#?a=x86_64&v=6.17.0&z=123...'
    scripts/decode-drm-panic.py 123...   # only the z payload
    scripts/decode-drm-panic.py -        # either of the two, read from stdin
"""

import sys
import urllib.parse
import zlib

GROUP_DIGITS = 17
GROUP_BYTES = 7


def unpack(z):
    """Turn the digits of the `z` parameter back into the compressed byte stream."""
    if not z:
        raise ValueError("empty payload: no z= value to decode")
    if not z.isdigit():
        bad = next(c for c in z if not c.isdigit())
        raise ValueError(f"the z payload must be decimal digits, found {bad!r}")

    out = bytearray()
    for start in range(0, len(z), GROUP_DIGITS):
        group = z[start:start + GROUP_DIGITS]
        width = GROUP_BYTES if len(group) == GROUP_DIGITS else len(group) * 2 // 5
        if width == 0:
            raise ValueError(f"trailing group of {len(group)} digits at offset {start} "
                             f"is too short to carry a byte")
        n = int(group)
        for _ in range(width):
            out.append(n % 256)
            n //= 256
        if n:
            raise ValueError(f"the group at digit {start} does not fit in {width} bytes: "
                             f"the payload is not a drm_panic QR code")
    return bytes(out)


def decode(z):
    """Return the kernel log the `z` payload carries."""
    try:
        return zlib.decompress(unpack(z))
    except zlib.error as e:
        raise ValueError(f"the payload is not a zlib stream: {e}") from None


def read_source(text):
    """Split the argument into its `z` payload and the other fields around it.

    Accepts the whole panic URL — where the fields live in the fragment, not the
    query — or the bare payload.
    """
    text = text.strip()
    if "=" not in text:
        return text, {}

    url = urllib.parse.urlparse(text)
    fields = urllib.parse.parse_qs((url.fragment or url.query).lstrip("?"))
    if "z" not in fields:
        raise ValueError("no z= parameter in that URL: there is nothing to decode")
    return fields["z"][0], {k: v[0] for k, v in fields.items()}


def main(argv):
    if len(argv) != 1 or argv[0] in ("-h", "--help"):
        print(__doc__.strip(), file=sys.stderr)
        return 2

    source = sys.stdin.read() if argv[0] == "-" else argv[0]
    try:
        z, fields = read_source(source)
        log = decode(z)
    except ValueError as e:
        print(f"decode-drm-panic: {e}", file=sys.stderr)
        return 1

    print(f"# drm_panic  architecture: {fields.get('a', 'unknown')}  "
          f"kernel: {fields.get('v', 'unknown')}")
    text = log.decode("utf-8", "replace")
    sys.stdout.write(text if text.endswith("\n") else text + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
