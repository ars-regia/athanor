#!/usr/bin/env bash
# Rechunks one built system image into an OCI layout (docs/architecture/doc_system_image.md,
# S9). The Containerfile leaves one layer per RUN, and a change in an early RUN rewrites every
# later layer, so an update downloaded about half the image whatever had changed.
# `rpm-ostree compose build-chunked-oci` packs the same root file system into layers grouped
# by package; a layer's digest stays the same while its packages do, so a machine downloads
# only the layers whose packages changed. A chunked layer holds whole files and depends on no
# other layer, so their order does not matter.
#
# The generated boot artefacts (initramfs, UKI) belong to no package and change with every
# kernel or dracut input: they are tagged as one component of their own with the
# user.component extended attribute, which the chunker honours, so a kernel change costs
# that layer and the kernel packages' layers and nothing else.
#
# The tool is the rpm-ostree the image itself ships (the base image carries it), run in a
# container of the image with the image mounted as a temporary read-write overlay: the
# attributes land in that overlay and nowhere else. The labels are carried over, except those
# the chunker writes itself (ostree.*, rpmostree.*, containers.bootc), which describe the
# unchunked layers. SOURCE_DATE_EPOCH, when set, reaches the tool.
#
# The chunker keeps a package in the layer it had in the previous build when it can read that
# build's manifest and configuration, so consecutive builds share layers even when the set of
# packages changes. --previous names that build: its manifest and configuration are copied into
# the output layout before the run (two small blobs, no layer), and a missing reference (the
# first chunked build of a tag) leaves the tool to plan from scratch.
#
# Usage: rechunk-image.sh [--previous REF] IMAGE OCI_DIR
#   REF      the previous chunked build as docker://registry/repo:tag
#   IMAGE    a local image (containers-storage), as system/build-image.sh tagged it
#   OCI_DIR  where the OCI layout is written, image reference "image"; it must not exist.
#            Push it with `system/build-image.sh ... --push-only --oci OCI_DIR`.
# Environment: RECHUNK_CPUS caps the CPUs of the tool's container (unset: no cap);
#              TMPDIR (default /var/tmp) holds the tool's ostree repository, about the
#              uncompressed size of the image.
set -euo pipefail

usage() {
    echo "usage: ${0##*/} [--previous REF] IMAGE OCI_DIR" >&2
    exit 2
}
previous=
if [[ ${1:-} == --previous ]]; then
    [[ -n ${2:-} ]] || usage
    previous=$2
    shift 2
fi
[[ $# -eq 2 && -n $1 && -n $2 ]] || usage
image=$1 out=$2
[[ ! -e $out ]] || {
    echo "${0##*/}: $out already exists" >&2
    exit 2
}
mkdir -p "$(dirname "$out")"
out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")

# seed_previous REF LAYOUT: put REF's manifest and configuration in LAYOUT as reference "image".
seed_previous() {
    local ref=$1 layout=$2 err config digest
    # One fetch, written byte for byte: the manifest's digest is the hash of these bytes.
    if ! err=$(skopeo inspect --raw "$ref" 2>&1 >"$layout/manifest.tmp"); then
        [[ $err == *"manifest unknown"* ]] || {
            echo "$err" >&2
            return 1
        }
        echo "no previous build at $ref: the layers are planned from scratch"
        rm "$layout/manifest.tmp"
        return 0
    fi
    # A chunked build annotates its layers with the components they hold; the unchunked
    # images published before rechunking do not, and carry no plan worth keeping.
    if ! jq -e '.layers | any(.annotations["ostree.components"] != null)' "$layout/manifest.tmp" >/dev/null; then
        echo "previous build at $ref is not a chunked OCI image: the layers are planned from scratch"
        rm "$layout/manifest.tmp"
        return 0
    fi
    digest=sha256:$(sha256sum "$layout/manifest.tmp" | cut -d' ' -f1)
    mv "$layout/manifest.tmp" "$layout/blobs/sha256/${digest#sha256:}"
    config=$(jq -r .config.digest "$layout/blobs/sha256/${digest#sha256:}")
    # By digest, so the configuration is the one this manifest names even if the tag moved.
    err=$(skopeo inspect --config --raw "${ref%:*}@$digest" 2>&1 >"$layout/blobs/sha256/${config#sha256:}") || {
        echo "$err" >&2
        return 1
    }
    [[ sha256:$(sha256sum "$layout/blobs/sha256/${config#sha256:}" | cut -d' ' -f1) == "$config" ]] || {
        echo "${0##*/}: configuration of $ref does not match its digest" >&2
        return 1
    }
    jq -n --arg d "$digest" --argjson s "$(stat -c %s "$layout/blobs/sha256/${digest#sha256:}")" \
        '{schemaVersion: 2, manifests: [{mediaType: "application/vnd.oci.image.manifest.v1+json",
          digest: $d, size: $s, annotations: {"org.opencontainers.image.ref.name": "image"}}]}' \
        >"$layout/index.json"
    echo "previous build: $ref ($digest)"
}

labels=()
while IFS= read -r label; do
    labels+=("--label=$label")
done < <(podman image inspect --format '{{json .Labels}}' "$image" |
    jq -r 'to_entries[] | select(.key | test("^(ostree\\.|rpmostree\\.|containers\\.bootc$)") | not) | "\(.key)=\(.value)"')

work=$(mktemp -d "${TMPDIR:-/var/tmp}/rechunk.XXXXXX")
# The tool runs as root in the user namespace, so what it leaves is owned by subordinate ids.
trap 'podman unshare rm -rf "$work"' EXIT
mkdir "$work/out"

cpus=()
[[ -z ${RECHUNK_CPUS:-} ]] || cpus=(--cpus "$RECHUNK_CPUS")
# The tool reads the previous build from its output reference, to keep the same packages in
# the same layers; a fresh layout holds none, and the tool then plans the layers from scratch.
mkdir -p "$work/out/blobs/sha256"
echo '{"imageLayoutVersion":"1.0.0"}' >"$work/out/oci-layout"
echo '{"schemaVersion":2,"manifests":[]}' >"$work/out/index.json"
[[ -z $previous ]] || seed_previous "$previous" "$work/out"
# CAP_SYS_ADMIN: without it rpm-ostree re-executes itself under `unshare --map-auto`, which
# needs a subordinate id range for root that a container does not have.
# shellcheck disable=SC2016 # expanded by the container's shell
podman run --rm --network=none --security-opt label=disable --cap-add SYS_ADMIN "${cpus[@]}" \
    --mount "type=image,source=$image,destination=/rootfs,rw=true" \
    -v "$work:/var/tmp" -e SOURCE_DATE_EPOCH \
    --entrypoint /usr/bin/bash "$image" -euo pipefail -c '
    shopt -s nullglob
    for f in /rootfs/usr/lib/modules/*/{initramfs.img,vmlinuz.efi,uki.efi} /rootfs/boot/efi/EFI/Linux/*.efi; do
      setfattr -n user.component -v athanor-boot "$f"
    done
    rpm-ostree compose build-chunked-oci --bootc --format-version=2 --rootfs=/rootfs \
      --output=oci:/var/tmp/out:image "$@"' rechunk "${labels[@]}"
podman unshare chown -R 0:0 "$work/out"
mv "$work/out" "$out"
echo "rechunked: $image -> oci:$out ($(skopeo inspect --raw "oci:$out" | jq '.layers | length') layers)"
