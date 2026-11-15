#!/usr/bin/python3
"""orca_control_center.py <page|panel> <transcript> <key>... - orca_walk.py through the control
center, as scene.sh's RIG_HOLD, with the control center started by bar_session.py --client
athanor-control-center --fixtures.

Orca starts first, and the walk then opens the control center as the bar's button does, through
cc_show.py's Show: Orca hears the panel announced as it opens and the first tile take the focus
(doc_control_center.md CC13, accessibility), or the page that Show named, whose Back button the
first Tab reaches.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cc_show  # noqa: E402
import orca_walk  # noqa: E402


def main(argv):
    if len(argv) < 3 or argv[0] not in cc_show.PAGES:
        print(__doc__, file=sys.stderr)
        return 2
    page, walk = argv[0], argv[1:]
    return orca_walk.main(walk, before=lambda: cc_show.show(page))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
