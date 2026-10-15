#!/usr/bin/env python3
"""Fails when a tar stream of the image's boot content carries a UKI.

Reads a tar on stdin (the bootupd payload, /usr/lib/ostree-boot and /boot of the image) and
reports every regular file that is a PE image with a .linux section, or that is an *.efi under
EFI/Linux. A UKI belongs to sealed composefs (release 1.1); until then none may reach the ESP.
"""

import struct
import sys
import tarfile


def is_uki_pe(head):
    """True when head, the first bytes of a file, is a PE image with a .linux section."""
    if head[:2] != b"MZ" or len(head) < 0x40:
        return False
    pe = struct.unpack_from("<I", head, 0x3C)[0]
    if head[pe : pe + 4] != b"PE\0\0" or len(head) < pe + 24:
        return False
    count, opt = (
        struct.unpack_from("<H", head, pe + 6)[0],
        struct.unpack_from("<H", head, pe + 20)[0],
    )
    table = pe + 24 + opt
    return any(
        head[o : o + 8].rstrip(b"\0") == b".linux"
        for o in range(table, min(table + 40 * count, len(head) - 7), 40)
    )


def findings(stream):
    found = []
    with tarfile.open(fileobj=stream, mode="r|") as tar:
        for m in tar:
            if not m.isfile():
                continue
            name = m.name.lstrip("./")
            if "EFI/Linux/" in name and name.lower().endswith(".efi"):
                found.append(f"{name}: an EFI/Linux/*.efi")
            elif is_uki_pe(tar.extractfile(m).read(4096)):
                found.append(f"{name}: a PE image with a .linux section")
    return found


if __name__ == "__main__":
    bad = findings(sys.stdin.buffer)
    for line in bad:
        print(f"check-image-kernel: UKI in the image, {line}", file=sys.stderr)
    sys.exit(1 if bad else 0)
