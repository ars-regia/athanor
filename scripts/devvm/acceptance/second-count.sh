#!/usr/bin/env bash
# Runs on the guest, as root, in a transient unit (run.sh, stage `sessions`): the case of UT6
# in which a counting session opens after the unlock and before the reboot. The unit has no
# login session, so the only session besides the graphical one is the one it opens itself.
# It arms Apply() from the graphical session, stops `bootc upgrade --from-downloaded` the
# moment it runs, opens a session of the other user, lets bootc go on, and logs each step to
# $ACC_LOG. Needs ACC_USER (the graphical user), ACC_OTHER, ACC_DELAY and ACC_LOG.
set -euo pipefail
log() { printf '%s %s\n' "$(date +%s)" "$*" >> "$ACC_LOG"; }
: > "$ACC_LOG"
sleep "$ACC_DELAY" # the SSH login that started this unit is over before the first count
out=/var/tmp/acc-call-Apply.out
rm -f "$out"
systemd-run --quiet --user --machine="$ACC_USER@.host" --collect --unit=acc-call-Apply -p "StandardOutput=file:$out" \
  dbus-send --system --print-reply --dest=os.athanor.Update1 /os/athanor/Update1 os.athanor.Update1.Apply
log "called Apply"
pid=
for _ in $(seq 1200); do
  pid=$(pgrep -f 'bootc upgrade --from-downloaded' | head -n 1) && [[ -n $pid ]] && break
  sleep 0.05
done
[[ -n $pid ]] || { log "no bootc upgrade --from-downloaded in 60 s"; exit 1; }
kill -STOP "$pid"
log "stopped bootc $pid"
systemd-run --quiet --unit=acc-class-user -p "User=$ACC_OTHER" -p PAMName=login -p Environment=XDG_SESSION_CLASS=user sleep 3600
for _ in $(seq 40); do
  loginctl list-sessions --no-legend | awk -v u="$ACC_OTHER" '$3 == u { found = 1 } END { exit !found }' && break
  sleep 0.5
done
loginctl list-sessions --no-legend | awk -v u="$ACC_OTHER" '$3 == u { found = 1 } END { exit !found }' || { kill -CONT "$pid"; log "no session of $ACC_OTHER"; exit 1; }
log "opened a session of $ACC_OTHER"
kill -CONT "$pid"
log "continued bootc $pid"
