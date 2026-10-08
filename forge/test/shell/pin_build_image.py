#!/usr/bin/env python3
"""Pin the published build stage of the shell rig.

    pin_build_image.py DIGESTFILE OUT_DIR

DIGESTFILE is what `rig.sh publish-build-image` wrote. The digest is validated and copied to
forge/test/shell/build-image.digest, and OUT_DIR gets the `title` and `body.md` that
forge/specs/azoth/open_bump_pr.sh turns into the pull request. The last word of the title is
the branch suffix, so it is the short digest.
"""

import re
import sys
from pathlib import Path

PIN = Path(__file__).resolve().parent / "build-image.digest"
DIGEST = re.compile(r"sha256:[0-9a-f]{64}")


def pin(digestfile: Path, out: Path, pinfile: Path = PIN) -> str:
    digest = digestfile.read_text().strip()
    if not DIGEST.fullmatch(digest):
        raise SystemExit(f"{digestfile}: not a sha256 digest: {digest!r}")
    short = digest.removeprefix("sha256:")[:12]
    pinfile.write_text(digest + "\n")
    out.mkdir(parents=True, exist_ok=True)
    (out / "title").write_text(f"ci(shell): pin the rig build stage at {short}\n")
    (out / "body.md").write_text(
        "Pins the published build stage of the shell rig "
        "(`athanor-shell-rig-build`) by digest, so that `rig.sh` pulls it instead of "
        "building it against the Fedora mirrors.\n\n"
        f"Digest: `{digest}`\n\n"
        "Opened by `publish-rig-build-image.yml`, which pushed this image and pulled it back "
        "by digest before opening this pull request. The package must be public for the "
        "hosted gate to pull it.\n"
    )
    return digest


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    pin(Path(sys.argv[1]), Path(sys.argv[2]))
