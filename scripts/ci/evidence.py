#!/usr/bin/env python3
"""The evidence files of UD4 (docs/architecture/doc_update_delivery.md): one per gate and
image, written by the gate, read by system/promote.sh before :stable moves.

  evidence.py write --gate G --image NAME --digest D --run-id N (--verdict V | --verdict-json F) --run-url U --out DIR
  evidence.py check --dir DIR --gate G --image NAME --digest D --run-id N
"""

import argparse
import datetime
import json
import pathlib
import re
import sys

GATES = ("iso-acceptance", "signature")
FIELDS = {"gate", "image", "digest", "run_id", "verdict", "workflow_run_url", "finished_at"}
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
IMAGE = re.compile(r"^athanor-[a-z0-9-]+$")


def args(argv):
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("write", "check"):
        s = sub.add_parser(name)
        s.add_argument("--gate", required=True, choices=GATES)
        s.add_argument("--image", required=True)
        s.add_argument("--digest", required=True)
        s.add_argument("--run-id", required=True)
    w = sub.choices["write"]
    v = w.add_mutually_exclusive_group(required=True)
    v.add_argument("--verdict", choices=("pass", "fail"))
    v.add_argument("--verdict-json", type=pathlib.Path)
    w.add_argument("--run-url", required=True)
    w.add_argument("--out", required=True, type=pathlib.Path)
    sub.choices["check"].add_argument("--dir", required=True, type=pathlib.Path)
    a = p.parse_args(argv)
    if not (IMAGE.match(a.image) and DIGEST.match(a.digest) and a.run_id.isdigit()):
        p.error("--image, --digest or --run-id is malformed")
    return a


def verdict_of(path):
    try:
        return "pass" if json.loads(path.read_text()).get("pass") is True else "fail"
    except (OSError, ValueError, AttributeError):
        return "fail"


def write(a):
    a.out.mkdir(parents=True, exist_ok=True)
    data = {
        "gate": a.gate, "image": a.image, "digest": a.digest, "run_id": int(a.run_id),
        "verdict": a.verdict or verdict_of(a.verdict_json), "workflow_run_url": a.run_url,
        "finished_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    (a.out / f"{a.gate}.{a.image}.json").write_text(json.dumps(data, indent=2) + "\n")
    return 0


def check(a):
    path = a.dir / f"{a.gate}.{a.image}.json"
    if not path.is_file():
        return refuse(f"no {a.gate} evidence for {a.image} ({path})")
    try:
        data = json.loads(path.read_text())
        finished = datetime.datetime.strptime(data["finished_at"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc)
    except (ValueError, KeyError, TypeError):
        return refuse(f"{path} is malformed")
    if set(data) != FIELDS:
        return refuse(f"{path} is malformed: fields {sorted(data)}")
    for field, want in (("gate", a.gate), ("image", a.image), ("digest", a.digest), ("run_id", int(a.run_id))):
        if data[field] != want:
            return refuse(f"{path}: {field.replace('_', ' ')} is {data[field]}, not {want}")
    if data["verdict"] != "pass":
        return refuse(f"{path}: verdict is {data['verdict']}")
    print(int(finished.timestamp()))
    return 0


def refuse(message):
    print(f"evidence.py: {message}", file=sys.stderr)
    return 1


def main(argv=None):
    a = args(sys.argv[1:] if argv is None else argv)
    return write(a) if a.cmd == "write" else check(a)


if __name__ == "__main__":
    raise SystemExit(main())
