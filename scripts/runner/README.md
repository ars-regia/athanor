# Self-hosted runner

The `self-hosted` jobs of the kernel workflows (`kernel-build.yml`, `nvidia-build.yml`,
`kernel-weekly.yml`, `kernel-bump.yml`) run on this runner. The repository is public
and a runner executes workflow code, so it lives in an ephemeral KVM guest, never on the
host itself.

## How it works

- `athanor-runner.service` runs `vm.sh` as a dynamic user, with no access to the host
  beyond `/dev/kvm`, its own state, runtime and log directories, and the network.
- For every job `vm.sh` asks GitHub for a just-in-time runner configuration
  (`generate-jitconfig`), valid for one job only, and boots the base image with
  `snapshot=on`: nothing the job writes to the system disk survives it.
- QEMU passes the configuration to the guest as the systemd credential `jitconfig`
  (`fw_cfg`); `actions-runner.service` in the guest runs the job. The end of the job is
  decided on the host: `vm.sh` polls the registration every 30 s and, once GitHub has
  removed it, sends an ACPI power-off through QMP and terminates the guest if it has not
  stopped within two minutes. The service restarts `vm.sh`, which boots a clean guest
  for the next job.
- The poll never trusts a single answer from GitHub to end a running job: a transport
  failure (DNS, a reset connection, a timeout) or a 429/5xx is retried a few times with
  a short backoff, and only a definitive 404 powers the guest off. If the API stays
  unreachable across many polls, `vm.sh` only logs a warning and leaves the guest
  running — its own `RuntimeMaxSec` (8 h in the unit) is the backstop, since killing a
  running job over a network blip is worse than a late shutdown.
- The guest's memory is lent, not reserved. An image build fills the guest with page
  cache, and QEMU does not give those pages back by itself: on 2026-09-28 the host ran
  out of memory twice and its OOM killer ended the job by killing QEMU. The guest now has
  a virtio balloon with free page reporting (what the guest frees returns to the host)
  and deflate-on-oom (the guest takes pages back rather than OOM itself), and
  `balloon.py`, started by `vm.sh` on a QMP socket of its own, steers it: every 2 s it
  sizes the guest so that the host keeps `HOST_RESERVE` available, never below
  `BALLOON_FLOOR` nor above `VM_MEMORY`. Under pressure the guest drops its cache first;
  once the host has room it grows back. The regulator is not part of the job: if it
  fails, the job goes on and the reason is in the journal (lines start with `balloon:`).
- Two extra disks: `cache.raw` persists across jobs (podman storage, `~/.cache/azoth`),
  `scratch.raw` is recreated empty before every job (the work directory).
- The GitHub token is a service credential encrypted with the host key and the TPM2
  (`/etc/credstore.encrypted/athanor-runner.github-token`), used only to create and
  remove runner registrations.
- Pull requests from forks never reach the runner: the self-hosted jobs of
  `kernel-build.yml` skip them, and the repository requires approval for every outside
  contributor.

Pins and sizing are in `runner.env`: Fedora Cloud Base and actions/runner by SHA-256,
12 vCPUs and up to 16 GB (at least 6 GB, keeping 3 GB available on the host), CPU and
I/O weights that leave the desktop responsive.

## Install

```sh
bash scripts/runner/build-image.sh
gh auth token | sudo scripts/runner/install.sh --image ~/.cache/athanor-runner/golden.qcow2
journalctl -fu athanor-runner.service
```

The guest serial console of the current job is `/var/log/athanor-runner/console.log`.

## Update

Change the pins in `runner.env`, rebuild the image and run `install.sh` again. A new
token (after `gh auth refresh` or a new PAT) needs `install.sh` again as well.

## Remove

```sh
sudo systemctl disable --now athanor-runner.service
sudo rm -rf /etc/systemd/system/athanor-runner.service /usr/local/libexec/athanor-runner \
  /etc/credstore.encrypted/athanor-runner.github-token /var/lib/private/athanor-runner \
  /var/log/private/athanor-runner
sudo systemctl daemon-reload
```
