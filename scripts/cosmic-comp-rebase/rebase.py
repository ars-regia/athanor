#!/usr/bin/env python3
"""Try Athanor's cosmic-comp patch set on the next upstream tag and report which patches apply.

Usage: rebase.py OUT_DIR [--spec FILE] [--patches DIR] [--repo URL] [--tag TAG]

Reads the pinned version from forge/specs/cosmic-comp/cosmic-comp.spec, lists upstream's
`epoch-X.Y.Z` tags, picks the newest stable one (the pinned one when nothing is newer, which
still proves the set applies), checks that tag out and applies the patches of
forge/specs/cosmic-comp/SOURCES in file-name order, as the spec's %autosetup does: each patch
on top of those before it. A patch that fails is reported and left out; the rest are still
tried. Writes OUT_DIR/report.json and OUT_DIR/report.md. Exits 1 when a patch does not apply,
0 when all apply or the set is empty (doc_compositor.md, CO3).
"""

import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = ROOT / "forge/specs/cosmic-comp/cosmic-comp.spec"
PATCHES = ROOT / "forge/specs/cosmic-comp/SOURCES"
REPO = "https://github.com/pop-os/cosmic-comp"
TAG = re.compile(
    r"^epoch-(\d+)\.(\d+)\.(\d+)$"
)  # stable releases only: no -alpha or -beta


def version_of(tag):
    m = TAG.match(tag)
    return tuple(int(n) for n in m.groups()) if m else None


def pinned_version(spec_text):
    m = re.search(r"^Version:\s+(\d+\.\d+\.\d+)\s*$", spec_text, re.M)
    if not m:
        sys.exit("cosmic-comp.spec: Version not found")
    return f"epoch-{m.group(1)}"


def select_tag(tags, pinned):
    """The newest stable tag, never older than the pinned one, and whether it is newer."""
    stable = [t for t in tags if version_of(t)]
    if not stable:
        sys.exit("no stable epoch-X.Y.Z tag upstream")
    newest = max(stable, key=version_of)
    if version_of(newest) <= version_of(pinned):
        return pinned, False
    return newest, True


def remote_tags(repo):
    out = subprocess.run(
        ["git", "ls-remote", "--tags", "--refs", repo],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [line.split("refs/tags/", 1)[1] for line in out.splitlines()]


def apply_patches(src, patches):
    """Apply each patch on top of the previous ones; a failing patch is left out."""
    rows = []
    for patch in patches:
        check = subprocess.run(
            ["git", "-C", str(src), "apply", "--check", "-p1", str(patch)],
            capture_output=True,
            text=True,
        )
        if check.returncode == 0:
            subprocess.run(
                ["git", "-C", str(src), "apply", "-p1", str(patch)], check=True
            )
        rows.append(
            {
                "patch": patch.name,
                "applies": check.returncode == 0,
                "detail": check.stderr.strip(),
            }
        )
    return rows


def render(tag, pinned, newer, rows):
    head = f"cosmic-comp rebase drill: {len(rows)} patch(es) on `{tag}` (pinned `{pinned}`, {'a newer tag' if newer else 'no newer tag upstream'})"
    if not rows:
        return head + "\n\nThe patch set is empty: there is nothing to rebase.\n"
    lines = [head, "", "| Patch | Applies |", "| --- | --- |"]
    lines += [f"| `{r['patch']}` | {'yes' if r['applies'] else 'no'} |" for r in rows]
    for r in rows:
        if not r["applies"]:
            lines += ["", f"`{r['patch']}`:", "", "```", r["detail"], "```"]
    return "\n".join(lines) + "\n"


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("out")
    ap.add_argument("--spec", type=pathlib.Path, default=SPEC)
    ap.add_argument("--patches", type=pathlib.Path, default=PATCHES)
    ap.add_argument("--repo", default=REPO)
    ap.add_argument("--tag", help="try this tag instead of the newest one")
    args = ap.parse_args(argv)

    pinned = pinned_version(args.spec.read_text())
    if args.tag:
        tag, newer = args.tag, version_of(args.tag) > version_of(pinned)
    else:
        tag, newer = select_tag(remote_tags(args.repo), pinned)
    patches = sorted(args.patches.glob("*.patch"))

    with tempfile.TemporaryDirectory() as tmp:
        src = pathlib.Path(tmp) / "src"
        subprocess.run(
            [
                "git",
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--branch",
                tag,
                args.repo,
                str(src),
            ],
            check=True,
        )
        rows = apply_patches(src, patches)

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    report = {"pinned": pinned, "tag": tag, "newer": newer, "patches": rows}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    text = render(tag, pinned, newer, rows)
    (out / "report.md").write_text(text)
    print(text)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a") as f:
            f.write(text)
    return 0 if all(r["applies"] for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
