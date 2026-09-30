#!/usr/bin/env bash
# bar-acceptance.sh [stage...]
# Package 2b.2 of docs/architecture/doc_bar.md in the dev VM's real session, under the real
# unit file and the real user manager: the bar starts as a Type=notify unit, its
# confinement leaves glycin's image sandbox working, it stays within its memory budget
# (section 5, item 17), high contrast reaches COSMIC's theme from inside it (BR3),
# the system modules reach NetworkManager, pipewire-pulse, BlueZ and UPower from inside
# it and hide what the VM lacks (package 2b.4), an output that comes and goes never
# restarts the process or leaks a surface, and a crash loop falls back to the vendor
# layout (SH8).
# athanor-shelld is masked for the run: COSMIC owns its bus names in this session.
# Deploys the binary, the unit and the vendor favourites from .scratch/shell-rig/bin and
# forge/specs/athanor-bar (build the binary with forge/test/shell/rig.sh build-bar). With no
# argument it runs every stage in order; with arguments, only those, in the order given.
# Prints PASS <stage> or FAIL <stage>: <what was read>, and exits non-zero on the first
# failure. Cleanup always runs on exit, through a trap. Screenshots go to
# .scratch/bar-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
DATA=$ROOT/forge/specs/athanor-bar/athanor-bar-1.0.0/data
SHOTS=$ROOT/.scratch/bar-acceptance
PSS_LIMIT_KB=$((64 * 1024))
STAGES=(deploy unit memory modules high-contrast hotplug crash-loop cleanup)
STAGE=
CLEANED=0

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

unit() { in_session systemctl --user "$@" athanor-bar; }
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
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-bar/failures"'
}

fresh_start() {
    if loaded athanor-bar; then
        unit stop || fail "systemctl --user stop athanor-bar"
    fi
    if unit_failed; then
        unit reset-failed || fail "systemctl --user reset-failed athanor-bar"
    fi
    clear_failures || fail "cannot clear the crash-loop record"
    unit start || fail "systemctl --user start athanor-bar: $(unit show -p Result --value)"
}

stage_deploy() {
    mkdir -p "$SHOTS"
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$DATA/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$DATA/favorites.toml:/usr/share/athanor/favorites.toml" > /dev/null
    in_session systemctl --user mask --runtime athanor-shelld.service > /dev/null ||
        fail "systemctl --user mask --runtime athanor-shelld.service"
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-bar.service found no unit"
    # libpulse and its GLib main loop come with the RPM's automatic dependencies; a binary
    # deployed by hand needs them in the image already.
    local libraries
    libraries=$(in_session ldd /usr/bin/athanor-bar) || fail "ldd /usr/bin/athanor-bar failed in the guest"
    if grep -q 'not found' <<< "$libraries"; then
        fail "libraries missing from the guest image: $(grep 'not found' <<< "$libraries")"
    fi
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
    journal=$(in_session "journalctl --user -u athanor-bar --since @$since --no-pager -o cat")
    if grep -qEi "$pattern" <<< "$journal"; then
        fail "sandbox errors in the journal: $(grep -Ei "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after 5 s: $(unit show -p Result --value)"
    runtime_athanor_writable || fail "mkdir/rmdir under %t/athanor failed inside the unit's own mount namespace"
    local config dir
    # shellcheck disable=SC2016 # expanded by the guest's shell
    config=$(in_session 'echo "${XDG_CONFIG_HOME:-$HOME/.config}"')
    for dir in athanor cosmic/com.system76.CosmicTheme.Dark/v1 cosmic/com.system76.CosmicTheme.Light/v1; do
        config_writable "$config/$dir" ||
            fail "creating and removing a file in $config/$dir failed inside the unit's own mount namespace"
    done
}

# Proves that %t/athanor is writable from *inside* athanor-bar.service's own confinement,
# not just from the host: nsenter --mount joins the unit's own mount namespace (what
# ProtectSystem=strict actually builds, and where RuntimeDirectory=athanor-bar athanor
# either does or does not make %t/athanor writable), so a plain SSH mkdir on the host proves
# nothing about it. --setuid/--setgid drop nsenter's root back to the session user before the
# mkdir, since RuntimeDirectoryMode=0700 makes the directory owned by that user, not root.
# This is what launch() (athanor-compositor-client) relies on for a started application's
# Wayland security context.
runtime_athanor_writable() {
    local uid
    uid=$(in_session id -u)
    in_unit_namespace "mkdir \"/run/user/$uid/athanor/bar-acceptance-$$\" &&
        rmdir \"/run/user/$uid/athanor/bar-acceptance-$$\""
}

# The same proof for the directories ConfigurationDirectory= binds read-write under
# ProtectHome=read-only: the favourites file (BR7) and COSMIC's is_high_contrast key in
# both theme modes (BR3). A file, not a directory, since that is what the bar writes there.
config_writable() { # config_writable DIR
    in_unit_namespace "touch \"$1/bar-acceptance-$$\" && rm \"$1/bar-acceptance-$$\""
}

# Runs a shell command as the session user inside athanor-bar.service's mount namespace.
in_unit_namespace() { # in_unit_namespace SHELL-COMMAND
    local pid uid
    pid=$(unit show -p MainPID --value)
    uid=$(in_session id -u)
    guest_ssh "sudo nsenter --target $pid --mount --setuid=$uid --setgid=$uid -- sh -c '$1'"
}

stage_memory() {
    [[ $(unit is-active) == active ]] || fresh_start
    # At rest: the bar has drawn, and no popover is open.
    sleep 10
    local pid pss
    pid=$(unit show -p MainPID --value)
    pss=$(in_session "awk '/^Pss:/ { print \$2 }' /proc/$pid/smaps_rollup")
    echo "memory: athanor-bar PSS $pss kB"
    [[ $pss =~ ^[0-9]+$ ]] || fail "Pss '$pss' from /proc/$pid/smaps_rollup"
    ((pss <= PSS_LIMIT_KB)) || fail "PSS $pss kB is above $PSS_LIMIT_KB kB (item 17)"
}

# The system modules against the VM's real services, under the real unit: libpulse reaches
# pipewire-pulse, NetworkManager accepts the secret agent, and the modules without hardware
# hide (bar_modules.py). The journal must show no refusal from a service.
stage_modules() {
    local since
    since=$(in_session date +%s)
    fresh_start
    in_session python3 - < "$HERE/bar_modules.py" || fail "a module did not reach its service (steps above)"
    local journal pattern='a system service refused or did not answer|refused the pairing agent|cannot export|does not parse|no GLib main loop'
    journal=$(in_session "journalctl --user -u athanor-bar --since @$since --no-pager -o cat")
    if grep -qE "$pattern" <<< "$journal"; then
        fail "refusals in the journal: $(grep -E "$pattern" <<< "$journal")"
    fi
    [[ $(unit is-active) == active ]] || fail "not active after the modules: $(unit show -p Result --value)"
    # Recorded, not judged: libpulse warns on every connect that it cannot create its cookie
    # under ProtectHome and Landlock, which pipewire-pulse does not need; a refused connection
    # is a different line. The polkit default decides whether joining a new network prompts.
    local notes="$SHOTS/modules-notes.txt"
    {
        echo "libpulse cookie warnings: $(grep -ci 'cookie' <<< "$journal")"
        echo "libpulse refused connections: $(grep -ciE 'connection refused|access denied|connection terminated' <<< "$journal")"
        echo "polkit, org.freedesktop.NetworkManager.settings.modify.system:"
        in_session pkaction --verbose --action-id org.freedesktop.NetworkManager.settings.modify.system |
            grep -E '^\s*implicit (any|inactive|active):'
    } > "$notes" || fail "could not record the modules' notes"
    cat "$notes"
    "$HERE/screenshot.sh" "$SHOTS/modules.png" > /dev/null
}

# Switches high contrast on and off through the bar's own menu over AT-SPI; the bar writes
# the key from inside its confinement (bar_high_contrast.py).
stage_high-contrast() {
    [[ $(unit is-active) == active ]] || fresh_start
    in_session python3 - < "$HERE/bar_high_contrast.py" ||
        fail "the switch did not reach COSMIC's is_high_contrast (steps above)"
    [[ $(unit is-active) == active ]] || fail "not active after switching: $(unit show -p Result --value)"
}

HEAD2=/sys/class/drm/card1-Virtual-2/status

# The second virtio head: status on or off, then a change uevent, which cosmic-comp needs
# to see the output come or go (the forced status alone raises none).
second_head() { # second_head on|off|detect
    guest_ssh "echo $1 | sudo tee $HEAD2 > /dev/null && sudo udevadm trigger --action=change /sys/class/drm/card1"
}
surfaces_are() { [[ $(in_session python3 - < "$HERE/bar_surfaces.py") == "$1" ]]; }

# An output that comes and goes, three times: the bar keeps its process, draws one populated
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
        wait_until 15 surfaces_are 2 || fail "cycle $cycle: $(in_session python3 - < "$HERE/bar_surfaces.py") populated surfaces with two outputs"
        second_head off
        wait_until 15 surfaces_are 1 || fail "cycle $cycle: $(in_session python3 - < "$HERE/bar_surfaces.py") populated surfaces with one output"
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
    # on the vendor layout, so the shell is never lost.
    wait_until 10 in_session "journalctl --user -u athanor-bar --since @$since -p err --no-pager -o cat |
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
            echo "cleanup: systemctl --user reset-failed athanor-bar failed" >&2
            failed=1
        }
    fi
    if loaded athanor-bar; then
        unit stop || {
            echo "cleanup: systemctl --user stop athanor-bar failed" >&2
            failed=1
        }
    fi
    clear_failures || {
        echo "cleanup: removing the crash-loop record failed" >&2
        failed=1
    }
    in_session systemctl --user unmask --runtime athanor-shelld.service > /dev/null || {
        echo "cleanup: systemctl --user unmask --runtime athanor-shelld.service failed" >&2
        failed=1
    }
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
trap cleanup_on_exit EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
