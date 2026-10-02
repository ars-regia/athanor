#!/usr/bin/env python3
"""Offline stand-in for skopeo, cosign and gh in the tests under system/tests.

Linked under the tool's name on PATH, it answers from the JSON fixture named by FAKE_REGISTRY
and appends every call to FAKE_LOG. Its error messages are the ones cosign v3.1.3 and skopeo
print against ghcr.io (observed 2026-09-17), because system/kernel-artifacts.sh classifies them.

Fixture keys:
  tags          {"registry/repo:tag" or "registry/repo@digest": "sha256:..."}
  errors        ["ref", ...]  transport failure for that reference, in every tool
  signature_transient_errors
                ["ref", ...]  cosign verify fails with a transient error (a Rekor lookup
                timeout) whose message still starts with "no matching signatures:", the same
                prefix a genuine identity mismatch uses; kernel-artifacts.sh must not fold
                this into "unsigned"
  attestation_errors
                ["ref", ...]  transport failure for that reference, but only in
                `cosign verify-attestation`: unlike `errors`, `skopeo inspect` and
                `cosign verify` on the same ref still succeed, so a test can pin an outage to
                the attestation check alone
  signatures    {"registry/repo@digest": "signing workflow identity"}
  attestations  {"registry/repo@digest": [{"identity": "...", "predicate": {...}}]}
  configs       {"registry/repo@digest": {label: value}}
  raw           {"registry/repo:tag": manifest JSON}
  sigstore_keys {"registry/repo@digest": path of the public key whose private half signed it}
                `skopeo copy --policy` accepts the image only when that key is among the
                keyPaths of the policy scope for the repository, and only when --registries.d
                is passed (without it containers/image never looks for sigstore attachments)
  packages      {"package": [package versions as the GitHub API returns them]}
  user_packages ["package", ...]  the container packages of the owner
  runs          [{"databaseId": 1, "headBranch": "iso-v0"}]
"""

import base64
import json
import os
import re
import sys


def fail(message, code=1):
    print(message, file=sys.stderr)
    return code


def verify(args, fx):
    """`skopeo copy --policy`: the source must carry a sigstore signature by a key of its scope."""
    ref = args[-2].removeprefix("docker://")
    with open(args[args.index("--policy") + 1]) as f:
        policy = json.load(f)
    rules = policy["transports"]["docker"].get(ref.split("@")[0], policy["default"])
    keys = []
    for rule in rules:
        for path in rule.get("keyPaths", []):
            with open(path) as f:
                keys.append(f.read())
    signer = fx.get("sigstore_keys", {}).get(ref)
    if signer is None or "--registries.d" not in args:
        return fail("FATA[0000] Source image rejected: A signature was required, but no signature exists")
    with open(signer) as f:
        if f.read() not in keys:
            return fail("FATA[0000] Source image rejected: cryptographic signature verification failed: "
                        "invalid signature when validating ASN.1 encoded signature")
    if args[-1].startswith("dir:"):
        os.mkdir(args[-1].removeprefix("dir:"))
    return 0


def skopeo(args, fx):
    if "copy" in args:
        if "--policy" in args:
            return verify(args, fx)
        # Recorded in FAKE_LOG by main(); the fixture is read-only, so nothing moves.
        return fail("fake skopeo: copy failed", 1) if args[-1].removeprefix("docker://") in fx.get("errors", []) else 0
    ref = args[-1].removeprefix("docker://")
    if ref in fx.get("errors", []):
        return fail(f'time="2026-09-17T00:00:00Z" level=fatal msg="Error parsing image name \\"docker://{ref}\\": pinging container registry: dial tcp: i/o timeout"')
    unknown = f'time="2026-09-17T00:00:00Z" level=fatal msg="Error parsing image name \\"docker://{ref}\\": reading manifest in {ref}: manifest unknown"'
    if "--config" in args:
        if ref not in fx.get("configs", {}):
            return fail(unknown, 2)
        print(json.dumps({"config": {"Labels": fx["configs"][ref]}}))
        return 0
    if "--raw" in args:
        if ref not in fx.get("raw", {}):
            return fail(unknown, 2)
        print(json.dumps(fx["raw"][ref]))
        return 0
    digest = fx.get("tags", {}).get(ref)
    if digest is None:
        return fail(unknown, 2)
    print(digest)
    return 0


def cosign(args, fx):
    ref = args[-1]
    if ref in fx.get("errors", []):
        return fail("Error: getting trusted root: GET https://tuf-repo-cdn.sigstore.dev/timestamp.json: 502 Bad Gateway")
    if ref in fx.get("signature_transient_errors", []) and args[0] == "verify":
        return fail("Error: no matching signatures: rekor lookup: 502 Bad Gateway\nerror during command execution: no matching signatures: rekor lookup: 502 Bad Gateway")
    regex = args[args.index("--certificate-identity-regexp") + 1]
    if args[0] == "verify":
        identity = fx.get("signatures", {}).get(ref)
        if identity is None:
            return fail("Error: no signatures found\nerror during command execution: no signatures found", 10)
        if not re.search(regex, identity):
            return fail(f'Error: no matching signatures: failed to verify certificate identity: no matching CertificateIdentity found, last error: expected SAN value to match regex "{regex}", got "{identity}"')
        return 0
    if args[0] == "verify-attestation":
        if ref in fx.get("attestation_errors", []):
            return fail("Error: getting trusted root: GET https://tuf-repo-cdn.sigstore.dev/timestamp.json: 502 Bad Gateway")
        entries = [e for e in fx.get("attestations", {}).get(ref, []) if re.search(regex, e["identity"])]
        if not entries:
            return fail("Error: no matching attestations: \nerror during command execution: no matching attestations: ")
        for entry in entries:
            statement = {"predicateType": "https://cosign.sigstore.dev/attestation/v1",
                         "predicate": {"Data": json.dumps(entry["predicate"]), "Timestamp": "2026-09-17T00:00:00Z"}}
            print(json.dumps({"payloadType": "application/vnd.in-toto+json",
                              "payload": base64.b64encode(json.dumps(statement).encode()).decode()}))
        return 0
    return fail(f"fake cosign: unsupported call {args}", 2)


def gh(args, fx):
    if args[:2] == ["run", "list"]:
        print(json.dumps(fx.get("runs", [])))
        return 0
    if args[0] == "api":
        if "DELETE" in args:
            return 0
        path = next(a for a in args[1:] if a.startswith("/"))
        if path.startswith("/users/") and path.split("?")[0].endswith("/packages"):
            print(json.dumps([{"name": name} for name in fx.get("user_packages", [])]))
            return 0
        match = re.match(r"/users/[^/]+/packages/container/([^/?]+)(/versions)?", path)
        if not match or match.group(1) not in fx.get("packages", {}):
            return fail("gh: Not Found (HTTP 404)")
        if match.group(2):
            print(json.dumps(fx["packages"][match.group(1)]))
        return 0
    return fail(f"fake gh: unsupported call {args}", 2)


def main():
    tool = os.path.basename(sys.argv[0])
    args = sys.argv[1:]
    with open(os.environ["FAKE_REGISTRY"]) as f:
        fx = json.load(f)
    with open(os.environ["FAKE_LOG"], "a") as log:
        log.write(json.dumps([tool] + args) + "\n")
    return {"skopeo": skopeo, "cosign": cosign, "gh": gh}[tool](args, fx)


if __name__ == "__main__":
    sys.exit(main())
