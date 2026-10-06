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
check-idempotency package registry="ghcr.io" owner="hr-mes" image_name="" base_digest="":
    just forge/check-idempotency "{{ package }}" "{{ registry }}" "{{ owner }}" "{{ image_name }}" "{{ base_digest }}"

# Runs full Rust security suite (Clippy policies, Cargo Vet, Cargo Deny)
[group('QA & Security')]
audit:
    just forge/audit

# Audits and enforces strict 0700/0400 permissions on Secure Boot & UKI signing keys
[group('QA & Security')]
secureboot-key-audit:
    just system/secureboot-key-audit

# Runs cargo-fuzz fuzzing suite on Rust spec targets
[group('QA & Security')]
fuzz component="all" time="60":
    just forge/fuzz "{{ component }}" "{{ time }}"

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

# Formats all shell scripts and Justfiles across workspace
[group('QA & Security')]
format:
    just forge/format
    just system/fix
    just --unstable --fmt -f Justfile

# ------------------------------------------------------------------------------
# 🧹 UTILITY & MAINTENANCE
# ------------------------------------------------------------------------------

# Cleans all build artifacts across Forge and System
[group('Utility')]
clean:
    just system/clean
    rm -rf RPMS_OUT/ forge/build/ idemp.out *.lock

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
clean-ghcr owner="hr-mes":
    just forge/clean-ghcr "{{ owner }}"

# Runs the entire CI pipeline locally via Act for rapid debugging
[group('Utility')]
test-ci-local:
    act -W .github/workflows/athanor-forge-orchestrator.yml --container-architecture linux/amd64
