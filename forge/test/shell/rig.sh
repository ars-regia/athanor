#!/usr/bin/env bash
# rig.sh - the one entry point of the shell test rig. Workflows call this and nothing
# else, so every gate runs the same way on a laptop and on the hosted runner.
#
#   rig.sh build-image      build the rig and build stages locally
#   rig.sh publish-image    push the rig stage and print its digest (needs a registry login)
#   rig.sh probe-sandbox    prove that bubblewrap, and with it glycin, works in the rig
#   rig.sh css-parse        GTK parse gate over the generated stylesheets
#   rig.sh cosmic-keys      every key COSMIC ships exists in our overlay
#   rig.sh cosmic-preview   capture cosmic-panel and Settings under the Calmo defaults
#   rig.sh build-greeter    release build of athanor-greeter-ui into <out>/bin
#   rig.sh build-layout     clippy, tests and release build of the layout crates (translator and chooser) into <out>/bin
#   rig.sh layer-guard      the greeter must refuse to run when the shim loads late
#   rig.sh greeter-preview  one capture of the greeter per variant, for the eye
#   rig.sh atspi <greeter|chooser>   every interactive widget has a role and a name
#   rig.sh rig-tests        unit tests of the rig's own scripts, against the rig's tools
#   rig.sh cosmic-panel-defaults   COSMIC's shipped panel keys equal the renderer's fixture
#   rig.sh chooser-e2e      press a preset in the chooser and wait for the panel configuration
#   rig.sh surface <name>          capture every case of a surface and compare with the goldens
#   rig.sh update-goldens <name>   replace the goldens with a fresh capture, deliberately
set -euo pipefail

root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)
rig=$root/forge/test/shell
out=${ATHANOR_RIG_OUT:-$root/.scratch/shell-rig}
registry=${ATHANOR_REGISTRY:-ghcr.io/hr-mes}
local_image=localhost/athanor-shell-rig

# The image: an explicit one, else the published one pinned by digest, else the local build.
rig_image() {
    if [ -n "${ATHANOR_RIG_IMAGE:-}" ]; then
        echo "$ATHANOR_RIG_IMAGE"
    elif [ -s "$rig/rig-image.digest" ]; then
        echo "$registry/athanor-shell-rig@$(cat "$rig/rig-image.digest")"
    else
        echo "$local_image:rig"
    fi
}

# label=disable: under SELinux's container_t bubblewrap cannot mount devpts, glycin's
# loaders die, and GTK draws every SVG icon blank without reporting anything.
in_rig() { # in_rig <image> <command...>
    local image=$1
    shift
    mkdir -p "$out"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" "$image" "$@"
}

# The seal icons ship inside the athanor-calmo RPM, laid out under
# /usr/share/icons/hicolor/scalable/status the way its %install does; the rig has no
# such package, so the overlay reproduces that one directory, not the whole RPM.
# The greeter captures hand the overlay to scene.sh as RIG_DATA_OVERLAY=/out/greeter-icons.
stage_greeter_icons() {
    mkdir -p "$out/greeter-icons/icons/hicolor/scalable/status"
    install -m 0644 "$root"/system/athanor-style/calmo/generated/icons/*.svg \
        "$out/greeter-icons/icons/hicolor/scalable/status/"
}

# Each capture_<surface> runs every case of its surface and appends the tags to $tags.
capture_greeter() {
    # Test-only catalogs: German for length, the pseudo-language for right-to-left.
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale
        msgfmt --check -o /out/locale/de.mo /repo/forge/test/shell/locale/de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-greeter-ui/athanor-greeter-ui-1.0.0/po/athanor-greeter-ui.pot /out/pseudo-rtl.po
        msgfmt -o /out/locale/rtl.mo /out/pseudo-rtl.po'
    stage_greeter_icons
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        override=()
        if [ "$catalog" != - ]; then
            override=(ATHANOR_I18N_CATALOG="/out/locale/$catalog")
        fi
        in_rig "$(rig_image)" env ATHANOR_GREETER_VARIANT="$variant" ATHANOR_LOGIN_USER=rig RIG_LOCALE="$locale" \
            RIG_DATA_OVERLAY=/out/greeter-icons "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 "$scale" "$tag" -- \
            /out/bin/athanor-greeter-ui
    done < <(python3 -B "$rig/cases.py" greeter)
}

# The seed of a layout case: the user document, as a user who picked it would have it.
seed_layout() { # seed_layout <dir> <preset> <panel> <dock or ->
    mkdir -p "$1/athanor"
    {
        printf 'schema = 1\n\n[output."*"]\npreset = "%s"\npanel = "%s"\n' "$2" "$3"
        if [ "$4" != - ]; then printf 'dock = "%s"\n' "$4"; fi
    } > "$1/athanor/layout.toml"
}

capture_layout() {
    while IFS=$'\t' read -r tag preset panel dock scale width height; do
        tags+=("$tag")
        seed_layout "$out/seed-$tag" "$preset" "$panel" "$dock"
        # A float capture whose gap holds stale panel content is a known cosmic-panel defect
        # (float_frame.py), not a result: capture it again, and fail after three.
        for attempt in 1 2 3; do
            in_rig "$(rig_image)" env RIG_SETTLE=8 RIG_LOCALE=en_US.UTF-8 RIG_CONFIG_SEED="/out/seed-$tag" \
                RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
                dbus-run-session -- /repo/forge/test/shell/scene.sh "$width" "$height" "$scale" "$tag" -- \
                /repo/forge/test/shell/layout_session.sh
            if [ "$preset" != float ] || in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/float_frame.py "/out/$tag.png"; then
                break
            fi
            if [ "$attempt" = 3 ]; then
                echo "rig.sh: $tag: stale panel content in the float gap on three captures running" >&2
                exit 1
            fi
            echo "rig.sh: $tag: stale panel content in the float gap, capturing again (attempt $attempt)" >&2
        done
    done < <(python3 -B "$rig/cases.py" layout --outputs 1)
}

capture_chooser() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/chooser
        msgfmt --check -o /out/locale/chooser/de.mo /repo/forge/test/shell/locale/chooser-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/athanor-layout-chooser.pot \
            /out/chooser-pseudo-rtl.po
        msgfmt -o /out/locale/chooser/rtl.mo /out/chooser-pseudo-rtl.po'
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        # The chooser follows COSMIC's mode (SH5): seed it as COSMIC Settings would.
        mkdir -p "$out/seed-$tag/cosmic/com.system76.CosmicTheme.Mode/v1"
        if [ "$variant" = dark ]; then printf true; else printf false; fi \
            > "$out/seed-$tag/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"
        override=()
        if [ "$catalog" != - ]; then
            override=(ATHANOR_I18N_CATALOG="/out/locale/chooser/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            /out/bin/athanor-layout-chooser
    done < <(python3 -B "$rig/cases.py" chooser)
}

case "${1:-}" in
build-image)
    podman build --target rig -t "$local_image:rig" -f "$rig/Containerfile" "$rig"
    podman build --target build -t "$local_image:build" -f "$rig/Containerfile" "$rig"
    ;;
publish-image)
    podman build --target rig -t "$local_image:rig" -f "$rig/Containerfile" "$rig"
    podman push --digestfile "$out/rig-image.digest" "$local_image:rig" "docker://$registry/athanor-shell-rig:latest"
    echo "published $registry/athanor-shell-rig@$(cat "$out/rig-image.digest")"
    echo "commit that digest as forge/test/shell/rig-image.digest together with the goldens it changes"
    ;;
probe-sandbox)
    in_rig "$(rig_image)" bwrap --unshare-all --ro-bind /usr /usr --symlink usr/lib64 /lib64 --dev /dev /usr/bin/true
    echo "bubblewrap works inside the rig: glycin can decode icons"
    ;;
css-parse)
    in_rig "$(rig_image)" bash -c 'python3 /repo/forge/test/shell/css_parse_gate.py --self-test /repo/system/athanor-style/calmo/generated/css/*.css'
    ;;
cosmic-keys)
    # Every key file COSMIC ships must exist in our overlay: resolution is per directory,
    # so a key we do not carry falls back to a compiled-in default, not to COSMIC's file.
    # shellcheck disable=SC2016  # the body is expanded by the shell inside the rig.
    in_rig "$(rig_image)" bash -c '
        status=0
        overlay=/repo/system/athanor-style/calmo/generated/cosmic/cosmic
        for dir in "$overlay"/*/v*; do
            stock=/usr/share/cosmic/${dir#"$overlay"/}
            [ -d "$stock" ] || { echo "not shipped by COSMIC: $stock"; status=1; continue; }
            for key in "$stock"/*; do
                [ -e "$dir/$(basename "$key")" ] || { echo "missing in the overlay: ${dir#"$overlay"/}/$(basename "$key")"; status=1; }
            done
        done
        exit $status'
    ;;
cosmic-preview)
    mkdir -p "$out/seed-dark/cosmic/com.system76.CosmicTheme.Mode/v1"
    printf 'true' > "$out/seed-dark/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"
    overlay=/repo/system/athanor-style/calmo/generated/cosmic
    in_rig "$(rig_image)" env RIG_PANEL=1 RIG_DATA_OVERLAY="$overlay" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 cosmic-preview-light -- cosmic-settings appearance
    in_rig "$(rig_image)" env RIG_PANEL=1 RIG_DATA_OVERLAY="$overlay" RIG_CONFIG_SEED=/out/seed-dark \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 cosmic-preview-dark -- cosmic-settings appearance
    echo "look at $out/cosmic-preview-light.png and $out/cosmic-preview-dark.png"
    ;;
build-greeter)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" \
        bash -c 'cargo clippy --locked -p athanor-greeter-ui -p athanor-style --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-greeter-ui -p athanor-style \
                 && cargo build --release --locked -p athanor-greeter-ui \
                 && install -m 0755 /out/target/release/athanor-greeter-ui /out/bin/ \
                 && python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-greeter-ui'
    ;;
build-layout)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$local_image:build" \
        bash -c 'cargo clippy --locked -p athanor-layout -p athanor-layout-translator -p athanor-layout-chooser --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-layout -p athanor-layout-translator -p athanor-layout-chooser \
                 && cargo build --release --locked -p athanor-layout-translator -p athanor-layout-chooser \
                 && install -m 0755 /out/target/release/athanor-layout-translator /out/target/release/athanor-layout-chooser /out/bin/'
    ;;
layer-guard)
    rm -f "$out/layer-guard.status"
    # Preloading libwayland-client reproduces the wrong load order on purpose.
    # shellcheck disable=SC2016  # the body is expanded by the shell inside the rig.
    in_rig "$(rig_image)" env RIG_SETTLE=6 ATHANOR_LOGIN_USER=rig \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 layer-guard -- \
        bash -c 'LD_PRELOAD=/usr/lib64/libwayland-client.so.0 /out/bin/athanor-greeter-ui; echo $? > /out/layer-guard.status; sleep 60'
    # A greeter that never exits writes no status file: report that, do not die on cat.
    status=$(cat "$out/layer-guard.status" 2> /dev/null) || status=
    if [ "$status" != 1 ] || ! grep -q "not a layer surface" "$out/layer-guard-client.log"; then
        echo "layer-guard: expected exit status 1 and the guard's message, got status '$status'" >&2
        exit 1
    fi
    echo "layer-guard: the greeter refused to run as an ordinary window"
    ;;
greeter-preview)
    stage_greeter_icons
    for variant in light dark light-hc dark-hc; do
        in_rig "$(rig_image)" env ATHANOR_GREETER_VARIANT="$variant" ATHANOR_LOGIN_USER=ermete RIG_LOCALE=en_US.UTF-8 \
            RIG_DATA_OVERLAY=/out/greeter-icons \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 "greeter-preview-$variant" -- \
            /out/bin/athanor-greeter-ui
    done
    echo "look at $out/greeter-preview-*.png"
    ;;
atspi)
    # A screen reader announces itself by setting IsEnabled; GTK exports its tree then.
    enable='busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true'
    case "${2:-}" in
    greeter)
        # 6 interactive widgets: password, sign in, contrast, three power chips.
        in_rig "$(rig_image)" env GTK_A11Y=atspi ATHANOR_LOGIN_USER=rig RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=6 \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-greeter-ui 6" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-greeter -- \
            bash -c "$enable && exec /out/bin/athanor-greeter-ui"
        ;;
    chooser)
        # 8 interactive widgets under the float preset: three styles, two panel edges, three docks.
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=6 \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-layout-chooser 8" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-chooser -- \
            bash -c "$enable && exec /out/bin/athanor-layout-chooser"
        ;;
    *)
        echo "rig.sh atspi: unknown surface '${2:-}'" >&2
        exit 2
        ;;
    esac
    ;;
chooser-e2e)
    # The translator and the panel as in a session, the chooser as the client; the check
    # presses a preset and waits for the configuration. The capture shows the result.
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/layout_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 chooser-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec /repo/forge/test/shell/layout_session.sh /out/bin/athanor-layout-chooser"
    ;;
surface | update-goldens)
    surface=${2:?usage: rig.sh $1 <surface>}
    golden=$rig/golden/$surface
    if [ "$1" = update-goldens ] && [ -n "$(git -C "$root" status --porcelain -- "$golden")" ]; then
        echo "rig.sh: $golden has uncommitted changes; commit or discard them first" >&2
        exit 1
    fi
    tags=()
    case "$surface" in
    greeter) capture_greeter ;;
    layout) capture_layout ;;
    chooser) capture_chooser ;;
    *)
        echo "rig.sh $1: unknown surface '$surface'" >&2
        exit 2
        ;;
    esac
    if [ "$1" = update-goldens ]; then
        mkdir -p "$golden"
        for tag in "${tags[@]}"; do
            cp "$out/$tag.png" "$golden/$tag.png"
            echo "golden replaced: forge/test/shell/golden/$surface/$tag.png"
        done
        echo "review every image above before committing; say in the commit why they changed"
    else
        in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/compare.py \
            "/repo/forge/test/shell/golden/$surface" /out "${tags[@]}"
    fi
    ;;
rig-tests)
    # The tests of scripts that run in the rig and call its tools (ImageMagick 7). The lint
    # job runs forge/test/shell/tests, which needs nothing beyond Python.
    in_rig "$(rig_image)" python3 -B -m unittest discover -s /repo/forge/test/shell/rig_tests -v
    ;;
cosmic-panel-defaults)
    # The renderer's tests read COSMIC's shipped keys from a committed fixture; this fails
    # when the COSMIC in the rig ships different ones, so an update cannot drift silently.
    # The rig has no diffutils: copy the shipped keys out and compare them on the host.
    in_rig "$(rig_image)" bash -c 'set -euo pipefail
        rm -rf /out/cosmic-shipped
        mkdir /out/cosmic-shipped
        cp -r /usr/share/cosmic/com.system76.CosmicPanel /usr/share/cosmic/com.system76.CosmicPanel.Panel \
            /usr/share/cosmic/com.system76.CosmicPanel.Dock /out/cosmic-shipped/'
    diff -r "$root/system/athanor-layout/fixtures/cosmic-panel-1.8.0" "$out/cosmic-shipped"
    echo "cosmic-panel-defaults: the fixture matches the COSMIC in the rig"
    ;;
*)
    sed -n '2,19p' "${BASH_SOURCE[0]}" >&2
    exit 2
    ;;
esac
