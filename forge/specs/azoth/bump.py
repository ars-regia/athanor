#!/usr/bin/env python3
"""The bump bot of the Athanor kernel (docs/architecture/doc_kernel_build.md, section 8).

    bump.py check   prints to stdout a JSON with the current pins, the new ones and the notes
    bump.py apply   rewrites pins.env, the FROM lines of the Containerfiles and the pins
                    table of KERNEL.md; prints the PR body (Markdown) to stdout

Kernel pair (spec, section 2): for the X.Y series that both Fedora (stable, F43 then F44)
and CachyOS (GitHub releases of CachyOS/linux) ship, the highest patch level X.Y.Z present
on both sides. KERNEL_CHANNEL=stable takes the newest common series, lts the longterm
one. Without a pair the kernel stays where it is and a note says so. With the pair, the
head commit of CachyOS/kernel-patches for the series and the commit of
linux-cachyos/config in force at the date of the CachyOS release move as well.
Outside the kernel: the NVIDIA versions (open from the GitHub tags, legacy from RPM Fusion)
within the pinned branch, and the digest of the base image of the Containerfiles, the system
base quay.io base-atomic in system/Containerfile included. Every run also verifies the NVIDIA
locks in system/nvidia/locks against the repository metadata and regenerates a lock whose
pin moved or whose packages the repository republished. The kernel hash manifests are not here: `build.sh --stage manifest` and `nvidia.sh manifest` write
them. Standard library only: it runs on the GitHub runner without installing anything.
"""

import json
import os
import re
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
PINS = HERE / "pins.env"
# The kernel's Containerfiles and the system image (docs/architecture/doc_system_image.md, S1):
# every FROM pinned by digest moves in the same bump.
CONTAINERFILES = [HERE / d / "Containerfile" for d in ("builder", "boot", "nvidia")] + [HERE.parents[2] / "system" / "Containerfile"]
NVIDIA_LOCK = HERE.parents[2] / "system" / "nvidia" / "lock.py"
LOCK_NOT_PUBLISHED = 3  # lock.py's exit code for a version the repository does not publish
LOCK_STALE = 4  # lock.py verify: the repository publishes the version with other files or checksums
KERNEL_MD = HERE / "KERNEL.md"
FEDORA_RELEASES = ("F43", "F44")  # in order of preference for the same patch level
LTS_SERIES = "6.18"  # KERNEL_CHANNEL=lts: the longterm Fedora and CachyOS maintain
KERNEL_RELEASES = "https://www.kernel.org/releases.json"
BODHI = "https://bodhi.fedoraproject.org/updates/"
NVR_RE = re.compile(r"^kernel-(\d+\.\d+\.\d+)-(\d+)\.fc(\d+)$")
CACHY_TAG_RE = re.compile(r"^cachyos-(\d+\.\d+\.\d+)-(\d+)$")
FROM_RE = re.compile(r"^FROM (\S+?):(\S+?)@(sha256:[0-9a-f]{64})(?: AS \S+)?$", re.M)
MANIFEST_ACCEPT = ", ".join(
    [
        "application/vnd.oci.image.index.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
        "application/vnd.oci.image.manifest.v1+json",
        "application/vnd.docker.distribution.manifest.v2+json",
    ]
)


def vtuple(version):
    return tuple(int(x) for x in version.split("."))


def series(version):
    return ".".join(version.split(".")[:2])


def http(url, headers=None, method="GET"):
    req = urllib.request.Request(url, headers=headers or {}, method=method)
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.headers, resp.read()


def gh(url):
    """One request to the GitHub API, with the job token when present (60/hour without)."""
    headers = {"Accept": "application/vnd.github+json"}
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    head, body = http(url, headers)
    return head, json.loads(body)


def github(endpoint, **params):
    """The items of a list endpoint, page after page."""
    url = f"https://api.github.com/{endpoint}?{urllib.parse.urlencode(params)}"
    while url:
        head, items = gh(url)
        yield from items
        link = head.get("Link", "")
        url = next(
            (m.group(1) for m in re.finditer(r'<([^>]+)>; rel="next"', link)), None
        )


def read_pins():
    return dict(re.findall(r"^(\w+)=(.*)$", PINS.read_text(), re.M))


# --- kernel -----------------------------------------------------------------------


def fedora_kernels():
    """{patch level: NVR} of the stable kernel builds; F43 before F44 for the same version."""
    found = {}
    for release in FEDORA_RELEASES:
        page, pages = 1, 1
        while page <= pages:
            query = {
                "packages": "kernel",
                "releases": release,
                "status": "stable",
                "rows_per_page": 100,
                "page": page,
            }
            data = json.loads(http(f"{BODHI}?{urllib.parse.urlencode(query)}")[1])
            pages, page = data["pages"], page + 1
            for update in data["updates"]:
                for build in update["builds"]:
                    m = NVR_RE.match(build["nvr"])
                    if not m:
                        continue
                    version, rel, fc = m.groups()
                    if version not in found or (
                        found[version][0] == release and int(rel) > found[version][1]
                    ):
                        found[version] = (release, int(rel), f"{version}-{rel}.fc{fc}")
    return {v: nvr for v, (_, _, nvr) in found.items()}


def cachyos_releases():
    """{patch level: (tag, published_at)} of the latest CachyOS/linux release per version."""
    found = {}
    for rel in github("repos/CachyOS/linux/releases", per_page=100):
        m = CACHY_TAG_RE.match(rel["tag_name"])
        if not m or rel["draft"] or rel["prerelease"]:
            continue
        version, n = m.group(1), int(m.group(2))
        if version not in found or n > found[version][0]:
            found[version] = (n, rel["tag_name"], rel["published_at"])
    return {v: (tag, at) for v, (_, tag, at) in found.items()}


def kernel_pair(pins, notes):
    """(version, Fedora NVR, CachyOS tag, release date) of the chosen pair, or None."""
    fedora, cachy = fedora_kernels(), cachyos_releases()
    common = sorted(set(fedora) & set(cachy), key=vtuple)
    if pins["KERNEL_CHANNEL"] == "lts":
        common = [v for v in common if series(v) == LTS_SERIES]
    elif pins["KERNEL_CHANNEL"] != "stable":
        sys.exit(f"KERNEL_CHANNEL={pins['KERNEL_CHANNEL']}: expected stable or lts")
    newest_fedora, newest_cachy = max(fedora, key=vtuple), max(cachy, key=vtuple)
    if not common:
        notes.append(
            f"kernel: no Fedora/CachyOS pair (Fedora {newest_fedora}, CachyOS {newest_cachy}); the kernel stays at {pins['FEDORA_KERNEL_NVR']}"
        )
        return None
    version = common[-1]
    if vtuple(newest_fedora) > vtuple(version) or vtuple(newest_cachy) > vtuple(
        version
    ):
        notes.append(
            f"kernel: the highest pair is {version} (Fedora {fedora[version]}, CachyOS {cachy[version][0]}); "
            f"beyond it, unpaired: Fedora {newest_fedora}, CachyOS {newest_cachy}"
        )
    return version, fedora[version], cachy[version][0], cachy[version][1]


def maintained_series():
    """The X.Y series kernel.org still maintains: stable and longterm entries not marked EOL."""
    releases = json.loads(http(KERNEL_RELEASES)[1])["releases"]
    return {
        series(r["version"])
        for r in releases
        if r["moniker"] in ("stable", "longterm") and not r["iseol"]
    }


def head_commit(repo, path, until=None):
    params = {"path": path, "per_page": 1}
    if until:
        params["until"] = until
    return next(github(f"repos/{repo}/commits", **params))["sha"]


# --- NVIDIA and base image -----------------------------------------------------------


def nvidia_open(current):
    """Highest (tag, commit) of NVIDIA/open-gpu-kernel-modules in the pinned (major) branch."""
    major = current.split(".")[0]
    tags = [
        t["name"]
        for t in github("repos/NVIDIA/open-gpu-kernel-modules/tags", per_page=100)
    ]
    best = max(
        (t for t in tags if re.fullmatch(rf"{major}\.\d+(\.\d+)?", t)), key=vtuple
    )
    # The commits endpoint dereferences an annotated tag too: it is the commit nvidia.sh verifies.
    return best, gh(
        f"https://api.github.com/repos/NVIDIA/open-gpu-kernel-modules/commits/{best}"
    )[1]["sha"]


def nvidia_legacy(current):
    """The highest version of the pinned (major) branch that RPM Fusion publishes: the image
    installs its packages, so NVIDIA's download index is not the source."""
    return lock_py("latest", "legacy", "--major", current.split(".")[0]).stdout.strip()


def lock_py(*args, allowed=(0,)):
    """Run system/nvidia/lock.py; an exit code outside `allowed` aborts the bot with its stderr."""
    done = subprocess.run([sys.executable, "-B", str(NVIDIA_LOCK), *args], capture_output=True, text=True)
    if done.returncode not in allowed:
        sys.exit(f"lock.py {' '.join(args)}: exit {done.returncode}\n{done.stderr.strip()}")
    return done


def lock_check(branch, version):
    """True when the branch's driver repository publishes every locked package at version."""
    return lock_py("check", branch, "--version", version, allowed=(0, LOCK_NOT_PUBLISHED)).returncode == 0


def lock_verify(branch, version):
    """"ok", "stale" (the repository publishes version with other files or checksums) or "gone"."""
    code = lock_py("verify", branch, "--version", version, allowed=(0, LOCK_NOT_PUBLISHED, LOCK_STALE)).returncode
    return {0: "ok", LOCK_NOT_PUBLISHED: "gone", LOCK_STALE: "stale"}[code]


def packaged_or_current(branch, candidate, current, notes, check=lock_check):
    """The candidate NVIDIA version if its driver packages exist (doc_system_image.md, S7), else the current pin."""
    if candidate == current or check(branch, candidate):
        return candidate
    notes.append(f"NVIDIA {branch} {candidate} is tagged upstream but its driver packages are not published yet: the pin stays at {current}")
    return current


def nvidia_version(branch, candidate, current, notes, check=lock_check, verify=lock_verify):
    """(version, regenerate the lock) for one NVIDIA branch (doc_system_image.md, S7).

    A newer candidate the repository packages moves the pin. Otherwise the current lock is
    verified against the repository metadata on every run: a new release or new checksums
    of the same version regenerate it, and a version the repository dropped with nothing
    newer to move to stops the bot, because every image build would fail on that lock."""
    if vtuple(candidate) > vtuple(current):
        version = packaged_or_current(branch, candidate, current, notes, check)
        if version != current:
            return version, True
    state = verify(branch, current)
    if state == "gone":
        sys.exit(f"NVIDIA {branch} {current}: the repository no longer publishes it and has no newer packaged version (candidate {candidate})")
    if state == "stale":
        notes.append(f"NVIDIA {branch} {current}: the repository republished the locked packages, the lock is regenerated")
    return current, state == "stale"


def image_digest(image, tag):
    """The digest that `podman pull image:tag` resolves: the one of the tag's manifest (index)."""
    registry, _, name = image.partition("/")
    head, _ = http(
        f"https://{registry}/v2/{name}/manifests/{tag}",
        {"Accept": MANIFEST_ACCEPT},
        method="HEAD",
    )
    digest = head.get("Docker-Content-Digest", "")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        sys.exit(f"{image}:{tag}: digest missing from the registry response")
    return digest


def base_images():
    """{"image:tag": pinned digest} from the FROM lines of the Containerfiles. One ref pinned
    at two digests is an error: apply rewrites only the old digest it knows about."""
    found = {}
    for cf in CONTAINERFILES:
        for m in FROM_RE.finditer(cf.read_text()):
            found.setdefault(f"{m.group(1)}:{m.group(2)}", {}).setdefault(m.group(3), []).append(str(cf))
    conflicts = [
        ref + " at " + ", ".join(f"{digest} ({' '.join(files)})" for digest, files in digests.items())
        for ref, digests in found.items()
        if len(digests) > 1
    ]
    if conflicts:
        sys.exit(f"base images pinned at more than one digest: {'; '.join(conflicts)}")
    return {ref: next(iter(digests)) for ref, digests in found.items()}


# --- check / apply ---------------------------------------------------------------------


def compute():
    pins = read_pins()
    new, notes, locks = {}, [], {}
    pair = kernel_pair(pins, notes)
    if pair:
        version, nvr, tag, published = pair
        if nvr != pins["FEDORA_KERNEL_NVR"] or tag != pins["CACHYOS_RELEASE"]:
            new["FEDORA_KERNEL_NVR"], new["CACHYOS_RELEASE"] = nvr, tag
        # The linux-cachyos config in force at the release date: the one CachyOS shipped
        # that kernel with, not today's head, which may belong to the next series.
        config = head_commit(
            "CachyOS/linux-cachyos", "linux-cachyos/config", until=published
        )
        if config != pins["CACHYOS_CONFIG_COMMIT"]:
            new["CACHYOS_CONFIG_COMMIT"] = config
        patches = head_commit("CachyOS/kernel-patches", series(version))
        if patches != pins["CACHYOS_PATCHES_COMMIT"]:
            new["CACHYOS_PATCHES_COMMIT"] = patches
    # A series kernel.org no longer maintains gets no fixes: the bot fails instead of
    # leaving the kernel there, and blocks the other bumps until a pair moves it.
    pinned = series(new.get("FEDORA_KERNEL_NVR", pins["FEDORA_KERNEL_NVR"]))
    maintained = maintained_series()
    if pinned not in maintained:
        sys.exit(
            f"kernel: series {pinned} is end of life on kernel.org and no Fedora/CachyOS pair "
            f"moves off it (maintained: {', '.join(sorted(maintained, key=vtuple))}; "
            f"{'; '.join(notes) or 'no notes'})"
        )
    open_tag, open_commit = nvidia_open(pins["NVIDIA_OPEN_VERSION"])
    open_version, regenerate = nvidia_version("open", open_tag, pins["NVIDIA_OPEN_VERSION"], notes)
    if open_version != pins["NVIDIA_OPEN_VERSION"]:
        new["NVIDIA_OPEN_VERSION"], new["NVIDIA_OPEN_COMMIT"] = open_version, open_commit
    if regenerate:
        locks["open"] = open_version
    legacy_version, regenerate = nvidia_version("legacy", nvidia_legacy(pins["NVIDIA_LEGACY_VERSION"]), pins["NVIDIA_LEGACY_VERSION"], notes)
    if legacy_version != pins["NVIDIA_LEGACY_VERSION"]:
        new["NVIDIA_LEGACY_VERSION"] = legacy_version
    if regenerate:
        locks["legacy"] = legacy_version
    images = {}
    for ref, pinned in base_images().items():
        digest = image_digest(*ref.rsplit(":", 1))
        if digest != pinned:
            images[ref] = {"old": pinned, "new": digest}
    return {
        "changed": bool(new or images or locks),
        "pins": pins,
        "new": new,
        "images": images,
        "locks": locks,
        "notes": notes,
    }


def pins_table(pins):
    rows = "\n".join(f"| `{k}` | `{v}` |" for k, v in pins.items())
    return f"<!-- pins:begin (table written by bump.py apply) -->\n| pin | value |\n| --- | --- |\n{rows}\n<!-- pins:end -->"


def apply(result):
    text = PINS.read_text()
    for key, value in result["new"].items():
        text, n = re.subn(rf"^{key}=.*$", f"{key}={value}", text, flags=re.M)
        if n != 1:
            sys.exit(f"pins.env: {key} found {n} times")
    PINS.write_text(text, newline="\n")
    for cf in CONTAINERFILES:
        content = cf.read_text()
        for ref, change in result["images"].items():
            content = content.replace(
                f"FROM {ref}@{change['old']}", f"FROM {ref}@{change['new']}"
            )
        cf.write_text(content, newline="\n")
    for branch, version in result["locks"].items():
        lock_py("generate", branch, "--version", version)
    md, n = re.subn(
        r"<!-- pins:begin.*?<!-- pins:end -->",
        lambda _: pins_table(read_pins()),
        KERNEL_MD.read_text(),
        flags=re.S,
    )
    if n != 1:
        sys.exit("KERNEL.md: pins:begin/pins:end markers missing")
    KERNEL_MD.write_text(md, newline="\n")


def body(result):
    lines = ["## Pins", "", "| pin | before | after |", "| --- | --- | --- |"]
    lines += [
        f"| `{k}` | `{result['pins'][k]}` | `{v}` |" for k, v in result["new"].items()
    ]
    lines += [
        f"| `{ref}` | `{c['old'][7:19]}` | `{c['new'][7:19]}` |"
        for ref, c in result["images"].items()
    ]
    lines += [
        f"| `system/nvidia/locks/{branch}.lock` | | regenerated at `{version}` |"
        for branch, version in result["locks"].items()
    ]
    if result["notes"]:
        lines += ["", "## Notes", ""] + [f"- {n}" for n in result["notes"]]
    return "\n".join(lines) + "\n"


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in ("check", "apply"):
        sys.exit(__doc__)
    result = compute()
    if sys.argv[1] == "check":
        print(json.dumps(result, indent=2))
        return
    apply(result)
    sys.stdout.write(body(result))


if __name__ == "__main__":
    main()
