#!/usr/bin/env bash
# The part of scripts/ci/fuzz.sh that runs inside the container: a pinned nightly, cargo-fuzz,
# then each target on a copy of its committed corpus. Not meant to be run by hand.
set -euo pipefail

# libFuzzer's instrumentation needs a nightly compiler; the date pins it (bump it in a pull
# request, with a green run of this job). rustup and cargo-fuzz are pinned like the tools of
# scripts/ci/install-tools.sh: by digest and by version.
NIGHTLY=nightly-2026-10-01
RUSTUP_VERSION=1.29.1
RUSTUP_SHA256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
CARGO_FUZZ_VERSION=0.13.2
SANITIZER=${FUZZ_SANITIZER:-address}

export RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
export PATH=$CARGO_HOME/bin:$PATH
# The registry volume (mounted at $CARGO_HOME/registry by fuzz.sh) is shared with the other
# rig jobs: keep their cache, build elsewhere.
export CARGO_TARGET_DIR=/out/target

curl -fsSL --retry 3 -o /tmp/rustup-init \
    "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init"
echo "$RUSTUP_SHA256  /tmp/rustup-init" | sha256sum -c -
chmod +x /tmp/rustup-init
/tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "$NIGHTLY"
cargo install --locked --version "$CARGO_FUZZ_VERSION" cargo-fuzz

cd /repo/fuzz
# cargo-fuzz has no --locked of its own: resolving with it first makes a fuzz/Cargo.lock that
# no longer matches fuzz/Cargo.toml an error instead of a silent rewrite.
cargo metadata --locked --format-version 1 >/dev/null
listed=$(cargo fuzz list)
mapfile -t targets <<<"$listed"
if [ -n "${FUZZ_TARGETS:-}" ]; then
    read -r -a targets <<<"$FUZZ_TARGETS"
fi

failed=()
for target in "${targets[@]}"; do
    mkdir -p "/out/corpus/$target" "/out/artifacts/$target" /out/logs
    if ! cp -r "/repo/fuzz/corpus/$target/." "/out/corpus/$target/"; then
        echo "$target: no seed corpus in fuzz/corpus/$target" >&2
        failed+=("$target")
        continue
    fi
    if [ -d "/corpus-in/$target" ]; then
        cp -r "/corpus-in/$target/." "/out/corpus/$target/"
    fi
    echo "== $target ($FUZZ_SECONDS s)"
    if ! cargo fuzz run --sanitizer "$SANITIZER" "$target" "/out/corpus/$target" -- \
        "-max_total_time=$FUZZ_SECONDS" -rss_limit_mb=4096 \
        "-artifact_prefix=/out/artifacts/$target/" >"/out/logs/$target.log" 2>&1; then
        failed+=("$target")
        tail -n 25 "/out/logs/$target.log"
    fi
done

if [ "${#failed[@]}" -gt 0 ]; then
    echo "FAILED: ${failed[*]}" >&2
    exit 1
fi
