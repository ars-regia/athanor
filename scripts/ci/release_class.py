#!/usr/bin/env python3
"""The security class and advisory ids of a release candidate (docs/architecture/doc_pipeline.md,
PL55, ADR-0127), derived per repository from durable data only: the release-class.toml files
committed between the baseline's commit and the candidate's, and the Fedora security advisories
fixed by the packages that differ between the baseline's and the candidate's RPM databases.

The databases are read without running any code of either image, since this job decides which
release is signed as security: `podman create` (never started), `podman cp`, `podman rm`, then
the runner's own rpm.
"""

import datetime
import gzip
import hashlib
import lzma
import pathlib
import re
import subprocess
import tempfile
import tomllib
import urllib.parse
import xml.etree.ElementTree as ET
from typing import NamedTuple

CLASS_GLOB = "release/class/*.toml"
METALINK = "https://mirrors.fedoraproject.org/metalink?repo=updates-released-f{}&arch=x86_64"
ALNUM = frozenset("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
DIGITS = frozenset("0123456789")


class ClassError(Exception):
    pass


class Advisory(NamedTuple):
    id: str
    type: str
    fixed: dict[str, str]  # package name -> fixed "epoch:version-release"


class Image(NamedTuple):
    ref: str  # repository@digest
    digest: str


class Baseline(NamedTuple):
    ref: str
    digest: str
    revision: str | None  # the org.opencontainers.image.revision label, if any


class Sources:
    """What derive reads outside git. The real one is PodmanSources; tests pass a fake."""

    repo_root: pathlib.Path

    def packages(self, ref):
        raise NotImplementedError

    def updateinfo(self, ref):
        raise NotImplementedError


# rpmvercmp and labelCompare, in the standard library.


def vercmp(a, b):
    if a == b:
        return 0
    i = j = 0
    while i < len(a) or j < len(b):
        while i < len(a) and a[i] not in ALNUM and a[i] not in "~^":
            i += 1
        while j < len(b) and b[j] not in ALNUM and b[j] not in "~^":
            j += 1
        ta, tb = a[i : i + 1] == "~", b[j : j + 1] == "~"
        if ta or tb:
            if not ta:
                return 1
            if not tb:
                return -1
            i, j = i + 1, j + 1
            continue
        ca, cb = a[i : i + 1] == "^", b[j : j + 1] == "^"
        if ca or cb:
            if i >= len(a):
                return -1
            if j >= len(b):
                return 1
            if not ca:
                return 1
            if not cb:
                return -1
            i, j = i + 1, j + 1
            continue
        if i >= len(a) or j >= len(b):
            break
        kind = DIGITS if a[i] in DIGITS else ALNUM - DIGITS
        si, sj = i, j
        while i < len(a) and a[i] in kind:
            i += 1
        while j < len(b) and b[j] in kind:
            j += 1
        left, right = a[si:i], b[sj:j]
        if not right:
            return 1 if kind is DIGITS else -1
        if kind is DIGITS:
            left, right = left.lstrip("0"), right.lstrip("0")
            if len(left) != len(right):
                return 1 if len(left) > len(right) else -1
        if left != right:
            return 1 if left > right else -1
    if i >= len(a) and j >= len(b):
        return 0
    return -1 if i >= len(a) else 1


def evr(text):
    """'epoch:version-release' -> (epoch, version, release); an absent epoch is 0."""
    epoch, _, rest = text.rpartition(":")
    version, _, release = rest.rpartition("-")
    if not version:
        raise ClassError(f"'{text}' is not epoch:version-release")
    return int(epoch or 0), version, release


def evrcmp(a, b):
    (ea, va, ra), (eb, vb, rb) = evr(a), evr(b)
    return (ea > eb) - (ea < eb) or vercmp(va, vb) or vercmp(ra, rb)


# The git half


def git(root, *args):
    r = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    if r.returncode:
        raise ClassError(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def git_half(base, candidate, repo_root, repository):
    """The advisory ids of the security release-class.toml files added or modified in
    base..candidate that name this repository, or None when there is none."""
    name = repository.rsplit("/", 1)[-1]
    if subprocess.run(
        ["git", "-C", str(repo_root), "cat-file", "-e", f"{base}^{{commit}}"],
        capture_output=True,
    ).returncode:
        raise ClassError(f"label commit {base} is not in the repository")
    paths = sorted(
        set(
            git(
                repo_root, "log", "--diff-filter=AM", "--name-only", "--format=",
                f"{base}..{candidate}", "--", CLASS_GLOB,
            ).split()
        )
    )
    ids, found = set(), False
    for path in paths:
        if subprocess.run(
            ["git", "-C", str(repo_root), "cat-file", "-e", f"{candidate}:{path}"],
            capture_output=True,
        ).returncode:
            continue  # added in the range and deleted again: the candidate does not carry it
        try:
            doc = tomllib.loads(git(repo_root, "show", f"{candidate}:{path}"))
        except tomllib.TOMLDecodeError as error:
            raise ClassError(f"{path}: {error}") from None
        extra = sorted(set(doc) - {"class", "advisories", "repositories"})
        advisories, scope = doc.get("advisories"), doc.get("repositories")
        if (
            doc.get("class") != "security"
            or extra
            or not isinstance(advisories, list)
            or not advisories
            or not all(isinstance(a, str) and a for a in advisories)
            or not (scope is None or isinstance(scope, list) and all(isinstance(s, str) for s in scope))
        ):
            raise ClassError(
                f"{path}: needs class = \"security\", a non-empty advisories list of strings and, optionally, repositories"
            )
        if scope is None or name in scope:
            found = True
            ids.update(advisories)
    return sorted(ids) if found else None


# The Fedora half


def parse_updateinfo(xml):
    """(the newest update date as epoch seconds, the advisories)."""
    try:
        root = ET.fromstring(xml)
    except ET.ParseError as error:
        raise ClassError(f"updateinfo is not XML: {error}") from None
    advisories, newest = [], 0
    for update in root.iter("update"):
        fixed = {}
        for p in update.iter("package"):
            fixed[p.get("name")] = f"{p.get('epoch') or 0}:{p.get('version')}-{p.get('release')}"
        advisories.append(Advisory(update.findtext("id", ""), update.get("type", ""), fixed))
        for tag in ("updated", "issued"):
            node = update.find(tag)
            if node is not None and node.get("date"):
                newest = max(newest, int(_epoch(node.get("date"))))
    return newest, advisories


def _epoch(date):
    if date.isdigit():
        return int(date)
    parsed = datetime.datetime.fromisoformat(date.replace(" ", "T"))
    return parsed.replace(tzinfo=parsed.tzinfo or datetime.timezone.utc).timestamp()


def fedora_half(old, new, advisories):
    """Ids of the security advisories whose fixed version lies after the old one and at or
    before the new one, for a package that differs between the two."""
    out = set()
    for a in advisories:
        if a.type != "security":
            continue
        for name, fixed in a.fixed.items():
            if (
                name in old
                and name in new
                and old[name] != new[name]
                and evrcmp(fixed, old[name]) > 0
                and evrcmp(fixed, new[name]) <= 0
            ):
                out.add(a.id)
    return sorted(out)


class PodmanSources(Sources):
    def __init__(self, repo_root):
        self.repo_root = repo_root
        self._seen = {}

    def _run(self, *args, text=True):
        r = subprocess.run(args, capture_output=True, text=text)
        if r.returncode:
            err = r.stderr.strip() if text else r.stderr.decode(errors="replace").strip()
            raise ClassError(f"{' '.join(args)}: {err}")
        return r.stdout

    def _read(self, ref):
        """The RPM names and versions and the VERSION_ID of ref, from a container that is
        created, copied from and removed, never started."""
        if ref in self._seen:
            return self._seen[ref]
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            ctr = self._run("podman", "create", "--entrypoint", "/bin/true", ref).strip()
            try:
                self._run("podman", "cp", f"{ctr}:/usr/lib/sysimage/rpm", str(tmp / "rpm"))
                self._run("podman", "cp", f"{ctr}:/usr/lib/os-release", str(tmp / "os-release"))
            finally:
                self._run("podman", "rm", ctr)
            listing = self._run(
                "rpm", "--dbpath", str(tmp / "rpm"), "-qa",
                "--qf", r"%{NAME} %|EPOCH?{%{EPOCH}}:{0}|:%{VERSION}-%{RELEASE}\n",
            )
            match = re.search(r'^VERSION_ID="?([0-9]+)"?$', (tmp / "os-release").read_text(), re.M)
        if not match:
            raise ClassError(f"{ref}: os-release has no VERSION_ID")
        packages = {}
        for line in listing.splitlines():
            name, version = line.split()
            if name not in packages or evrcmp(version, packages[name]) > 0:
                packages[name] = version
        self._seen[ref] = packages, match.group(1)
        return self._seen[ref]

    def packages(self, ref):
        return self._read(ref)[0]

    def _fetch(self, url):
        return self._run("curl", "-fsSL", "--retry", "3", "--max-time", "120", url, text=False)

    def updateinfo(self, ref):
        """updateinfo.xml of the updates repository of ref's VERSION_ID, through the Fedora
        metalink, with the repomd.xml and the updateinfo checked against their checksums."""
        try:
            metalink = ET.fromstring(self._fetch(METALINK.format(self._read(ref)[1])))
        except ET.ParseError as error:
            raise ClassError(f"the Fedora metalink is not XML: {error}") from None
        tag = lambda e: e.tag.rsplit("}", 1)[-1]  # noqa: E731
        want = next(
            (h.text for h in metalink.iter() if tag(h) == "hash" and h.get("type") == "sha256"), None
        )
        urls = [u.text.strip() for u in metalink.iter() if tag(u) == "url" and u.text and u.text.strip().endswith("/repomd.xml")]
        if not want or not urls:
            raise ClassError("the Fedora metalink names no repomd.xml with a sha256")
        errors = []
        for url in urls:
            try:
                repomd = self._fetch(url)
                if hashlib.sha256(repomd).hexdigest() != want:
                    raise ClassError(f"{url} does not match the metalink checksum")
                return self._updateinfo(url.removesuffix("repodata/repomd.xml"), repomd)
            except ClassError as error:
                errors.append(str(error))
        raise ClassError("updateinfo unavailable: " + "; ".join(errors))

    def _updateinfo(self, base, repomd):
        try:
            root = ET.fromstring(repomd)
        except ET.ParseError as error:
            raise ClassError(f"repomd.xml is not XML: {error}") from None
        tag = lambda e: e.tag.rsplit("}", 1)[-1]  # noqa: E731
        data = next((d for d in root if tag(d) == "data" and d.get("type") == "updateinfo"), None)
        if data is None:
            raise ClassError("repomd.xml has no updateinfo")
        loc = next(e for e in data if tag(e) == "location").get("href")
        want = next(e.text for e in data if tag(e) == "checksum" and e.get("type") == "sha256")
        blob = self._fetch(urllib.parse.urljoin(base, loc))
        if hashlib.sha256(blob).hexdigest() != want:
            raise ClassError(f"{loc} does not match the repomd.xml checksum")
        if loc.endswith(".gz"):
            return gzip.decompress(blob)
        if loc.endswith(".xz"):
            return lzma.decompress(blob)
        if loc.endswith(".zst"):
            r = subprocess.run(["zstd", "-dc"], input=blob, capture_output=True)
            if r.returncode:
                raise ClassError(f"zstd: {r.stderr.decode(errors='replace').strip()}")
            return r.stdout
        return blob


def derive(repository, baseline, candidate, commit, sources):
    """The ClassRecord of one repository. `commit` is the candidate's commit."""
    record = {
        "repository": repository,
        "baseline_digest": baseline.digest if baseline else None,
        "candidate_digest": candidate.digest,
        "class": "feature",
        "advisories": [],
        "class_reason": "no-baseline",
        "updateinfo_timestamp": None,
    }
    if baseline is None:
        return record
    try:
        ids, record["class_reason"] = set(), ""
        if baseline.revision:
            ids.update(git_half(baseline.revision, commit, sources.repo_root, repository) or [])
        else:
            record["class_reason"] = "unlabelled-baseline"
        stamp, advisories = parse_updateinfo(sources.updateinfo(candidate.ref))
        ids.update(
            fedora_half(sources.packages(baseline.ref), sources.packages(candidate.ref), advisories)
        )
    except ClassError as error:
        raise ClassError(f"{repository}: {error}") from None
    record["updateinfo_timestamp"] = stamp
    record["advisories"] = sorted(ids)
    if ids:
        record["class"] = "security"
    return record
