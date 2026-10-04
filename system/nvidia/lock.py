#!/usr/bin/env python3
"""Locks for the third-party NVIDIA packages of the system image variants
(docs/architecture/doc_system_image.md, S4, S5 and S7): the driver of the open and legacy
branches, and NVIDIA's container toolkit (the container-toolkit set) both variants ship.

  lock.py generate SET --version V   resolve the set's packages at exactly V in its
                                    repository and write locks/<SET>.lock with the SHA-256 of
                                    each downloaded RPM, checked against the metadata first
  lock.py fetch SET --out DIR        download the locked RPMs and verify each SHA-256;
                                    prints the lock's version
  lock.py check SET --version V      exit 0 if the repository publishes the set at V
  lock.py latest SET --major M       print the newest version M.* of the set's primary
                                    package in the repository metadata
  lock.py verify SET --version V     compare locks/<SET>.lock, which must be at V, with the
                                    repository metadata; RPMs are downloaded only from a
                                    repository whose metadata gives another checksum than SHA-256
  lock.py mirrored SET               print the mirror reference of locks/<SET>.lock and exit 0
                                    if the mirror holds every locked RPM

SET is open, legacy or container-toolkit (BRANCHES).

The mirror (doc_system_image.md, S7) is the OCI repository $KERNEL_REGISTRY/athanor-nvidia-rpms
(default ghcr.io/<owner>): mirror.sh pushes each locked RPM as a blob, so its digest is the
SHA-256 the lock records and the lock is the mirror's index. fetch takes each RPM from the
mirror, anonymously, and from the lock's URL when the mirror lacks it: the vendor repositories
keep only the newest builds, so a lock outlives their files.

Exit codes: 0 success, 3 the requested version is not published, 4 (verify) the repository
publishes V with other files or checksums than the lock, 5 (mirrored) the mirror lacks a locked
RPM, 1 any other error (network, metadata, lock file), 2 usage.

Locks always record SHA-256, the mirror's blob digest; a repository whose metadata gives
SHA-512 (NVIDIA's) is checked by SHA-512 when the lock is written. GPG signatures are verified
by build-rpms.sh with rpmkeys and the keys in keys/: the lock covers what the unsigned
repository metadata of negativo17 cannot.
"""

import argparse
import functools
import gzip
import hashlib
import json
import os
import pathlib
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import xml.parsers.expat

HERE = pathlib.Path(__file__).resolve().parent
LOCKS = HERE / "locks"
COMMON = "{http://linux.duke.edu/metadata/common}"
REPO = "{http://linux.duke.edu/metadata/repo}"
ARCHES = ("x86_64", "noarch")
NOT_PUBLISHED = 3
STALE = 4
NOT_MIRRORED = 5
MIRROR = "athanor-nvidia-rpms"
# The repository metadata checksums a lock can be generated from, with their names in messages.
CHECKSUMS = {"sha256": "SHA-256", "sha512": "SHA-512"}
BRANCHES = {
    "open": {
        "primary": "nvidia-driver",
        "baseurl": "https://negativo17.org/repos/nvidia/fedora-43/x86_64/",
        "packages": [
            "nvidia-driver", "nvidia-driver-common", "nvidia-driver-cuda", "nvidia-driver-cuda-libs",
            "nvidia-driver-libs", "nvidia-kmod-common", "nvidia-modprobe", "nvidia-persistenced",
        ],
        # nvidia-kmod-common requires (nvidia-driver-selinux if selinux-policy-targeted).
        "companions": ["nvidia-driver-selinux"],
    },
    "legacy": {
        "primary": "xorg-x11-drv-nvidia",
        "baseurl": "https://download1.rpmfusion.org/nonfree/fedora/updates/43/x86_64/",
        "packages": [
            "nvidia-modprobe", "nvidia-persistenced", "nvidia-settings", "xorg-x11-drv-nvidia",
            "xorg-x11-drv-nvidia-cuda", "xorg-x11-drv-nvidia-cuda-libs", "xorg-x11-drv-nvidia-libs",
            "xorg-x11-drv-nvidia-power",
        ],
        "companions": [],
    },
    # CDI specs and the OCI hook for GPU containers (S4). NVIDIA's repository, not Fedora's
    # golang-github-nvidia-container-toolkit, which lags the security fixes. Not a kernel input:
    # its version lives in the lock alone, not in pins.env.
    "container-toolkit": {
        "primary": "nvidia-container-toolkit",
        "baseurl": "https://nvidia.github.io/libnvidia-container/stable/rpm/x86_64/",
        "packages": [
            "libnvidia-container-tools", "libnvidia-container1", "nvidia-container-toolkit",
            "nvidia-container-toolkit-base",
        ],
        "companions": [],
    },
}


class LockError(Exception):
    """A lock that cannot be produced or honoured; the message names the package or file."""


class NotPublished(LockError):
    """The repository does not publish the branch at the requested version."""


def http_get(url):
    with urllib.request.urlopen(url, timeout=120) as response:
        return response.read()


def mirror_repository():
    """<registry>/athanor-nvidia-rpms, the registry resolved as system/kernel-artifacts.sh does."""
    owner = os.environ.get("GITHUB_REPOSITORY_OWNER", "hr-mes").lower()
    return f"{os.environ.get('KERNEL_REGISTRY') or f'ghcr.io/{owner}'}/{MIRROR}"


def mirror_tag(branch, version, lock_path):
    """One tag per lock content: a relock never retags the blobs an older lock still names."""
    return f"{branch}-{version}-{hashlib.sha256(lock_path.read_bytes()).hexdigest()[:12]}"


def bearer_challenge(header):
    """The token URL of a `WWW-Authenticate: Bearer realm=...,service=...,scope=...` challenge."""
    params = dict(re.findall(r'(\w+)="([^"]*)"', header or ""))
    if not header or not header.lower().startswith("bearer ") or "realm" not in params:
        raise LockError(f"unsupported registry authentication challenge: {header!r}")
    realm = params.pop("realm")
    return realm + ("&" if "?" in realm else "?") + urllib.parse.urlencode(params)


class Mirror:
    """Anonymous reads of the OCI mirror by digest (OCI distribution spec): the token comes from
    the registry's own Bearer challenge, so any registry that allows anonymous pulls works."""

    def __init__(self, repository):
        self.host, _, self.name = repository.partition("/")
        self.token = None

    def _open(self, sha, method):
        url = f"https://{self.host}/v2/{self.name}/blobs/sha256:{sha}"
        for attempt in (1, 2):
            request = urllib.request.Request(url, method=method)
            if self.token:
                # Unredirected: the blob redirects to a CDN URL that must not see the token.
                request.add_unredirected_header("Authorization", f"Bearer {self.token}")
            try:
                return urllib.request.urlopen(request, timeout=120)
            except urllib.error.HTTPError as error:
                if error.code != 401 or attempt == 2:
                    raise
                with urllib.request.urlopen(bearer_challenge(error.headers.get("WWW-Authenticate")), timeout=60) as reply:
                    body = json.load(reply)
                self.token = body.get("token") or body.get("access_token")
                if not self.token:
                    raise LockError(f"{self.host}: the token endpoint returned no token")
        raise AssertionError("unreachable")

    def has(self, sha):
        try:
            with self._open(sha, "HEAD"):
                return True
        except urllib.error.HTTPError as error:
            # 403: ghcr's answer for a repository that is private or does not exist yet.
            if error.code in (403, 404):
                return False
            raise

    def get(self, sha):
        with self._open(sha, "GET") as response:
            return response.read()


def from_mirror(mirror, sha, name):
    """The RPM's bytes from the mirror, or None when the mirror cannot give the locked ones."""
    try:
        data = mirror.get(sha)
    except (LockError, OSError) as error:
        print(f"lock.py: {name}: not in the mirror ({error}), using the lock's URL", file=sys.stderr)
        return None
    if hashlib.sha256(data).hexdigest() != sha:
        print(f"lock.py: {name}: the mirror returned other bytes, using the lock's URL", file=sys.stderr)
        return None
    return data


def parse_xml(data):
    """Repository metadata comes from the network: refuse any DTD, so no entity can expand
    or resolve. The expat DOCTYPE and ENTITY handlers below are the enforcement; the NUL and
    strict UTF-8 checks are an extra layer that refuses non-UTF-8 encodings before parsing."""
    if not data:
        raise LockError("repository metadata is empty: refused")
    if b"\x00" in data:
        raise LockError("repository metadata contains NUL bytes: refused")
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        raise LockError("repository metadata is not UTF-8: refused")
    # Single-pass parse: expat parser with TreeBuilder and DTD/entity refusal
    def _refuse(*_):
        raise LockError("repository metadata declares a DTD or entities: refused")
    parser = xml.parsers.expat.ParserCreate(namespace_separator="}")
    builder = ET.TreeBuilder()
    # Wrap TreeBuilder handlers to convert namespace format from "ns}tag" to "{ns}tag"
    def _start_element(name, attrs):
        if "}" in name:
            ns, tag = name.split("}", 1)
            name = f"{{{ns}}}{tag}"
        builder.start(name, attrs)
    def _end_element(name):
        if "}" in name:
            ns, tag = name.split("}", 1)
            name = f"{{{ns}}}{tag}"
        builder.end(name)
    parser.StartElementHandler = _start_element
    parser.EndElementHandler = _end_element
    parser.CharacterDataHandler = builder.data
    parser.StartDoctypeDeclHandler = _refuse
    parser.EntityDeclHandler = _refuse
    try:
        parser.Parse(data, True)
        return builder.close()
    except LockError:
        raise
    except xml.parsers.expat.ExpatError as e:
        raise LockError(str(e))


def attribute(element, tag, name, what):
    """`name` of the child `tag` of `element`, or LockError naming `what`."""
    child = element.find(tag)
    value = None if child is None else child.get(name)
    if not value:
        raise LockError(f"{what}: no {tag.rsplit('}', 1)[-1]} {name} in the repository metadata")
    return value


def primary_href(repomd):
    for data in parse_xml(repomd).iter(f"{REPO}data"):
        if data.get("type") == "primary":
            return attribute(data, f"{REPO}location", "href", "repomd.xml primary")
    raise LockError("repomd.xml has no primary metadata")


def rpmvercmp(a, b):
    """rpm's rpmvercmp: alphanumeric segments, numbers above letters, '~' sorts before
    anything (even the end of the string), '^' after the end but before any segment."""
    i = j = 0
    while i < len(a) or j < len(b):
        while i < len(a) and not (a[i].isascii() and a[i].isalnum()) and a[i] not in "~^":
            i += 1
        while j < len(b) and not (b[j].isascii() and b[j].isalnum()) and b[j] not in "~^":
            j += 1
        x, y = a[i:i + 1], b[j:j + 1]
        if "~" in (x, y):
            if x != y:
                return -1 if x == "~" else 1
            i, j = i + 1, j + 1
            continue
        if "^" in (x, y):
            if not x:
                return -1
            if not y:
                return 1
            if x != y:
                return 1 if y == "^" else -1
            i, j = i + 1, j + 1
            continue
        if not (x and y):
            break
        digits = x.isdigit()
        kind = str.isdigit if digits else str.isalpha
        si, sj = i, j
        while i < len(a) and a[i].isascii() and kind(a[i]):
            i += 1
        while j < len(b) and b[j].isascii() and kind(b[j]):
            j += 1
        one, two = a[si:i], b[sj:j]
        if not two:
            return 1 if digits else -1
        if digits:
            one, two = one.lstrip("0"), two.lstrip("0")
            if len(one) != len(two):
                return -1 if len(one) < len(two) else 1
        if one != two:
            return -1 if one < two else 1
    if i >= len(a) and j >= len(b):
        return 0
    return -1 if i >= len(a) else 1


def evr_compare(one, two):
    """Order of two (epoch, version, release) tuples as rpm sorts them; epochs are integers."""
    if one[0] != two[0]:
        return -1 if one[0] < two[0] else 1
    return rpmvercmp(one[1], two[1]) or rpmvercmp(one[2], two[2])


def select(primary_xml, names, version, companions=()):
    """One entry per name at exactly `version`, and one per companion (a package whose version
    does not follow the driver's) at any version, for x86_64 or noarch. Each takes its newest
    published release: a repository may keep several rebuilds of one version, and the lock
    records the chosen file by SHA-256 either way."""
    releases = {name: [] for name in (*names, *companions)}
    for pkg in parse_xml(primary_xml).iter(f"{COMMON}package"):
        name = pkg.findtext(f"{COMMON}name")
        if name not in releases:
            continue
        ver = attribute(pkg, f"{COMMON}version", "ver", name)
        if pkg.findtext(f"{COMMON}arch") not in ARCHES or (name in names and ver != version):
            continue
        checksum = attribute(pkg, f"{COMMON}checksum", "type", name)
        if checksum not in CHECKSUMS:
            raise LockError(f"{name}: checksum type {checksum}, {' or '.join(CHECKSUMS)} required")
        digest = pkg.findtext(f"{COMMON}checksum")
        if not digest:
            raise LockError(f"{name}: empty checksum in the repository metadata")
        entry = {"name": name, "href": attribute(pkg, f"{COMMON}location", "href", name),
                 "checksum": checksum, "digest": digest}
        epoch = pkg.find(f"{COMMON}version").get("epoch") or "0"
        if not epoch.isascii() or not epoch.isdigit():
            raise LockError(f"{name}: epoch {epoch!r} in the repository metadata is not a number")
        evr = (int(epoch), ver, attribute(pkg, f"{COMMON}version", "rel", name))
        releases[name].append((evr, entry))
    missing = sorted(n for n in names if not releases[n])
    if missing:
        raise NotPublished(f"not published at {version}: {', '.join(missing)}")
    missing = sorted(n for n in companions if not releases[n])
    if missing:
        raise LockError(f"not published: {', '.join(missing)}")
    chosen = {}
    for name, published in releases.items():
        newest = max((evr for evr, _ in published), key=functools.cmp_to_key(evr_compare))
        top = [entry for evr, entry in published if evr_compare(evr, newest) == 0]
        if len(top) > 1:
            raise LockError(f"ambiguous at its newest release: {name}")
        chosen[name] = top[0]
    return [chosen[n] for n in sorted(chosen)]


def vtuple(version):
    return tuple(int(part) for part in version.split("."))


def newest(primary_xml, name, major):
    """The highest version `major`.* of package `name`, for x86_64 or noarch."""
    versions = {
        attribute(pkg, f"{COMMON}version", "ver", name)
        for pkg in parse_xml(primary_xml).iter(f"{COMMON}package")
        if pkg.findtext(f"{COMMON}name") == name and pkg.findtext(f"{COMMON}arch") in ARCHES
    }
    try:
        in_major = [v for v in versions if v.split(".")[0] == major]
        best = max(in_major, key=vtuple, default=None)
    except ValueError:
        raise LockError(f"{name}: non-numeric version among {', '.join(sorted(versions))}")
    if best is None:
        raise NotPublished(f"{name}: no version {major}.* published")
    return best


def write_lock(path, branch, version, baseurl, entries):
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = [f"# branch {branch}", f"# version {version}", f"# repository {baseurl}"]
    lines += [f"{sha}  {url}" for sha, url in sorted(entries, key=lambda e: e[1])]
    path.write_text("\n".join(lines) + "\n")


def read_lock(path):
    branch = version = baseurl = None
    entries = []
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if line.startswith("# branch "):
            branch = line.split(" ", 2)[2]
        elif line.startswith("# version "):
            version = line.split(" ", 2)[2]
        elif line.startswith("# repository "):
            baseurl = line.split(" ", 2)[2]
        elif line and not line.startswith("#"):
            sha, sep, url = line.partition("  ")
            if not sep or len(sha) != 64 or any(c not in "0123456789abcdef" for c in sha) or not url or " " in url:
                raise LockError(f"{path}:{number}: malformed lock line, expected '<sha256>  <url>'")
            entries.append((sha, url))
    if not branch or not version or not baseurl or not entries:
        raise LockError(f"{path}: incomplete lock")
    return branch, version, baseurl, entries


def primary_xml(branch, download):
    base = BRANCHES[branch]["baseurl"]
    href = primary_href(download(base + "repodata/repomd.xml"))
    return gzip.decompress(download(base + href))


def resolve(branch, version, download):
    return select(primary_xml(branch, download), BRANCHES[branch]["packages"], version,
                  BRANCHES[branch]["companions"])


def downloaded_sha256(entry, url, download):
    """The SHA-256 of the RPM at `url`, once its bytes match the metadata's checksum."""
    data = download(url)
    got = hashlib.new(entry["checksum"], data).hexdigest()
    if got != entry["digest"]:
        raise LockError(f"{url}: downloaded {CHECKSUMS[entry['checksum']]} {got} differs from the repository metadata {entry['digest']}")
    return hashlib.sha256(data).hexdigest()


def load_lock(locks, branch):
    """The entries of locks/<branch>.lock, refused unless it belongs to the branch's repository."""
    path = locks / f"{branch}.lock"
    locked_branch, version, baseurl, entries = read_lock(path)
    if locked_branch != branch:
        raise LockError(f"{path}: lock of branch {locked_branch}, {branch} requested")
    if baseurl != BRANCHES[branch]["baseurl"]:
        raise LockError(f"{path}: repository {baseurl}, the {branch} branch uses {BRANCHES[branch]['baseurl']}")
    outside = [url for _, url in entries if not url.startswith(baseurl)]
    if outside:
        raise LockError(f"{path}: URL outside the repository {baseurl}: {', '.join(outside)}")
    return path, version, entries


def main(argv=None, download=http_get, mirror=None):
    parser = argparse.ArgumentParser(description="Locks for the third-party NVIDIA RPMs.")
    parser.add_argument("command", choices=("generate", "fetch", "check", "latest", "verify", "mirrored"))
    parser.add_argument("branch", choices=sorted(BRANCHES))
    parser.add_argument("--version")
    parser.add_argument("--major")
    parser.add_argument("--out", type=pathlib.Path)
    parser.add_argument("--locks", type=pathlib.Path, default=LOCKS)
    args = parser.parse_args(argv)
    try:
        if args.command in ("generate", "check", "verify") and not args.version:
            raise LockError(f"{args.command} needs --version")
        if args.command == "check":
            resolve(args.branch, args.version, download)
            return 0
        if args.command == "latest":
            if not args.major:
                raise LockError("latest needs --major")
            print(newest(primary_xml(args.branch, download), BRANCHES[args.branch]["primary"], args.major))
            return 0
        if args.command == "verify":
            path, version, entries = load_lock(args.locks, args.branch)
            if version != args.version:
                raise LockError(f"{path}: lock at {version}, {args.version} expected")
            base = BRANCHES[args.branch]["baseurl"]
            published = {
                (e["digest"] if e["checksum"] == "sha256" else downloaded_sha256(e, base + e["href"], download), base + e["href"])
                for e in resolve(args.branch, version, download)
            }
            if published != set(entries):
                changed = sorted(url for _, url in published.symmetric_difference(entries))
                print(f"lock.py: {path} differs from the repository metadata at {version}: {', '.join(changed)}", file=sys.stderr)
                return STALE
            return 0
        if args.command == "generate":
            base = BRANCHES[args.branch]["baseurl"]
            entries = []
            for entry in resolve(args.branch, args.version, download):
                url = base + entry["href"]
                entries.append((downloaded_sha256(entry, url, download), url))
            write_lock(args.locks / f"{args.branch}.lock", args.branch, args.version, base, entries)
            return 0
        mirror = mirror or Mirror(mirror_repository())
        if args.command == "mirrored":
            path, version, entries = load_lock(args.locks, args.branch)
            print(f"{mirror_repository()}:{mirror_tag(args.branch, version, path)}")
            missing = [url.rsplit("/", 1)[1] for sha, url in entries if not mirror.has(sha)]
            if missing:
                print(f"lock.py: the mirror lacks {', '.join(missing)}", file=sys.stderr)
                return NOT_MIRRORED
            return 0
        if not args.out:
            raise LockError("fetch needs --out")
        _, version, entries = load_lock(args.locks, args.branch)
        args.out.mkdir(parents=True, exist_ok=True)
        for sha, url in entries:
            name = url.rsplit("/", 1)[1]
            data = from_mirror(mirror, sha, name)
            source = "the mirror"
            if data is None:
                data, source = download(url), url
            got = hashlib.sha256(data).hexdigest()
            if got != sha:
                raise LockError(f"{url}: SHA-256 {got}, locked {sha}")
            (args.out / name).write_bytes(data)
            print(f"lock.py: {name} from {source}", file=sys.stderr)
        print(version)
        return 0
    except NotPublished as error:
        print(f"lock.py: {error}", file=sys.stderr)
        return NOT_PUBLISHED
    except (LockError, OSError) as error:
        print(f"lock.py: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
