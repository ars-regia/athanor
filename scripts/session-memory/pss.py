"""Sum of the proportional set size of the user's session, read inside the guest.

Standard library only, and small on purpose: forge/test/iso/memory_case.py sends this file
to the guest over the serial console, which carries no checkout. It prints the report as a
framed block that report.py reads back from the console log and verifies:

    PSS_BEGIN <bytes> <crc32 hex>
    PSS_D <index> <76 characters of base64 of the JSON>      (one line per chunk)
    PSS_END

The kernel and systemd write to the same console and can cut into a line; the byte count
and the checksum make a damaged block detectable, and the block is printed twice so one
damaged copy is not the end of the measurement.

The processes counted are those of the user's slice (user-<uid>.slice: the user manager,
which holds the desktop, and the session scopes), minus the scope this script itself runs
in, which is the serial login that asked.
"""

import base64
import json
import os
import pathlib
import zlib


def collect(uid=None, root="/sys/fs/cgroup", proc="/proc"):
    uid = os.getuid() if uid is None else uid
    root, proc = pathlib.Path(root), pathlib.Path(proc)
    own = (proc / "self" / "cgroup").read_text().strip().rsplit("::", 1)[-1].lstrip("/")
    rows, unreadable = [], 0
    for procs in sorted(
        (root / "user.slice" / f"user-{uid}.slice").rglob("cgroup.procs")
    ):
        unit = str(procs.parent.relative_to(root))
        if unit == own:
            continue
        try:
            pids = procs.read_text().split()
        except FileNotFoundError:
            continue  # a transient scope ended after the walk found it
        for pid in pids:
            try:
                comm = (proc / pid / "comm").read_text().strip()
                for line in (proc / pid / "smaps_rollup").read_text().splitlines():
                    if line.startswith("Pss:"):
                        rows.append(
                            {
                                "pid": int(pid),
                                "comm": comm,
                                "unit": unit,
                                "pss_kb": int(line.split()[1]),
                            }
                        )
                        break
            except (FileNotFoundError, ProcessLookupError):
                continue  # the process ended between the two reads
            except PermissionError:
                unreadable += 1
    return {
        "uid": uid,
        "pss_kb": sum(r["pss_kb"] for r in rows),
        "unreadable": unreadable,
        "processes": rows,
    }


def frame(data, width=76):
    raw = json.dumps(data, separators=(",", ":")).encode()
    text = base64.b64encode(raw).decode()
    lines = [
        f"PSS_D {i} {text[o : o + width]}"
        for i, o in enumerate(range(0, len(text), width))
    ]
    return "\n".join([f"PSS_BEGIN {len(raw)} {zlib.crc32(raw):08x}", *lines, "PSS_END"])


if __name__ == "__main__":
    block = frame(collect())
    print(block)
    print(block)
