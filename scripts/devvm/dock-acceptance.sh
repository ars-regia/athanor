#!/usr/bin/env bash
# dock-acceptance.sh [stage...]
# Package 2c of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager: the dock starts as a Type=notify unit, its
# confinement leaves glycin's image sandbox working and the favourites file writable, it
# stays within its memory budget (section 5, item 17), a pinned entry pressed on the dock
# starts behind a Wayland security context in its own transient unit (item 8), an output
# that comes and goes never restarts the process or leaks a surface, and a crash loop falls
# back to the vendor layout (SH8).
# Deploys the binary and the unit from .scratch/shell-rig/bin and forge/specs/athanor-dock,
# and the vendor favourites from forge/specs/athanor-bar (the dock requires the bar's
# package, which ships them). Build the binary with forge/test/shell/rig.sh build-dock.
# With no argument it runs every stage in order; with arguments, only those, in the order
# given. Prints PASS <stage> or FAIL <stage>: <what was read>, and exits non-zero on the
# first failure. Cleanup always runs on exit, through a trap, and leaves the unit as it
# found it: running when it was running at the start, as on an image that ships it, stopped
# otherwise. Screenshots go to .scratch/dock-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
DATA=$ROOT/forge/specs/athanor-dock/athanor-dock-1.0.0/data
BAR_DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHOTS=$ROOT/.scratch/dock-acceptance
PSS_LIMIT_KB=$((48 * 1024))
STAGES=(deploy unit memory launch hotplug crash-loop cleanup)
STAGE=
STATE_BEFORE=
CLEANED=0
# The launch stage's entry, and the file its wayland-info writes the globals it sees to.
LAUNCH_ID=os.athanor.DockAcceptanceWaylandInfo
GLOBALS=/tmp/athanor-dock-acceptance-globals
# Globals only the main socket offers: an application behind the context sees none of them
# (the list compositor-acceptance.sh checks).
PRIVILEGED=(zcosmic_toplevel_info_v1 zcosmic_toplevel_manager_v1 ext_workspace_manager_v1
    zcosmic_workspace_manager_v2 zwlr_layer_shell_v1 ext_data_control_manager_v1
    zwlr_data_control_manager_v1 wp_security_context_manager_v1
    zcosmic_keyboard_layout_manager_v1 cosmic_a11y_manager_v1)

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

unit() { in_session systemctl --user "$@" athanor-dock; }
loaded() { [[ $(in_session systemctl --user show -p LoadState --value "$1") == loaded ]]; }
unit_failed() { [[ $(unit show -p ActiveState --value) == failed ]]; }

new_main_pid() { # new_main_pid OLD-PID: active, with a different, real MainPID
    [[ $(unit show -p ActiveState --value) == active ]] || return 1
    local pid
    pid=$(unit show -p MainPID --value)
    [[ $pid != "$1" && $pid != 0 ]]
}

# The record of the crash loop survives a stop (RuntimeDirectoryPreserve=yes): a stage that
# needs a clean start clears it.
clear_failures() {
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-dock/failures"'
}

fresh_start() {
    if loaded athanor-dock; then
        unit stop || fail "systemctl --user stop athanor-dock"
    fi
    if unit_failed; then
        unit reset-failed || fail "systemctl --user reset-failed athanor-dock"
    fi
    clear_failures || fail "cannot clear the crash-loop record"
    unit start || fail "systemctl --user start athanor-dock: $(unit show -p Result --value)"
}

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-dock:/usr/bin/athanor-dock" \
        "$DATA/athanor-dock.service:/usr/lib/systemd/user/athanor-dock.service" \
        "$BAR_DATA/favorites.toml:/usr/share/athanor/favorites.toml" > /dev/null
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-dock.service found no unit"
}

stage_unit() {
    local since
    since=$(in_session date +%s)
    fresh_start
    # Type=notify: start returns only after READY=1, so active here means READY was sent.
    [[ $(unit is-active) == active ]] || fail "is-active: $(unit is-active)"
    sleep 5
    "$HERE/screenshot.sh" "$SHOTS/unit.png" > /dev/null
    # glycin decodes icons in a bubblewrap sandbox; a syscall or an address family the
    # unit forbids shows up here, and as blank icons in unit.png.
    local journal pattern='glycin|bwrap|bubblewrap|seccomp|operation not permitted|SIGSYS'
    journal=$(in_session "journalctl --user -u athanor-dock --since @$since --no-pager -o cat")
    if grep -qEi "$pattern" <<< "$journal"; then
        fail "sandbox errors in the journal: $(grep -Ei "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after 5 s: $(unit show -p Result --value)"
    runtime_athanor_writable || fail "mkdir/rmdir under %t/athanor failed inside the unit's own mount namespace"
    local config
    # shellcheck disable=SC2016 # expanded by the guest's shell
    config=$(in_session 'echo "${XDG_CONFIG_HOME:-$HOME/.config}"')
    config_writable "$config/athanor" ||
        fail "creating and removing a file in $config/athanor failed inside the unit's own mount namespace"
}

# Proves that %t/athanor is writable from inside athanor-dock.service's own confinement,
# where launch() (athanor-compositor-client) creates a started application's Wayland
# security context: nsenter --mount joins the unit's mount namespace, and --setuid/--setgid
# drop to the session user, who owns the directory (RuntimeDirectoryMode=0700).
runtime_athanor_writable() {
    local uid
    uid=$(in_session id -u)
    in_unit_namespace "mkdir \"/run/user/$uid/athanor/dock-acceptance-$$\" &&
        rmdir \"/run/user/$uid/athanor/dock-acceptance-$$\""
}

# The same proof for the directory ConfigurationDirectory= binds read-write under
# ProtectHome=read-only, where the dock writes the favourites file (BR7).
config_writable() { # config_writable DIR
    in_unit_namespace "touch \"$1/dock-acceptance-$$\" && rm \"$1/dock-acceptance-$$\""
}

# Runs a shell command as the session user inside athanor-dock.service's mount namespace.
in_unit_namespace() { # in_unit_namespace SHELL-COMMAND
    local pid uid
    pid=$(unit show -p MainPID --value)
    uid=$(in_session id -u)
    guest_ssh "sudo nsenter --target $pid --mount --setuid=$uid --setgid=$uid -- sh -c '$1'"
}

# The session user's favourites file, which the dock and the bar share.
favorites_path() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    in_session 'echo "${XDG_CONFIG_HOME:-$HOME/.config}/athanor/favorites.toml"'
}

# The application units the session holds, one per line, sorted.
app_units() {
    in_session "systemctl --user list-units --all --plain --no-legend 'app-athanor-*' | cut -d' ' -f1 | sort"
}
# The application units that were not among BEFORE, a list app_units printed earlier.
new_units() { comm -13 <(printf '%s\n' "$1") <(app_units); }
has_new_unit() { [[ -n $(new_units "$1") ]]; }

# Item 8 from the dock: a pinned entry pressed on the dock starts in its own transient unit,
# behind a security context that offers none of the privileged globals.
stage_launch() {
    local file before launched seen main global environment display
    file=$(favorites_path)
    [[ $(unit is-active) == active ]] || fresh_start
    in_session test -e "$file" || fail "no favourites file at $file: the dock writes it on its first start"
    ! in_session test -e "$file.dock-acceptance" ||
        fail "$file.dock-acceptance is left from an earlier run: restore it by hand first"
    # The session's favourites are kept aside; cleanup puts them back.
    in_session cp -p "$file" "$file.dock-acceptance"
    printf '%s\n' '[Desktop Entry]' 'Type=Application' "Name=$LAUNCH_ID" \
        "Exec=sh -c \"wayland-info > $GLOBALS; exec sleep 600\"" |
        in_session "mkdir -p ~/.local/share/applications && cat > ~/.local/share/applications/$LAUNCH_ID.desktop"
    printf 'schema = 1\nfavorites = ["%s.desktop"]\n' "$LAUNCH_ID" | in_session "cat > $file"
    in_session rm -f "$GLOBALS"
    # A restart reads the new entry and the favourites at start, not through a monitor.
    fresh_start
    before=$(app_units)
    in_session python3 - athanor-dock "$LAUNCH_ID" < "$HERE/dock_press.py" ||
        fail "no pinned $LAUNCH_ID button on the dock"
    wait_until 10 has_new_unit "$before" || fail "no new application unit after the press"
    launched=$(new_units "$before")
    [[ $launched =~ ^app-athanor-os\.athanor\.DockAcceptanceWaylandInfo@[0-9a-f]{32}\.service$ ]] ||
        fail "unit name '$launched'"
    wait_until 10 in_session "grep -q \"^interface: 'wl_compositor'\" $GLOBALS" ||
        fail "no wayland-info output in $GLOBALS for $launched"
    seen=$(in_session cat "$GLOBALS" | sed -n "s/^interface: '\([a-z0-9_]*\)'.*/\1/p")
    main=$(in_session wayland-info | grep -c '^interface: ')
    (($(wc -l <<< "$seen") < main)) || fail "$(wc -l <<< "$seen") globals behind the context, $main on the main socket"
    for global in "${PRIVILEGED[@]}"; do
        if grep -qx "$global" <<< "$seen"; then
            fail "$global is offered behind the context"
        fi
    done
    environment=$(in_session systemctl --user show -p Environment --value "$launched")
    display=$(tr ' ' '\n' <<< "$environment" | sed -n 's/^WAYLAND_DISPLAY=//p')
    [[ $display =~ ^/run/user/[0-9]+/athanor/[0-9a-f]{32}/wayland$ ]] || fail "WAYLAND_DISPLAY '$display'"
    in_session systemctl --user stop "$launched"
}

stage_memory() {
    [[ $(unit is-active) == active ]] || fresh_start
    # At rest: the dock has drawn, and no menu is open.
    sleep 10
    local pid pss
    pid=$(unit show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-dock PSS $pss kB"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 17)"
}

HEAD2=/sys/class/drm/card1-Virtual-2/status

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}
surfaces() { in_session python3 - athanor-dock < "$HERE/bar_surfaces.py"; }
surfaces_are() { [[ $(surfaces) == "$1" ]]; }

# An output that comes and goes, three times: the dock keeps its process, draws one populated
# surface per output, and never destroys a departed output's surface (cosmic-comp 1.8.0
# closes the connection of a client that does).
stage_hotplug() {
    guest_ssh "test -e $HEAD2" || fail "one head: start the dev VM with GPU_OUTPUTS=2 (devvm.env)"
    [[ $(unit is-active) == active ]] || fresh_start
    local pid restarts cycle
    pid=$(unit show -p MainPID --value)
    restarts=$(unit show -p NRestarts --value)
    for cycle in 1 2 3; do
        second_head on
        wait_until 15 surfaces_are 2 || fail "cycle $cycle: $(surfaces) populated surfaces with two outputs"
        second_head off
        wait_until 15 surfaces_are 1 || fail "cycle $cycle: $(surfaces) populated surfaces with one output"
        [[ $(unit show -p MainPID --value) == "$pid" ]] || fail "cycle $cycle: MainPID changed from $pid"
    done
    [[ $(unit show -p NRestarts --value) == "$restarts" ]] || fail "NRestarts went from $restarts to $(unit show -p NRestarts --value)"
    [[ $(unit is-active) == active ]] || fail "not active after hotplug: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/hotplug.png" > /dev/null
}

stage_crash-loop() {
    # SIGKILL, not SIGSEGV: std's stack-overflow handler swallows a SIGSEGV sent by kill(2)
    # (see shelld-acceptance.sh). SH8 counts failures, not which signal caused them.
    local since
    since=$(in_session date +%s)
    fresh_start
    local round pid
    for round in 1 2 3 4 5; do
        pid=$(unit show -p MainPID --value)
        unit kill --kill-whom=main -s SIGKILL
        wait_until 90 new_main_pid "$pid" || fail "round $round: no new MainPID after killing $pid"
    done
    # The sixth start is the one past five failures in the window: it logs at err and runs
    # on the vendor layout, so the dock is never lost.
    wait_until 10 in_session "journalctl --user -u athanor-dock --since @$since -p err --no-pager -o cat |
        grep -q 'keeps failing'" || fail "no 'keeps failing' at err priority after five kills"
    [[ $(unit is-active) == active ]] || fail "not active after the give-up: $(unit show -p Result --value)"
    "$HERE/screenshot.sh" "$SHOTS/crash-loop.png" > /dev/null
}

stage_cleanup() {
    CLEANED=1
    local failed=0
    if guest_ssh "test -e $HEAD2"; then
        second_head detect || {
            echo "cleanup: restoring $HEAD2 to detect failed" >&2
            failed=1
        }
    fi
    if unit_failed; then
        unit reset-failed || {
            echo "cleanup: systemctl --user reset-failed athanor-dock failed" >&2
            failed=1
        }
    fi
    if loaded athanor-dock; then
        unit stop || {
            echo "cleanup: systemctl --user stop athanor-dock failed" >&2
            failed=1
        }
    fi
    clear_failures || {
        echo "cleanup: removing the crash-loop record failed" >&2
        failed=1
    }
    local file
    file=$(favorites_path)
    if in_session test -e "$file.dock-acceptance"; then
        in_session mv "$file.dock-acceptance" "$file" || {
            echo "cleanup: restoring $file from $file.dock-acceptance failed" >&2
            failed=1
        }
    fi
    in_session "rm -f $GLOBALS ~/.local/share/applications/$LAUNCH_ID.desktop" || {
        echo "cleanup: removing the launch stage's entry and globals failed" >&2
        failed=1
    }
    if [[ $STATE_BEFORE == active ]]; then
        unit start || {
            echo "cleanup: systemctl --user start athanor-dock failed; it was running before the run" >&2
            failed=1
        }
    fi
    return "$failed"
}

cleanup_on_exit() {
    ((CLEANED)) || {
        STAGE=cleanup
        stage_cleanup
    }
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
STATE_BEFORE=$(unit show -p ActiveState --value)
trap cleanup_on_exit EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
