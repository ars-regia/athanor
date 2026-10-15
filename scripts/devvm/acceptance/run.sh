#!/usr/bin/env bash
# The acceptance of docs/architecture/doc_update_trust.md, section 6, items 1 to 15, on the
# development VM, against the registry and keys of images.sh. Repeatable: `reset.sh` first.
# Every bootc call is made by the units themselves (item 14): this script only starts units
# and calls the bus. Usage: run.sh [START_AT]   (a stage name below; default: the first)
set -euo pipefail
# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

guard_no_ci
STAGES=(install migrate metered download sessions apply refuse older goback rotate recover podman probes report)
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

stage_apply() { # items 2, 3 and 14
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
  expect "3: verified after the reboot" .verified.reason signature
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
  guest_ssh sudo systemctl start athanor-update-state.service
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

stage_podman() { # item 15
  guest_ssh 'podman pull docker.io/library/busybox:latest && podman save -o /var/tmp/acc-busybox.tar busybox && podman rmi busybox && podman load -i /var/tmp/acc-busybox.tar && printf "FROM busybox\nRUN true\n" | podman build -t acc-build - && rm /var/tmp/acc-busybox.tar'
  pass "15: podman pull from another registry, podman load and podman build work with the policy in force"
}

stage_probes() { # the runtime audit probes (issue #312), against expectations.toml
  local facts=$ACC_STATE/probes-facts.json
  guest_ssh python3 - collect < "$ACC_HERE/probes.py" > "$facts"
  # The verdict names each check; a failure stops the run, and the facts stay in $ACC_STATE.
  python3 -B "$ACC_HERE/probes.py" check "$facts" "$ACC_HERE/expectations.toml" "$ROOT/forge/config/contacts.toml" || die "FAIL  runtime probes (facts: $facts)"
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
