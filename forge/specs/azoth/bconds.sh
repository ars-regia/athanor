# shellcheck shell=bash
# The kernel.spec build conditions, sourced by build.sh and by lock.sh: the same
# choices for dnf builddep (--define), for rpmbuild (--with/--without) and for the
# BuildRequires the builder's toolchain lock resolves.
# clang_lto stays on even with LTO off in kernel-local: it is the only bcond through
# which kernel.spec passes HOSTCC=clang CC=clang LLVM=1 to process_configs.sh; without it
# the config would be evaluated with gcc and kCFI would vanish.
WITH=(toolchain_clang clang_lto)
WITHOUT=(debug tools perf libperf bpftool ynl selftests doc)
BCONDS=() DEFINES=()
for x in "${WITH[@]}"; do BCONDS+=(--with "$x"); DEFINES+=(--define "_with_$x 1"); done
for x in "${WITHOUT[@]}"; do BCONDS+=(--without "$x"); DEFINES+=(--define "_without_$x 1"); done
