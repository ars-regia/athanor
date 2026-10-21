#!/usr/bin/env bash
# rig.sh - the one entry point of the shell test rig. Workflows call this and nothing
# else, so every gate runs the same way on a laptop and on the hosted runner.
#
#   rig.sh build-image      build the rig and build stages locally, layered on the published rig;
#                           with the build stage pinned (build-image.digest, a full reference), pull it instead
#   rig.sh publish-image    push the rig stage and print its digest (needs a registry login)
#   rig.sh publish-build-image  push the build stage and print its digest (needs a registry login)
#   rig.sh probe-sandbox    prove that bubblewrap, and with it glycin, works in the rig
#   rig.sh css-parse        GTK parse gate over the generated stylesheets
#   rig.sh cosmic-keys      every key COSMIC ships exists in our overlay
#   rig.sh cosmic-preview   capture COSMIC Settings' appearance page under the Calmo defaults
#   rig.sh build-greeter    release build of athanor-greeter-ui into <out>/bin
#   rig.sh build-layout     clippy, tests and release build of athanor-layout, the chooser and athanor-unit into <out>/bin
#   rig.sh build-compositor-client  clippy, tests and release build of cc-probe, and of cc-launch without gtk, into <out>/bin
#   rig.sh no-gtk           athanor-apps, the compositor client, athanor-style and athanor-dock without gtk: no GTK crate in their tree, clippy and tests
#   rig.sh build-shelld     clippy, tests and release build of athanor-shelld into <out>/bin
#   rig.sh build-compositor clippy, tests and release build of athanor-compositor into <out>/bin
#   rig.sh build-toolkit    clippy, tests and release build of athanor-toolkit's chooser, text field, test input method, layer surfaces, popups, test pointer and gallery into <out>/bin,
#                           with and without the gpu feature; the binaries carry it, so ATHANOR_TOOLKIT_RENDERER=gpu works
#   rig.sh compositor-globals   the globals athanor-compositor offers, read by wayland-info under it, the privileged ones refused (run build-compositor first)
#   rig.sh compositor-soak  athanor-compositor headless on Pixman, a GTK4 client and eglgears_wayland on llvmpipe, for RIG_SOAK seconds (600; run build-compositor first)
#   rig.sh build-bar        clippy, tests and release build of athanor-bar (and athanor-apps; athanor-controls, whose tests need a display) into <out>/bin, with the DT_NEEDED check
#   rig.sh build-control-center   clippy, tests (the panel's on a display) and release build of athanor-control-center (with athanor-services and athanor-controls) into <out>/bin, with the DT_NEEDED check
#   rig.sh cargo <args>     any cargo command in the build stage (read-only checkout); passes ATHANOR_REQUIRE_QALC through
#   rig.sh build-dock       clippy, tests and release build of athanor-dock (and athanor-apps) into <out>/bin, with the DT_NEEDED check
#   rig.sh build-launcher   clippy, tests (qalc required) and release build of athanor-launcher and athanor-preview-render into <out>/bin, with the DT_NEEDED check
#   rig.sh dock-roundtrip   the dock's surface off screen and back, three times, under cosmic-comp (BR7)
#   rig.sh shelld-e2e       athanor-shelld on a session bus: names, notifications, refusal of the private interface, tray watcher, memory
#   rig.sh bar-e2e          athanor-bar in a scene: READY, live layout, mandatory keys, running windows, the favourites import and pinning, the power menu against a fake logind, memory
#   rig.sh bar-modules-e2e  athanor-bar's network, Bluetooth, audio and battery modules against dbusmock, PipeWire and an MPRIS player; memory with every module loaded
#   rig.sh dock-e2e         athanor-dock in a scene: READY, openers, running windows, pinning, the favourites followed live, the knob and presets live, memory
#   rig.sh notifications-e2e  athanor-bar against a fake of athanor-shelld's private interface: popups, list, actions, do not disturb, hostile input, restart, memory
#   rig.sh shield-e2e       athanor-bar's trust shield against trust_state.py's files and a fake os.athanor.Update1: seal, sheet, Restart to update, Go back, refusals, hostile strings
#   rig.sh tray-e2e         athanor-bar as the tray host of athanor-shelld (run build-shelld first): items, dbusmenu menu, activation, the refused List, the watcher's restart, memory
#   rig.sh compositor-e2e   the compositor client against cosmic-comp, and against sway without the COSMIC globals
#   rig.sh layer-guard <greeter|bar|dock|launcher>   the surface must refuse to run when the shim loads late
#   rig.sh greeter-preview  one capture of the greeter per variant, for the eye
#   rig.sh bar-preview      one capture of the bar per factory layout, for the eye
#   rig.sh atspi <greeter|chooser|bar|bar-modules|dock|launcher|toolkit-chooser|gallery>   every interactive widget has a role and a name
#   rig.sh atspi <greeter|chooser|bar|dock> it   the same in Italian, with the shipped catalog
#                           bar: RIG_COMPOSITOR=athanor runs it under athanor-compositor, as its unit (run build-compositor first)
#   rig.sh atspi gallery [it]   the gallery's tree holds all of golden/atspi's, the GTK twin's; ATHANOR_GALLERY_BIN runs another gallery
#                           chooser: the transcript must hold all of golden/atspi's; ATHANOR_CHOOSER_BIN runs another chooser
#   rig.sh orca <chooser|dock|gallery> [it]   Orca's speech for a scripted walk holds the GTK surface's (OS8; needs ATHANOR_RIG_IMAGE with Orca)
#                           gallery: one walk per page, ATHANOR_GALLERY_BIN runs another gallery
#                           ATHANOR_DOCK_BIN runs another dock in `atspi dock` and `orca dock`
#   rig.sh measure-chooser  stripped size, start-up time to the first frame and memory after a minute of the GTK chooser and of the toolkit's with each renderer, and the toolkit's lines (T1 task 7; run build-layout and build-toolkit first)
#   rig.sh input-method     athanor-toolkit's text field takes a preedit and a commit from a test input method (run build-toolkit first)
#   rig.sh toolkit-layer    athanor-toolkit's layer surfaces under headless sway: one per output as outputs are added, removed, added again and scaled to 1.5 (run build-toolkit first)
#   rig.sh toolkit-popup    athanor-toolkit's tooltip and menu popups of a layer surface under headless sway: placement, grab, dismissal, keys and activation (run build-toolkit first)
#   rig.sh launcher-e2e     athanor-launcher in seven scenes, confined: READY, calculator, toggle, show time, memory, frozen provider, hostile names, no localsearch, accessible tree, a command and a refused one
#   rig.sh launcher-window-preview   the window row's preview carries the window's thumbnail
#   rig.sh chooser-e2e      press a preset in the chooser and wait until the bar and the dock draw it (run build-bar and build-dock first)
#   rig.sh surface <greeter|layout (run build-bar and build-dock first)|chooser|bar|bar-power|bar-input|bar-calendar|bar-accessibility|bar-tiling|bar-popups|bar-notifications|bar-tray|bar-network|bar-bluetooth|bar-audio|bar-battery|bar-shield|dock|launcher (run build-launcher first)|toolkit-chooser (run build-compositor and build-toolkit first)|toolkit-gallery (the same)|gtk-gallery>  capture every case of a surface and compare with the goldens
#   rig.sh update-goldens <name>   replace the goldens with a fresh capture, deliberately
set -euo pipefail

root=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)
rig=$root/forge/test/shell
out=${ATHANOR_RIG_OUT:-$root/.scratch/shell-rig}
# The dock the atspi and orca scenes run, as a path inside the rig.
dock_bin=${ATHANOR_DOCK_BIN:-/out/bin/athanor-dock}
dock_app=${dock_bin##*/}
# RIG_HOLD is split into words, so the name it carries must be one word.
case $dock_app in
'' | *[[:space:]]*)
    echo "rig.sh: ATHANOR_DOCK_BIN must end in a file name without spaces, not '$dock_bin'" >&2
    exit 2
    ;;
esac
registry=${ATHANOR_REGISTRY:-ghcr.io/ars-regia}
registry=${registry,,} # OCI references are lowercase; an owner such as "Ars-Regia" is not
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

# The build stage, where cargo runs: an explicit one, else the published one pinned by digest,
# else the local build. Pinned, no recipe resolves packages against the Fedora mirrors.
build_image() {
    if [ -n "${ATHANOR_RIG_BUILD_IMAGE:-}" ]; then
        echo "$ATHANOR_RIG_BUILD_IMAGE"
    elif [ -s "$rig/build-image.digest" ]; then
        cat "$rig/build-image.digest" # the full reference that was pushed, see publish-build-image
    else
        echo "$local_image:build"
    fi
}

# The base of the fixtures' layer: the published rig when there is one (see the Containerfile).
rig_base=()
if [ -s "$rig/rig-image.digest" ]; then
    rig_base=(--build-arg "RIG_BASE=$registry/athanor-shell-rig@$(cat "$rig/rig-image.digest")")
fi

# label=disable: under SELinux's container_t bubblewrap cannot mount devpts, glycin's
# loaders die, and GTK draws every SVG icon blank without reporting anything.
# RIG_OUTPUTS=2 runs only in the throwaway guest of kvm/guest.sh, as root: scene.sh drives
# the vkms card on cosmic-comp's KMS backend, which needs the DRM devices and the udev database.
# RIG_COMPOSITOR=athanor reaches session.sh, which nests athanor-compositor in place of cosmic-comp,
# and runs the container in systemd mode, whose cgroup tree is writable: bar_session.py --unit
# puts the bar in its unit's cgroup there, for the compositor's allow-list (OS4).
# ATHANOR_TOOLKIT_RENDERER reaches athanor-toolkit's clients: cpu (the default) or gpu, which
# draws through wgpu on the rig's llvmpipe, as the rig has no render node.
in_rig() { # in_rig <image> <command...>
    local image=$1 opts=()
    shift
    if [ "${RIG_OUTPUTS:-1}" = 2 ]; then
        opts=(--device /dev/dri -v /run/udev:/run/udev:ro -e RIG_OUTPUTS)
    fi
    if [ -n "${RIG_COMPOSITOR:-}" ]; then
        opts+=(-e RIG_COMPOSITOR)
    fi
    if [ "${RIG_COMPOSITOR:-}" = athanor ]; then
        opts+=(--systemd=always)
    fi
    if [ -n "${ATHANOR_TOOLKIT_RENDERER:-}" ]; then
        opts+=(-e ATHANOR_TOOLKIT_RENDERER)
    fi
    mkdir -p "$out"
    podman run --rm --memory 6g --security-opt label=disable "${opts[@]}" \
        -v "$root:/repo:ro" -v "$out:/out" "$image" "$@"
}

# The seal icons ship inside the athanor-calmo RPM, laid out under
# /usr/share/icons/hicolor/scalable/status the way its %install does; the rig has no
# such package, so the overlay reproduces that one directory, not the whole RPM.
# The greeter captures hand the overlay to scene.sh as RIG_DATA_OVERLAY=/out/greeter-icons.
# The bar's runs see both the COSMIC theme overlay and the seal icons of its shield.
bar_overlay=/repo/system/athanor-style/calmo/generated/cosmic:/out/greeter-icons
stage_greeter_icons() {
    mkdir -p "$out/greeter-icons/icons/hicolor/scalable/status"
    install -m 0644 "$root"/system/athanor-style/calmo/generated/icons/*.svg \
        "$out/greeter-icons/icons/hicolor/scalable/status/"
}

# The greeter lists the accounts NSS returns (doc_lock_and_prompts.md, LP19), and the rig
# image has none in the human range: each greeter run first adds `rig`, uid 1000, to the
# container's /etc/passwd, so the default card is Rig's.
with_account=(bash -c 'echo "rig:x:1000:1000:Rig:/home/rig:/bin/bash" >> /etc/passwd && exec "$@"' with-account)

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
        in_rig "$(rig_image)" "${with_account[@]}" env ATHANOR_GREETER_VARIANT="$variant" RIG_LOCALE="$locale" \
            RIG_DATA_OVERLAY=/out/greeter-icons "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 "$scale" "$tag" -- \
            python3 /repo/forge/test/shell/bar_session.py --client athanor-greeter-ui
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

# The seed of a bar case: the layout document, two keyboard layouts so the input source
# shows, one favourite the rig has installed, and COSMIC's mode (SH5).
seed_scheme() { # seed_scheme <dir> <light|dark>: COSMIC's mode, for cosmic-comp, and the
    # GNOME colour scheme, which the shell reads (doc_visual_language.md, VL4); scene.sh sets
    # the second from <dir>.gsettings
    mkdir -p "$1/cosmic/com.system76.CosmicTheme.Mode/v1"
    if [ "$2" = dark ]; then printf true; else printf false; fi \
        > "$1/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"
    if [ "$2" = dark ]; then
        echo "org.gnome.desktop.interface color-scheme prefer-dark"
    else
        echo "org.gnome.desktop.interface color-scheme default"
    fi > "$1.gsettings"
}

seed_bar() { # seed_bar <dir> <preset> <panel> <dock or -> <light|dark>
    seed_layout "$1" "$2" "$3" "$4"
    mkdir -p "$1/cosmic/com.system76.CosmicComp/v1" "$1/cosmic/com.system76.CosmicAppletTime/v1"
    printf '(rules: "", model: "pc105", layout: "us,it", variant: ",", options: None, repeat_delay: 600, repeat_rate: 25)' \
        > "$1/cosmic/com.system76.CosmicComp/v1/xkb_config"
    seed_scheme "$1" "$5"
    # Pins the 24-hour clock the scenes' goldens show: en_US and ar_EG would otherwise
    # switch to 12 hours by locale.
    printf true > "$1/cosmic/com.system76.CosmicAppletTime/v1/military_time"
    printf 'schema = 1\nfavorites = ["com.system76.CosmicSettings.desktop"]\n' > "$1/athanor/favorites.toml"
}

# The layout cases: the bar and the dock as the session runs them, drawing the seeded preset.
capture_layout() {
    require athanor-bar build-bar
    require athanor-dock build-dock
    stage_greeter_icons
    while IFS=$'\t' read -r tag preset panel dock scale width height; do
        tags+=("$tag")
        seed_layout "$out/seed-$tag" "$preset" "$panel" "$dock"
        in_rig "$(rig_image)" env RIG_SETTLE=8 RIG_LOCALE=en_US.UTF-8 RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY="$bar_overlay" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh "$width" "$height" "$scale" "$tag" -- \
            python3 /repo/forge/test/shell/bar_session.py --log --beside athanor-dock
    done < <(python3 -B "$rig/cases.py" layout --outputs "${RIG_LAYOUT_OUTPUTS:-1}")
}

# The chooser's test catalogs, German and the right-to-left pseudo-language, made from the GTK
# chooser's: both choosers read them.
stage_chooser_catalogs() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/chooser
        msgfmt --check -o /out/locale/chooser/de.mo /repo/forge/test/shell/locale/chooser-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/athanor-layout-chooser.pot \
            /out/chooser-pseudo-rtl.po
        msgfmt -o /out/locale/chooser/rtl.mo /out/chooser-pseudo-rtl.po'
}

# ATHANOR_CHOOSER_BIN runs another chooser in the GTK chooser's scenes, athanor-toolkit's
# (doc_own_stack.md, T1 task 4); its captures then differ from the GTK goldens by design.
capture_chooser() {
    stage_chooser_catalogs
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        seed_scheme "$out/seed-$tag" "$variant"
        override=()
        if [ "$catalog" != - ]; then
            override=(ATHANOR_I18N_CATALOG="/out/locale/chooser/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            "${ATHANOR_CHOOSER_BIN:-/out/bin/athanor-layout-chooser}"
    done < <(python3 -B "$rig/cases.py" chooser)
}

# doc_own_stack.md, C1 task 1 and T1 task 4: athanor-toolkit's chooser under
# athanor-compositor, in the GTK chooser's catalogs, mirrored by the right-to-left one. At 1.5
# the toolkit draws at 1.5 itself, through wp_fractional_scale_v1 and wp_viewporter.
capture_toolkit_chooser() {
    require athanor-compositor build-compositor
    require athanor-toolkit-chooser build-toolkit
    stage_chooser_catalogs
    while IFS=$'\t' read -r tag _ scale locale catalog; do
        tags+=("$tag")
        override=()
        if [ "$catalog" != - ]; then
            override=(ATHANOR_I18N_CATALOG="/out/locale/chooser/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_COMPOSITOR=athanor RIG_LOCALE="$locale" "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            /out/bin/athanor-toolkit-chooser
    done < <(python3 -B "$rig/cases.py" toolkit-chooser)
}

# The gallery's pages in English, left to right and right to left, at both scales: the toolkit's
# under athanor-compositor, and its GTK twin (forge/test/shell/gallery_gtk.py) under cosmic-comp,
# whose captures are the references to match. Each case passes its page and direction to the
# binary as arguments.
capture_gallery() { # capture_gallery <toolkit-gallery|gtk-gallery>
    local surface=$1 compositor=() bin=/repo/forge/test/shell/gallery_gtk.py
    if [ "$surface" = toolkit-gallery ]; then
        require athanor-compositor build-compositor
        require athanor-toolkit-gallery build-toolkit
        compositor=(RIG_COMPOSITOR=athanor)
        bin=/out/bin/athanor-toolkit-gallery
    fi
    while IFS=$'\t' read -r tag variant scale locale _ page direction; do
        tags+=("$tag")
        seed_scheme "$out/seed-$tag" "$variant"
        args=(--page "$page")
        if [ "$direction" = rtl ]; then args+=(--rtl); fi
        in_rig "$(rig_image)" env "${compositor[@]}" RIG_LOCALE="$locale" RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            "$bin" "${args[@]}"
    done < <(python3 -B "$rig/cases.py" "$surface")
}

require() { # require <binary> <recipe>: a scene that runs one of our binaries
    if [ ! -x "$out/bin/$1" ]; then
        echo "rig.sh: $out/bin/$1 is missing; run rig.sh $2 first" >&2
        exit 1
    fi
}

# doc_bar.md, BR9: the bar under its own preset with one running window, the five
# popovers the bar owns in 2b.2, opened by ATHANOR_BAR_OPEN over the float preset,
# 2b.3's notification popups (four waiting notifications: three show), the notification
# list and a tray menu, the popovers of the four system modules against
# system_fixtures.py, and the shield's sheet over a downloaded update.
capture_bar() { # capture_bar <surface>
    local surface=$1 open="" preset=float panel=top dock=visible settle=8 stable=""
    local session=(python3 /repo/forge/test/shell/bar_session.py)
    case "$surface" in
    bar) preset=bar panel=bottom dock=- session+=(--window) ;;
    bar-power) open=power ;;
    bar-input) open=input-source ;;
    bar-calendar) open=clock ;;
    bar-accessibility) open=accessibility ;;
    bar-tiling) open=tiling ;;
    bar-popups) session+=(--notifications) ;;
    bar-notifications) open=notifications session+=(--notifications) ;;
    bar-tray)
        require athanor-shelld build-shelld
        open=tray session+=(--tray)
        ;;
    # The system modules against the fixtures, which start before the bar: a longer settle.
    bar-network) open=network settle=12 session+=(--fixtures) ;;
    # The adapter already discovers, so the popover opens at its final size: cosmic-comp
    # at a fractional scale leaves the edge column of a popover drawn before it grew. Its
    # bottom edge was still settling at 1.5 right-to-left (#545): capture until it is stable.
    bar-bluetooth) open=bluetooth settle=12 stable=10 session+=(--fixtures --discovering) ;;
    bar-audio) open=audio settle=12 session+=(--fixtures) ;;
    bar-battery) open=battery settle=12 session+=(--fixtures) ;;
    # The shield's sheet above a bottom panel (item 13's third condition), with a downloaded
    # update so Restart to update and every row of the sheet show.
    bar-shield) preset=bar panel=bottom dock=- open=shield session+=(--trust-state downloaded) ;;
    esac
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/bar
        msgfmt --check -o /out/locale/bar/de.mo /repo/forge/test/shell/locale/bar-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-bar/athanor-bar-1.0.0/po/athanor-bar.pot /out/bar-pseudo-rtl.po
        msgfmt -o /out/locale/bar/rtl.mo /out/bar-pseudo-rtl.po'
    stage_greeter_icons
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        seed_bar "$out/seed-$tag" "$preset" "$panel" "$dock" "$variant"
        override=()
        if [ "$catalog" != - ]; then
            override+=(ATHANOR_I18N_CATALOG="/out/locale/bar/$catalog")
        fi
        if [ -n "$open" ]; then
            override+=(ATHANOR_BAR_OPEN="$open")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_SETTLE="$settle" RIG_STABLE="$stable" RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY="$bar_overlay" "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- "${session[@]}"
    done < <(python3 -B "$rig/cases.py" "$surface")
}

# doc_bar.md, BR9: the dock with one running window, beside a bottom panel so that it
# stands on the start edge: the left one, the right one in the right-to-left cases.
capture_dock() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/dock
        msgfmt --check -o /out/locale/dock/de.mo /repo/forge/test/shell/locale/dock-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-dock/athanor-dock-1.0.0/po/athanor-dock.pot /out/dock-pseudo-rtl.po
        msgfmt -o /out/locale/dock/rtl.mo /out/dock-pseudo-rtl.po'
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        seed_bar "$out/seed-$tag" float bottom visible "$variant"
        override=()
        if [ "$catalog" != - ]; then
            override+=(ATHANOR_I18N_CATALOG="/out/locale/dock/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_SETTLE=8 RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            python3 /repo/forge/test/shell/bar_session.py --client athanor-dock --window
    done < <(python3 -B "$rig/cases.py" dock)
}

# doc_launcher.md, LA11: the launcher shown at start over the float layout with the query
# "cc window": the CC Window application on top, its preview (icon, name, description,
# "System image"), the settings pages and the web row. The rig has no localsearch, so no
# file group; the launcher runs without libfaketime and paints no time, and usage is empty,
# so the order is the ranking's alone.
capture_launcher() {
    in_rig "$(rig_image)" bash -c '
        set -euo pipefail
        mkdir -p /out/locale/launcher
        msgfmt --check -o /out/locale/launcher/de.mo /repo/forge/test/shell/locale/launcher-de.po
        python3 /repo/forge/test/shell/locale/make_pseudo_rtl.py \
            /repo/forge/specs/athanor-launcher/athanor-launcher-1.0.0/po/athanor-launcher.pot /out/launcher-pseudo-rtl.po
        msgfmt -o /out/locale/launcher/rtl.mo /out/launcher-pseudo-rtl.po'
    while IFS=$'\t' read -r tag variant scale locale catalog; do
        tags+=("$tag")
        seed_bar "$out/seed-$tag" float top visible "$variant"
        override=()
        if [ "$catalog" != - ]; then
            override+=(ATHANOR_I18N_CATALOG="/out/locale/launcher/$catalog")
        fi
        in_rig "$(rig_image)" env RIG_LOCALE="$locale" RIG_SETTLE=8 RIG_CONFIG_SEED="/out/seed-$tag" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            ATHANOR_LAUNCHER_SHOW="cc window" "${override[@]}" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 "$scale" "$tag" -- \
            bash -c "python3 /repo/forge/test/shell/launcher_fixtures.py \
                     && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher --log"
    done < <(python3 -B "$rig/cases.py" launcher)
}

case "${1:-}" in
build-image)
    if [ "$(rig_image)" = "$local_image:rig" ] || [ "$(build_image)" = "$local_image:build" ]; then
        podman build "${rig_base[@]}" --target rig -t "$local_image:rig" -f "$rig/Containerfile" "$rig"
    fi
    if [ "$(build_image)" = "$local_image:build" ]; then
        podman build "${rig_base[@]}" --target build -t "$local_image:build" -f "$rig/Containerfile" "$rig"
    else
        podman pull "$(build_image)"
    fi
    ;;
publish-image)
    podman build "${rig_base[@]}" --target rig -t "$local_image:rig" -f "$rig/Containerfile" "$rig"
    podman push --digestfile "$out/rig-image.digest" "$local_image:rig" "docker://$registry/athanor-shell-rig:latest"
    echo "published $registry/athanor-shell-rig@$(cat "$out/rig-image.digest")"
    echo "commit that digest as forge/test/shell/rig-image.digest together with the goldens it changes"
    ;;
publish-build-image)
    # The build stage only compiles and runs unit tests, so it is published on its own: the rig's
    # digest, and with it the pixels of the goldens, stay where they are.
    mkdir -p "$out"
    podman build "${rig_base[@]}" --target build -t "$local_image:build" -f "$rig/Containerfile" "$rig"
    podman push --digestfile "$out/build-image.digest" "$local_image:build" "docker://$registry/athanor-shell-rig-build:latest"
    echo "$registry/athanor-shell-rig-build@$(cat "$out/build-image.digest")" > "$out/build-image.ref"
    echo "published $(cat "$out/build-image.ref")"
    echo "commit that reference as forge/test/shell/build-image.digest together with the Containerfile change it builds"
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
    seed_scheme "$out/seed-dark" dark
    overlay=/repo/system/athanor-style/calmo/generated/cosmic
    in_rig "$(rig_image)" env RIG_DATA_OVERLAY="$overlay" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 cosmic-preview-light -- cosmic-settings appearance
    in_rig "$(rig_image)" env RIG_DATA_OVERLAY="$overlay" RIG_CONFIG_SEED=/out/seed-dark \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 cosmic-preview-dark -- cosmic-settings appearance
    echo "look at $out/cosmic-preview-light.png and $out/cosmic-preview-dark.png"
    ;;
build-greeter)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
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
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-layout -p athanor-layout-chooser -p athanor-unit --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-layout -p athanor-layout-chooser -p athanor-unit \
                 && cargo build --release --locked -p athanor-layout-chooser \
                 && install -m 0755 /out/target/release/athanor-layout-chooser /out/bin/'
    ;;
build-compositor-client)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-compositor-client --all-targets -- -D warnings \
                 && cargo clippy --locked -p athanor-compositor-client --no-default-features --features calloop --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-compositor-client --features calloop \
                 && cargo build --release --locked -p athanor-compositor-client --example cc-probe \
                 && install -m 0755 /out/target/release/examples/cc-probe /out/bin/ \
                 && cargo build --release --locked -p athanor-compositor-client --no-default-features --features calloop --example cc-launch \
                 && install -m 0755 /out/target/release/examples/cc-launch /out/bin/'
    ;;
no-gtk)
    mkdir -p "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" bash forge/test/shell/no_gtk.sh
    ;;
build-shelld)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-unit -p athanor-shelld --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-unit -p athanor-shelld \
                 && cargo build --release --locked -p athanor-shelld \
                 && install -m 0755 /out/target/release/athanor-shelld /out/bin/'
    ;;
build-compositor)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-compositor --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-compositor \
                 && cargo build --release --locked -p athanor-compositor \
                 && install -m 0755 /out/target/release/athanor-compositor /out/bin/'
    ;;
build-toolkit)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-toolkit --all-targets -- -D warnings \
                 && cargo clippy --locked -p athanor-toolkit --all-targets --features gpu -- -D warnings \
                 && cargo rustdoc --locked -p athanor-toolkit -- -D warnings \
                 && cargo test --locked -p athanor-toolkit \
                 && cargo test --locked -p athanor-toolkit --features gpu \
                 && cargo build --release --locked -p athanor-toolkit --features gpu --example chooser --example field --example input_method --example layer --example popup --example pointer \
                 && install -m 0755 /out/target/release/examples/chooser /out/bin/athanor-toolkit-chooser \
                 && install -m 0755 /out/target/release/examples/field /out/bin/athanor-toolkit-field \
                 && install -m 0755 /out/target/release/examples/input_method /out/bin/athanor-toolkit-input-method \
                 && install -m 0755 /out/target/release/examples/layer /out/bin/athanor-toolkit-layer \
                 && install -m 0755 /out/target/release/examples/popup /out/bin/athanor-toolkit-popup \
                 && install -m 0755 /out/target/release/examples/pointer /out/bin/athanor-toolkit-pointer \
                 && cargo clippy --locked --manifest-path system/athanor-toolkit/gallery/Cargo.toml --all-targets -- -D warnings \
                 && cargo build --release --locked --manifest-path system/athanor-toolkit/gallery/Cargo.toml \
                 && install -m 0755 /out/target/release/athanor-toolkit-gallery /out/bin/athanor-toolkit-gallery'
    ;;
compositor-globals)
    # doc_own_stack.md, C1 tasks 1, 4 and 5: what a client of athanor-compositor binds. The rig
    # has no systemd and no allow-list, so wayland-info runs in no unit and is granted none of
    # the privileged globals of task 5; the fixture checks the grant.
    require athanor-compositor build-compositor
    rm -f "$out/compositor-globals.txt"
    in_rig "$(rig_image)" env RIG_COMPOSITOR=athanor RIG_SETTLE=2 \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 compositor-globals -- \
        bash -c 'wayland-info > /out/compositor-globals.txt && exec sleep 60'
    missing=()
    for interface in wl_compositor wl_shm wl_seat wl_output xdg_wm_base zwp_linux_dmabuf_v1 wp_viewporter \
        wp_fractional_scale_manager_v1 wp_presentation wp_cursor_shape_manager_v1 xdg_activation_v1 \
        wl_data_device_manager zwlr_layer_shell_v1 ext_workspace_manager_v1 \
        wp_security_context_manager_v1 ext_idle_notifier_v1; do
        grep -q "interface: '$interface'" "$out/compositor-globals.txt" || missing+=("$interface")
    done
    for capability in pointer keyboard; do
        grep -Eq "capabilities:.*\b$capability\b" "$out/compositor-globals.txt" || missing+=("wl_seat $capability")
    done
    for interface in ext_foreign_toplevel_list_v1 ext_image_copy_capture_manager_v1 \
        ext_foreign_toplevel_image_capture_source_manager_v1 ext_session_lock_manager_v1; do
        ! grep -q "interface: '$interface'" "$out/compositor-globals.txt" || missing+=("refusal of $interface")
    done
    if [ "${#missing[@]}" != 0 ]; then
        echo "compositor-globals: missing ${missing[*]}; see $out/compositor-globals.txt" >&2
        exit 1
    fi
    echo "compositor-globals: the 16 standard globals, a seat with pointer and keyboard, and none of the 4 privileged ones"
    ;;
compositor-soak)
    # doc_own_stack.md, C1 task 2: the headless backend under two clients drawing without pause;
    # the rig has no render node, so the compositor draws on Pixman and the clients on llvmpipe.
    # mesa-demos installs eglgears_wayland under /usr/lib64/mesa, off PATH.
    # compositor_soak.sh rewrites its logs in /out/compositor-soak.
    require athanor-compositor build-compositor
    in_rig "$(rig_image)" dbus-run-session -- /repo/forge/test/shell/compositor_soak.sh \
        /out/compositor-soak "${RIG_SOAK:-600}" \
        /out/bin/athanor-compositor --headless --output 1280x800 --output 800x600@1.5 -- \
        'GDK_BACKEND=wayland exec python3 /repo/forge/test/shell/gtk_animation.py' 'exec /usr/lib64/mesa/eglgears_wayland'
    ;;
cargo)
    shift
    mkdir -p "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -e ATHANOR_REQUIRE_QALC -w /repo "$(build_image)" cargo "$@"
    ;;
build-bar)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-apps -p athanor-controls -p athanor-bar --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-apps -p athanor-bar \
                 && forge/test/shell/with-display.sh cargo test --locked -p athanor-controls \
                 && cargo build --release --locked -p athanor-bar \
                 && install -m 0755 /out/target/release/athanor-bar /out/bin/ \
                 && python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-bar'
    ;;
build-control-center)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-services -p athanor-controls -p athanor-control-center --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-services -p athanor-control-center --bins --lib \
                 && forge/test/shell/with-display.sh cargo test --locked -p athanor-control-center --bins \
                 && cargo build --release --locked -p athanor-control-center \
                 && install -m 0755 /out/target/release/athanor-control-center /out/bin/ \
                 && python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-control-center'
    ;;
build-dock)
    mkdir -p "$out/bin" "$out/target"
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -w /repo "$(build_image)" \
        bash -c 'cargo clippy --locked -p athanor-apps -p athanor-dock --all-targets -- -D warnings \
                 && cargo test --locked -p athanor-apps -p athanor-dock \
                 && cargo build --release --locked -p athanor-dock \
                 && install -m 0755 /out/target/release/athanor-dock /out/bin/ \
                 && python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-dock'
    ;;
build-launcher)
    mkdir -p "$out/bin" "$out/target"
    # ATHANOR_REQUIRE_QALC: the calculator's test runs the real qalc and may not skip here.
    # The decoder is built on its own: the workspace enables zbus' tokio runtime for the
    # shell, and glycin runs on zbus' async-io. Cargo unifies features within one command,
    # so a command that holds both packages does not compile glycin-core. The decoder's own
    # spec builds with -p, as here.
    podman run --rm --memory 6g --security-opt label=disable \
        -v "$root:/repo:ro" -v "$out:/out" -v athanor-cargo-registry:/root/.cargo/registry \
        -e CARGO_TARGET_DIR=/out/target -e ATHANOR_REQUIRE_QALC=1 -w /repo "$(build_image)" \
        bash -c 'set -euo pipefail
                 for packages in "-p athanor-search -p athanor-preview -p athanor-launcher" "-p athanor-preview-render"; do
                     cargo clippy --locked $packages --all-targets -- -D warnings
                     cargo test --locked $packages
                     cargo build --release --locked $packages
                 done
                 install -m 0755 /out/target/release/athanor-launcher /out/target/release/athanor-preview-render /out/bin/
                 python3 -B forge/scripts/check_shim_link_order.py /out/bin/athanor-launcher'
    ;;
dock-roundtrip)
    # BR7's knob none and the bar preset take the surface off screen, and visible brings it
    # back: cosmic-comp must keep the dock's connection across every round trip.
    seed_bar "$out/seed-dock-roundtrip" float top visible light
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-dock-roundtrip \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/dock_roundtrip.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 dock-roundtrip -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec /out/bin/athanor-dock"
    ;;
shelld-e2e)
    rm -f "$out/shelld-e2e.log"
    in_rig "$(rig_image)" dbus-run-session -- python3 /repo/forge/test/shell/shelld_e2e.py
    ;;
bar-e2e)
    stage_greeter_icons
    seed_bar "$out/seed-bar-e2e" float top visible light
    # No favourites file: the first start imports the favourites and saves them (BR7).
    rm "$out/seed-bar-e2e/athanor/favorites.toml"
    rm -f "$out/bar-e2e-logind.log"
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-bar-e2e \
        RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/bar_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 bar-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --hang CanReboot --window --pinnable"
    ;;
bar-modules-e2e)
    stage_greeter_icons
    seed_bar "$out/seed-bar-modules-e2e" float top visible light
    rm -f "$out"/bar-modules-e2e-*.log
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-bar-modules-e2e \
        RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/bar_modules_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 bar-modules-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --fixtures"
    ;;
dock-e2e)
    seed_bar "$out/seed-dock-e2e" float top visible light
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-dock-e2e \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        RIG_HOLD="python3 /repo/forge/test/shell/dock_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 dock-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-dock --window --pinnable"
    ;;
notifications-e2e)
    stage_greeter_icons
    seed_bar "$out/seed-notifications-e2e" float top visible light
    rm -f "$out/notifications-e2e-notifications.log"
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_CONFIG_SEED=/out/seed-notifications-e2e \
        RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/notifications_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 notifications-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --notifications --respawn"
    ;;
shield-e2e)
    stage_greeter_icons
    seed_bar "$out/seed-shield-e2e" float top visible light
    rm -f "$out/shield-e2e-logind.log"
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_CONFIG_SEED=/out/seed-shield-e2e \
        RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/shield_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 shield-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --notifications --trust-state downloaded"
    ;;
tray-e2e)
    require athanor-shelld build-shelld
    stage_greeter_icons
    seed_bar "$out/seed-tray-e2e" float top visible light
    rm -f "$out/tray-e2e-tray.log" "$out/tray-e2e-shelld.log"
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_CONFIG_SEED=/out/seed-tray-e2e ATHANOR_BAR_OPEN=tray \
        RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/tray_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 tray-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --tray --respawn"
    ;;
compositor-e2e)
    # cosmic-comp reads the keyboard layouts from its configuration: two, so the switch shows.
    seed=$out/compositor-e2e-seed/cosmic/com.system76.CosmicComp/v1
    mkdir -p "$seed"
    printf '(rules: "", model: "pc105", layout: "us,it", variant: ",", options: None, repeat_delay: 600, repeat_rate: 25)' > "$seed/xkb_config"
    in_rig "$(rig_image)" env RIG_CONFIG_SEED=/out/compositor-e2e-seed \
        RIG_HOLD="python3 /repo/forge/test/shell/compositor_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 compositor-e2e -- \
        python3 /repo/forge/test/shell/cc_window.py 1
    ;;
launcher-e2e)
    # Every stage is its own scene: the fixtures differ, and each launcher starts fresh.
    require athanor-launcher build-launcher
    for stage in e2e frozen-provider hostile no-localsearch tree command command-refused; do
        query="cc window" fixtures=""
        case $stage in
        e2e) query="2+2*3" ;;
        frozen-provider) fixtures=--frozen ;;
        hostile) query=gnp fixtures=--hostile ;;
        command) query=">ls" ;;
        command-refused) query='>"unclosed' ;;
        esac
        # bar_session.py appends to the launcher log: a run must not count the last one's lines.
        rm -f "$out/launcher-$stage-athanor-launcher.log"
        seed_bar "$out/seed-launcher-$stage" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
            RIG_CONFIG_SEED="/out/seed-launcher-$stage" \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            ATHANOR_LAUNCHER_SHOW="$query" \
            RIG_HOLD="python3 /repo/forge/test/shell/launcher_e2e.py $stage" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 "launcher-$stage" -- \
            bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                     && python3 /repo/forge/test/shell/launcher_fixtures.py $fixtures \
                     && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher --log"
    done
    ;;
launcher-window-preview)
    # doc_launcher.md, LA6: a window's preview carries its thumbnail (Task 10's capture).
    require athanor-launcher build-launcher
    seed_bar "$out/seed-launcher-window-preview" float top visible light
    in_rig "$(rig_image)" env RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-launcher-window-preview \
        RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
        ATHANOR_LAUNCHER_SHOW=cc-window-1 GTK_A11Y=atspi \
        RIG_HOLD="python3 /repo/forge/test/shell/launcher_e2e.py window-preview" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 launcher-window-preview -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher --window --log"
    in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/compare.py \
        /repo/forge/test/shell/golden/launcher-window-preview /out launcher-window-preview
    ;;
layer-guard)
    surface=${2:?usage: rig.sh layer-guard <greeter|bar|dock|launcher>}
    case "$surface" in
    greeter) binary=athanor-greeter-ui ;;
    bar) binary=athanor-bar ;;
    dock) binary=athanor-dock ;;
    launcher) binary=athanor-launcher ;;
    *)
        echo "rig.sh layer-guard: unknown surface '$surface'" >&2
        exit 2
        ;;
    esac
    rm -f "$out/layer-guard-$surface.status"
    # Preloading libwayland-client reproduces the wrong load order on purpose.
    in_rig "$(rig_image)" "${with_account[@]}" env RIG_SETTLE=6 \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 "layer-guard-$surface" -- \
        bash -c "LD_PRELOAD=/usr/lib64/libwayland-client.so.0 /out/bin/$binary; echo \$? > /out/layer-guard-$surface.status; sleep 60"
    # A surface that never exits writes no status file: report that, do not die on cat.
    status=$(cat "$out/layer-guard-$surface.status" 2> /dev/null) || status=
    if [ "$status" != 1 ] || ! grep -q "not a layer surface" "$out/layer-guard-$surface-client.log"; then
        echo "layer-guard: expected exit status 1 and the guard's message from $binary, got status '$status'" >&2
        exit 1
    fi
    echo "layer-guard: $binary refused to run as an ordinary window"
    ;;
greeter-preview)
    stage_greeter_icons
    for variant in light dark light-hc dark-hc; do
        in_rig "$(rig_image)" "${with_account[@]}" env ATHANOR_GREETER_VARIANT="$variant" RIG_LOCALE=en_US.UTF-8 \
            RIG_DATA_OVERLAY=/out/greeter-icons \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1920 1080 1.0 "greeter-preview-$variant" -- \
            /out/bin/athanor-greeter-ui
    done
    echo "look at $out/greeter-preview-*.png"
    ;;
bar-preview)
    stage_greeter_icons
    # The three factory layouts, for the eye; the goldens are `surface bar`.
    while read -r preset panel dock; do
        seed_layout "$out/seed-bar-preview-$preset" "$preset" "$panel" "$dock"
        in_rig "$(rig_image)" env RIG_LOCALE=en_US.UTF-8 RIG_CONFIG_SEED="/out/seed-bar-preview-$preset" \
            RIG_DATA_OVERLAY="$bar_overlay" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 "bar-preview-$preset" -- \
            /out/bin/athanor-bar
    done << 'EOF'
float top visible
bar bottom -
minimal top none
EOF
    echo "look at $out/bar-preview-*.png"
    ;;
atspi)
    # A screen reader announces itself by setting IsEnabled; GTK exports its tree then.
    enable='busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true'
    # In Italian as well as English (doc_shell.md, section 7 item 3 and section 8 item 6): the
    # shipped catalog, and a node that must carry its Italian name, so an English tree fails.
    lang=(RIG_LOCALE=en_US.UTF-8)
    required=
    case "${3:-}" in
    '' | it) ;;
    *) echo "atspi: the language is 'it' or none, not '$3'" >&2; exit 2 ;;
    esac
    if [ "${3:-}" = it ] && [ "${2:-}" = gallery ]; then
        # The gallery takes its language from --lang; "Pulsanti" is its Italian page.
        required=Pulsanti
        lang=(RIG_LOCALE=it_IT.UTF-8)
    elif [ "${3:-}" = it ]; then
        declare -A italian=([greeter]="athanor-greeter-ui Immagine di sistema verificata"
            [chooser]="athanor-layout-chooser Isola" [bar]="athanor-bar Alimentazione"
            [dock]="athanor-dock Applicazioni")
        [ -n "${italian[${2:-}]:-}" ] || { echo "atspi ${2:-} has no Italian check" >&2; exit 2; }
        read -r component required <<< "${italian[$2]}"
        in_rig "$(rig_image)" bash -c "mkdir -p /out/locale/it &&
            msgfmt --check -o /out/locale/it/$component.mo /repo/forge/specs/$component/$component-1.0.0/po/it.po"
        lang=(RIG_LOCALE=it_IT.UTF-8 ATHANOR_I18N_CATALOG="/out/locale/it/$component.mo")
    fi
    case "${2:-}" in
    greeter)
        # 7 interactive widgets: back to the list, password, sign in, contrast, three power
        # chips; and the seal, which names the badge a verified state backs (doc_shell.md,
        # section 7, item 3).
        in_rig "$(rig_image)" "${with_account[@]}" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=6 \
            ATSPI_REQUIRED_NAME="${required:-System image verified}" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-greeter-ui 7" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-greeter -- \
            bash -c "$enable && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-greeter-ui"
        ;;
    chooser)
        # 8 interactive widgets under the float preset: three styles, two panel edges, three docks.
        # ATHANOR_CHOOSER_BIN runs another chooser, athanor-toolkit's (doc_own_stack.md, T1), in
        # the same scene. The transcript, every node's role, name, states and value, must hold
        # all the reference holds: the GTK chooser's, recorded (OS8).
        # To record it again, copy <out>/atspi-chooser[-it].json of a GTK run into golden/atspi/.
        bin=${ATHANOR_CHOOSER_BIN:-/out/bin/athanor-layout-chooser}
        transcript=atspi-chooser${3:+-$3}.json
        in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=6 \
            ATSPI_REQUIRED_NAME="$required" ATSPI_TRANSCRIPT="/out/$transcript" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py ${bin##*/} 8" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-chooser -- \
            bash -c "$enable && exec $bin"
        in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/atspi_compare.py \
            "/repo/forge/test/shell/golden/atspi/$transcript" "/out/$transcript"
        ;;
    gallery)
        # The gallery's first page: the page toggle, a button, three toggles and a field, and the
        # title bar's two buttons in the toolkit's. ATHANOR_GALLERY_BIN runs the GTK twin
        # (forge/test/shell/gallery_gtk.py), or the toolkit's gallery by default; its transcript must
        # hold all the reference holds, the GTK twin's, recorded (OS8).
        # To record it again, copy <out>/atspi-gallery[-it].json of a GTK run into golden/atspi/.
        bin=${ATHANOR_GALLERY_BIN:-/out/bin/athanor-toolkit-gallery}
        if [ -z "${ATHANOR_GALLERY_BIN:-}" ]; then require athanor-toolkit-gallery build-toolkit; fi
        transcript=atspi-gallery${3:+-$3}.json
        in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=6 \
            ATSPI_REQUIRED_NAME="$required" ATSPI_TRANSCRIPT="/out/$transcript" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py ${bin##*/} 7" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-gallery -- \
            bash -c "$enable && exec $bin --lang ${3:-en}"
        in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/atspi_compare.py \
            "/repo/forge/test/shell/golden/atspi/$transcript" "/out/$transcript"
        ;;
    toolkit-chooser)
        # The same 8 choices in athanor-toolkit's chooser, under athanor-compositor, through
        # AccessKit (doc_own_stack.md, C1 task 1).
        require athanor-compositor build-compositor
        require athanor-toolkit-chooser build-toolkit
        in_rig "$(rig_image)" env RIG_COMPOSITOR=athanor RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=6 \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-toolkit-chooser 8" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-toolkit-chooser -- \
            bash -c "$enable && exec /out/bin/athanor-toolkit-chooser"
        ;;
    bar)
        # 8 interactive widgets under float: workspaces, applications, clock, input source,
        # accessibility, tiling, power, the shield. Under athanor-compositor the bar runs as its
        # unit, or the compositor closes its surface on the top layer (OS4, C1 task 7), and shows
        # 6: input source and tiling need zcosmic_keyboard_layout_manager_v1 and
        # zcosmic_workspace_manager_v2, whose private replacements are still to be specified
        # (docs/spikes/c1-protocols.md, section 3).
        unit='' widgets=8
        if [ "${RIG_COMPOSITOR:-}" = athanor ]; then
            require athanor-compositor build-compositor
            unit=--unit widgets=6
        fi
        stage_greeter_icons
        seed_bar "$out/seed-atspi-bar" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-atspi-bar \
            RIG_DATA_OVERLAY="$bar_overlay" ATSPI_REQUIRED_NAME="$required" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-bar $widgets" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-bar -- \
            bash -c "$enable && exec python3 /repo/forge/test/shell/bar_session.py $unit"
        # GTK keeps exporting the tree of a surface the compositor closed, so the walk alone
        # passes a bar that is not on screen; the compositor's log says when it closed one
        # (src/layer.rs).
        if [ -n "$unit" ] && grep -q "not granted them was closed" "$out/atspi-bar-compositor.log"; then
            echo "atspi bar: athanor-compositor closed the bar's surface; see $out/atspi-bar-compositor.log" >&2
            exit 1
        fi
        ;;
    bar-modules)
        # 12 interactive widgets under float with every fixture: the 8 of `bar`, and audio,
        # Bluetooth, network and battery. The fixtures start first, so the settle is longer.
        stage_greeter_icons
        seed_bar "$out/seed-atspi-bar-modules" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=12 RIG_CONFIG_SEED=/out/seed-atspi-bar-modules \
            RIG_DATA_OVERLAY="$bar_overlay" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-bar 12" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-bar-modules -- \
            bash -c "$enable && exec python3 /repo/forge/test/shell/bar_session.py --fixtures"
        ;;
    dock)
        # 4 interactive widgets under float: launcher, workspaces, the pinned COSMIC
        # Settings of the seed, applications. ATHANOR_DOCK_BIN runs another dock, athanor-toolkit's
        # (doc_own_stack.md, the dock plan), whose transcript must hold all the GTK dock's holds.
        # To record it again, copy <out>/atspi-dock[-it].json of a GTK run into golden/atspi/.
        transcript=atspi-dock${3:+-$3}.json
        seed_bar "$out/seed-atspi-dock" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-atspi-dock \
            ATSPI_REQUIRED_NAME="$required" ATSPI_TRANSCRIPT="/out/$transcript" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py $dock_app 4" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-dock -- \
            bash -c "$enable && exec python3 /repo/forge/test/shell/bar_session.py --client $(printf %q "$dock_bin")"
        in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/atspi_compare.py \
            "/repo/forge/test/shell/golden/atspi/$transcript" "/out/$transcript"
        ;;
    launcher)
        # 1 interactive widget by atspi_check.py's list of roles: the entry, named "Search".
        # The list ("Results") and its rows are not in that list, and a header row has no
        # name by design; launcher_e2e.py reads the rows' names from the tree instead.
        seed_bar "$out/seed-atspi-launcher" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-atspi-launcher \
            ATHANOR_LAUNCHER_SHOW="cc window" \
            RIG_HOLD="python3 /repo/forge/test/shell/atspi_check.py athanor-launcher 1" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 atspi-launcher -- \
            bash -c "$enable && python3 /repo/forge/test/shell/launcher_fixtures.py \
                     && exec python3 /repo/forge/test/shell/bar_session.py --client athanor-launcher"
        ;;
    *)
        echo "rig.sh atspi: unknown surface '${2:-}'" >&2
        exit 2
        ;;
    esac
    ;;
orca)
    # doc_own_stack.md, OS8: Orca's speech transcript for a scripted walk through a surface, in
    # English and in Italian, must hold every utterance of the GTK surface's, recorded first in
    # golden/orca/. ATHANOR_CHOOSER_BIN and ATHANOR_DOCK_BIN run another chooser and another dock,
    # athanor-toolkit's. The published rig has no Orca yet: ATHANOR_RIG_IMAGE names a local build
    # of the Containerfile's rig stage.
    # To record it again, copy <out>/orca-<surface>[-it].txt of a GTK run into golden/orca/.
    case "${2:-}" in
    chooser) component=athanor-layout-chooser ;;
    dock) component=athanor-dock ;;
    gallery) component= ;;
    *) echo "rig.sh orca: the surface is 'chooser', 'dock' or 'gallery', not '${2:-}'" >&2; exit 2 ;;
    esac
    lang=(RIG_LOCALE=en_US.UTF-8)
    case "${3:-}" in
    '') ;;
    it)
        if [ -z "$component" ]; then
            lang=(RIG_LOCALE=it_IT.UTF-8)
        else
            in_rig "$(rig_image)" bash -c "mkdir -p /out/locale/it && msgfmt --check -o /out/locale/it/$component.mo \
                /repo/forge/specs/$component/$component-1.0.0/po/it.po"
            lang=(RIG_LOCALE=it_IT.UTF-8 ATHANOR_I18N_CATALOG="/out/locale/it/$component.mo")
        fi
        ;;
    *) echo "rig.sh orca: the language is 'it' or none, not '$3'" >&2; exit 2 ;;
    esac
    if ! in_rig "$(rig_image)" rpm -q orca wtype; then
        echo "rig.sh orca: $(rig_image) has no Orca; build the Containerfile's rig stage and name it in ATHANOR_RIG_IMAGE" >&2
        exit 1
    fi
    enable='busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true'
    transcript=orca-$2${3:+-$3}.txt
    case "$2" in
    chooser)
        bin=${ATHANOR_CHOOSER_BIN:-/out/bin/athanor-layout-chooser}
        # Ten controls, Minimize, Close and the eight choices; the eleventh Tab comes back round.
        in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=6 \
            RIG_HOLD="python3 /repo/forge/test/shell/orca_walk.py /out/$transcript Tab*11" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 orca-chooser -- \
            bash -c "$enable && exec $bin"
        ;;
    gallery)
        # One walk per page, started on that page (--page), whose toggle holds the focus in both
        # galleries: Space selects the page, then one Tab for each page toggle after it and for
        # each control of the page (cases.py gallery-pages). The walks follow one another in
        # the transcript, in the order of the pages. ATHANOR_GALLERY_BIN runs the GTK twin.
        bin=${ATHANOR_GALLERY_BIN:-/out/bin/athanor-toolkit-gallery}
        if [ -z "${ATHANOR_GALLERY_BIN:-}" ]; then require athanor-toolkit-gallery build-toolkit; fi
        rm -f "$out/$transcript"
        while IFS=$'\t' read -r page tabs; do
            in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=6 \
                RIG_HOLD="python3 /repo/forge/test/shell/orca_walk.py /out/orca-gallery-$page.txt space Tab*$tabs" \
                dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 orca-gallery -- \
                bash -c "$enable && exec $bin --page $page --lang ${3:-en}"
            cat "$out/orca-gallery-$page.txt" >> "$out/$transcript"
        done < <(python3 -B "$rig/cases.py" gallery-pages)
        ;;
    dock)
        # orca_dock.py runs the test window and a minimised second app, then starts the dock
        # again so that it holds the focus, on Launcher. Six Tabs go round its six buttons
        # (Launcher, Workspaces, the pinned COSMIC Settings, the test window, the minimised app,
        # Applications); three more reach the test window, whose menu Shift+F10 opens, the
        # arrows walk, and Escape closes, back on the button (AX7). cosmic-comp then hands the
        # keyboard to the window (#614); orca_dock.py drops that line.
        seed_bar "$out/seed-orca-dock" float top visible light
        in_rig "$(rig_image)" env GTK_A11Y=atspi "${lang[@]}" RIG_SETTLE=8 RIG_CONFIG_SEED=/out/seed-orca-dock \
            RIG_HOLD="python3 /repo/forge/test/shell/orca_dock.py $dock_app /out/$transcript Tab*9 shift+F10 Down*4 Escape" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 orca-dock -- \
            bash -c "$enable && exec python3 /repo/forge/test/shell/bar_session.py --client $(printf %q "$dock_bin") --window --pinnable --respawn"
        ;;
    esac
    in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/orca_compare.py \
        "/repo/forge/test/shell/golden/orca/$transcript" "/out/$transcript"
    ;;
measure-chooser)
    # doc_own_stack.md, T1 task 7: the GTK chooser and athanor-toolkit's, with vello_cpu and with
    # vello_gpu, in the same scene under cosmic-comp, one after the other; measure_chooser.py
    # launches each, so the scene's own client only holds it open. The tables go to
    # <out>/measure/report.md.
    require athanor-layout-chooser build-layout
    require athanor-toolkit-chooser build-toolkit
    mkdir -p "$out/measure"
    runs=(athanor-layout-chooser: athanor-toolkit-chooser:cpu athanor-toolkit-chooser:gpu)
    for run in "${runs[@]}"; do
        bin=${run%%:*} renderer=${run#*:}
        name=$bin${renderer:+-$renderer}
        ATHANOR_TOOLKIT_RENDERER=$renderer in_rig "$(rig_image)" env RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=2 \
            RIG_DATA_OVERLAY=/repo/system/athanor-style/calmo/generated/cosmic \
            RIG_HOLD="python3 -B /repo/forge/test/shell/measure_chooser.py run /out/bin/$bin /out/measure/$name.json" \
            dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 "measure-$name" -- sleep 600
    done
    in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/measure_chooser.py report /repo/system/athanor-toolkit \
        /out/measure/athanor-layout-chooser.json /out/measure/athanor-toolkit-chooser-cpu.json \
        /out/measure/athanor-toolkit-chooser-gpu.json > "$out/measure/report.md"
    cat "$out/measure/report.md"
    ;;
input-method)
    # doc_own_stack.md, T1 task 6 (OS9 item 3): the test input method pre-edits and commits into
    # athanor-toolkit's text field under cosmic-comp, which offers zwp_input_method_v2 to the
    # scene's clients because none is behind a security context.
    require athanor-toolkit-field build-toolkit
    require athanor-toolkit-input-method build-toolkit
    in_rig "$(rig_image)" env RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=4 \
        RIG_HOLD="python3 /repo/forge/test/shell/input_method_check.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 input-method -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec /out/bin/athanor-toolkit-field"
    echo "look at $out/input-method-preedit.png (the preedit underlined) and $out/input-method-commit.png"
    ;;
toolkit-layer)
    # The dock plan, task 3: the driver keeps one layer surface per output. sway's headless
    # backend adds and removes outputs, which athanor-compositor's fixture cannot yet (C1 task 8).
    require athanor-toolkit-layer build-toolkit
    in_rig "$(rig_image)" env XDG_RUNTIME_DIR=/run/user/1000 python3 /repo/forge/test/shell/toolkit_layer_check.py
    ;;
toolkit-popup)
    # The dock plan, task 4: a menu and a tooltip in xdg-popups of a layer surface, under sway's
    # headless backend with a virtual pointer and wtype's keyboard.
    require athanor-toolkit-popup build-toolkit
    require athanor-toolkit-pointer build-toolkit
    in_rig "$(rig_image)" env XDG_RUNTIME_DIR=/run/user/1000 python3 /repo/forge/test/shell/toolkit_popup_check.py
    ;;
chooser-e2e)
    # The bar and the dock as in a session, the chooser beside them; the check presses a
    # preset and waits until both have drawn it. The capture shows the result.
    require athanor-bar build-bar
    require athanor-dock build-dock
    stage_greeter_icons
    in_rig "$(rig_image)" env GTK_A11Y=atspi RIG_LOCALE=en_US.UTF-8 RIG_SETTLE=8 \
        RIG_DATA_OVERLAY="$bar_overlay" \
        RIG_HOLD="python3 /repo/forge/test/shell/layout_e2e.py" \
        dbus-run-session -- /repo/forge/test/shell/scene.sh 1280 800 1.0 chooser-e2e -- \
        bash -c "busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true \
                 && exec python3 /repo/forge/test/shell/bar_session.py --log --beside athanor-dock --beside /out/bin/athanor-layout-chooser"
    # The bar at the depth a fresh start gives the pressed preset: the bottom 40 rows of the
    # capture, below where float's dock stood, against their golden. A live change once kept
    # float's 56px surface under the bar's 36px strip.
    in_rig "$(rig_image)" magick /out/chooser-e2e.png -gravity south -crop x40+0+0 +repage /out/chooser-e2e-bar.png
    in_rig "$(rig_image)" python3 -B /repo/forge/test/shell/compare.py \
        /repo/forge/test/shell/golden/chooser-e2e /out chooser-e2e-bar
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
    bar | bar-power | bar-input | bar-calendar | bar-accessibility | bar-tiling | bar-popups | bar-notifications | bar-tray | bar-network | bar-bluetooth | bar-audio | bar-battery | bar-shield) capture_bar "$surface" ;;
    dock) capture_dock ;;
    launcher) capture_launcher ;;
    toolkit-chooser) capture_toolkit_chooser ;;
    toolkit-gallery | gtk-gallery) capture_gallery "$surface" ;;
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
*)
    sed -n '2,/^set -euo pipefail$/{/^#/p}' "${BASH_SOURCE[0]}" >&2
    exit 2
    ;;
esac
