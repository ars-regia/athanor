#!/usr/bin/env python3
"""Writes release-request.json, what the build asks release.yml to release
(docs/architecture/doc_pipeline.md, PL54 and PL55). release.yml's candidate job checks every
field against the API's record of this run and the hashes against the files it downloads.

  release_request.py --image-digests FILE --kernel-digests DIR --out DIR

Environment: GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT, GITHUB_REPOSITORY, GITHUB_WORKFLOW_REF,
GITHUB_REF, GITHUB_SHA, BUILDER_HASH (the content hash of this run's builder image).
"""

import argparse
import hashlib
import json
import os
import pathlib
import sys

import schema


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--image-digests", required=True, type=pathlib.Path)
    p.add_argument("--kernel-digests", required=True, type=pathlib.Path)
    p.add_argument("--out", required=True, type=pathlib.Path)
    a = p.parse_args(argv)
    env = os.environ
    try:
        files = {
            "image-digests.txt": a.image_digests,
            "kernel-digests.txt": a.kernel_digests / "kernel-digests.txt",
            "kernel-unsigned.txt": a.kernel_digests / "kernel-unsigned.txt",
        }
        workflow = (
            env["GITHUB_WORKFLOW_REF"]
            .removeprefix(env["GITHUB_REPOSITORY"] + "/")
            .split("@", 1)[0]
        )
        request = {
            "schema_version": 1,
            "build_run_id": int(env["GITHUB_RUN_ID"]),
            "build_run_attempt": int(env["GITHUB_RUN_ATTEMPT"]),
            "workflow_path": workflow,
            "ref": env["GITHUB_REF"],
            "commit": env["GITHUB_SHA"],
            "builder_content_hash": env["BUILDER_HASH"],
            "sha256": {name: sha256(path) for name, path in files.items()},
        }
        schema.validate(request, schema.load("release-request"))
    except (KeyError, ValueError, OSError) as error:
        print(f"release_request.py: no request written: {error!r}", file=sys.stderr)
        return 1
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "release-request.json").write_text(json.dumps(request, indent=2) + "\n")
    print(json.dumps(request, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
