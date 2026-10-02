#!/usr/bin/env bash
# One job of the self-hosted runner (scripts/runner/README.md). Asks GitHub for a
# just-in-time runner configuration, boots the base image with snapshot=on (no write
# reaches it) and a scratch disk recreated empty, and passes the configuration to the
# guest as a systemd credential through fw_cfg. The job is over when GitHub removes the
# just-in-time registration: vm.sh then powers the guest off through QMP, whatever the
# runner inside does, and athanor-runner.service starts the next one. Runs as the
# dynamic user of the service: STATE_DIRECTORY holds golden.qcow2 and the disks, the
# GitHub token is the service credential github-token and never appears on a command
# line.
#
# Reaching the GitHub API is never allowed to end a running job: a DNS blip, a reset
# connection or a 5xx/429 is retried with backoff, and only a definitive 404 (the
# registration is actually gone) powers the guest off. See registration_status().
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/runner.env"
die() { echo "error: $*" >&2; exit 1; }

STATE=${STATE_DIRECTORY:?run by athanor-runner.service}
RUNTIME=${RUNTIME_DIRECTORY:?run by athanor-runner.service}
LOGS=${LOGS_DIRECTORY:?run by athanor-runner.service}
TOKEN=${CREDENTIALS_DIRECTORY:?run by athanor-runner.service}/github-token
[[ -f $STATE/golden.qcow2 ]] || die "$STATE/golden.qcow2 missing: run build-image.sh and install.sh"
umask 077

# How often the registration is checked; how many times and how long registration_status
# backs off on a transient failure within one check; how long the API can stay
# unreachable before it is merely logged (the guest's own job timeout, RuntimeMaxSec in
# the unit, is the real backstop — killing a running job over a network blip is worse
# than a late shutdown); how long the guest has to power off after the ACPI request
# before it is terminated.
POLL_SECONDS=30
POLL_RETRY_ATTEMPTS=4
POLL_RETRY_SECONDS=5
UNREACHABLE_WARN_SECONDS=900
POWERDOWN_TIMEOUT=120

github() { # github METHOD PATH [JSON]: HTTP status to stdout, body to $RUNTIME/response.
           # Exits 0 whenever the request reached GitHub, whatever status came back; a
           # nonzero exit is curl's own, from a transport failure (DNS, a reset
           # connection, a timeout) that never produced a status at all. Callers must
           # never assign its result outside an `if`/`||`: under `set -e`, a bare
           # `status=$(github ...)` aborts the whole script the instant curl fails —
           # the bug that once let a DNS blip cancel a running job.
  curl -sS -o "$RUNTIME/response" -w '%{http_code}' -X "$1" -H @- \
    -H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28' \
    ${3:+--data "$3"} "https://api.github.com/$2" <<< "Authorization: Bearer $(< "$TOKEN")"
}

registration_status() { # sets $reg_status to a definitive HTTP status (whatever GitHub
  # actually returned, including 200/404) or to "unreachable" once a transport failure
  # or a 429/5xx has survived POLL_RETRY_ATTEMPTS tries with a short doubling backoff.
  local attempt=1 delay=$POLL_RETRY_SECONDS
  while true; do
    if reg_status=$(github GET "repos/$REPOSITORY/actions/runners/$id"); then
      case $reg_status in
        429 | 5[0-9][0-9]) : ;; # transient HTTP failure, retry below
        *) return 0 ;;
      esac
    else
      reg_status=unreachable
    fi
    (( attempt < POLL_RETRY_ATTEMPTS )) || return 0
    sleep "$delay"
    delay=$(( delay * 2 ))
    attempt=$(( attempt + 1 ))
  done
}

qmp() { # qmp COMMAND: one command on the QMP socket of the guest, failing on a QMP error
  python3 - "$RUNTIME/qmp.sock" "$1" <<'PY'
import json
import socket
import sys

sock = socket.socket(socket.AF_UNIX)
sock.connect(sys.argv[1])
stream = sock.makefile("rw")
stream.readline()  # the greeting
for command in ("qmp_capabilities", sys.argv[2]):
    stream.write(json.dumps({"execute": command}) + "\n")
    stream.flush()
    while True:  # asynchronous events may arrive before the reply
        reply = json.loads(stream.readline())
        if "error" in reply:
            sys.exit(f"{command}: {reply['error']['desc']}")
        if "return" in reply:
            break
PY
}

name=athanor-vm-$(date -u +%Y%m%d%H%M%S)
body=$(jq -cn --arg name "$name" --arg labels "$RUNNER_LABELS" \
  '{name: $name, runner_group_id: 1, labels: ($labels | split(",")), work_folder: "/var/lib/runner/work"}')
if status=$(github POST "repos/$REPOSITORY/actions/runners/generate-jitconfig" "$body"); then
  [[ $status == 201 ]] || die "generate-jitconfig: HTTP $status: $(< "$RUNTIME/response")"
else
  die "generate-jitconfig: GitHub unreachable"
fi
id=$(jq -r .runner.id "$RUNTIME/response")
jq -r .encoded_jit_config "$RUNTIME/response" > "$RUNTIME/jitconfig"
rm "$RUNTIME/response"

qemu_pid=''
balloon_pid=''
guest_running() { [[ $qemu_pid ]] && kill -0 "$qemu_pid" 2> /dev/null; }

stop_guest() { # ACPI power-off, then termination if the guest has not complied in time
  guest_running || return 0
  qmp system_powerdown || echo "warning: runner $name ($id): QMP power-off failed" >&2
  local waited
  for ((waited = 0; waited < POWERDOWN_TIMEOUT; waited += 5)); do
    guest_running || return 0
    sleep 5
  done
  echo "warning: runner $name ($id): guest still running after ${POWERDOWN_TIMEOUT}s, terminating it" >&2
  if ! kill "$qemu_pid" 2> /dev/null; then
    echo "runner $name ($id): guest was already gone when terminating it" >&2
  fi
}

cleanup() {
  # Runs as the EXIT trap under set -e: every fallible command here is guarded, or a
  # network hiccup at the end of a successful job would report the job as failed.
  stop_guest
  # The regulator exits by itself once QEMU closes its socket; this covers a guest that
  # never came up.
  if [[ $balloon_pid ]] && kill -0 "$balloon_pid" 2> /dev/null; then
    kill "$balloon_pid" 2> /dev/null || echo "runner $name ($id): balloon regulator already gone" >&2
  fi
  rm -f "$RUNTIME/jitconfig"
  # GitHub removes a just-in-time runner after its job; one whose guest stopped before
  # taking a job stays registered and is removed here. The API being unreachable at this
  # point is only ever logged, never turned into a failing exit.
  registration_status
  case $reg_status in
    200)
      if status=$(github DELETE "repos/$REPOSITORY/actions/runners/$id"); then
        [[ $status == 204 ]] || echo "warning: runner $name ($id) not removed: HTTP $status" >&2
      else
        echo "warning: runner $name ($id) not removed: GitHub unreachable" >&2
      fi ;;
    404) ;;
    unreachable) echo "warning: runner $name ($id) not checked: GitHub unreachable" >&2 ;;
    *) echo "warning: runner $name ($id) not checked: HTTP $reg_status" >&2 ;;
  esac
  rm -f "$RUNTIME/response"
}
trap cleanup EXIT
trap 'exit 143' TERM

[[ -f $STATE/cache.raw ]] || truncate -s "$CACHE_DISK_SIZE" "$STATE/cache.raw"
rm -f "$STATE/scratch.raw"
truncate -s "$SCRATCH_DISK_SIZE" "$STATE/scratch.raw"

echo "runner $name ($id): booting, serial console in $LOGS/console.log"
qemu-system-x86_64 \
  -machine q35,accel=kvm -cpu host -smp "$VM_CPUS" -m "$VM_MEMORY" \
  -nodefaults -display none -no-reboot -serial "file:$LOGS/console.log" \
  -qmp "unix:$RUNTIME/qmp.sock,server=on,wait=off" \
  -qmp "unix:$RUNTIME/balloon.sock,server=on,wait=off" \
  -device virtio-rng-pci \
  -device virtio-balloon-pci,free-page-reporting=on,deflate-on-oom=on \
  -drive "if=none,id=system,format=qcow2,snapshot=on,file=$STATE/golden.qcow2" \
  -device virtio-blk-pci,drive=system,bootindex=0 \
  -drive "if=none,id=cache,format=raw,discard=unmap,file=$STATE/cache.raw" \
  -device virtio-blk-pci,drive=cache,serial=runner-cache \
  -drive "if=none,id=scratch,format=raw,discard=unmap,file=$STATE/scratch.raw" \
  -device virtio-blk-pci,drive=scratch,serial=runner-scratch \
  -netdev passt,id=net0 -device virtio-net-pci,netdev=net0 \
  -fw_cfg "name=opt/io.systemd.credentials/jitconfig,file=$RUNTIME/jitconfig" &
qemu_pid=$!

# The balloon regulator (balloon.py) has a QMP socket of its own, so stop_guest never
# waits on it. It is not part of the job: if it fails, the job goes on and the reason
# is in the journal.
VM_MEMORY=$VM_MEMORY BALLOON_FLOOR=$BALLOON_FLOOR HOST_RESERVE=$HOST_RESERVE \
  python3 "$HERE/balloon.py" "$RUNTIME/balloon.sock" &
balloon_pid=$!

# The guest may power itself off (the runner service exits after its job); the host does
# not rely on it and ends the guest once the registration is gone. A GitHub outage never
# ends it: registration_status already retries a transient failure, and even a long one
# only logs a warning here (see UNREACHABLE_WARN_SECONDS above) — only a definitive 404
# powers the guest off.
unreachable_since=''
while guest_running; do
  sleep "$POLL_SECONDS" & wait $!
  guest_running || break
  registration_status
  case $reg_status in
    404)
      echo "runner $name ($id): job finished, registration removed by GitHub; powering the guest off"
      stop_guest
      break ;;
    200)
      unreachable_since='' ;;
    *)
      if [[ -z $unreachable_since ]]; then
        unreachable_since=$SECONDS
      elif (( SECONDS - unreachable_since >= UNREACHABLE_WARN_SECONDS )); then
        echo "warning: runner $name ($id): GitHub unreachable for ${UNREACHABLE_WARN_SECONDS}s (last: $reg_status); guest kept running, its own RuntimeMaxSec is the backstop" >&2
        unreachable_since=$SECONDS # re-arm: warn again after another stretch, not every tick
      fi ;;
  esac
done

if wait "$qemu_pid"; then
  echo "runner $name ($id): guest powered off"
else
  echo "runner $name ($id): qemu exited with status $?"
fi
