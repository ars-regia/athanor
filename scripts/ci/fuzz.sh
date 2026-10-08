#!/usr/bin/env bash
# Runs every fuzz target of fuzz/ for a fixed time each, in the build stage of the shell rig
# (the image that carries the GTK stack the bar and the compositor client link).
#
# usage: scripts/ci/fuzz.sh
# environment:
#   FUZZ_SECONDS  seconds per target (default 240)
#   FUZZ_TARGETS  space-separated target names (default: every [[bin]] of fuzz/Cargo.toml)
#   FUZZ_CORPUS_IN a persistent corpus directory (<target>/ inside): merged with the seeds of
#                 fuzz/corpus before each target runs, and updated with the grown corpus after
#                 the run, crash or not; unset, every run starts from the seeds
#   FUZZ_BUDGET_MINUTES  the time the caller allows the whole run (default 150, the timeout
#                 of fuzz.yml); FUZZ_BUILD_MINUTES the part of it kept for the image and the
#                 builds (default 30). The targets times FUZZ_SECONDS must fit in the rest.
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
budget=${FUZZ_BUDGET_MINUTES:-150}
build=${FUZZ_BUILD_MINUTES:-30}

for setting in "FUZZ_SECONDS=$seconds" "FUZZ_BUDGET_MINUTES=$budget" "FUZZ_BUILD_MINUTES=$build"; do
    case ${setting#*=} in
        '' | *[!0-9]* | 0*) echo "$setting: a positive integer is required" >&2; exit 2 ;;
    esac
done
if [ -n "${FUZZ_TARGETS:-}" ]; then
    read -r -a named <<<"$FUZZ_TARGETS"
    count=${#named[@]}
else
    count=$(grep -c '^\[\[bin\]\]' "$root/fuzz/Cargo.toml")
fi
if [ $((count * seconds + build * 60)) -gt $((budget * 60)) ]; then
    echo "$count targets x $seconds s plus $build min of builds exceed the $budget min budget" >&2
    exit 2
fi

bash "$root/forge/test/shell/rig.sh" build-image
image=${ATHANOR_RIG_BUILD_IMAGE:-localhost/athanor-shell-rig:build}

mkdir -p "$out"
corpus_in=()
if [ -n "${FUZZ_CORPUS_IN:-}" ]; then
    mkdir -p "$FUZZ_CORPUS_IN"
    corpus_in=(-v "$(realpath "$FUZZ_CORPUS_IN"):/corpus-in:ro")
fi

# cargo-fuzz creates directories under fuzz/ (git-ignored), so the checkout is writable.
# The registry volume is the CARGO_HOME of fuzz-container.sh (/opt/cargo), not the image's.
status=0
podman run --rm --memory 8g --security-opt label=disable \
    -v "$root:/repo" -v "$out:/out" -v athanor-cargo-registry:/opt/cargo/registry \
    "${corpus_in[@]}" \
    -e FUZZ_SECONDS="$seconds" -e FUZZ_TARGETS="${FUZZ_TARGETS:-}" \
    -e FUZZ_SANITIZER="${FUZZ_SANITIZER:-address}" \
    "$image" bash /repo/scripts/ci/fuzz-container.sh || status=$?

if [ -n "${FUZZ_CORPUS_IN:-}" ] && [ -d "$out/corpus" ]; then
    cp -r "$out/corpus/." "$FUZZ_CORPUS_IN/"
fi
exit "$status"
