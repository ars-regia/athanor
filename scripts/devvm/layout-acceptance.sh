#!/usr/bin/env bash
# layout-acceptance.sh [stage...]
# Acceptance item 10 of docs/architecture/doc_shell.md (stage 1c, the layout) in the dev
# VM's real session: the translator's unit under systemd --user, the real cosmic-comp and
# cosmic-panel, rotation. Deploys the release binaries from .scratch/shell-rig/bin (build
# them with forge/test/shell/rig.sh build-layout). With no argument it runs every stage in
# order; with arguments, only those, in the order given. Prints PASS <stage> or
# FAIL <stage>: <what was read>, and exits non-zero on the first failure. Screenshots go to
# .scratch/layout-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

BIN=$ROOT/.scratch/shell-rig/bin
SHOTS=$ROOT/.scratch/layout-acceptance
STAGES=(deploy first-session-small first-session-portrait rotation presets-live degrade
    mandatory-mid-session memory crash-loop)
# Every stage sets the output to this mode; the first-session stages change only scale and
# transform on top of it.
WIDTH=1920 HEIGHT=1080
PANEL=com.system76.CosmicPanel
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

# The configuration is written before the panel redraws: give it time to draw first.
shot() {
    sleep 2
    "$HERE/screenshot.sh" "$SHOTS/$1.png" > /dev/null
}

# The first output's name; cosmic-randr colours its list even without a terminal.
output() { in_session cosmic-randr list | sed 's/\x1b\[[0-9;]*m//g' | awk '/^[A-Za-z]/ { print $1; exit }'; }
mode() { # mode [cosmic-randr mode options...]
    in_session cosmic-randr mode "$@" "$(output)" "$WIDTH" "$HEIGHT"
}

# The names in cosmic-panel's entries, comma-separated. cosmic-panel rewrites the file in
# pretty RON when it starts, so the names are compared, not the text.
entries() { in_session "cat ~/.config/cosmic/$PANEL/v1/entries" | grep -o '"[^"]*"' | tr -d '"' | paste -sd, -; }
key() { in_session "cat ~/.config/cosmic/$PANEL.$1/v1/$2"; } # key ENTRY KEY
is_entries() { [[ $(entries) == "$1" ]]; }
is_key() { [[ $(key "$1" "$2") == "$3" ]]; } # is_key ENTRY KEY VALUE

# Replaces the user document in one step, as the chooser does.
write_document() { # write_document TOML
    printf '%s' "$1" | in_session 'mkdir -p ~/.config/athanor &&
    cat > ~/.config/athanor/.layout.toml.acc && mv ~/.config/athanor/.layout.toml.acc ~/.config/athanor/layout.toml'
}
document() { # document KEY=VALUE... : a schema 1 document with the wildcard output's keys
    local text=$'schema = 1\n[output."*"]\n' pair
    for pair in "$@"; do text+="${pair%%=*} = \"${pair#*=}\""$'\n'; done
    write_document "$text"
}

unit() { in_session systemctl --user "$@" athanor-layout.service; }
# reset-failed also clears the start limit, so a rerun within ten minutes still starts.
start_unit() {
    unit reset-failed
    unit start
}
# A user who never had a session: no layout document, no marker, COSMIC's panel defaults.
# The panel stops too: it watches its configuration directories, and loses them to the rm.
new_user() {
    in_session systemctl --user stop cosmic-panel.service
    unit stop
    in_session "rm -f ~/.config/athanor/layout.toml ~/.local/state/athanor/layout-first-session \
    ~/.local/state/athanor/layout-cosmic-panel &&
    find ~/.config/cosmic -maxdepth 1 -name '$PANEL*' -exec rm -r {} +"
}

stage_deploy() {
    "$HERE/deploy.sh" \
        "$BIN/athanor-layout-translator:/usr/bin/athanor-layout-translator" \
        "$BIN/athanor-layout-chooser:/usr/bin/athanor-layout-chooser" \
        "$ROOT/forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/data/athanor-layout.service:/usr/lib/systemd/user/athanor-layout.service" \
        "$ROOT/system/athanor-layout/vendor/10-athanor.toml:/usr/share/athanor/layout/10-athanor.toml" > /dev/null
    guest_ssh 'sudo mkdir -p /usr/lib/systemd/user/athanor-session.target.wants &&
    sudo ln -sfn ../athanor-layout.service /usr/lib/systemd/user/athanor-session.target.wants/athanor-layout.service'
    in_session systemctl --user daemon-reload
    unit cat > /dev/null || fail "systemctl --user cat athanor-layout.service found no unit"
}

# The login order: the translator, which is Before=cosmic-panel.service, then the panel.
login() {
    start_unit
    in_session systemctl --user start cosmic-panel.service
}

first_session() { # first_session PRESET : the pick, its marker, the entries, the unit
    local want_entries=$2
    wait_until 10 in_session "grep -qx 'preset = \"$1\"' ~/.config/athanor/layout.toml" ||
        fail "layout.toml: $(in_session 'cat ~/.config/athanor/layout.toml' 2>&1)"
    [[ $(in_session 'cat ~/.local/state/athanor/layout-first-session') == "$1" ]] ||
        fail "marker: $(in_session 'cat ~/.local/state/athanor/layout-first-session' 2>&1)"
    wait_until 5 is_entries "$want_entries" || fail "entries: $(entries)"
    [[ $(unit is-active) == active ]] || fail "unit: $(unit is-active)"
}

stage_first-session-small() {
    new_user
    mode --scale 2 --transform normal
    login
    first_session bar Panel
    shot first-session-small
}

stage_first-session-portrait() {
    new_user
    mode --scale 1 --transform rotate90
    login
    first_session float Panel,Dock
    shot first-session-portrait
    mode --scale 1 --transform normal
}

stage_rotation() {
    document preset=float panel=bottom
    wait_until 10 is_key Dock anchor Left || fail "Dock anchor in landscape: $(key Dock anchor)"
    local comp translator
    comp=$(in_session pidof cosmic-comp)
    translator=$(unit show -p MainPID --value)
    mode --scale 1 --transform rotate90
    wait_until 5 is_key Dock anchor Bottom || fail "Dock anchor in portrait: $(key Dock anchor)"
    [[ $(in_session pidof cosmic-comp) == "$comp" ]] || fail "cosmic-comp restarted"
    [[ $(unit show -p MainPID --value) == "$translator" ]] || fail "the translator restarted"
    shot rotation
    mode --scale 1 --transform normal
    wait_until 5 is_key Dock anchor Left || fail "Dock anchor back in landscape: $(key Dock anchor)"
}

stage_presets-live() {
    local preset want
    # The factory knobs: the float preset has a dock, the bar and the minimal one none.
    for preset in float:Panel,Dock bar:Panel minimal:Panel; do
        want=${preset#*:} preset=${preset%%:*}
        document "preset=$preset"
        wait_until 5 is_entries "$want" || fail "$preset: entries $(entries)"
        shot "presets-$preset"
    done
    document preset=float dock=auto-hide
    wait_until 5 is_key Dock autohide Always || fail "auto-hide: Dock autohide $(key Dock autohide)"
    document preset=float dock=none
    wait_until 5 is_entries Panel || fail "dock none: entries $(entries)"
}

stage_degrade() {
    document preset=float
    wait_until 5 is_key Panel anchor_gap true || fail "float first: Panel anchor_gap $(key Panel anchor_gap)"
    write_document $'schema = 1\ncolour = "red"\n[output."*"]\npreset = "minimal"\n'
    local before
    before=$(in_session sha256sum ~/.config/athanor/layout.toml)
    sleep 3
    [[ $(in_session sha256sum ~/.config/athanor/layout.toml) == "$before" ]] || fail "the rejected document was rewritten"
    in_session 'journalctl --user -u athanor-layout -p err --since "-1min" --no-pager | grep -q "unknown key"' ||
        fail "no 'unknown key' error in the journal"
    # The nearest preset, minimal: no dock, and none of the float panel's gap.
    is_entries Panel || fail "entries $(entries)"
    is_key Panel anchor_gap false || fail "Panel anchor_gap $(key Panel anchor_gap)"
}

stage_mandatory-mid-session() {
    guest_ssh 'sudo find /etc/athanor -maxdepth 1 -name layout -exec rm -r {} +'
    document preset=float panel=top
    wait_until 5 is_key Panel anchor Top || fail "Panel anchor before the policy: $(key Panel anchor)"
    guest_ssh 'sudo mkdir -p /etc/athanor/layout &&
    printf "schema = 1\nmandatory = [\"panel\"]\n[output.\"*\"]\npanel = \"bottom\"\n" |
    sudo tee /etc/athanor/layout/50-panel.toml > /dev/null'
    # GLib watches a missing directory by polling its parent.
    wait_until 10 is_key Panel anchor Bottom || fail "Panel anchor under the policy: $(key Panel anchor)"
    # Detached: the chooser's main loop would hold the SSH call.
    in_session systemd-run --user --quiet --unit=layout-chooser-acc /usr/bin/athanor-layout-chooser
    shot mandatory-chooser
    in_session systemctl --user stop layout-chooser-acc
    guest_ssh 'sudo rm /etc/athanor/layout/50-panel.toml && sudo rmdir /etc/athanor/layout'
}

stage_memory() {
    local peak high
    peak=$(unit show -p MemoryPeak --value)
    high=$(unit show -p MemoryHigh --value)
    echo "memory: MemoryPeak $peak bytes, MemoryHigh $high"
    [[ $peak =~ ^[0-9]+$ && $high =~ ^[0-9]+$ ]] || fail "MemoryPeak '$peak', MemoryHigh '$high'"
    ((peak < high)) || fail "MemoryPeak $peak is not below MemoryHigh $high"
}

# The unit has restarted after a kill, or has stopped for good.
restarted_or_stopped() { # restarted_or_stopped OLD-PID
    local state
    state=$(unit show -p ActiveState --value)
    [[ $state == inactive || $state == failed ]] ||
        [[ $state == active && $(unit show -p MainPID --value) != "$1" ]]
}

stage_crash-loop() {
    # A stop empties the runtime directory, and with it the crash-loop record of a previous run.
    unit stop
    start_unit
    document preset=minimal
    wait_until 5 is_key Panel anchor_gap false || fail "minimal first: Panel anchor_gap $(key Panel anchor_gap)"
    local since round pid kills=(--kill-whom=main --kill-whom=all)
    since=$(guest_ssh date '+%Y-%m-%d\ %H:%M:%S')
    for round in 1 2 3 4 5 6; do
        [[ $(unit show -p ActiveState --value) == active ]] || break
        pid=$(unit show -p MainPID --value)
        # Alternately a crash of the main process, which the ExecStopPost records, and a kill
        # of the whole cgroup, which takes the ExecStopPost down too and is counted at the
        # next start.
        unit kill "${kills[round % 2]}" -s SIGKILL
        # The restart delay grows to 60 s (RestartSteps=5).
        wait_until 90 restarted_or_stopped "$pid" || fail "round $round: $(unit show -p ActiveState,SubState)"
    done
    [[ $(unit show -p ActiveState --value) == inactive ]] || fail "the unit ended $(unit show -p ActiveState --value)"
    in_session "journalctl --user -u athanor-layout --since '$since' --no-pager | grep -q 'keeps failing'" ||
        fail "no 'keeps failing' in the journal"
    # The vendor layout, not an empty desktop.
    is_entries Panel,Dock || fail "entries $(entries)"
    is_key Panel anchor_gap true || fail "Panel anchor_gap $(key Panel anchor_gap)"
    shot crash-loop
    start_unit
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
