#!/usr/bin/env bash
# greeter-acceptance.sh [stage...]
# Items 23 and 24 of docs/architecture/doc_lock_and_prompts.md section 5 (the greeter's
# account list and "Other user", LP19) on the dev VM's own greeter, as the image installs it.
# Items 18 to 22 are `cargo test -p athanor-greeter-ui`. Keys reach the greeter through a
# throwaway uinput keyboard and the greeter is read over AT-SPI, as greetd's user, by
# greeter_probe.py; the accounts' password is their name, as the dev VM's user's is. With no
# argument it runs every stage in order, once, on a freshly installed VM sitting at the
# greeter with nobody logged in; with arguments, only those, in the order given. Prints PASS
# <stage> or FAIL <stage>: <what was read>, and exits non-zero on the first failure.
# Screenshots go to .scratch/greeter-acceptance/; look at them.
#
# The list is read over AT-SPI, not off the screen. The VM's own user is a card too: the
# stages compare the order of the five test accounts and ignore other cards.
#
#   accounts  ada (1001), bea (1002, chage -E 0), cy (1003, passwd -l), dan (1005), svc (1004,
#             nologin) and run (63893), each with the password of its name
#   list      before any sign-in the test accounts are listed Ada, Bea, Cy, Dan; svc and run
#             are not (item 23, first clause). Valid only before `order`.
#   refused   bea sees PAM's expiry message and is not signed in; cy gets the ordinary
#             failure text, not the expiry message
#   order     dan, then ada, sign in at the greeter and are logged out; after a reboot the
#             order is Ada, Dan, Bea, Cy and ada's card is the open one
#   fallback  lastlog2.db at mode 0600: the order is Ada, Bea, Cy, Dan and the journal holds
#             one line saying it fell back to the name order; the mode is put back
#   timing    under "Other user", nobody-here and dan with a wrong password, 20 attempts each in
#             batches of 4 with `faillock --user dan --reset` between, show the same prompt and
#             the same failure text; the difference of the medians of the seconds from submit
#             to failure text stays within the interquartile range of nobody-here's. The bound
#             is provisional until the first run measures it (item 23)
#   hidden    user-list = false in /etc/athanor/greeter.toml: no account name on screen (the
#             screenshot) or in the AT-SPI tree, "Other user"'s field alone (item 24, first
#             clause); the file is removed after
#   names     with the list shown, each card is named "full name, user name" (the name Orca
#             reads; Orca itself is not run: no athanor-a11y at the greeter yet), item 24
#   keyboard  the keyboard alone signs in as ada from the list and through "Other user", and
#             Escape from "Other user" returns to the list (item 24, last clause)
#   cleanup   removes the accounts, the setting and the saved mode; run only on request
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

SHOTS=$ROOT/.scratch/greeter-acceptance
STAGES=(accounts list refused order fallback timing hidden names keyboard)
ACCOUNTS=(ada bea cy dan)
FULL_NAMES=(Ada Bea Cy Dan)
LASTLOG=/var/lib/lastlog/lastlog2.db
SETTING=/etc/athanor/greeter.toml
PROBE_DIR=/run/greeter-acceptance
ATTEMPTS=20
BATCH=4
STAGE=
WORK=$(mktemp -d)
# What a stage changed in the guest and must put back, even when it fails.
MODE_CHANGED='' SETTING_WRITTEN=''
finish() {
    rm -rf "$WORK"
    [[ -z $MODE_CHANGED ]] || guest_ssh sudo chmod 0644 "$LASTLOG"
    [[ -z $SETTING_WRITTEN ]] || guest_ssh sudo rm -f "$SETTING"
}
trap finish EXIT

fail() { # fail WHAT-WAS-READ
    echo "FAIL $STAGE: $*"
    exit 1
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

# Puts the probe and its key tables in the guest; /run is empty after every boot.
push_probe() {
    guest_ssh "sudo install -d -m 0755 $PROBE_DIR"
    local file
    for file in keyboard_type.py greeter_probe.py; do
        guest_ssh "sudo tee $PROBE_DIR/$file > /dev/null" < "$HERE/$file"
    done
}

probe() { guest_ssh "sudo python3 $PROBE_DIR/greeter_probe.py $(printf '%q ' "$@")"; }

type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$HERE/keyboard_type.py"; }

shot() { # shot NAME: the greeter, as greetd's user, against its own compositor
    sleep 2
    # shellcheck disable=SC2016 # expanded by the guest's shell
    guest_ssh 'sudo -u greetd env XDG_RUNTIME_DIR=/run/user/$(id -u greetd) WAYLAND_DISPLAY=wayland-1 grim -' \
        > "$SHOTS/$1.png" || echo "note: no screenshot $1 (grim refused)" >&2
}

greeter_up() { probe tree > /dev/null; }

list_is_shown() { [[ $(cards | jq -r .list_shown) == true ]]; }

# The cards as JSON: list, selected, open, list_shown.
cards() { probe cards; }

# The test accounts of a card list, in the order shown: "Ada Bea Cy Dan".
tested() { # tested JSON
    jq -r --argjson n "$(printf '%s\n' "${FULL_NAMES[@]}" | jq -R . | jq -s .)" \
        '[.list[] | split(", ")[0] | select(. as $x | $n | index($x))] | join(" ")' <<< "$1"
}

expect_order() { # expect_order JSON "Ada Bea Cy Dan"
    local order
    order=$(tested "$1")
    [[ $order == "$2" ]] || fail "the test accounts are listed '$order', not '$2'; the list: $1"
}

# The account has an active session: loginctl lists a session of its user.
signed_in() { [[ -n $(guest_ssh loginctl list-sessions --no-legend | awk -v u="$1" '$3 == u') ]]; }

# Signs in from the open greeter through the keyboard and logs out at once.
sign_in_and_out() { # sign_in_and_out USER FULL-NAME
    probe open-card "$2" || fail "no card of $2 to open"
    type_keys "$1" @enter
    wait_until 60 signed_in "$1" || fail "no session of $1 60 s after the password"
    guest_ssh sudo loginctl terminate-user "$1"
    wait_until 60 greeter_up || fail "no greeter 60 s after $1 logged out"
}

stage_accounts() {
    if guest_ssh getent passwd ada > /dev/null; then
        fail "ada exists: this stage wants a freshly installed VM; run cleanup first"
    fi
    local uids=(1001 1002 1003 1005) i name
    for i in 0 1 2 3; do
        guest_ssh "sudo useradd -m -u ${uids[i]} -c ${FULL_NAMES[i]} -s /bin/bash ${ACCOUNTS[i]}"
    done
    guest_ssh sudo useradd -m -u 1004 -c Svc -s /usr/sbin/nologin svc
    guest_ssh sudo useradd -m -u 63893 -c Run -s /bin/bash run
    for name in "${ACCOUNTS[@]}"; do
        guest_ssh "echo $name:$name | sudo chpasswd"
    done
    guest_ssh sudo chage -E 0 bea
    guest_ssh sudo passwd -l cy
    push_probe
    # The greeter read the accounts when it started: start it again.
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up ||
        fail "no greeter on greetd's accessibility bus 60 s after restarting greetd: the journal says 'runs without accessibility' when the bus was missing at its start"
}

stage_list() {
    push_probe
    local json
    json=$(cards)
    expect_order "$json" "Ada Bea Cy Dan"
    grep -qE 'Svc|Run' <<< "$json" && fail "svc or run is listed: $json"
    grep -qF "Other user" <<< "$json" || fail "no Other user card: $json"
    shot list
}

stage_refused() {
    local alerts
    probe open-card Bea
    type_keys bea @enter
    alerts=$(probe alerts 20) || fail "no failure text for bea"
    grep -qi 'expired' <<< "$alerts" || fail "bea saw no expiry message: $alerts"
    ! signed_in bea || fail "bea is signed in"
    shot expired
    probe open-card Cy
    type_keys cy @enter
    alerts=$(probe alerts 20) || fail "no failure text for cy"
    ! grep -qiE 'expired|locked' <<< "$alerts" || fail "cy's refusal is not the ordinary text: $alerts"
    ! signed_in cy || fail "cy is signed in"
    shot locked
}

stage_order() {
    local json
    sign_in_and_out dan Dan
    sign_in_and_out ada Ada
    guest_ssh sudo systemctl reboot || echo "note: the reboot cut the connection" >&2
    sleep 10
    wait_until 180 guest_ssh true || fail "no SSH 180 s after the reboot"
    push_probe
    wait_until 120 greeter_up || fail "no greeter 120 s after the reboot"
    json=$(cards)
    expect_order "$json" "Ada Dan Bea Cy"
    [[ $(jq -r .open <<< "$json") == "Password for Ada" ]] || fail "ada's card is not the open one: $json"
    shot order
}

stage_fallback() {
    local since json log lines
    MODE_CHANGED=1
    guest_ssh sudo chmod 0600 "$LASTLOG"
    since=$(guest_ssh date +%s)
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restarting greetd"
    json=$(cards)
    expect_order "$json" "Ada Bea Cy Dan"
    log=$(guest_ssh sudo journalctl -q -b --since "@$since" -o cat)
    mapfile -t lines < <(grep -F "the accounts follow the name order" <<< "$log")
    ((${#lines[@]} == 1)) || fail "the journal holds ${#lines[@]} lines about the name order since the restart, not one"
    guest_ssh sudo chmod 0644 "$LASTLOG"
    MODE_CHANGED=''
}

# Types the same sign-in failure COUNT times under "Other user", BATCH at a time, for NAME.
collect() { # collect NAME OUTFILE
    : > "$2"
    local done_=0
    while ((done_ < ATTEMPTS)); do
        probe attempts "$1" wrong-password "$BATCH" >> "$2" || fail "the attempts for $1 stopped after $done_"
        done_=$((done_ + BATCH))
        guest_ssh sudo faillock --user dan --reset
    done
}

stage_timing() {
    local alerts
    collect nobody-here "$WORK/nobody"
    collect dan "$WORK/dan"
    # The text: one more attempt each, read in full and compared.
    probe attempts nobody-here wrong-password 1 > /dev/null
    alerts=$(probe alerts 5) || fail "no failure text for nobody-here"
    probe attempts dan wrong-password 1 > /dev/null
    guest_ssh sudo faillock --user dan --reset
    [[ $(probe alerts 5) == "$alerts" ]] || fail "the failure texts differ: nobody-here '$alerts'"
    python3 -B "$HERE/greeter_probe.py" compare "$WORK/nobody" "$WORK/dan" ||
        fail "the medians of nobody-here and dan differ by more than the delay's spread"
}

stage_hidden() {
    local tree name
    guest_ssh sudo install -d -m 0755 /etc/athanor
    SETTING_WRITTEN=1
    guest_ssh "echo 'user-list = false' | sudo tee $SETTING > /dev/null"
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restarting greetd"
    tree=$(probe tree)
    # A whole word, so that "cy" is not found in "Privacy".
    for name in "${ACCOUNTS[@]}" "${FULL_NAMES[@]}" "$(guest_ssh id -un)"; do
        if jq -e --arg n "$name" 'any(.[]; .showing and ((.name + " " + .text) | test("(^|[^a-z])" + $n + "([^a-z]|$)"; "i")))' <<< "$tree" > /dev/null; then
            fail "'$name' is in the AT-SPI tree: $tree"
        fi
    done
    jq -e 'any(.[]; .showing and .name == "User name")' <<< "$tree" > /dev/null || fail "no name field: $tree"
    shot hidden
    guest_ssh sudo rm -f "$SETTING"
    SETTING_WRITTEN=''
    guest_ssh sudo systemctl restart greetd
    wait_until 60 greeter_up || fail "no greeter 60 s after restoring the list"
}

stage_names() {
    local json name i
    json=$(cards)
    for i in 0 1 2 3; do
        name="${FULL_NAMES[i]}, ${ACCOUNTS[i]}"
        jq -e --arg n "$name" '.list | index($n)' <<< "$json" > /dev/null || fail "no card named '$name': $json"
    done
    jq -e '.selected != null' <<< "$json" > /dev/null || fail "no card is selected: $json"
}

stage_keyboard() {
    # From the list: Escape, the letter, Enter, the password.
    probe open-card Ada
    type_keys ada @enter
    wait_until 60 signed_in ada || fail "ada did not sign in from the list"
    guest_ssh sudo loginctl terminate-user ada
    wait_until 60 greeter_up || fail "no greeter 60 s after ada logged out"
    # Through "Other user": the name, then PAM's password question.
    probe open-other
    type_keys ada @enter
    # PAM's question has no marker the stage could wait on from here; a fixed time.
    sleep 3
    type_keys ada @enter
    wait_until 60 signed_in ada || fail "ada did not sign in through Other user"
    guest_ssh sudo loginctl terminate-user ada
    wait_until 60 greeter_up || fail "no greeter 60 s after ada logged out"
    # Escape from "Other user" returns to the list.
    probe open-other
    type_keys @escape
    wait_until 10 list_is_shown || fail "Escape did not return to the list"
}

stage_cleanup() {
    local name
    guest_ssh sudo rm -f "$SETTING"
    guest_ssh sudo chmod 0644 "$LASTLOG"
    for name in "${ACCOUNTS[@]}" svc run; do
        guest_ssh "if getent passwd $name > /dev/null; then sudo userdel -r $name; fi"
    done
}

run=("$@")
((${#run[@]})) || run=("${STAGES[@]}")
for STAGE in "${run[@]}"; do
    [[ " ${STAGES[*]} cleanup " == *" $STAGE "* ]] || fail "unknown stage: one of ${STAGES[*]} cleanup"
done
mkdir -p "$SHOTS"
for STAGE in "${run[@]}"; do
    "stage_$STAGE"
    echo "PASS $STAGE"
done
