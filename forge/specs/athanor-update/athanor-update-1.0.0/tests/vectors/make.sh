#!/usr/bin/env bash
# Regenerates the signature-object and release-attestation test vectors of athanor-update
# under tests/vectors/made.
# Two throwaway P-256 keys are created, used and deleted: only public material is left
# (public keys, payloads, signatures), and that is what gets committed. ECDSA signatures
# are randomised, so a second run gives different bytes with the same meaning; the tests
# assert meaning. tests/vectors/real is not generated here: it is what
# `skopeo copy --sign-by-sigstore-private-key` (containers/image 5.39.2) wrote during
# spike U1, fetched back with `skopeo copy … dir:`.
# Needs openssl and python3 (standard library only).
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
out=$here/made
secrets=$(mktemp -d)
trap 'rm -rf "$secrets"' EXIT
rm -rf "$out"
mkdir -p "$out"
for key in a b; do
  openssl ecparam -name prime256v1 -genkey -noout -out "$secrets/$key.pem"
  openssl ec -in "$secrets/$key.pem" -pubout -out "$out/$key.pub" 2> /dev/null
done

DIGEST=sha256:$(printf 'the image manifest' | sha256sum | cut -d' ' -f1)
export DIGEST
echo "$DIGEST" > "$out/image-digest"

payload() { # payload REFERENCE TYPE [EXTRA-CRITICAL-MEMBER]
  python3 - "$@" <<'PY'
import json, os, sys
critical = {"identity": {"docker-reference": sys.argv[1]}, "image": {"docker-manifest-digest": os.environ["DIGEST"]}, "type": sys.argv[2]}
if len(sys.argv) > 3:
    critical[sys.argv[3]] = True
sys.stdout.write(json.dumps({"critical": critical, "optional": {"creator": "make.sh"}}, separators=(",", ":")))
PY
}

sign() { # sign KEY FILE -> base64 DER
  openssl dgst -sha256 -sign "$secrets/$1.pem" "$2" | base64 -w0
}

# The layers of an object arrive on stdin, so the script is an argument and not a here-document.
OBJECT_PY='
import hashlib, json, pathlib, sys
dest = pathlib.Path(sys.argv[1]); dest.mkdir(parents=True)
layers = []
for line in sys.stdin.read().splitlines():
    blob_file, signature = line.split(" ", 1)
    blob = pathlib.Path(blob_file).read_bytes()
    digest = hashlib.sha256(blob).hexdigest()
    (dest / digest).write_bytes(blob)
    layers.append({"mediaType": "application/vnd.dev.cosign.simplesigning.v1+json", "digest": "sha256:" + digest, "size": len(blob),
                   "annotations": {"dev.cosignproject.cosign/signature": signature}})
config = b"{}"
config_digest = hashlib.sha256(config).hexdigest()
(dest / config_digest).write_bytes(config)
manifest = {"schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": "sha256:" + config_digest, "size": len(config)},
            "layers": layers}
(dest / "manifest.json").write_text(json.dumps(manifest, separators=(",", ":")))
'
object() { # object NAME: reads "BLOBFILE SIGNATURE" lines on stdin, one layer each
  python3 -c "$OBJECT_PY" "$out/$1"
}

work=$secrets/work
mkdir "$work"
REPO=registry.example/owner/athanor-system

# good: two layers in push order, b first and a second, as a rotation leaves them.
payload "$REPO:101" "cosign container image signature" > "$work/good"
printf '%s %s\n%s %s\n' "$work/good" "$(sign b "$work/good")" "$work/good" "$(sign a "$work/good")" | object good

payload "$REPO:101" "atomic container signature" > "$work/wrong-type"
printf '%s %s\n' "$work/wrong-type" "$(sign a "$work/wrong-type")" | object wrong-type

payload "$REPO:101" "cosign container image signature" unexpected > "$work/extra-field"
printf '%s %s\n' "$work/extra-field" "$(sign a "$work/extra-field")" | object extra-field

payload "registry.example/owner/athanor-system-nvidia:101" "cosign container image signature" > "$work/other-repo"
printf '%s %s\n' "$work/other-repo" "$(sign a "$work/other-repo")" | object other-repo

# raw-signature: the 64 bytes r||s instead of ASN.1 DER.
raw=$(openssl dgst -sha256 -sign "$secrets/a.pem" "$work/good" | python3 -c '
import base64, sys
der = sys.stdin.buffer.read()
def integers(buf):
    assert buf[0] == 0x30; i = 2
    while i < len(buf):
        assert buf[i] == 0x02; n = buf[i + 1]; yield int.from_bytes(buf[i + 2:i + 2 + n], "big"); i += 2 + n
r, s = integers(der)
sys.stdout.write(base64.b64encode(r.to_bytes(32, "big") + s.to_bytes(32, "big")).decode())')
printf '%s %s\n' "$work/good" "$raw" | object raw-signature

# reserialised: the same JSON value with other bytes, under the signature of the original.
python3 -c 'import json,sys; sys.stdout.write(json.dumps(json.load(open(sys.argv[1])), indent=1))' "$work/good" > "$work/reserialised"
printf '%s %s\n' "$work/reserialised" "$(sign a "$work/good")" | object reserialised

# descriptor-mismatch: a validly signed blob stored under the name of other bytes.
printf '%s %s\n' "$work/good" "$(sign a "$work/good")" | object descriptor-mismatch
good_hex=$(sha256sum "$work/good" | cut -d' ' -f1)
other_hex=$(printf 'other bytes' | sha256sum | cut -d' ' -f1)
mv "$out/descriptor-mismatch/$good_hex" "$out/descriptor-mismatch/$other_hex"
sed -i "s/$good_hex/$other_hex/" "$out/descriptor-mismatch/manifest.json"

# bundle: what cosign 3 publishes instead, an index with no layers of ours.
mkdir "$out/bundle"
printf '%s' '{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","artifactType":"application/vnd.dev.sigstore.bundle.v0.3+json","digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","size":2}]}' > "$out/bundle/manifest.json"

# Release attestations (ADR-0127 points 1 to 4), as `cosign attest --new-bundle-format=false`
# stores `<repo>:sha256-<hex>.att`: one DSSE envelope per layer, signed over the DSSE
# pre-authentication encoding of an in-toto statement. Each line on stdin is one layer, a
# JSON object: `key` signs; `class` (absent: no class member), `advisories`, `repo`, `digest`,
# `type` (the statement's `_type`), `predicateType` and `payloadType` default to a valid
# statement for $REPO and $DIGEST; `cosign` orders the members as cosign 3.0.6 writes them,
# `_type` v0.1 first; `extraSubject` adds a second subject after the first;
# `duplicateClass` writes `"class":"feature"` before the class member, in the signed bytes;
# `tamper` is `signature` (one byte flipped), `der` (the DER cut short), `pae` (signed over the
# encoding less its last byte) or `payload` (feature signed, then the class changed to
# security); `encoding` writes the payload's base64 `unpadded` or with `trailing-bits` set;
# `pad` adds that many spaces after the envelope.
ATT_PY='
import base64, hashlib, json, os, pathlib, subprocess, sys
dest = pathlib.Path(sys.argv[1]); secrets = pathlib.Path(sys.argv[2]); dest.mkdir(parents=True)
def pae(kind, body):
    kind = kind.encode()
    return b"DSSEv1 %d %s %d %s" % (len(kind), kind, len(body), body)
layers = []
for line in sys.stdin.read().splitlines():
    spec = json.loads(line)
    predicate = {"advisories": spec.get("advisories", []), "class_reason": "make.sh", "build_time": "2026-10-21T00:00:00Z",
                 "revision": "0" * 40, "build_run_id": "1", "updateinfo_timestamp": None}
    if "class" in spec:
        predicate["class"] = spec["class"]
    subject = [{"name": spec.get("repo", os.environ["REPO"]), "digest": {"sha256": spec.get("digest", os.environ["DIGEST"]).removeprefix("sha256:")}}]
    if spec.get("extraSubject"):
        subject.append({"name": os.environ["REPO"] + "-nvidia", "digest": subject[0]["digest"]})
    if spec.get("cosign"):
        statement = {"_type": "https://in-toto.io/Statement/v0.1", "predicateType": os.environ["PREDICATE"], "subject": subject, "predicate": predicate}
    else:
        statement = {"_type": spec.get("type", "https://in-toto.io/Statement/v1"), "subject": subject,
                     "predicateType": spec.get("predicateType", os.environ["PREDICATE"]), "predicate": predicate}
    body = json.dumps(statement, separators=(",", ":")).encode()
    if spec.get("duplicateClass"):
        body = body.replace(b"\"class\":", b"\"class\":\"feature\",\"class\":", 1)
    if "encoding" in spec and len(body) % 3 == 0:
        body += b" "
    kind = spec.get("payloadType", "application/vnd.in-toto+json")
    tamper = spec.get("tamper")
    signed = pae(kind, body)[:-1] if tamper == "pae" else pae(kind, body)
    sig = subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(secrets / (spec["key"] + ".pem"))], input=signed, capture_output=True, check=True).stdout
    if tamper == "signature":
        sig = sig[:-1] + bytes([sig[-1] ^ 1])
    elif tamper == "der":
        sig = sig[:-1]
    elif tamper == "payload":
        tampered = body.replace(b"\"class\":\"feature\"", b"\"class\":\"security\"")
        assert tampered != body
        body = tampered
    payload = base64.b64encode(body).decode()
    if spec.get("encoding") == "unpadded":
        payload = payload.rstrip("=")
    elif spec.get("encoding") == "trailing-bits":
        alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        last = len(payload.rstrip("=")) - 1
        payload = payload[:last] + alphabet[alphabet.index(payload[last]) + 1] + payload[last + 1:]
    assert spec.get("encoding") is None or base64.b64decode(payload + "==") == body
    envelope = {"payloadType": kind, "payload": payload, "signatures": [{"keyid": "", "sig": base64.b64encode(sig).decode()}]}
    blob = json.dumps(envelope, separators=(",", ":")).encode() + b" " * spec.get("pad", 0)
    digest = hashlib.sha256(blob).hexdigest()
    (dest / digest).write_bytes(blob)
    layers.append({"mediaType": "application/vnd.dsse.envelope.v1+json", "digest": "sha256:" + digest, "size": len(blob),
                   "annotations": {"dev.cosignproject.cosign/signature": "", "predicateType": statement["predicateType"]}})
config = b"{}"
config_digest = hashlib.sha256(config).hexdigest()
(dest / config_digest).write_bytes(config)
manifest = {"schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": "sha256:" + config_digest, "size": len(config)},
            "layers": layers}
(dest / "manifest.json").write_text(json.dumps(manifest, separators=(",", ":")))
'
attestation() { # attestation NAME: reads one JSON layer specification per line on stdin
  python3 -c "$ATT_PY" "$out/$1" "$secrets"
}
export REPO PREDICATE='https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b'
security='"class":"security","advisories":["CVE-2026-0001"]'
attestation att-security <<< "{\"key\":\"a\",$security}"
attestation att-feature <<< '{"key":"a","class":"feature"}'
attestation att-no-class <<< '{"key":"a"}'
attestation att-bad-signature <<< "{\"key\":\"a\",$security,\"tamper\":\"signature\"}"
attestation att-malformed-der <<< "{\"key\":\"a\",$security,\"tamper\":\"der\"}"
attestation att-truncated-pae <<< "{\"key\":\"a\",$security,\"tamper\":\"pae\"}"
attestation att-tampered-payload <<< '{"key":"a","class":"feature","tamper":"payload"}'
attestation att-other-digest <<< "{\"key\":\"a\",$security,\"digest\":\"sha256:$(printf 'another manifest' | sha256sum | cut -d' ' -f1)\"}"
attestation att-other-repo <<< "{\"key\":\"a\",$security,\"repo\":\"registry.example/owner/athanor-system-nvidia\"}"
attestation att-wrong-type <<< "{\"key\":\"a\",$security,\"predicateType\":\"https://slsa.dev/provenance/v1\"}"
attestation att-wrong-payload-type <<< "{\"key\":\"a\",$security,\"payloadType\":\"application/json\"}"
attestation att-unknown-key <<< "{\"key\":\"b\",$security}"
printf '%s\n' "{\"key\":\"b\",$security}" '{"key":"a","class":"feature"}' | attestation att-mixed
attestation att-oversized <<< "{\"key\":\"a\",$security,\"pad\":70000}"
attestation att-statement-v01 <<< "{\"key\":\"a\",$security,\"type\":\"https://in-toto.io/Statement/v0.1\"}"
attestation att-statement-v2 <<< "{\"key\":\"a\",$security,\"type\":\"https://in-toto.io/Statement/v2\"}"
# cosign 3.0.6 (generateCustomStatement): `_type` v0.1, one subject named by the repository
# without tag or digest, `{"sha256": <bare hex>}`.
attestation att-cosign <<< "{\"key\":\"a\",$security,\"cosign\":true,\"repo\":\"ghcr.io/owner/athanor-system\"}"
attestation att-two-subjects <<< "{\"key\":\"a\",$security,\"extraSubject\":true}"
attestation att-duplicate-class <<< "{\"key\":\"a\",$security,\"duplicateClass\":true}"
attestation att-unpadded <<< "{\"key\":\"a\",$security,\"encoding\":\"unpadded\"}"
attestation att-trailing-bits <<< "{\"key\":\"a\",$security,\"encoding\":\"trailing-bits\"}"
attestation att-long-advisory <<< "{\"key\":\"a\",\"class\":\"security\",\"advisories\":[\"CVE-2026-$(printf '%0100d' 1)\"]}"
attestation att-many-advisories <<< "{\"key\":\"a\",\"class\":\"security\",\"advisories\":$(python3 -c 'import json; print(json.dumps(["CVE-2026-%05d" % n for n in range(1025)]))')}"
attestation att-empty-advisories <<< '{"key":"a","class":"security","advisories":[]}'
# A verified statement of another predicate type says nothing of the class, either way.
printf '%s\n' "{\"key\":\"a\",$security}" '{"key":"a","class":"feature","predicateType":"https://slsa.dev/provenance/v1"}' | attestation att-foreign-feature
printf '%s\n' '{"key":"a","class":"feature"}' "{\"key\":\"a\",$security,\"predicateType\":\"https://slsa.dev/provenance/v1\"}" | attestation att-foreign-security

find "$out" -type f | sort
