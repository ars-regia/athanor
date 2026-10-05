#!/usr/bin/env bash
# Signs the vmlinuz that GRUB boots through shim, outside the image build
# (docs/architecture/doc_kernel_profile.md, D43 and section 4). With the project key the script
# takes no kernel from its caller, nor from the artifacts of the run: it resolves the azoth image
# of the pins itself
# with system/kernel-artifacts.sh resolve, which accepts only a digest signed by kernel-build.yml
# on a trusted branch (cosign, keyless, the identity pinned in that script) and attested as
# built from the inputs of this checkout, in the registry of KERNEL_REGISTRY or its default,
# never one named by a file another job could have written. That digest is the one pulled, so a
# job that holds the key signs that kernel and nothing else. The signing itself runs in the
# locked forge/specs/azoth/sign image with the network off (sign/sign.sh), and the result must
# verify against the certificate before it is written. Needs cosign, skopeo and jq on PATH.
#
# Usage: sign-kernel.sh --out DIR [--throwaway]
#   release      the project key in SECUREBOOT_SIGNING_KEY, checked against the committed
#                certificate forge/specs/azoth/keys/secureboot/athanor-secureboot.pem. Runs only
#                in the sign-only job of the signing environment.
#   --throwaway  a key generated for this run and deleted with it, for a build that is never
#                published (pull-request check, local rehearsal); its certificate is written to
#                DIR/throwaway-certificate.pem so the image build can verify against it. The
#                kernel is the one in the caller's kernel artifacts (KERNEL_ARTIFACTS_DIR).
# Writes DIR/vmlinuz and DIR/kver.
#
# The key leaves the environment before the first child process starts and is kept in a shell
# variable no child inherits. It reaches a file only after the kernel is pulled and the sign
# image built: written by the shell's own printf, under umask 077, into a private directory on
# tmpfs, mounted read-only into the container, and removed as soon as the container exits. It
# is never on a command line, in a world-readable file, or in the environment of another
# process (as system/sign-images.sh). The registry login, when the kernel image is not public,
# is the caller's business.
set -euo pipefail

usage() {
    echo "usage: ${0##*/} --out DIR [--throwaway]" >&2
    exit 2
}
OUT='' THROWAWAY=false
while [[ $# -gt 0 ]]; do
    case $1 in
    --out)
        [[ $# -ge 2 && -n $2 ]] || usage
        OUT=$2
        shift 2
        ;;
    --throwaway)
        THROWAWAY=true
        shift
        ;;
    *) usage ;;
    esac
done
[[ -n $OUT ]] || usage

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
azoth=$root/forge/specs/azoth
signing_key=''
if [[ $THROWAWAY == false ]]; then
    [[ -n ${SECUREBOOT_SIGNING_KEY:-} ]] || {
        echo "${0##*/}: SECUREBOOT_SIGNING_KEY is not available to this job: check the signing environment" >&2
        exit 2
    }
    signing_key=$SECUREBOOT_SIGNING_KEY
fi
unset SECUREBOOT_SIGNING_KEY

umask 077
keys=$(mktemp -d -p "${XDG_RUNTIME_DIR:-/dev/shm}" sign-kernel.XXXXXX)
# The kernel RPMs and the verified artifacts hold no secret: they go to disk.
rpms=$(mktemp -d)
verified=$(mktemp -d)
trap 'rm -rf "$keys" "$rpms" "$verified"' EXIT
mkdir -p "$OUT"
out=$(cd "$OUT" && pwd)

if [[ $THROWAWAY == true ]]; then
    # Nothing trusts a throwaway signature, and the build that asks for one takes its other
    # inputs from the caller's kernel artifacts: the kernel comes from them too.
    artifact() { bash "$root/system/kernel-artifacts.sh" "$@"; }
else
    artifact() { KERNEL_ARTIFACTS_DIR=$verified bash "$root/system/kernel-artifacts.sh" "$@"; }
    artifact resolve
fi
digest=$(artifact get kernel_digest) || {
    echo "${0##*/}: the kernel of the pins is not published, signed by kernel-build.yml on a trusted branch and attested for this checkout: nothing to sign" >&2
    exit 1
}
nvr=$(artifact get nvr)
pinned=$(bash "$azoth/nvr.sh")
[[ $nvr == "$pinned" ]] || {
    echo "${0##*/}: the kernel artifacts were resolved for ${nvr}, the pins give ${pinned}: run system/kernel-artifacts.sh resolve again" >&2
    exit 2
}
image="$(artifact get registry)/azoth@$digest"

bash "$root/forge/scripts/retry.sh" podman pull "$image"
ctr=$(podman create "$image" /kernel)
podman cp "$ctr:/." "$rpms/"
podman rm "$ctr" > /dev/null
podman build -t localhost/azoth-sign -f "$azoth/sign/Containerfile" "$azoth"

if [[ $THROWAWAY == true ]]; then
    # The parameters of the project certificate (keys/profiles/secureboot.cnf): RSA 4096,
    # digitalSignature, codeSigning.
    cert=$out/throwaway-certificate.pem
    openssl req -quiet -new -x509 -newkey rsa:4096 -sha256 -nodes -days 1 -subj "/CN=Athanor throwaway kernel signing key" \
        -addext basicConstraints=critical,CA:FALSE -addext keyUsage=digitalSignature -addext extendedKeyUsage=codeSigning \
        -keyout "$keys/key" -out "$cert"
    chmod 0644 "$cert"
    echo "${0##*/}: --throwaway: the kernel is signed with a key generated for this run, and an image built from it must not be published"
else
    cert=$azoth/keys/secureboot/athanor-secureboot.pem
    printf '%s\n' "$signing_key" > "$keys/key"
    signing_key=''
fi

# SELinux labels are left alone (label=disable) rather than relabelled (:z): relabelling
# would rewrite the context of the checkout and of the key directory on the host.
status=0
podman run --rm --network=none --security-opt label=disable \
    -v "$azoth/sign/sign.sh:/usr/local/bin/sign.sh:ro" \
    -v "$rpms:/rpms:ro" \
    -v "$keys/key:/run/sign/key:ro" \
    -v "$cert:/run/sign/cert.pem:ro" \
    -v "$out:/out" \
    localhost/azoth-sign bash /usr/local/bin/sign.sh /rpms /run/sign/key /run/sign/cert.pem /out || status=$?
rm -f "$keys/key"
[[ $status -eq 0 ]] || exit "$status"

kver=$(< "$out/kver")
[[ $kver == "$nvr.x86_64" ]] || {
    echo "${0##*/}: signed the vmlinuz of $kver, the verified kernel is $nvr" >&2
    exit 1
}
chmod 0644 "$out/vmlinuz" "$out/kver"
echo "${0##*/}: $out/vmlinuz ($kver, $image) signed by $(openssl x509 -in "$cert" -noout -subject)"
