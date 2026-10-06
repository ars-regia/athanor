#!/usr/bin/env bash
# The DNF tier repositories of the forge: aggregated by an unprivileged build job, signed and
# published by a sign-only job, so that RPM_GPG_KEY never reaches a job that runs build code
# (docs/architecture/doc_kernel_profile.md D43, maintainer decision A2-33). call-system-image.yml
# runs `fetch` in build-repo, which hands forge/repo-cache to sign-repo as an artifact; sign-repo
# runs the other commands.
#
# Each command runs on the runner. Those that need the forge tools start the builder image
# ghcr.io/<owner>/athanor-builder:$BUILDER_CONTENT_HASH with the checkout at /workspace and work
# in forge/ inside it; the caller has logged podman in to the registry.
#
# Usage: tier_repos.sh COMMAND
#   fetch         pull the per-package images into repo-cache/repo-tier[0-3] and the rolling
#                 aggregate repo-cache/repo (fetch_repo_rpms.sh), unsigned, and record the
#                 content hash of each in repo-cache/content.sha256
#   pages-tree    copy the four tier repositories to repo-cache/pages/tier[0-3] without the RPMs
#                 GitHub Pages refuses, for deploy-pages
#   sign          with RPM_GPG_KEY (and RPM_GPG_PASSPHRASE if the key has one): sign every RPM,
#                 write the repository metadata and sign repodata/repomd.xml, in each repository
#                 above, and export the public key to repo-cache/RPM-GPG-KEY-athanor. Without the
#                 key the metadata is written unsigned, with a warning.
#   publish       push each repository as athanor-forge-<tier>-repo:latest (the rolling aggregate
#                 as athanor-forge-rolling-repo:latest) when its content hash changed
#   deploy-pages  publish repo-cache/pages, the public key and the iPXE recovery files on the
#                 gh-pages branch
#
# The key leaves the environment before the first child process starts. It reaches a file only
# through the shell's own printf, under umask 077, in a private directory on tmpfs that is
# mounted read-only into a container with the network off, and removed when the script exits.
# Inside, it is imported into a GNUPGHOME created for the run and removed with it.
set -euo pipefail

die() {
    echo "${0##*/}: $*" >&2
    exit 1
}

REPOS=(repo-cache/repo-tier0 repo-cache/repo-tier1 repo-cache/repo-tier2 repo-cache/repo-tier3 repo-cache/repo)

# ---------------------------------------------------------------------------------------------
# Inside the builder image, in /workspace/forge.
# ---------------------------------------------------------------------------------------------

# Container storage on the workspace mount, i.e. the runner's disk, addressed by path: no bind
# mount, which the builder could not perform anyway. Then the registry login for buildah.
containers_storage() {
    export BUILDAH_ISOLATION=chroot XDG_DATA_HOME=/var/tmp XDG_CONFIG_HOME=/var/tmp \
        XDG_CACHE_HOME=/var/tmp _CONTAINERS_USERNS_CONFIGURED="done"
    mkdir -p /workspace/var-tmp/containers
    printf '%s\n' '[storage]' 'driver = "vfs"' \
        'runroot = "/workspace/var-tmp/containers/runroot"' \
        'graphroot = "/workspace/var-tmp/containers/storage"' \
        '[storage.options.vfs]' 'ignore_chown_errors = "true"' > /var/tmp/storage.conf
    export CONTAINERS_STORAGE_CONF=/var/tmp/storage.conf
    printf '%s\n' "$GITHUB_TOKEN" | buildah login ghcr.io -u "$GITHUB_ACTOR" --password-stdin
}

# The hash the published image carries in its tier.content.sha256 label, over the unsigned
# RPMs: computed as it was before the signature moved out of the build job, so that an
# unchanged repository still matches the image already published.
content_hash() {
    find "$1" -name '*.rpm' -type f | sort | xargs -r sha256sum | sha256sum | awk '{print $1}'
}

inside_fetch() {
    containers_storage
    bash scripts/fetch_repo_rpms.sh "$GITHUB_REPOSITORY_OWNER"
    local dir
    for dir in "${REPOS[@]}"; do
        printf '%s %s\n' "$(content_hash "$dir")" "$dir"
    done > repo-cache/content.sha256
}

# Runs with the network off. /run/sign/key, when mounted, holds RPM_GPG_KEY and
# /run/sign/passphrase its passphrase.
inside_sign() {
    local dirs=("${REPOS[@]}") dir
    for dir in repo-cache/pages/tier{0..3}; do
        if [[ -d $dir ]]; then dirs+=("$dir"); fi
    done
    if [[ ! -f /run/sign/key ]]; then
        for dir in "${dirs[@]}"; do createrepo_c "$dir"; done
        return
    fi

    GNUPGHOME=$(mktemp -d)
    export GNUPGHOME
    trap 'gpgconf --kill gpg-agent; rm -rf "$GNUPGHOME"' EXIT
    local unlock=(--batch --pinentry-mode loopback)
    if [[ -f /run/sign/passphrase ]]; then unlock+=(--passphrase-file /run/sign/passphrase); fi
    gpg "${unlock[@]}" --import /run/sign/key
    local primaries
    mapfile -t primaries < <(gpg --batch --list-secret-keys --with-colons |
        awk -F: '$1 == "sec" { primary = 1; next } primary && $1 == "fpr" { print $10; primary = 0 }')
    [[ ${#primaries[@]} -eq 1 ]] || die "RPM_GPG_KEY must hold exactly one secret key, it holds ${#primaries[@]}"
    local fpr=${primaries[0]}
    gpg --batch --armor --export "$fpr" > repo-cache/RPM-GPG-KEY-athanor

    # rpmkeys checks the signatures against the exported public key alone, in a database of
    # its own, and fails a package that carries no signature (_pkgverify_level).
    local rpmdb
    rpmdb=$(mktemp -d)
    rpmkeys --define "_dbpath $rpmdb" --import repo-cache/RPM-GPG-KEY-athanor
    for dir in "${dirs[@]}"; do
        find "$dir" -name '*.rpm' -type f -exec rpmsign --define "_gpg_path $GNUPGHOME" \
            --define "_gpg_name $fpr" --define "_gpg_sign_cmd_extra_args ${unlock[*]}" \
            --addsign {} +
        find "$dir" -name '*.rpm' -type f -exec rpmkeys --define "_dbpath $rpmdb" \
            --define "_pkgverify_level signature" --checksig {} +
        createrepo_c "$dir"
        gpg "${unlock[@]}" --yes --local-user "$fpr" --armor --detach-sign "$dir/repodata/repomd.xml"
        gpg --batch --verify "$dir/repodata/repomd.xml.asc" "$dir/repodata/repomd.xml"
    done
    rm -rf "$rpmdb"
    echo "${0##*/}: ${#dirs[@]} repositories signed with $fpr"
}

publish_one() {
    local dir=$1 image=$2 hash=$3 ctr
    ctr=$(buildah from scratch)
    (cd "$dir" && buildah copy "$ctr" . /)
    buildah config --label tier.content.sha256="$hash" "$ctr"
    buildah commit --omit-timestamp "$ctr" "$image"
    buildah push "$image"
    buildah rm "$ctr"
    buildah rmi "$image"
}

inside_publish() {
    containers_storage
    local hash dir tier image config published pids=() pid
    while read -r hash dir; do
        tier=${dir#repo-cache/repo}
        tier=${tier#-}
        image="ghcr.io/${GITHUB_REPOSITORY_OWNER,,}/athanor-forge-${tier:-rolling}-repo:latest"
        published=''
        # An image that does not exist yet has no label: it is pushed.
        if config=$(skopeo inspect --config "docker://$image" 2> /dev/null); then
            published=$(jq -r '.config.Labels["tier.content.sha256"] // ""' <<< "$config")
        fi
        if [[ $hash == "$published" ]]; then
            echo "${image##*/}: RPM content unchanged, not pushed"
            continue
        fi
        publish_one "$dir" "$image" "$hash" &
        pids+=($!)
    done < repo-cache/content.sha256
    for pid in "${pids[@]}"; do wait "$pid"; done
}

inside_deploy_pages() {
    local url="https://${GITHUB_ACTOR}:${GITHUB_TOKEN}@github.com/${GITHUB_REPOSITORY}.git"
    local pages=/workspace/pages_repo tier
    if git ls-remote --exit-code --heads "$url" gh-pages > /dev/null; then
        git clone --depth 1 -b gh-pages "$url" "$pages"
    else
        git init -b gh-pages "$pages"
    fi
    git config --global --add safe.directory "$pages"
    git -C "$pages" config user.name "Athanor Forge"
    git -C "$pages" config user.email "forge@athanor.os"

    for tier in tier0 tier1 tier2 tier3; do
        rm -rf "${pages:?}/$tier"
        cp -a "repo-cache/pages/$tier" "$pages/$tier"
    done
    if [[ -f repo-cache/RPM-GPG-KEY-athanor ]]; then
        cp repo-cache/RPM-GPG-KEY-athanor "$pages/"
    fi
    mkdir -p "$pages/recovery"
    cp /usr/share/ipxe/ipxe-x86_64.efi "$pages/recovery/ipxe.efi"
    printf '%s\n' '#!ipxe' 'dhcp' \
        "chain https://${GITHUB_REPOSITORY_OWNER}.github.io/${GITHUB_REPOSITORY#*/}/recovery/menu.ipxe || shell" \
        > "$pages/recovery/boot.ipxe"

    git -C "$pages" add -A
    if git -C "$pages" rev-parse --verify --quiet HEAD > /dev/null; then
        git -C "$pages" commit --amend -m "Deploy the DNF and Flatpak channels of Athanor OS"
    else
        git -C "$pages" commit -m "Deploy the DNF and Flatpak channels of Athanor OS"
    fi
    git -C "$pages" push -f "$url" gh-pages
}

if [[ ${1:-} == --inside ]]; then
    case ${2:-} in
    fetch) inside_fetch ;;
    sign) inside_sign ;;
    publish) inside_publish ;;
    deploy-pages) inside_deploy_pages ;;
    *) die "unknown command inside the builder: ${2:-}" ;;
    esac
    exit 0
fi

# ---------------------------------------------------------------------------------------------
# On the runner.
# ---------------------------------------------------------------------------------------------

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

builder() {
    echo "ghcr.io/${GITHUB_REPOSITORY_OWNER,,}/athanor-builder:${BUILDER_CONTENT_HASH:?BUILDER_CONTENT_HASH is not set}"
}

# in_builder PODMAN_OPTION... -- COMMAND
in_builder() {
    local options=() image
    image=$(builder)
    while [[ $1 != -- ]]; do
        options+=("$1")
        shift
    done
    # SELinux labels are left alone (label=disable) rather than relabelled (:z): relabelling
    # would rewrite the context of the checkout on the host.
    podman run --rm --security-opt label=disable --security-opt seccomp=unconfined \
        -v "$root:/workspace" -w /workspace/forge "${options[@]}" \
        "$image" bash scripts/tier_repos.sh --inside "$2"
}

# The registry credentials and the repository names reach the container through --env NAME,
# which copies the value from this environment and keeps it off the command line.
registry_env=(--env GITHUB_TOKEN --env GITHUB_ACTOR --env GITHUB_REPOSITORY_OWNER --env GITHUB_REPOSITORY)

sign() {
    local key=${RPM_GPG_KEY:-} passphrase=${RPM_GPG_PASSPHRASE:-} keys options=(--network=none)
    unset RPM_GPG_KEY RPM_GPG_PASSPHRASE
    keys=$(umask 077 && mktemp -d -p "${XDG_RUNTIME_DIR:-/dev/shm}" sign-repos.XXXXXX)
    trap 'rm -rf "$keys"' EXIT
    if [[ -n $key ]]; then
        (umask 077 && printf '%s\n' "$key" > "$keys/key")
        if [[ -n $passphrase ]]; then
            (umask 077 && printf '%s\n' "$passphrase" > "$keys/passphrase")
        fi
        options+=(-v "$keys:/run/sign:ro")
    else
        echo "::warning::RPM_GPG_KEY is not set in the signing environment: the tier repositories are published unsigned"
    fi
    key='' passphrase=''
    bash "$root/forge/scripts/retry.sh" podman pull "$(builder)"
    in_builder "${options[@]}" -- sign
}

pages_tree() {
    local tier pages=$root/forge/repo-cache/pages
    rm -rf "$pages"
    mkdir -p "$pages"
    for tier in tier0 tier1 tier2 tier3; do
        cp -a "$root/forge/repo-cache/repo-$tier" "$pages/$tier"
    done
    # GitHub Pages refuses files above 100 MB; keep the channel well below.
    find "$pages" -type f -name '*.rpm' -size +49M -delete
}

case ${1:-} in
fetch | publish) in_builder --privileged "${registry_env[@]}" -- "$1" ;;
deploy-pages) in_builder "${registry_env[@]}" -- deploy-pages ;;
sign) sign ;;
pages-tree) pages_tree ;;
*)
    echo "usage: ${0##*/} fetch|pages-tree|sign|publish|deploy-pages" >&2
    exit 2
    ;;
esac
