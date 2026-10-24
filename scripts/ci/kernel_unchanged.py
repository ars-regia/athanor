#!/usr/bin/env python3
"""Whether a pull request leaves the kernel check's inputs as its base has them.

A pull request whose kernel is not published builds Azoth on the single self-hosted runner
(call-kernel.yml). It does not when both of these hold, because its kernel is then its base's,
which the push build of the base builds and publishes (kernel-build.yml), and build, boot and
kmod are skipped:

1. build-inputs.py, each commit's own, prints the same inputs at BASE and at HEAD;
2. every CHECKED path, what the boot matrix and the NVIDIA modules read besides the kernel
   RPMs, has the same git object at BASE and at HEAD.

A fetch or git error, a CHECKED path missing at either commit, or a build-inputs.py that fails
means "build required", never a skip: the decision is written, a warning is printed and the
exit status is 0.

Usage: kernel_unchanged.py --base SHA --out DIR [--repo DIR]
Writes DIR/decision.json, {"unchanged": bool, "reason": str, "base": SHA, "paths": {...}}, and
DIR/summary.md.
"""

import argparse
import json
import pathlib
import sys
import tempfile

from kernel_promotion import build_inputs, git

# What the boot and kmod jobs read besides the kernel RPMs, whose inputs build-inputs.py covers:
# the boot matrix (boot.sh, boot/, the guest kernel's boot in microvm/, cmdline, the
# certificates and test profiles under keys/, lock.sh in the boot and nvidia images, the kvm
# action), the NVIDIA modules (nvidia.sh, nvidia/, the NVIDIA_* pins build-inputs.py drops,
# optional-branch.sh, nvidia-build.yml), and the workflow that runs them. Also what shapes the
# build and is outside build-inputs.py: the NVR the RPMs are named and checked by (nvr.sh), the
# keys the sources are verified with (SOURCES/keys), every image directory whose package lock
# `lock.sh check` verifies during the build (builder, signer, besides boot and nvidia), and the
# script that builds the builder. A directory compares as one tree object.
CHECKED = (
    "forge/specs/azoth/nvr.sh",
    "forge/specs/azoth/SOURCES/keys",
    "forge/specs/azoth/builder",
    "forge/specs/azoth/signer",
    "scripts/ci/build-builder.sh",
    "forge/specs/azoth/pins.env",
    "forge/specs/azoth/boot.sh",
    "forge/specs/azoth/boot",
    "forge/specs/azoth/microvm",
    "forge/specs/azoth/cmdline",
    "forge/specs/azoth/keys",
    "forge/specs/azoth/lock.sh",
    "forge/specs/azoth/nvidia.sh",
    "forge/specs/azoth/nvidia",
    "system/kernel-artifacts.sh",
    "system/optional-branch.sh",
    ".github/actions/kvm",
    ".github/workflows/call-kernel.yml",
    ".github/workflows/nvidia-build.yml",
)


def decide(repo, base):
    git(repo, "fetch", "--no-tags", "--depth=1", "origin", base)
    base = git(repo, "rev-parse", "--verify", f"{base}^{{commit}}")
    decision = {"unchanged": False, "base": base, "paths": {}}
    with tempfile.TemporaryDirectory() as tmp:
        inputs = {
            rev: build_inputs(repo, rev, pathlib.Path(tmp) / rev)
            for rev in (base, "HEAD")
        }
    if inputs[base] != inputs["HEAD"]:
        return {**decision, "reason": f"build-inputs.py differs from the base's {base}"}
    for path in CHECKED:
        ids = {
            "base": git(repo, "rev-parse", f"{base}:{path}"),
            "head": git(repo, "rev-parse", f"HEAD:{path}"),
        }
        decision["paths"][path] = ids
        if ids["base"] != ids["head"]:
            return {**decision, "reason": f"{path} differs from the base's {base}"}
    reason = f"the kernel inputs (build-inputs.py) and what boot and kmod read are identical to the base's {base}"
    return {**decision, "unchanged": True, "reason": reason}


def check(repo, base):
    """decide; any failure of git or of build-inputs.py is a decision to build, never a skip."""
    try:
        return decide(repo, base)
    except Exception as error:  # noqa: BLE001 - the boundary of "any failure means build"
        detail = getattr(error, "stderr", None) or ""
        if isinstance(detail, bytes):
            detail = detail.decode(errors="replace")
        return {
            "unchanged": False,
            "base": base,
            "paths": {},
            "reason": f"error: {type(error).__name__}: {error} {detail}".strip(),
        }


def summary(decision):
    lines = ["### Kernel inputs against the base", ""]
    if not decision["unchanged"]:
        return "\n".join([*lines, f"Build required: {decision['reason']}.", ""])
    return "\n".join(
        [
            *lines,
            f"Not built: {decision['reason']}. The push build of the base builds and publishes this "
            "kernel; build, boot and kmod are skipped.",
            "",
        ]
    )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--base", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--repo", default=".")
    args = parser.parse_args(argv)
    decision = check(args.repo, args.base)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "decision.json").write_text(
        json.dumps(decision, indent=2, sort_keys=True) + "\n"
    )
    (out / "summary.md").write_text(summary(decision))
    if decision["unchanged"]:
        print(
            f"::notice title=Kernel inputs::{decision['reason']}: build, boot and kmod skipped"
        )
    else:
        # Loud: a pull request that builds the kernel holds the self-hosted runner for hours.
        level = "warning" if decision["reason"].startswith("error: ") else "notice"
        print(f"::{level} title=Kernel inputs::build required: {decision['reason']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
