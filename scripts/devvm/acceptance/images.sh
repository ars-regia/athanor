#!/usr/bin/env bash
# Builds, publishes and signs the images of the acceptance in a throwaway registry on the
# host (docs/architecture/doc_update_trust.md, section 6). Keys are generated here, stay
# under $ACC_STATE/keys and sign nothing else.
#   tag  created     keys shipped  signed with
#   v1   2026-09-10  1             1            the machine's starting point
#   v2   2026-09-15  1             1            through system/sign-images.sh, as the pipeline will
#   v3   2026-09-16  1             (nothing)
#   v3w  2026-09-16  1             other
#   v3b  2026-09-16  cosign-1      cosign-1, as a cosign 3 bundle only
#   old  2026-09-01  1             1
#   v4   2026-09-17  1 and 2       1            rotation, first half
#   v5   2026-09-18  1 and 2       2            rotation, second half
#   v6   2026-09-19  1 and 2       2            after the recovery it is "signed with the old key"
#   v7   2026-09-20  3             3            the recovery target
# Usage: images.sh     (needs podman, skopeo, jq; `nix` for the cosign 3 bundle of v3b)
set -euo pipefail
# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

keys=$ACC_STATE/keys
mkdir -p "$keys"
: > "$keys/empty.pass"
for key in acc-1 acc-2 acc-3 other; do
  [[ -f $keys/$key.pub ]] || skopeo generate-sigstore-key --output-prefix "$keys/$key" --passphrase-file "$keys/empty.pass"
done

# skopeo reads registries.conf from $HOME/.config/containers or /etc only (CONTAINERS_REGISTRIES_CONF
# is podman's): the pipeline scripts run with a throwaway HOME that marks the local registry insecure.
mkdir -p "$ACC_STATE/home/.config/containers"
printf '[[registry]]\nlocation = "%s"\ninsecure = true\n' "${ACC_REGISTRY%%/*}" > "$ACC_STATE/home/.config/containers/registries.conf"
# skopeo writes a sigstore attachment only where registries.d enables it.
mkdir -p "$ACC_STATE/registries.d"
printf 'docker:\n  %s:\n    use-sigstore-attachments: true\n' "$ACC_REGISTRY" > "$ACC_STATE/registries.d/acceptance.yaml"

podman container exists athanor-acc-registry || podman run -d --name athanor-acc-registry -p "127.0.0.1:$ACC_PORT:5000" docker.io/library/registry:2
rpm=$(find "$ACC_RPM_DIR" -name 'athanor-update-1*.x86_64.rpm' | sort -V | tail -n 1)
[[ -n $rpm ]] || die "no athanor-update RPM under $ACC_RPM_DIR: run forge/scripts/build_rolling_local.sh update"

build() { # build TAG CREATED KEY...
  local tag=$1 created=$2 context
  shift 2
  context=$(mktemp -d)
  mkdir "$context/rpm" "$context/keys"
  cp "$rpm" "$context/rpm/"
  for key in "$@"; do cp "$keys/$key.pub" "$context/keys/athanor-image-${key#acc-}.pub"; done
  podman build --format docker -f "$ACC_HERE/Containerfile" -t "$REPO:$tag" \
    --build-arg "BASE=$ACC_BASE" --build-arg "REGISTRY=$ACC_REGISTRY" --build-arg "MARKER=$tag" --build-arg "GUEST_USER=$GUEST_USER" \
    --label "org.opencontainers.image.created=${created}T10:00:00Z" --label "org.opencontainers.image.version=43.${created//-/}.0" "$context"
  rm -rf "$context"
  podman push --tls-verify=false "$REPO:$tag"
}
sign() { # sign TAG KEY
  skopeo --registries.d "$ACC_STATE/registries.d" copy --src-tls-verify=false --dest-tls-verify=false --sign-by-sigstore-private-key "$keys/$2.private" \
    --sign-passphrase-file "$keys/empty.pass" "docker://$REPO:$1" "docker://$REPO:$1"
}

build v1 2026-09-10 acc-1; sign v1 acc-1
build old 2026-09-01 acc-1; sign old acc-1
build v3 2026-09-16 acc-1
build v3w 2026-09-16 acc-1; sign v3w other
build v4 2026-09-17 acc-1 acc-2; sign v4 acc-1
build v5 2026-09-18 acc-1 acc-2; sign v5 acc-2
build v6 2026-09-19 acc-1 acc-2; sign v6 acc-2
build v7 2026-09-20 acc-3; sign v7 acc-3

# v2 goes the pipeline's way: three repositories, the digests file, sign-images.sh.
build v2 2026-09-15 acc-1
for name in athanor-system-nvidia athanor-system-nvidia-legacy; do
  skopeo copy --src-tls-verify=false --dest-tls-verify=false "docker://$REPO:v2" "docker://$ACC_REGISTRY/$name:v2"
done
mkdir -p "$ACC_STATE/pipeline-keys"
cp "$keys/acc-1.pub" "$ACC_STATE/pipeline-keys/athanor-image-1.pub"
HOME=$ACC_STATE/home bash "$ROOT/system/image-digests.sh" --registry "$ACC_REGISTRY" --tag v2 --out "$ACC_STATE/image-digests.txt"
COSIGN_PRIVATE_KEY=$(< "$keys/acc-1.private") COSIGN_PASSWORD='' SIGN_KEYS_DIR=$ACC_STATE/pipeline-keys \
  HOME=$ACC_STATE/home bash "$ROOT/system/sign-images.sh" "$ACC_STATE/image-digests.txt"

# v3b: the only signature is what cosign 3 writes, a bundle index at sha256-<hex>.
# cosign signs with a key pair of its own making; the image ships that public key, so the
# refusal is about the format and not about the key.
[[ -f $keys/cosign-1.pub ]] || (cd "$keys" && COSIGN_PASSWORD='' nix run nixpkgs#cosign -- generate-key-pair --output-key-prefix cosign-1)
build v3b 2026-09-16 cosign-1
printf '%s' '{"mediaType":"application/vnd.dev.sigstore.signingconfig.v0.2+json","rekorTlogConfig":{},"tsaConfig":{}}' > "$ACC_STATE/no-rekor.json"
digest=$(skopeo inspect --tls-verify=false --format '{{.Digest}}' "docker://$REPO:v3b")
COSIGN_PASSWORD='' nix run nixpkgs#cosign -- sign --yes --allow-insecure-registry --signing-config "$ACC_STATE/no-rekor.json" \
  --key "$keys/cosign-1.key" "$REPO@$digest"
point_stable v1
echo "images published under $ACC_REGISTRY; stable -> v1"
