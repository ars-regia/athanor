#!/usr/bin/env bash
# Fails when a built RPM requires a versioned soname that nothing provides: neither the RPMs
# of the same build, nor the container's own packages, nor the Fedora repositories the
# system image installs from. The builder links against Nix libraries, whose sonames can differ from Fedora's
# (libdisplay-info.so.3 against Fedora's .so.2): such a package builds and publishes, and
# the failure would only appear when the system image installs its tier, after the merge
# (issue 460). Unversioned sonames are build_spec.sh's check_sonames, which runs first.
#
# A soname that only another Athanor package provides, built in another job, fails here: no
# spec has one today, and the first that does needs a way to name it.
#
# OpenSSL 3 (libssl.so.3, libcrypto.so.3) fails whoever provides it: the image carries
# OpenSSL 4 only, and the openssl3-libs compat package of Fedora 45 must not reach it (M3).
#
# Runs in the Fedora image system/Containerfile pins (run_spec_build.sh), with network for
# the repository metadata. A metadata download or a query that fails is an error, never a pass.
# Usage: check_rpm_sonames.sh RPM...
set -euo pipefail

[[ $# -ge 1 ]] || {
    echo "usage: ${0##*/} RPM..." >&2
    exit 2
}
here=$(dirname "${BASH_SOURCE[0]}")

# A soname with a version after .so, as rpm's elfdeps writes it: libfoo.so.3()(64bit), or
# libc.so.6(GLIBC_2.34)(64bit) for a symbol version.
soname='^[^[:space:]()]+\.so\.[0-9][^[:space:]()]*\('
openssl3='^lib(ssl|crypto)\.so\.3\('

built=$(rpm -qp --provides "$@" | sed 's/ .*//' | sort -u)
bash "$here/retry.sh" dnf5 -q makecache

bad=0
for rpm in "$@"; do
    requires=$(rpm -qp --requires "$rpm" | sed 's/ .*//')
    while read -r requirement; do
        [[ $requirement =~ $soname ]] || continue
        if [[ $requirement =~ $openssl3 ]]; then
            echo "error: $(basename "$rpm") requires $requirement: OpenSSL 3 must not reach the image, link against OpenSSL 4 (M3)" >&2
            bad=1
            continue
        fi
        grep -qxF -- "$requirement" <<< "$built" && continue
        rpm -q --quiet --whatprovides "$requirement" && continue
        providers=$(dnf5 -q repoquery --whatprovides "$requirement") || {
            echo "error: dnf5 repoquery failed for $requirement" >&2
            exit 1
        }
        [[ -n $providers ]] && continue
        echo "error: $(basename "$rpm") requires $requirement, which no package provides" >&2
        bad=1
    done <<< "$requires"
done
exit "$bad"
