#!/usr/bin/env bash
# The builder image a spec build runs in, for Spec Build Check and build_changed_specs.sh.
#
#   builder_image.sh build ARCHIVE
#       Builds the builder image of this checkout with Nix (flake output builderImage, as
#       call-build-builder.yml does) into the image archive ARCHIVE. Publishes nothing.
#   builder_image.sh resolve ARCHIVE IMAGE
#       Prints the image to run: ARCHIVE loaded into podman when it exists (the change touches
#       the builder's inputs), else IMAGE, the published builder, pulled from its registry.
set -euo pipefail

usage="usage: builder_image.sh build ARCHIVE | resolve ARCHIVE IMAGE"
command=${1:?$usage}
archive=${2:?$usage}

case $command in
build)
    root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)
    image=$(nix build --extra-experimental-features "nix-command flakes" --no-link \
        --print-out-paths "$root#builderImage")
    cp "$image" "$archive"
    ;;
resolve)
    published=${3:?$usage}
    if [[ -f $archive ]]; then
        loaded=$(podman load -i "$archive" | sed -n 's/^Loaded image[^:]*: //p')
        [[ -n $loaded ]] || {
            echo "builder_image.sh: podman load reported no image name" >&2
            exit 1
        }
        echo "$loaded"
    else
        bash "$(dirname "${BASH_SOURCE[0]}")/retry.sh" podman pull "$published" >&2
        echo "$published"
    fi
    ;;
*)
    echo "$usage" >&2
    exit 2
    ;;
esac
