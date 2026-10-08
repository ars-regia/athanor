# ==============================================================================
# 🌋 Athanor OS - Main Workspace Task Runner (Justfile)
# Centralized entrypoint for Forge build system, System image builder, and QA/CI pipeline
# Declarative, Nix-like hermetic target graph connecting all repository scripts
# ==============================================================================

mod forge 'forge/Justfile'
mod system 'system/Justfile'

# Default: List all available workspace targets
[private]
default:
    @just --list

# ------------------------------------------------------------------------------
# 📦 TOP-LEVEL BUILD PIPELINE (Nix-like Declarative Graph)
# ------------------------------------------------------------------------------

# Complete pipeline: Matrix -> Forge RPMs -> Kernel -> System Image
[group('Pipeline')]
all: matrix rpms kernel system-build

# Evaluates package dynamic matrix
[group('Pipeline')]
matrix:
    just forge/dynamic-matrix

# Builds custom & upstream RPM packages
[group('Pipeline')]
rpms package="":
    #!/usr/bin/env bash
    if [ -n "{{ package }}" ]; then \
        just forge/build-rolling "{{ package }}"; \
    else \
        just forge/fetch-repo-rpms; \
    fi

# Builds the Azoth kernel
[group('Pipeline')]
kernel stage="build":
    just forge/kernel-build "{{ stage }}"

# Builds System bootc container image
[group('Pipeline')]
system-build target_image=env('IMAGE_NAME', 'athanor-system') tag=env('DEFAULT_TAG', 'latest'):
    just system/build "{{ target_image }}" "{{ tag }}"

# Builds system bootc container image locally in offline fallback mode (GH Actions outage fallback)
[group('Pipeline')]
build-offline target_image="localhost/athanor-system" tag="offline":
    ./forge/scripts/build-offline.sh "{{ target_image }}" "{{ tag }}"

# Builds QCOW2 VM disk image from system bootc container
[group('Pipeline')]
disk-qcow2 target_image=("localhost/" + env('IMAGE_NAME', 'athanor-system')) tag=env('DEFAULT_TAG', 'latest'):
    just system/build-vm "qcow2" "{{ target_image }}" "{{ tag }}"

# Builds Anaconda ISO image from system bootc container
[group('Pipeline')]
disk-iso target_image=("localhost/" + env('IMAGE_NAME', 'athanor-system')) tag=env('DEFAULT_TAG', 'latest'):
    just system/build-iso "{{ target_image }}" "{{ tag }}"

# ------------------------------------------------------------------------------
# 🛡️ QA, AUDIT & HERMETIC BENCHMARK (Nix Paradigm)
# ------------------------------------------------------------------------------

# Runs hermetic build inside bubblewrap sandbox without network (Nix paradigm)
[group('QA & Security')]
hermetic-build lockfile="athanor-build.lock":
    just forge/hermetic-build "{{ lockfile }}"

# Check idempotency of a package build against GHCR SHA-256 digest
[group('QA & Security')]
check-idempotency package registry="ghcr.io" owner="ars-regia" image_name="" base_digest="":
    just forge/check-idempotency "{{ package }}" "{{ registry }}" "{{ owner }}" "{{ image_name }}" "{{ base_digest }}"

# Runs full Rust security suite (Clippy policies, Cargo Vet, Cargo Deny)
[group('QA & Security')]
audit:
    just forge/audit

# Runs AWS Kani formal verification proofs on Rust spec targets
[group('QA & Security')]
verify component:
    just forge/verify "{{ component }}"

# Validates NVIDIA kernel module loading and GPU device nodes
[group('QA & Security')]
test-nvidia:
    just forge/test-nvidia-modules

# Runs documentation sync via OpenWiki
[group('Documentation')]
openwiki-sync:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -d "openwiki" ]; then \
        openwiki --update --print; \
    else \
        openwiki --init --print; \
    fi

# Runs all workspace linters (ShellCheck, shfmt, Just syntax)
[group('QA & Security')]
lint:
    just forge/lint
    just system/lint
    just check-syntax

# The pull request gate (ADR-0075, doc_pipeline.md PL3): the `check` job of pr.yml runs exactly
# this, and a contributor runs it before pushing. Workflow lint (actionlint, with shellcheck on
# every run: block), Justfile syntax, every verify.py check with the findings listed in
# scripts/ci/known-red.txt excused (the list may only shrink against BASE), every Python
# test directory of the repository, `cargo test` over the root workspace (check-rust) and the
# licence, ban and source policy of deny.toml (check-deny).
[group('QA & Security')]
check base="HEAD":
    #!/usr/bin/env bash
    set -euo pipefail
    command -v actionlint >/dev/null || { echo "check: actionlint is not on PATH (scripts/ci/install-tools.sh)" >&2; exit 1; }
    command -v shellcheck >/dev/null || { echo "check: shellcheck is not on PATH; actionlint would skip the run: blocks" >&2; exit 1; }
    actionlint -no-color -pyflakes=
    just check-syntax
    python3 -B scripts/verify.py --known-red scripts/ci/known-red.txt --known-red-base "{{ base }}"
    python3 -B forge/specs/athanor-kernel-profile/kernel_profile.py check
    python3 -B system/athanor-style/calmo/contrast.py
    python3 -B system/athanor-style/calmo/generate.py --check
    # This suite holds no TestCase: its own main() runs the cases, and discovery would find none.
    python3 -B forge/test/iso/test_verdict.py
    dirs=$(git ls-files -- ':(glob)**/test_*.py' | xargs -n1 dirname | sort -u | grep -vx 'forge/test/iso')
    for dir in $dirs; do
        echo "check: unit tests in $dir"
        python3 -B -m unittest discover -s "$dir"
    done
    just check-rust
    just check-deny

# The Rust half of the gate: the tests of every crate of the root workspace, in the build stage
# of the shell rig (forge/test/shell/Containerfile), which carries the GTK, glycin, PAM, TPM and
# D-Bus headers the workspace links. athanor-preview-render runs in a second cargo command:
# glycin needs zbus on async-io while the workspace enables zbus' tokio runtime, and cargo
# unifies features within one command (CODE-N06). The visual shell tests are not here: pr.yml
# runs them when changes.json selects `shell`.
[group('QA & Security')]
check-rust:
    #!/usr/bin/env bash
    set -euo pipefail
    bash forge/test/shell/rig.sh build-image
    # The calculator's test runs the real qalc of the rig and may not skip here.
    export ATHANOR_REQUIRE_QALC=1
    bash forge/test/shell/rig.sh cargo test --locked --workspace --exclude athanor-preview-render
    bash forge/test/shell/rig.sh cargo test --locked -p athanor-preview-render

# The dependency policy of deny.toml over every lockfile that resolves a shipped binary:
# licences, bans and sources. Advisories are not here: they depend on a database that changes
# without a commit, so rust-security-audit.yml runs them on a schedule. The recovery kiosk and
# the attestation crate are excluded from the root workspace but still inherit its
# [workspace.dependencies], so cargo cannot resolve them on their own and cargo-deny cannot
# read them; they join this list when they get a workspace of their own. The calmo theme tool
# is a build-time derive tool that no spec installs and pins libcosmic to a git revision.
[group('QA & Security')]
check-deny:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo-deny >/dev/null || { echo "check-deny: cargo-deny is not on PATH (scripts/ci/install-tools.sh)" >&2; exit 1; }
    for manifest in Cargo.toml; do
        echo "check-deny: $manifest"
        cargo deny --locked --manifest-path "$manifest" --config deny.toml check licenses bans sources
    done

# Formats all shell scripts and Justfiles across workspace
[group('QA & Security')]
format:
    just forge/format
    just system/fix
    just --unstable --fmt -f Justfile

# ------------------------------------------------------------------------------
# 🧹 UTILITY & MAINTENANCE
# ------------------------------------------------------------------------------

# Cleans build artifacts (cargo target, RPMS_OUT, System outputs); never removes tracked files
[group('Utility')]
clean:
    just system/clean
    cargo clean
    rm -rf RPMS_OUT/
    rm -f idemp.out

# Checks syntax of all Justfiles in repository
[group('Utility')]
check-syntax:
    just --unstable --fmt --check -f Justfile
    just forge/check-syntax
    just system/check

# Auto-updates spec versions from upstream releases
[group('Utility')]
update-specs:
    just forge/update-specs

# Cleans old and untagged GHCR container images
[group('Utility')]
clean-ghcr owner="ars-regia":
    just forge/clean-ghcr "{{ owner }}"

# Runs the entire CI pipeline locally via Act for rapid debugging
[group('Utility')]
test-ci-local:
    act -W .github/workflows/athanor-forge-orchestrator.yml --container-architecture linux/amd64
