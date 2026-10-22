#!/usr/bin/env bash
# The acceptance of docs/architecture/doc_update_trust.md, section 6, items 1 to 19, on the
# development VM, against the registry and keys of images.sh. Repeatable: `reset.sh` first.
# Every bootc call is made by the units themselves (item 14): this script only starts units
# and calls the bus. Usage: run.sh [START_AT]   (a stage name below; default: the first)
set -euo pipefail
# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

guard_no_ci
STAGES=(install migrate metered download sessions apply security postpone optout class securitygreenboot refuse older goback rotate recover console greenboot podman probes report)
start=${1:-install}
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
  local failed=() unit state
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
  # request answers Busy (#547).
  quiesce_checks
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


# --- UT13: security-class updates (doc_update_trust.md section 6, items 16 to 19) ---
# They run after `apply`, on v2, over the images vsec1 to vsec5, v8, vsec-bad, vsec-noclass and vsec-gb
# of images.sh (all built on 2026-09-15, so the stages after them still find their images newer than
# the newest build the guest booted), and end on v2 again, where the stages after them expect it.

# The default device marked metered, as the stage metered does; meter_off takes the mark back.
meter_on() {
  ACC_METERED_DEV=$(default_device)
  [[ -n $ACC_METERED_DEV ]] || die "FAIL  UT12: the guest has no default route"
  trap metered_cleanup EXIT
  guest_ssh sudo nmcli device modify "$ACC_METERED_DEV" connection.metered yes > /dev/null
  [[ $(nm_metered) == 'u 1' ]] || die "FAIL  UT12: NetworkManager reads Metered '$(nm_metered)' after the mark, expected 'u 1'"
}
meter_off() {
  metered_cleanup
  trap - EXIT
  [[ $(nm_metered) == 'u 4' ]] || die "FAIL  UT12: NetworkManager reads Metered '$(nm_metered)' after the cleanup, expected 'u 4' (guessed no)"
}

# The guest's clock seven days ahead, for the postpone's expiry; restored (with NTP as it was) after the
# boot that follows, and by the EXIT trap when a stage fails in between.
ACC_CLOCK_NTP=''
ACC_CLOCK_MOVED=''
clock_ahead() {
  ACC_CLOCK_NTP=$(guest_ssh timedatectl show -p NTP --value)
  ACC_CLOCK_MOVED=1
  guest_ssh sudo timedatectl set-ntp false
  # shellcheck disable=SC2016 # the guest's shell expands the date
  guest_ssh 'sudo date -s @$(( $(date +%s) + 7 * 86400 ))' > /dev/null
}
clock_restore() {
  [[ -n $ACC_CLOCK_MOVED ]] || return 0
  guest_ssh "sudo date -s @$(date +%s)" > /dev/null || return 1
  if [[ $ACC_CLOCK_NTP == yes ]]; then guest_ssh sudo timedatectl set-ntp true || return 1; fi
  ACC_CLOCK_MOVED=''
}

# The trust shield's sheet in the guest's session, driven as shield-acceptance.sh does: the bar is
# restarted so that it reads the state afresh, the shield pressed, and a row asked for by its exact
# name. The rows depend on the sheet of BR6 items 6 and 7 (plan Task 10).
SHIELD_HEADERS='System image verified|Not verified yet|Update refused'
shield_sheet() {
  guest_ssh sudo systemctl --user --machine="$GUEST_USER@.host" restart athanor-bar.service
  session_probe press "$SHIELD_HEADERS" > /dev/null || die "FAIL  no showing shield named any of '$SHIELD_HEADERS'"
}
session_probe() { # session_probe showing|hidden|press NAME
  # shellcheck disable=SC2016 # expanded by the guest's shell
  guest_ssh "export XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=wayland-1 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus; python3 - $1 athanor-bar $(printf '%q' "$2")" < "$ACC_HERE/../shield_probe.py"
}
sheet_shows() { session_probe showing "$2" || die "FAIL  $1: the sheet does not show '$2'"; }
sheet_hides() { session_probe hidden "$2" || die "FAIL  $1: the sheet shows '$2'"; }

# A request the check or the user manager must have refused or accepted: dbus-send prints no
# "Error" for a reply.
expect_reply() { # expect_reply DESCRIPTION OUTPUT
  [[ $2 != *Error* ]] || die "FAIL  $1: $2"
  pass "$1"
}

# Waits for the next boot of the digest DIGEST to be counted as applied at an online check: the
# lineage's anchor (lineage.json) moves to it and no lineage is open.
expect_applied() { # expect_applied DESCRIPTION DIGEST
  for _ in $(seq 12); do
    check_now
    [[ $(lineage_anchor) == "$2" ]] && break
    sleep 10
  done
  [[ $(lineage_anchor) == "$2" ]] || die "FAIL  $1: the lineage's anchor is '$(lineage_anchor)', expected $2"
  expect "$1: and no lineage is open" .postpone null
}

stage_security() { # items 16 and 17; doc_shell.md section 7, item 4; UD37
  local boot digest1 digest2 notice m
  [[ $(marker) == v2 ]] || die "stage security starts on v2, where apply leaves the guest: it boots $(marker)"
  digest1=$(image_digest vsec1)
  digest2=$(image_digest vsec2)
  quiesce_checks
  notify_watch
  meter_on
  point_stable vsec1
  check_now
  expect "16/UT13: on a metered network a security digest is downloaded and unlocked for the next shutdown" .update will-apply-at-next-shutdown
  expect "16: with no error" .last_error none
  deployment_unlocked || die "FAIL  16: the staged deployment is still locked"
  expect "17: the lineage is open and not postponed" .postpone.postponed false
  expect "17: with seven boots to go" .postpone.boots_remaining 7
  expect "17: the opt-out is off" .automatic_security_updates true
  meter_off
  # Plan Task 9: the notice has a "Postpone" action and no "Later".
  notice=$(wait_notice "17: the security notice" postpone)
  jq -e '.ids | index("later") | not' <<< "$notice" > /dev/null || die "FAIL  17: the security notice offers Later: $notice"
  jq -e '.summary == "A security update is ready"' <<< "$notice" > /dev/null || die "FAIL  17: the security notice is titled otherwise: $notice"
  pass "17: the notice offers Postpone and no Later"
  # Plan Task 10: the sheet offers Postpone beside Restart to update.
  shield_sheet
  sheet_shows "17: a security digest is waiting" Postpone
  sheet_shows "17: a security digest is waiting" "Restart to update"
  boot=$(boot_id)
  sleep 600
  [[ $(boot_id) == "$boot" && $(marker) == v2 ]] || die "FAIL  16: the guest rebooted by itself within ten minutes"
  expect "16: and the digest still waits for the next shutdown" .update will-apply-at-next-shutdown
  pass "16: nothing reboots by itself"
  reboot_guest
  [[ $(marker) == vsec1 ]] || die "FAIL  16: the restart the user started booted $(marker), expected vsec1"
  pass "16: a restart the user starts boots the security update"
  quiesce_checks
  expect_applied "17: once a security digest is applied the budget is new" "$digest1"
  request_and_reboot call_root GoBack v2 "16: one GoBack() returns to the previous version"
  quiesce_checks
  check_now
  expect "16: the digest left is held" .update held
  [[ $(lineage_anchor) == "$digest1" ]] || die "FAIL  17: GoBack() moved the lineage's anchor"
  pass "17: and GoBack() does not reset the budget"
  # The soft reboot of UD37: the guest keeps its boot id, and the new deployment is what runs after.
  point_stable vsec2
  check_now
  expect "16: a newer security digest is unlocked" .update will-apply-at-next-shutdown
  boot=$(boot_id)
  guest_ssh sudo systemd-run --quiet --on-active=2 systemctl soft-reboot
  for _ in $(seq 120); do
    m=''
    if m=$(guest_ssh -q cat /usr/share/athanor-acceptance-marker) && [[ $m == vsec2 ]]; then break; fi
    sleep 5
  done
  [[ $m == vsec2 ]] || die "FAIL  16: a soft reboot did not boot vsec2 in 10 minutes (marker '$m')"
  tunnel
  echo "EVIDENCE  soft reboot: boot id before $boot, after $(boot_id)"
  pass "16: a soft reboot the user starts applies it as well (UD37)"
  quiesce_checks
  expect "16: and the booted digest is the one attested" .booted.digest "$digest2"
  notify_unwatch
}

stage_postpone() { # item 17
  local first expiry out digest4
  [[ $(marker) == vsec2 ]] || die "stage postpone follows security: the guest boots $(marker), expected vsec2"
  digest4=$(image_digest vsec4)
  quiesce_checks
  point_stable vsec3
  check_now
  expect "17: a security digest is unlocked" .update will-apply-at-next-shutdown
  first=$(state .postpone.first_notice_at)
  out=$(call_active Postpone)
  expect_reply "17: Postpone() as the active user asks for no password" "$out"
  # The reply may carry the expiry; when it does, it is first_notice_at plus seven days.
  expiry=$(grep -oE '(u?int64|u?x) [0-9]+' <<< "$out" | awk '{ print $2 }')
  [[ -z $expiry || $expiry == $((first + 604800)) ]] || die "FAIL  17: Postpone() answered $expiry, expected first_notice_at + 7 days = $((first + 604800))"
  expect "17: the digest is downloaded again" .update downloaded
  expect "17: the lineage is postponed" .postpone.postponed true
  expect "17: and first_notice_at did not move" .postpone.first_notice_at "$first"
  deployment_locked || die "FAIL  17: the deployment was not locked again"
  # Plan Task 10: the sheet carries the expiry in place of the button.
  expiry=$(guest_ssh "LC_ALL=en_US.UTF-8 date -d @$((first + 604800)) +'%-d %B, %H:%M'")
  shield_sheet
  sheet_shows "17: postponed" "The security update is postponed, at most until $expiry; then it installs at the next shutdown or restart."
  sheet_hides "17: postponed" Postpone
  power_cycle
  [[ $(marker) == vsec2 ]] || die "FAIL  17: a shutdown applied a postponed update"
  pass "17: a shutdown keeps the old version"
  quiesce_checks
  [[ $(state .postpone.boots_remaining) -lt 7 ]] || die "FAIL  17: the boot was not counted: $(state .postpone.boots_remaining) remain"
  expect_error "17: a second Postpone() is refused" "$(call_active Postpone)" os.athanor.Update1.Error.AlreadyPostponed
  expect "17: and first_notice_at did not move" .postpone.first_notice_at "$first"
  point_stable vsec4
  check_now
  expect "17: a newer digest of the lineage stays locked" .update downloaded
  deployment_locked || die "FAIL  17: the newer digest was unlocked during the postpone"
  expect_error "17: and Postpone() after it is published is refused" "$(call_active Postpone)" os.athanor.Update1.Error.AlreadyPostponed
  expect "17: with first_notice_at unmoved" .postpone.first_notice_at "$first"
  # The seven days pass: the shutdown that would apply unlocks the deployment (Task 7).
  trap 'clock_restore' EXIT
  clock_ahead
  power_cycle
  clock_restore
  trap - EXIT
  [[ $(marker) == vsec4 ]] || die "FAIL  17: the shutdown after seven days booted $(marker), expected vsec4"
  pass "17: after seven days the shutdown boots the newer digest"
  quiesce_checks
  expect_applied "17: and a new lineage starts" "$digest4"
  echo "EVIDENCE  postpone: athanor-update-state.service at shutdown: $(guest_ssh sudo journalctl -q -b -1 -u athanor-update-state.service -o cat --no-pager | tail -n 3 | tr '\n' ';')"
}

stage_optout() { # item 17
  local notice first
  [[ $(marker) == vsec4 ]] || die "stage optout follows postpone: the guest boots $(marker), expected vsec4"
  quiesce_checks
  expect_error "17: SetAutomaticSecurityUpdates(false) from an SSH session asks for an administrator" "$(call_ssh SetAutomaticSecurityUpdates boolean:false)" os.athanor.Update1.Error.NotAuthorized
  expect "17: and nothing changed" .automatic_security_updates true
  expect_reply "17: an administrator turns it off" "$(call_root SetAutomaticSecurityUpdates boolean:false)"
  expect "17: the state publishes the opt-out" .automatic_security_updates false
  [[ $(guest_ssh sudo cat /var/lib/athanor-update/automatic-security-updates) == off ]] || die "FAIL  17: the opt-out file does not read off"
  notify_watch
  meter_on
  point_stable vsec5
  check_now
  expect "17: on a metered network a security digest is not downloaded with the opt-out on" .update available
  expect "17: with no error" .last_error none
  [[ $(guest_ssh sudo bootc status --format json | jq -c .status.staged) == null ]] || die "FAIL  17: a deployment was staged on a metered network"
  meter_off
  check_now
  expect "17: unmetered, the digest stays downloaded" .update downloaded
  deployment_locked || die "FAIL  17: the digest was unlocked with the opt-out on"
  expect "17: and no lineage opens" .postpone null
  # Plan Task 10: the persistent warning row.
  shield_sheet
  sheet_shows "17: the opt-out is on" "Automatic security updates are off: security fixes wait until someone installs them."
  sheet_shows "17: the opt-out is on" "Automatic security updates"
  # The notice still arrives (Task 9: the notice of UT11, with Later and no Postpone).
  notice=$(wait_notice "17: with the opt-out on the notice still arrives" apply)
  jq -e '.ids | (index("later") != null) and (index("postpone") == null)' <<< "$notice" > /dev/null || die "FAIL  17: the notice of an opted-out machine is not the one of UT11: $notice"
  pass "17: the notice still arrives"
  expect_reply "17: an administrator turns it back on" "$(call_root SetAutomaticSecurityUpdates boolean:true)"
  expect "17: the waiting digest is unlocked" .update will-apply-at-next-shutdown
  deployment_unlocked || die "FAIL  17: turning the switch on did not unlock the digest"
  expect "17: and the lineage clock starts" .postpone.postponed false
  expect "17: with a new budget" .postpone.boots_remaining 7
  first=$(state .postpone.first_notice_at)
  (($(now_guest) - first < 600)) || die "FAIL  17: first_notice_at $first is not the time the switch was turned on"
  shield_sheet
  sheet_hides "17: the opt-out is off" "Automatic security updates are off: security fixes wait until someone installs them."
  notify_unwatch
  reboot_guest
  [[ $(marker) == vsec5 ]] || die "FAIL  17: the restart booted $(marker), expected vsec5"
  quiesce_checks
  expect_applied "17: vsec5 is applied" "$(image_digest vsec5)"
}

stage_class() { # item 18
  local tag
  [[ $(marker) == vsec5 ]] || die "stage class follows optout: the guest boots $(marker), expected vsec5"
  quiesce_checks
  for tag in v8 vsec-bad vsec-noclass; do
    point_stable "$tag"
    check_now
    expect "18: $tag waits for a confirmation" .update downloaded
    deployment_locked || die "FAIL  18: $tag was unlocked"
    expect "18: and opens no lineage" .postpone null
    check_now
    expect "18: and still waits at the next check" .update downloaded
    [[ $(marker) == vsec5 ]] || die "FAIL  18: the guest rebooted"
  done
  request_and_reboot call_root Apply vsec-noclass "18: Apply() installs the image whose attestation has no class"
}

stage_securitygreenboot() { # item 19
  local prev anchor left gb held boot
  [[ $(marker) == vsec-noclass ]] || die "stage securitygreenboot follows class: the guest boots $(marker), expected vsec-noclass"
  gb=$(image_digest vsec-gb)
  held=$(guest_ssh 'test ! -e /var/lib/athanor-update/held || cat /var/lib/athanor-update/held') || die "cannot read the guest's held digest"
  [[ $held != "$gb" ]] || die "stage securitygreenboot runs once per guest and the digest of vsec-gb is already held: reset.sh, then run.sh from install"
  prev=$(marker)
  anchor=$(lineage_anchor)
  quiesce_checks
  point_stable vsec-gb
  check_now
  expect "19: the security image whose greeter does not start is unlocked" .update will-apply-at-next-shutdown
  reboot_guest
  [[ $(marker) == vsec-gb ]] || die "FAIL  19: the restart booted $(marker), expected vsec-gb"
  left=$(state .booted.digest) || die "FAIL  19: the update service's state is unreadable after the boot"
  for _ in $(seq 60); do
    [[ $(guest_ssh sudo bootc status --format json | jq -r .status.rollbackQueued) == true ]] && break
    sleep 5
  done
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.rollbackQueued) == true ]] || die "FAIL  19: greenboot did not queue the rollback 5 minutes after the boot"
  expect_text "19: greenboot's required check failed" "$(guest_ssh sudo journalctl -b -u greenboot-healthcheck.service -o cat --no-pager)" \
    "required script /etc/greenboot/check/required.d/10-greetd-running.sh failed!"
  quiesce_checks
  check_now
  [[ $(guest_ssh cat /var/lib/athanor-update/held) == "$left" ]] || die "FAIL  19: the digest greenboot returned from is not held"
  [[ $(lineage_anchor) == "$anchor" ]] || die "FAIL  19: a boot greenboot returned from counted as applied"
  pass "19: the digest is held and the boot did not count as applied"
  boot=$(boot_id)
  guest_ssh sudo systemd-run --quiet --on-active=2 systemctl reboot
  wait_reboot "$boot" || die "FAIL  19: the guest did not come back from the restart in 10 minutes"
  [[ $(marker) == "$prev" ]] || die "FAIL  19: after the restart $(marker) is booted, expected the previous deployment $prev"
  quiesce_checks
  for n in 1 2 3; do
    check_now
    expect "19: check $n does not download the held security digest" .update held
    [[ $(guest_ssh sudo bootc status --format json | jq -r .status.staged) == null ]] || die "FAIL  19: the held digest was staged"
  done
  # The stages after these expect v2, whose digest the older stage compares with its nvidia copy.
  point_stable v2
  guest_ssh sudo bootc switch --enforce-container-sigpolicy --transport registry "$REPO:stable"
  reboot_guest
  [[ $(marker) == v2 ]] || die "FAIL  the guest did not return to v2 after the security stages: it boots $(marker)"
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

# stage_install waits for the guest itself; a later start needs its SSH and the registry tunnel.
[[ $start == install ]] || wait_ssh
run=''
for stage in "${STAGES[@]}"; do
  [[ $stage == "$start" ]] && run=1
  [[ -n $run ]] || continue
  echo "== $stage"
  "stage_$stage"
done
