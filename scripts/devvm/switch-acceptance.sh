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
# On the VM the run logs the guest user in through greetd's initial_session, removes the
# unit files earlier acceptance scripts left in the user's configuration, and on exit puts
# the image's greetd configuration and container policy back and checks both.
#
# Prints PASS <what> for each check and exits non-zero on the first failure. Screenshots go
# to .scratch/switch-acceptance/.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# pass, die, guest_ssh, boot_id, reboot_guest, point_stable, expect_until, trust_tag,
# restore_policy, REPO, ACC_STATE.
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
# The systemd unit of the process that owns a bus name, from its cgroup. Fails when the name
# has no owner: the bus answers NameHasNoOwner and busctl exits non-zero, with no activation.
owner_unit() {
    in_session "out=\$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus \
    GetConnectionUnixProcessID s $1) && pid=\${out#u } && [ \"\$pid\" -gt 0 ] && sed -n 's,^0::.*/,,p' /proc/\$pid/cgroup"
}

# Unit files and drop-ins that an acceptance script put under the user's configuration, such
# as shelld-acceptance.sh's private-bus drop-in: left behind, they move the daemon off the
# session bus.
# shellcheck disable=SC2088 # the tilde and the globs are expanded by the target's shell
ACCEPTANCE_UNITS='~/.config/systemd/user/athanor-*acceptance* ~/.config/systemd/user/athanor-*.d/*acceptance*'
acceptance_units() { run "for f in $ACCEPTANCE_UNITS; do if [ -e \"\$f\" ]; then echo \"\$f\"; fi; done"; }
no_acceptance_units() {
    local left
    left=$(acceptance_units)
    [[ -z $left ]] || fail "acceptance unit files are left under the user's configuration: $left"
}

# The guest user's session on seat0, started by greetd's initial_session as the update-trust
# acceptance images start it: the switched image's greetd runs only the greeter. The command
# is the one the greeter starts after a login (athanor-greeter-ui, src/auth.rs). tmpfiles
# relinks /etc/greetd/config.toml to the image's own file at every boot (L+), so the
# session is started again after each reboot, and restore_greetd puts the link back.
GREETD_OWN=/usr/share/athanor-system-config/greetd.toml
SESSION_COMMAND=/usr/bin/athanor-session
seat_session() {
    guest_ssh loginctl list-sessions --no-legend | awk -v user="$GUEST_USER" '$3 == user && $4 == "seat0" { found = 1 } END { exit !found }'
}
start_session() {
    if ! guest_ssh "grep -qF '[initial_session]' $GREETD_OWN"; then
        guest_ssh test -x "$SESSION_COMMAND" || fail "$SESSION_COMMAND is missing: greetd has no session to start"
        {
            guest_ssh cat "$GREETD_OWN"
            printf '\n[initial_session]\ncommand = "%s"\nuser = "%s"\n' "$SESSION_COMMAND" "$GUEST_USER"
        } | guest_ssh "sudo install -m 0644 /dev/stdin /etc/greetd/config.toml"
        restart_greetd_login
    fi
    wait_until 90 seat_session || fail "greetd's initial_session left no session of $GUEST_USER on seat0"
    wait_until 60 in_session "systemctl --user is-active --quiet athanor-bar.service" || fail "athanor-bar.service is not active in the session"
}
# greetd runs initial_session only on its first start after boot: that start creates the
# runfile, and every later start skips initial_session while the file exists (greetd(5),
# runfile). A restart meant to log the user in removes it first. The path is the config's
# [general] runfile, or greetd's default.
restart_greetd_login() {
    local runfile
    runfile=$(guest_ssh cat /etc/greetd/config.toml |
        awk -F '"' '/^[[:space:]]*\[/ { general = ($0 ~ /^[[:space:]]*\[general\]/) }
            general && /^[[:space:]]*runfile[[:space:]]*=/ { print $2 }')
    runfile=${runfile:-/run/greetd.run}
    [[ $runfile =~ ^/[A-Za-z0-9._/-]+$ ]] || fail "greetd's runfile is not a plain absolute path: '$runfile'"
    guest_ssh "sudo rm -f $runfile && sudo systemctl restart greetd"
}
restore_greetd() {
    guest_ssh sudo ln -sfn "$GREETD_OWN" /etc/greetd/config.toml
    [[ $(guest_ssh readlink /etc/greetd/config.toml) == "$GREETD_OWN" ]] || die "FAIL  the greetd configuration was not restored"
}
# Every check runs, each in a subshell since die exits; any failure fails the run.
cleanup() {
    local status=0
    (restore_policy) || status=1
    (restore_greetd) || status=1
    (no_acceptance_units) || status=1
    ((status == 0)) || exit 1
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
        unit=$(owner_unit "$name") || fail "$name has no owner on the session bus"
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
    in_session "busctl --user call -- org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications \
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
        in_session "pidof $process" > /dev/null || fail "$process did not start from its shortcut ($press presses)"
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
    no_acceptance_units
    pass "leftovers: the marker stops the pick; the stale COSMIC panel tree is inert; no acceptance unit is left"
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
    # Each deployment keeps its own /etc: neither may carry the trust of the switch or the
    # acceptance login.
    restore_policy
    restore_greetd
    pass "bootc rollback returns to the pre-switch image, its policy and greetd as shipped"
    guest_ssh sudo bootc rollback
    reboot_guest
    stage_packages
    restore_policy
    restore_greetd
    pass "bootc rollback returns to the switched image, its policy and greetd as shipped"
    start_session
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
    # The switched image trusts the project key for this repository; v1 is signed with acc-1.
    trust_tag v1 "$ACC_STATE/keys/acc-1.pub"
    know_acc_registry
    guest_ssh sudo bootc switch --enforce-container-sigpolicy --transport registry "$REPO:v1"
    restore_policy
    reboot_guest
    wait_until 600 guest_ssh test -e /var/lib/athanor-update/migrated || fail "the migration did not complete"
    reboot_guest
    point_stable v2
    expect_until "12: v2 downloaded" .update downloaded 20
    wait_until 120 offered_once || fail "the downloaded update was never offered"
    for session in 2 3; do # initial_session logs the user in again: restart_greetd_login drops the runfile
        restart_greetd_login
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

if [[ $target == vm ]]; then
    wait_ssh
    # A failed stage must leave neither the trust of a switch (acceptance/lib.sh) nor the
    # acceptance login behind; and a unit file an earlier acceptance left must not decide
    # who owns the session's names.
    trap cleanup EXIT
    run "rm -f $ACCEPTANCE_UNITS"
    in_session systemctl --user daemon-reload
    start_session
fi
for stage in "${stages[@]}"; do
    "stage_${stage//-/_}"
done
