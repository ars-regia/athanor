#!/usr/bin/env bash
# Downloads every unexpired acceptance evidence artifact of the repository (acceptance-<run>-
# <attempt>, uploaded by .github/workflows/iso-acceptance.yml) into DIR, one directory per
# artifact, for system/promote-auto.sh. The GitHub-specific half of the promotion: the choice
# itself reads files on disk. No evidence at all is not an error, there is then nothing to
# promote.
# Usage: fetch-evidence.sh DIR
# Environment: GITHUB_REPOSITORY (owner/name); gh authenticated (GH_TOKEN) with actions: read.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && -n ${GITHUB_REPOSITORY:-} ]] || {
    echo "usage: GITHUB_REPOSITORY=owner/name ${0##*/} DIR" >&2
    exit 2
}
dir=$1
mkdir -p "$dir"
artifacts=$(gh api --paginate "repos/$GITHUB_REPOSITORY/actions/artifacts?per_page=100" \
    --jq '.artifacts[] | select(.expired | not) | select(.name | test("^acceptance-[0-9]+-[0-9]+$")) | "\(.id) \(.name)"')
count=0
while read -r id name; do
    [[ -n $id ]] || continue
    gh api "repos/$GITHUB_REPOSITORY/actions/artifacts/$id/zip" >"$dir/$name.zip"
    unzip -q -d "$dir/$name" "$dir/$name.zip"
    rm "$dir/$name.zip"
    count=$((count + 1))
done <<<"$artifacts"
echo "acceptance evidence: $count artifact(s) in $dir"
