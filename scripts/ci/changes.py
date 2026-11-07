#!/usr/bin/env python3
"""Change detection for pr.yml: which areas of the tree a change touches.

Usage: changes.py BASE OUT [EVENT]

Lists the files that differ between BASE and HEAD (both sides of a rename) and writes OUT,
the changes.json of doc_pipeline.md section 3.3:

  {"base": "<sha>", "kernel": bool, "specs": bool, "shell": bool, "rust_audit": bool,
   "docs_only": bool}

pr.yml runs a build job only for an area set to true. The areas repeat the path filters the
build workflows had before pr.yml (kernel-build.yml, spec-build-check.yml, shell-surfaces.yml);
`rust_audit` selects the clippy and advisory audit of rust-security-audit.yml, which ran on
every pull request before it had an area;
an area exists only with the pr.yml job that consumes it, so the image check joins with its job
(doc_pipeline.md PB11, PB12). `shell` selects the visual tests of the shell rig only: the tests
of every crate of the workspace run in `just check` on every change. A root Cargo.toml that only
adds workspace members, or a Cargo.lock that only adds packages of the workspace (no `source`),
does not select `shell`: a new crate no surface uses yet changes nothing the rig draws, and a
surface that starts using it changes its own Cargo.toml. A change to the selection
itself (pr.yml or scripts/ci) selects every area, so a change to the gate is tested by every job
it gates.
docs_only is true when every changed file is documentation and no area is selected.
EVENT is the event that runs pr.yml. On merge_group the builds (kernel, specs, shell) are never
selected: the pull request proved them, and the release build proves them again (PL44, M3).
docs_only is still measured on the paths.

Path filters never decide whether pr.yml runs (PL3): this script decides which of its jobs
do, and the gate job reads the same file.
"""

import json
import pathlib
import subprocess
import sys
import tomllib

# An entry ending in "/" names a directory and everything under it, one starting with "*/"
# a file name in any directory, one starting with "*." a file extension; any other names a file.
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
        # The select job's lookup of the checkout's builder by content hash.
        "forge/scripts/check_idempotency.sh",
        "forge/scripts/retry.sh",
        "forge/scripts/registry_probe.sh",
        ".github/workflows/spec-build-check.yml",
    ),
    # The crates the shell rig draws, the rig itself, and what changes every crate's build.
    "shell": (
        "system/athanor-style/",
        "forge/specs/athanor-greeter-ui/",
        "system/athanor-layout/",
        "forge/specs/athanor-layout-chooser/",
        "system/athanor-sulphur-client/",
        "system/athanor-sal/",
        "forge/specs/athanor-sulphur/",
        "system/athanor-i18n/",
        "forge/specs/athanor-shelld/",
        "forge/specs/athanor-bar/",
        "forge/specs/athanor-dock/",
        "forge/specs/athanor-osd/",
        "system/athanor-search/",
        "system/athanor-preview/",
        "system/athanor-preview-render/",
        "forge/specs/athanor-launcher/",
        "system/athanor-apps/",
        "system/athanor-controls/",
        "system/athanor-services/",
        "forge/specs/athanor-control-center/",
        "system/athanor-unit/",
        "system/athanor-trust-state/",
        "forge/test/shell/",
        "Cargo.toml",
        "Cargo.lock",
        ".cargo/",
        ".github/workflows/shell-surfaces.yml",
    ),
    # What clippy and the cargo-deny advisories read, and the builder image they run in. The
    # job pulls the published builder, so a builder change is audited again by the weekly run
    # once the new image is out. rust-security-audit.yml's push filter repeats this area.
    "rust_audit": (
        "*.rs",
        "*/Cargo.toml",
        "*/Cargo.lock",
        "*/rust-toolchain",
        "*/rust-toolchain.toml",
        "*/clippy.toml",
        "*/.clippy.toml",
        ".cargo/",
        "deny.toml",
        "forge/builder/",
        "flake.nix",
        "flake.lock",
        "forge/scripts/retry.sh",
        "scripts/ci/security-audit.sh",
        ".github/workflows/rust-security-audit.yml",
    ),
}
# Inside an area's directories, what belongs to another workflow: the kernel spec is Kernel
# Build's, not Spec Build Check's.
EXCLUDED = {"specs": ("forge/specs/azoth/",)}
EVERY_AREA = (".github/workflows/pr.yml", "scripts/ci/")
# The areas that build, which the merge_group run never selects (doc_pipeline.md PL44).
BUILDS = ("kernel", "specs", "shell")


def touches(path, entries):
    """Whether path is one of entries: a directory ("dir/"), a name or extension anywhere
    ("*/name", "*.ext"), or a file."""
    return any(
        path == e
        or (e.endswith("/") and path.startswith(e))
        or (e.startswith("*") and ("/" + path).endswith(e[1:]))
        for e in entries
    )


def is_documentation(path):
    return path.startswith("docs/") or path.endswith(".md")


def only_adds_members(path, old, new):
    """Whether the root Cargo.toml or Cargo.lock, from text old to text new, only adds
    workspace members: in the manifest, entries of `workspace.members` and nothing else; in
    the lock, packages without a `source`, every other package unchanged."""
    try:
        old, new = tomllib.loads(old), tomllib.loads(new)
    except tomllib.TOMLDecodeError:
        return False
    if path == "Cargo.toml":
        if not all(isinstance(text.get("workspace", {}), dict) for text in (old, new)):
            return False
        before = old.get("workspace", {}).pop("members", [])
        after = new.get("workspace", {}).pop("members", [])
        # A glob adds every crate it matches, not one new crate.
        added = [member for member in after if member not in before]
        return (
            set(before) <= set(after)
            and not any(isinstance(m, str) and set(m) & set("*?[") for m in added)
            and canonical(old) == canonical(new)
        )
    before = {canonical(package) for package in old.pop("package", [])}
    after = new.pop("package", [])
    return (
        canonical(old) == canonical(new)
        and before <= {canonical(package) for package in after}
        and all("source" not in package for package in after if canonical(package) not in before)
    )


def canonical(value):
    """value as text that tells apart what == does not: true from 1, 1 from 1.0."""
    return json.dumps(value, sort_keys=True, default=str)


def classify(changed, members_only=()):
    """The areas the changed paths select, and whether the change is documentation only.
    members_only: root Cargo files that only add workspace members, which do not select shell."""
    every = any(touches(path, EVERY_AREA) for path in changed)
    excluded = {**EXCLUDED, "shell": tuple(members_only)}
    result = {
        area: every
        or any(
            touches(path, entries) and not touches(path, excluded.get(area, ()))
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


def show(cwd, rev, path):
    """The file at rev, or None when rev has no such file: an added or deleted root Cargo file
    is not a members-only change, and selects shell."""
    done = subprocess.run(
        ["git", "-C", str(cwd), "show", f"{rev}:{path}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        encoding="utf-8",
        errors="surrogateescape",
    )
    if done.returncode != 0:
        print(f"changes: no {path} at {rev[:12]}: {done.stderr.strip()}")
        return None
    return done.stdout


def main(base, out, cwd=".", event="pull_request"):
    # A two-dot tree diff: BASE is an ancestor of HEAD (the merge commit's first parent on
    # pull_request, the queue's base on merge_group), and only the two trees are needed.
    # -z: without it git quotes and escapes a path with a non-ASCII byte, a tab or a quote,
    # and the quoted form would match no area.
    out_z = git(cwd, "diff", "--name-only", "--no-renames", "-z", base, "HEAD")
    changed = [path for path in out_z.split("\0") if path]
    result = {"base": git(cwd, "rev-parse", "--verify", f"{base}^{{commit}}").strip()}
    members_only = [
        path
        for path in ("Cargo.toml", "Cargo.lock")
        if path in changed
        and None not in (texts := (show(cwd, base, path), show(cwd, "HEAD", path)))
        and only_adds_members(path, *texts)
    ]
    for path in members_only:
        print(f"changes: {path} only adds workspace members and does not select shell")
    result.update(classify(changed, members_only))
    if event == "merge_group":
        result.update(dict.fromkeys(BUILDS, False))
    path = pathlib.Path(out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result, sort_keys=True) + "\n")
    print(f"changes: {len(changed)} file(s) changed since {result['base'][:12]} ({event})")
    for key in (*AREAS, "docs_only"):
        print(f"changes: {key}={'yes' if result[key] else 'no'}")


if __name__ == "__main__":
    if len(sys.argv) not in (3, 4):
        sys.exit("usage: changes.py BASE OUT [EVENT]")
    main(sys.argv[1], sys.argv[2], event=sys.argv[3] if len(sys.argv) == 4 else "pull_request")
