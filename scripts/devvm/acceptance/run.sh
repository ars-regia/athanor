#!/usr/bin/env bash
# The acceptance of docs/architecture/doc_update_trust.md, section 6, items 1 to 15, on the
# development VM, against the registry and keys of images.sh. Repeatable: `reset.sh` first.
# Every bootc call is made by the units themselves (item 14): this script only starts units
# and calls the bus. Usage: run.sh [START_AT]   (a stage name below; default: the first)
#
# run.sh --ci [START_AT] runs instead the upgrade and rollback of doc_update_delivery.md UD18
# (ADR-0130) on the VM of forge/test/iso/run_iso_test.sh, which calls it in CI and passes the
# candidate, its mirror, the forwarded SSH port and the key of the run in the environment.
set -euo pipefail
ACC_CI=''
if [[ ${1:-} == --ci ]]; then
  [[ ${CI:-} == true ]] || {
    echo "run.sh --ci runs only in CI (CI=true), on the VM of forge/test/iso/run_iso_test.sh" >&2
    exit 2
  }
  ACC_CI=1
  shift
fi
# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

if [[ -n $ACC_CI ]]; then
  # A hosted runner: the self-hosted runner's guard does not apply.
  STAGES=(seed upgrade check goback check-back)
  start=${1:-seed}
else
  guard_no_ci
  STAGES=(install migrate metered download sessions apply refuse older goback rotate recover console greenboot podman probes report)
  start=${1:-install}
fi
[[ " ${STAGES[*]} " == *" $start "* ]] || die "unknown stage '$start': one of ${STAGES[*]}"

stage_install() { # the machine starts from v1, on a reference that verifies nothing
  point_stable v1 # where images.sh left it; an earlier run moved it on
  wait_ssh
  know_acc_registry
  guest_ssh sudo bootc switch --transport registry "$REPO:v1"
  reboot_guest
  [[ $(marker) == v1 ]] || die "the guest did not boot v1"
}

staged_signature() { guest_ssh sudo bootc status --format json | jq -r '.status.staged.image.image.signature // empty'; }

stage_migrate() { # items 7 and 8
  expect "7: not verified before the migration" .verified.reason media
  expect "7: and nothing is downloaded meanwhile" .update none
  guest_ssh 'systemctl is-active athanor-update-migrate.service || sudo journalctl -u athanor-update-migrate.service -n 5 --no-pager'
  # The switch is staged with the policy enforced; the stamp follows the boot of it.
  for _ in $(seq 60); do [[ $(staged_signature) == containerPolicy ]] && break; sleep 10; done
  [[ $(staged_signature) == containerPolicy ]] || die "FAIL  the migration did not stage the signed reference in 10 minutes"
  reboot_guest
  for _ in $(seq 30); do guest_ssh test -e /var/lib/athanor-update/migrated && break; sleep 10; done
  guest_ssh test -e /var/lib/athanor-update/migrated || die "FAIL  the migration did not complete after the boot of the signed reference"
  [[ $(marker) == v1 ]] || die "FAIL  7: the migration changed the version"
  expect "7: verified after the migration, with no update in between" .verified.reason signature
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.booted.image.image.signature) == containerPolicy ]] || die "FAIL  8: the reference does not enforce the policy"
  pass "8: the machine follows the signed reference"
}

# The device of the default route, and NetworkManager's verdict on the network the check reads.
default_device() { guest_ssh "ip route show default | awk '{ print \$5; exit }'"; }
nm_metered() { guest_ssh busctl --system get-property org.freedesktop.NetworkManager /org/freedesktop/NetworkManager org.freedesktop.NetworkManager Metered; }

# `nmcli device modify` changes the applied connection, not the profile on disk: reapplying the
# profile drops the mark, and so does a reboot.
ACC_METERED_DEV=''
metered_cleanup() {
  [[ -n $ACC_METERED_DEV ]] || return 0
  guest_ssh sudo nmcli device reapply "$ACC_METERED_DEV" || { echo "FAIL  the cleanup of the metered stage could not reapply $ACC_METERED_DEV" >&2; return 1; }
  ACC_METERED_DEV=''
}

# The check is started by hand, and a start of the service moves the timer's next run six hours
# on: the stage stops the timer for its duration and ends with a reboot, whose timer run two
# minutes after boot is the one the download stage waits for.
stage_metered() { # UT12; doc_shell.md section 7, item 4: on a metered network the check runs and the download does not
  local before metered
  guest_ssh sudo systemctl stop athanor-update-check.timer
  ACC_METERED_DEV=$(default_device)
  [[ -n $ACC_METERED_DEV ]] || die "FAIL  UT12: the guest has no default route"
  trap metered_cleanup EXIT
  guest_ssh sudo nmcli device modify "$ACC_METERED_DEV" connection.metered yes > /dev/null
  metered=$(nm_metered)
  [[ $metered == 'u 1' ]] || die "FAIL  UT12: NetworkManager reads Metered '$metered' after the mark, expected 'u 1'"
  before=$(state .last_successful_check)
  point_stable v2
  check_now
  expect "UT12: on a metered network the check finds the update" .update available
  # A download that failed also leaves the update available: only no error shows that none was tried.
  expect "UT12: with no error" .last_error none
  [[ $(state .last_successful_check) != "$before" ]] || die "FAIL  UT12: the check did not reach the registry"
  [[ $(guest_ssh sudo bootc status --format json | jq -c .status.staged) == null ]] || die "FAIL  UT12: a deployment was staged on a metered network"
  pass "UT12: and nothing is downloaded"
  metered_cleanup
  trap - EXIT
  metered=$(nm_metered)
  [[ $metered == 'u 4' ]] || die "FAIL  UT12: NetworkManager reads Metered '$metered' after the cleanup, expected 'u 4' (guessed no)"
  reboot_guest
}

stage_download() { # item 1
  point_stable v2
  expect_until "1: downloaded by the timer with no user action" .update downloaded 20
  power_cycle
  [[ $(marker) == v1 ]] || die "FAIL  1: a poweroff applied the update"
  pass "1: a poweroff boots the old version"
  expect_until "8: and it keeps updating" .update downloaded 20
}

# What the stage `sessions` leaves behind is taken away on every exit, a failed one included.
ACC_SSH_HELD=''
ACC_OTHER_MADE=''
sessions_cleanup() {
  local failed=() unit
  end_call || failed+=(end_call)
  tunnel || failed+=(tunnel)
  end_units || failed+=(end_units)
  if [[ -n $ACC_SSH_HELD ]]; then
    end_ssh_session || failed+=(end_ssh_session)
    ACC_SSH_HELD=''
  fi
  for unit in acc-second-count acc-inhibit-second "$ACC_INHIBITOR"; do
    stop_system_unit "$unit.service" || failed+=("stop $unit")
  done
  unmount_wrapper || failed+=(unmount_wrapper)
  polkit_log_restore || failed+=(polkit_log_restore)
  if [[ -n $ACC_OTHER_MADE ]]; then
    remove_other || failed+=(remove_other)
    ACC_OTHER_MADE=''
  fi
  ((${#failed[@]} == 0)) || { echo "FAIL  the cleanup of the sessions stage failed at: ${failed[*]}" >&2; return 1; }
}

stage_sessions() { # item 2, the other-session rule of UT6 (ADR-0103 D22, #433); nothing is applied
  local since log since_agent m
  # A run that starts here boots cold, and the boot's check ran before the tunnel was up.
  [[ $(state .update) == downloaded ]] || check_now
  expect_until "2: a downloaded update to ask about" .update downloaded 20
  polkit_pattern_selfcheck
  trap sessions_cleanup EXIT
  polkit_log_info
  untunnel
  # A run killed before its trap ran leaves the user behind: take it away first.
  if guest_ssh getent passwd "$ACC_OTHER" > /dev/null; then remove_other; fi
  guest_ssh "sudo useradd -m -p '*' $ACC_OTHER"
  ACC_OTHER_MADE=1
  # Every call below ends at this inhibitor if it is authorised, so none can reboot the guest
  # (the order of Apply() is the polkit check, then the inhibitor query, then the unlock). It
  # is not one of ACC_UNITS, which end_units empties, and each call checks that it is held.
  guest_ssh sudo systemd-run --quiet "--unit=$ACC_INHIBITOR" systemd-inhibit --what=shutdown --mode=block --who=acceptance --why=ut6 sleep 3600

  expect_no_prompt "2: a caller launched from a user unit, its user's session the only one: no administrator prompt"

  class_session greeter
  class_session lock-screen
  class_session background
  expect_no_prompt "2: only greeter, lock-screen and background sessions besides the caller's: no password"
  end_units

  # A second user left logged in: the state a fast user switch leaves, a session of another
  # uid. The greeter's way of making it is not driven here.
  class_session user
  expect_prompts "2: a second user's session: Apply() asks for administrator authentication" os.athanor.update.apply-multi-session 1
  end_units

  ssh_session
  ACC_SSH_HELD=1
  expect_prompts "2: a second session of the same user, over SSH: it asks for administrator authentication" os.athanor.update.apply-multi-session 1
  # The session is kept for the next case: with it open, nothing is kept between two calls.
  expect_prompts "2: with another session open, two consecutive calls each ask: nothing is kept" os.athanor.update.apply-multi-session 2
  end_ssh_session
  ACC_SSH_HELD=''

  # A counting session that opens after the unlock and before the reboot. The sessions inhibitor
  # would end the call before the unlock, so second-count.sh drops it once the wrapper is seen
  # from the update service's own mount namespace, takes its own when bootc is held at the
  # unlock, and the administrator is then refused by stopping the agent.
  guest_ssh "sudo tee /var/tmp/acc-second-count.sh > /dev/null" < "$ACC_HERE/second-count.sh"
  since=$(now_guest)
  ACC_UNITS+=(acc-class-user)
  guest_ssh sudo systemd-run --quiet --unit=acc-second-count --collect -E "ACC_USER=$GUEST_USER" -E "ACC_OTHER=$ACC_OTHER" -E "ACC_DELAY=$ACC_CALL_DELAY" -E "ACC_INHIBITOR=$ACC_INHIBITOR" -E ACC_LOG=/var/tmp/acc-second-count.log bash /var/tmp/acc-second-count.sh
  # No SSH login until the script has opened its session and bootc has gone on, so that the
  # session the second count sees is the one made on purpose (the script checks the rest).
  sleep $((ACC_CALL_DELAY + 60))
  for _ in $(seq 60); do
    prompted "$since" && break
    sleep 1
  done
  log=$(guest_ssh cat /var/tmp/acc-second-count.log)
  prompted "$since" || die "FAIL  2: no administrator prompt after the session opened between the unlock and the reboot (second-count.sh logged: $log)"
  grep -q 'bootc held' <<< "$log" || die "FAIL  2: bootc was not held at the unlock: $log"
  grep -q 'opened a session' <<< "$log" || die "FAIL  2: the session did not open while bootc was held: $log"
  deployment_locked || die "FAIL  2: the deployment was unlocked while the second prompt waited"
  pass "2: a session opened after the unlock: the deployment is locked again while the second prompt waits"
  # The refusal: the agent goes away, polkitd cancels the pending check and the service gets
  # an answer other than a grant. (Stopping the dbus-send would leave the check pending.)
  since_agent=$(now_guest)
  guest_ssh pkill -x cosmic-osd
  call_says Apply os.athanor.Update1.Error.NotAuthorized 30 || die "FAIL  2: the refused prompt did not end as NotAuthorized: $(guest_ssh cat "$(call_out Apply)")"
  deployment_locked || die "FAIL  2: the deployment was unlocked after the prompt was refused"
  [[ $(marker) == v1 ]] || die "FAIL  2: the guest rebooted after the prompt was refused"
  ! polkit_grants "$since" "action os.athanor.update.apply-multi-session for" > /dev/null || die "FAIL  2: polkitd granted the second prompt"
  echo "EVIDENCE  second count: second-count.sh logged: $(tr '\n' ';' <<< "$log")"
  echo "EVIDENCE  second count: polkit-agent-helper@ lines: $(journal_since "$since" 'polkit-agent-helper@*' | head -n 3 | tr '\n' ';')"
  pass "2: and it stays locked, with no reboot, when the prompt is refused"
  # SIGTERM is a clean exit, which the unit does not restart: bring the agent back.
  guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" restart cosmic-osd.service
  for _ in $(seq 30); do
    polkit_grants_agent "$since_agent" && break
    sleep 1
  done
  polkit_grants_agent "$since_agent" || die "FAIL  cosmic-osd did not register as the polkit agent again in 30 s"
  guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" is-active --quiet cosmic-osd.service || die "FAIL  cosmic-osd is not active"
  for m in /usr/bin/bootc /run/acc-bootc-real; do
    guest_ssh "findmnt -n $m; test \$? -eq 1" || die "FAIL  $m is still a mount point: the wrapper of second-count.sh was not removed"
  done

  sessions_cleanup || die "FAIL  the cleanup of the sessions stage"
  trap - EXIT
  [[ $(state .update) == downloaded ]] || die "FAIL  2: the update is no longer downloaded"
}

# doc_shell.md section 7, item 5: after the restart into the update, the notifier names the
# running version in one notification. Its log line carries the digest and the version it named;
# two more polls of the notifier (30 seconds each) must not announce it again.
running_notice() { # running_notice DESCRIPTION
  # The notifier colours its fields even in the journal; the colour codes are dropped first.
  local digest version journal lines=''
  digest=$(state .booted.digest)
  version=$(state .booted.version)
  for _ in $(seq 36); do
    journal=$(guest_ssh sudo journalctl -q -b -o cat _SYSTEMD_USER_UNIT=athanor-update-notify.service) || die "FAIL  $1: the notifier's journal could not be read"
    lines=$(awk '/running version announced/ { gsub(/\033\[[0-9;]*m/, ""); print }' <<< "$journal")
    [[ -n $lines ]] && break
    sleep 5
  done
  [[ -n $lines ]] || die "FAIL  $1: the notifier announced no running version in three minutes"
  sleep 65
  journal=$(guest_ssh sudo journalctl -q -b -o cat _SYSTEMD_USER_UNIT=athanor-update-notify.service) || die "FAIL  $1: the notifier's journal could not be read"
  lines=$(awk '/running version announced/ { gsub(/\033\[[0-9;]*m/, ""); print }' <<< "$journal")
  [[ $(wc -l <<< "$lines") == 1 ]] || die "FAIL  $1: announced more than once: $lines"
  # images.sh labels plain versions (43.<date>.0), which the notifier's display() leaves as they are.
  [[ $lines == *"digest=$digest version=$version" ]] || die "FAIL  $1: it named another version than $version ($digest): $lines"
  pass "$1"
}

stage_apply() { # items 2, 3 and 14; doc_shell.md section 7, item 5
  local check
  expect_error "2: from an SSH session it asks for administrator authentication" "$(call_ssh Apply)" os.athanor.Update1.Error.NotAuthorized
  for member in "org.freedesktop.DBus.Properties GetAll s os.athanor.Update1" "org.freedesktop.DBus.Peer Ping"; do
    # shellcheck disable=SC2086  # the member and its arguments are separate words
    expect_error "2: busctl cannot reach $member" "$(guest_ssh busctl --system call os.athanor.Update1 /os/athanor/Update1 $member 2>&1)" "ccess denied"
  done
  guest_ssh "sudo systemd-run --quiet --unit=acc-inhibit systemd-inhibit --what=shutdown --mode=block --who=acceptance --why=item-2 sleep 600"
  expect_error "2: with a reboot inhibitor held it refuses" "$(call_active Apply)" os.athanor.Update1.Error.Blocked
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.staged.downloadOnly) == true ]] || die "FAIL  2: the deployment was unlocked"
  pass "2: and nothing is unlocked"
  guest_ssh sudo systemctl stop acc-inhibit.service
  request_and_reboot call_active Apply v2 "2: Apply() from the active session reboots into the new version with no password"
  pass "14: both bootc calls succeeded under the units' own hardening"
  running_notice "doc_shell.md 7.5: after the restart one notification names the running version"
  expect "3: verified after the reboot" .verified.reason signature
  # The timer's check runs two minutes after the boot and holds the store meanwhile, so a
  # request answers Busy (#547): stop the timer and let a check it already started, or queued
  # before the stop, end.
  guest_ssh sudo systemctl stop athanor-update-check.timer
  for _ in $(seq 60); do
    check=$(guest_ssh 'systemctl show -p ActiveState --value athanor-update-check.service && systemctl list-jobs --no-legend athanor-update-check.service') ||
      die "FAIL  2: cannot read the check's state"
    [[ $check == activating* || $check == *athanor-update-check* ]] || break
    sleep 5
  done
  [[ $check != activating* && $check != *athanor-update-check* ]] || die "FAIL  2: the check the timer started still runs after five minutes"
  expect_error "2: with nothing downloaded it refuses" "$(call_active Apply)" os.athanor.Update1.Error.NothingDownloaded
  guest_ssh "sudo cp --remove-destination /dev/stdin /etc/containers/policy.json" <<< '{"default":[{"type":"insecureAcceptAnything"}]}'
  check_now
  expect "3: a permissive local policy reads not verified" .verified.reason policy-not-in-force
  guest_ssh sudo ln -sfn /usr/share/athanor/containers/policy.json /etc/containers/policy.json
  check_now
  expect "3: and the link restores it" .verified.reason signature
  for unit in athanor-update.service athanor-update-check.service; do
    guest_ssh systemd-analyze security --no-pager "--threshold=$([[ $unit == athanor-update.service ]] && echo 55 || echo 60)" "$unit" > /dev/null || die "FAIL  11: $unit is above its exposure threshold"
  done
  pass "11: both services are below the stated exposure"
}

stage_refuse() { # items 4 and 12
  local body
  for tag in v3 v3w v3b; do
    point_stable "$tag"
    check_now
    expect "4/12: $tag is refused" .update refused
    expect "4/12: with the error code policy" .last_error policy
    # The deployments' image references are schema fields; every other value must be free of
    # registry text and URLs.
    body=$(guest_ssh cat /run/athanor-update/state.json | jq -c 'del(.booted.image, .downloaded.image, .previous.image)')
    ! grep -Eiq "signature was required|cryptographic|http|$ACC_PORT/" <<< "$body" || die "FAIL  4: registry text in the state file: $body"
  done
  pass "4: no registry text appears in the state file"
}

stage_older() { # item 9
  point_stable old
  check_now
  expect "9: an older signed digest is published as older-than-booted" .update older-than-booted
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.staged) == null ]] || die "FAIL  9: something was downloaded"
  pass "9: and nothing is downloaded"
  booted=$(state .booted.digest)
  # images.sh copied v2 to the nvidia repository and sign-images.sh signed it there: the
  # same digest, under a signature made for another repository.
  other=$(skopeo inspect --tls-verify=false --format '{{.Digest}}' "docker://$ACC_REGISTRY/athanor-system-nvidia:v2")
  [[ $booted == "$other" ]] || die "the nvidia copy of v2 must share the digest of the booted image"
  guest_ssh "sudo rm -rf /var/lib/athanor-update/signatures/${booted#sha256:} && sudo skopeo copy --policy /usr/share/athanor/containers/attachments-policy.json docker://$ACC_REGISTRY/athanor-system-nvidia:sha256-${other#sha256:}.sig dir:/var/lib/athanor-update/signatures/${booted#sha256:}"
  # The unit stays active (RemainAfterExit=yes): only a restart publishes again.
  guest_ssh sudo systemctl restart athanor-update-state.service
  expect "9: a valid signature object copied from another of our repositories reads no-signature" .verified.reason no-signature
  guest_ssh "sudo rm -rf /var/lib/athanor-update/signatures/${booted#sha256:}"
  point_stable v2
  check_now
  expect "9: the next check fetches the right object again" .verified.reason signature
  # The same image under a repository the policy does not scope.
  skopeo copy --src-tls-verify=false --dest-tls-verify=false "docker://$REPO:v2" "docker://${ACC_REGISTRY%/*}/elsewhere/athanor-system:v2"
  guest_ssh sudo bootc switch --enforce-container-sigpolicy --transport registry "${ACC_REGISTRY%/*}/elsewhere/athanor-system:v2"
  reboot_guest
  expect "9: a deployment outside the policy reads reference-out-of-scope" .verified.reason reference-out-of-scope
  # Back at v1, not v2: this round trip leaves v2 in both deployments, and goback needs a
  # previous deployment that holds another digest.
  point_stable v1
  guest_ssh sudo bootc switch --enforce-container-sigpolicy --transport registry "$REPO:stable"
  reboot_guest
  expect "9: and back inside it reads verified" .verified.reason signature
}

stage_goback() { # items 5 and 13
  point_stable v2
  check_now
  expect "5: v2 is downloaded over v1" .update downloaded
  request_and_reboot call_root Apply v2 "5: and applied, so the previous deployment holds v1"
  expect_error "5: GoBack() asks for administrator authentication from an SSH session" "$(call_ssh GoBack)" os.athanor.Update1.Error.NotAuthorized
  guest_ssh pkaction --verbose --action-id os.athanor.update.rollback | grep -c auth_admin$ | grep -qx 3 || die "FAIL  5: the rollback action is not auth_admin for every kind of session"
  pass "5: and the action is auth_admin for every kind of session (the prompt in the session is looked at by hand: screenshot.sh)"
  left=$(state .booted.digest)
  request_and_reboot call_root GoBack v1 "5: GoBack() boots the previous digest"
  [[ $(guest_ssh cat /var/lib/athanor-update/held) == "$left" ]] || die "FAIL  5: the digest left is not held"
  for n in 1 2 3; do
    check_now
    expect "5/13: check $n does not download the held digest" .update held
    [[ $(guest_ssh sudo bootc status --format json | jq -r .status.staged) == null ]] || die "FAIL  13: the held digest was staged"
  done
  point_stable v4
  check_now
  expect "5: a newer digest is offered" .update downloaded
}

stage_rotate() { # item 6
  request_and_reboot call_root Apply v4 "6: an image that ships a second key and is signed with the first is accepted"
  point_stable v5
  check_now
  expect "6: the next image, signed with the second key, is accepted" .update downloaded
  request_and_reboot call_root Apply v5 "6: and it boots"
  expect "6: and reads verified" .verified.reason signature
}

stage_recover() { # item 10
  guest_ssh "sudo cp /dev/stdin /root/acc-3.pub" < "$ACC_STATE/keys/acc-3.pub"
  guest_ssh sudo athanor-update recover-key begin /root/acc-3.pub
  point_stable v6
  check_now
  expect "10: after the recovery an image signed with the old key is refused" .update refused
  point_stable v7
  check_now
  expect "10: and one signed with the new key is downloaded" .update downloaded
  guest_ssh sudo athanor-update recover-key finish && die "FAIL  10: finish must wait for an image that ships the new key"
  request_and_reboot call_root Apply v7 "10: the image signed with the new key boots"
  guest_ssh sudo athanor-update recover-key finish
  check_now
  expect "10: the machine is on the new key with the old one removed" .verified.reason signature
  [[ $(guest_ssh readlink /etc/containers/policy.json) == /usr/share/athanor/containers/policy.json ]] || die "FAIL  10: the policy link was not restored"
}

# What agetty prints above the login of tty1: /etc/issue and /etc/issue.d (doc_recovery.md, R3).
# agetty reads the attributes of its terminal even to show the issue, so SSH allocates one
# (-tt), whose line ends carry a carriage return that the needles do not.
issue_text() { guest_ssh -q -tt sudo agetty --show-issue < /dev/null | tr -d '\r'; }
expect_text() { # expect_text DESCRIPTION TEXT NEEDLE...: every NEEDLE is a fixed string of TEXT
  local needle
  for needle in "${@:3}"; do
    grep -qF -- "$needle" <<< "$2" || die "FAIL  $1: '$needle' is not in: $2"
  done
}

# doc_recovery.md section 6, acceptance 1 (and 5, which the reboot at its end gives): the
# greeter fails three times in a minute. The failing greeter is a drop-in in /run (ExecStart
# is /usr/bin/false, restarted every 2 s); StartLimitBurst and OnFailure are the image's own.
# Looked at by hand: the framebuffer of tty1 (the VM's serial console is not tty1).
# The drop-in, the failed greetd and /run/athanor-recovery all live in /run: a failure
# midway reboots, so that the next run starts from a normal boot.
console_cleanup() { reboot_guest; }
stage_console() { # recovery items 1 and 5
  local dropin=/run/systemd/system/greetd.service.d/50-acceptance-fail.conf text health
  guest_ssh test ! -e /run/athanor-recovery || die "FAIL  recovery 5: /run/athanor-recovery exists on a normal boot"
  text=$(issue_text) || die "FAIL  agetty --show-issue failed in the guest"
  ! grep -qF "The desktop did not start." <<< "$text" || die "FAIL  recovery 5: the plain banner carries the recovery message"
  # greenboot's required check waits for greetd: breaking greetd while it runs fails the boot's
  # health check and queues a rollback. Wait until greenboot has passed this boot.
  for _ in $(seq 60); do
    health=$(guest_ssh systemctl show -p ActiveState --value greenboot-healthcheck.service) || die "FAIL  recovery 1: cannot read greenboot's state"
    [[ $health == activating || $health == inactive ]] || break
    sleep 5
  done
  [[ $health == active ]] || die "FAIL  recovery 1: greenboot did not pass this boot before the stage (state $health)"
  trap console_cleanup EXIT
  printf '[Service]\nExecStart=\nExecStart=/usr/bin/false\nRestart=on-failure\nRestartSec=2s\n' |
    guest_ssh "sudo mkdir -p ${dropin%/*} && sudo tee $dropin > /dev/null"
  guest_ssh "sudo systemctl daemon-reload && sudo systemctl --no-block restart greetd.service"
  # The target wants the getty without ordering against it: wait for both.
  for _ in $(seq 45); do
    guest_ssh 'systemctl is-active --quiet athanor-recovery.target && systemctl is-active --quiet getty@tty1.service' && break
    sleep 2
  done
  guest_ssh systemctl is-active --quiet athanor-recovery.target || die "FAIL  recovery 1: athanor-recovery.target is not active 90 s after greetd failed"
  pass "recovery 1: three failures of greetd in a minute start athanor-recovery.target"
  guest_ssh systemctl is-active --quiet getty@tty1.service || die "FAIL  recovery 1: no getty on tty1"
  text=$(issue_text) || die "FAIL  agetty --show-issue failed in the guest"
  expect_text "recovery 1: tty1 has a login and, above it, the message in both languages" "$text" \
    "The desktop did not start." "Il desktop non si è avviato." "sudo athanor-update go-back" "journalctl -b -u greetd"
  ! grep -qF "failed its health check" <<< "$text" || die "FAIL  recovery 1: a boot that passed its health check says it failed it"
  # The drop-in is in /run, so the reboot takes it back; the plain banner must come back too.
  trap - EXIT
  reboot_guest
  guest_ssh test ! -e /run/athanor-recovery || die "FAIL  recovery 5: /run/athanor-recovery exists after the reboot"
  text=$(issue_text) || die "FAIL  agetty --show-issue failed in the guest"
  ! grep -qF "The desktop did not start." <<< "$text" || die "FAIL  recovery 5: the recovery message survived the reboot"
  guest_ssh systemctl is-active --quiet greetd.service || die "FAIL  recovery 5: greetd is not active after the reboot"
  pass "recovery 5: a normal boot shows the plain banner and /run/athanor-recovery does not exist"
}

# doc_recovery.md section 6, acceptance 7 (R5, A2-26): a deployment whose greetd.service runs
# /usr/bin/false (the image `bad` of images.sh) is applied and booted. It is signed with the
# key of the stage `recover`, so this stage follows it. It runs once per guest: the
# digest of `bad` stays held afterwards, as UT6 wants (README); the digest `goback` holds
# is another one, and the service replaces it.
stage_greenboot() { # recovery item 7
  local prev left boot boots text bad held
  bad=$(skopeo inspect --tls-verify=false --format '{{.Digest}}' "docker://$REPO:bad") || die "cannot read the digest of $REPO:bad"
  held=$(guest_ssh 'test ! -e /var/lib/athanor-update/held || cat /var/lib/athanor-update/held') ||
    die "cannot read the guest's held digest"
  [[ $held != "$bad" ]] ||
    die "stage greenboot runs once per guest and the digest of bad is already held: reset.sh, then run.sh from install"
  prev=$(marker)
  [[ $prev != bad ]] || die "the guest is booted on bad from an earlier run: reset.sh, then run.sh from install"
  point_stable bad
  check_now
  expect "recovery 7: the image whose greeter does not start is downloaded" .update downloaded
  request_and_reboot call_root Apply bad "recovery 7: and booted"
  left=$(state .booted.digest) || die "FAIL  recovery 7: the update service's state is unreadable after the boot"
  # greenboot's check waits up to two minutes for greetd; the mark is the queued rollback.
  for _ in $(seq 60); do
    [[ $(guest_ssh sudo bootc status --format json | jq -r .status.rollbackQueued) == true ]] && break
    sleep 5
  done
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.rollbackQueued) == true ]] || die "FAIL  recovery 7: the deployment was not marked (rollbackQueued) 5 minutes after the boot"
  text=$(guest_ssh sudo journalctl -b -u greenboot-healthcheck.service -o cat --no-pager)
  expect_text "recovery 7: greenboot's required check 10-greetd-running.sh failed and queued the rollback" "$text" \
    "required script /etc/greenboot/check/required.d/10-greetd-running.sh failed!" \
    "Greenboot check: greetd has failed." "Rollback successful"
  guest_ssh systemctl is-active --quiet athanor-recovery.target || die "FAIL  recovery 7: the recovery console did not start"
  guest_ssh systemctl is-active --quiet getty@tty1.service || die "FAIL  recovery 7: no getty on tty1"
  text=$(issue_text) || die "FAIL  agetty --show-issue failed in the guest"
  expect_text "recovery 7: tty1 says, in both languages, that this version failed its health check" "$text" \
    "The desktop did not start." "Il desktop non si è avviato." "failed its health check" "non ha superato il controllo di salute" "sudo systemctl reboot"
  ! grep -qF "sudo athanor-update go-back" <<< "$text" || die "FAIL  recovery 7: the console of a marked deployment sends the person to go-back, which would undo the queued return"
  # The service learns of the mark from a check and holds the booted digest (UT6); the timer
  # would run one two minutes after the boot.
  check_now
  [[ $(guest_ssh cat /var/lib/athanor-update/held) == "$left" ]] || die "FAIL  recovery 7: the digest of the marked deployment is not held"
  pass "recovery 7: the marked digest is held"
  # Nothing reboots by itself: ten minutes later it is the same boot, on the same deployment.
  boot=$(boot_id)
  boots=$(guest_ssh journalctl --list-boots --no-pager | wc -l) || die "FAIL  recovery 7: journalctl --list-boots failed"
  sleep 600
  [[ $(boot_id) == "$boot" ]] || die "FAIL  recovery 7: the guest rebooted by itself within ten minutes"
  [[ $(guest_ssh journalctl --list-boots --no-pager | wc -l) == "$boots" ]] || die "FAIL  recovery 7: journalctl --list-boots shows a new boot"
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.booted.image.imageDigest) == "$left" ]] || die "FAIL  recovery 7: bootc status no longer shows the marked deployment booted"
  pass "recovery 7: ten minutes later the machine has not rebooted and the marked deployment is still booted"
  guest_ssh sudo systemd-run --quiet --on-active=2 systemctl reboot
  wait_reboot "$boot" || die "FAIL  recovery 7: the guest did not come back from the restart in 10 minutes"
  [[ $(marker) == "$prev" ]] || die "FAIL  recovery 7: after the restart $(marker) is booted, expected the previous deployment $prev"
  pass "recovery 7: after the restart the previous deployment is booted"
  for n in 1 2 3; do
    check_now
    expect "recovery 7: check $n does not download the marked digest" .update held
    [[ $(guest_ssh sudo bootc status --format json | jq -r .status.staged) == null ]] || die "FAIL  recovery 7: the marked digest was staged"
  done
  point_stable "$prev"
}


stage_podman() { # item 15
  guest_ssh 'podman pull docker.io/library/busybox:latest && podman save -o /var/tmp/acc-busybox.tar busybox && podman rmi busybox && podman load -i /var/tmp/acc-busybox.tar && printf "FROM busybox\nRUN true\n" | podman build -t acc-build - && rm /var/tmp/acc-busybox.tar'
  pass "15: podman pull from another registry, podman load and podman build work with the policy in force"
}

stage_probes() { # the runtime audit probes (issue #312), against expectations.toml
  local facts=$ACC_STATE/probes-facts.json
  guest_ssh python3 - collect < "$ACC_HERE/probes.py" > "$facts"
  # The verdict names each check; a failure stops the run, and the facts stay in $ACC_STATE.
  python3 -B "$ACC_HERE/probes.py" check "$facts" "$ACC_HERE/expectations.toml" "$ROOT/forge/config/contacts.toml" "$ACC_PORT" || die "FAIL  runtime probes (facts: $facts)"
}

stage_report() {
  guest_ssh "systemctl --user --machine=$GUEST_USER@.host is-active athanor-update-notify.service" || die "FAIL  the notifier is not running under its Landlock ruleset"
  pass "UT11: the notifier runs confined; its notifications are looked at by hand (scripts/devvm/screenshot.sh)"
  echo "acceptance complete"
}

# --- The upgrade acceptance of doc_update_delivery.md UD18, in CI (run.sh --ci) ---
# The file seeded in /etc is one the image ships with fixed content (ADR-0130): the candidate
# shipping the same bytes is what makes the three-way merge of /etc the thing tested.
ETC_PROBE=/etc/security/pwquality.conf.d/50-athanor.conf
VAR_PROBE=/var/lib/athanor-upgrade-probe/seed
MIRROR_DROPIN=/etc/containers/registries.conf.d/99-upgrade-acceptance.conf
PHASES=${ACC_CI_OUT:-}/upgrade-phases.txt

# Records the runner's free space and stops under four gigabytes, before a stage fills it.
disk_floor() {
  local free
  { echo "== $(date -u +%FT%TZ) before ${FUNCNAME[1]}"; df -h; } >> "$ACC_CI_OUT/df.txt"
  free=$(df --output=avail -B1G "$ACC_CI_OUT" | tail -n 1)
  ((free >= 4)) || die "FAIL  ${free// /} GB free on the runner, under the floor of 4 GB"
}

# The probes of forge/test/iso/console.py, run over SSH: the answers become phases under
# PREFIX in upgrade-phases.txt, which verdict.py judges as it judges the first boot's. The
# greeter's password is typed on the guest's keyboard, as the console types it.
probe() { # probe NAME PREFIX
  python3 "$ROOT/forge/test/iso/console.py" --print-probe "$1" | guest_ssh bash -s |
    python3 "$ROOT/forge/test/iso/console.py" --classify "$2" >> "$PHASES"
}
session_probes() { # session_probes PREFIX
  probe MOK "$1"
  probe GREETER "$1"
  type_keys "$GUEST_USER" @enter
  probe SESSION "$1"
  probe SETTINGS "$1"
}

# The seeds of ci_stage_seed, all still there.
seeds_kept() { # seeds_kept DESCRIPTION
  local nonce
  nonce=$(< "$ACC_STATE/nonce")
  [[ $(guest_ssh cat upgrade-probe) == "$nonce" ]] || die "FAIL  $1: the seed in /home is gone"
  [[ $(guest_ssh sudo cat "$VAR_PROBE") == "$nonce" ]] || die "FAIL  $1: the seed in /var is gone"
  [[ $(guest_ssh sudo tail -n 1 "$ETC_PROBE") == "# upgrade-acceptance $nonce" ]] || die "FAIL  $1: the edit of $ETC_PROBE is gone"
  pass "$1: the seeds in /home, /var and $ETC_PROBE are kept"
}

ci_stage_seed() { # UD18 2
  local from nonce
  disk_floor
  from=$(booted_digest)
  [[ $from =~ ^sha256:[0-9a-f]{64}$ ]] || die "FAIL  UD18 2: the booted digest reads '$from'"
  [[ $from != "$ATHANOR_UPDATE_TO" ]] || die "FAIL  UD18 2: the candidate is :stable already: there is nothing to upgrade from"
  echo "$from" > "$ACC_STATE/from-digest"
  guest_ssh sha256sum "/usr$ETC_PROBE" | cut -d' ' -f1 > "$ACC_STATE/shipped-sum"
  nonce=$(od -An -tx1 -N16 /dev/urandom | tr -d ' \n')
  echo "$nonce" > "$ACC_STATE/nonce"
  guest_ssh "echo $nonce > upgrade-probe && sudo mkdir -p ${VAR_PROBE%/*} && echo $nonce | sudo tee $VAR_PROBE > /dev/null && echo '# upgrade-acceptance $nonce' | sudo tee -a $ETC_PROBE > /dev/null"
  pass "UD18 2: $from seeded in /home, /var and $ETC_PROBE"
}

ci_stage_upgrade() { # UD18 3 and 4
  local since journal class settled=downloaded ttys
  disk_floor
  printf '[[registry]]\nprefix = "%s"\nlocation = "%s"\n\n[[registry.mirror]]\nlocation = "%s"\ninsecure = true\n' \
    "$ATHANOR_UPDATE_REPOSITORY" "$ATHANOR_UPDATE_REPOSITORY" "$ATHANOR_UPDATE_MIRROR" |
    guest_ssh sudo tee "$MIRROR_DROPIN" > /dev/null
  quiet_checks "UD18 4"
  since=$(now_guest)
  check_now
  # Required since ADR-0130: without the attestation the client only warns and goes on.
  journal=$(journal_since "$since" athanor-update-check.service)
  [[ $journal != *"release attestation could not be fetched"* ]] || die "FAIL  UD18 3: the candidate's attestation was not read"
  guest_ssh sudo test -d "/var/lib/athanor-update/attestations/${ATHANOR_UPDATE_TO#sha256:}" ||
    die "FAIL  UD18 3: the candidate's attestation was not stored"
  # A stored attestation is not a verified one: the client reads an attestation no key of its
  # policy verifies as the feature class. Its class is held to the one serve_candidate.sh
  # verified on the runner, read from the lines of check.rs, as the state does not keep it.
  if [[ $journal == *"the candidate is of the feature class"* ]]; then
    class=feature
  elif [[ $journal == *"security update unlocked for the next shutdown"* ]]; then
    class=security
  else
    die "FAIL  UD18 3: the check's journal names no class for the candidate"
  fi
  [[ $class == "$ATHANOR_UPDATE_CLASS" ]] ||
    die "FAIL  UD18 3: the guest settled the candidate as the $class class; its attestation, verified on the runner, says $ATHANOR_UPDATE_CLASS"
  pass "UD18 3: the guest reads the candidate as the $class class, as its attestation verified on the runner says"
  # UT13: with automatic updates on, the default, the check unlocks a security candidate for
  # the next shutdown itself (check.rs settle_security); a feature candidate waits for Apply().
  [[ $class == feature ]] || settled=will-apply-at-next-shutdown
  [[ $(state .update) == "$settled" && $(state .downloaded.digest) == "$ATHANOR_UPDATE_TO" ]] ||
    die "FAIL  UD18 4: the check did not settle the $class candidate as $settled: update '$(state .update)', digest '$(state .downloaded.digest)', last_error '$(state .last_error)' from '$(state .last_error_host)'"
  pass "UD18 4: the client's own check downloads the $class candidate from the mirror and settles it as $settled"
  if [[ $class == security ]]; then
    # Apply() unlocks a download-only deployment, and refuses this one, unlocked already, as
    # NothingDownloaded (requests.rs): the shutdown applies it.
    reboot_into_digest guest_ssh "sudo systemd-run --quiet --on-active=2 systemctl reboot" "$ATHANOR_UPDATE_TO" \
      "UD18 4: the security candidate boots at the next shutdown"
    return
  fi
  # A login on the serial console is a second session of the user: Apply() would ask.
  ttys=$(guest_ssh loginctl list-sessions --json=short | jq -r '[.[].tty // empty] | join(" ")')
  [[ " $ttys " != *" ttyS0 "* ]] || die "FAIL  UD18 4: a session on ttyS0 is open, and Apply() would ask for an administrator"
  reboot_into_digest call_active Apply "$ATHANOR_UPDATE_TO" "UD18 4: Apply() from the active session boots the candidate"
}

ci_stage_check() { # UD18 5
  local status image signature sum
  disk_floor
  expect "UD18 5: the candidate is verified" .verified.reason signature
  status=$(guest_ssh sudo bootc status --format json)
  image=$(jq -r .status.booted.image.image.image <<< "$status")
  signature=$(jq -r .status.booted.image.image.signature <<< "$status")
  [[ $image == *:stable && $signature == containerPolicy ]] || die "FAIL  UD18 5: the machine follows $image ($signature), not :stable under the policy"
  pass "UD18 5: the machine still follows :stable under the signature policy"
  sum=$(guest_ssh sha256sum "/usr$ETC_PROBE" | cut -d' ' -f1)
  [[ $sum == "$(< "$ACC_STATE/shipped-sum")" ]] || die "FAIL  UD18 5: the candidate changed the shipped probe file: the merge is not tested"
  seeds_kept "UD18 5"
  session_probes upgraded-
}

ci_stage_goback() { # UD18 6
  disk_floor
  reboot_into_digest call_root GoBack "$(< "$ACC_STATE/from-digest")" "UD18 6: GoBack() boots the from digest"
}

ci_stage_check_back() { # UD18 6
  local stable
  disk_floor
  guest_ssh test -e "$MIRROR_DROPIN" || die "FAIL  UD18 6: the mirror drop-in is gone after GoBack"
  # The mirror still offers the candidate as :stable, so a check meets it again.
  stable=$(skopeo inspect --tls-verify=false --format '{{.Digest}}' "docker://127.0.0.1:${ATHANOR_UPDATE_MIRROR#*:}:stable")
  [[ $stable == "$ATHANOR_UPDATE_TO" ]] || die "FAIL  UD18 6: the mirror's :stable is $stable, not the candidate"
  quiet_checks "UD18 6"
  check_now
  expect "UD18 6: the client holds the candidate it went back from" .update held
  [[ $(guest_ssh sudo cat /var/lib/athanor-update/held) == "$ATHANOR_UPDATE_TO" ]] || die "FAIL  UD18 6: the held digest is not the candidate"
  seeds_kept "UD18 6"
  session_probes rolled-back-
}

if [[ -n $ACC_CI ]]; then
  wait_ssh
  prefix=ci_stage_
else
  # stage_install waits for the guest itself; a later start needs its SSH and the registry tunnel.
  [[ $start == install ]] || wait_ssh
  prefix=stage_
fi
run=''
for stage in "${STAGES[@]}"; do
  [[ $stage == "$start" ]] && run=1
  [[ -n $run ]] || continue
  echo "== $stage"
  "$prefix${stage//-/_}"
done
