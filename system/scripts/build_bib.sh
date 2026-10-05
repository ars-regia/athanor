#!/bin/bash
set -euo pipefail

TARGET_IMAGE="${1:-localhost/athanor-system}"
TAG="${2:-latest}"
TYPE="${3:-qcow2}"
BIB_IMAGE="${BIB_IMAGE:-quay.io/centos-bootc/bootc-image-builder:latest}"

config="disk_config/disk.toml"
if [[ "${TYPE}" == "iso" ]]; then
    config="disk_config/iso.toml"
fi

BUILDTMP=$(mktemp -d -t _build-bib.XXXXXXXXXX)
sudo podman run --rm -it --privileged --pull=newer --net=host \
  --security-opt label=type:unconfined_t \
  -v "$(pwd)/${config}:/config.toml:ro" \
  -v "$BUILDTMP:/output" \
  -v /var/lib/containers/storage:/var/lib/containers/storage \
  "${BIB_IMAGE}" --type "${TYPE}" --use-librepo=True --rootfs=btrfs --config /config.toml \
  "${TARGET_IMAGE}:${TAG}"

if [[ "${TYPE}" == "qcow2" ]]; then
    sudo qemu-img convert -f qcow2 -O vhdx "$BUILDTMP/qcow2/disk.qcow2" "$BUILDTMP/disk.vhdx"
fi

mkdir -p output
sudo mv -f "$BUILDTMP"/* output/
sudo rmdir "$BUILDTMP"
sudo chown -R "${USER}:${USER}" output/
