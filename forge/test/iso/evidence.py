#!/usr/bin/env python3
"""Writes the acceptance evidence of one installer run: acceptance-<run>.json.

The file is the predicate iso-acceptance.yml attests, keyless, on each system image digest the
ISO was built from; system/promote.sh moves :stable only to a digest that carries such an
attestation, signed by iso-acceptance.yml on a trusted branch, whose predicate says "pass" and
matches the promotion target field by field (docs/architecture/doc_update_trust.md, D1).

It attests exactly what was verified and nothing looser:
  - the run, the commit and the digests come from the labels the build put on the ISO image
    (system/publish-iso.sh): the images the ISO installed, as pushed, never what a tag points
    at later. All three images must be named, by sha256 digest;
  - the ISO image itself, by the digest the install job pulled (ISO_DIGEST): attest.sh attests
    only when that digest carries the build's keyless signature, which is what makes the
    labels above the build's word and not the word of whoever last moved the ISO tag;
  - "pass" only when the verdict passed and every check in CHECKS is present and true. A
    missing, extra or false check, or a run that left no verdict (the virtual machine never
    started), is a "fail", recorded rather than skipped;
  - the acceptance run that produced it: repository, run, attempt, ref, event, commit and
    workflow, from the GitHub Actions environment, so the verifier can refuse evidence from
    another branch or trigger even if the signing identity were looser than it is.
The images the test did not boot (the NVIDIA variants: hosted runners have no NVIDIA GPU) are
attested as built from the same run and commit as the one it did, which tested_image names.

Usage: evidence.py VERDICT_DIR ISO_LABELS ISO_DIGEST OUT_DIR
  ISO_LABELS  the ISO image's labels as JSON (podman image inspect --format '{{json .Labels}}')
  ISO_DIGEST  the manifest digest of that ISO image (sha256:<hex>)
Environment: GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT, GITHUB_REF,
             GITHUB_EVENT_NAME, GITHUB_SHA, GITHUB_WORKFLOW_REF (set by GitHub Actions).
"""

import datetime
import json
import os
import pathlib
import re
import sys

RUN_LABEL = "io.athanor.run-id"
REVISION_LABEL = "org.opencontainers.image.revision"
DIGEST_LABEL = "io.athanor.image-digest."
IMAGES = ("athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy")
TESTED_IMAGE = "athanor-system"
# The checks forge/test/iso/verdict.py writes to verdict.json; system/promote.sh requires the
# same set (system/tests/test_promote.py keeps the three lists equal).
CHECKS = (
    "installed",
    "kickstart-done",
    "profile",
    "karg",
    "greeter",
    "session",
    "settings",
    "no-guest-failure",
)
CONTEXT = {
    "repository": ("GITHUB_REPOSITORY", r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+"),
    "run_id": ("GITHUB_RUN_ID", r"[0-9]+"),
    "run_attempt": ("GITHUB_RUN_ATTEMPT", r"[0-9]+"),
    "ref": ("GITHUB_REF", r"refs/[A-Za-z0-9._/-]+"),
    "event": ("GITHUB_EVENT_NAME", r"[a-z_]+"),
    "sha": ("GITHUB_SHA", r"[0-9a-f]{40}"),
    "workflow_ref": (
        "GITHUB_WORKFLOW_REF",
        r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/\.github/workflows/[A-Za-z0-9_.-]+@refs/[A-Za-z0-9._/-]+",
    ),
}
DIGEST = re.compile(r"sha256:[0-9a-f]{64}")


def label(labels: dict, key: str, pattern: str) -> str:
    value = labels.get(key)
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        raise ValueError(
            f"the ISO image's {key} label is missing or malformed ({value!r}): "
            "it cannot be bound to the images it installed"
        )
    return value


def evidence(
    verdict_dir: pathlib.Path,
    labels: dict,
    iso: str,
    env: dict,
    now: datetime.datetime,
) -> dict:
    if not DIGEST.fullmatch(iso):
        raise ValueError(
            f"the ISO image digest is malformed ({iso!r}): the evidence cannot name what it tested"
        )
    run = label(labels, RUN_LABEL, r"[0-9]+")
    revision = label(labels, REVISION_LABEL, r"[0-9a-f]{40}")
    images = {
        name: label(labels, DIGEST_LABEL + name, DIGEST.pattern) for name in IMAGES
    }
    extra = sorted(
        k
        for k in labels
        if k.startswith(DIGEST_LABEL) and k.removeprefix(DIGEST_LABEL) not in IMAGES
    )
    if extra:
        raise ValueError(
            f"the ISO image names images this evidence does not know: {extra}"
        )
    context = {}
    for field, (variable, pattern) in CONTEXT.items():
        value = env.get(variable, "")
        if not re.fullmatch(pattern, value):
            raise ValueError(
                f"{variable} is missing or malformed ({value!r}): not a GitHub Actions run"
            )
        context[field] = value
    verdict_file = verdict_dir / "verdict.json"
    verdict = json.loads(verdict_file.read_text()) if verdict_file.exists() else {}
    checks = verdict.get("checks") if isinstance(verdict.get("checks"), dict) else {}
    passed = (
        verdict.get("pass") is True
        and set(checks) == set(CHECKS)
        and all(checks[name] is True for name in CHECKS)
    )
    return {
        "schema": 1,
        "kind": "athanor-acceptance",
        "run_id": run,
        "revision": revision,
        "result": "pass" if passed else "fail",
        "images": images,
        "tested_image": TESTED_IMAGE,
        "iso": iso,
        # Only the checks this run recorded, each as recorded: a missing one is absent, never true.
        "tests": {name: checks[name] is True for name in CHECKS if name in checks},
        "finished_at": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "acceptance": context,
    }


def main() -> int:
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    verdict_dir, labels_file, iso, out_dir = sys.argv[1:]
    labels = json.loads(pathlib.Path(labels_file).read_text()) or {}
    try:
        doc = evidence(
            pathlib.Path(verdict_dir),
            labels,
            iso,
            dict(os.environ),
            datetime.datetime.now(datetime.timezone.utc),
        )
    except ValueError as e:
        print(f"evidence.py: {e}", file=sys.stderr)
        return 1
    out = pathlib.Path(out_dir) / f"acceptance-{doc['run_id']}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{out}: {doc['result']} for run {doc['run_id']} at {doc['revision']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
