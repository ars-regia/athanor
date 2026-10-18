#!/usr/bin/env bash
# no_gtk.sh - the shell's shared crates without their gtk feature, for a surface drawn
# without GTK (plan 2026-10-18-toolkit-dock, task 2): no GTK crate in their tree, and clippy
# and the tests pass, the compositor client with its calloop source. Runs in the build stage
# of the rig: rig.sh no-gtk.
set -euo pipefail

crates=(athanor-apps athanor-compositor-client athanor-style athanor-dock)
status=0
for crate in "${crates[@]}"; do
    tree=$(cargo tree --locked --no-default-features -p "$crate" -e normal,build --prefix none --format '{p}')
    if gtk=$(grep -E '^(gtk4|gdk4)[-a-z0-9]* ' <<<"$tree" | sort -u); then
        echo "no_gtk.sh: $crate without gtk still depends on:" >&2
        echo "$gtk" >&2
        status=1
    else
        echo "no_gtk.sh: $crate without gtk has no GTK crate in its tree"
    fi
done
[ "$status" = 0 ] || exit "$status"

packages=()
for crate in "${crates[@]}"; do
    packages+=(-p "$crate")
done
features=(--no-default-features --features athanor-compositor-client/calloop)
cargo clippy --locked "${features[@]}" "${packages[@]}" --all-targets -- -D warnings
cargo test --locked "${features[@]}" "${packages[@]}"
