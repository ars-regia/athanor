#!/usr/bin/env python3
"""Selects what Spec Build Check (.github/workflows/spec-build-check.yml) builds for a change.

Usage: select_check_specs.py BASE OUT_DIR

Reads the files changed since BASE (git diff BASE...HEAD) and writes:

  OUT_DIR/builder     "true" when the change touches the inputs of the builder image (the
                      flake and forge/builder): the check then builds that image from the
                      checkout instead of pulling the published one.
  OUT_DIR/specs.json  the spec directories to build, relative to forge/ (specs/<name>, as
                      run_spec_build.sh takes them), sorted.

Only what the DAG builds is built: the custom_packages of forge/config/packages.json, from
dag_orchestrator.py --list-spec-dirs. A change to what every spec build goes through (the
builder image, forge/config/rpmmacros, build_spec.sh, run_spec_build.sh, fetch_sources.sh)
rebuilds all of them; any other change builds the DAG's spec directories it touches. Every other
changed directory is named and skipped: the kernel (Kernel Build builds it), Nix-built packages,
specs the DAG does not list, and directories the change deleted or that hold no .spec.
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


def touches(path, inputs):
    """Whether path is one of inputs, a trailing slash naming a whole directory."""
    return any(path == i or (i.endswith("/") and path.startswith(i)) for i in inputs)


def select(changed, dag_dirs, has_spec):
    """Returns (builder, specs, skipped): whether the builder image must be built from the
    change, the spec directories to build, and {directory: reason} for those left out."""
    builder = any(touches(path, BUILDER_INPUTS) for path in changed)
    dirs = {
        "/".join(path.split("/")[1:3])
        for path in changed
        if path.startswith("forge/specs/") and path.count("/") >= 3
    }
    if any(touches(path, SHARED_INPUTS) for path in changed):
        dirs.update(dag_dirs)
    specs, skipped = [], {}
    for directory in sorted(dirs):
        if directory not in dag_dirs:
            skipped[directory] = "is not built by the DAG"
        elif not has_spec(directory):
            skipped[directory] = "holds no spec after the change"
        else:
            specs.append(directory)
    return builder, specs, skipped


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args], check=True, stdout=subprocess.PIPE, text=True
    ).stdout


def main(base, out_dir):
    root = pathlib.Path(
        git(pathlib.Path(__file__).parent, "rev-parse", "--show-toplevel").strip()
    )
    changed = git(root, "diff", "--name-only", f"{base}...HEAD").split()
    dag_dirs = subprocess.run(
        [sys.executable, "scripts/dag_orchestrator.py", "--list-spec-dirs"],
        cwd=root / "forge",
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout.split()
    if not dag_dirs:
        sys.exit("select_check_specs: the DAG lists no spec directory")
    builder, specs, skipped = select(
        changed, set(dag_dirs), lambda d: any((root / "forge" / d).glob("*.spec"))
    )

    out = pathlib.Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    (out / "builder").write_text("true\n" if builder else "false\n")
    (out / "specs.json").write_text(json.dumps(specs) + "\n")

    print(f"select_check_specs: builder from this change: {'yes' if builder else 'no'}")
    for directory, reason in skipped.items():
        print(f"select_check_specs: forge/{directory} {reason}, skipped")
    print(f"select_check_specs: {len(specs)} spec(s) selected, {len(skipped)} skipped")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: select_check_specs.py BASE OUT_DIR")
    main(*sys.argv[1:])
