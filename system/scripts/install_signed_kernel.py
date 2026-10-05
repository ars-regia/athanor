#!/usr/bin/env python3
"""Installs the vmlinuz that system/sign-kernel.sh signed in place of the kernel package's own.

Usage: install_signed_kernel.py SIGNED_DIR KVER

Runs in the image build, which holds no key (docs/architecture/doc_kernel_profile.md, D43):
the signature was made in a sign-only job, and this only checks it with the public
certificate. SIGNED_DIR holds `vmlinuz` and `certificate.pem`, placed there by
system/build-image.sh: the committed project certificate for a release, the throwaway one
for a build that is never published. Three checks, any of which fails the build:

  - the signed vmlinuz verifies against the certificate (sbverify);
  - it carries that one signature and no other, since shim accepts an image as soon as any
    one of its signatures is trusted;
  - it is the kernel the package installed at /usr/lib/modules/KVER/vmlinuz: the two files
    are equal once the parts a signature changes are left out, which are exactly the parts
    Authenticode leaves out of the image hash (the PE checksum, the certificate table entry
    and the certificate table itself), plus the zero padding a signer appends before the
    table. A vmlinuz signed for another kernel never reaches the image.

The file is then replaced with a rename in the same directory, so a symlink in its place
is replaced rather than followed.
"""

import os
import pathlib
import shutil
import struct
import subprocess
import sys

PE32_PLUS = 0x20B
PE32 = 0x10B
CERTIFICATE_TABLE = 4  # index of the security entry among the data directories


def signed_content(image: bytes) -> bytes:
    """The bytes of a PE image that no Authenticode signature changes."""
    if image[:2] != b"MZ":
        raise ValueError("not a PE image: no MZ header")
    (pe,) = struct.unpack_from("<I", image, 0x3C)
    if image[pe : pe + 4] != b"PE\0\0":
        raise ValueError("not a PE image: no PE signature")
    optional = pe + 24
    (magic,) = struct.unpack_from("<H", image, optional)
    if magic == PE32_PLUS:
        directories = optional + 112
    elif magic == PE32:
        directories = optional + 96
    else:
        raise ValueError(f"unknown optional header magic {magic:#x}")
    checksum = optional + 64
    entry = directories + 8 * CERTIFICATE_TABLE
    address, size = struct.unpack_from("<II", image, entry)
    end = address if size else len(image)
    if end > len(image):
        raise ValueError("certificate table beyond the end of the image")
    content = image[:checksum] + image[checksum + 4 : entry] + image[entry + 8 : end]
    return content.rstrip(b"\0")


def signatures(path: pathlib.Path) -> int:
    listing = subprocess.run(
        ["sbverify", "--list", str(path)], check=True, capture_output=True, text=True
    ).stdout
    return sum(line.startswith("signature ") for line in listing.splitlines())


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: install_signed_kernel.py SIGNED_DIR KVER", file=sys.stderr)
        return 2
    signed_dir, kver = pathlib.Path(sys.argv[1]), sys.argv[2]
    signed = signed_dir / "vmlinuz"
    certificate = signed_dir / "certificate.pem"
    target = pathlib.Path("/usr/lib/modules") / kver / "vmlinuz"

    subprocess.run(["sbverify", "--cert", str(certificate), str(signed)], check=True)
    count = signatures(signed)
    if count != 1:
        sys.exit(f"{signed}: {count} signatures, expected exactly 1")
    if signed_content(signed.read_bytes()) != signed_content(target.read_bytes()):
        sys.exit(f"{signed} is not the kernel the package installed at {target}")

    staged = target.with_name(".vmlinuz.signed")
    shutil.copyfile(signed, staged)
    staged.chmod(target.stat().st_mode & 0o7777)
    os.replace(staged, target)
    print(f"installed the signed vmlinuz of {kver}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
