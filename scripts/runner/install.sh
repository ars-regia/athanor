#!/usr/bin/env bash
# Installs the self-hosted runner on this host (scripts/runner/README.md): vm.sh, balloon.py,
# runner.env and README.md under /usr/local/libexec/athanor-runner, the unit under
# /etc/systemd/system, the polkit rule that lets the service inhibit sleep during a job
# under /etc/polkit-1/rules.d, the base image built by build-image.sh into the state directory
# of the service, and the GitHub token, read from standard input and encrypted with
# systemd-creds (host key and TPM2) into /etc/credstore.encrypted. The token is never
# written in clear and never appears on a command line. Stops a running runner first:
# its job, if any, is lost.
#
# Usage: gh auth token | sudo scripts/runner/install.sh --image ~/.cache/athanor-runner/golden.qcow2
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
die() { echo "error: $*" >&2; exit 1; }

IMAGE=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --image) IMAGE=$2; shift 2 ;;
    *) die "unknown argument: $1" ;;
  esac
done
[[ $EUID -eq 0 ]] || die "run as root: gh auth token | sudo $0 --image IMAGE"
[[ -f $IMAGE ]] || die "--image: base image not found: ${IMAGE:-<none>} (build-image.sh builds it)"
[[ ! -t 0 ]] || die "pipe the GitHub token on standard input: gh auth token | sudo $0 --image IMAGE"

UNIT=athanor-runner.service
LIBEXEC=/usr/local/libexec/athanor-runner
CREDENTIAL=/etc/credstore.encrypted/athanor-runner.github-token
# DynamicUser=yes keeps the state directory under /var/lib/private; systemd hands it to
# the dynamic user when the service starts.
STATE=/var/lib/private/athanor-runner

if systemctl is-active --quiet "$UNIT"; then systemctl stop "$UNIT"; fi

install -d -m 0755 "$LIBEXEC"
install -m 0755 "$HERE/vm.sh" "$HERE/balloon.py" "$LIBEXEC/"
install -m 0644 "$HERE/runner.env" "$HERE/README.md" "$LIBEXEC/"
install -m 0644 "$HERE/$UNIT" "/etc/systemd/system/$UNIT"
# -D: creates rules.d only where polkit is missing, never changes the mode of the existing one.
install -D -m 0644 "$HERE/50-athanor-runner-inhibit.rules" \
  /etc/polkit-1/rules.d/50-athanor-runner-inhibit.rules

install -d -m 0700 /etc/credstore.encrypted
systemd-creds encrypt --with-key=host+tpm2 --name=github-token - "$CREDENTIAL"
[[ $(systemd-creds decrypt --name=github-token "$CREDENTIAL" - | wc -c) -gt 1 ]] \
  || die "the token read from standard input is empty"

install -d -m 0700 /var/lib/private
install -d -m 0700 "$STATE"
install -m 0644 "$IMAGE" "$STATE/golden.qcow2"

systemctl daemon-reload
systemctl enable --now "$UNIT"
echo "installed; follow it with: journalctl -fu $UNIT"
