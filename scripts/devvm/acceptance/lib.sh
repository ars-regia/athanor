# shellcheck shell=bash
# Shared helpers of the update and trust acceptance (scripts/devvm/acceptance/README.md).
# Sourced, never run. Everything here uses a throwaway registry and throwaway keys.
ACC_HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$ACC_HERE/../../.." && pwd)
if [[ ${ACC_CI:-} == 1 ]]; then
  # run.sh --ci: the VM of forge/test/iso/run_iso_test.sh, not the dev VM. That script hands
  # over the forwarded SSH port and the key of the run; the guest's user is the kickstart's.
  die() {
    echo "$*" >&2
    exit 1
  }
  for var in ATHANOR_UPDATE_TO ATHANOR_UPDATE_REPOSITORY ATHANOR_UPDATE_MIRROR ACC_CI_OUT SSH_PORT ACC_SSH_KEY; do
    [[ -n ${!var:-} ]] || die "run.sh --ci needs $var (forge/test/iso/run_iso_test.sh sets it)"
  done
  [[ $ATHANOR_UPDATE_TO =~ ^sha256:[0-9a-f]{64}$ ]] || die "ATHANOR_UPDATE_TO is not a digest: '$ATHANOR_UPDATE_TO'"
  [[ $SSH_PORT =~ ^[0-9]+$ ]] || die "SSH_PORT is not a port: '$SSH_PORT'"
  STATE=$ACC_CI_OUT/acceptance-state
  GUEST_USER=collaudo
  SYSTEM_IMAGE=$ATHANOR_UPDATE_REPOSITORY
  ssh_key() { echo "$ACC_SSH_KEY"; }
  guest_ssh() {
    ssh -i "$ACC_SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=10 -p "$SSH_PORT" \
      -o "UserKnownHostsFile=$STATE/known_hosts" -o StrictHostKeyChecking=accept-new "$GUEST_USER@127.0.0.1" "$@"
  }
else
  # shellcheck source-path=SCRIPTDIR
  source "$ACC_HERE/../devvm.env"
fi

ACC_PORT=${ACC_PORT:-5000}
# The container and the systemd user unit of the registry, and the image prefix on its port: set
# here, so a second registry on another port starts and reuses nothing of the one on 5000.
ACC_REGISTRY_NAME=${ACC_REGISTRY_NAME:-athanor-acc-registry}
ACC_REGISTRY=${ACC_REGISTRY:-localhost:${ACC_PORT}/acc}
ACC_STATE=${ACC_STATE:-$STATE/acceptance}
ACC_BASE=${ACC_BASE:-$SYSTEM_IMAGE:latest}
ACC_RPM_DIR=${ACC_RPM_DIR:-$ROOT/RPMS_OUT}
REPO=$ACC_REGISTRY/athanor-system
UPDATE1=(os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1)
mkdir -p "$ACC_STATE"

# The 31 GB host does not run the 12 GB runner guest and this 8 GB VM together. Only a job on
# a self-hosted runner, the guest on this host, counts: jobs on GitHub's hosted runners use
# none of its memory (maintainer, 2026-10-15). This checks the start only: a job that starts
# later can still bring the host under memory pressure. A host without the runner unit, such
# as a laptop, runs no runner guest, and the check does not apply there.
guard_no_ci() {
  local busy
  systemctl is-active --quiet athanor-runner.service || return 0
  busy=$(gh api "repos/${GITHUB_REPOSITORY:-ars-regia/athanor}/actions/runners" --jq '[.runners[] | select(.busy)] | length')
  [[ $busy =~ ^[0-9]+$ ]] || die "the runner state could not be read: '$busy'"
  [[ $busy -eq 0 ]] || die "$busy self-hosted runner(s) busy: the dev VM must not run beside a CI job on this host"
}

wait_ssh() {
  for _ in $(seq 180); do
    if guest_ssh -q true 2> /dev/null; then tunnel; return 0; fi
    sleep 2
  done
  die "no SSH on 127.0.0.1:$SSH_PORT after 6 minutes"
}

# The guest reaches the host's registry on its own 127.0.0.1, so signer and verifier spell
# the repository identically (a signature records the reference it was made for). In CI the
# guest reaches the candidate's mirror through QEMU's gateway instead: no tunnel, and so no
# second session of the guest user to open or close.
tunnel() {
  [[ ${ACC_CI:-} != 1 ]] || return 0
  guest_ssh -q "ss -ltn | grep -q ':$ACC_PORT '" 2> /dev/null && return 0
  ssh -i "$(ssh_key)" -p "$SSH_PORT" -o "UserKnownHostsFile=$STATE/known_hosts" -o ExitOnForwardFailure=yes \
    -f -N -R "$ACC_PORT:127.0.0.1:$ACC_PORT" "$GUEST_USER@127.0.0.1"
}

# The tunnel is a login of the guest user with no terminal that lasts the whole run: logind
# lists it as a session of class user, and since #433 the update service counts it as a second
# session of the caller's user. A call from the active session closes it, so that the cases
# make every session the service counts; check_now opens it again, as the check needs it.
untunnel() {
  [[ ${ACC_CI:-} != 1 ]] || return 0
  if pgrep -f -- "-R $ACC_PORT:127.0.0.1:$ACC_PORT" > /dev/null; then
    pkill -f -- "-R $ACC_PORT:127.0.0.1:$ACC_PORT"
  fi
  for _ in $(seq 20); do
    guest_ssh "! ss -ltn | grep -q ':$ACC_PORT '" && return 0
    sleep 1
  done
  die "FAIL  the tunnel to the registry is still open 20 s after it was closed"
}

boot_id() { guest_ssh cat /proc/sys/kernel/random/boot_id; }
# Succeeds once the guest answers from a boot other than BOOT: with a deployment staged, the
# old boot keeps answering for a while, as ostree finalizes the deployment on the way down.
wait_reboot() { # wait_reboot BOOT
  local now deadline=$((SECONDS + 600))
  while ((SECONDS < deadline)); do
    sleep 2
    if now=$(guest_ssh -q cat /proc/sys/kernel/random/boot_id 2> /dev/null) && [[ $now != "$1" ]]; then
      tunnel
      return 0
    fi
  done
  return 1
}

# Scheduled two seconds ahead, so the SSH command returns before the connection drops and
# its exit status means something.
reboot_guest() {
  local boot
  boot=$(boot_id)
  guest_ssh sudo systemd-run --quiet --on-active=2 systemctl reboot
  wait_reboot "$boot" || die "the guest did not come back from the reboot in 10 minutes"
}
power_cycle() {
  guest_ssh sudo systemd-run --quiet --on-active=2 systemctl poweroff
  for _ in $(seq 60); do systemctl --user -q is-active "$UNIT" || break; sleep 2; done
  "$ACC_HERE/../start.sh" > /dev/null
  wait_ssh
}

state() { guest_ssh cat /run/athanor-update/state.json | jq -r "$1"; }
marker() { guest_ssh cat /usr/share/athanor-acceptance-marker; }
check_now() { tunnel && guest_ssh sudo systemctl start athanor-update-check.service; }
point_stable() { skopeo copy --src-tls-verify=false --dest-tls-verify=false "docker://$REPO:$1" "docker://$REPO:stable" > /dev/null; }
# The images outside the acceptance set do not know the throwaway registry, which serves
# plain HTTP: a switch from one of them needs the drop-in every acceptance image ships
# (Containerfile).
know_acc_registry() {
  printf '[[registry]]\nlocation = "%s"\ninsecure = true\n' "${ACC_REGISTRY%%/*}" |
    guest_ssh sudo tee /etc/containers/registries.conf.d/50-acceptance.conf > /dev/null
}

# One tag trusted for one pull: the policy the guest runs, plus a sigstoreSigned scope for
# REPO:TAG with KEY, and a registries.d entry for the same tag. A tag scope is more specific
# than the repository scope of the rendered policy, which would otherwise decide, so every
# other scope reads as rendered; and two registries.d files declaring one scope are a hard
# error, so the entry names the tag too. The signature stays required. restore_policy takes
# it all back: call it before the reboot, since ostree merges /etc into the staged
# deployment on the way down, and again from the EXIT trap.
RENDERED_POLICY=/usr/share/athanor/containers/policy.json
ACC_TRUST_KEY=/etc/containers/athanor-acceptance-trust.pub
ACC_TRUST_YAML=/etc/containers/registries.d/50-athanor-acceptance-trust.yaml
trust_tag() { # trust_tag TAG KEY
  guest_ssh "sudo tee $ACC_TRUST_KEY > /dev/null" < "$2"
  guest_ssh cat "$RENDERED_POLICY" |
    jq --arg scope "$REPO:$1" --arg key "$ACC_TRUST_KEY" \
      '.transports.docker[$scope] = [{type: "sigstoreSigned", keyPath: $key, signedIdentity: {type: "matchRepository"}}]' |
    guest_ssh "sudo cp --remove-destination /dev/stdin /etc/containers/policy.json"
  printf 'docker:\n  %s:\n    use-sigstore-attachments: true\n' "$REPO:$1" | guest_ssh "sudo tee $ACC_TRUST_YAML > /dev/null"
}
restore_policy() {
  guest_ssh "sudo ln -sfn $RENDERED_POLICY /etc/containers/policy.json && sudo rm -f $ACC_TRUST_KEY $ACC_TRUST_YAML"
  [[ $(guest_ssh readlink /etc/containers/policy.json) == "$RENDERED_POLICY" ]] || die "FAIL  the policy link was not restored"
  guest_ssh "test ! -e $ACC_TRUST_KEY && test ! -e $ACC_TRUST_YAML" || die "FAIL  the acceptance trust was left on the guest"
}

pass() { echo "PASS  $*"; }
expect() { # expect DESCRIPTION JQ-FILTER VALUE
  local got
  got=$(state "$2")
  [[ $got == "$3" ]] || die "FAIL  $1: $2 is '$got', expected '$3'"
  pass "$1"
}
expect_until() { # expect_until DESCRIPTION JQ-FILTER VALUE MINUTES
  for _ in $(seq $(($4 * 6))); do
    [[ $(state "$2" 2> /dev/null) == "$3" ]] && { pass "$1"; return 0; }
    sleep 10
  done
  die "FAIL  $1: $2 never became '$3' in $4 minutes (it is '$(state "$2")')"
}

# dbus-send, not busctl: an error is asserted by its name, and busctl prints only its message.
# A call as the user of the graphical session (active, local): what the notifier is. It is made
# by a user timer ACC_CALL_DELAY seconds after this SSH login has ended: since ADR-0103 D22 any
# session of class user besides the caller's makes Apply() ask for an administrator, and the
# login that carries this command is one. call_active_start only arms the call.
# The timer is told to fire to the millisecond: its default accuracy is a minute, and a call
# that came late met the polling logins of the harness.
ACC_CALL_DELAY=8
ACC_CALL_SETTLE=45
call_out() { printf '/var/tmp/acc-call-%s.out' "$1"; }
call_active_start() { # call_active_start METHOD
  local out
  out=$(call_out "$1")
  untunnel
  guest_ssh "sudo rm -f $out && sudo systemd-run --quiet --user --machine=$GUEST_USER@.host --on-active=$ACC_CALL_DELAY --timer-property=AccuracySec=1ms --collect --unit=acc-call-$1 -p StandardOutput=file:$out dbus-send --system --print-reply --dest=${UPDATE1[0]} ${UPDATE1[1]} ${UPDATE1[2]}.$1"
}
call_active() { # call_active METHOD: the reply, or the error, of the call
  local out
  call_active_start "$1"
  # No login until the call is over: one open at the count before the reboot, which follows
  # the unlock by a few seconds, is a session of the caller's uid and makes the service ask.
  sleep $((ACC_CALL_DELAY + ACC_CALL_SETTLE))
  for _ in $(seq 40); do
    out=$(guest_ssh cat "$(call_out "$1")") && [[ -n $out ]] && { echo "$out"; return 0; }
    sleep 1
  done
  return 1
}
# A call from this SSH session (inactive, no polkit agent).
call_ssh() { guest_ssh -T dbus-send --system --print-reply --dest="${UPDATE1[0]}" "${UPDATE1[1]}" "${UPDATE1[2]}.$1" 2>&1; }
# A call as root, which polkit always authorizes: drives a scenario past a password prompt.
call_root() { guest_ssh sudo dbus-send --system --print-reply --dest="${UPDATE1[0]}" "${UPDATE1[1]}" "${UPDATE1[2]}.$1" 2>&1; }
expect_error() { # expect_error DESCRIPTION OUTPUT ERROR-NAME
  [[ $2 == *"$3"* ]] || die "FAIL  $1: expected $3, got: $2"
  pass "$1"
}

# A request that ends in a reboot: the bus connection may drop before the reply, so the exit
# status of the call says nothing. What is asserted is the version that boots.
request_and_reboot() { # request_and_reboot CALLER METHOD EXPECTED-MARKER DESCRIPTION
  local out status=0 boot
  boot=$(boot_id)
  out=$("$1" "$2") || status=$?
  wait_reboot "$boot" || die "FAIL  $4: no reboot in 10 minutes (call exit $status: $out)"
  [[ $(marker) == "$3" ]] || die "FAIL  $4: booted $(marker), expected $3 (call exit $status: $out)"
  pass "$4"
}

# The image digest the guest booted, as bootc reports it.
booted_digest() { guest_ssh sudo bootc status --format json | jq -r .status.booted.image.imageDigest; }
# request_and_reboot for a guest without the acceptance marker: what is asserted is the digest
# that boots.
reboot_into_digest() { # reboot_into_digest CALLER METHOD EXPECTED-DIGEST DESCRIPTION
  local out status=0 boot got
  boot=$(boot_id)
  out=$("$1" "$2") || status=$?
  wait_reboot "$boot" || die "FAIL  $4: no reboot in 10 minutes (call exit $status: $out)"
  got=$(booted_digest)
  [[ $got == "$3" ]] || die "FAIL  $4: booted $got, expected $3 (call exit $status: $out)"
  pass "$4"
}
# Stops the check timer and lets a check it already started, or queued before the stop, end:
# a check holds the store, and a request meanwhile answers Busy (#547).
quiet_checks() { # quiet_checks DESCRIPTION
  local check
  guest_ssh sudo systemctl stop athanor-update-check.timer
  for _ in $(seq 60); do
    check=$(guest_ssh 'systemctl show -p ActiveState --value athanor-update-check.service && systemctl list-jobs --no-legend athanor-update-check.service') ||
      die "FAIL  $1: cannot read the check's state"
    [[ $check == activating* || $check == *athanor-update-check* ]] || return 0
    sleep 5
  done
  die "FAIL  $1: the check the timer started still runs after five minutes"
}

# --- The other-session rule of UT6 (doc_update_trust.md section 6, item 2; ADR-0103 D22) ---
# The prompt is observed in two places. The agent of the graphical session opens a
# polkit-agent-helper@ instance for each request that needs a password (a request polkit
# answers by itself opens none), and polkitd logs the action it granted once the password is
# typed. The password of the dev VM is the user's name. guest_ssh does not multiplex
# (ControlMaster off, no ControlPersist): each call is a login of its own, and a login of the
# graphical session's user is a counting session while it lasts, hence the waits before a call.
ACC_OTHER=acc-other
ACC_INHIBITOR=acc-inhibit-sessions
ACC_UNITS=()

now_guest() { guest_ssh date +%s; }
type_keys() { guest_ssh "sudo python3 - $(printf '%q ' "$@")" < "$ACC_HERE/../keyboard_type.py"; }
# The journal of the units matching UNIT-GLOB since the epoch second SINCE; an SSH or
# journalctl failure stops the run instead of reading as an empty journal. journalctl -u fails
# on a glob that names no unit the journal holds ("Failed to add filter for units: No data
# available"), as on a fresh guest before the first polkit-agent-helper@ instance: the glob is
# first matched against the unit fields it expands from, and no match is an empty journal.
journal_since() { # journal_since SINCE UNIT
  local units unit matched=''
  if [[ $2 == *'*'* ]]; then
    # shellcheck disable=SC2016 # the guest's shell expands the field
    units=$(guest_ssh 'for field in _SYSTEMD_UNIT UNIT; do sudo journalctl -F "$field" || exit; done') || die "FAIL  the units of the journal could not be read"
    # shellcheck disable=SC2053 # UNIT is a glob
    while IFS= read -r unit; do [[ $unit == $2 ]] && matched=1; done <<< "$units"
    [[ -n $matched ]] || return 0
  fi
  guest_ssh sudo journalctl -q -b --since "@$1" -o cat -u "'$2'" || die "FAIL  the journal of $2 could not be read"
}
# Whether the agent opened a helper since the epoch second SINCE.
prompted() { local out; out=$(journal_since "$1" 'polkit-agent-helper@*') || die "FAIL  reading the helper journal"; [[ -n $out ]]; }
# The polkitd lines since SINCE that granted an action: TEXT is a fixed string of the line,
# "action os.athanor.update.apply-multi-session for" for one action, "action os.athanor.update."
# for any of ours. Fails when there are none. A refusal reads "FAILED to authenticate to gain
# authorization for action ...", so a grant is also a line of "successfully authenticated".
polkit_grant_lines() { grep -F "successfully authenticated" | grep -F "authorization for $1"; }
polkit_grants() { # polkit_grants SINCE TEXT
  local out
  out=$(journal_since "$1" polkit.service) || die "FAIL  reading the polkitd journal"
  polkit_grant_lines "$2" <<< "$out"
}
# Whether polkitd logged the registration of an authentication agent since SINCE.
polkit_grants_agent() {
  local out
  out=$(journal_since "$1" polkit.service) || die "FAIL  reading the polkitd journal"
  grep -qF "Registered Authentication Agent" <<< "$out"
}
# polkitd logs a grant and an agent's registration at info, and the image runs it at notice:
# the stage raises the level in a /run drop-in, so the grants it counts are in the journal. The
# restart drops the agent, so cosmic-osd is restarted, and its registration, an info line,
# proves the level took. polkit_log_restore takes the drop-in back; a reboot would too.
ACC_POLKIT_DROPIN=/run/systemd/system/polkit.service.d/50-acceptance-log-info.conf
polkit_restart_agent() {
  local since
  since=$(now_guest)
  guest_ssh "sudo systemctl daemon-reload && sudo systemctl restart polkit.service" || return 1
  guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" restart cosmic-osd.service || return 1
  for _ in $(seq 30); do
    polkit_grants_agent "$since" && return 0
    sleep 1
  done
  return 1
}
polkit_log_info() {
  local exec
  # A drop-in left by a killed run would be read back as a second ExecStart.
  polkit_log_restore || die "FAIL  could not remove a leftover $ACC_POLKIT_DROPIN"
  exec=$(guest_ssh systemctl cat polkit.service | grep '^ExecStart=/') || die "FAIL  no ExecStart in polkit.service"
  [[ $exec == *--log-level=notice* ]] || die "FAIL  polkitd is not started at --log-level=notice: $exec"
  printf '[Service]\nExecStart=\n%s\n' "${exec/--log-level=notice/--log-level=info}" |
    guest_ssh "sudo mkdir -p ${ACC_POLKIT_DROPIN%/*} && sudo tee $ACC_POLKIT_DROPIN > /dev/null"
  polkit_restart_agent || die "FAIL  cosmic-osd did not register with polkitd at --log-level=info in 30 s"
}
polkit_log_restore() {
  local rc=0
  guest_ssh "test -e $ACC_POLKIT_DROPIN" || rc=$?
  case $rc in
    0) ;;
    1) return 0 ;; # no drop-in
    *) return 1 ;; # ssh failed: the drop-in may still be there
  esac
  guest_ssh sudo rm -f "$ACC_POLKIT_DROPIN" || return 1
  guest_ssh "sudo systemctl daemon-reload && sudo systemctl restart polkit.service" || return 1
  guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" restart cosmic-osd.service
}
# The shape of the line polkitd logs for a granted prompt, matched by both forms above.
POLKIT_SAMPLE='Operator of unix-session:c2 successfully authenticated as unix-user:x to gain ONE-SHOT authorization for action os.athanor.update.apply-multi-session for system-bus-name::1.55 [dbus-send] (owned by unix-user:x)'
POLKIT_FAILED_SAMPLE='Operator of unix-session:1 FAILED to authenticate to gain authorization for action os.athanor.update.apply-multi-session for system-bus-name::1.122 [dbus-send] (owned by unix-user:x)'
polkit_pattern_selfcheck() {
  local text
  for text in "action os.athanor.update.apply-multi-session for" "action os.athanor.update."; do
    polkit_grant_lines "$text" <<< "$POLKIT_SAMPLE" > /dev/null || die "FAIL  the polkitd pattern '$text' does not match a granted line"
    ! polkit_grant_lines "$text" <<< "$POLKIT_FAILED_SAMPLE" > /dev/null || die "FAIL  the polkitd pattern '$text' matches a refused line"
  done
}
# Waits SECONDS at most for the output of a call to hold TEXT.
call_says() { # call_says METHOD TEXT SECONDS
  for _ in $(seq "$3"); do
    guest_ssh grep -qF -- "$2" "$(call_out "$1")" && return 0
    sleep 1
  done
  return 1
}
# What the user manager did with the armed call, for a failure message: the timer and the
# service state, and the unit's lines of the user journal.
call_diag() { # call_diag METHOD
  guest_ssh "sudo systemctl --user --machine=$GUEST_USER@.host list-timers --all --no-pager; sudo systemctl --user --machine=$GUEST_USER@.host status acc-call-$1.timer acc-call-$1.service --no-pager; sudo journalctl _UID=\$(id -u $GUEST_USER) -u user@\$(id -u $GUEST_USER).service --since -3min --no-pager | tail -n 20" 2>&1
}
deployment_locked() { [[ $(guest_ssh sudo bootc status --format json | jq -r .status.staged.downloadOnly) == true ]]; }

# The inhibitor that ends every authorised call of the stage at Error.Blocked, so that no case
# can reboot the guest: started by the stage, checked before each call.
need_inhibitor() { guest_ssh systemctl is-active --quiet "$ACC_INHIBITOR.service" || die "FAIL  the shutdown inhibitor $ACC_INHIBITOR is gone: a call could reboot the guest"; }

# The LoadState of a unit of the system manager; fails when it cannot be read.
unit_state() { guest_ssh systemctl show -p LoadState --value "$1"; }
# Units of the class_session kind, stopped by end_units. A name is registered before the unit
# is started, and a unit that never loaded is not stopped. The cleanup functions run every step
# and return 1 if one failed (set -e does not apply to them: they are called in a list).
stop_system_unit() { # stop_system_unit UNIT.service
  local state
  state=$(unit_state "$1") || return 1
  if [[ $state == loaded ]]; then guest_ssh sudo systemctl stop "$1" || return 1; fi
}
end_unit() { # end_unit NAME
  local unit keep=() rc=0
  stop_system_unit "$1.service" || rc=1
  for unit in "${ACC_UNITS[@]}"; do [[ $unit == "$1" ]] || keep+=("$unit"); done
  ACC_UNITS=("${keep[@]}")
  return $rc
}
end_units() {
  local rc=0
  while ((${#ACC_UNITS[@]})); do end_unit "${ACC_UNITS[0]}" || rc=1; done
  return $rc
}
# A pending or finished call of the user manager, its timer included.
end_call() {
  local unit state rc=0
  for unit in acc-call-Apply.timer acc-call-Apply.service; do
    state=$(guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" show -p LoadState --value "$unit") || { rc=1; continue; }
    if [[ $state == loaded ]]; then guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" stop "$unit" || rc=1; fi
  done
  return $rc
}
# A session of the other user, of the logind class CLASS, held open by a unit with a PAM stack.
class_session() { # class_session CLASS
  local classes
  ACC_UNITS+=("acc-class-$1")
  guest_ssh sudo systemd-run --quiet "--unit=acc-class-$1" -p "User=$ACC_OTHER" -p PAMName=login -p "Environment=XDG_SESSION_CLASS=$1" sleep 3600
  for _ in $(seq 20); do
    classes=$(other_classes)
    grep -qx "$1" <<< "$classes" && return 0
    sleep 1
  done
  die "FAIL  no session of class $1 for $ACC_OTHER after 20 s (classes: $(tr '\n' ' ' <<< "$classes"))"
}
other_classes() { # the logind class of each session of the other user, one per line
  guest_ssh bash -s "$ACC_OTHER" <<'REMOTE'
for s in $(loginctl list-sessions --no-legend | awk -v u="$1" '$3 == u { print $1 }'); do
  loginctl show-session "$s" -p Class --value
done
REMOTE
}
# Whether the other user still has processes (exit 1) or none (0).
other_gone() { guest_ssh "loginctl list-users --no-legend | awk -v u=$ACC_OTHER '\$2 == u { f = 1 } END { exit f }'"; }
# Waits for the other user to have no session, then removes the account.
remove_other() {
  # terminate-user fails for a user with no session and no lingering: nothing to end then.
  if [[ -n $(guest_ssh loginctl list-users --no-legend | awk -v u="$ACC_OTHER" '$2 == u') ]]; then
    guest_ssh sudo loginctl terminate-user "$ACC_OTHER" || return 1
  fi
  for _ in $(seq 30); do other_gone && break; sleep 1; done
  other_gone || { echo "FAIL  $ACC_OTHER still has processes 30 s after terminate-user" >&2; return 1; }
  guest_ssh sudo userdel -r "$ACC_OTHER"
}
# The wrapper of second-count.sh over bootc, if it is still mounted.
unmount_wrapper() {
  # shellcheck disable=SC2016 # expanded by the guest's shell
  guest_ssh 'for m in /usr/bin/bootc /run/acc-bootc-real; do if findmnt -n "$m" > /dev/null; then sudo umount "$m" || exit 1; fi; done'
}
# A second login of the user of the graphical session, over SSH; ended by end_ssh_session.
ssh_session() {
  guest_ssh -f -T 'exec -a acc-session-hold sleep 900' < /dev/null
  for _ in $(seq 20); do
    guest_ssh pgrep -f '[a]cc-session-hold' > /dev/null && return 0 # the output is the pid; its absence is the exit status
    sleep 1
  done
  die "FAIL  the second SSH session of $GUEST_USER did not start in 20 s"
}
end_ssh_session() { # a process already gone is fine (pgrep 1); an SSH failure is not
  local rc=0
  guest_ssh pgrep -f '[a]cc-session-hold' > /dev/null || rc=$? # the pid is not wanted
  case $rc in
    0) guest_ssh pkill -f '[a]cc-session-hold' ;;
    1) return 0 ;;
    *) return 1 ;;
  esac
}

# Arms Apply() from the graphical session and checks that it asked an administrator: the
# agent opens a helper, the password is typed into it, polkitd logs the grant of ACTION once
# and the call comes back as Blocked (the stage holds a shutdown inhibitor, so nothing is
# applied). Called N times in a row it checks that each call asked again.
expect_prompts() { # expect_prompts DESCRIPTION ACTION N
  local since call n
  since=$(now_guest)
  for n in $(seq "$3"); do
    need_inhibitor
    call=$(now_guest)
    call_active_start Apply
    sleep $((ACC_CALL_DELAY + 2)) # no login meanwhile: the count is of the sessions the case made
    for _ in $(seq 30); do
      prompted "$call" && break
      sleep 1
    done
    prompted "$call" || die "FAIL  $1: call $n opened no prompt in the agent"
    sleep 2 # the dialog takes the keys only once it is drawn
    type_keys "$(guest_ssh id -un)" @enter
    call_says Apply os.athanor.Update1.Error.Blocked 30 || die "FAIL  $1: call $n did not reach the inhibitor check after the password: $(guest_ssh cat "$(call_out Apply)")"
    [[ $(polkit_grants "$since" "action $2 for" | grep -c .) -eq $n ]] || die "FAIL  $1: after call $n polkitd logged $(polkit_grants "$since" "action $2 for" | grep -c .) grants of $2, expected $n"
  done
  deployment_locked || die "FAIL  $1: the deployment was unlocked"
  echo "EVIDENCE  $1:"
  polkit_grants "$since" "action $2 for" | sed 's/^/EVIDENCE    /'
  pass "$1"
}

# Arms Apply() and checks that no one was asked: no helper opens, polkitd grants nothing, and
# the call is authorised, so it ends at the inhibitor the stage holds.
expect_no_prompt() { # expect_no_prompt DESCRIPTION
  local since
  need_inhibitor
  since=$(now_guest)
  call_active_start Apply
  sleep $((ACC_CALL_DELAY + 2))
  call_says Apply os.athanor.Update1.Error.Blocked 30 || die "FAIL  $1: not authorised without a password: $(guest_ssh cat "$(call_out Apply)" 2>&1; call_diag Apply)"
  ! prompted "$since" || die "FAIL  $1: the agent opened a prompt"
  ! polkit_grants "$since" "action os.athanor.update." > /dev/null || die "FAIL  $1: polkitd granted an action after a password"
  deployment_locked || die "FAIL  $1: the deployment was unlocked"
  echo "EVIDENCE  $1: no polkit-agent-helper@ instance and no polkitd grant since @$since; the call ended at os.athanor.Update1.Error.Blocked"
  pass "$1"
}
