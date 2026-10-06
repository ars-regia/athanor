#!/usr/bin/env python3
"""Selects what Spec Build Check (.github/workflows/spec-build-check.yml) builds for a change.

Usage: select_check_specs.py BASE OUT_DIR

Reads the files changed since BASE (git diff BASE...HEAD) and writes:

  OUT_DIR/builder     "true" when the change touches the inputs of the builder image (the
                      flake and forge/builder): the check then builds that image from the
                      checkout instead of pulling the published one.
  OUT_DIR/specs.json  the spec directories to build, relative to forge/ (specs/<name>, as
                      run_spec_build.sh takes them), sorted.

A change to what every spec build goes through (the builder image, forge/config/rpmmacros,
build_spec.sh, run_spec_build.sh, fetch_sources.sh) rebuilds every spec the DAG builds, from
dag_orchestrator.py --list-spec-dirs; any other change builds the spec directories it touches.
Kernel Build builds forge/specs/azoth and the DAG builds athanor-telemetry with Nix, so both
are left out, as are directories that hold no .spec.
"""

import json
import pathlib
import subprocess
import sys

# What check_idempotency.sh hashes for the builder and the flake actually turns into the
# image. forge/config/rpmmacros and packages.json are in that hash too, but the image does
# not carry them: build_spec.sh copies rpmmacros from the checkout into every build.
BUILDER_INPUTS = ("flake.nix", "flake.lock", "forge/builder/")
SHARED_INPUTS = BUILDER_INPUTS + (
    "forge/config/rpmmacros",
    "forge/scripts/build_spec.sh",
    "forge/scripts/run_spec_build.sh",
    "forge/scripts/fetch_sources.sh",
)
OWN_WORKFLOW = {
    "specs/azoth": "Kernel Build builds it",
    "specs/athanor-telemetry": "the DAG builds it with Nix",
}


def touches(path, inputs):
    """Whether path is one of inputs, a trailing slash naming a whole directory."""
    return any(path == i or (i.endswith("/") and path.startswith(i)) for i in inputs)


def select(changed, dag_dirs, has_spec):
    """Returns (builder, specs, skipped): whether the builder image must be built from the
    change, the spec directories to build, and {directory: reason} for those left out."""
    builder = any(touches(path, BUILDER_INPUTS) for path in changed)
    full = any(touches(path, SHARED_INPUTS) for path in changed)
    dirs = {
        "/".join(path.split("/")[1:3])
        for path in changed
        if path.startswith("forge/specs/") and path.count("/") >= 3
    }
    if full:
        dirs.update(dag_dirs)
    specs, skipped = [], {}
    for directory in sorted(dirs):
        if directory in OWN_WORKFLOW:
            skipped[directory] = OWN_WORKFLOW[directory]
        elif not has_spec(directory):
            skipped[directory] = "it holds no .spec"
        else:
            specs.append(directory)
    return builder, specs, skipped


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args], check=True, stdout=subprocess.PIPE, text=True
    ).stdout


def main(base, out_dir):
    root = pathlib.Path(git(pathlib.Path(__file__).parent, "rev-parse", "--show-toplevel").strip())
    changed = git(root, "diff", "--name-only", f"{base}...HEAD").split()
    dag_dirs = []
    if any(touches(path, SHARED_INPUTS) for path in changed):
        dag_dirs = subprocess.run(
            [sys.executable, "scripts/dag_orchestrator.py", "--list-spec-dirs"],
            cwd=root / "forge",
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout.split()
    builder, specs, skipped = select(
        changed, dag_dirs, lambda d: any((root / "forge" / d).glob("*.spec"))
    )

    out = pathlib.Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    (out / "builder").write_text("true\n" if builder else "false\n")
    (out / "specs.json").write_text(json.dumps(specs) + "\n")

    if dag_dirs:
        print(
            "select_check_specs: the change reaches every spec build, all DAG specs selected"
        )
    print(f"select_check_specs: builder from this change: {'yes' if builder else 'no'}")
    for directory, reason in skipped.items():
        print(f"select_check_specs: {directory} skipped, {reason}")
    print(f"select_check_specs: {len(specs)} spec(s): {' '.join(specs)}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: select_check_specs.py BASE OUT_DIR")
    main(*sys.argv[1:])
