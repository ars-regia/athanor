"""Sum of the proportional set size of the user's session, read inside the guest.

Standard library only, and small on purpose: forge/test/iso/memory_case.py sends this file
to the guest over the serial console, which carries no checkout. Prints one line,
`PSS_REPORT <json>`, which report.py reads back from the console log.

The processes counted are those of the user's slice (user-<uid>.slice: the user manager,
which holds the desktop, and the session scopes), minus the scope this script itself runs
in, which is the serial login that asked.
"""

import json
import os
import pathlib


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
        for pid in procs.read_text().split():
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


if __name__ == "__main__":
    print("PSS_" + "REPORT " + json.dumps(collect(), separators=(",", ":")))
