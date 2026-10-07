#!/usr/bin/env bash
# Rust security audit: clippy over the root workspace and the cargo-deny policy
# (deny.toml) over its lockfile.
# Logs land in $AUDIT_OUT (default: audit-out) so a workflow can upload the directory.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="${AUDIT_OUT:-$root/audit-out}"
mkdir -p "$out"
cd "$root"

# cargo unifies features across the packages of one invocation: glycin drives zbus with
# async-io, the rest of the workspace with tokio, so the preview decoder is linted alone.
cargo clippy --workspace --exclude athanor-preview-render --all-targets --all-features 2>&1 | tee "$out/clippy.log"
cargo clippy -p athanor-preview-render --all-targets --all-features 2>&1 | tee -a "$out/clippy.log"

# --config is explicit: cargo-deny would otherwise look for a deny.toml next to the
# manifest and fall back to its defaults.
cargo deny --manifest-path Cargo.toml --config deny.toml check 2>&1 | tee "$out/deny.log"
