#!/usr/bin/env bash
# Builds one forge spec into forge/RPMS, inside the builder image with the repository mounted
# at /workspace and forge/ as the working directory. run_spec_build.sh calls it twice, in two
# containers that share the builder's HOME (a podman volume at /root):
#
#   fetch  with network: the Source files (verified against SOURCES/sources.sha256), the crates
#          and the Go modules the build will need, into ~/rpmbuild/SOURCES, ~/.cargo and ~/go.
#   build  with --network=none: rpmbuild from what the fetch stage left in HOME.
#
# Usage: build_spec.sh fetch|build SPEC_DIR   (SPEC_DIR relative to forge/, e.g. specs/athanor-tetragon)
#
# The DAG (call-dag-compile.yml) and Spec Build Check (build_changed_specs.sh) both run it through
# run_spec_build.sh, so a green check is the build the DAG does. Rust compiles through sccache:
# the DAG mounts its per-package cache at ~/.cache/sccache.
set -euo pipefail

usage="usage: build_spec.sh fetch|build SPEC_DIR"
STAGE=${1:?$usage}
SPEC_DIR=${2:?$usage}

# /tmp belongs to each container, not to the shared HOME.
mkdir -p /tmp
chmod 1777 /tmp

# A spec without Source builds the checkout in place (rpmbuild --build-in-place): rpm 4.20 runs
# %build in BUILD/<name>-<ver>-build, which --build-in-place makes a symlink to the workspace,
# so `cargo build` and paths relative to the repository root work. Specs with a Source take the
# ordinary path, %prep included.
in_place() {
    [[ -z $(rpmspec -q --srpm --qf '[%{SOURCE} ]' "$SPEC_DIR"/*.spec) ]]
}

fetch() {
    cp config/rpmmacros ~/.rpmmacros
    mkdir -p ~/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
    if [[ -d $SPEC_DIR/SOURCES ]]; then
        cp -a "$SPEC_DIR"/SOURCES/. ~/rpmbuild/SOURCES/
    fi
    bash scripts/fetch_sources.sh "$SPEC_DIR" ~/rpmbuild/SOURCES

    if in_place; then
        # The workspace's crates, and those of any other manifest the spec names, at the
        # versions of their Cargo.lock: the spec builds with --locked.
        local parsed manifest
        parsed=$(rpmspec -P "$SPEC_DIR"/*.spec)
        grep -qE '^[[:space:]]*cargo[[:space:]]' <<< "$parsed" || return 0
        for manifest in Cargo.toml $(sed -n 's/.*--manifest-path[ =]\([^ ]*\).*/\1/p' <<< "$parsed" | sort -u); do
            (cd /workspace && cargo fetch --locked --manifest-path "$manifest")
        done
    else
        # Unpack the sources and fetch what their lockfiles pin; the build stage unpacks
        # them again, from the same verified archives, without network.
        rpmbuild -bp --nodeps "$SPEC_DIR"/*.spec
        local lock mod
        while IFS= read -r lock; do
            cargo fetch --manifest-path "${lock%.lock}.toml"
        done < <(find ~/rpmbuild/BUILD -maxdepth 4 -name Cargo.lock)
        while IFS= read -r mod; do
            (cd "$(dirname "$mod")" && go mod download)
        done < <(find ~/rpmbuild/BUILD -maxdepth 4 -name go.mod)
    fi
}

build() {
    export RUSTC_WRAPPER=sccache
    sccache --start-server
    if in_place; then
        (cd /workspace && rpmbuild -bb --nodeps --build-in-place "/workspace/forge/$SPEC_DIR"/*.spec)
    else
        rpmbuild -bb --nodeps "$SPEC_DIR"/*.spec
    fi
    mkdir -p /workspace/forge/RPMS
    cp ~/rpmbuild/RPMS/*/*.rpm /workspace/forge/RPMS/
    sccache --show-stats
}

case $STAGE in
fetch) fetch ;;
build) build ;;
*)
    echo "$usage" >&2
    exit 2
    ;;
esac
