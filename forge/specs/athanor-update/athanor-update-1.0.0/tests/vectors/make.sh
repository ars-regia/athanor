#!/usr/bin/env bash
# Regenerates the signature-object test vectors of athanor-update under tests/vectors/made.
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

find "$out" -type f | sort
