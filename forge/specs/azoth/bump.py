#!/usr/bin/env python3
"""The bump bot of the Athanor kernel (docs/architecture/doc_kernel_build.md, section 8).

    bump.py check --group kernel|system   prints to stdout a JSON with the current pins, the
                                          new ones and the notes of that group
    bump.py apply --group kernel|system   rewrites the files of that group; prints the PR body
                                          (Markdown) to stdout
    bump.py cosmic-comp check|apply OUT_DIR   the cosmic-comp tracking alone (below), its own
                                          PR; apply writes OUT_DIR/title and OUT_DIR/body.md,
                                          and only when something moved
    bump.py verify                        verifies only the NVIDIA locks at their pins against
                                          the repositories and exits non-zero naming each stale
                                          lock (a version gone is a note); the workflow runs it while a
                                          system bump PR is open, when check and apply do not

Two groups, one pull request each, because they are verified differently
(docs/architecture/doc_build_ordering.md, O7 and O8):

  kernel   pins.env, the pins table of KERNEL.md, the FROM lines of the kernel's Containerfiles
           (forge/specs/azoth/{builder,boot,nvidia}) and the NVIDIA lock of a branch whose pin
           moves. Kernel Build proves it before azoth:<nvr> exists; System Image Check skips
           the images of a pure pin bump.
  system   the FROM lines of system/Containerfile and the NVIDIA locks the repository
           republished at an unchanged version. System Image Check builds the three images
           against the published kernel and reports the package difference a person reviews.

Kernel pair (spec, section 2): for the X.Y series that both Fedora (stable, F43 then F44)
and CachyOS (GitHub releases of CachyOS/linux) ship, the highest patch level X.Y.Z present
on both sides. KERNEL_CHANNEL=stable takes the newest common series, lts the longterm
one. Without a pair the kernel stays where it is and a note says so. With the pair, the
head commit of CachyOS/kernel-patches for the series and the commit of
linux-cachyos/config in force at the date of the CachyOS release move as well.
cosmic-comp (forge/specs/cosmic-comp): the newest stable F43 build on Bodhi against the spec's
Version and fedora_release; a difference rewrites the spec and the archive pin. It runs on its own
(`cosmic-comp-bump.yml`, its own branch and PR, never auto-merged), apart from the kernel bump.
Outside the kernel: the NVIDIA versions (open from the GitHub tags, legacy from RPM Fusion)
within the pinned branch, and the digest of the base image of each group's Containerfiles.
The system group verifies the NVIDIA locks in system/nvidia/locks against the repository
metadata and regenerates a lock whose packages the repository republished; the kernel group
regenerates the lock of a pin it moves. A ref pinned in both groups (fedora:43) may sit at
two digests between the merges of the two pull requests: the groups build separately. The kernel hash manifests are not here: `build.sh --stage manifest` and `nvidia.sh manifest` write
them. Standard library only: it runs on the GitHub runner without installing anything.
"""

import hashlib
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
# The Containerfiles of each group (docs/architecture/doc_system_image.md, S1): every FROM
# pinned by digest moves with the bump of its group.
GROUP_CONTAINERFILES = {
    "kernel": [HERE / d / "Containerfile" for d in ("builder", "boot", "nvidia")],
    "system": [HERE.parents[2] / "system" / "Containerfile"],
}
NVIDIA_LOCK = HERE.parents[2] / "system" / "nvidia" / "lock.py"
TOOLKIT_LOCK = NVIDIA_LOCK.parent / "locks" / "container-toolkit.lock"
LOCK_NOT_PUBLISHED = 3  # lock.py's exit code for a version the repository does not publish
LOCK_STALE = 4  # lock.py verify: the repository publishes the version with other files or checksums
KERNEL_MD = HERE / "KERNEL.md"
COSMIC_COMP_DIR = HERE.parents[0] / "cosmic-comp"
COSMIC_COMP_SPEC = COSMIC_COMP_DIR / "cosmic-comp.spec"
COSMIC_COMP_RE = re.compile(r"^cosmic-comp-(\d+\.\d+\.\d+)-(\d+)\.fc43$")
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


# --- cosmic-comp --------------------------------------------------------------------------


def cosmic_comp_nvrs():
    """The Bodhi answer for the stable F43 builds of cosmic-comp."""
    query = {"packages": "cosmic-comp", "releases": "F43", "status": "stable", "rows_per_page": 100}
    return json.loads(http(f"{BODHI}?{urllib.parse.urlencode(query)}")[1])


def cosmic_comp_newest(data):
    """The newest `version-release.fc43` among the builds of a Bodhi answer."""
    found = []
    for update in data["updates"]:
        for build in update["builds"]:
            m = COSMIC_COMP_RE.match(build["nvr"])
            if m:
                found.append((vtuple(m.group(1)), int(m.group(2)), f"{m.group(1)}-{m.group(2)}.fc43"))
    if not found:
        sys.exit("cosmic-comp: Bodhi lists no stable F43 build")
    return max(found)[2]


def cosmic_comp_pin(spec):
    """`version-release.fc43` of the spec: Fedora's build it is made from."""
    version = re.search(r"^Version:\s*(\S+)$", spec, re.M)
    release = re.search(r"^%global fedora_release (\S+)$", spec, re.M)
    if not version or not release:
        sys.exit("cosmic-comp.spec: Version or fedora_release not found")
    return f"{version.group(1)}-{release.group(1)}"


def cosmic_comp_spec(spec, nvr, commit):
    """The spec rewritten for Fedora's build NVR: the Athanor suffix starts again at 1."""
    version, release = COSMIC_COMP_RE.match(f"cosmic-comp-{nvr}").groups()
    for pattern, value in (
        (r"^(Version:\s*).*$", rf"\g<1>{version}"),
        (r"^(%global fedora_release ).*$", rf"\g<1>{release}.fc43"),
        (r"^(Release:\s*%\{fedora_release\}\.athanor).*$", r"\g<1>1"),
        (r"^(%global commit ).*$", rf"\g<1>{commit}"),
    ):
        spec, n = re.subn(pattern, value, spec, flags=re.M)
        if n != 1:
            sys.exit(f"cosmic-comp.spec: {pattern} found {n} times")
    return spec


def cosmic_comp_move():
    """{"old", "new"} when Bodhi's newest stable F43 build differs from the spec's, else None."""
    old = cosmic_comp_pin(COSMIC_COMP_SPEC.read_text())
    new = cosmic_comp_newest(cosmic_comp_nvrs())
    return {"old": old, "new": new} if new != old else None


def apply_cosmic_comp(move):
    """Rewrites the spec and the pin of its archive. The patch is not touched: the DAG build
    (%autosetup -p1) fails when it no longer applies, and a reviewer decides if upstream has it."""
    version = move["new"].split("-")[0]
    tag = github_commit(f"epoch-{version}")
    COSMIC_COMP_SPEC.write_text(cosmic_comp_spec(COSMIC_COMP_SPEC.read_text(), move["new"], tag["sha"]), newline="\n")
    archive = http(f"https://github.com/pop-os/cosmic-comp/archive/epoch-{version}/cosmic-comp-{version}.tar.gz")[1]
    (COSMIC_COMP_DIR / "SOURCES" / "sources.sha256").write_text(f"{hashlib.sha256(archive).hexdigest()}  cosmic-comp-{version}.tar.gz\n", newline="\n")


def github_commit(ref):
    return gh(f"https://api.github.com/repos/pop-os/cosmic-comp/commits/{ref}")[1]


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


def toolkit_version(lock=TOOLKIT_LOCK):
    """The locked version of NVIDIA's container toolkit: not a kernel input, so its lock is its
    only pin (doc_system_image.md, S7)."""
    for line in lock.read_text().splitlines():
        if line.startswith("# version "):
            return line.split(" ", 2)[2]
    sys.exit(f"{lock}: no version line")


def nvidia_toolkit(current):
    """The highest version of the locked major that NVIDIA's container toolkit repository publishes."""
    return lock_py("latest", "container-toolkit", "--major", current.split(".")[0]).stdout.strip()


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


def nvidia_pin(branch, candidate, current, notes, check=lock_check):
    """The version the bot pins for one NVIDIA branch (doc_system_image.md, S7): a
    newer candidate the repository packages, else the current pin. A moved pin regenerates
    its lock in the same pull request."""
    if vtuple(candidate) > vtuple(current):
        return packaged_or_current(branch, candidate, current, notes, check)
    return current


def nvidia_relock(branch, current, notes, verify=lock_verify):
    """True when the system group regenerates the lock of one NVIDIA branch at its current pin.

    The lock is verified against the repository metadata on every run: a new release or new
    checksums of the same version regenerate it. A version the repository dropped is only a
    note: image builds take the locked packages from the mirror, the mirror step that follows
    in the same job fails when the mirror lacks them, and moving a driver pin is the kernel
    group's job, so it never holds back the system base."""
    state = verify(branch, current)
    if state == "gone":
        notes.append(gone_note(branch, current))
    if state == "stale":
        notes.append(f"NVIDIA {branch} {current}: the repository republished the locked packages, the lock is regenerated")
    return state == "stale"


def gone_note(branch, version):
    return f"NVIDIA {branch} {version}: the repository no longer publishes it; image builds take the locked packages from the mirror (mirror.sh fails when it lacks them), and the bot moves the pin once a newer version is packaged"


def lock_problems(pins, toolkit, notes, verify=lock_verify):
    """One line per NVIDIA lock that must be regenerated at the pinned version; a version the
    repository dropped goes to notes (nvidia_relock). `toolkit` is the container toolkit's
    version, which has no pin outside its lock."""
    problems = []
    for branch, version in (("open", pins["NVIDIA_OPEN_VERSION"]), ("legacy", pins["NVIDIA_LEGACY_VERSION"]), ("container-toolkit", toolkit)):
        state = verify(branch, version)
        if state == "stale":
            problems.append(f"NVIDIA {branch} {version}: the repository republished the locked packages, the lock must be regenerated")
        elif state == "gone":
            notes.append(gone_note(branch, version))
    return problems


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


def base_images(containerfiles):
    """{"image:tag": pinned digest} from the FROM lines of one group's Containerfiles. One ref
    pinned at two digests within a group is an error: apply rewrites only the old digest it
    knows about. Across groups it is not: each group moves its own copy of the ref."""
    found = {}
    for cf in containerfiles:
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


def compute(group):
    pins = read_pins()
    new, notes, locks = {}, [], {}
    if group == "kernel":
        kernel_pins(pins, new, notes, locks)
    else:
        for branch in ("open", "legacy"):
            version = pins[f"NVIDIA_{branch.upper()}_VERSION"]
            if nvidia_relock(branch, version, notes):
                locks[branch] = version
        # The container toolkit is not a kernel input: its version moves here, with the system
        # base. A moved version changes the lock's `# version` line, which bot_merge.py refuses,
        # so that pull request waits for a person.
        toolkit = toolkit_version()
        toolkit_next = nvidia_pin("container-toolkit", nvidia_toolkit(toolkit), toolkit, notes)
        if toolkit_next != toolkit or nvidia_relock("container-toolkit", toolkit, notes):
            locks["container-toolkit"] = toolkit_next
    images = {}
    if new and all(key.startswith("NVIDIA_") for key in new):
        # check-plan accepts NVIDIA pins only alone (modules-missing, doc_build_ordering.md
        # O7): a new base of the kernel's Containerfiles waits for the next run.
        notes.append("NVIDIA pins move in a pull request of their own: the base images of the kernel's Containerfiles wait for the next run")
    else:
        for ref, pinned in base_images(GROUP_CONTAINERFILES[group]).items():
            digest = image_digest(*ref.rsplit(":", 1))
            if digest != pinned:
                images[ref] = {"old": pinned, "new": digest}
    return {
        "group": group,
        "changed": bool(new or images or locks),
        "pins": pins,
        "new": new,
        "images": images,
        "locks": locks,
        "notes": notes,
    }


def kernel_pins(pins, new, notes, locks):
    """The kernel group's pins: the Fedora/CachyOS pair and the NVIDIA versions, into new;
    the lock of each NVIDIA branch whose pin moves, into locks."""
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
    # leaving the kernel there, and blocks the kernel group until a pair moves it.
    pinned = series(new.get("FEDORA_KERNEL_NVR", pins["FEDORA_KERNEL_NVR"]))
    maintained = maintained_series()
    if pinned not in maintained:
        sys.exit(
            f"kernel: series {pinned} is end of life on kernel.org and no Fedora/CachyOS pair "
            f"moves off it (maintained: {', '.join(sorted(maintained, key=vtuple))}; "
            f"{'; '.join(notes) or 'no notes'})"
        )
    open_tag, open_commit = nvidia_open(pins["NVIDIA_OPEN_VERSION"])
    open_version = nvidia_pin("open", open_tag, pins["NVIDIA_OPEN_VERSION"], notes)
    if open_version != pins["NVIDIA_OPEN_VERSION"]:
        new["NVIDIA_OPEN_VERSION"], new["NVIDIA_OPEN_COMMIT"] = open_version, open_commit
        locks["open"] = open_version
    legacy_version = nvidia_pin("legacy", nvidia_legacy(pins["NVIDIA_LEGACY_VERSION"]), pins["NVIDIA_LEGACY_VERSION"], notes)
    if legacy_version != pins["NVIDIA_LEGACY_VERSION"]:
        new["NVIDIA_LEGACY_VERSION"] = legacy_version
        locks["legacy"] = legacy_version


def pins_table(pins):
    rows = "\n".join(f"| `{k}` | `{v}` |" for k, v in pins.items())
    return f"<!-- pins:begin (table written by bump.py apply) -->\n| pin | value |\n| --- | --- |\n{rows}\n<!-- pins:end -->"


def apply(result):
    """Rewrites the files of result's group, and only those."""
    group = result["group"]
    if result["new"]:
        text = PINS.read_text()
        for key, value in result["new"].items():
            text, n = re.subn(rf"^{key}=.*$", f"{key}={value}", text, flags=re.M)
            if n != 1:
                sys.exit(f"pins.env: {key} found {n} times")
        PINS.write_text(text, newline="\n")
    for cf in GROUP_CONTAINERFILES[group]:
        content = cf.read_text()
        for ref, change in result["images"].items():
            content = content.replace(
                f"FROM {ref}@{change['old']}", f"FROM {ref}@{change['new']}"
            )
        cf.write_text(content, newline="\n")
    for branch, version in result["locks"].items():
        lock_py("generate", branch, "--version", version)
    if group != "kernel":
        return
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


def cosmic_comp_body(move):
    return (
        "## Pins\n\n| package | before | after |\n| --- | --- | --- |\n"
        f"| `cosmic-comp` | `{move['old']}` | `{move['new']}` |\n\n"
        "Fedora's stable F43 build moved. This PR rewrites `forge/specs/cosmic-comp/cosmic-comp.spec` "
        "and `SOURCES/sources.sha256`; the patches are untouched. Please check whether upstream "
        "(pop-os/cosmic-comp) has merged the layer-surface focus fix (`Patch0`) or the `GIT_HASH` "
        "change to build.rs (`Patch1`): if so, drop that patch and its file. If a patch no longer "
        "applies, the DAG build fails (`%autosetup -p1`) and it needs a refresh.\n\n"
        "The spec builds upstream's release at Fedora's version, and its `License:` and `Requires:` "
        "were copied from Fedora's spec, which the bump does not rewrite: diff Fedora's "
        "`cosmic-comp.spec` between the two builds (src.fedoraproject.org/rpms/cosmic-comp) "
        "and carry any License or Requires change. Never auto-merged.\n"
    )


def cosmic_comp_title(move):
    return f"chore(cosmic-comp): bump to {move['new']}"


def cosmic_comp_main(action, out_dir=None):
    """`bump.py cosmic-comp check|apply`: independent of the kernel bump, and the only caller of
    the cosmic-comp lookup, so a failure here never reaches the kernel path and the reverse."""
    move = cosmic_comp_move()
    if action == "check":
        print(json.dumps({"changed": bool(move), "move": move}, indent=2))
        return
    if move:
        apply_cosmic_comp(move)
        out = Path(out_dir)
        out.mkdir(parents=True, exist_ok=True)
        (out / "title").write_text(cosmic_comp_title(move) + "\n", newline="\n")
        (out / "body.md").write_text(cosmic_comp_body(move), newline="\n")


def main():
    args = sys.argv[1:]
    if len(args) > 1 and args[0] == "cosmic-comp":
        if args[1:] == ["check"] or (len(args) == 3 and args[1] == "apply"):
            return cosmic_comp_main(*args[1:3])
        sys.exit(__doc__)
    if args == ["verify"]:
        notes = []
        problems = lock_problems(read_pins(), toolkit_version(), notes)
        print("\n".join(notes))
        if problems:
            sys.exit("\n".join(problems))
        print("NVIDIA locks: every lock matches its repository")
        return
    if len(args) != 3 or args[0] not in ("check", "apply") or args[1] != "--group" or args[2] not in GROUP_CONTAINERFILES:
        sys.exit(__doc__)
    result = compute(args[2])
    if args[0] == "check":
        print(json.dumps(result, indent=2))
        return
    apply(result)
    sys.stdout.write(body(result))


if __name__ == "__main__":
    main()
