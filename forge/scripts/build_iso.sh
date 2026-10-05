#!/usr/bin/env bash
# build_iso.sh IMAGE OUTPUT_DIR: installer ISO of a bootc image, built with
# bootc-image-builder.
#
# Not the osbuild/bootc-image-builder-action: the builder looks for the installer
# package list as <ID>-<VERSION_ID>.yaml, where ID comes from the os-release of the
# image being installed, and its search paths are compiled in. Athanor declares
# ID=athanor, which matches none of the definitions shipped inside the builder, so ours
# has to be mounted over that directory, and the action passes no volumes of its own.
#
# The builder reads the image from the root container storage and does not pull it, so
# this script does. The build configuration has no option of its own: the builder reads
# whatever is mounted at /config.toml. Its output belongs to root, hence the --chown.
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: build_iso.sh IMAGE OUTPUT_DIR" >&2; exit 2; }
image=$1 output=$2
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
builder=${BIB_IMAGE:-quay.io/centos-bootc/bootc-image-builder:latest}
config="${repo}/system/disk_config/iso.toml"
defs="${repo}/system/disk_config/defs"

[[ -f $config ]] || { echo "ISO configuration missing: $config" >&2; exit 2; }
[[ -d $defs ]] || { echo "distro definitions missing: $defs" >&2; exit 2; }

# The builder needs the root container storage. Already root (a container): run
# directly. Otherwise sudo, which must be there. Defined before its first use.
sudo() {
  if [[ $(id -u) -eq 0 ]]; then
    "$@"
  elif command -v sudo > /dev/null; then
    command sudo "$@"
  else
    echo "build_iso.sh must run as root or with sudo available" >&2
    return 1
  fi
}

mkdir -p "$output"

# The builder works through the root container storage, which on a hosted runner is
# either absent or left by another tool with a different graph driver recorded in its
# database; podman then refuses to open it ("database graph driver does not match").
# Declare the driver and start that store empty, as the upstream action does
# (osbuild/bootc-image-builder issue 446). Nothing of ours lives there: the image is
# built rootless, in the user's own store, and pushed to the registry before this runs.
sudo mkdir -p /etc/containers
printf '[storage]\ndriver = "overlay"\nrunroot = "/run/containers/storage"\ngraphroot = "/var/lib/containers/storage"\n' \
  | sudo tee /etc/containers/storage.conf > /dev/null
sudo rm -rf /var/lib/containers/storage

# Both pulls cross the network to a registry that answers 502 now and then: quay.io did
# for the builder in run 34375049610, after the system image had been built, signed and
# pushed, and the whole job was lost to it. They are retried the way the workflows'
# own pulls are; retry.sh is run through sudo rather than the other way round so that
# the function above still decides whether sudo is needed.
retry="${repo}/forge/scripts/retry.sh"
sudo bash "$retry" podman pull "$builder"
sudo bash "$retry" podman pull "$image"

# --rootfs: the builder formats the installed root with ext4, xfs or btrfs; the image
# installs btrfs.
sudo podman run --rm --privileged --security-opt label=type:unconfined_t \
  -v /var/lib/containers/storage:/var/lib/containers/storage \
  -v "${config}:/config.toml:ro" \
  -v "${defs}:/usr/share/bootc-image-builder/defs:ro" \
  -v "${output}:/output" \
  "$builder" build \
    --type anaconda-iso \
    --rootfs btrfs \
    --use-librepo=True \
    --chown "$(id -u):$(id -g)" \
    --output /output \
    "$image"
