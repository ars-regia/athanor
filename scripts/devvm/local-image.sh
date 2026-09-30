#!/usr/bin/env bash
# local-image.sh [--push-to-vm] SPEC_DIR... - a local system image carrying the RPMs of an
# unmerged branch, for the acceptance on the development VM (scripts/devvm/README.md).
#
#   1. builds the RPMs of the given forge/specs directories in the builder image, as
#      call-dag-compile.yml does, into .scratch/local-image/rpms;
#   2. serves them to system/build-image.sh through a tier3 overlay on the acceptance
#      registry (localhost:5000): the published tier3 minus the rebuilt packages and minus
#      the packages the switch removes, plus the new RPMs;
#   3. builds localhost:5000/acc/athanor-system:switch-<short hash> with the tier3
#      reference remapped to the overlay, and pushes it to the acceptance registry. The UKI
#      is signed with build-image.sh's throwaway key: the image never leaves this host;
#   4. with --push-to-vm, switches the development VM to it and reboots it.
#
# The tag goes to .scratch/local-image/tag. OWNER (default ghcr.io/hr-mes), BUILDER and
# TIER3 name the published images. Needs podman, rpm and, for --push-to-vm, gh and the VM.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(git -C "$HERE" rev-parse --show-toplevel)
OWNER=${OWNER:-ghcr.io/hr-mes}
BUILDER=${BUILDER:-$OWNER/athanor-builder:latest}
TIER3=${TIER3:-$OWNER/athanor-forge-tier3-repo}
OVERLAY=localhost:5000/${TIER3#*/}
# Packages the switch retires: their published RPMs must not reach the image.
REMOVED=(athanor-layout-translator)

usage() { sed -n '2,/^set -euo pipefail$/{/^#/{s/^# \{0,1\}//;p}}' "${BASH_SOURCE[0]}"; }
push=false
while [[ $# -gt 0 ]]; do
    case $1 in
    -h | --help)
        usage
        exit 0
        ;;
    --push-to-vm)
        push=true
        shift
        ;;
    -*)
        usage >&2
        exit 2
        ;;
    *) break ;;
    esac
done
[[ $# -gt 0 ]] || {
    usage >&2
    exit 2
}
for spec in "$@"; do
    compgen -G "$ROOT/forge/specs/$spec/*.spec" > /dev/null || {
        echo "${0##*/}: forge/specs/$spec has no spec file" >&2
        exit 2
    }
done

work=$ROOT/.scratch/local-image
rm -rf "$work"
mkdir -p "$work/rpms" "$work/repo"

# The acceptance registry, as acceptance/images.sh runs it.
podman container exists athanor-acc-registry ||
    podman run -d --name athanor-acc-registry -p 127.0.0.1:5000:5000 docker.io/library/registry:2
podman start athanor-acc-registry > /dev/null

# 1. The RPMs. A spec with a Source follows the ordinary path, %prep included; one without
# builds the checkout in place (call-dag-compile.yml, "Compile").
bash "$ROOT/forge/scripts/retry.sh" podman pull "$BUILDER"
for spec in "$@"; do
    podman run --rm --security-opt label=disable -e "SPEC_DIR=specs/$spec" \
        -v "$ROOT:/workspace" -w /workspace/forge "$BUILDER" bash -euo pipefail -c '
      cp config/rpmmacros ~/.rpmmacros
      mkdir -p ~/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
      if [ -d "$SPEC_DIR/SOURCES" ]; then cp -a "$SPEC_DIR"/SOURCES/. ~/rpmbuild/SOURCES/; fi
      bash scripts/fetch_sources.sh "$SPEC_DIR" ~/rpmbuild/SOURCES
      if [ -n "$(rpmspec -q --srpm --qf "[%{SOURCE} ]" "$SPEC_DIR"/*.spec)" ]; then
        rpmbuild -bb --nodeps "$SPEC_DIR"/*.spec
      else
        (cd /workspace && rpmbuild -bb --nodeps --build-in-place "/workspace/forge/$SPEC_DIR"/*.spec)
      fi
      cp ~/rpmbuild/RPMS/*/*.rpm /workspace/.scratch/local-image/rpms/'
done

# 2. The overlay. The published tier3 holds one directory per package and no repodata: the
# Containerfile installs every *.rpm it finds, so the new RPMs go in a directory of their own.
bash "$ROOT/forge/scripts/retry.sh" podman pull "$TIER3:latest"
cid=$(podman create "$TIER3:latest" /none)
podman export "$cid" | tar -x -C "$work/repo"
podman rm "$cid" > /dev/null
mapfile -t drop < <(find "$work/rpms" -name '*.rpm' -exec rpm -qp --qf '%{NAME}\n' {} +)
drop+=("${REMOVED[@]}")
while IFS= read -r -d '' rpm; do
    name=$(rpm -qp --qf '%{NAME}' "$rpm")
    for gone in "${drop[@]}"; do
        if [[ $name == "$gone" ]]; then
            rm "$rpm"
            break
        fi
    done
done < <(find "$work/repo" -name '*.rpm' -print0)
mkdir "$work/repo/local-image"
cp "$work"/rpms/*.rpm "$work/repo/local-image/"
printf 'FROM scratch\nCOPY repo/ /\n' > "$work/Containerfile.overlay"
podman build -t "$OVERLAY:latest" -f "$work/Containerfile.overlay" "$work"
podman push --tls-verify=false "$OVERLAY:latest"

# 3. The image. build-image.sh pulls with --pull=newer, which would replace a locally
# retagged tier3 with the published one: the overlay is served by a registries.conf remap.
cp /etc/containers/registries.conf "$work/registries.conf"
printf '\n[[registry]]\nprefix = "%s"\nlocation = "%s"\ninsecure = true\n' "$TIER3" "$OVERLAY" >> "$work/registries.conf"
tag=switch-$(git -C "$ROOT" rev-parse --short HEAD)
bash "$ROOT/system/kernel-artifacts.sh" require-ready
CONTAINERS_REGISTRIES_CONF=$work/registries.conf \
    bash "$ROOT/system/build-image.sh" --gpu none --registry localhost:5000/acc --tag "$tag"
# The remap left the local tier3 tag pointing at the overlay: take the published one back.
bash "$ROOT/forge/scripts/retry.sh" podman pull "$TIER3:latest"
podman push --tls-verify=false "localhost:5000/acc/athanor-system:$tag"
echo "$tag" > "$work/tag"
echo "${0##*/}: localhost:5000/acc/athanor-system:$tag"

# 4. The development VM on it.
if $push; then
    # shellcheck source-path=SCRIPTDIR
    source "$HERE/acceptance/lib.sh"
    guard_no_ci
    wait_ssh
    guest_ssh "printf '[[registry]]\nlocation = \"localhost:5000\"\ninsecure = true\n' | sudo tee /etc/containers/registries.conf.d/50-acceptance.conf > /dev/null"
    guest_ssh sudo bootc switch --transport registry "$REPO:$tag"
    reboot_guest
    wait_ssh
fi
