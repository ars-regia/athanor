#!/usr/bin/env bash
# Runs on the guest, as root, in a transient unit (run.sh, stage `sessions`): the case of UT6
# in which a counting session opens after the unlock and before the reboot. The unit has no
# login session, so the only session besides the graphical one is the one it opens itself, and
# the harness opens no login until this script is done (run.sh waits).
#
# Apply() unlocks with `bootc upgrade --from-downloaded`. /usr/bin/bootc is replaced for the
# duration by a bind-mounted wrapper that, for that one command, creates /run/acc-bootc-entered
# and waits for /run/acc-bootc-go before it runs the real bootc (bind-mounted at
# /run/acc-bootc-real). The window is as long as this script keeps it, whatever a poll
# catches.
#
# Two safety nets, so that no outcome restarts the guest:
#  1. The stage's inhibitor ($ACC_INHIBITOR) is dropped here, and only after the update
#     service, restarted after the mounts and activated by State(), is seen through
#     nsenter to have the wrapper at /usr/bin/bootc and the real bootc at /run/acc-bootc-real.
#     If it does not, the script exits with the inhibitor still held.
#  2. The wrapper is entered only after the service's inhibitor query, so this script takes a
#     block inhibitor of its own the moment the wrapper is entered. If the second count misses
#     the session, the reboot ends as Error.Blocked. The wrapper gives up after 120 s (bootc
#     then fails and the service refuses), which also restarts nothing.
# Needs ACC_USER (the graphical user), ACC_OTHER, ACC_DELAY, ACC_INHIBITOR and ACC_LOG.
set -euo pipefail
log() { printf '%s %s\n' "$(date +%s)" "$*" >> "$ACC_LOG"; }
: > "$ACC_LOG"
wrapper=/run/acc-bootc-wrapper
counts() { # counts: the sessions of ACC_USER or ACC_OTHER, as `session user class`
  local s
  for s in $(loginctl list-sessions --no-legend | awk -v a="$ACC_USER" -v b="$ACC_OTHER" '$3 == a || $3 == b { print $1 }'); do
    echo "$s $(loginctl show-session "$s" -p Name --value) $(loginctl show-session "$s" -p Class --value)"
  done
}
cleanup() {
  local m
  for m in /usr/bin/bootc /run/acc-bootc-real; do
    if findmnt -n "$m" > /dev/null; then
      for _ in $(seq 40); do
        umount "$m" && break
        sleep 0.5
      done
      if findmnt -n "$m" > /dev/null; then log "could not unmount $m"; fi
    fi
  done
  rm -f "$wrapper" /run/acc-bootc-real /run/acc-bootc-entered /run/acc-bootc-go
  log "wrapper removed"
}
trap cleanup EXIT
cat > "$wrapper" << 'WRAPPER'
#!/bin/bash
if [[ ${1:-} == upgrade && ${2:-} == --from-downloaded ]]; then
  : > /run/acc-bootc-entered
  for _ in $(seq 1200); do
    [[ -e /run/acc-bootc-go ]] && exec /run/acc-bootc-real "$@"
    sleep 0.1
  done
  echo "acc wrapper: no go file in 120 s, bootc not run" >&2
  exit 1
fi
exec /run/acc-bootc-real "$@"
WRAPPER
chmod 0755 "$wrapper"
chcon --reference=/usr/bin/bootc "$wrapper"
rm -f /run/acc-bootc-entered /run/acc-bootc-go
real_sum=$(sha256sum /usr/bin/bootc | cut -d' ' -f1)
wrapper_sum=$(sha256sum "$wrapper" | cut -d' ' -f1)
: > /run/acc-bootc-real
mount --bind /usr/bin/bootc /run/acc-bootc-real
mount --bind "$wrapper" /usr/bin/bootc
log "wrapper mounted"

# The update service builds its mount namespace when it starts: stop it, start it again with a
# call that needs no polkit, and read the two files from inside that namespace.
systemctl stop athanor-update.service
dbus-send --system --print-reply --dest=os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1.State > /dev/null
pid=$(systemctl show -p MainPID --value athanor-update.service)
[[ $pid -gt 0 ]] || { log "athanor-update.service has no main pid after State()"; exit 1; }
seen=$(nsenter -t "$pid" -m sha256sum /usr/bin/bootc /run/acc-bootc-real | cut -d' ' -f1 | tr '\n' ' ')
[[ $seen == "$wrapper_sum $real_sum " ]] || { log "the update service does not see the wrapper (saw: $seen; wanted: $wrapper_sum $real_sum); the inhibitor stays"; exit 1; }
log "the update service (pid $pid) sees the wrapper and the real bootc"
systemctl stop "$ACC_INHIBITOR.service"
log "stage inhibitor dropped"

sleep "$ACC_DELAY" # the SSH login that started this unit is over before the first count
out=/var/tmp/acc-call-Apply.out
rm -f "$out"
systemd-run --quiet --user --machine="$ACC_USER@.host" --collect --unit=acc-call-Apply -p "StandardOutput=file:$out" \
  dbus-send --system --print-reply --dest=os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1.Apply
log "called Apply"
for _ in $(seq 600); do
  [[ -e /run/acc-bootc-entered ]] && break
  sleep 0.1
done
[[ -e /run/acc-bootc-entered ]] || { log "bootc never reached the unlock in 60 s"; exit 1; }
systemd-run --quiet --unit=acc-inhibit-second systemd-inhibit --what=shutdown --mode=block --who=acceptance --why=ut6-second-count sleep 3600
log "bootc held at the unlock; inhibitor taken"
systemd-run --quiet --unit=acc-class-user -p "User=$ACC_OTHER" -p PAMName=login -p Environment=XDG_SESSION_CLASS=user sleep 3600
for _ in $(seq 100); do
  counts | awk -v u="$ACC_OTHER" '$2 == u { found = 1 } END { exit !found }' && break
  sleep 0.2
done
counts | awk -v u="$ACC_OTHER" '$2 == u { found = 1 } END { exit !found }' || { log "no session of $ACC_OTHER in 20 s; the wrapper times out and the call fails"; exit 1; }
# The session the second count sees must be this one: the caller's user may have only its
# graphical session, whatever the class of the others.
extra=$(counts | awk -v u="$ACC_USER" '$2 == u && $3 !~ /^(greeter|lock-screen|background|background-light|manager|manager-early|none)$/ { n++ } END { print n + 0 }')
log "sessions at the unlock: $(counts | tr '\n' ';')"
[[ $extra -eq 1 ]] || { log "$ACC_USER has $extra counting sessions, expected the graphical one only; the wrapper times out"; exit 1; }
log "opened a session of $ACC_OTHER"
: > /run/acc-bootc-go
log "released bootc"
# The wrapper stays mounted until the wrapper and the real bootc have finished.
for _ in $(seq 300); do
  pgrep -f 'bootc upgrade --from-downloaded' > /dev/null || break
  sleep 0.2
done
if pgrep -f 'bootc upgrade --from-downloaded' > /dev/null; then log "bootc still runs after 60 s"; exit 1; fi
