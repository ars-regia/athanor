#!/usr/bin/env bash
# compositor-acceptance.sh [stage...]
# Package 2a of docs/architecture/doc_shell.md in the dev VM's real session, where the rig
# has no user manager: an application launched by athanor-compositor-client runs in its own
# transient unit behind a security context (doc_bar.md, BR2), a Terminal=true entry runs in
# the default terminal, and the openers of cosmic-launcher, cosmic-app-library and
# cosmic-workspaces start and show them. Deploys cc-probe from .scratch/shell-rig/bin (build
# it with forge/test/shell/rig.sh build-compositor-client). With no argument it runs every
# stage in order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/compositor-acceptance/; whether an opener showed its surface is read from them.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
SHOTS=$ROOT/.scratch/compositor-acceptance
STAGES=(deploy restricted terminal openers)
# Globals only the main socket offers: an application behind the context sees none of them.
PRIVILEGED=(zcosmic_toplevel_info_v1 zcosmic_toplevel_manager_v1 ext_workspace_manager_v1
    zcosmic_workspace_manager_v2 zwlr_layer_shell_v1 ext_data_control_manager_v1
    zwlr_data_control_manager_v1 wp_security_context_manager_v1
    zcosmic_keyboard_layout_manager_v1 cosmic_a11y_manager_v1)
STAGE=

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@" 2> /dev/null; do
        ((SECONDS < deadline)) || return 1
        sleep 1
    done
}

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
}

shot() {
    sleep 2
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null
}

# A desktop entry in the session user's data directory. The ids keep to the characters a
# unit name carries unescaped, so the unit names need no quoting over SSH.
entry() { # entry ID LINE...
    local id=$1
    shift
    printf '%s\n' '[Desktop Entry]' 'Type=Application' "Name=$id" "$@" |
        in_session "mkdir -p ~/.local/share/applications && cat > ~/.local/share/applications/$id.desktop"
}

# Launches a desktop entry through the compositor client; prints the unit's name. ID is the
# entry's basename as written by entry(), without the .desktop suffix cc-probe's
# DesktopAppInfo lookup requires.
launch() { # launch DESKTOP-ID
    local out
    out=$(in_session cc-probe launch "$1.desktop") || fail "cc-probe launch $1: $out"
    sed -n 's/^{"launched":"\(.*\)"}$/\1/p' <<< "$out"
}

absent() { ! in_session test -e "$1"; }
# One NameHasOwner call: `busctl status` looks the owner up, then its credentials, and fails
# with ENXIO when the owner exits in between. An error is neither owned nor unowned, and its
# message stays in the guest: the caller polls.
has_owner() { # has_owner NAME: prints "b true" or "b false"
    in_session busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus \
        NameHasOwner s "$1" "2> /dev/null"
}

# The application units and socket directories the session holds, one per line, sorted.
app_units() {
    in_session "systemctl --user list-units --all --plain --no-legend 'app-athanor-*' | cut -d' ' -f1 | sort"
}
socket_dirs() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    in_session 'ls -1 "/run/user/$(id -u)/athanor" | sort'
}
owned() { [[ $(has_owner "$1") == "b true" ]]; }
unowned() { [[ $(has_owner "$1") == "b false" ]]; }

stage_deploy() {
    "$HERE/deploy.sh" "$BIN/cc-probe:/usr/bin/cc-probe" > /dev/null
    entry os.athanor.AcceptanceWaylandInfo \
        'Exec=sh -c "wayland-info > /tmp/athanor-acceptance-globals; exec sleep 600"'
    entry os.athanor.AcceptanceTerminal 'Exec=sleep 600' 'Terminal=true'
    in_session command -v xdg-terminal-exec > /dev/null ||
        fail "xdg-terminal-exec is not in the image; until an image with it is installed: scripts/devvm/ssh.sh sudo dnf -y install xdg-terminal-exec"
}

stage_restricted() {
    local unit environment display directory seen main global units dirs out
    in_session rm -f /tmp/athanor-acceptance-globals
    unit=$(launch os.athanor.AcceptanceWaylandInfo)
    [[ $unit =~ ^app-athanor-os\.athanor\.AcceptanceWaylandInfo@[0-9a-f]{32}\.service$ ]] ||
        fail "unit name '$unit'"
    wait_until 10 in_session "grep -q \"^interface: 'wl_compositor'\" /tmp/athanor-acceptance-globals" ||
        fail "no wayland-info output in /tmp/athanor-acceptance-globals for $unit"
    seen=$(in_session cat /tmp/athanor-acceptance-globals | sed -n "s/^interface: '\([a-z0-9_]*\)'.*/\1/p")
    main=$(in_session wayland-info | grep -c '^interface: ')
    (($(wc -l <<< "$seen") < main)) || fail "$(wc -l <<< "$seen") globals behind the context, $main on the main socket"
    for global in "${PRIVILEGED[@]}"; do
        if grep -qx "$global" <<< "$seen"; then
            fail "$global is offered behind the context"
        fi
    done
    environment=$(in_session systemctl --user show -p Environment --value "$unit")
    display=$(tr ' ' '\n' <<< "$environment" | sed -n 's/^WAYLAND_DISPLAY=//p')
    [[ $display =~ ^/run/user/[0-9]+/athanor/[0-9a-f]{32}/wayland$ ]] || fail "WAYLAND_DISPLAY '$display'"
    [[ $environment == *XDG_ACTIVATION_TOKEN=* ]] || fail "no activation token in '$environment'"
    directory=${display%/wayland}
    [[ $(in_session stat -c %a "$directory") == 700 ]] || fail "$directory has mode $(in_session stat -c %a "$directory")"
    [[ $(in_session systemctl --user show -p Type,ExitType --value "$unit" | paste -sd,) == exec,cgroup ]] ||
        fail "unit type $(in_session systemctl --user show -p Type,ExitType --value "$unit" | paste -sd,)"
    # Fail closed: a context offers no wp_security_context_manager_v1, so a launch made from
    # the restricted socket starts nothing and leaves no socket directory behind.
    units=$(app_units)
    dirs=$(socket_dirs)
    if out=$(in_session WAYLAND_DISPLAY="$display" cc-probe launch os.athanor.AcceptanceWaylandInfo.desktop 2>&1); then
        fail "cc-probe launch on the restricted socket succeeded: $out"
    fi
    [[ $out == *wp_security_context_manager_v1* ]] ||
        fail "cc-probe launch on the restricted socket failed without naming wp_security_context_manager_v1: $out"
    [[ $(app_units) == "$units" ]] || fail "a unit appeared after the refused launch: $(app_units)"
    [[ $(socket_dirs) == "$dirs" ]] || fail "a socket directory appeared after the refused launch: $(socket_dirs)"
    in_session systemctl --user stop "$unit"
    wait_until 5 absent "$directory" || fail "$directory outlived its unit"
}

stage_terminal() {
    local unit main
    unit=$(launch os.athanor.AcceptanceTerminal)
    wait_until 10 in_session "systemd-cgls --no-pager --user-unit $unit | grep -q 'sleep 600'" ||
        fail "no 'sleep 600' in $unit: $(in_session systemd-cgls --no-pager --user-unit "$unit")"
    main=$(in_session "ps -o comm= -p \$(systemctl --user show -p MainPID --value $unit)")
    [[ -n $main && $main != sleep ]] || fail "the unit's main process is '$main', not a terminal"
    shot terminal
    in_session systemctl --user stop "$unit"
}

# cc-probe open must end with the component shown, whether it was stopped (cold), already
# shown, or running and hidden (warm). Each screenshot is named after what it must show.
stage_openers() {
    local opener name program hide
    for opener in \
        "launcher|com.system76.CosmicLauncher|cosmic-launcher|org.freedesktop.DbusActivation ActivateAction sasa{sv} '\"Close\"' 0 0" \
        "app-library|com.system76.CosmicAppLibrary|cosmic-app-library|org.freedesktop.DbusActivation ActivateAction sasa{sv} '\"Close\"' 0 0" \
        "workspaces|com.system76.CosmicWorkspaces|cosmic-workspaces|com.system76.CosmicWorkspaces Hide"; do
        IFS='|' read -r opener name program hide <<< "$opener"
        # Cold: the component is not running, so the opener starts it first. Neither -x
        # nor -f: comm is truncated to 15 bytes for two of these names, and -f matches the
        # in_session shell's own "bash -c ..." command line, which carries the pattern and
        # kills the SSH session's shell instead. pidof matches argv[0]'s basename, so it
        # has no comm limit and never matches the shell.
        in_session "if pids=\$(pidof $program); then kill \$pids; fi"
        wait_until 5 unowned "$name" || fail "$name is still owned after kill"
        in_session cc-probe open "$opener" | grep -q '^{"opened"' || fail "cc-probe open $opener"
        owned "$name" || fail "$name is not owned after open"
        shot "$opener-shown-after-cold-open"
        in_session cc-probe open "$opener" | grep -q '^{"opened"' || fail "cc-probe open $opener, shown"
        shot "$opener-still-shown-after-second-open"
        # Warm: the component closes itself through its own interface and keeps running.
        # cosmic-app-library ignores a request to show within 100 ms of hiding, which is
        # how a click on its panel button closes it; the pause keeps this step out of it.
        in_session busctl --user call "$name" "/${name//.//}" "$hide" ||
            fail "$name did not hide"
        sleep 1
        owned "$name" || fail "$name exited when it hid"
        in_session cc-probe open "$opener" | grep -q '^{"opened"' || fail "cc-probe open $opener, warm"
        shot "$opener-shown-after-warm-open"
        in_session "if pids=\$(pidof $program); then kill \$pids; fi"
    done
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
mkdir -p "$SHOTS"
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
