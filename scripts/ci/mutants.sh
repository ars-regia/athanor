#!/usr/bin/env bash
# Mutation testing: cargo-mutants changes the code of the named crates one mutant at a time and
# runs the crate's tests against each. A mutant the tests miss is a behaviour no test pins.
# Not part of `just check`: a run takes hours, so maintenance.yml runs it weekly.
#
# usage: scripts/ci/mutants.sh CRATE...
# environment:
#   MUTANTS_OUT     output directory (default .scratch/mutants): mutants.out/ of cargo-mutants with
#                   missed.txt, caught.txt, timeout.txt and unviable.txt, and summary.txt
#   MUTANTS_IN_RIG  1 runs cargo-mutants in the build stage of the shell rig (forge/test/shell/rig.sh),
#                   which carries the system libraries athanor-sal and the GTK crates link; the
#                   script builds the image first. Unset, it runs cargo on this machine.
#   MUTANTS_EXTRA   further cargo-mutants arguments, space-separated (e.g. "--file src/lib.rs")
#   cargo-mutants   must be on PATH (scripts/ci/install-tools.sh)
# exit status: cargo-mutants exits 0 (all caught), 2 (some mutants missed) and 3 (some timed out):
# each is a finished run that produced a report, so each is 0 here. Any other status (a usage
# error, a baseline that fails, a crash) is a failed run, and its status is returned.
set -euo pipefail

[ $# -gt 0 ] || { echo "usage: mutants.sh CRATE..." >&2; exit 2; }
command -v cargo-mutants >/dev/null || { echo "mutants.sh: cargo-mutants is not on PATH (scripts/ci/install-tools.sh)" >&2; exit 1; }

root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)
out=${MUTANTS_OUT:-$root/.scratch/mutants}
mkdir -p "$out"
out=$(realpath "$out")

packages=()
for crate in "$@"; do packages+=(--package "$crate"); done
read -r -a extra <<<"${MUTANTS_EXTRA:-}"

# --jobs 1: the mutants of one crate share the machine's cores through cargo. --cargo-arg --locked:
# a mutation run never moves the lockfile.
args=(mutants --jobs 1 --cargo-arg=--locked "${packages[@]}" "${extra[@]}")
status=0
if [ "${MUTANTS_IN_RIG:-}" = 1 ]; then
    # The rig mounts the checkout read-only and $out as /out; cargo-mutants copies the tree first.
    bash "$root/forge/test/shell/rig.sh" build-image
    ATHANOR_RIG_OUT=$out RIG_CARGO_MUTANTS=$(command -v cargo-mutants) \
        bash "$root/forge/test/shell/rig.sh" cargo "${args[@]}" --output /out || status=$?
else
    (cd "$root" && cargo "${args[@]}" --output "$out") || status=$?
fi

case $status in
0 | 2 | 3) ;;
*)
    echo "mutants.sh: cargo-mutants failed with status $status" >&2
    exit "$status"
    ;;
esac

results=$out/mutants.out
[ -s "$results/outcomes.json" ] || { echo "mutants.sh: $results/outcomes.json is missing" >&2; exit 1; }
{
    echo "cargo-mutants exit status $status (0 all caught, 2 missed, 3 timeouts)"
    for kind in caught missed timeout unviable; do
        printf '%s: %s\n' "$kind" "$(wc -l <"$results/$kind.txt")"
    done
    echo
    echo "Missed mutants:"
    cat "$results/missed.txt"
    echo
    echo "Timeouts:"
    cat "$results/timeout.txt"
} | tee "$out/summary.txt"
