#!/usr/bin/env bash
# upgrade.sh: switches the guest to the newest published system image and reboots into
# it. `bootc upgrade` cannot do this here: create.sh installs from an ISO whose kickstart
# pins `ostreecontainer` to the run-id tag it was built with (scripts/devvm/devvm.ks),
# and upgrade only re-pulls that same tag, never a newer one. This runs the equivalent
# `bootc switch` to `:latest` instead, the way the dev VM verification reports did by hand.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source-path=SCRIPTDIR
source "$HERE/devvm.env"

guest_ssh sudo bootc switch --transport registry "$SYSTEM_IMAGE:latest"
guest_ssh sudo systemctl reboot
echo "rebooting; waiting for SSH"
sleep 5 # give the reboot time to actually drop the current SSH session
for _ in $(seq 180); do
  guest_ssh -q true 2> /dev/null && break
  sleep 2
done
guest_ssh -q true || die "no SSH on 127.0.0.1:$SSH_PORT after 6 minutes"
echo "up on $SYSTEM_IMAGE:latest"
