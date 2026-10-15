#!/usr/bin/env python3
"""Fails when a tar stream of the image's boot content carries a UKI.

Reads a tar on stdin (the bootupd payload, /usr/lib/ostree-boot and /boot of the image) and
reports every regular file that is a PE image with a .linux section, or that is an *.efi under
EFI/Linux. A UKI belongs to sealed composefs (release 1.1); until then none may reach the ESP.
"""

import struct
import sys
import tarfile


CAP = 64 * 1024


def is_uki_pe(read):
    """True when the PE image read(n) returns has a .linux section; None past the header cap."""
    head = read(4096)
    if head[:2] != b"MZ" or len(head) < 0x40:
        return False
    pe = struct.unpack_from("<I", head, 0x3C)[0]
    if pe + 24 > CAP:
        return None
    head += read(pe + 24 - len(head)) if len(head) < pe + 24 else b""
    if head[pe : pe + 4] != b"PE\0\0" or len(head) < pe + 24:
        return False
    count, opt = (
        struct.unpack_from("<H", head, pe + 6)[0],
        struct.unpack_from("<H", head, pe + 20)[0],
    )
    table = pe + 24 + opt
    end = table + 40 * count
    if end > CAP:
        return None
    head += read(end - len(head)) if len(head) < end else b""
    return any(
        head[o : o + 8].rstrip(b"\0") == b".linux"
        for o in range(table, min(end, len(head) - 7), 40)
    )


def findings(stream):
    found = []
    with tarfile.open(fileobj=stream, mode="r|") as tar:
        for m in tar:
            if not m.isfile():
                continue
            name = m.name.removeprefix("./")
            if "/efi/linux/" in "/" + name.lower() and name.lower().endswith(".efi"):
                found.append(f"{name}: an EFI/Linux/*.efi")
                continue
            uki = is_uki_pe(tar.extractfile(m).read)
            if uki is None:
                found.append(f"{name}: PE header past 64 KiB")
            elif uki:
                found.append(f"{name}: a PE image with a .linux section")
    return found


if __name__ == "__main__":
    try:
        bad = findings(sys.stdin.buffer)
    except (tarfile.TarError, OSError) as err:
        print(
            f"check-image-kernel: could not read the image's boot content: {err}",
            file=sys.stderr,
        )
        sys.exit(2)
    for line in bad:
        print(f"check-image-kernel: UKI in the image, {line}", file=sys.stderr)
    sys.exit(1 if bad else 0)
