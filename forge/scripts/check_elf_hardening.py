#!/usr/bin/env python3
"""Fail a built RPM whose /usr/bin/athanor-* executables lack the hardening of the workspace's
build policy (#1350; ADR-0075 decision 3, ADR-0143 decision 2).

build_spec.sh runs it on every RPM after rpmbuild. Each ELF file the RPM ships as
/usr/bin/athanor-* must be:

- a position-independent executable: ELF type ET_DYN with a PT_INTERP program header (a shared
  library is ET_DYN without one);
- linked with full RELRO: a PT_GNU_RELRO program header, and immediate binding, DF_BIND_NOW in
  DT_FLAGS or DF_1_NOW in DT_FLAGS_1 (ld's `-z relro -z now`);
- built with frame pointers (`-C force-frame-pointers=yes`). The binaries are stripped, so the
  test reads the call frame information of `.eh_frame`, which unwinding keeps: a function with a
  frame pointer sets its canonical frame address on %rbp, DWARF register 6 of the x86-64 psABI
  (section 3.6.2), with DW_CFA_def_cfa or DW_CFA_def_cfa_register (DWARF 5, section 6.4.2.2).
  At least FRAME_POINTER_SHARE of the frame descriptions must do so: code the build's flags do
  not reach (a precompiled standard library, assembly) may have none. Measured on 2026-11-25:
  RPMs built under the macros' RUSTFLAGS, without frame pointers, had 3 to 10 % (athanor-sulphur
  639 of 21875; athanor-shelld 541 of 6028, the count binutils' `readelf --debug-dump=frames`
  gives too), and release builds with the flags of .cargo/config.toml 94 to 99 %
  (athanor-sulphur 12867 of 13020, athanor-bar 2115 of 2246).

Files that are not ELF (shell wrappers such as athanor-sulphur-session) are skipped. Usage:
check_elf_hardening.py RPM... ; exit 1 on a finding, 2 on a file it cannot read.
"""

import struct
import subprocess
import sys
import tempfile
from pathlib import Path

FRAME_POINTER_SHARE = 0.5
PREFIX = "usr/bin/athanor-"

ET_DYN = 3
PT_DYNAMIC, PT_INTERP, PT_GNU_RELRO = 2, 3, 0x6474E552
DT_NULL, DT_FLAGS, DT_FLAGS_1 = 0, 30, 0x6FFFFFFB
DF_BIND_NOW, DF_1_NOW = 0x8, 0x1
RBP = 6


class ElfError(ValueError):
    pass


def uleb(data, pos):
    result = shift = 0
    while True:
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        shift += 7
        if byte < 0x80:
            return result, pos


def sleb(data, pos):
    result = shift = 0
    while True:
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        shift += 7
        if byte < 0x80:
            if byte & 0x40:
                result -= 1 << shift
            return result, pos


# DW_EH_PE_* value formats (low nibble) by size; absptr is 8 bytes on x86-64.
POINTER_SIZE = {0x0: 8, 0x2: 2, 0x3: 4, 0x4: 8, 0xA: 2, 0xB: 4, 0xC: 8}


def pointer_size(encoding):
    if encoding == 0xFF:
        return 0
    try:
        return POINTER_SIZE[encoding & 0x0F]
    except KeyError:
        raise ElfError(
            f"unsupported pointer encoding {encoding:#x} in .eh_frame"
        ) from None


def sets_cfa_on_rbp(data, pos, end):
    """Whether the call frame instructions in data[pos:end] put the CFA on %rbp."""
    while pos < end:
        op = data[pos]
        pos += 1
        high = op & 0xC0
        if high == 0x40 or high == 0xC0:  # advance_loc, restore
            continue
        if high == 0x80:  # offset: register in low bits, ULEB offset
            _, pos = uleb(data, pos)
            continue
        if op == 0x0C:  # def_cfa: register, offset
            reg, pos = uleb(data, pos)
            _, pos = uleb(data, pos)
            if reg == RBP:
                return True
        elif op == 0x12:  # def_cfa_sf: register, signed offset
            reg, pos = uleb(data, pos)
            _, pos = sleb(data, pos)
            if reg == RBP:
                return True
        elif op == 0x0D:  # def_cfa_register
            reg, pos = uleb(data, pos)
            if reg == RBP:
                return True
        elif op in (0x00, 0x0A, 0x0B):  # nop, remember_state, restore_state
            pass
        elif op == 0x01:  # set_loc: an address, which only this FDE's encoding sizes
            return False
        elif op in (0x02, 0x03, 0x04):  # advance_loc1, 2, 4
            pos += {0x02: 1, 0x03: 2, 0x04: 4}[op]
        elif op in (0x06, 0x07, 0x08, 0x0E, 0x2E):  # one ULEB
            _, pos = uleb(data, pos)
        elif op in (0x05, 0x09, 0x14, 0x2F):  # two ULEBs
            _, pos = uleb(data, pos)
            _, pos = uleb(data, pos)
        elif op in (0x11, 0x15):  # ULEB, SLEB
            _, pos = uleb(data, pos)
            _, pos = sleb(data, pos)
        elif op == 0x13:  # def_cfa_offset_sf
            _, pos = sleb(data, pos)
        elif op == 0x0F:  # def_cfa_expression: block
            length, pos = uleb(data, pos)
            pos += length
        elif op in (0x10, 0x16):  # expression, val_expression: register, block
            _, pos = uleb(data, pos)
            length, pos = uleb(data, pos)
            pos += length
        else:
            raise ElfError(f"unknown call frame instruction {op:#x} in .eh_frame")
    return False


def frame_pointer_share(eh_frame):
    """(FDEs whose CFA moves to %rbp, all FDEs) of an .eh_frame section's bytes."""
    data, pos, cies, hits, total = eh_frame, 0, {}, 0, 0
    while pos + 4 <= len(data):
        start = pos
        (length,) = struct.unpack_from("<I", data, pos)
        pos += 4
        if length == 0:  # terminator
            break
        if length == 0xFFFFFFFF:
            raise ElfError("64-bit DWARF in .eh_frame is not supported")
        end = pos + length
        (cie_id,) = struct.unpack_from("<I", data, pos)
        if cie_id == 0:
            cies[start] = parse_cie(data, pos + 4, end)
        else:
            cie = cies.get(pos - cie_id)
            if cie is None:
                raise ElfError(f"FDE at {start:#x} names no CIE read before it")
            fde_encoding, augmented = cie
            body = pos + 4 + 2 * pointer_size(fde_encoding)
            if augmented:
                skip, body = uleb(data, body)
                body += skip
            total += 1
            hits += sets_cfa_on_rbp(data, body, end)
        pos = end
    return hits, total


def parse_cie(data, pos, end):
    """(FDE pointer encoding, has augmentation data) of the CIE whose body starts at pos."""
    version = data[pos]
    pos += 1
    nul = data.index(0, pos)
    augmentation = data[pos:nul].decode("ascii")
    pos = nul + 1
    _, pos = uleb(data, pos)  # code alignment
    _, pos = sleb(data, pos)  # data alignment
    if version == 1:
        pos += 1
    else:
        _, pos = uleb(data, pos)
    encoding = 0x00
    if augmentation.startswith("z"):
        _, pos = uleb(data, pos)
        for letter in augmentation[1:]:
            if letter == "R":
                encoding = data[pos]
                pos += 1
            elif letter == "P":
                personality = data[pos]
                pos += 1 + pointer_size(personality)
            elif letter == "L":
                pos += 1
            elif letter in "SB":
                pass
            else:
                raise ElfError(f"unknown CIE augmentation {augmentation!r}")
    return encoding, augmentation.startswith("z")


def problems(data):
    """The hardening findings of the ELF executable whose bytes are data."""
    if data[:4] != b"\x7fELF":
        raise ElfError("not an ELF file")
    if data[4] != 2 or data[5] != 1:
        raise ElfError("not a 64-bit little-endian ELF file")
    (e_type,) = struct.unpack_from("<H", data, 16)
    e_phoff, e_shoff = struct.unpack_from("<QQ", data, 32)
    e_phentsize, e_phnum, e_shentsize, e_shnum, e_shstrndx = struct.unpack_from(
        "<HHHHH", data, 54
    )
    segments = [
        struct.unpack_from("<IIQQQQQQ", data, e_phoff + i * e_phentsize)
        for i in range(e_phnum)
    ]
    types = {s[0] for s in segments}
    found = []
    if e_type != ET_DYN or PT_INTERP not in types:
        found.append("not a position-independent executable (ET_DYN with PT_INTERP)")
    if PT_GNU_RELRO not in types:
        found.append("no PT_GNU_RELRO segment (link with -z relro)")
    flags = flags_1 = 0
    for p_type, _, p_offset, _, _, p_filesz, _, _ in segments:
        if p_type != PT_DYNAMIC:
            continue
        for at in range(p_offset, p_offset + p_filesz, 16):
            tag, value = struct.unpack_from("<qQ", data, at)
            if tag == DT_NULL:
                break
            if tag == DT_FLAGS:
                flags |= value
            elif tag == DT_FLAGS_1:
                flags_1 |= value
    if not (flags & DF_BIND_NOW or flags_1 & DF_1_NOW):
        found.append("no immediate binding, DF_BIND_NOW or DF_1_NOW (link with -z now)")
    sections = [
        struct.unpack_from("<IIQQQQIIQQ", data, e_shoff + i * e_shentsize)
        for i in range(e_shnum)
    ]
    names = sections[e_shstrndx][4] if e_shnum else 0
    eh_frame = None
    for name, _, _, _, offset, size, *_ in sections:
        if data[names + name : data.index(0, names + name)] == b".eh_frame":
            eh_frame = data[offset : offset + size]
    if eh_frame is None:
        found.append("no .eh_frame section to read the frame pointers from")
    else:
        hits, total = frame_pointer_share(eh_frame)
        if total == 0 or hits / total < FRAME_POINTER_SHARE:
            found.append(
                f"frame pointers in {hits} of {total} frame descriptions, under "
                f"{FRAME_POINTER_SHARE:.0%} (build with -C force-frame-pointers=yes)"
            )
    return found


def rpm_executables(rpm, into):
    """The /usr/bin/athanor-* files of RPM, unpacked under into."""
    payload = subprocess.run(
        ["rpm2cpio", str(rpm)], check=True, capture_output=True
    ).stdout
    subprocess.run(
        ["cpio", "-idm", "--quiet", "--no-absolute-filenames", f"./{PREFIX}*"],
        cwd=into,
        input=payload,
        check=True,
        capture_output=True,
    )
    return sorted(p for p in Path(into, "usr/bin").glob("athanor-*") if p.is_file())


def main(argv):
    if not argv:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    bad = 0
    for rpm in argv:
        with tempfile.TemporaryDirectory() as tmp:
            try:
                files = rpm_executables(rpm, tmp)
            except subprocess.CalledProcessError as e:
                print(
                    f"error: {rpm}: cannot unpack: {e.stderr.decode(errors='replace').strip()}",
                    file=sys.stderr,
                )
                return 2
            for path in files:
                data = path.read_bytes()
                if data[:4] != b"\x7fELF":
                    continue
                try:
                    found = problems(data)
                except (ElfError, struct.error, IndexError, ValueError) as e:
                    print(
                        f"error: {Path(rpm).name}: /usr/bin/{path.name}: cannot read it: {e}",
                        file=sys.stderr,
                    )
                    return 2
                for finding in found:
                    print(
                        f"error: {Path(rpm).name}: /usr/bin/{path.name}: {finding}",
                        file=sys.stderr,
                    )
                    bad = 1
    return bad


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
