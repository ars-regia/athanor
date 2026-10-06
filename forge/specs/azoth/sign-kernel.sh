#!/usr/bin/env bash
# The signatures of the kernel artefacts (docs/architecture/doc_ci.md, D43;
# docs/architecture/doc_kernel_profile.md, key table): vmlinuz for Secure Boot, the NVIDIA
# modules for the module keyring. Runs in the signer/Containerfile image, pulled by digest:
# sign-file comes from Fedora's kernel-devel and sbsign from sbsigntools, both installed from
# signer/toolchain.lock, so nothing built by this project runs next to a key. The image carries
# this script at /usr/local/bin/sign-kernel.sh, so its digest pins it too. signer/run.sh calls
# `prepare` and `check-modules` in the key-less steps, `modules` and `vmlinuz` in the step that
# holds the keys, and `verify` in the publish job.
#
# Usage: sign-kernel.sh prepare --kernel DIR --devel DIR --out DIR
#        sign-kernel.sh check-modules --kver KVER --dir DIR
#        sign-kernel.sh modules --key FILE --cert FILE --hash FILE --kver KVER --dir DIR
#        sign-kernel.sh vmlinuz --key FILE --cert FILE --in DIR --out DIR
#        sign-kernel.sh verify  --cert FILE --dir DIR --kernel DIR
#   prepare        key-less: from the kernel-core RPM in --kernel, OUT/vmlinuz and OUT/kver;
#                  from the kernel-devel RPM in --devel, OUT/module-sig-hash
#                  (CONFIG_MODULE_SIG_HASH)
#   check-modules  DIR holds only what nvidia.sh build writes for kernel KVER: per branch (open,
#                  legacy) BRANCH/kver equal to KVER, BRANCH/version, the build log next to
#                  BRANCH, and BRANCH/lib/modules/KVER/extra/nvidia/ with nvidia.ko and only
#                  the nvidia-{drm,modeset,uvm,peermem}.ko beside it, each a regular file whose
#                  vermagic is of KVER. Anything else, a symlink included, fails
#   modules        check-modules, then sign every module with the hash named in --hash, in
#                  place, and check with modinfo that the signer is the CN of --cert
#   vmlinuz        remove every signature of IN/vmlinuz (the kernel build leaves Fedora's test
#                  certificate on it), sign it with sbsign into OUT/vmlinuz, copy IN/kver, verify
#                  that OUT/vmlinuz without its signature is IN/vmlinuz without its own
#   verify         DIR/vmlinuz carries exactly one signature, it verifies against --cert (PEM),
#                  and without it DIR/vmlinuz is the vmlinuz of the kernel-core RPM in --kernel,
#                  without the signature that RPM carries; DIR/kver names that RPM's kernel
# A key is a file the caller wrote with mode 0600; this script never prints it.
set -euo pipefail
shopt -s inherit_errexit nullglob

die() {
    echo "sign-kernel: $*" >&2
    exit 1
}
usage() {
    sed -n '/^# Usage:/,/^# A key/{/^# A key/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}" >&2
    exit 2
}
# The hashes CONFIG_MODULE_SIG_HASH can name (kernel/module/Kconfig).
HASHES='sha256 sha384 sha512 sha3-256 sha3-384 sha3-512'

STAGE=${1:-}
[[ $# -gt 0 ]] && shift
KERNEL='' DEVEL='' OUT='' KEY='' CERT='' HASH='' DIR='' IN='' KVER=''
while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
    --kernel) KERNEL=$2 ;;
    --devel) DEVEL=$2 ;;
    --out) OUT=$2 ;;
    --key) KEY=$2 ;;
    --cert) CERT=$2 ;;
    --hash) HASH=$2 ;;
    --dir) DIR=$2 ;;
    --in) IN=$2 ;;
    --kver) KVER=$2 ;;
    *) usage ;;
    esac
    shift 2
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

one() { # one GLOB: the single path the glob names
    local -a found
    # shellcheck disable=SC2206 # the argument is a glob on purpose
    found=($1)
    [[ ${#found[@]} -eq 1 ]] || die "expected exactly one $1, found ${#found[@]}"
    echo "${found[0]}"
}

cert_cn() { # cert_cn FILE: the CN of the certificate, PEM or DER
    local subject
    subject=$(openssl x509 -in "$1" -noout -subject -nameopt RFC2253 2> /dev/null ||
        openssl x509 -in "$1" -inform DER -noout -subject -nameopt RFC2253)
    subject=${subject#subject=}
    subject=${subject#CN=}
    echo "${subject%%,*}"
}

signatures() { # signatures FILE: how many signatures the PE image carries
    local list
    list=$(sbverify --list "$1") || die "sbverify --list $1 failed"
    grep -c '^signature [0-9]' <<< "$list" || [[ $? -eq 1 ]]
}

extract() { # extract RPM DIR PATH: PATH from the RPM's payload into DIR
    mkdir -p "$2"
    (cd "$2" && rpm2cpio "$1" | cpio -idm --quiet "$3")
}

rpm_vmlinuz() { # rpm_vmlinuz: the kver of the kernel-core RPM in $KERNEL; its vmlinuz is
    # then $WORK/core/lib/modules/<kver>/vmlinuz
    local core kver
    core=$(one "$KERNEL/kernel-core-*.rpm")
    extract "$(realpath "$core")" "$WORK/core" './lib/modules/*/vmlinuz'
    kver=$(one "$WORK/core/lib/modules/*/vmlinuz")
    kver=${kver%/vmlinuz}
    echo "${kver##*/}"
}

strip() { # strip SRC DST: DST is SRC without any signature
    local count
    cp "$1" "$2"
    count=$(signatures "$2")
    while ((count > 0)); do
        sbattach --remove "$2" > /dev/null
        count=$((count - 1))
    done
    [[ $(signatures "$2") -eq 0 ]] || die "$1 still carries a signature after removing them"
}

same_image() { # same_image A B: A and B are the same vmlinuz once their signatures are removed
    strip "$1" "$WORK/same-a"
    strip "$2" "$WORK/same-b"
    cmp -s "$WORK/same-a" "$WORK/same-b" || die "$1 without its signature is not $2 without its own"
}

prepare() {
    local devel kver hash
    [[ $KERNEL && $DEVEL && $OUT ]] || usage
    devel=$(one "$DEVEL/kernel-devel-*.rpm")
    kver=$(rpm_vmlinuz)
    extract "$(realpath "$devel")" "$WORK/devel" "./usr/src/kernels/$kver/.config"
    hash=$(sed -n 's/^CONFIG_MODULE_SIG_HASH="\(.*\)"$/\1/p' "$WORK/devel/usr/src/kernels/$kver/.config")
    [[ " $HASHES " == *" $hash "* ]] || die "CONFIG_MODULE_SIG_HASH of $kver is '$hash', not one of: $HASHES"
    mkdir -p "$OUT"
    cp "$WORK/core/lib/modules/$kver/vmlinuz" "$OUT/vmlinuz"
    echo "$kver" > "$OUT/kver"
    echo "$hash" > "$OUT/module-sig-hash"
    echo "prepared vmlinuz of $kver, modules hashed with $hash"
}

check_modules() { # sets KOS, the modules under $DIR, once $DIR passed the allow-list
    local entry rel branch vermagic kver_re
    [[ $KVER =~ ^[0-9][A-Za-z0-9._+-]*$ ]] || die "--kver '$KVER' is not a kernel release"
    [[ -d $DIR && ! -L $DIR ]] || die "$DIR is not a directory"
    kver_re=${KVER//./\\.}
    kver_re=${kver_re//+/\\+}
    KOS=()
    local -A branches=()
    while IFS= read -r -d '' entry; do
        rel=${entry#"$DIR"/}
        [[ ! -L $entry ]] || die "$rel is a symlink"
        if [[ -d $entry ]]; then
            [[ $rel =~ ^(open|legacy)(/lib(/modules(/$kver_re(/extra(/nvidia)?)?)?)?)?$ ]] ||
                die "$rel: unexpected directory"
            branches[${BASH_REMATCH[1]}]=1
            continue
        fi
        [[ -f $entry ]] || die "$rel is not a regular file"
        if [[ $rel =~ ^(open|legacy)/lib/modules/$kver_re/extra/nvidia/nvidia(-drm|-modeset|-uvm|-peermem)?\.ko$ ]]; then
            vermagic=$(modinfo -F vermagic "$entry")
            [[ $vermagic == "$KVER "* ]] || die "$rel: vermagic '$vermagic' is not of kernel $KVER"
            KOS+=("$entry")
        elif [[ $rel =~ ^(open|legacy)/kver$ ]]; then
            [[ $(< "$entry") == "$KVER" ]] || die "$rel names $(< "$entry"), not $KVER"
        elif ! [[ $rel =~ ^(open|legacy)/version$ || $rel =~ ^(open|legacy)-build\.log$ ]]; then
            die "$rel: not an NVIDIA module of kernel $KVER, nor a file nvidia.sh build writes"
        fi
    done < <(find "$DIR" -mindepth 1 -print0)
    [[ ${#branches[@]} -gt 0 ]] || die "no module under $DIR/{open,legacy}/lib/modules/"
    for branch in "${!branches[@]}"; do
        [[ -f $DIR/$branch/lib/modules/$KVER/extra/nvidia/nvidia.ko ]] ||
            die "$branch/lib/modules/$KVER/extra/nvidia/nvidia.ko is missing"
    done
    mapfile -t KOS < <(printf '%s\n' "${KOS[@]}" | sort)
    echo "$DIR: ${#KOS[@]} modules of $KVER in ${!branches[*]}, nothing else"
}

modules() {
    local hash cn ko signer sign_file
    [[ $KEY && $CERT && $HASH && $KVER && $DIR ]] || usage
    hash=$(< "$HASH")
    [[ " $HASHES " == *" $hash "* ]] || die "$HASH names '$hash', not one of: $HASHES"
    sign_file=${SIGN_FILE:-$(one '/usr/src/kernels/*/scripts/sign-file')}
    cn=$(cert_cn "$CERT")
    check_modules
    for ko in "${KOS[@]}"; do
        "$sign_file" "$hash" "$KEY" "$CERT" "$ko"
        signer=$(modinfo -F signer "$ko")
        [[ $signer == "$cn" ]] || die "${ko##*/}: signer \"$signer\", expected \"$cn\""
        echo "${ko#"$DIR"/}: signed by \"$signer\" with $hash"
    done
}

verify() { # verify FILE: exactly one signature, valid against $CERT
    local count
    count=$(signatures "$1")
    [[ $count -eq 1 ]] || die "$1 carries $count signatures, expected exactly one"
    sbverify --cert "$CERT" "$1" > /dev/null || die "$1 does not verify against $CERT"
    echo "$1: signed, verifies against $(cert_cn "$CERT")"
}

vmlinuz() {
    [[ $KEY && $CERT && $IN && $OUT ]] || usage
    [[ -s $IN/vmlinuz && -s $IN/kver ]] || die "$IN lacks vmlinuz or kver: run sign-kernel.sh prepare"
    strip "$IN/vmlinuz" "$WORK/vmlinuz"
    mkdir -p "$OUT"
    sbsign --key "$KEY" --cert "$CERT" --output "$OUT/vmlinuz" "$WORK/vmlinuz"
    cp "$IN/kver" "$OUT/kver"
    verify "$OUT/vmlinuz"
    same_image "$OUT/vmlinuz" "$IN/vmlinuz"
}

case $STAGE in
prepare) prepare ;;
check-modules)
    [[ $KVER && $DIR ]] || usage
    check_modules
    ;;
modules) modules ;;
vmlinuz) vmlinuz ;;
verify)
    [[ $CERT && $DIR && $KERNEL ]] || usage
    verify "$DIR/vmlinuz"
    kver=$(rpm_vmlinuz)
    [[ $(< "$DIR/kver") == "$kver" ]] || die "$DIR/kver names $(< "$DIR/kver"), the kernel-core RPM is $kver"
    same_image "$DIR/vmlinuz" "$WORK/core/lib/modules/$kver/vmlinuz"
    echo "$DIR/vmlinuz: the vmlinuz of $kver, signed"
    ;;
*) usage ;;
esac
