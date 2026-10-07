#!/usr/bin/env bash
# Installs the pinned tools `just check` needs on a machine that lacks them (the GitHub
# runner of pr.yml): just and actionlint from their release archives, each verified against
# its SHA-256 before it is unpacked, and pykickstart (ksvalidator) and PyYAML in a virtual
# environment. DIR becomes that environment: put DIR/bin first on PATH, and python3 then has
# PyYAML for the D43 lint of verify.py workflows. The archives stay in DIR/download.
#
# Usage: install-tools.sh DIR
set -euo pipefail

dir=${1:?usage: install-tools.sh DIR}

JUST_URL=https://github.com/casey/just/releases/download/1.57.0/just-1.57.0-x86_64-unknown-linux-musl.tar.gz
JUST_SHA256=45b548094283cb9739af8f13273b8cddeee869f5b4ef2bb631b1f311cb566155
ACTIONLINT_URL=https://github.com/rhysd/actionlint/releases/download/v1.7.9/actionlint_1.7.9_linux_amd64.tar.gz
ACTIONLINT_SHA256=233b280d05e100837f4af1433c7b40a5dcb306e3aa68fb4f17f8a7f45a7df7b4

# fetch URL SHA256 FILE: downloads URL to FILE and fails unless FILE has digest SHA256.
fetch() {
    curl -fsSL --retry 3 -o "$3" "$1"
    echo "$2  $3" | sha256sum -c -
}

python3 -m venv "$dir"
mkdir -p "$dir/download"
fetch "$JUST_URL" "$JUST_SHA256" "$dir/download/just.tar.gz"
fetch "$ACTIONLINT_URL" "$ACTIONLINT_SHA256" "$dir/download/actionlint.tar.gz"
tar -xzf "$dir/download/just.tar.gz" -C "$dir/bin" just
tar -xzf "$dir/download/actionlint.tar.gz" -C "$dir/bin" actionlint
"$dir/bin/pip" install --quiet pykickstart==3.78 pyyaml==6.0.2
