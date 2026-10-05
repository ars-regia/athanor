#!/usr/bin/env bash
# What a machine on OLD downloads to move to NEW: the layers of NEW whose digest OLD does not
# have, with their compressed size (docs/architecture/doc_system_image.md, S9). Both
# references are skopeo transports: docker://registry/repo:tag, oci:/path, containers-storage:...
# The CI build compares the published moving tag with the freshly chunked layout before it
# pushes, and prints the result to the job summary.
#
# Usage: layer-delta.sh OLD_REF NEW_REF
# Prints a Markdown table row set; exits 0. When OLD does not exist (the first build of a tag)
# it says so and exits 0; any other failure to read either manifest is an error.
set -euo pipefail

[[ $# -eq 2 ]] || {
    echo "usage: ${0##*/} OLD_REF NEW_REF" >&2
    exit 2
}
old_ref=$1 new_ref=$2

manifest() {
    local out
    if ! out=$(skopeo inspect --raw "$1" 2>&1); then
        [[ $out == *"manifest unknown"* ]] && return 3
        echo "$out" >&2
        return 1
    fi
    jq -e 'has("layers")' > /dev/null <<< "$out" ||
        {
            echo "${0##*/}: $1 is not a single-image manifest" >&2
            return 1
        }
    printf '%s\n' "$out"
}

new=$(manifest "$new_ref")
status=0
old=$(manifest "$old_ref") || status=$?
if [[ $status -eq 3 ]]; then
    echo "no previous image at $old_ref: every layer of $new_ref is new"
    old='{"layers":[]}'
elif [[ $status -ne 0 ]]; then
    exit "$status"
fi

jq -rn --argjson old "$old" --argjson new "$new" --arg old_ref "$old_ref" --arg new_ref "$new_ref" '
  def mib: . / 1048576 | . * 10 | round / 10;
  ($old.layers | map(.digest)) as $have
  | ($new.layers | map(.size) | add) as $total
  | [$new.layers[] | select(.digest as $d | $have | index($d) | not)] as $fresh
  | ($fresh | map(.size) | add // 0) as $bytes
  | "| from | to | layers | changed layers | download | of the image |",
    "|---|---|---|---|---|---|",
    "| \($old_ref) | \($new_ref) | \($new.layers | length) | \($fresh | length) | \($bytes | mib) MiB | \(if $total > 0 then ($bytes * 1000 / $total | round / 10) else 0 end) % |"'
