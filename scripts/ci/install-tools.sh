#!/usr/bin/env bash
# Installs the pinned tools `just check` needs on a machine that lacks them (the GitHub
# runner of pr.yml): just, actionlint and cargo-deny from their release archives, each verified against
# its SHA-256 before it is unpacked, cosign and the registry (distribution) that the real-cosign
# attest check runs against (scripts/ci/cosign-attest-check.sh), and pykickstart (ksvalidator) and PyYAML in a virtual
# environment, every Python package verified by the hashes of requirements-build.txt and
# requirements.txt. DIR becomes that environment: put DIR/bin first on PATH, and python3 then
# has PyYAML for the D43 lint of verify.py workflows. The archives stay in DIR/download.
#
# Usage: install-tools.sh DIR
set -euo pipefail

dir=${1:?usage: install-tools.sh DIR}
here=$(dirname "$(realpath "$0")")

JUST_URL=https://github.com/casey/just/releases/download/1.57.0/just-1.57.0-x86_64-unknown-linux-musl.tar.gz
JUST_SHA256=45b548094283cb9739af8f13273b8cddeee869f5b4ef2bb631b1f311cb566155
ACTIONLINT_URL=https://github.com/rhysd/actionlint/releases/download/v1.7.9/actionlint_1.7.9_linux_amd64.tar.gz
ACTIONLINT_SHA256=233b280d05e100837f4af1433c7b40a5dcb306e3aa68fb4f17f8a7f45a7df7b4
CARGO_DENY_URL=https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz
CARGO_DENY_SHA256=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
# The cosign the workflows install and the check runs: the version that sigstore/cosign-installer
# at COSIGN_INSTALLER installs by default (its action.yml, cosign-release). A signing job may not
# set cosign-release (verify.py workflows, D43), so the installer commit pins the version.
# scripts/ci/tests/test_cosign_attest.py requires every workflow to use that commit: a bump of the
# action moves the cosign of the releases, and the check must move with it (#705).
# shellcheck disable=SC2034 # read by scripts/ci/tests/test_cosign_attest.py
COSIGN_INSTALLER=6f9f17788090df1f26f669e9d70d6ae9567deba6
COSIGN_VERSION=v3.0.6
COSIGN_URL=https://github.com/sigstore/cosign/releases/download/$COSIGN_VERSION/cosign-linux-amd64
COSIGN_SHA256=c956e5dfcac53d52bcf058360d579472f0c1d2d9b69f55209e256fe7783f4c74
REGISTRY_URL=https://github.com/distribution/distribution/releases/download/v3.0.0/registry_3.0.0_linux_amd64.tar.gz
REGISTRY_SHA256=61c9a2c0d5981a78482025b6b69728521fbc78506d68b223d4a2eb825de5ca3d

# fetch URL SHA256 FILE: downloads URL to FILE and fails unless FILE has digest SHA256.
fetch() {
    curl -fsSL --retry 3 -o "$3" "$1"
    echo "$2  $3" | sha256sum -c -
}

python3 -m venv "$dir"
# pykickstart is published only as a source archive: its build backend is installed first, by
# hash, and the archive is built against it instead of a backend pip would fetch unverified.
"$dir/bin/pip" install --quiet --require-hashes --only-binary=:all: -r "$here/requirements-build.txt"
"$dir/bin/pip" install --quiet --require-hashes --no-build-isolation -r "$here/requirements.txt"
mkdir -p "$dir/download"
fetch "$JUST_URL" "$JUST_SHA256" "$dir/download/just.tar.gz"
fetch "$ACTIONLINT_URL" "$ACTIONLINT_SHA256" "$dir/download/actionlint.tar.gz"
tar -xzf "$dir/download/just.tar.gz" -C "$dir/bin" just
tar -xzf "$dir/download/actionlint.tar.gz" -C "$dir/bin" actionlint
fetch "$CARGO_DENY_URL" "$CARGO_DENY_SHA256" "$dir/download/cargo-deny.tar.gz"
tar -xzf "$dir/download/cargo-deny.tar.gz" -C "$dir/bin" --strip-components=1 \
    cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny
fetch "$COSIGN_URL" "$COSIGN_SHA256" "$dir/download/cosign"
install -m 0755 "$dir/download/cosign" "$dir/bin/cosign"
fetch "$REGISTRY_URL" "$REGISTRY_SHA256" "$dir/download/registry.tar.gz"
tar -xzf "$dir/download/registry.tar.gz" -C "$dir/bin" registry
