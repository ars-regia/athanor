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
#   bad  2026-09-21  3             3            v7 whose greetd.service runs /usr/bin/false (stage greenboot)
# Usage: images.sh     (needs podman, skopeo, jq, curl; `nix` for the cosign 3 bundle of v3b)
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

# The registry runs in a user unit of its own, named by ACC_REGISTRY_NAME: started from a run that is
# itself a transient unit, its processes landed in that unit's cgroup and died with it (#556).
# The container keeps the images across runs; the unit only runs it.
podman container exists "$ACC_REGISTRY_NAME" ||
  podman create --name "$ACC_REGISTRY_NAME" -p "127.0.0.1:$ACC_PORT:5000" docker.io/library/registry:2
if ! systemctl --user -q is-active "$ACC_REGISTRY_NAME"; then
  # A unit still stopping would make systemd-run refuse the name: wait until it is gone.
  for _ in $(seq 30); do
    [[ $(systemctl --user show -P ActiveState "$ACC_REGISTRY_NAME") == deactivating ]] || break
    sleep 1
  done
  # One started outside the unit by an earlier images.sh: the unit takes it over.
  podman stop "$ACC_REGISTRY_NAME" > /dev/null
  systemd-run --user --quiet --collect --unit="$ACC_REGISTRY_NAME" podman start --attach "$ACC_REGISTRY_NAME"
fi
# The unit does not wait for the registry to listen: the first push would race it.
for _ in $(seq 30); do
  curl -fs -o /dev/null "http://127.0.0.1:$ACC_PORT/v2/" && break
  sleep 1
done
curl -fsS -o /dev/null "http://127.0.0.1:$ACC_PORT/v2/" || die "the acceptance registry does not answer on 127.0.0.1:$ACC_PORT: journalctl --user -u $ACC_REGISTRY_NAME"
rpm=$(find "$ACC_RPM_DIR" -name 'athanor-update-1*.x86_64.rpm' | sort -V | tail -n 1)
[[ -n $rpm ]] || die "no athanor-update RPM under $ACC_RPM_DIR: run forge/scripts/build_rolling_local.sh update"
pull_base

build() { # build TAG CREATED KEY...
  local tag=$1 created=$2 context break_greetd=
  if [[ $tag == bad ]]; then break_greetd=1; fi
  shift 2
  context=$(mktemp -d)
  mkdir "$context/rpm" "$context/keys"
  cp "$rpm" "$context/rpm/"
  for key in "$@"; do cp "$keys/$key.pub" "$context/keys/athanor-image-${key#acc-}.pub"; done
  podman build --format docker -f "$ACC_HERE/Containerfile" -t "$REPO:$tag" \
    --build-arg "BASE=$ACC_BASE" --build-arg "REGISTRY=$ACC_REGISTRY" --build-arg "MARKER=$tag" --build-arg "GUEST_USER=$GUEST_USER" --build-arg "BREAK_GREETD=$break_greetd" \
    --label "org.opencontainers.image.created=${created}T10:00:00Z" --label "org.opencontainers.image.version=43.${created//-/}.0" "$context"
  rm -rf "$context"
  podman push --tls-verify=false "$REPO:$tag"
}
sign() { # sign TAG KEY
  skopeo --registries.d "$ACC_STATE/registries.d" copy --src-tls-verify=false --dest-tls-verify=false --sign-by-sigstore-private-key "$keys/$2.private" \
    --sign-passphrase-file "$keys/empty.pass" "docker://$REPO:$1" "docker://$REPO:$1"
}

build v1 2026-09-10 acc-1; sign v1 acc-1
# After the first build, as a base the caller set may reach this host only through it.
podman image inspect --format "built on $ACC_BASE: {{.Digest}}, created {{.Created}}" "$ACC_BASE"
build old 2026-09-01 acc-1; sign old acc-1
build v3 2026-09-16 acc-1
build v3w 2026-09-16 acc-1; sign v3w other
build v4 2026-09-17 acc-1 acc-2; sign v4 acc-1
build v5 2026-09-18 acc-1 acc-2; sign v5 acc-2
build v6 2026-09-19 acc-1 acc-2; sign v6 acc-2
build v7 2026-09-20 acc-3; sign v7 acc-3
build bad 2026-09-21 acc-3; sign bad acc-3

# v2 goes the pipeline's way: three repositories, the digests file, sign-images.sh.
build v2 2026-09-15 acc-1
for name in athanor-system-nvidia athanor-system-nvidia-legacy; do
  skopeo copy --src-tls-verify=false --dest-tls-verify=false "docker://$REPO:v2" "docker://$ACC_REGISTRY/$name:v2"
done
mkdir -p "$ACC_STATE/pipeline-keys"
cp "$keys/acc-1.pub" "$ACC_STATE/pipeline-keys/athanor-image-1.pub"
HOME=$ACC_STATE/home bash "$ROOT/system/image-digests.sh" --registry "$ACC_REGISTRY" --tag v2 --out "$ACC_STATE/image-digests.txt"
COSIGN_PRIVATE_KEY=$(< "$keys/acc-1.private") COSIGN_PASSWORD='' SIGN_KEYS_DIR=$ACC_STATE/pipeline-keys \
  HOME=$ACC_STATE/home bash "$ROOT/system/sign-images.sh" --registry "$ACC_REGISTRY" "$ACC_STATE/image-digests.txt"

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
