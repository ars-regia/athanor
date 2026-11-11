#!/usr/bin/env bash
# Moves Sulphur's Smithay pin to the head of Smithay's master (doc_own_stack.md, OS3).
#
#   scripts/bump/smithay.sh OUT_DIR
#
# Smithay publishes no release, so Sulphur takes it at a git revision. This rewrites that
# revision in Sulphur's Cargo.toml and refreshes Cargo.lock through cargo. It writes
# OUT_DIR/title and OUT_DIR/body.md, which open_bump_pr.sh turns into a pull request, only when
# master has moved and no pull request with the label smithay-bump is open: one bump at a time,
# fixed on its own branch when master changed the API, and the next one after its merge.
# Needs git, cargo and an authenticated gh.
set -euo pipefail
(($# == 1)) || {
    sed -n '2,10p' "$0" >&2
    exit 2
}
out=$1
root=$(git rev-parse --show-toplevel)
manifest=forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/Cargo.toml
url=https://github.com/Smithay/smithay
mapfile -t pins < <(grep -oP "git = \"${url}\", rev = \"\K[0-9a-f]{40}" "$root/$manifest" | sort -u)
((${#pins[@]} == 1)) || {
    echo "smithay.sh: ${manifest} pins Smithay at ${#pins[@]} revisions, expected one" >&2
    exit 1
}
old=${pins[0]}
new=$(gh api repos/Smithay/smithay/commits/master --jq .sha)
[[ $new =~ ^[0-9a-f]{40}$ ]] || {
    echo "smithay.sh: Smithay's master answered '${new}', not a commit" >&2
    exit 1
}
mkdir -p "$out"
if [[ $new == "$old" ]]; then
    echo "Smithay is pinned at the head of master (${old:0:12}): nothing to move"
    exit 0
fi
open=$(gh pr list --label smithay-bump --state open --json number --jq '.[].number')
if [[ -n $open ]]; then
    echo "the Smithay bump #${open//$'\n'/, #} is open: the next one opens after it is merged or closed"
    exit 0
fi
sed -i "s/rev = \"${old}\"/rev = \"${new}\"/" "$root/$manifest"
cargo update --manifest-path "$root/Cargo.toml" -p smithay -p smithay-drm-extras
grep -q "^source = \"git+${url}?rev=${new}#${new}\"$" "$root/Cargo.lock" ||
    {
        echo "smithay.sh: Cargo.lock does not take Smithay at ${new} after cargo update" >&2
        exit 1
    }
commits=$(gh api "repos/Smithay/smithay/compare/${old}...${new}" \
    --jq '.commits[] | "\(.sha[:9]) \(.commit.message | split("\n")[0])"')
echo "chore(sulphur): bump Smithay to ${new:0:12}" > "$out/title"
cat > "$out/body.md" << EOF
Moves Sulphur's Smithay pin from \`${old:0:12}\` to \`${new:0:12}\`, the head of Smithay's master
([compare](${url}/compare/${old}...${new})). Smithay changes its API on master without a
release: when this pull request does not build, Sulphur is fixed on this branch before the merge.
Opened weekly by \`smithay-bump.yml\`; the next bump opens once this one is merged or closed.
The commits, fenced so that their @names and #numbers neither notify nor link here:

\`\`\`text
${commits}
\`\`\`
EOF
