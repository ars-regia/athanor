#!/usr/bin/env bash
# Arms auto-merge on a bot pull request whose change has the shape the bot produces, after
# the check that builds it went green: Spec Build Check calls it with `spec`, System Image
# Check with `system`. The merge then waits for the required checks of the branch protection.
#
# Usage: bot_automerge.sh spec|system PR HEAD_SHA   (GH_TOKEN: a token that may merge)
#        bot_automerge.sh disarm BRANCH
#
# spec   — branch chore/update-specs-zero-trust (forge-util-update-specs.yml); only *.spec and
#          SOURCES/sources.sha256 under forge/specs/<package>/ (azoth excluded), only Version,
#          Release and manifest lines changed, and every Version keeps its leftmost non-zero
#          component (1.6.0 -> 1.7.1 merges, 1.9 -> 2.0 and 0.3 -> 0.4 wait for a person).
# system — branch bump/system-* with label system-bump (kernel-bump.yml, system group); only
#          system/Containerfile and system/nvidia/locks/*.lock, only the digests of existing
#          FROM lines and the RPM lines of the locks changed.
#
# disarm — turns auto-merge off on the open pull request of BRANCH before a bot pushes to it
#          again: GitHub keeps auto-merge armed across pushes by a writer, and the new head
#          would merge on the required checks alone, before its own build ran.
#
# A pull request of any other shape stays for a person: the reason goes to the log and the
# job summary and the script exits 0. A failing gh call fails the run.
set -euo pipefail

usage="usage: bot_automerge.sh spec|system PR HEAD_SHA | disarm BRANCH"
kind=${1:?$usage}
if [[ $kind == disarm ]]; then
    branch=${2:?$usage}
    armed=$(gh pr list --head "$branch" --state open --json number,autoMergeRequest \
        --jq '.[] | select(.autoMergeRequest != null) | .number')
    for pr in $armed; do
        gh pr merge "$pr" --disable-auto
        echo "bot_automerge: #$pr auto-merge turned off before the bot pushes to $branch"
    done
    exit 0
fi
pr=${2:?$usage}
sha=${3:?$usage}
[[ $kind == spec || $kind == system ]] || {
    echo "$usage" >&2
    exit 2
}

report() {
    echo "bot_automerge: #$pr $*"
    if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then echo "bot_automerge: #$pr $*" >> "$GITHUB_STEP_SUMMARY"; fi
}
refuse() {
    report "stays for a person: $*"
    exit 0
}

view=$(gh pr view "$pr" --json state,headRefName,headRefOid,isCrossRepository,labels,files)
field() { jq -r "$1" <<< "$view"; }

[[ $(field .state) == OPEN ]] || refuse "is not open"
[[ $(field .isCrossRepository) == false ]] || refuse "comes from a fork"
[[ $(field .headRefOid) == "$sha" ]] || refuse "head moved past the checked commit $sha"

branch=$(field .headRefName)
files=$(field '.files[].path')
case $kind in
spec)
    [[ $branch == chore/update-specs-zero-trust ]] || refuse "branch $branch is not the spec bot's"
    allowed='^forge/specs/[^/]+/([^/]+\.spec|SOURCES/sources\.sha256)$'
    ;;
system)
    [[ $branch == bump/system-* ]] || refuse "branch $branch is not the system bump bot's"
    field '.labels[].name' | grep -qx system-bump || refuse "has no system-bump label"
    allowed='^system/(Containerfile|nvidia/locks/[^/]+\.lock)$'
    ;;
esac
[[ -n $files ]] || refuse "changes no file"
while IFS= read -r f; do
    [[ $f =~ $allowed && $f != forge/specs/azoth/* ]] || refuse "touches $f, outside what the bot changes"
done <<< "$files"

# The first line that breaks the shape, or nothing. Both sides of the diff are read: a line
# the bot would never write and a line it would never remove both send the PR to a person.
verdict=$(gh pr diff "$pr" | awk '
  function fail(msg) { print file ": " msg; bad = 1; exit }
  # A length check, not [0-9a-f]{64}: mawk, the awk of Ubuntu runners, may lack intervals.
  function hex64(s) { return s ~ /^[0-9a-f]+$/ && length(s) == 64 }
  # Same components up to and including the leftmost non-zero one of the old version.
  function same_major(a, b,   pa, pb, n, i, j) {
    n = split(a, pa, "."); split(b, pb, ".")
    for (i = 1; i < n && pa[i] ~ /^0+$/; i++) {}
    for (j = 1; j <= i; j++) if (pa[j] != pb[j]) return 0
    return 1
  }
  /^diff --git / { if (old != "" || new != "") fail("Version removed or added"); file = substr($4, 3); next }
  /^(\+\+\+|---) / { next }
  /^[-+]/ {
    sign = substr($0, 1, 1); line = substr($0, 2)
    if (file ~ /\.spec$/) {
      if (line ~ /^Release:[ \t]+[^ \t]+$/) next
      if (line !~ /^Version:[ \t]+[^ \t]+$/) fail("changes a line other than Version or Release: " line)
      split(line, v, /[ \t]+/)
      if (sign == "-") old = v[2]; else new = v[2]
      if (old != "" && new != "") {
        if (!same_major(old, new)) fail("Version " old " -> " new " changes the major version")
        old = new = ""
      }
    } else if (file ~ /sources\.sha256$/ || file ~ /\.lock$/) {
      split(line, h, "  ")
      if (!(line ~ /^[0-9a-f]+  [^ ]+$/ && hex64(h[1]))) fail("changes a line that is not a sha256 entry: " line)
    } else if (file == "system/Containerfile") {
      n = split(line, w, /[ @]/)
      if (!(n == 5 && w[1] == "FROM" && w[4] == "AS" && w[3] ~ /^sha256:/ && hex64(substr(w[3], 8))))
        fail("changes a line other than a FROM digest: " line)
      key = w[2] " " w[5]
      if (sign == "-") from[key]++; else from[key]--
    }
  }
  END {
    if (bad) exit
    if (old != "" || new != "") { print file ": Version removed or added"; exit }
    for (k in from) if (from[k] != 0) { print "system/Containerfile: FROM " k " added or removed"; exit }
  }
')
[[ -z $verdict ]] || refuse "$verdict"

gh pr merge "$pr" --auto --squash --match-head-commit "$sha"
report "auto-merge armed at $sha ($kind bot, the change has the bot's shape)"
