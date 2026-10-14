#!/usr/bin/env bash
# Runs on the guest, as root, in a transient unit (run.sh, stage `sessions`): the case of UT6
# in which a counting session opens after the unlock and before the reboot. The unit has no
# login session, so the only session besides the graphical one is the one it opens itself.
#
# Apply() unlocks with `bootc upgrade --from-downloaded`. /usr/bin/bootc is replaced for the
# duration by a bind-mounted wrapper that, for that one command, creates /run/acc-bootc-entered
# and waits for /run/acc-bootc-go before it runs the real bootc (bind-mounted at
# /run/acc-bootc-real). The window is as long as this script keeps it, whatever a poll
# catches. Its safety net: the wrapper is only entered after the service's inhibitor query, so
# the sessions stage's own inhibitor could not be held. This script takes one the moment the
# wrapper is entered and keeps it until the stage stops it: if the second count misses the
# session, the reboot ends as Error.Blocked and the guest stays up. The wrapper gives up after
# 120 s (bootc then fails and the service refuses), which also reboots nothing.
# Needs ACC_USER (the graphical user), ACC_OTHER, ACC_DELAY and ACC_LOG.
set -euo pipefail
log() { printf '%s %s\n' "$(date +%s)" "$*" >> "$ACC_LOG"; }
: > "$ACC_LOG"
wrapper=/run/acc-bootc-wrapper
cleanup() {
  umount /usr/bin/bootc
  umount /run/acc-bootc-real
  rm -f "$wrapper" /run/acc-bootc-real /run/acc-bootc-entered /run/acc-bootc-go
  log "unmounted the wrapper"
}
cat > "$wrapper" << 'WRAPPER'
#!/bin/bash
if [[ ${1:-} == upgrade && ${2:-} == --from-downloaded ]]; then
  : > /run/acc-bootc-entered
  for _ in $(seq 1200); do
    [[ -e /run/acc-bootc-go ]] && exec /run/acc-bootc-real "$@"
    sleep 0.1
  done
  exit 1
fi
exec /run/acc-bootc-real "$@"
WRAPPER
chmod 0755 "$wrapper"
chcon --reference=/usr/bin/bootc "$wrapper"
rm -f /run/acc-bootc-entered /run/acc-bootc-go
: > /run/acc-bootc-real
mount --bind /usr/bin/bootc /run/acc-bootc-real
mount --bind "$wrapper" /usr/bin/bootc
trap cleanup EXIT
log "wrapper mounted"
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
  loginctl list-sessions --no-legend | awk -v u="$ACC_OTHER" '$3 == u { found = 1 } END { exit !found }' && break
  sleep 0.2
done
loginctl list-sessions --no-legend | awk -v u="$ACC_OTHER" '$3 == u { found = 1 } END { exit !found }' || { log "no session of $ACC_OTHER in 20 s; the wrapper times out and the call fails"; exit 1; }
log "opened a session of $ACC_OTHER"
: > /run/acc-bootc-go
log "released bootc"
# The wrapper stays mounted until the real bootc has finished.
for _ in $(seq 300); do
  pgrep -f 'acc-bootc-real' > /dev/null || break
  sleep 0.2
done
