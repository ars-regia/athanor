#!/usr/bin/env bash
# session-acceptance.sh [stage...]
# The two criteria of issue #144 that no test of the repository reaches, in the dev VM's real
# session: the volume keys and a Flatpak application's Open and Save. Keys reach the compositor
# through a throwaway uinput keyboard (keyboard_type.py). With no argument it runs every stage
# in order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/session-acceptance/; look at them. The polkit criterion of #144 is
# `lock-acceptance.sh polkit`. Log in at the greeter first.
#
#   volume  cosmic-settings-daemon runs and has bound its varlink socket, the one its unit's
#           `ReadWritePaths=%t` makes writable (#198); the image binds XF86AudioRaiseVolume to
#           the system action VolumeRaise; then, on a null sink made in the guest with pactl
#           (the VM has no sound device) and set as the default at 40 %, three presses of the
#           volume-up key raise the volume that `wpctl get-volume @DEFAULT_AUDIO_SINK@` reads
#           and three of volume-down lower it again. The daemon's user journal holds no
#           "panicked" from the start of the stage.
#   portal  org.gnome.TextEditor, a Flatpak from Flathub, opens an existing file of the home
#           directory through the file chooser (Ctrl+O, Ctrl+L, the path, Enter), which
#           athanor-portals.conf routes to xdg-desktop-portal-gtk (#171); the document portal
#           exports it under $XDG_RUNTIME_DIR/doc/by-app/<app id>/<document id>/; a character
#           typed and saved with Ctrl+S changes the file on disk, so the grant is writable.
#
# What this does not show. No log line marks the chooser as drawn: the stage waits a fixed time
# after Ctrl+O, and the screenshots are the only check of what it looks like. The volume keys
# are pressed on the compositor's default shortcuts: a user's own binding of the key in
# ~/.config/cosmic/com.system76.CosmicSettings.Shortcuts would change what happens, and the
# stage does not read it.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

SHOTS=$ROOT/.scratch/session-acceptance
STAGES=(volume portal)
STAGE=
DAEMON=cosmic-settings-daemon.service
# cosmic-settings-daemon's varlink audio service binds this name in the runtime directory
# (doc_session_daemons.md, SD1; doc_osd.md, section 1). Expanded by the guest's shell.
# shellcheck disable=SC2016 # expanded by the guest's shell
SOCKET='$XDG_RUNTIME_DIR/com.system76.CosmicSettings'
SHORTCUTS=/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults
SINK=session_acceptance
APP=org.gnome.TextEditor
EDITOR_UNIT=session-acceptance-editor
# shellcheck disable=SC2016 # expanded by the guest's shell
DOCS='$XDG_RUNTIME_DIR/doc'
PORTALS_CONF=/usr/share/xdg-desktop-portal/athanor-portals.conf
CLEANUP=()

# Runs a command as the session user, with the session's bus and compositor.
in_session() {
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; $*"
}

# Polls COMMAND once a second until it succeeds; returns 1 after SECONDS. What COMMAND writes
# to stderr goes through, so a broken SSH connection shows beside the FAIL line.
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@"; do
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
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null || echo "note: no screenshot $1 (grim refused)" >&2
}

type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }

# Undoes what the stages changed, on every exit, a failed one included. A step that fails is
# named, and a run that passed fails: it left the guest changed.
cleanup() {
    local status=$? step failed=0
    for step in "${CLEANUP[@]}"; do
        "$step" || { echo "FAIL cleanup step $step: the guest keeps what it undoes" >&2; failed=1; }
    done
    ((status != 0 || failed == 0)) || exit 1
}
trap cleanup EXIT

USER_NAME=$(guest_ssh id -un)
SESSION=$(guest_ssh loginctl show-user "$USER_NAME" -p Display --value) || SESSION=
[[ -n $SESSION ]] || fail "no graphical session for $USER_NAME: log in at the greeter first"


# The default sink's volume in whole percent; nothing when wpctl or SSH fails.
volume() {
    in_session "wpctl get-volume @DEFAULT_AUDIO_SINK@" | awk '$1 == "Volume:" { printf "%d\n", $2 * 100 + 0.5 }'
}
volume_is() {
    local v
    v=$(volume) && [[ -n $v && $v -eq $1 ]]
} # volume_is PERCENT
volume_above() {
    local v
    v=$(volume) && [[ -n $v && $v -gt $1 ]]
} # volume_above PERCENT
volume_below() {
    local v
    v=$(volume) && [[ -n $v && $v -lt $1 ]]
} # volume_below PERCENT

default_sink_is() { [[ $(in_session pactl get-default-sink) == "$1" ]]; }

PREVIOUS_SINK=
NULL_MODULE=
# Runs after unload_sink. A guest without audio hardware has only WirePlumber's fallback
# sink auto_null, which is gone while the null sink exists: setting it would fail, so the
# choice set-default-sink stored is cleared and WirePlumber picks the default again.
restore_sink() {
    [[ -n $PREVIOUS_SINK ]] || return 0
    if in_session pactl list short sinks | awk '{ print $2 }' | grep -qxF -- "$PREVIOUS_SINK"; then
        in_session pactl set-default-sink "$PREVIOUS_SINK"
    else
        in_session pw-metadata -n default 0 default.configured.audio.sink -d
    fi
}
unload_sink() {
    [[ -z $NULL_MODULE ]] || in_session pactl unload-module "$NULL_MODULE"
}

stage_volume() {
    local since up down log
    since=$(guest_ssh date +%s)
    [[ $(in_session systemctl --user is-active $DAEMON) == active ]] || fail "$DAEMON is not active"
    in_session "test -S $SOCKET" || fail "no socket $SOCKET: the daemon's audio service did not bind (ReadWritePaths=%t, #144)"
    # cosmic-comp runs the system action for the key (doc_osd.md, section 1): its defaults bind
    # the key to VolumeRaise, and the system_actions map names the command that action runs.
    in_session "grep -F XF86AudioRaiseVolume $SHORTCUTS" | grep -qF VolumeRaise ||
        fail "$SHORTCUTS does not bind XF86AudioRaiseVolume to VolumeRaise"
    in_session "grep -h VolumeRaise /usr/share/cosmic/*/v1/system_actions" | grep -q . ||
        fail "no system_actions file under /usr/share/cosmic names VolumeRaise"
    # Given a command, cosmic-comp runs as a kiosk and loads no key bindings at all, unless
    # Athanor's patch is told to keep them (#510).
    in_session "pgrep -a -x cosmic-comp" | grep -qF -- --keep-shortcuts ||
        fail "the session's cosmic-comp runs without --keep-shortcuts: it loads no key bindings (#510)"

    PREVIOUS_SINK=$(in_session pactl get-default-sink) || fail "pactl get-default-sink: no PulseAudio server in the session"
    CLEANUP+=(unload_sink restore_sink)
    NULL_MODULE=$(in_session pactl load-module module-null-sink "sink_name=$SINK") || fail "pactl could not load a null sink"
    in_session pactl set-default-sink "$SINK" || fail "pactl set-default-sink $SINK"
    wait_until 10 default_sink_is "$SINK" || fail "the default sink is not $SINK 10 s after set-default-sink"
    in_session wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.40 || fail "wpctl set-volume"
    wait_until 10 volume_is 40 || fail "the default sink's volume is '$(volume)' %, not 40, 10 s after wpctl set-volume"
    # The daemon follows the default sink through PipeWire's events; no log line says it has.
    sleep 2
    shot volume-before

    type_keys '@volup*3:0.5' || fail "keyboard_type.py could not press volume up"
    wait_until 10 volume_above 40 || fail "the volume is '$(volume)' % 10 s after three volume-up presses, from 40: the keys did not reach the daemon"
    up=$(volume)
    shot volume-up
    type_keys '@voldown*3:0.5' || fail "keyboard_type.py could not press volume down"
    wait_until 10 volume_below "$up" || fail "the volume is '$(volume)' % 10 s after three volume-down presses, from $up"
    down=$(volume)
    shot volume-down
    echo "volume: 40 -> $up -> $down"

    # A journal that cannot be read fails the stage: it must not read as one without a panic.
    log=$(guest_ssh sudo journalctl -q -b --since "@$since" -o cat "_SYSTEMD_USER_UNIT=$DAEMON") ||
        fail "cannot read $DAEMON's journal"
    if grep -F panicked <<< "$log"; then
        fail "$DAEMON panicked since the stage started"
    fi
}

# Flatpak moves the application into a scope of its own: stopping $EDITOR_UNIT stops only
# `flatpak run`, and a surviving instance takes over the next run's window.
stop_editor() {
    local apps
    apps=$(in_session flatpak ps --columns=application) || return 1
    grep -qx "$APP" <<< "$apps" || return 0
    in_session flatpak kill "$APP"
}
reset_override() { in_session flatpak override --user --reset "$APP"; }
remove_document() { in_session "rm -f $DOCUMENT"; }

editor_runs() {
    local apps
    apps=$(in_session flatpak ps --columns=application) && grep -qx "$APP" <<< "$apps"
}

# Whether the document portal lists the document under the application's directory.
# The pattern is double-quoted for the guest, whose shell expands $DOCS's variable.
document_exported() { in_session "compgen -G \"$DOCS/by-app/$APP/*/$NAME\"" > /dev/null; }

stage_portal() {
    local flatpaks home before after
    guest_ssh grep -qx 'org.freedesktop.impl.portal.FileChooser=gtk' "$PORTALS_CONF" ||
        fail "$PORTALS_CONF does not route FileChooser to gtk"
    flatpaks=$(in_session flatpak list --app --columns=application)
    if ! grep -qx "$APP" <<< "$flatpaks"; then
        in_session flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo ||
            fail "flatpak remote-add flathub"
        in_session flatpak install --user -y flathub "$APP" > /dev/null || fail "flatpak install $APP"
    fi

    # shellcheck disable=SC2016 # expanded by the guest's shell
    home=$(guest_ssh 'printf %s "$HOME"')
    # A name of this run: the document portal keeps a record of every document it exported,
    # and a record left by an earlier run would read as this run's export.
    NAME=session-acceptance-$(date +%s).txt
    DOCUMENT=$home/$NAME
    ! editor_runs || fail "$APP already runs: the stage needs an instance it starts (flatpak kill $APP)"
    CLEANUP+=(stop_editor remove_document)
    # Flathub's Text Editor holds --filesystem=host, and the portal exports a chosen file only to
    # an application that cannot already reach it: without this override the file opens by its
    # own path and the document portal is never exercised. The cleanup resets the user override,
    # so the stage refuses to start over one it did not write.
    [[ -z $(in_session flatpak override --user --show "$APP") ]] ||
        fail "$APP has a user override; the stage would replace it (flatpak override --user --show $APP)"
    CLEANUP+=(reset_override)
    in_session flatpak override --user --nofilesystem=host "$APP" || fail "flatpak override --nofilesystem=host $APP"
    in_session "printf 'athanor session acceptance\\n' > $DOCUMENT" || fail "cannot write $DOCUMENT"
    before=$(in_session "sha256sum < $DOCUMENT") || fail "cannot read $DOCUMENT"

    in_session systemd-run --user --collect --unit="$EDITOR_UNIT" flatpak run "$APP" || fail "systemd-run $APP"
    wait_until 60 editor_runs || fail "$APP is not in flatpak ps 60 s after it was started"
    # No log line marks the window as drawn: a fixed settle time, as for the locker's surfaces.
    sleep 8
    shot editor

    type_keys @ctrl+o || fail "keyboard_type.py could not press Ctrl+O"
    sleep 4
    shot chooser
    type_keys @ctrl+l "$DOCUMENT" @enter || fail "keyboard_type.py could not type the path"
    shot path
    # GTK 4.20's chooser in xdg-desktop-portal-gtk keeps the dialog open after the first Enter
    # in its location entry (seen on the dev VM, 2026-10-15); the second opens the file. A
    # second Enter after an opened file would type into the editor, so it is pressed only when
    # the first exported nothing.
    wait_until 5 document_exported || type_keys @enter || fail "keyboard_type.py could not press Enter"
    wait_until 20 document_exported || {
        shot no-export
        fail "no $DOCS/by-app/$APP/*/$NAME 20 s after the path: $(in_session "ls -R $DOCS/by-app 2>&1" | head -n 20 | tr '\n' '|')"
    }
    sleep 2
    shot opened

    type_keys x @ctrl+s || fail "keyboard_type.py could not type and save"
    sleep 2
    shot saved
    after=$(in_session "sha256sum < $DOCUMENT") || fail "cannot read $DOCUMENT after the save"
    [[ $after != "$before" ]] || fail "$DOCUMENT is unchanged after a typed character and Ctrl+S"
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || fail "unknown stage: one of ${STAGES[*]}"
done
mkdir -p "$SHOTS"
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
