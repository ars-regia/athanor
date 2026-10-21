#!/usr/bin/env python3
"""The security class and advisory ids of a release candidate (docs/architecture/doc_pipeline.md,
PL55, ADR-0127), derived per repository from durable data only: the release-class.toml files
committed between the baseline's commit and the candidate's, and the Fedora security advisories
fixed by the packages that differ between the baseline's and the candidate's RPM databases.

The databases are read without running any code of either image, since this job decides which
release is signed as security: `podman create` (never started), `podman cp`, `podman rm`, then
the runner's own rpm.
"""

import gzip
import hashlib
import lzma
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib
import urllib.parse
import xml.etree.ElementTree as ET
from typing import NamedTuple

CLASS_DIR = "release/class/"
CLASS_NAME = re.compile(r"^release/class/[0-9]{4}-[0-9]{2}-[0-9]{2}-[a-z0-9-]+\.toml$")
ADVISORY_ID = re.compile(r"^[A-Za-z0-9._-]+$")
SANITY = ("filesystem", "glibc")  # every Fedora image has them: an empty list is a failed read
TIMEOUT = 900  # seconds for one external command; a pull of an image is the longest
MAX_FILESIZE = 256 * 1024 * 1024  # bytes of one download from a Fedora mirror
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
        """(the timestamp of the updateinfo entry of repomd.xml or None, updateinfo.xml)"""
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
    if not version or not epoch.isdigit() and epoch:
        raise ClassError(f"'{text}' is not epoch:version-release")
    return int(epoch or 0), version, release


def evrcmp(a, b):
    (ea, va, ra), (eb, vb, rb) = evr(a), evr(b)
    return (ea > eb) - (ea < eb) or vercmp(va, vb) or vercmp(ra, rb)


# The git half


def git(root, *args, ok=(0,)):
    """stdout of git; a status outside ok is an error, never an absence."""
    try:
        r = subprocess.run(
            ["git", "-C", str(root), *args], capture_output=True, text=True, timeout=TIMEOUT
        )
    except subprocess.TimeoutExpired:
        raise ClassError(f"git {' '.join(args)}: timed out") from None
    if r.returncode not in ok:
        raise ClassError(f"git {' '.join(args)}: exit {r.returncode}: {r.stderr.strip()}")
    return r.stdout if r.returncode == 0 else None


def nul(text):
    return [p for p in text.split("\0") if p]


def git_half(base, candidate, repo_root, repository):
    """The advisory ids of the security release-class.toml files that the candidate carries
    and whose last touching commit lies in base..candidate, for this repository, or None when
    there is none. A path under release/class/ with another name than the convention, at the
    candidate or touched in the range, is an error."""
    name = repository.rsplit("/", 1)[-1]
    if git(repo_root, "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}", ok=(0, 1)) is None:
        raise ClassError(f"label commit {base} is not in the repository")
    carried = nul(git(repo_root, "ls-tree", "-r", "-z", "--name-only", candidate, "--", CLASS_DIR))
    touched = nul(
        git(
            repo_root, "log", "-z", "--name-only", "--format=", "--no-renames", "--full-history",
            f"{base}..{candidate}", "--", CLASS_DIR,
        )
    )  # fmt: skip
    bad = sorted(p for p in {*carried, *touched} if not CLASS_NAME.match(p))
    if bad:
        raise ClassError(f"{bad} under {CLASS_DIR} do not match <YYYY-MM-DD>-<slug>.toml")
    in_range = set(
        nul(
            git(
                repo_root, "rev-list", "-z", "--full-history", f"{base}..{candidate}", "--", CLASS_DIR
            )
        )
    )
    ids, found = set(), False
    for path in carried:
        last = nul(git(repo_root, "log", "-1", "-z", "--full-history", "--format=%H", candidate, "--", path))
        if not last or last[0] not in in_range:
            continue
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
            or not all(isinstance(a, str) and ADVISORY_ID.match(a) for a in advisories)
            or not (scope is None or isinstance(scope, list) and all(isinstance(s, str) for s in scope))
        ):
            raise ClassError(
                f"{path}: needs class = \"security\", a non-empty advisories list of ids and, optionally, repositories"
            )
        if scope is None or name in scope:
            found = True
            ids.update(advisories)
    return sorted(ids) if found else None


# The Fedora half


def parse_updateinfo(xml):
    """The advisories of an updateinfo.xml."""
    try:
        root = ET.fromstring(xml)
    except ET.ParseError as error:
        raise ClassError(f"updateinfo is not XML: {error}") from None
    advisories = []
    for update in root.iter("update"):
        fixed = {}
        for p in update.iter("package"):
            if not (p.get("name") and p.get("version") and p.get("release")):
                raise ClassError(f"advisory {update.findtext('id', '')}: a package lacks name, version or release")
            fixed[p.get("name")] = f"{p.get('epoch') or 0}:{p.get('version')}-{p.get('release')}"
        advisories.append(Advisory(update.findtext("id", ""), update.get("type", ""), fixed))
    return advisories


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


def run(*args, text=True, timeout=TIMEOUT, input=None):
    try:
        r = subprocess.run(args, capture_output=True, text=text, timeout=timeout, input=input)
    except subprocess.TimeoutExpired:
        raise ClassError(f"{' '.join(args)}: timed out") from None
    if r.returncode:
        err = r.stderr if text else r.stderr.decode(errors="replace")
        raise ClassError(f"{' '.join(args)}: {err.strip()}")
    return r.stdout


def local_name(e):
    return e.tag.rsplit("}", 1)[-1]


class PodmanSources(Sources):
    def __init__(self, repo_root):
        self.repo_root = repo_root
        self._seen = {}

    def _read(self, ref):
        """The RPM names and versions and the VERSION_ID of ref, from a container that is
        created, copied from and removed, never started; the pulled image is removed after."""
        if ref in self._seen:
            return self._seen[ref]
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            failure = None
            ctr = run("podman", "create", "--entrypoint", "/bin/true", ref).strip()
            try:
                run("podman", "cp", f"{ctr}:/usr/lib/sysimage/rpm", str(tmp / "rpm"))
                run("podman", "cp", f"{ctr}:/usr/lib/os-release", str(tmp / "os-release"))
            except ClassError as error:
                failure = error
            cleanup = []
            for cmd in (("podman", "rm", ctr), ("podman", "rmi", ref)):
                try:
                    run(*cmd)
                except ClassError as error:
                    cleanup.append(str(error))
            if failure:
                raise ClassError(f"{ref}: {failure}" + (f" (cleanup also failed: {'; '.join(cleanup)})" if cleanup else ""))
            for line in cleanup:
                print(f"release_class: cleanup: {line}", file=sys.stderr)
            packages = self._packages(ref, tmp / "rpm")
            match = re.search(r'^VERSION_ID="?([0-9]+)"?$', (tmp / "os-release").read_text(), re.M)
        if not match:
            raise ClassError(f"{ref}: os-release has no VERSION_ID")
        self._seen[ref] = packages, match.group(1)
        return self._seen[ref]

    def _packages(self, ref, dbpath):
        if not (dbpath / "rpmdb.sqlite").is_file():
            raise ClassError(f"{ref}: no rpmdb.sqlite in /usr/lib/sysimage/rpm")
        listing = run(
            "rpm", "--dbpath", str(dbpath), "-qa",
            "--qf", r"%{NAME} %|EPOCH?{%{EPOCH}}:{0}|:%{VERSION}-%{RELEASE}\n",
            timeout=300,
        )  # fmt: skip
        packages = {}
        for line in listing.splitlines():
            name, version = line.split()
            if name not in packages or evrcmp(version, packages[name]) > 0:
                packages[name] = version
        missing = [n for n in SANITY if n not in packages]
        if missing:
            raise ClassError(f"{ref}: the RPM database lacks {missing}: an empty or unreadable database")
        return packages

    def packages(self, ref):
        return self._read(ref)[0]

    def _fetch(self, url):
        return run(
            "curl", "-fsSL", "--proto", "=https", "--proto-redir", "=https", "--retry", "3",
            "--max-time", "120", "--max-filesize", str(MAX_FILESIZE), "--", url, text=False,
        )  # fmt: skip

    def updateinfo(self, ref):
        """(timestamp, updateinfo.xml) of the updates repository of ref's VERSION_ID, through
        the Fedora metalink, with the repomd.xml and the updateinfo checked against their
        checksums; the timestamp is the one repomd.xml gives the updateinfo entry."""
        try:
            metalink = ET.fromstring(self._fetch(METALINK.format(self._read(ref)[1])))
        except ET.ParseError as error:
            raise ClassError(f"the Fedora metalink is not XML: {error}") from None
        want = next(
            (h.text for h in metalink.iter() if local_name(h) == "hash" and h.get("type") == "sha256"),
            None,
        )
        urls = [
            u.text.strip()
            for u in metalink.iter()
            if local_name(u) == "url"
            and u.text
            and u.text.strip().startswith("https://")
            and u.text.strip().endswith("/repomd.xml")
        ]
        if not want or not urls:
            raise ClassError("the Fedora metalink names no https repomd.xml with a sha256")
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
        data = next((d for d in root if local_name(d) == "data" and d.get("type") == "updateinfo"), None)
        if data is None:
            raise ClassError("repomd.xml has no updateinfo")
        field = {local_name(e): e for e in data}
        loc = field["location"].get("href") if "location" in field else None
        checksum = field.get("checksum")
        if not loc or checksum is None or checksum.get("type") != "sha256" or not checksum.text:
            raise ClassError("the updateinfo entry of repomd.xml has no location or sha256 checksum")
        stamp = None
        if "timestamp" in field:
            try:
                stamp = int(float(field["timestamp"].text))
            except (TypeError, ValueError):
                raise ClassError(f"updateinfo timestamp '{field['timestamp'].text}' is not a number") from None
        blob = self._fetch(urllib.parse.urljoin(base, loc))
        if hashlib.sha256(blob).hexdigest() != checksum.text:
            raise ClassError(f"{loc} does not match the repomd.xml checksum")
        if loc.endswith(".gz"):
            return stamp, gzip.decompress(blob)
        if loc.endswith(".xz"):
            return stamp, lzma.decompress(blob)
        if loc.endswith(".zst"):
            return stamp, run("zstd", "-dc", text=False, input=blob, timeout=300)
        return stamp, blob


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
        stamp, xml = sources.updateinfo(candidate.ref)
        ids.update(
            fedora_half(
                sources.packages(baseline.ref), sources.packages(candidate.ref), parse_updateinfo(xml)
            )
        )
        bad = sorted(i for i in ids if not ADVISORY_ID.match(i))
        if bad:
            raise ClassError(f"advisory ids {bad} are not of the form [A-Za-z0-9._-]+")
    except ClassError as error:
        raise ClassError(f"{repository}: {error}") from None
    record["updateinfo_timestamp"] = stamp
    record["advisories"] = sorted(ids)
    if ids:
        record["class"] = "security"
    return record
