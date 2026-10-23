#!/usr/bin/env bash
# The GitHub Release of a promotion (doc_pipeline.md PL42, ADR-0131). The Release is named after
# the UT9 version of the promoted build and carries its evidence bundle, whose bundle.sha256
# the plan job of promote-stable.yml already signed keylessly: nothing is signed here.
# Usage: github-release.sh plan ARTIFACTS_DIR
#        github-release.sh publish BUNDLE_DIR
# plan (the plan job, read access): chooses the Release from ARTIFACTS_DIR/promotion.json and
#   the tags at its revision, and writes ARTIFACTS_DIR/release.json (tag, title, latest,
#   notes_sha256, and the fallback when the tag is a v0.x tag), which evidence-bundle.sh create
#   signs with the bundle and the release approval covers. A v0.x tag counts only when it is
#   annotated, verified by GitHub and tagged by MAINTAINER_TAG_EMAIL (ADR-0131 decisions 3, 7);
#   any other v0.x tag at the revision fails the plan, naming the tag.
# publish (the github-release job, contents: write only): checks that BUNDLE_DIR, verified by
#   evidence-bundle.sh verify-dir in the step before, is this promotion run's and that the
#   approved choice still holds, packs it into one deterministic tar and publishes the Release
#   as a draft first, then for real. It never replaces an asset. Prints the Release's Markdown
#   line for the job summary.
# Environment: GH_TOKEN; GITHUB_REPOSITORY; MAINTAINER_TAG_EMAIL (read only when a v0.x tag
#   names the revision); RELEASE_BRANCH (default iso-v0); RELEASE_NOTES_DIR (default
#   docs/releases); publish also GITHUB_SERVER_URL, GITHUB_RUN_ID and RUN_ID.
# Exit status: 0 done or already done; 1 refused; 2 usage.
set -euo pipefail
shopt -s inherit_errexit
# One collation for the summary the notes are built from: a recovery from a workstation hashes
# the same notes as the job.
export LC_ALL=C

usage() {
    sed -n 's/^# Usage: //p; s/^#        //p' "$0" >&2
    exit 2
}
die() {
    echo "${0##*/}: $*" >&2
    exit 1
}

[[ -n ${GITHUB_REPOSITORY:-} ]] || usage
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
notes_dir=${RELEASE_NOTES_DIR:-$root/docs/releases}
branch=${RELEASE_BRANCH:-iso-v0}
repo=$GITHUB_REPOSITORY
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
VERSION_TAG='^v([0-9]+\.[0-9]+\.[0-9]+)$'

gh_must() { # gh_must ARGS... -> gh's output; any failure stops the script, naming the call
    gh "$@" > "$work/out" 2> "$work/err" || {
        cat "$work/err" >&2
        die "gh $* failed"
    }
    cat "$work/out"
}

gh_or_absent() { # gh_or_absent ARGS... -> gh's output, nothing when it answers "not found"
    if gh "$@" > "$work/out" 2> "$work/err"; then
        cat "$work/out"
        return
    fi
    # Anything but "not found" is a real failure, never read as absent.
    grep -q -e 'release not found' -e '(HTTP 404)' "$work/err" || {
        cat "$work/err" >&2
        die "gh $* failed"
    }
}

RELEASE_FIELDS=isDraft,isPrerelease,tagName,name,targetCommitish,body,assets,url
release_view() { # release_view TAG -> the Release at TAG, draft or not; nothing when there is none
    gh_or_absent release view "$1" -R "$repo" --json "$RELEASE_FIELDS"
}

tag_commit() { # tag_commit TAG -> the commit TAG names; nothing when the tag does not exist
    local ref sha
    ref=$(gh_or_absent api "/repos/$repo/git/ref/tags/$1")
    [[ -n $ref ]] || return 0
    sha=$(jq -r '.object.sha' <<< "$ref")
    if [[ $(jq -r '.object.type' <<< "$ref") == tag ]]; then
        sha=$(gh_must api "/repos/$repo/git/tags/$sha" | jq -r '.object.sha')
    fi
    echo "$sha"
}

other_build() { # other_build RELEASE_JSON RUN -> the build run of an evidence asset that is not RUN's
    jq -r --arg run "$2" '[.assets[].name | select(startswith("evidence-"))
        | (capture("^evidence-(?<run>[0-9]+)-[0-9]+\\.tar$").run // .) | select(. != $run)] | first // empty' <<< "$1"
}

# check_version_tag WHAT TAG REVISION -> the subject of the tag's message. ADR-0131 decisions 3
# and 7: an annotated tag on REVISION, verified by GitHub, by the maintainer's email.
check_version_tag() {
    local what="$1 $2" tag=$2 revision=$3 ref object type commit verified email
    [[ -n ${MAINTAINER_TAG_EMAIL:-} ]] ||
        die "$what names $revision and MAINTAINER_TAG_EMAIL is not set: no v0.x tag can be checked as the maintainer's (ADR-0131 decision 3)"
    ref=$(gh_or_absent api "/repos/$repo/git/ref/tags/$tag")
    [[ -n $ref ]] || die "$what no longer exists"
    type=$(jq -r '.object.type' <<< "$ref")
    [[ $type == tag ]] ||
        die "$what is a lightweight tag: a 0.x release is the maintainer's signed, annotated tag (ADR-0131 decision 7); see the recovery in docs/operations/github-settings.md section 9"
    object=$(gh_must api "/repos/$repo/git/tags/$(jq -r '.object.sha' <<< "$ref")")
    commit=$(jq -r 'if .object.type == "commit" then .object.sha else "" end' <<< "$object")
    [[ $commit == "$revision" ]] || die "$what names commit '$commit' (object.sha), not the revision $revision"
    verified=$(jq -r '.verification.verified' <<< "$object")
    [[ $verified == true ]] || die "$what is not verified by GitHub (verification.verified $verified, ADR-0131 decision 7)"
    email=$(jq -r '.tagger.email // empty' <<< "$object")
    [[ $email == "$MAINTAINER_TAG_EMAIL" ]] ||
        die "$what was tagged by '$email' (tagger.email), not MAINTAINER_TAG_EMAIL (ADR-0131 decision 7)"
    jq -r '.message | split("\n")[0]' <<< "$object"
}

check_promotion_tag() { # check_promotion_tag TAG REVISION: absent, or on REVISION
    local at
    at=$(tag_commit "$1")
    [[ -z $at || $at == "$2" ]] || die "$1 names commit $at, not the revision $2: a promotion tag never moves"
}

notes() { # notes DIR TAG OUT: the notes of the Release at TAG, from the promotion in DIR
    local dir=$1 tag=$2 out=$3 version file first heading
    : > "$out"
    if [[ $tag =~ $VERSION_TAG ]]; then
        version=${BASH_REMATCH[1]}
        file=$notes_dir/$version.md
        [[ -f $file ]] || die "$tag has no release notes: $file is missing"
        first=$(head -n 1 "$file")
        heading="^# (.*[^0-9.])?${version//./\\.}([^0-9.].*)?$"
        [[ $first =~ $heading ]] || die "the first line of $file, for $tag, is not a level-1 heading naming $version: '$first'"
        cat "$file" > "$out"
        echo >> "$out"
    fi
    bash "$root/system/evidence-bundle.sh" summary --promotion "$dir" "$tag" >> "$out"
}

on_branch() { # on_branch REVISION: REVISION is in the history of the release branch, as trusted-run.sh checks a run
    local head status
    # The branches endpoint knows branches only, and compare takes two commit ids.
    head=$(gh_must api "/repos/$repo/branches/$branch" --jq .commit.sha)
    [[ $head =~ ^[0-9a-f]{40}$ ]] || die "branch $branch has no commit id ('$head')"
    status=$(gh_must api "/repos/$repo/compare/$1...$head" --jq .status)
    [[ $status == identical || $status == ahead ]] ||
        die "the revision $1 is not on $branch ($status): a 0.x release is a signed tag on $branch (ADR-0105 item 2)"
}

field() { jq -er "$1 | select(. != null) | tostring" "$2" || die "$2 has no $1"; }

plan() {
    local dir=$1 run version revision refs lines name type sha tag title latest subject release other fallback=null
    local -a tags=()
    run=$(field .run_id "$dir/promotion.json")
    version=$(field .version "$dir/promotion.json")
    revision=$(field .revision "$dir/promotion.json")
    on_branch "$revision"

    # The v0.x tags at the revision, lightweight ones included: they fail below, never pass as "no tag".
    refs=$(gh_must api "/repos/$repo/git/matching-refs/tags/v")
    lines=$(jq -r '.[] | "\(.ref | ltrimstr("refs/tags/")) \(.object.type) \(.object.sha)"' <<< "$refs")
    while read -r name type sha; do
        [[ $name =~ $VERSION_TAG ]] || continue
        if [[ $type == tag ]]; then sha=$(gh_must api "/repos/$repo/git/tags/$sha" | jq -r '.object.sha'); fi
        if [[ $sha == "$revision" ]]; then tags+=("$name"); fi
    done <<< "$lines"
    ((${#tags[@]} <= 1)) || die "more than one v0.x tag names $revision: ${tags[*]}"

    tag=promoted-$run title=$version latest=false
    if ((${#tags[@]} == 1)); then
        subject=$(check_version_tag "the v0.x tag" "${tags[0]}" "$revision")
        # Published Releases only: the plan's token cannot be relied on to see drafts (decision 7 of the plan).
        release=$(release_view "${tags[0]}")
        other=''
        if [[ -n $release && $(jq -r '.isDraft' <<< "$release") == false ]]; then other=$(other_build "$release" "$run"); fi
        if [[ -n $other ]]; then
            echo "${0##*/}: the Release of ${tags[0]} holds the evidence of build run $other: build run $run gets its own, promoted-$run" >&2
        else
            tag=${tags[0]} title="$subject ($version)" latest=true
            check_promotion_tag "promoted-$run" "$revision"
            notes "$dir" "promoted-$run" "$work/fallback.md"
            fallback=$(jq -n --arg tag "promoted-$run" --arg title "$version" --arg sha "$(sha256sum < "$work/fallback.md" | cut -d' ' -f1)" \
                '{tag: $tag, title: $title, latest: false, notes_sha256: $sha}')
        fi
    fi
    if [[ $tag != v* ]]; then check_promotion_tag "$tag" "$revision"; fi
    notes "$dir" "$tag" "$work/notes.md"
    jq -n -S --indent 2 --arg tag "$tag" --arg title "$title" --argjson latest "$latest" \
        --arg sha "$(sha256sum < "$work/notes.md" | cut -d' ' -f1)" --argjson fallback "$fallback" \
        '{tag: $tag, title: $title, latest: $latest, notes_sha256: $sha} + (if $fallback == null then {} else {fallback: $fallback} end)' \
        > "$work/release.json"
    mv "$work/release.json" "$dir/release.json"
    cat "$dir/release.json" >&2
}

pack() { # pack DIR OUT: one tar with the same bytes for the same files, whatever their modes and times
    tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --mode='u=rwX,go=rX' --format=gnu -cf "$2" -C "$1" .
}

asset_sha() { # asset_sha TAG NAME -> the SHA-256 of the asset NAME already on the Release at TAG
    local into
    into=$(mktemp -d -p "$work")
    gh_must release download "$1" -R "$repo" -p "$2" -D "$into" > /dev/null
    sha256sum < "$into/$2" | cut -d' ' -f1
}

body_sha() { # body_sha < TEXT -> the SHA-256 of TEXT without carriage returns and with one final newline
    local text
    text=$(tr -d '\r')
    printf '%s\n' "$text" | sha256sum | cut -d' ' -f1
}

earlier_promotion() { # earlier_promotion RELEASE NAME -> an evidence asset of the Release that is not NAME
    jq -r --arg name "$2" '[.assets[].name | select(startswith("evidence-") and . != $name)] | first // empty' <<< "$1"
}

# differs RELEASE TAG TITLE LATEST NOTES REVISION -> what in the Release at TAG is not the approved
# Release; nothing when it is the approved one exactly.
differs() {
    local release=$1 tag=$2 title=$3 latest=$4 notes=$5 revision=$6 fields have want draft current latest_tag
    fields=$(jq -r --arg title "$title" --arg revision "$revision" '[
        if .name != $title then "title \(.name | @json)" else empty end,
        if .targetCommitish != $revision then "target \(.targetCommitish)" else empty end,
        if .isPrerelease then "prerelease" else empty end] | join(", ")' <<< "$release")
    have=$(jq -j '.body // ""' <<< "$release" | body_sha)
    want=$(body_sha < "$notes")
    if [[ $have != "$want" ]]; then fields+="${fields:+, }body (SHA-256 $have, approved $want)"; fi
    draft=$(jq -r '.isDraft' <<< "$release")
    if [[ $draft == false && $tag =~ $VERSION_TAG && $latest == true ]]; then
        current=$(gh_or_absent api "/repos/$repo/releases/latest")
        latest_tag=$(jq -r '.tag_name // empty' <<< "$current")
        if [[ $latest_tag != "$tag" ]]; then fields+="${fields:+, }latest (the latest Release is ${latest_tag:-none})"; fi
    fi
    echo "$fields"
}

# publish_state TAG TITLE LATEST NOTES ASSET SHA REVISION: the table of decision 7 of the plan.
publish_state() {
    local tag=$1 title=$2 latest=$3 notes=$4 asset=$5 sha=$6 revision=$7 release name have='' draft kind fields
    local flag=--latest=false recovery prior
    name=${asset##*/}
    if [[ $latest == true ]]; then flag=--latest; fi
    recovery="gh release delete $tag -R $repo (without --cleanup-tag), then re-run the github-release job: gh run rerun $GITHUB_RUN_ID --job <job id> -R $repo"
    release=$(release_view "$tag")
    if [[ -z $release ]]; then
        gh_must release create "$tag" "$asset" -R "$repo" --draft --target "$revision" --title "$title" --notes-file "$notes"
        gh_must release edit "$tag" -R "$repo" --draft=false "$flag"
        return
    fi
    draft=$(jq -r '.isDraft' <<< "$release")
    kind=published
    if [[ $draft == true ]]; then kind=draft; fi
    prior=$(earlier_promotion "$release" "$name")
    # A published Release with an earlier promotion's evidence gets this promotion's asset and
    # nothing else (ADR-0131 decision 6). Any other Release at the tag is this job's from an
    # interrupted run, or stray: anyone with contents: write can make one, so it must be the
    # approved Release exactly before the signed asset goes on it.
    if [[ $draft == true || -z $prior ]]; then
        fields=$(differs "$release" "$tag" "$title" "$latest" "$notes" "$revision")
        [[ -z $fields ]] ||
            die "the $kind Release at $tag is not the approved one ('$title' on $revision): $fields. Recovery, by the maintainer only: $recovery"
    fi
    if jq -e --arg name "$name" 'any(.assets[]; .name == $name)' <<< "$release" > /dev/null; then have=$(asset_sha "$tag" "$name"); fi
    if [[ -z $have ]]; then
        gh_must release upload "$tag" "$asset" -R "$repo"
    elif [[ $have != "$sha" && $draft == true ]]; then
        die "the draft Release at $tag already holds $name with SHA-256 $have, not this bundle's $sha. Recovery, by the maintainer only, if the draft holds only this run's asset (gh release view $tag -R $repo --json isDraft,assets): $recovery"
    elif [[ $have != "$sha" ]]; then
        die "the published Release at $tag already holds $name with SHA-256 $have, not this bundle's $sha: there is no automatic recovery, a published asset is never replaced; the bundle changed after publication, investigate"
    fi
    if [[ $draft == true ]]; then gh_must release edit "$tag" -R "$repo" --draft=false "$flag"; fi
}

# read_back TAG NAME SHA REVISION TITLE LATEST NOTES: the Release is published, is the approved
# one, holds the asset and its tag names REVISION.
read_back() {
    local release at fields prior
    release=$(gh_must release view "$1" -R "$repo" --json "$RELEASE_FIELDS")
    [[ $(jq -r '.isDraft' <<< "$release") == false ]] || die "the Release at $1 is still a draft after it was published"
    prior=$(earlier_promotion "$release" "$2")
    if [[ -n $prior ]]; then
        # An earlier promotion's notes name its own asset: only the title is this promotion's.
        fields=$(jq -r --arg title "$5" 'if .name != $title then "title \(.name | @json)" else "" end' <<< "$release")
    else
        fields=$(differs "$release" "$1" "$5" "$6" "$7" "$4")
    fi
    [[ -z $fields ]] || die "the Release at $1, as published, is not the approved one ('$5' on $4): $fields"
    jq -e --arg name "$2" 'any(.assets[]; .name == $name)' <<< "$release" > /dev/null || die "the Release at $1 does not hold $2"
    [[ $(asset_sha "$1" "$2") == "$3" ]] || die "the Release at $1 holds $2 with other bytes than this bundle's ($3)"
    at=$(tag_commit "$1")
    [[ $at == "$4" ]] || die "the tag $1 names ${at:-nothing}, not the revision $4"
    echo "GitHub Release: [$5]($(jq -r '.url' <<< "$release")), asset \`$2\` (SHA-256 \`$3\`)"
}

publish() {
    local dir=$1 run trigger revision tag title latest approved key='' other asset sha
    [[ -n ${RUN_ID:-} && -n ${GITHUB_RUN_ID:-} && -n ${GITHUB_SERVER_URL:-} ]] || usage
    # verify-dir proves the bundle is signed, not that it is this run's.
    run=$(field .run_id "$dir/promotion.json")
    [[ $run == "$RUN_ID" ]] || die "$dir is the bundle of build run $run, not $RUN_ID"
    trigger=$(field .trigger "$dir/promotion.json")
    [[ $trigger == "$GITHUB_SERVER_URL/$repo/actions/runs/$GITHUB_RUN_ID" ]] ||
        die "$dir was planned by $trigger, not by this promotion run $GITHUB_RUN_ID"
    revision=$(field .revision "$dir/promotion.json")

    tag=$(field .tag "$dir/release.json")
    if [[ $tag =~ $VERSION_TAG ]]; then
        # The tag is checked first: a bad v0.x tag fails closed even beside another build's Release.
        check_version_tag "release.json tag" "$tag" "$revision" > /dev/null
        other=$(other_build "$(release_view "$tag")" "$run")
        if [[ -n $other ]]; then
            key=.fallback
            tag=$(field .fallback.tag "$dir/release.json")
            echo "The Release of $(field .tag "$dir/release.json") holds the evidence of build run $other: build run $run is published at $tag, the fallback the release approval covered (ADR-0131 decision 6)."
        fi
    fi
    [[ $tag =~ $VERSION_TAG || $tag == "promoted-$run" ]] || die "release.json$key names the tag $tag, neither a v0.x tag nor promoted-$run"
    if [[ $tag != v* ]]; then check_promotion_tag "$tag" "$revision"; fi
    title=$(field "$key.title" "$dir/release.json")
    latest=$(field "$key.latest" "$dir/release.json")
    approved=$(field "$key.notes_sha256" "$dir/release.json")
    notes "$dir" "$tag" "$work/notes.md"
    sha=$(sha256sum < "$work/notes.md" | cut -d' ' -f1)
    [[ $sha == "$approved" ]] || die "the notes of $tag hash to $sha, not the approved release.json$key.notes_sha256 $approved"

    asset=$work/evidence-$run-$GITHUB_RUN_ID.tar
    pack "$dir" "$asset"
    sha=$(sha256sum < "$asset" | cut -d' ' -f1)
    publish_state "$tag" "$title" "$latest" "$work/notes.md" "$asset" "$sha" "$revision"
    read_back "$tag" "${asset##*/}" "$sha" "$revision" "$title" "$latest" "$work/notes.md"
}

case ${1:-} in
plan)
    [[ $# -eq 2 ]] || usage
    plan "$2"
    ;;
publish)
    [[ $# -eq 2 ]] || usage
    publish "$2"
    ;;
*) usage ;;
esac
