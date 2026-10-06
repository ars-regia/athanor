#!/usr/bin/env bash
# Rust security audit: clippy over the root workspace, a compile check of the frozen
# shell workspace, and the cargo-deny policy (deny.toml) over both lockfiles.
# Logs land in $AUDIT_OUT (default: audit-out) so a workflow can upload the directory.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="${AUDIT_OUT:-$root/audit-out}"
mkdir -p "$out"
cd "$root"

# eBPF crates build for bpfel-unknown-none and the greeter needs GTK headers the
# audit container does not carry; none of them is clippy-checked on the host.
cargo clippy --workspace \
    --exclude ebpf-core --exclude athanor-sysmon-ebpf --exclude athanor-greeter \
    --all-targets --all-features 2>&1 | tee "$out/clippy.log"

# The frozen shell workspace (doc_shell.md, SH4) is outside the root workspace but
# still ships: the portal execs it. Its crate root allows all clippy lints, so the
# gate on it is that it compiles against its own committed lock.
cargo check --locked --manifest-path forge/specs/athanor-shell-rs/Cargo.toml --workspace

# Every lockfile that resolves a shipped binary is judged by the one policy in
# deny.toml. --config is explicit: cargo-deny would otherwise look for a deny.toml
# next to each manifest and fall back to its defaults.
for manifest in Cargo.toml forge/specs/athanor-shell-rs/Cargo.toml; do
    name="$(basename "$(dirname "$manifest")")"
    cargo deny --manifest-path "$manifest" --config deny.toml check 2>&1 | tee "$out/deny-$name.log"
done

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    { echo '## clippy (last 50 lines)'; echo '```'; tail -n 50 "$out/clippy.log"; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
fi
