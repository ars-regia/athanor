#!/usr/bin/env python3
"""Change detection for pr.yml: which areas of the tree a change touches.

Usage: changes.py BASE OUT

Lists the files that differ between BASE and HEAD (both sides of a rename) and writes OUT,
the changes.json of doc_pipeline.md section 3.3:

  {"base": "<sha>", "kernel": bool, "specs": bool, "docs_only": bool}

pr.yml runs a build job only for an area set to true. The areas repeat the path filters the
build workflows had before pr.yml (kernel-build.yml, spec-build-check.yml); an area exists
only with the pr.yml job that consumes it, so the image and shell checks join with their jobs
(doc_pipeline.md PB11, PB12). A change to the selection itself (pr.yml or
scripts/ci) selects every area, so a change to the gate is tested by every job it gates.
docs_only is true when every changed file is documentation and no area is selected.

Path filters never decide whether pr.yml runs (PL3): this script decides which of its jobs
do, and the gate job reads the same file.
"""

import json
import pathlib
import subprocess
import sys

# An entry ending in "/" names a directory and everything under it; any other names a file.
AREAS = {
    "kernel": (
        "forge/specs/azoth/",
        "system/kernel-artifacts.sh",
        ".github/actions/kvm/",
        ".github/workflows/kernel-build.yml",
        ".github/workflows/call-kernel.yml",
        ".github/workflows/nvidia-build.yml",
    ),
    "specs": (
        "forge/specs/",
        "forge/config/rpmmacros",
        "forge/builder/",
        "flake.nix",
        "flake.lock",
        "forge/scripts/build_spec.sh",
        "forge/scripts/run_spec_build.sh",
        "forge/scripts/fetch_sources.sh",
        "forge/scripts/select_check_specs.py",
        "forge/scripts/builder_image.sh",
        ".github/workflows/spec-build-check.yml",
    ),
}
# Inside an area's directories, what belongs to another workflow: the kernel spec is Kernel
# Build's, not Spec Build Check's.
EXCLUDED = {"specs": ("forge/specs/azoth/",)}
EVERY_AREA = (".github/workflows/pr.yml", "scripts/ci/")


def touches(path, entries):
    """Whether path is one of entries, an entry ending in "/" naming a whole directory."""
    return any(path == e or (e.endswith("/") and path.startswith(e)) for e in entries)


def is_documentation(path):
    return path.startswith("docs/") or path.endswith(".md")


def classify(changed):
    """The areas the changed paths select, and whether the change is documentation only."""
    every = any(touches(path, EVERY_AREA) for path in changed)
    result = {
        area: every
        or any(
            touches(path, entries) and not touches(path, EXCLUDED.get(area, ()))
            for path in changed
        )
        for area, entries in AREAS.items()
    }
    result["docs_only"] = (
        bool(changed)
        and not any(result.values())
        and all(is_documentation(path) for path in changed)
    )
    return result


def git(cwd, *args):
    # surrogateescape: a path that is not UTF-8 still round-trips instead of failing the job.
    return subprocess.run(
        ["git", "-C", str(cwd), *args],
        check=True,
        stdout=subprocess.PIPE,
        encoding="utf-8",
        errors="surrogateescape",
    ).stdout


def main(base, out, cwd="."):
    # A two-dot tree diff: BASE is an ancestor of HEAD (the merge commit's first parent on
    # pull_request, the queue's base on merge_group), and only the two trees are needed.
    # -z: without it git quotes and escapes a path with a non-ASCII byte, a tab or a quote,
    # and the quoted form would match no area.
    out_z = git(cwd, "diff", "--name-only", "--no-renames", "-z", base, "HEAD")
    changed = [path for path in out_z.split("\0") if path]
    result = {"base": git(cwd, "rev-parse", "--verify", f"{base}^{{commit}}").strip()}
    result.update(classify(changed))
    path = pathlib.Path(out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result, sort_keys=True) + "\n")
    print(f"changes: {len(changed)} file(s) changed since {result['base'][:12]}")
    for key in (*AREAS, "docs_only"):
        print(f"changes: {key}={'yes' if result[key] else 'no'}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: changes.py BASE OUT")
    main(*sys.argv[1:])
