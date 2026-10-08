#!/usr/bin/env bash
# Runs every fuzz target of fuzz/ for a fixed time each, in the build stage of the shell rig
# (the image that carries the GTK stack the bar and the compositor client link).
#
# usage: scripts/ci/fuzz.sh
# environment:
#   FUZZ_SECONDS  seconds per target (default 240)
#   FUZZ_TARGETS  space-separated target names (default: every [[bin]] of fuzz/Cargo.toml)
#   FUZZ_OUT      output directory (default .scratch/fuzz): artifacts/<target>/ holds the
#                 crashing inputs, corpus/<target>/ the corpus a run grew, logs/<target>.log
#                 the libFuzzer output; fuzz/corpus is never modified
# exit status: 1 when a target crashed or could not run. Every target runs either way.
#
# A crashing input is a finding: commit it to fuzz/corpus/<target>/ with the fix, and the
# replay test of athanor-fuzz-entries keeps it a regression test in the pull request gate.
set -euo pipefail

root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)
out=${FUZZ_OUT:-$root/.scratch/fuzz}
seconds=${FUZZ_SECONDS:-240}

bash "$root/forge/test/shell/rig.sh" build-image
image=${ATHANOR_RIG_BUILD_IMAGE:-localhost/athanor-shell-rig:build}

mkdir -p "$out"
# cargo-fuzz creates directories under fuzz/ (git-ignored), so the checkout is writable.
podman run --rm --memory 8g --security-opt label=disable \
    -v "$root:/repo" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
    -e FUZZ_SECONDS="$seconds" -e FUZZ_TARGETS="${FUZZ_TARGETS:-}" \
    -e FUZZ_SANITIZER="${FUZZ_SANITIZER:-address}" \
    "$image" bash /repo/scripts/ci/fuzz-container.sh
