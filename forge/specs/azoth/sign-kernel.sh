#!/usr/bin/env bash
# The signatures of the kernel artefacts (docs/architecture/doc_ci.md, D43;
# docs/architecture/doc_kernel_profile.md, key table): vmlinuz for Secure Boot, the NVIDIA
# modules for the module keyring. Runs in the signer/Containerfile image, pulled by digest:
# sign-file comes from Fedora's kernel-devel and sbsign from sbsigntools, both installed from
# signer/toolchain.lock, so nothing built by this project runs next to a key. The sign-only job
# of .github/workflows/nvidia-kmod.yml calls `modules` and `vmlinuz`; the key-less jobs before
# and after it call `prepare` and `verify`.
#
# Usage: sign-kernel.sh prepare --kernel DIR --devel DIR --out DIR
#        sign-kernel.sh modules --key FILE --cert FILE --hash FILE --dir DIR
#        sign-kernel.sh vmlinuz --key FILE --cert FILE --in DIR --out DIR
#        sign-kernel.sh verify  --cert FILE --dir DIR
#   prepare  key-less: from the kernel-core RPM in --kernel, OUT/vmlinuz and OUT/kver; from the
#            kernel-devel RPM in --devel, OUT/module-sig-hash (CONFIG_MODULE_SIG_HASH)
#   modules  sign every .ko under DIR/*/lib/modules/ with the hash named in --hash, in place,
#            and check with modinfo that the signer is the CN of --cert
#   vmlinuz  remove every signature of IN/vmlinuz (the kernel build leaves Fedora's test
#            certificate on it), sign it with sbsign into OUT/vmlinuz, copy IN/kver, verify
#   verify   DIR/vmlinuz carries exactly one signature and it verifies against --cert (PEM)
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
KERNEL='' DEVEL='' OUT='' KEY='' CERT='' HASH='' DIR='' IN=''
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

prepare() {
    local core devel kver hash
    [[ $KERNEL && $DEVEL && $OUT ]] || usage
    core=$(one "$KERNEL/kernel-core-*.rpm")
    devel=$(one "$DEVEL/kernel-devel-*.rpm")
    extract "$(realpath "$core")" "$WORK/core" './lib/modules/*/vmlinuz'
    kver=$(one "$WORK/core/lib/modules/*/vmlinuz")
    kver=${kver%/vmlinuz}
    kver=${kver##*/}
    extract "$(realpath "$devel")" "$WORK/devel" "./usr/src/kernels/$kver/.config"
    hash=$(sed -n 's/^CONFIG_MODULE_SIG_HASH="\(.*\)"$/\1/p' "$WORK/devel/usr/src/kernels/$kver/.config")
    [[ " $HASHES " == *" $hash "* ]] || die "CONFIG_MODULE_SIG_HASH of $kver is '$hash', not one of: $HASHES"
    mkdir -p "$OUT"
    cp "$WORK/core/lib/modules/$kver/vmlinuz" "$OUT/vmlinuz"
    echo "$kver" > "$OUT/kver"
    echo "$hash" > "$OUT/module-sig-hash"
    echo "prepared vmlinuz of $kver, modules hashed with $hash"
}

modules() {
    local hash cn ko signer sign_file
    [[ $KEY && $CERT && $HASH && $DIR ]] || usage
    hash=$(< "$HASH")
    [[ " $HASHES " == *" $hash "* ]] || die "$HASH names '$hash', not one of: $HASHES"
    sign_file=${SIGN_FILE:-$(one '/usr/src/kernels/*/scripts/sign-file')}
    cn=$(cert_cn "$CERT")
    mapfile -t kos < <(find "$DIR" -path '*/lib/modules/*' -name '*.ko' | sort)
    [[ ${#kos[@]} -gt 0 ]] || die "no module under $DIR/*/lib/modules/"
    for ko in "${kos[@]}"; do
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
    local count
    [[ $KEY && $CERT && $IN && $OUT ]] || usage
    [[ -s $IN/vmlinuz && -s $IN/kver ]] || die "$IN lacks vmlinuz or kver: run sign-kernel.sh prepare"
    cp "$IN/vmlinuz" "$WORK/vmlinuz"
    count=$(signatures "$WORK/vmlinuz")
    while ((count > 0)); do
        sbattach --remove "$WORK/vmlinuz" > /dev/null
        count=$((count - 1))
    done
    [[ $(signatures "$WORK/vmlinuz") -eq 0 ]] || die "$IN/vmlinuz still carries a signature after removing them"
    mkdir -p "$OUT"
    sbsign --key "$KEY" --cert "$CERT" --output "$OUT/vmlinuz" "$WORK/vmlinuz"
    cp "$IN/kver" "$OUT/kver"
    verify "$OUT/vmlinuz"
}

case $STAGE in
prepare) prepare ;;
modules) modules ;;
vmlinuz) vmlinuz ;;
verify)
    [[ $CERT && $DIR ]] || usage
    verify "$DIR/vmlinuz"
    ;;
*) usage ;;
esac
