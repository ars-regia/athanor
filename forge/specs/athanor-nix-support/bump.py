#!/usr/bin/env python3
"""The bump bot of the nixpkgs pin in the system flake registry (#154, decision A2-13).

    bump.py check            prints a JSON with the current pin, the new one and whether it moved
    bump.py apply OUT_DIR    rewrites the registry and nixpkgs-branch when the pin moved, and writes
                             OUT_DIR/title and OUT_DIR/body.md for open_bump_pr.sh

The registry (SOURCES/usr/share/athanor/nix/registry.json) points `nixpkgs` at a revision of a
stable release branch with its narHash. The branch is in nixpkgs-branch, next to this script: the
registry cannot carry it, because Nix refuses a GitHub input with both a ref and a rev.

Branch: the newest nixos-YY.05 or nixos-YY.11 that exists on NixOS/nixpkgs and whose release is
settled, from the 15th of the month after the release month (June 15, December 15), so the move to
the next release, every six months, is a bump like any other. Never backwards: a branch older than
the pinned one is never chosen. Revision: the head of that branch. narHash: the NAR hash of the
GitHub archive of the revision, computed here (the top directory stripped, as Nix unpacks it).
Descent: a new revision on the pinned branch must descend from the current pin (GitHub's compare
API says "ahead" or "identical"); a force-pushed or rewritten branch fails instead of moving the
pin. Across a release change the two branches are siblings off master, so "diverged" is accepted
there and only an unrelated history ("behind" or an error) fails. The API call uses GITHUB_TOKEN
when set, read-only.
Standard library and git only: it runs on the GitHub runner without installing anything.
"""

import base64
import datetime
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REGISTRY = HERE / "SOURCES" / "usr" / "share" / "athanor" / "nix" / "registry.json"
BRANCH_FILE = HERE / "nixpkgs-branch"
REPO = "https://github.com/NixOS/nixpkgs"
API = "https://api.github.com/repos/NixOS/nixpkgs"
BRANCH_RE = re.compile(r"^nixos-(\d\d)\.(05|11)$")


def branch_version(branch):
    m = BRANCH_RE.match(branch)
    if not m:
        sys.exit(f"bump.py: {branch!r} is not a nixos-YY.05 or nixos-YY.11 branch")
    return int(m.group(1)), int(m.group(2))


def settled(branch, today):
    """True from the 15th of the month after the branch's release month."""
    year, month = branch_version(branch)
    return today >= datetime.date(2000 + year, month + 1, 15)


def choose_branch(heads, current, today):
    """The newest settled release branch among `heads`, never older than `current`."""
    candidates = [b for b in heads if BRANCH_RE.match(b) and settled(b, today)]
    best = max(candidates, key=branch_version, default=current)
    if branch_version(best) < branch_version(current):
        best = current
    if best not in heads:
        sys.exit(f"bump.py: the pinned branch {current} is not on {REPO}")
    return best


def remote_heads():
    """{branch: commit} for the release branches of nixpkgs."""
    out = subprocess.run(
        ["git", "ls-remote", "--heads", REPO, "refs/heads/nixos-*"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    heads = {}
    for line in out.splitlines():
        sha, ref = line.split("\t")
        branch = ref.removeprefix("refs/heads/")
        if BRANCH_RE.match(branch):
            heads[branch] = sha
    return heads


def compare_status(base, head):
    """GitHub's verdict on `head` relative to `base`: ahead, identical, behind or diverged."""
    req = urllib.request.Request(
        f"{API}/compare/{base}...{head}?per_page=1",
        headers={"Accept": "application/vnd.github+json"},
    )
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)["status"]


def verify_descent(old, new):
    """Fails unless the new revision descends from the current pin (see the module docstring)."""
    if old["rev"] == new["rev"]:
        return
    allowed = {"ahead", "identical"}
    if new["branch"] != old["branch"]:
        allowed.add("diverged")
    status = compare_status(old["rev"], new["rev"])
    if status not in allowed:
        sys.exit(
            f"bump.py: {new['rev']} is {status!r} relative to the pinned {old['rev']}, "
            f"not {' or '.join(sorted(allowed))}: the branch was rewritten, not advanced"
        )


def nar_string(h, data):
    h.update(len(data).to_bytes(8, "little"))
    h.update(data)
    h.update(b"\0" * (-len(data) % 8))


def nar_node(h, node):
    nar_string(h, b"(")
    nar_string(h, b"type")
    if isinstance(node, dict):
        nar_string(h, b"directory")
        for name in sorted(node):
            nar_string(h, b"entry")
            nar_string(h, b"(")
            nar_string(h, b"name")
            nar_string(h, name)
            nar_string(h, b"node")
            nar_node(h, node[name])
            nar_string(h, b")")
    elif node[0] == "symlink":
        nar_string(h, b"symlink")
        nar_string(h, b"target")
        nar_string(h, node[1])
    else:
        nar_string(h, b"regular")
        if node[1]:
            nar_string(h, b"executable")
            nar_string(h, b"")
        nar_string(h, b"contents")
        nar_string(h, node[2])
    nar_string(h, b")")


def tar_tree(archive):
    """The tree of a gzipped tar, its single top directory stripped, as nested dicts: a file is
    ("file", executable, contents), a symbolic link ("symlink", target)."""
    root = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        for member in tar:
            if member.type == tarfile.XGLTYPE or member.name in ("pax_global_header",):
                continue
            parts = [p.encode("utf-8", "surrogateescape") for p in member.name.split("/") if p not in ("", ".")][
                1:
            ]
            if not parts:
                continue
            node = root
            for part in parts[:-1]:
                node = node.setdefault(part, {})
            if member.isdir():
                node.setdefault(parts[-1], {})
            elif member.issym():
                node[parts[-1]] = ("symlink", member.linkname.encode("utf-8", "surrogateescape"))
            elif member.isfile():
                node[parts[-1]] = (
                    "file",
                    bool(member.mode & 0o100),
                    tar.extractfile(member).read(),
                )
            else:
                sys.exit(
                    f"bump.py: unexpected tar member {member.name} (type {member.type!r})"
                )
    return root


def nar_hash(tree):
    h = hashlib.sha256()
    nar_string(h, b"nix-archive-1")
    nar_node(h, tree)
    return "sha256-" + base64.b64encode(h.digest()).decode()


def archive_nar_hash(rev):
    with urllib.request.urlopen(f"{REPO}/archive/{rev}.tar.gz", timeout=600) as resp:
        return nar_hash(tar_tree(resp.read()))


def read_pin():
    registry = json.loads(REGISTRY.read_text())
    (entry,) = [
        f
        for f in registry["flakes"]
        if f["from"] == {"type": "indirect", "id": "nixpkgs"}
    ]
    return {
        "branch": BRANCH_FILE.read_text().strip(),
        "rev": entry["to"]["rev"],
        "narHash": entry["to"]["narHash"],
    }


def registry_for(pin):
    return {
        "version": 2,
        "flakes": [
            {
                "from": {"type": "indirect", "id": "nixpkgs"},
                "to": {
                    "type": "github",
                    "owner": "NixOS",
                    "repo": "nixpkgs",
                    "rev": pin["rev"],
                    "narHash": pin["narHash"],
                },
            }
        ],
    }


def compute(today):
    old = read_pin()
    heads = remote_heads()
    branch = choose_branch(heads, old["branch"], today)
    if branch == old["branch"] and heads[branch] == old["rev"]:
        return {"changed": False, "old": old, "new": old}
    new = {"branch": branch, "rev": heads[branch]}
    verify_descent(old, new)
    new["narHash"] = archive_nar_hash(new["rev"])
    return {"changed": True, "old": old, "new": new}


def title(result):
    new = result["new"]
    return f"chore(nix): bump nixpkgs to {new['branch']}-{new['rev'][:12]}"


def body(result):
    old, new = result["old"], result["new"]
    text = (
        "## Pin\n\n| | before | after |\n| --- | --- | --- |\n"
        f"| branch | `{old['branch']}` | `{new['branch']}` |\n"
        f"| rev | `{old['rev']}` | `{new['rev']}` |\n"
        f"| narHash | `{old['narHash']}` | `{new['narHash']}` |\n\n"
        "The system flake registry (`/usr/share/athanor/nix/registry.json`) moves `nixpkgs` to the "
        f"head of `{new['branch']}`; the narHash was computed from the GitHub archive of the revision. "
        f"Changes: {REPO}/compare/{old['rev']}...{new['rev']}\n"
    )
    if new["branch"] != old["branch"]:
        text += (
            f"\n**Release branch change.** `{old['branch']}` is replaced by `{new['branch']}`. "
            "Read the release notes of the new NixOS release (backward-incompatible changes of "
            "nixpkgs) before merging: users who resolve `nixpkgs` through the registry get the new "
            "release on their next `nix profile upgrade`.\n"
        )
    return text + "\nNever auto-merged.\n"


def main():
    args = sys.argv[1:]
    if args not in (["check"],) and not (len(args) == 2 and args[0] == "apply"):
        sys.exit(__doc__)
    result = compute(datetime.datetime.now(datetime.timezone.utc).date())
    if args[0] == "check":
        print(json.dumps(result, indent=2))
        return
    if result["changed"]:
        REGISTRY.write_text(
            json.dumps(registry_for(result["new"]), indent=2) + "\n", newline="\n"
        )
        BRANCH_FILE.write_text(result["new"]["branch"] + "\n", newline="\n")
        out = Path(args[1])
        out.mkdir(parents=True, exist_ok=True)
        (out / "title").write_text(title(result) + "\n", newline="\n")
        (out / "body.md").write_text(body(result), newline="\n")


if __name__ == "__main__":
    main()
