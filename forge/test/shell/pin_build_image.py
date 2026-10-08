#!/usr/bin/env python3
"""Pin the published build stage of the shell rig.

    pin_build_image.py REFFILE OUT_DIR

REFFILE is the build-image.ref that `rig.sh publish-build-image` wrote: the full reference
`<registry>/athanor-shell-rig-build@sha256:<digest>` that was pushed. It is validated and copied to
forge/test/shell/build-image.digest, which rig.sh pulls as it is, so the producer decides the
registry and the consumer never recomputes it. A bare digest (the old form, never committed) is
rejected. OUT_DIR gets the `title` and `body.md` that forge/specs/azoth/open_bump_pr.sh turns
into the pull request; the last word of the title is the branch suffix, the short digest.
"""

import re
import sys
from pathlib import Path

PIN = Path(__file__).resolve().parent / "build-image.digest"
REF = re.compile(r"[a-z0-9][a-z0-9._:/-]*/athanor-shell-rig-build@sha256:([0-9a-f]{64})")


def pin(reffile: Path, out: Path, pinfile: Path = PIN) -> str:
    ref = reffile.read_text().strip()
    m = REF.fullmatch(ref)
    if not m:
        raise SystemExit(f"{reffile}: not a lowercase <registry>/athanor-shell-rig-build@sha256:<digest>: {ref!r}")
    short = m.group(1)[:12]
    pinfile.write_text(ref + "\n")
    out.mkdir(parents=True, exist_ok=True)
    (out / "title").write_text(f"ci(shell): pin the rig build stage at {short}\n")
    (out / "body.md").write_text(
        "Pins the published build stage of the shell rig by digest, so that `rig.sh` pulls it "
        "instead of building it against the Fedora mirrors.\n\n"
        f"Reference: `{ref}`\n\n"
        "Opened by `publish-rig-build-image.yml`, which pushed this image, pulled it back by "
        "digest and checked that it can be pulled without a login, as the hosted gate does.\n"
    )
    return ref


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    pin(Path(sys.argv[1]), Path(sys.argv[2]))
