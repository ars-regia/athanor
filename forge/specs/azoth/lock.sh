#!/usr/bin/env bash
# The package locks of the kernel's build environments (docs/architecture/doc_kernel_build.md,
# section 5). Each ENV/NAME.packages (builder/toolchain, boot/toolchain, boot/schbench,
# nvidia/toolchain) lists what a stage of ENV/Containerfile needs; ENV/NAME.lock names, by
# sha256, every RPM that stage installs on top of the digest-pinned base: those packages,
# their dependencies and the base packages they upgrade. builder/toolchain also holds the
# BuildRequires of the pinned kernel.spec with the build conditions of bconds.sh. The same
# RPMs at every build give the same kernel from the same pins (section 3, step 8).
#
#   generate [--force] [ENV/NAME...]  on the host: resolve each lock (default: all) again
#                       in a bare container of its base, unless its header already
#                       matches its inputs (--force: anyway, to move the packages forward)
#   check [ENV]         every lock header matches its inputs (base, packages, and for the
#                       builder FEDORA_KERNEL_NVR and bconds.sh); with ENV, inside its image,
#                       the image was built from ENV/toolchain.lock: build.sh runs it
#   resolve ENV/NAME    inside that container: what generate runs
#   install LOCK        in a Containerfile: fetch the locked RPMs from koji, which keeps
#                       every build, splice back the signature header koji keeps in
#                       data/sigcache, check sha256 and signature, install them with
#                       every repository disabled
set -euo pipefail

# Physical paths, as git rev-parse prints them (generate strips the repository prefix).
HERE=$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)
INSTALLED=/usr/local/share/azoth/toolchain.lock
SPEC_LOCK=builder/toolchain
KOJI=https://kojipkgs.fedoraproject.org
die() {
    echo "lock.sh: $*" >&2
    exit 1
}

names() { # names: every ENV/NAME with a .packages list
    local p
    for p in "$HERE"/*/*.packages; do
        p=${p#"$HERE"/}
        echo "${p%.packages}"
    done
}

packages() { # packages ENV/NAME: its package list, one per line, without comments
    sed 's/#.*//' "$HERE/$1.packages" | tr -s ' \t' '\n' | sed '/^$/d'
}

base() { # base ENV/NAME: the one image every stage of ENV/Containerfile starts from
    local refs
    refs=$(sed -n 's/^FROM \([^ ]*\).*/\1/p' "$HERE/${1%/*}/Containerfile" | sort -u)
    [[ $refs =~ ^[^[:space:]]+@sha256:[0-9a-f]{64}$ ]] || die "${1%/*}/Containerfile: not one digest-pinned base"
    echo "$refs"
}

header() { # header ENV/NAME: the lines that tie the lock to its inputs
    local inputs
    if [[ $1 == "$SPEC_LOCK" ]]; then
        inputs=$({ packages "$1"; cat "$HERE/bconds.sh"; } | sha256sum | cut -d' ' -f1)
        # shellcheck source=pins.env
        source "$HERE/pins.env"
        printf '# base: %s\n# kernel: %s\n# inputs: %s\n' "$(base "$1")" "$FEDORA_KERNEL_NVR" "$inputs"
    else
        inputs=$(packages "$1" | sha256sum | cut -d' ' -f1)
        printf '# base: %s\n# inputs: %s\n' "$(base "$1")" "$inputs"
    fi
}

current() { # current ENV/NAME: the lock header matches its inputs
    [[ -f $HERE/$1.lock && $(grep -E '^# (base|kernel|inputs): ' "$HERE/$1.lock") == "$(header "$1")" ]]
}

release_key() { # release_key RELEASE: the primary key file of a Fedora release in the base
    echo "/etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$1-primary"
}

os_release() { (. /etc/os-release && echo "$VERSION_ID"); }

splice() { # splice SIG RPM OUT: koji.splice_rpm_sighdr in coreutils
    # The signature header follows the 96-byte lead: a 16-byte preamble whose last two
    # big-endian words count the index entries and the data bytes, padded to 8 bytes.
    local il dl size
    read -r il dl < <(od -An -tu4 --endian=big -j 104 -N 8 "$2")
    size=$(((16 + 16 * il + dl + 7) / 8 * 8))
    {
        head -c 96 "$2"
        cat "$1"
        tail -c +$((96 + size + 1)) "$2"
    } > "$3"
}

fetch() { # fetch DIR KEYID ENTRY...: signed RPMs into DIR, ENTRY = FILE:NAME/VERSION/RELEASE
    local dir=$1 keyid=$2 entry file nvr arch
    shift 2
    for entry in "$@"; do
        file=${entry%%:*} nvr=${entry#*:}
        arch=${file%.rpm}
        arch=${arch##*.}
        printf 'url = "%s"\noutput = "%s"\n' \
            "$KOJI/packages/$nvr/$arch/$file" "$dir/$file.unsigned" \
            "$KOJI/packages/$nvr/data/sigcache/$keyid/$arch/$file.sig" "$dir/$file.sig"
    done > "$dir/curl.conf"
    curl --fail --silent --show-error --retry 3 --parallel --parallel-max 8 --config "$dir/curl.conf"
    for entry in "$@"; do
        file=${entry%%:*}
        splice "$dir/$file.sig" "$dir/$file.unsigned" "$dir/$file"
        rm "$dir/$file.sig" "$dir/$file.unsigned"
    done
}

checksig() { # checksig KEYFILE FPR RPM...: every RPM signed by the Fedora key FPR of KEYFILE
    local fpr=$2 rpm
    rpmkeys --import "$1"
    shift 2
    for rpm in "$@"; do
        rpmkeys --checksig --verbose "$rpm" | grep "signature, key fingerprint: $fpr: OK" > /dev/null ||
            die "not signed by the Fedora key $fpr: ${rpm##*/}"
    done
}

case ${1:-} in
generate)
    shift
    force=false
    if [[ ${1:-} == --force ]]; then
        force=true
        shift
    fi
    if (($# == 0)); then
        mapfile -t all < <(names)
        set -- "${all[@]}"
    fi
    repo=$(git -C "$HERE" rev-parse --show-toplevel)
    for name in "$@"; do
        [[ -f $HERE/$name.packages ]] || die "no $name.packages"
        if [[ $force == false ]] && current "$name"; then
            echo "$name.lock is current"
            continue
        fi
        podman run --rm --pull=newer --security-opt label=disable \
            -v "$repo:/forge" -w /forge "$(base "$name")" \
            bash "/forge/${HERE#"$repo"/}/lock.sh" resolve "$name"
    done
    ;;

resolve)
    name=${2:?ENV/NAME}
    lock=$HERE/$name.lock
    mapfile -t packages < <(packages "$name")
    work=$(mktemp -d)
    printf 'install_weak_deps=False\nkeepcache=True\n' >> /etc/dnf/dnf.conf
    rpm -qa --qf '%{NEVRA}\n' | sort > "$work/before"
    dnf -y install "${packages[@]}"
    if [[ $name == "$SPEC_LOCK" ]]; then
        # shellcheck source=pins.env
        source "$HERE/pins.env"
        # shellcheck source=bconds.sh
        source "$HERE/bconds.sh"
        fetch "$work" "${FEDORA_KEY_FPR: -8}" "kernel-$FEDORA_KERNEL_NVR.src.rpm:kernel/${FEDORA_KERNEL_NVR%%-*}/${FEDORA_KERNEL_NVR#*-}"
        checksig "$(release_key "${FEDORA_KERNEL_NVR##*.fc}")" "$FEDORA_KEY_FPR" "$work/kernel-$FEDORA_KERNEL_NVR.src.rpm"
        rpm -i --define "_topdir $work/top" "$work/kernel-$FEDORA_KERNEL_NVR.src.rpm"
        dnf -y builddep "${DEFINES[@]}" "$work/top/SPECS/kernel.spec"
    fi
    rpm -qa --qf '%{NEVRA}\n' | grep -v '^gpg-pubkey-' | sort > "$work/after"
    # The packages come from the base's release, signed by its key, whatever release the
    # pinned SRPM is from.
    mapfile -t fprs < <(gpg --show-keys --with-colons "$(release_key "$(os_release)")" | sed -n 's/^fpr:*\([0-9A-F]\{40\}\):$/\1/p')
    {
        header "$name"
        echo "# key: ${fprs[0],,}"
        comm -13 "$work/before" "$work/after" | while read -r nevra; do
            file=$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}.rpm' "$nevra")
            src=$(rpm -q --qf '%{SOURCERPM}' "$nevra")
            src=${src%.src.rpm} # name-version-release
            rel=${src##*-} src=${src%-*}
            cached=$(find /var/cache/libdnf5 -name "$file" -print -quit)
            [[ -n $cached ]] || die "not in the dnf cache: $file"
            printf '%s  %s  %s/%s/%s\n' "$(sha256sum < "$cached" | cut -d' ' -f1)" "$file" \
                "${src%-*}" "${src##*-}" "$rel"
        done
    } > "$lock.new"
    mv "$lock.new" "$lock"
    echo "$name.lock: $(grep -vc '^#' "$lock") RPMs"
    ;;

install)
    lock=${2:?LOCK}
    fpr=$(sed -n 's/^# key: //p' "$lock")
    [[ $fpr =~ ^[0-9a-f]{40}$ ]] || die "$lock without a key fingerprint"
    mapfile -t entries < <(grep -v '^#' "$lock" | while read -r _ file nvr; do echo "$file:$nvr"; done)
    work=$(mktemp -d)
    fetch "$work" "${fpr: -8}" "${entries[@]}"
    grep -v '^#' "$lock" | while read -r sum file _; do echo "$sum  $work/$file"; done |
        sha256sum --check --quiet --strict || die "an RPM does not match $lock"
    checksig "$(release_key "$(os_release)")" "$fpr" "$work"/*.rpm
    dnf -y install --disablerepo='*' "$work"/*.rpm
    rm -rf "$work"
    dnf clean all
    ;;

check)
    while read -r name; do
        current "$name" || die "$name.lock does not match its base, $name.packages, FEDORA_KERNEL_NVR or bconds.sh: run lock.sh generate"
    done < <(names)
    [[ -z ${2:-} || ! -f $INSTALLED ]] || [[ $(<"$INSTALLED") == "$(<"$HERE/$2/toolchain.lock")" ]] ||
        die "the $2 image was built from another toolchain.lock: rebuild it"
    ;;

*) die "usage: ${0##*/} generate [--force] [ENV/NAME...] | check [ENV] | resolve ENV/NAME | install LOCK" ;;
esac
