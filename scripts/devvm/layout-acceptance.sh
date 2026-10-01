#!/usr/bin/env bash
# layout-acceptance.sh [stage...]
# Acceptance item 10 of docs/architecture/doc_shell.md (the layout) in the dev VM's real
# session: athanor-bar and athanor-dock under systemd --user, the real cosmic-comp,
# rotation. Each check reads the "layout applied" line the bar and the dock log, with the
# layout's Debug form, whenever the layout they draw changes. Deploys the release binaries
# from .scratch/shell-rig/bin (build them with forge/test/shell/rig.sh build-bar and
# build-dock) and the units from forge/specs. With no argument it runs every stage in
# order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. The user's layout
# document and first-session marker are put back on exit, through a trap. Screenshots go
# to .scratch/layout-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
SPECS=$ROOT/forge/specs
SHOTS=$ROOT/.scratch/layout-acceptance
STAGES=(deploy first-session-small first-session-portrait rotation presets-live degrade
    mandatory-mid-session crash-loop)
# Every stage sets the output to this mode; the first-session stages change only scale and
# transform on top of it.
WIDTH=1920 HEIGHT=1080
SURFACES=(athanor-bar athanor-dock)
# The user's files the stages replace, and where they wait to be put back. The guest's
# shell expands the tilde.
# shellcheck disable=SC2088
DOCUMENT='~/.config/athanor/layout.toml'
# shellcheck disable=SC2088
MARKER='~/.local/state/athanor/layout-first-session'
SAVED='.layout-acceptance'
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

# The layout is logged before the surfaces redraw: give them time to draw first.
shot() {
    sleep 2
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null
}

# The first output's name; cosmic-randr colours its list even without a terminal.
output() { in_session cosmic-randr list | sed 's/\x1b\[[0-9;]*m//g' | awk '/^[A-Za-z]/ { print $1; exit }'; }
mode() { # mode [cosmic-randr mode options...]
    in_session cosmic-randr mode "$@" "$(output)" "$WIDTH" "$HEIGHT"
}

now() { in_session date +%s; }

# The last layout UNIT logged since SINCE (seconds since the epoch, guest clock).
last_applied() { # last_applied UNIT SINCE
    in_session "journalctl --user -u $1 --since @$2 -g 'layout applied' -o cat --no-pager | tail -n 1"
}
# The layout UNIT has in force: each surface logs its layout at start and at each change
# only, so the last one it logged this boot. A document that changes nothing logs nothing.
in_force() { # in_force UNIT
    in_session "journalctl --user -u $1 -b -g 'layout applied' -o cat --no-pager | tail -n 1"
}
# Both surfaces have LAYOUT in force, the Debug form of athanor_layout::Layout.
is_applied() { # is_applied LAYOUT
    local surface
    for surface in "${SURFACES[@]}"; do
        [[ $(in_force "$surface") == *"Layout { $1 }"* ]] || return 1
    done
}
# Waits for both surfaces to have LAYOUT in force, or fails the stage with what each logged
# since SINCE, when the document was written.
applied() { # applied SECONDS SINCE LAYOUT
    wait_until "$1" is_applied "$3" ||
        fail "want Layout { $3 }; athanor-bar: $(last_applied athanor-bar "$2"); athanor-dock: $(last_applied athanor-dock "$2")"
}
FLOAT='preset: Float, panel: Top, dock: Visible'
BAR='preset: Bar, panel: Bottom, dock: Off'
MINIMAL='preset: Minimal, panel: Top, dock: Off'

# Replaces the user document in one step, as the chooser does.
write_document() { # write_document TOML
    printf '%s' "$1" | in_session "mkdir -p ~/.config/athanor &&
    cat > ~/.config/athanor/.layout.toml.acc && mv ~/.config/athanor/.layout.toml.acc $DOCUMENT"
}
document() { # document KEY=VALUE... : a schema 1 document with the wildcard output's keys
    local text=$'schema = 1\n[output."*"]\n' pair
    for pair in "$@"; do text+="${pair%%=*} = \"${pair#*=}\""$'\n'; done
    write_document "$text"
}

surfaces() { # surfaces SYSTEMCTL-VERB : on the bar and the dock together
    in_session systemctl --user "$1" "${SURFACES[@]}"
}
# The crash-loop record survives a stop (RuntimeDirectoryPreserve=yes): a clean start
# clears it, and reset-failed clears the start limit, so a rerun still starts.
clear_failures() {
    # shellcheck disable=SC2016 # $XDG_RUNTIME_DIR is expanded by the guest's shell
    in_session 'rm -f "$XDG_RUNTIME_DIR/athanor-bar/failures" "$XDG_RUNTIME_DIR/athanor-dock/failures"'
}
login() {
    surfaces stop
    clear_failures
    surfaces reset-failed
    surfaces start
}
# A user who never had a session: no layout document and no first-session marker.
new_user() {
    surfaces stop
    in_session "rm -f $DOCUMENT $MARKER"
}

# The user's document and marker, moved aside before the first stage that replaces them.
save_user() {
    in_session "set -e; for file in $DOCUMENT $MARKER; do
        if [ -e \"\$file\" ] && [ ! -e \"\$file$SAVED\" ]; then cp -p \"\$file\" \"\$file$SAVED\"; fi
    done"
}
# Puts them back, or removes what the stages wrote where the user had nothing, then
# restarts the surfaces on them.
restore_user() {
    local status=$?
    trap - EXIT
    local failed=0
    in_session "set -e; for file in $DOCUMENT $MARKER; do
        if [ -e \"\$file$SAVED\" ]; then mv \"\$file$SAVED\" \"\$file\"; else rm -f \"\$file\"; fi
    done" || {
        echo "cleanup: restoring the layout document and the first-session marker failed" >&2
        failed=1
    }
    login || {
        echo "cleanup: restarting athanor-bar and athanor-dock failed" >&2
        failed=1
    }
    ((failed == 0)) || exit 1
    exit "$status"
}

stage_deploy() {
    "$HERE/deploy.sh" \
        "$BIN/athanor-bar:/usr/bin/athanor-bar" \
        "$BIN/athanor-dock:/usr/bin/athanor-dock" \
        "$BIN/athanor-layout-chooser:/usr/bin/athanor-layout-chooser" \
        "$SPECS/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service:/usr/lib/systemd/user/athanor-bar.service" \
        "$SPECS/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service:/usr/lib/systemd/user/athanor-dock.service" \
        "$ROOT/system/athanor-layout/vendor/10-athanor.toml:/usr/share/athanor/layout/10-athanor.toml" > /dev/null
    in_session systemctl --user daemon-reload
    local surface
    for surface in "${SURFACES[@]}"; do
        in_session systemctl --user cat "$surface" > /dev/null || fail "systemctl --user cat $surface found no unit"
    done
}

first_session() { # first_session PRESET LAYOUT SINCE : the pick, its marker, the drawing
    wait_until 10 in_session "grep -qx 'preset = \"$1\"' $DOCUMENT" ||
        fail "layout.toml: $(in_session "cat $DOCUMENT" 2>&1)"
    [[ $(in_session "cat $MARKER") == "$1" ]] || fail "marker: $(in_session "cat $MARKER" 2>&1)"
    applied 10 "$3" "$2"
}

stage_first-session-small() {
    new_user
    mode --scale 2 --transform normal
    local since
    since=$(now)
    login
    first_session bar "$BAR" "$since"
    shot first-session-small
    mode --scale 1 --transform normal
}

stage_first-session-portrait() {
    new_user
    mode --scale 1 --transform rotate90
    local since
    since=$(now)
    login
    first_session float "$FLOAT" "$since"
    shot first-session-portrait
    mode --scale 1 --transform normal
}

pid_of() { in_session systemctl --user show -p MainPID --value "$1"; }

# The layout stays the same across a rotation, so nothing is logged: the check is that
# the compositor and both surfaces live through it; which edge the dock takes in each
# shape is athanor_layout::placement's, under its unit tests. The screenshots show it.
stage_rotation() {
    local since
    since=$(now)
    document preset=float panel=bottom
    applied 10 "$since" 'preset: Float, panel: Bottom, dock: Visible'
    local comp bar dock
    comp=$(in_session pidof cosmic-comp)
    bar=$(pid_of athanor-bar)
    dock=$(pid_of athanor-dock)
    shot rotation-landscape
    mode --scale 1 --transform rotate90
    shot rotation-portrait
    [[ $(in_session pidof cosmic-comp) == "$comp" ]] || fail "cosmic-comp restarted"
    [[ $(pid_of athanor-bar) == "$bar" ]] || fail "athanor-bar restarted"
    [[ $(pid_of athanor-dock) == "$dock" ]] || fail "athanor-dock restarted"
    mode --scale 1 --transform normal
    shot rotation-back
}

stage_presets-live() {
    local since preset want
    # The factory knobs: the float preset has a dock, the bar and the minimal one none.
    for preset in "float:$FLOAT" "bar:$BAR" "minimal:$MINIMAL"; do
        want=${preset#*:} preset=${preset%%:*}
        since=$(now)
        document "preset=$preset"
        applied 5 "$since" "$want"
        shot "presets-$preset"
    done
    since=$(now)
    document preset=float dock=auto-hide
    applied 5 "$since" 'preset: Float, panel: Top, dock: AutoHide'
    since=$(now)
    document preset=float dock=none
    applied 5 "$since" 'preset: Float, panel: Top, dock: Off'
}

stage_degrade() {
    local since
    since=$(now)
    document preset=float
    applied 5 "$since" "$FLOAT"
    since=$(now)
    write_document $'schema = 1\ncolour = "red"\n[output."*"]\npreset = "minimal"\n'
    local before
    before=$(in_session sha256sum "$DOCUMENT")
    # The nearest preset, minimal: no dock, and none of the float panel's gap.
    applied 5 "$since" "$MINIMAL"
    sleep 3
    [[ $(in_session sha256sum "$DOCUMENT") == "$before" ]] || fail "the rejected document was rewritten"
    in_session "journalctl --user -u athanor-bar -p err --since @$since --no-pager | grep -q 'unknown key'" ||
        fail "no 'unknown key' error in athanor-bar's journal"
}

stage_mandatory-mid-session() {
    # An image ships no /etc/athanor: only a run cut short leaves the layout directory.
    guest_ssh '[ ! -d /etc/athanor/layout ] || sudo rm -r /etc/athanor/layout'
    local since
    since=$(now)
    document preset=float panel=top
    applied 5 "$since" "$FLOAT"
    since=$(now)
    guest_ssh 'sudo mkdir -p /etc/athanor/layout &&
    printf "schema = 1\nmandatory = [\"panel\"]\n[output.\"*\"]\npanel = \"bottom\"\n" |
    sudo tee /etc/athanor/layout/50-panel.toml > /dev/null'
    # GLib watches a missing directory by polling its parent.
    applied 10 "$since" 'preset: Float, panel: Bottom, dock: Visible'
    # Detached: the chooser's main loop would hold the SSH call.
    in_session systemd-run --user --quiet --unit=layout-chooser-acc /usr/bin/athanor-layout-chooser
    shot mandatory-chooser
    in_session systemctl --user stop layout-chooser-acc
    guest_ssh 'sudo rm /etc/athanor/layout/50-panel.toml && sudo rmdir /etc/athanor/layout'
}

new_bar_pid() { # new_bar_pid OLD-PID: active, with a different, real MainPID
    [[ $(in_session systemctl --user show -p ActiveState --value athanor-bar) == active ]] || return 1
    local pid
    pid=$(pid_of athanor-bar)
    [[ $pid != "$1" && $pid != 0 ]]
}

# SH8: a bar in a crash loop draws the vendor layout, not the user's, and not nothing.
# bar-acceptance.sh and dock-acceptance.sh test the loop itself; this stage, what it draws.
stage_crash-loop() {
    local since
    since=$(now)
    document preset=minimal
    applied 5 "$since" "$MINIMAL"
    login
    since=$(now)
    local round pid
    for round in 1 2 3 4 5; do
        pid=$(pid_of athanor-bar)
        in_session systemctl --user kill --kill-whom=main -s SIGKILL athanor-bar
        wait_until 90 new_bar_pid "$pid" || fail "round $round: no new MainPID after killing $pid"
    done
    wait_until 10 in_session "journalctl --user -u athanor-bar --since @$since -p err --no-pager -o cat |
        grep -q 'keeps failing'" || fail "no 'keeps failing' at err priority after five kills"
    wait_until 10 in_session "journalctl --user -u athanor-bar --since @$since -g 'layout applied' -o cat --no-pager |
        tail -n 1 | grep -qF 'Layout { $FLOAT }'" ||
        fail "the give-up drew $(last_applied athanor-bar "$since"), not the vendor layout"
    shot crash-loop
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} " == *" $STAGE "* ]] || die "unknown stage '$STAGE': one of ${STAGES[*]}"
done
mkdir -p "$SHOTS"
save_user
trap restore_user EXIT
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
