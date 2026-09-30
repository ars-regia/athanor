#!/usr/bin/python3
"""trust_state.py - fixed /run/athanor-update/state.json files for the shield (doc_bar.md
BR9: "a fixed state.json stands in for the trust state").

    python3 trust_state.py NAME [--now EPOCH] [--out PATH]

NAME is one of NAMES; "missing" removes the file. The file is written 0644 to a temporary
name and renamed, as the system side does (UT7). Times are set relative to --now, which
defaults to the instant scene.sh freezes the client at, so a capture never changes with the
day it runs. The dev VM passes --now "$(date +%s)" instead.
"""

import argparse
import json
import os
from pathlib import Path

FROZEN = 1789725600  # 2026-09-18 10:00 UTC, scene.sh's faketime
PATH = Path("/run/athanor-update/state.json")
DAY = 86400
NAMES = ("verified", "downloaded", "attention", "refused", "hostile", "missing")
IMAGE = "registry.example/owner/athanor-system:stable"
HOSTILE_VERSION = "43.\u202eevil\u0007" + "9" * 300
HOSTILE_HOST = "<b>registry</b>\u200f.example"


def deployment(digit, version, build_time):
    return {
        "image": IMAGE,
        "digest": "sha256:" + digit * 64,
        "version": version,
        "build_time": build_time,
    }


def state(name, now):
    booted = deployment("1", "43.20260915.2", now - 3 * DAY)
    base = {
        "schema": 1,
        "booted": booted,
        "downloaded": None,
        "previous": deployment("0", "43.20260901.1", now - 17 * DAY),
        "verified": {"value": True, "reason": "signature"},
        "update": "none",
        "policy": {
            "path": "/etc/containers/policy.json",
            "sha256": "ab" * 32,
            "shipped": True,
        },
        "secure_boot": {
            "secure_boot": 1,
            "setup_mode": 0,
            "mok_sb_state": None,
            "lockdown": "integrity",
        },
        "newest_booted_build_time": now - 3 * DAY,
        "last_successful_check": now - 30 * 3600,
        "last_error": "none",
        "last_error_host": None,
    }
    if name == "verified":
        return base
    if name == "downloaded":
        return {
            **base,
            "downloaded": deployment("2", "43.20260917.1", now - DAY),
            "update": "downloaded",
        }
    if name == "attention":
        return {
            **base,
            "previous": None,
            "verified": {"value": False, "reason": "media"},
            "last_successful_check": None,
            "policy": {**base["policy"], "shipped": False},
            "secure_boot": {
                "secure_boot": 0,
                "setup_mode": 0,
                "mok_sb_state": None,
                "lockdown": "none",
            },
        }
    if name == "refused":
        return {
            **base,
            "update": "refused",
            "last_error": "policy",
            "last_error_host": "registry.example",
        }
    if name == "hostile":
        return {
            **base,
            "booted": {**booted, "version": HOSTILE_VERSION},
            "last_error": "registry",
            "last_error_host": HOSTILE_HOST,
        }
    if name == "missing":
        return None
    raise ValueError(f"unknown trust state {name!r}; one of {', '.join(NAMES)}")


def write(name, now=FROZEN, path=PATH):
    content = state(name, now)
    if content is None:
        path.unlink(missing_ok=True)
        return
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}")
    temporary.write_text(json.dumps(content), encoding="utf-8")
    temporary.chmod(0o644)
    os.replace(temporary, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("name", choices=NAMES)
    parser.add_argument("--now", type=int, default=FROZEN)
    parser.add_argument("--out", type=Path, default=PATH)
    args = parser.parse_args()
    write(args.name, args.now, args.out)


if __name__ == "__main__":
    main()
