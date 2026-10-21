#!/usr/bin/env python3
"""The release attestation of PB4b (docs/architecture/doc_pipeline.md, ADR-0127): one unsigned
in-toto statement per image, written by `candidate` from the class records of PL55, attested by
`sign-images` with `cosign attest` and the image key, and verified by `verify-images` with the
public keys of the checkout.

  release_attestation.py write --candidate FILE --out DIR
      DIR/<repository basename>/<hex>.json, one statement per class record.
  release_attestation.py predicates --digests FILE --dir DIR --out DIR
      For system/sign-images.sh: requires one statement in DIR per line of the digests file,
      naming that image and nothing else, and writes the predicate of each to DIR/<hex>.json.
  release_attestation.py verify --statement FILE --keys-dir DIR REPOSITORY@DIGEST
      For system/verify-images.sh: at least one attestation verifies with a key of DIR, and
      every one that verifies says what the statement says.

Exit 1 with the reason on stderr when a statement or an attestation is refused.
"""

import argparse
import base64
import json
import pathlib
import re
import subprocess
import sys

import schema

PREDICATE_TYPE = "https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b"
STATEMENT_TYPE = "https://in-toto.io/Statement/v1"
PAYLOAD_TYPE = "application/vnd.in-toto+json"
PREDICATE_KEYS = {
    "class",
    "advisories",
    "class_reason",
    "build_time",
    "revision",
    "build_run_id",
    "updateinfo_timestamp",
}
DIGEST = re.compile(r"^sha256:([0-9a-f]{64})$")
TIMEOUT = 300  # seconds for one cosign verification


class Refused(Exception):
    pass


def statement(record, build_time, revision, build_run_id):
    """The statement of one class record (ADR-0127 points 1 and 3)."""
    return {
        "_type": STATEMENT_TYPE,
        "subject": [
            {
                "name": record["repository"],
                "digest": {
                    "sha256": record["candidate_digest"].removeprefix("sha256:")
                },
            }
        ],
        "predicateType": PREDICATE_TYPE,
        "predicate": {
            "class": record["class"],
            "advisories": record["advisories"],
            "class_reason": record["class_reason"],
            "build_time": build_time,
            "revision": revision,
            "build_run_id": build_run_id,
            "updateinfo_timestamp": record["updateinfo_timestamp"],
        },
    }


def path_of(out, repository, digest):
    return (
        out / repository.rsplit("/", 1)[-1] / f"{digest.removeprefix('sha256:')}.json"
    )


def write_statements(candidate_json, out):
    candidate = json.loads(candidate_json.read_text())
    schema.validate(candidate, schema.load("release-candidate"))
    request = candidate["request"]
    written = []
    for record in candidate["classes"]:
        path = path_of(out, record["repository"], record["candidate_digest"])
        path.parent.mkdir(parents=True, exist_ok=True)
        s = statement(
            record,
            candidate["build_time"],
            request["commit"],
            str(request["build_run_id"]),
        )
        path.write_text(json.dumps(s, indent=2) + "\n")
        written.append(path)
    return written


def load_statement(path, repository, digest):
    """The statement at path, refused unless it attests exactly repository@digest."""
    try:
        s = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise Refused(
            f"{repository}@{digest}: no readable release statement at {path}: {error}"
        ) from None
    subject = [
        {"name": repository, "digest": {"sha256": digest.removeprefix("sha256:")}}
    ]
    if not (
        isinstance(s, dict)
        and s.get("_type") == STATEMENT_TYPE
        and s.get("predicateType") == PREDICATE_TYPE
        and s.get("subject") == subject
        and isinstance(s.get("predicate"), dict)
        and set(s["predicate"]) == PREDICATE_KEYS
    ):
        raise Refused(f"{path}: not the release statement of {repository}@{digest}")
    return s


def predicates(digests, statements, out):
    expected = set()
    out.mkdir(parents=True, exist_ok=True)
    for line in digests.read_text().splitlines():
        repository, _tag, digest = line.split()
        if not DIGEST.match(digest):
            raise Refused(f"{repository}: '{digest}' is not a digest")
        path = path_of(statements, repository, digest)
        expected.add(path)
        s = load_statement(path, repository, digest)
        (out / f"{digest.removeprefix('sha256:')}.json").write_text(
            json.dumps(s["predicate"])
        )
    extra = sorted(
        str(p) for p in statements.rglob("*") if p.is_file() and p not in expected
    )
    if extra:
        raise Refused(
            f"release statements for no image of {digests}: {', '.join(extra)}"
        )


def verify(ref, statement_path, keys_dir):
    repository, _, digest = ref.partition("@")
    if not DIGEST.match(digest):
        raise Refused(f"'{ref}' is not a reference by digest")
    expected = load_statement(statement_path, repository, digest)
    keys = sorted(keys_dir.glob("*.pub"))
    if not keys:
        raise Refused(f"no *.pub under {keys_dir}")
    envelopes = []
    for key in keys:
        # A key that signed nothing here fails with its reason on stderr; another may verify.
        try:
            r = subprocess.run(
                ["cosign", "verify-attestation", "--key", str(key), "--type", PREDICATE_TYPE,
                 "--insecure-ignore-tlog=true", "--new-bundle-format=false", ref],
                stdout=subprocess.PIPE, text=True, timeout=TIMEOUT,
            )  # fmt: skip
        except subprocess.TimeoutExpired:
            raise Refused(
                f"{ref}: cosign verify-attestation with {key} ran past {TIMEOUT} s"
            ) from None
        if r.returncode == 0:
            envelopes += [
                json.loads(line) for line in r.stdout.splitlines() if line.strip()
            ]
    if not envelopes:
        raise Refused(f"{ref}: no attestation verifies with a key of {keys_dir}")
    for envelope in envelopes:
        try:
            payload = json.loads(base64.b64decode(envelope["payload"], validate=True))
            same = envelope["payloadType"] == PAYLOAD_TYPE and all(
                payload[k] == expected[k]
                for k in ("predicateType", "subject", "predicate")
            )
        except (KeyError, TypeError, ValueError):
            same = False
        if not same:
            raise Refused(
                f"{ref}: an attestation differs from the release statement {statement_path}"
            )


def main(argv):
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    w = sub.add_parser("write")
    w.add_argument("--candidate", required=True, type=pathlib.Path)
    w.add_argument("--out", required=True, type=pathlib.Path)
    d = sub.add_parser("predicates")
    d.add_argument("--digests", required=True, type=pathlib.Path)
    d.add_argument("--dir", required=True, type=pathlib.Path)
    d.add_argument("--out", required=True, type=pathlib.Path)
    v = sub.add_parser("verify")
    v.add_argument("--statement", required=True, type=pathlib.Path)
    v.add_argument("--keys-dir", required=True, type=pathlib.Path)
    v.add_argument("ref")
    a = p.parse_args(argv)
    try:
        if a.cmd == "write":
            for path in write_statements(a.candidate, a.out):
                print(f"release statement: {path}")
        elif a.cmd == "predicates":
            predicates(a.digests, a.dir, a.out)
        else:
            verify(a.ref, a.statement, a.keys_dir)
            print(f"attestation verified: {a.ref}")
    except (Refused, ValueError) as error:
        print(f"release_attestation.py {a.cmd}: refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
