# shellcheck shell=bash
# Shared helpers of the update and trust acceptance (scripts/devvm/acceptance/README.md).
# Sourced, never run. Everything here uses a throwaway registry and throwaway keys.
ACC_HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$ACC_HERE/../../.." && pwd)
# shellcheck source-path=SCRIPTDIR
source "$ACC_HERE/../devvm.env"

ACC_REGISTRY=${ACC_REGISTRY:-localhost:5000/acc}
ACC_PORT=${ACC_PORT:-5000}
ACC_STATE=${ACC_STATE:-$STATE/acceptance}
ACC_BASE=${ACC_BASE:-$SYSTEM_IMAGE:latest}
ACC_RPM_DIR=${ACC_RPM_DIR:-$ROOT/RPMS_OUT}
REPO=$ACC_REGISTRY/athanor-system
UPDATE1=(os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1)
mkdir -p "$ACC_STATE"

# The 31 GB host cannot hold the 16 GB runner guest and this 8 GB VM together.
guard_no_ci() {
  local busy
  busy=$(gh api "repos/${GITHUB_REPOSITORY:-hr-mes/athanor}/actions/runs?status=in_progress&per_page=1" --jq .total_count)
  [[ $busy -eq 0 ]] || die "$busy workflow run(s) in progress: the dev VM must not run beside a CI job on this host"
}

wait_ssh() {
  for _ in $(seq 180); do
    if guest_ssh -q true 2> /dev/null; then tunnel; return 0; fi
    sleep 2
  done
  die "no SSH on 127.0.0.1:$SSH_PORT after 6 minutes"
}

# The guest reaches the host's registry on its own 127.0.0.1, so signer and verifier spell
# the repository identically (a signature records the reference it was made for).
tunnel() {
  guest_ssh -q "ss -ltn | grep -q ':$ACC_PORT '" 2> /dev/null && return 0
  ssh -i "$(ssh_key)" -p "$SSH_PORT" -o "UserKnownHostsFile=$STATE/known_hosts" -o ExitOnForwardFailure=yes \
    -f -N -R "$ACC_PORT:127.0.0.1:$ACC_PORT" "$GUEST_USER@127.0.0.1"
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
check_now() { guest_ssh sudo systemctl start athanor-update-check.service; }
point_stable() { skopeo copy --src-tls-verify=false --dest-tls-verify=false "docker://$REPO:$1" "docker://$REPO:stable" > /dev/null; }

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
# A call as the user of the graphical session (active, local): what the notifier is.
call_active() { guest_ssh sudo systemd-run --quiet --wait --pipe --user --machine="$GUEST_USER@.host" dbus-send --system --print-reply --dest="${UPDATE1[0]}" "${UPDATE1[1]}" "${UPDATE1[2]}.$1" 2>&1; }
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
