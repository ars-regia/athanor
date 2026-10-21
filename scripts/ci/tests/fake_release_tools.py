#!/usr/bin/env python3
"""Offline stand-in for podman, rpm, curl and git in test_release_candidate.py, linked under
the tool's name on PATH. Every call is appended to FAKE_LOG. It models only what
release_class.PodmanSources and git_half ask: an image of Fedora 45 holding openssl, a Fedora
mirror whose updateinfo holds no advisory, and a repository in which every commit exists and
no release-class.toml was committed. Anything else of git goes to the real one."""

import gzip
import hashlib
import os
import pathlib
import sys

# mtime=0: every process that runs this fake must serve the same bytes as the checksum in
# REPOMD, which another process may have computed in a different second.
UPDATEINFO = gzip.compress(b"<updates></updates>", mtime=0)
REPOMD = (
    '<repomd xmlns="http://linux.duke.edu/metadata/repo"><data type="updateinfo"><timestamp>1760000000</timestamp>'
    f'<checksum type="sha256">{hashlib.sha256(UPDATEINFO).hexdigest()}</checksum>'
    '<location href="repodata/updateinfo.xml.gz"/></data></repomd>'
).encode()
METALINK = (
    '<metalink xmlns="http://www.metalinker.org/"><files><file name="repomd.xml"><verification>'
    f'<hash type="sha256">{hashlib.sha256(REPOMD).hexdigest()}</hash></verification><resources>'
    '<url protocol="https">https://mirror.example/updates/45/x86_64/repodata/repomd.xml</url>'
    "</resources></file></files></metalink>"
).encode()


def main(name, args):
    with open(os.environ["FAKE_LOG"], "a") as log:
        log.write(" ".join([name, *args]) + "\n")
    if name == "git" and not {"rev-parse", "ls-tree", "log", "rev-list", "show"} & set(args):
        os.execv("/usr/bin/git", ["git", *args])
    if name == "git":
        return 0  # every commit exists, no release-class.toml is carried
    if name == "podman":
        if args[0] == "create":
            print("fake-container")
        elif args[0] == "cp":
            dest = pathlib.Path(args[2])
            if dest.name == "rpm":
                dest.mkdir()
                (dest / "rpmdb.sqlite").write_text("")
            else:
                dest.write_text('VERSION_ID="45"\n')
        elif args[0] not in ("rm", "rmi"):
            print(f"fake podman: {args[0]} is never run on a candidate", file=sys.stderr)
            return 2
        return 0
    if name == "rpm":
        print("openssl 0:3.5.1-1.fc45\nfilesystem 0:3.18-1.fc45\nglibc 0:2.42-1.fc45")
        return 0
    if name == "curl":
        url = args[-1]
        body = METALINK if "metalink" in url else REPOMD if url.endswith("repomd.xml") else UPDATEINFO
        sys.stdout.buffer.write(body)
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main(pathlib.Path(sys.argv[0]).name, sys.argv[1:]))
