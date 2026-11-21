#!/usr/bin/env bash
# Rust security audit: clippy (warnings denied) over the root workspace and the toolkit's gallery,
# the implicit panics of the root workspace against scripts/ci/implicit-panics.txt (verify.py),
# and the advisory check of the cargo-deny policy (deny.toml) over their lockfiles and fuzz/'s, and
# `cargo audit --deny yanked` over the vendored lockfiles of scripts/ci/audited-lockfiles.txt.
# Licences, bans and sources of the same policy run in the pull request gate (`just check-deny`), so they are not repeated here.
# Logs land in $AUDIT_OUT (default: audit-out) so a workflow can upload the directory.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="${AUDIT_OUT:-$root/audit-out}"
mkdir -p "$out"
cd "$root"

# Warnings fail the job: the unsafe lints of .cargo/config.toml are set to warn there.
# cargo unifies features across the packages of one invocation: glycin drives zbus with
# async-io, the rest of the workspace with tokio, so the preview decoder is linted alone.
cargo clippy --workspace --exclude athanor-preview-render --all-targets --all-features -- -D warnings 2>&1 | tee "$out/clippy.log"
cargo clippy -p athanor-preview-render --all-targets --all-features -- -D warnings 2>&1 | tee -a "$out/clippy.log"
cargo clippy --manifest-path system/athanor-sal/gallery/Cargo.toml --locked --all-targets -- -D warnings 2>&1 | tee -a "$out/clippy.log"

# --config is explicit: cargo-deny would otherwise look for a deny.toml next to the
# manifest and fall back to its defaults.
# The toolkit's gallery and fuzz/ have a workspace and a lockfile of their own, so they are audited
# beside the root's.
: > "$out/deny.log"
for manifest in Cargo.toml system/athanor-sal/gallery/Cargo.toml fuzz/Cargo.toml; do
    cargo deny --manifest-path "$manifest" --config deny.toml check advisories 2>&1 | tee -a "$out/deny.log"
done

# The vendored lockfiles cargo-deny cannot read (no workspace) are audited by cargo-audit, which
# reads a bare lockfile. Yanked crates are denied, as deny.toml does; unsound and unmaintained
# advisories are reported, not denied, until greenboot-rs carries the rustix fix (#1302). An
# `unaudited` line of the list is skipped. An empty or missing list fails.
list=scripts/ci/audited-lockfiles.txt
[ -f "$list" ] || { echo "security-audit: $list is missing" >&2; exit 1; }
mapfile -t entries < "$list"
locks=()
for entry in "${entries[@]}"; do
    case "$entry" in '' | '#'* | 'unaudited '*) ;; *) locks+=("$entry") ;; esac
done
[ "${#locks[@]}" -gt 0 ] || { echo "security-audit: $list names no lockfile" >&2; exit 1; }
: > "$out/audit.log"
for lock in "${locks[@]}"; do
    echo "security-audit: cargo audit $lock" | tee -a "$out/audit.log"
    cargo audit --deny yanked --file "$lock" < /dev/null 2>&1 | tee -a "$out/audit.log"
done

# The implicit panics per crate (overflow, index, string slice) against their budget (#1352), last:
# under `set -e` its failure would hide the results of cargo-deny and cargo-audit. The check runs
# the two workspace clippy runs of the start again with its lints, on the same target, and fails on
# a crate over its line, a crate without one and a line above the measured count.
ATHANOR_IMPLICIT_PANICS_CARGO=cargo python3 -B scripts/verify.py implicit-panics 2>&1 | tee "$out/implicit-panics.log"
