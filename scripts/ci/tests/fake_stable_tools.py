#!/usr/bin/env python3
"""Offline stand-in for skopeo, gh and cosign in test_stable_promotions.py, linked under the
tool's name on PATH. It answers from the JSON file named by FAKE_STABLE:
  tags      {"registry/repo:tag": digest}, a tag that is absent answers "manifest unknown"
  list      {"registry/repo": [tags]}
  bundles   {"registry/repo:tag": path of the layer tar `skopeo copy` writes}
  errors    ["text", ...]  any call whose arguments contain the text fails
  api       {"gh api path": JSON}, printed through `jq -r` with the call's --jq filter
  cosign_ok false: `cosign verify-blob` rejects the signature"""

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys


def main(name, args):
    fx = json.loads(pathlib.Path(os.environ["FAKE_STABLE"]).read_text())
    line = " ".join(args)
    for text in fx.get("errors", []):
        if text in line:
            print(f"fake {name}: transport failure for {text}", file=sys.stderr)
            return 1
    if name == "cosign":
        return 0 if fx.get("cosign_ok", True) else 1
    if name == "gh":
        jq = args[args.index("--jq") + 1]
        path = next(a for a in args[1:] if not a.startswith("-") and a != jq)
        body = fx["api"].get(path)
        if body is None:
            print(f"fake gh: HTTP 404 {path}", file=sys.stderr)
            return 1
        return subprocess.run(
            ["jq", "-r", jq], input=json.dumps(body), text=True
        ).returncode
    ref = args[-1].removeprefix("docker://")
    if args[0] == "list-tags":
        print(json.dumps({"Repository": ref, "Tags": fx["list"][ref]}))
    elif args[0] == "inspect":
        if ref not in fx["tags"]:
            print(
                f'level=fatal msg="reading manifest {ref}: manifest unknown"',
                file=sys.stderr,
            )
            return 2
        print(fx["tags"][ref])
    elif args[0] == "copy":
        layer = pathlib.Path(fx["bundles"][args[-2].removeprefix("docker://")])
        out = pathlib.Path(ref.removeprefix("dir:"))
        out.mkdir()
        digest = hashlib.sha256(layer.read_bytes()).hexdigest()
        shutil.copy(layer, out / digest)
        (out / "manifest.json").write_text(
            json.dumps({"layers": [{"digest": f"sha256:{digest}"}]})
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(pathlib.Path(sys.argv[0]).name, sys.argv[1:]))
