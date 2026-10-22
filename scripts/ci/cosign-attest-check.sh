#!/usr/bin/env bash
# Runs system/sign-images.sh --attestations for real: the real cosign and skopeo, a throwaway
# key, a throwaway registry on 127.0.0.1, and the verification release_attestation.py makes.
# The unit tests of sign-images.sh mock cosign, so they could not see that cosign 3 refused
# --tlog-upload=false without --use-signing-config=false (#705); this does. cosign, skopeo and
# registry (distribution) must be on PATH: scripts/ci/install-tools.sh installs the first and the
# last at the version the signing jobs of release.yml use.
#
# Usage: cosign-attest-check.sh
# Environment: SIGN_IMAGES, the script to run (default system/sign-images.sh); it must sit in
#              system/ of a checkout, which is how it finds the rest.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
sign=${SIGN_IMAGES:-$root/system/sign-images.sh}
for tool in cosign skopeo registry python3; do
    command -v "$tool" > /dev/null || {
        echo "${0##*/}: $tool is not on PATH (scripts/ci/install-tools.sh)" >&2
        exit 2
    }
done

work=$(mktemp -d)
registry_pid=''
cleanup() {
    local status=$?
    [[ -z $registry_pid ]] || kill "$registry_pid"
    # The registry logs every 404 a push expects as an error: shown only when the check failed.
    [[ $status -eq 0 || ! -s $work/registry.log ]] || {
        echo "registry log:" >&2
        cat "$work/registry.log" >&2
    }
    rm -rf "$work"
}
trap cleanup EXIT

port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
host=127.0.0.1:$port
mkdir "$work/keys" "$work/store"

cat > "$work/registry.yml" << EOF
version: 0.1
log:
  level: error
  accesslog:
    disabled: true
storage:
  filesystem:
    rootdirectory: $work/store
http:
  addr: $host
EOF
registry serve "$work/registry.yml" > "$work/registry.log" 2>&1 &
registry_pid=$!
python3 -c '
import socket, sys, time
for _ in range(100):
    try:
        socket.create_connection(("127.0.0.1", int(sys.argv[1])), 1).close()
        sys.exit(0)
    except OSError:
        time.sleep(0.2)
sys.exit("registry did not start")' "$port"

# skopeo reaches the plain-http registry through the user-level registries.conf. cosign needs no
# flag for 127.0.0.1.
mkdir -p "$work/home/.config/containers"
printf '[[registry]]\nlocation = "%s"\ninsecure = true\n' "$host" > "$work/home/.config/containers/registries.conf"
export HOME=$work/home

# The throwaway key. sign-images.sh takes the key and the passphrase from the environment; the
# public half stands for system/keys.
(cd "$work/keys" && COSIGN_PASSWORD=throwaway cosign generate-key-pair > /dev/null)
export COSIGN_PRIVATE_KEY COSIGN_PASSWORD=throwaway
COSIGN_PRIVATE_KEY=$(< "$work/keys/cosign.key")
rm "$work/keys/cosign.key"
export SIGN_KEYS_DIR=$work/keys

# One empty image per shipped repository: a config and a manifest without layers.
config='{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}'
config_digest=$(printf '%s' "$config" | sha256sum | cut -d' ' -f1)
manifest=$(printf '{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:%s","size":%s},"layers":[]}' "$config_digest" "${#config}")
manifest_digest=$(printf '%s' "$manifest" | sha256sum | cut -d' ' -f1)
layout=$work/layout
mkdir -p "$layout/blobs/sha256"
printf '{"imageLayoutVersion":"1.0.0"}' > "$layout/oci-layout"
printf '%s' "$config" > "$layout/blobs/sha256/$config_digest"
printf '%s' "$manifest" > "$layout/blobs/sha256/$manifest_digest"
printf '{"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:%s","size":%s}]}' "$manifest_digest" "${#manifest}" > "$layout/index.json"

owner=$host/owner
printf '%s athanor-check sha256:%s\n' "$owner/athanor-system" "$manifest_digest" > "$work/digests.txt"
skopeo copy --preserve-digests "oci:$layout" "docker://$owner/athanor-system:athanor-check"

# The statement of the release, as candidate writes it. Its schema takes no registry port, so the
# statement is built here with the same function, not from a release-candidate.json.
statement=$work/statements/athanor-system/$manifest_digest.json
mkdir -p "${statement%/*}"
python3 -B -c '
import json, sys
sys.path.insert(0, sys.argv[1] + "/scripts/ci")
import release_attestation as ra
record = {"repository": sys.argv[2], "candidate_digest": "sha256:" + sys.argv[3], "class": "feature",
          "advisories": [], "class_reason": "", "updateinfo_timestamp": None}
print(json.dumps(ra.statement(record, "2026-10-21T04:00:00Z", "c" * 40, "1")))' \
    "$root" "$owner/athanor-system" "$manifest_digest" > "$statement"

bash "$sign" --registry "$owner" --attestations "$work/statements" "$work/digests.txt"
python3 -B "$root/scripts/ci/release_attestation.py" verify --keys-dir "$work/keys" \
    --statement "$statement" "$owner/athanor-system@sha256:$manifest_digest"
