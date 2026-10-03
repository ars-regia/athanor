#!/usr/bin/env python3
"""Move the watched forge specs to their newest upstream release.

Run from forge/. For each entry of upstream-watch.json ({"repo": "owner/name", "spec":
"specs/<dir>/<file>.spec"}) the GitHub API gives the latest release; when it is newer than
the spec's Version, the spec gets that Version and Release 1, and fetch_sources.sh --pin
downloads the sources again and rewrites SOURCES/sources.sha256. The Source lines are never
touched: they keep their %{version} URLs, so the build fetches exactly what was hashed.

releases/latest never returns a pre-release or a draft. A repository without releases, a
tag that is not a plain version, an API or download failure: each one fails the run, so a
broken watch entry is a red run and not a package that silently stops updating.

Usage: zero_trust_updater.py REPORT. The bumps are listed in REPORT, which the workflow
uses as the pull request body; no file means nothing to update.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request

WATCH_FILE = "upstream-watch.json"
VERSION = re.compile(r"^\d+(\.\d+)*$")


def fetch_json(url):
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json"})
    token = os.environ.get("GH_TOKEN")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=30) as response:
        return json.load(response)


def latest_release(repo, fetch=fetch_json):
    """(version, tag) of the repository's latest release."""
    tag = fetch(f"https://api.github.com/repos/{repo}/releases/latest")["tag_name"]
    version = tag[1:] if tag.startswith("v") else tag
    if not VERSION.match(version):
        raise ValueError(f"{repo}: latest release tag {tag!r} is not a plain version")
    return version, tag


def as_tuple(version):
    return tuple(int(part) for part in version.split("."))


def spec_version(text):
    m = re.search(r"^Version:\s+(\S+)\s*$", text, re.M)
    if not m:
        raise ValueError("no Version line")
    return m.group(1)


def bump(text, version):
    """The spec text at VERSION with Release back to 1; every other line unchanged."""
    text, n = re.subn(
        r"^(Version:\s+)\S+", lambda m: m.group(1) + version, text, count=1, flags=re.M
    )
    text, r = re.subn(
        r"^(Release:\s+)\d+", lambda m: m.group(1) + "1", text, count=1, flags=re.M
    )
    if n != 1 or r != 1:
        raise ValueError("Version or Release line not found")
    return text


def pin_sources(spec_dir):
    with tempfile.TemporaryDirectory() as downloads:
        subprocess.run(
            ["bash", "scripts/fetch_sources.sh", "--pin", spec_dir, downloads],
            check=True,
        )


def main(report, fetch=fetch_json, pin=pin_sources):
    with open(WATCH_FILE) as f:
        watch = json.load(f)
    rows = []
    for item in watch:
        repo, spec = item["repo"], item["spec"]
        with open(spec) as f:
            text = f.read()
        current = spec_version(text)
        latest, tag = latest_release(repo, fetch)
        if as_tuple(latest) <= as_tuple(current):
            print(f"{spec}: {current} is current (upstream {latest})")
            continue
        print(f"{spec}: {current} -> {latest}", flush=True)
        with open(spec, "w") as f:
            f.write(bump(text, latest))
        pin(os.path.dirname(spec))
        rows.append(
            f"| `{os.path.basename(spec)}` | {current} | {latest} | https://github.com/{repo}/releases/tag/{tag} |"
        )
    if rows:
        os.makedirs(os.path.dirname(report) or ".", exist_ok=True)
        with open(report, "w") as f:
            f.write(
                "## Spec updates\n\n| spec | before | after | release |\n| --- | --- | --- | --- |\n"
            )
            f.write("\n".join(rows) + "\n")
            f.write(
                "\nSpec Build Check builds every changed spec. A green build arms auto-merge "
                "when the leftmost non-zero version component is unchanged; any other bump "
                "waits for a review.\n"
            )
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: zero_trust_updater.py REPORT")
    sys.exit(main(sys.argv[1]))
