#!/usr/bin/env bash
# Builds one forge spec into forge/RPMS, inside the builder image with the repository mounted
# at /workspace and forge/ as the working directory. The DAG (call-dag-compile.yml) and Spec
# Build Check (build_changed_specs.sh) both run it, so a green check is the build the DAG does.
#
# Usage: build_spec.sh SPEC_DIR   (relative to forge/, e.g. specs/athanor-tetragon)
#
# Rust compiles through sccache: the DAG mounts its per-package cache at ~/.cache/sccache.
set -euo pipefail

SPEC_DIR=${1:?usage: build_spec.sh SPEC_DIR}

mkdir -p /tmp
chmod 1777 /tmp
mkdir -p ~
cp config/rpmmacros ~/.rpmmacros
mkdir -p ~/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

if [[ -d $SPEC_DIR/SOURCES ]]; then
    cp -a "$SPEC_DIR"/SOURCES/. ~/rpmbuild/SOURCES/
fi
bash scripts/fetch_sources.sh "$SPEC_DIR" ~/rpmbuild/SOURCES

export RUSTC_WRAPPER=sccache
sccache --start-server

# A spec without Source builds the checkout in place (rpmbuild --build-in-place): rpm 4.20 runs
# %build in BUILD/<name>-<ver>-build, which --build-in-place makes a symlink to the workspace,
# so `cargo build` and paths relative to the repository root work. Specs with a Source take the
# ordinary path, %prep included.
if [[ -n $(rpmspec -q --srpm --qf '[%{SOURCE} ]' "$SPEC_DIR"/*.spec) ]]; then
    rpmbuild -bb --nodeps "$SPEC_DIR"/*.spec
else
    (cd /workspace && rpmbuild -bb --nodeps --build-in-place "/workspace/forge/$SPEC_DIR"/*.spec)
fi

mkdir -p /workspace/forge/RPMS
cp ~/rpmbuild/RPMS/*/*.rpm /workspace/forge/RPMS/
sccache --show-stats
