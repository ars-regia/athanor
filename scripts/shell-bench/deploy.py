"""Installs a build of the shell on the reference machine for the maintainer to judge.

The crates are built in the shell rig's build stage (forge/test/shell/rig.sh cargo), copied
to the machine, and /usr is made writable for this boot only with bootc usr-overlay: a
reboot returns the machine to its image. Refuses to run while soak.py or bench.py runs on
this host, so a measurement is never disturbed.
"""

import argparse
import os
import pathlib
import subprocess
import sys

import machine

ROOT = machine.HERE.parent.parent
RIG = ROOT / "forge" / "test" / "shell" / "rig.sh"
MEASURING = r"shell-bench/(soak|bench)\.py"


# Per crate: the data files and where the spec's %install puts them. The binary always goes
# to /usr/bin/<crate>; a later crate (athanor-control-center) is one more entry.
CRATES = {
    "athanor-bar": {"/usr/lib/systemd/user/athanor-bar.service": "athanor-bar.service"},
    "athanor-dock": {
        "/usr/lib/systemd/user/athanor-dock.service": "athanor-dock.service"
    },
    "athanor-shelld": {
        "/usr/lib/systemd/user/athanor-shelld.service": "athanor-shelld.service",
        "/usr/share/dbus-1/services/org.freedesktop.Notifications.service": "org.freedesktop.Notifications.service",
        "/usr/share/dbus-1/services/org.kde.StatusNotifierWatcher.service": "org.kde.StatusNotifierWatcher.service",
    },
}


def _data(crate, name):
    return ROOT / "forge" / "specs" / crate / f"{crate}-1.0.0" / "data" / name


def plan(crate, built_dir):
    """The (source, destination) pairs of CRATE; the binary comes from BUILT_DIR."""
    if crate not in CRATES:
        raise ValueError(f"unknown crate {crate!r}; known: {', '.join(sorted(CRATES))}")
    pairs = [
        (pathlib.Path(built_dir) / crate, pathlib.PurePosixPath("/usr/bin") / crate)
    ]
    for dst, name in CRATES[crate].items():
        pairs.append((_data(crate, name), pathlib.PurePosixPath(dst)))
    return pairs


def _pgrep():
    out = subprocess.run(
        ["pgrep", "-af", MEASURING], capture_output=True, text=True, check=False
    )
    return [line for line in out.stdout.splitlines() if "deploy.py" not in line]


def refuse_while_measuring(lister=_pgrep):
    running = lister()
    if running:
        sys.exit(
            "refusing to deploy while a measurement runs on this host:\n"
            + "\n".join(running)
        )


def build(crates):
    """Release build in the rig's build stage; returns the directory of the binaries."""
    subprocess.run(
        [
            str(RIG),
            "cargo",
            "build",
            "--release",
            "--locked",
            "-j",
            "4",
            *[arg for crate in crates for arg in ("-p", crate)],
        ],
        check=True,
    )
    out = pathlib.Path(
        os.environ.get("ATHANOR_RIG_OUT", ROOT / ".scratch" / "shell-rig")
    )
    return out / "target" / "release"


def commit():
    def git(*args):
        return subprocess.run(
            ["git", "-C", str(ROOT), *args], capture_output=True, text=True, check=True
        ).stdout.strip()

    dirty = git("status", "--porcelain", "--untracked-files=no")
    return git("rev-parse", "--short", "HEAD") + ("-dirty" if dirty else "")


def install(m, src, dst):
    mode = "0755" if dst.parent.name == "bin" else "0644"
    m.run(f"sudo -n install -D -m {mode} /dev/stdin {dst}", stdin=src.read_bytes())


def deploy(m, crates, pairs):
    # /usr is read-only on an image-based system until the overlay is mounted.
    try:
        m.run("sudo -n test -w /usr/bin")
    except RuntimeError:
        m.run("sudo -n bootc usr-overlay")
    for src, dst in pairs:
        install(m, src, dst)
    m.systemctl("daemon-reload")
    for crate in crates:
        m.systemctl("restart", crate + ".service")
    print(f"installed {commit()} on {m.host}: {', '.join(crates)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--host", required=True)
    parser.add_argument(
        "--built",
        type=pathlib.Path,
        help="directory of already built binaries; skips the build",
    )
    parser.add_argument("crates", nargs="+", choices=sorted(CRATES))
    args = parser.parse_args()

    refuse_while_measuring()
    built = args.built or build(args.crates)
    pairs = [pair for crate in args.crates for pair in plan(crate, built)]
    for src, _ in pairs:
        if not src.is_file():
            sys.exit(f"missing {src}")
    deploy(machine.Machine(args.host), args.crates, pairs)


if __name__ == "__main__":
    main()
