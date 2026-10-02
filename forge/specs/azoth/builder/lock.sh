#!/usr/bin/env bash
# The kernel builder's toolchain lock (docs/architecture/doc_kernel_build.md, section 5).
# toolchain.lock names, by sha256, every RPM the builder installs on top of its
# digest-pinned base: the tools below and the BuildRequires of the pinned kernel.spec
# with the build conditions of bconds.sh, plus the base packages they upgrade. The same
# RPMs at every build give the same kernel from the same pins (section 3, step 8).
#
#   generate [--force]  on the host: resolve the lock again in a bare container of the
#                       Containerfile base, unless check passes (--force: anyway, to move
#                       the toolchain forward)
#   check               the lock was resolved for this base, FEDORA_KERNEL_NVR, TOOLS and
#                       bconds.sh (its header), and inside the builder it is the lock the
#                       image was built from: build.sh runs it before the build
#   resolve             inside that container: what generate runs
#   install             in the Containerfile: fetch the locked RPMs from koji, which keeps
#                       every build, splice back the signature header koji keeps in
#                       data/sigcache, check sha256 and signature, install them with
#                       every repository disabled
set -euo pipefail

# Physical paths, as git rev-parse prints them (generate strips the repository prefix).
HERE=$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)
INSTALLED=/usr/local/share/azoth/toolchain.lock
LOCK=$HERE/toolchain.lock
KOJI=https://kojipkgs.fedoraproject.org
die() {
    echo "lock.sh: $*" >&2
    exit 1
}

# The tools of the build itself; kernel.spec brings the rest through its BuildRequires.
# LLVM toolchain (clang, lld), RPM tools and the dnf5 builddep plugin, rust and bindgen
# for CONFIG_RUST, dwarves (pahole) for the BTF, gnupg2 for the source signatures,
# python3 for Fedora's merge.py, python3-koji to restitch the SRPM signature.
TOOLS=(
    rpm-build dnf5-plugins python3-koji
    clang lld llvm
    gcc gcc-c++ make flex bison bc patch diffutils findutils
    ncurses-devel elfutils-libelf-devel elfutils-devel openssl-devel openssl dwarves
    rust cargo rustfmt bindgen-cli
    tar cpio xz curl git gnupg2 python3
)

header() { # header: the lines that tie the lock to its inputs
    local base inputs
    base=$(sed -n 's/^FROM //p' "$HERE/Containerfile")
    inputs=$({ printf '%s\n' "${TOOLS[@]}"; cat "$HERE/../bconds.sh"; } | sha256sum | cut -d' ' -f1)
    # shellcheck source=../pins.env
    source "$HERE/../pins.env"
    printf '# base: %s\n# kernel: %s\n# inputs: %s\n' "$base" "$FEDORA_KERNEL_NVR" "$inputs"
}

current() { # current: the lock header matches its inputs
    [[ -f $LOCK && $(grep -E '^# (base|kernel|inputs): ' "$LOCK") == "$(header)" ]]
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
    [[ ${2:-} == --force ]] || ! current ||
        {
            echo "toolchain.lock is current"
            exit 0
        }
    repo=$(git -C "$HERE" rev-parse --show-toplevel)
    podman run --rm --pull=newer --security-opt label=disable \
        -v "$repo:/forge" -w /forge "$(sed -n 's/^FROM //p' "$HERE/Containerfile")" \
        bash "/forge/${HERE#"$repo"/}/lock.sh" resolve
    ;;

resolve)
    # shellcheck source=../pins.env
    source "$HERE/../pins.env"
    # shellcheck source=../bconds.sh
    source "$HERE/../bconds.sh"
    work=$(mktemp -d)
    printf 'install_weak_deps=False\nkeepcache=True\n' >> /etc/dnf/dnf.conf
    rpm -qa --qf '%{NEVRA}\n' | sort > "$work/before"
    dnf -y install "${TOOLS[@]}"
    fetch "$work" "${FEDORA_KEY_FPR: -8}" "kernel-$FEDORA_KERNEL_NVR.src.rpm:kernel/${FEDORA_KERNEL_NVR%%-*}/${FEDORA_KERNEL_NVR#*-}"
    checksig "$(release_key "${FEDORA_KERNEL_NVR##*.fc}")" "$FEDORA_KEY_FPR" "$work/kernel-$FEDORA_KERNEL_NVR.src.rpm"
    rpm -i --define "_topdir $work/top" "$work/kernel-$FEDORA_KERNEL_NVR.src.rpm"
    dnf -y builddep "${DEFINES[@]}" "$work/top/SPECS/kernel.spec"
    rpm -qa --qf '%{NEVRA}\n' | grep -v '^gpg-pubkey-' | sort > "$work/after"
    # The toolchain comes from the base's release, signed by its key, whatever release
    # the pinned SRPM is from.
    mapfile -t fprs < <(gpg --show-keys --with-colons "$(release_key "$(os_release)")" | sed -n 's/^fpr:*\([0-9A-F]\{40\}\):$/\1/p')
    {
        header
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
    } > "$LOCK.new"
    mv "$LOCK.new" "$LOCK"
    echo "toolchain.lock: $(grep -vc '^#' "$LOCK") RPMs"
    ;;

install)
    fpr=$(sed -n 's/^# key: //p' "$LOCK")
    [[ $fpr =~ ^[0-9a-f]{40}$ ]] || die "toolchain.lock without a key fingerprint"
    mapfile -t entries < <(grep -v '^#' "$LOCK" | while read -r _ file nvr; do echo "$file:$nvr"; done)
    work=$(mktemp -d)
    fetch "$work" "${fpr: -8}" "${entries[@]}"
    grep -v '^#' "$LOCK" | while read -r sum file _; do echo "$sum  $work/$file"; done |
        sha256sum --check --quiet --strict || die "an RPM does not match toolchain.lock"
    checksig "$(release_key "$(os_release)")" "$fpr" "$work"/*.rpm
    dnf -y install --disablerepo='*' "$work"/*.rpm
    rm -rf "$work"
    dnf clean all
    ;;

check)
    current || die "toolchain.lock does not match the builder base, FEDORA_KERNEL_NVR, TOOLS or bconds.sh: run builder/lock.sh generate"
    [[ ! -f $INSTALLED ]] || cmp -s "$INSTALLED" "$LOCK" ||
        die "the builder image was built from another toolchain.lock: rebuild it"
    ;;

*) die "usage: ${0##*/} generate [--force] | check | resolve | install" ;;
esac
