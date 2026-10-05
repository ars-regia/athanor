#!/usr/bin/env python3
"""Writes the acceptance evidence of one installer run: acceptance-<run>.json.

system/promote.sh refuses to move :stable to a digest without this file saying "pass" for
that exact digest (docs/architecture/doc_update_trust.md, D1). The run and the digests come
from the labels the build put on the ISO image (call-system-image.yml, "Publish the ISO"):
the system images the ISO was built from, as pushed, so the evidence names what the test
installed and not what a tag points at later. The result comes from verdict.json; a run that
left no verdict (the virtual machine never started) is recorded as a failure, not skipped.

Usage: evidence.py VERDICT_DIR ISO_LABELS ACCEPTANCE_RUN_ID OUT_DIR
  ISO_LABELS  the ISO image's labels as JSON (podman image inspect --format '{{json .Labels}}')
"""

import datetime
import json
import pathlib
import sys

RUN_LABEL = "io.athanor.run-id"
DIGEST_LABEL = "io.athanor.image-digest."


def evidence(
    verdict_dir: pathlib.Path, labels: dict, acceptance_run: str, now: datetime.datetime
) -> dict:
    run = labels.get(RUN_LABEL, "")
    images = {
        k.removeprefix(DIGEST_LABEL): v
        for k, v in labels.items()
        if k.startswith(DIGEST_LABEL)
    }
    if not run.isdigit() or not images:
        raise ValueError(
            f"the ISO image carries no {RUN_LABEL} or {DIGEST_LABEL}* labels: "
            "it predates the evidence gate and cannot be promoted"
        )
    verdict_file = verdict_dir / "verdict.json"
    verdict = (
        json.loads(verdict_file.read_text())
        if verdict_file.exists()
        else {"pass": False, "checks": {}}
    )
    return {
        "schema": 1,
        "run_id": run,
        "acceptance_run_id": acceptance_run,
        "result": "pass" if verdict["pass"] is True else "fail",
        "images": images,
        "tests": verdict["checks"],
        "finished_at": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
    }


def main() -> int:
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    verdict_dir, labels_file, acceptance_run, out_dir = sys.argv[1:]
    labels = json.loads(pathlib.Path(labels_file).read_text()) or {}
    try:
        doc = evidence(
            pathlib.Path(verdict_dir),
            labels,
            acceptance_run,
            datetime.datetime.now(datetime.timezone.utc),
        )
    except ValueError as e:
        print(f"evidence.py: {e}", file=sys.stderr)
        return 1
    out = pathlib.Path(out_dir) / f"acceptance-{doc['run_id']}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{out}: {doc['result']} for run {doc['run_id']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
