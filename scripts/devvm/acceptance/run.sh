#!/usr/bin/env bash
# The acceptance of docs/architecture/doc_update_trust.md, section 6, items 1 to 15, on the
# development VM, against the registry and keys of images.sh. Repeatable: `reset.sh` first.
# Every bootc call is made by the units themselves (item 14): this script only starts units
# and calls the bus. Usage: run.sh [START_AT]   (a stage name below; default: the first)
set -euo pipefail
# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

guard_no_ci
STAGES=(install migrate download apply refuse older goback rotate recover podman report)
start=${1:-install}
[[ " ${STAGES[*]} " == *" $start "* ]] || die "unknown stage '$start': one of ${STAGES[*]}"

stage_install() { # the machine starts from v1, on a reference that verifies nothing
  point_stable v1 # where images.sh left it; an earlier run moved it on
  wait_ssh
  # The installed image does not know the throwaway registry: the first switch needs the
  # drop-in that every acceptance image ships (Containerfile).
  printf '[[registry]]\nlocation = "%s"\ninsecure = true\n' "${ACC_REGISTRY%%/*}" |
    guest_ssh sudo tee /etc/containers/registries.conf.d/50-acceptance.conf > /dev/null
  guest_ssh sudo bootc switch --transport registry "$REPO:v1"
  reboot_guest
  [[ $(marker) == v1 ]] || die "the guest did not boot v1"
}

stage_migrate() { # items 7 and 8
  expect "7: not verified before the migration" .verified.reason media
  expect "7: and nothing is downloaded meanwhile" .update none
  guest_ssh 'systemctl is-active athanor-update-migrate.service || sudo journalctl -u athanor-update-migrate.service -n 5 --no-pager'
  for _ in $(seq 60); do guest_ssh test -e /var/lib/athanor-update/migrated && break; sleep 10; done
  guest_ssh test -e /var/lib/athanor-update/migrated || die "FAIL  the migration did not complete in 10 minutes"
  reboot_guest
  [[ $(marker) == v1 ]] || die "FAIL  7: the migration changed the version"
  expect "7: verified after the migration, with no update in between" .verified.reason signature
  [[ $(guest_ssh sudo bootc status --format json | jq -r .status.booted.image.image.signature) == containerPolicy ]] || die "FAIL  8: the reference does not enforce the policy"
  pass "8: the machine follows the signed reference"
}

stage_download() { # item 1
  point_stable v2
  expect_until "1: downloaded by the timer with no user action" .update downloaded 20
  power_cycle
  [[ $(marker) == v1 ]] || die "FAIL  1: a poweroff applied the update"
  pass "1: a poweroff boots the old version"
  expect_until "8: and it keeps updating" .update downloaded 20
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

stage_report() {
  guest_ssh "systemctl --user --machine=$GUEST_USER@.host is-active athanor-update-notify.service" || die "FAIL  the notifier is not running under its Landlock ruleset"
  pass "UT11: the notifier runs confined; its notifications are looked at by hand (scripts/devvm/screenshot.sh)"
  echo "acceptance complete"
}

run=''
for stage in "${STAGES[@]}"; do
  [[ $stage == "$start" ]] && run=1
  [[ -n $run ]] || continue
  echo "== $stage"
  "stage_$stage"
done
