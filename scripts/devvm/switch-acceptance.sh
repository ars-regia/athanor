#!/usr/bin/env bash
# switch-acceptance.sh [--here] [STAGE...] - the acceptance of the stage 2 switch
# (docs/architecture/doc_shell.md section 8, doc_bar.md section 5) on an image without the
# COSMIC shell: the development VM by default, switched with local-image.sh --push-to-vm;
# --here runs on this machine instead (the maintainer's desktop, upgraded in place).
#
# Stages, in the default order: packages owners presets activation launcher leftovers
# presets-live rollback notifier. Two more run only when named: orca (Orca reads the
# shield sheet; needs SHIELD_HEADING) and hardware (interactive, --here only). launcher,
# rollback and notifier need the VM: launcher presses keys through QEMU's monitor, rollback
# reboots twice, and notifier leaves the guest on the update-trust acceptance images, so it
# runs last; local-image.sh --push-to-vm puts the guest back on the switched image.
#
# Prints PASS <what> for each check and exits non-zero on the first failure. Screenshots go
# to .scratch/switch-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# pass, die, guest_ssh, boot_id, reboot_guest, point_stable, expect_until, REPO.
# shellcheck source-path=SCRIPTDIR
source "$HERE/acceptance/lib.sh"

SHOTS=$ROOT/.scratch/switch-acceptance
# The heading of 2b.5's shield sheet (ui/shield.rs, English locale), which Orca must speak.
SHIELD_HEADING=${SHIELD_HEADING:-}
DEFAULT_STAGES=(packages owners presets activation launcher leftovers presets-live rollback notifier)

usage() { sed -n '2,/^set -euo pipefail$/{/^#/{s/^# \{0,1\}//;p}}' "${BASH_SOURCE[0]}"; }
target=vm
while [[ $# -gt 0 ]]; do
    case $1 in
    -h | --help)
        usage
        exit 0
        ;;
    --here)
        target=here
        shift
        ;;
    -*)
        usage >&2
        exit 2
        ;;
    *) break ;;
    esac
done
stages=("$@")
[[ ${#stages[@]} -gt 0 ]] || stages=("${DEFAULT_STAGES[@]}")
for stage in "${stages[@]}"; do
    if [[ " ${DEFAULT_STAGES[*]} orca hardware " != *" $stage "* ]]; then
        echo "${0##*/}: unknown stage $stage" >&2
        exit 2
    fi
done

fail() { die "FAIL  $*"; }
run() { # run COMMAND: on the target, in a login-less shell
    if [[ $target == vm ]]; then guest_ssh "$*"; else bash -c "$*"; fi
}
in_session() { # in_session COMMAND: as the session user, with its bus and compositor
    # shellcheck disable=SC2016 # expanded by the target's shell
    run 'export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-1}' \
        'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus;' "$*"
}
wait_until() { # wait_until SECONDS COMMAND...
    local deadline=$((SECONDS + $1))
    shift
    until "$@" 2> /dev/null; do
        ((SECONDS < deadline)) || return 1
        sleep 1
    done
}
shot() { # shot NAME: the session's screen, after it had time to redraw
    sleep 2
    mkdir -p "$SHOTS"
    in_session grim - > "$SHOTS/$1.png"
}
vm_only() { # vm_only WHY: skips the calling stage off the VM
    [[ $target == vm ]] && return 0
    echo "SKIP  $1"
    return 1
}
# The systemd unit of the process that owns a bus name, from its cgroup.
owner_unit() {
    in_session "pid=\$(busctl --user status $1 | sed -n 's/^PID=//p') && sed -n 's,^0::.*/,,p' /proc/\$pid/cgroup"
}

stage_packages() {
    local pkg
    for pkg in cosmic-panel cosmic-applets cosmic-notifications athanor-layout-translator; do
        if run rpm -q --quiet "$pkg"; then fail "$pkg is installed"; fi
    done
    run rpm -q --quiet cosmic-comp cosmic-launcher cosmic-app-library || fail "cosmic-comp, the launcher or the app library is missing"
    pass "item 2: the COSMIC shell packages are gone; cosmic-comp, the launcher and the app library stay"
}

stage_owners() {
    local name unit
    for name in org.freedesktop.Notifications org.kde.StatusNotifierWatcher; do
        unit=$(owner_unit "$name")
        [[ $unit == athanor-shelld.service ]] || fail "$name is owned by '$unit', not athanor-shelld.service"
    done
    pass "item 2: both names are owned by athanor-shelld"
}

stage_presets() {
    local unit state
    for unit in athanor-bar athanor-dock athanor-shelld; do
        state=$(in_session "systemctl --user is-enabled $unit.service") || fail "$unit.service is $state, not enabled"
        in_session "systemctl --user is-active --quiet $unit.service" || fail "$unit.service is not active"
    done
    pass "BR8: the bar, the dock and shelld are enabled by preset and running"
}

# Review Focus 3: with shelld stopped, a call to either of its names starts it again.
stage_activation() {
    in_session "systemctl --user stop athanor-shelld.service"
    in_session "busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications \
    Notify susssasa{sv}i switch-acceptance 0 '' switch-acceptance activation 0 0 -1" > /dev/null ||
        fail "Notify without a running athanor-shelld failed"
    wait_until 10 in_session "systemctl --user is-active --quiet athanor-shelld.service" || fail "Notify did not activate athanor-shelld"
    in_session "systemctl --user stop athanor-shelld.service"
    in_session "busctl --user get-property org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher \
    IsStatusNotifierHostRegistered" > /dev/null || fail "the watcher did not answer without a running athanor-shelld"
    wait_until 10 in_session "systemctl --user is-active --quiet athanor-shelld.service" || fail "the watcher did not activate athanor-shelld"
    pass "Review Focus 3: both names activate athanor-shelld"
}

# P4 (4): Super and Super+A without cosmic-panel. A cold first press only starts the process.
stage_launcher() {
    vm_only "launcher needs the VM's QEMU monitor" || return 0
    local keys process press
    for keys in meta_l meta_l-a; do
        process=cosmic-launcher
        [[ $keys == meta_l-a ]] && process=cosmic-app-library
        for press in 1 2; do
            printf 'sendkey %s\n' "$keys" | socat - "unix:$STATE/monitor.sock" > /dev/null
            sleep 2
        done
        in_session "pgrep -x $process" > /dev/null || fail "$process did not start from its shortcut ($press presses)"
        shot "p4-$process"
        printf 'sendkey esc\n' | socat - "unix:$STATE/monitor.sock" > /dev/null
    done
    pass "P4: Super opens cosmic-launcher and Super+A cosmic-app-library without cosmic-panel"
}

# Review Focus 1: the first-session marker stops a second pick, and the COSMIC panel
# configuration left in the home directory is inert.
stage_leftovers() {
    # shellcheck disable=SC2016 # expanded by the target's shell
    in_session 'test -e "${XDG_STATE_HOME:-$HOME/.local/state}/athanor/layout-first-session"' || fail "no first-session marker"
    if [[ -n $(in_session "journalctl --user -b -g 'first session: default layout picked' -o cat") ]]; then
        [[ $target == vm ]] || fail "the upgraded desktop picked a layout again"
    fi
    if [[ -n $(in_session "journalctl --user -b -u athanor-bar -u athanor-dock -g CosmicPanel -o cat") ]]; then
        fail "the bar or the dock reads the stale CosmicPanel configuration"
    fi
    pass "leftovers: the marker stops the pick; the stale COSMIC panel tree is inert"
}

# Item 4: the document the chooser writes (rig.sh chooser-e2e presses the chooser itself)
# applies live, drawn by the bar and the dock. The user's own document comes back afterwards.
stage_presets_live() {
    local preset unit
    # shellcheck disable=SC2016 # expanded by the target's shell
    in_session 'cp -p ~/.config/athanor/layout.toml ~/.config/athanor/layout.toml.switch-acceptance'
    for preset in float bar minimal; do
        in_session "printf '%s\n' 'schema = 1' '[output.\"*\"]' 'preset = \"$preset\"' > ~/.config/athanor/layout.toml"
        for unit in athanor-bar athanor-dock; do
            wait_until 10 in_session "journalctl --user -u $unit -g 'layout applied' -o cat --since -15s | grep -qi 'preset: $preset'" ||
                fail "$unit did not apply $preset (the user's document is in ~/.config/athanor/layout.toml.switch-acceptance)"
        done
        shot "preset-$preset"
    done
    in_session 'mv ~/.config/athanor/layout.toml.switch-acceptance ~/.config/athanor/layout.toml'
    pass "item 4: the three presets apply live, drawn by the bar and the dock"
}

stage_rollback() {
    vm_only "rollback on the desktop is manual (the plan's Task 16)" || return 0
    guest_ssh sudo bootc rollback
    reboot_guest
    guest_ssh rpm -q --quiet cosmic-panel || fail "bootc rollback did not bring back the pre-switch deployment"
    pass "bootc rollback returns to the pre-switch image"
    guest_ssh sudo bootc rollback
    reboot_guest
    stage_packages
    pass "bootc rollback returns to the switched image"
}

# Item 12: an update downloaded once is offered once, not again at each session start. The
# update-trust acceptance images, built on the local image, stage a real signed download.
offered_once() {
    [[ $(in_session "journalctl --user -b -u athanor-update-notify.service -g 'update offered' -o cat | wc -l") == 1 ]]
}
stage_notifier() {
    vm_only "notifier needs the update-trust acceptance images" || return 0
    local session
    ACC_BASE="$ACC_REGISTRY/athanor-system:$(cat "$ROOT/.scratch/local-image/tag")" \
    ACC_RPM_DIR="$ROOT/.scratch/local-image/rpms" "$HERE/acceptance/images.sh"
    point_stable v1
    guest_ssh sudo bootc switch --transport registry "$REPO:v1"
    reboot_guest
    wait_until 600 guest_ssh test -e /var/lib/athanor-update/migrated || fail "the migration did not complete"
    reboot_guest
    point_stable v2
    expect_until "12: v2 downloaded" .update downloaded 20
    wait_until 120 offered_once || fail "the downloaded update was never offered"
    for session in 2 3; do # greetd's initial_session logs the user in again on each start
        guest_ssh sudo systemctl restart greetd
        wait_until 60 in_session "systemctl --user is-active --quiet athanor-update-notify.service" || fail "session $session did not start"
        sleep 70 # two poll periods of the notifier (POLL = 30 s)
        offered_once || fail "session $session offered the announced update again"
    done
    pass "item 12: a downloaded update is offered once, not at each session start"
}

# Item 6: Orca speaks the shield sheet. Orca runs as a transient user unit, so it neither
# holds the SSH session open nor needs a pattern kill to stop.
stage_orca() {
    [[ -n $SHIELD_HEADING ]] || fail "SHIELD_HEADING is unset: the shield sheet heading of 2b.5 (Task 13)"
    # shellcheck disable=SC2016 # expanded by the target's shell
    local log='$XDG_RUNTIME_DIR/orca-switch.log'
    in_session "systemctl --user set-environment ATHANOR_BAR_OPEN=shield && systemctl --user restart athanor-bar.service"
    in_session "systemd-run --user --quiet --collect --unit=switch-acceptance-orca orca --replace --debug-file=$log"
    sleep 8
    in_session "grep -qF '$SHIELD_HEADING' $log" || fail "Orca did not speak the shield sheet"
    in_session "systemctl --user stop switch-acceptance-orca.service; systemctl --user unset-environment ATHANOR_BAR_OPEN && systemctl --user restart athanor-bar.service"
    pass "item 6: Orca reads the shield"
}

# Item 15: real radios, answered by the maintainer.
stage_hardware() {
    [[ $target == here ]] || {
        echo "SKIP  hardware runs with --here"
        return 0
    }
    local answer
    read -rp "Open the bar's network module and join a Wi-Fi network with a password. Joined? [y/N] " answer
    [[ $answer == y ]] || fail "Wi-Fi join"
    read -rp "Open the Bluetooth module and pair a device that asks for a PIN confirmation. Paired? [y/N] " answer
    [[ $answer == y ]] || fail "Bluetooth pairing"
    pass "item 15: Wi-Fi with a password and Bluetooth with a PIN on real hardware"
}

if [[ $target == vm ]]; then wait_ssh; fi
for stage in "${stages[@]}"; do
    "stage_${stage//-/_}"
done
