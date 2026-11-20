---
id: ADR-0144
title: "The QEMU guest agent is off until a person turns it on, and then answers an allow-list"
date: 2026-11-20
status: accepted
issues: []
areas: [security]
---

# 0144. The QEMU guest agent is off until a person turns it on, and then answers an allow-list

## Context

Spike S1 (finding F6, 2026-11-20) ran the runtime probes on the Fedora 45 guest. The
acceptance expectation `[units] absent = ["qemu-guest-agent.service"]` (#996) failed: the
image ships `qemu-guest-agent`, and it always has. The Fedora 43 package set of 2026-11-18 holds
`qemu-guest-agent-2:10.1.5-1.fc43`, and the Fedora 45 set holds `11.1.3-1.fc45`. The package
comes with `base-atomic`, and nothing in the repository adds it or removes it.

Two things start it, on any Athanor machine that a QEMU or KVM hypervisor runs with the agent's
virtio port:

- Fedora's `90-default.preset` enables it, and its `WantedBy=` is the port's device.
- Its own `99-qemu-guest-agent.rules` sets `SYSTEMD_WANTS` on the port, so the device starts it
  even when it is disabled.

Fedora runs it with no RPC refused (`QEMU_GA_ARGS` is empty). Whoever holds the host's end of
the channel can do all of the following as root in the guest:

- run any command (`guest-exec`);
- read and write any file;
- set a user's password and add SSH keys.

On hardware the port never appears, and the agent never starts.

[ADR-0089](0089-defaults-that-contact-or-listen.md) turns off the defaults that contact or
listen until the person turns them on. The agent listens to the hypervisor, and no Athanor
document chose it.

## Decision

Decided by the maintainer on 2026-11-20.

1. The image keeps `qemu-guest-agent` installed, and it is **off by default**:
   - `80-athanor-base.preset` disables it.
   - `99-zz-athanor-qemu-guest-agent.rules` sorts after Fedora's rule and clears its
     `SYSTEMD_WANTS`, so the device alone no longer starts the agent.

   The unit is not masked. One command turns it on, and it stays on through updates, because
   the link it writes is a local change that bootc's `/etc` merge keeps:
   `sudo systemctl enable --now qemu-guest-agent.service`.
2. Once turned on, the agent answers only an allow-list, set by the drop-in
   `qemu-guest-agent.service.d/10-athanor.conf` through `--allow-rpcs`. The list holds what a
   hypervisor needs:
   - the guest's addresses, name, system, time zone and time, and setting the time;
   - the file system list, freeze and thaw for a consistent snapshot, and trim;
   - shutdown.

   It refuses commands, files, passwords, SSH keys, the users' list, suspend, and changes to
   CPUs and memory.

   The drop-in also sets `NoNewPrivileges=yes` (TM8), `LockPersonality=yes`,
   `RestrictRealtime=yes` and `RestrictSUIDSGID=yes`. Under `no_new_privs` the kernel refuses
   systemd's transition to Fedora's confined domain, `virt_qemu_ga_t`, unless the policy
   grants `nnp_transition`, and the agent would run in `init_t`. That was measured on Fedora 45
   on 2026-11-20. So `athanor-selinux` ships `athanor_qemu_ga.cil`, which grants it, as
   `athanor_nix_daemon.cil` does for nix-daemon.

   A `QEMU_GA_ARGS` line written in `/etc/sysconfig/qemu-ga` replaces the list, because systemd
   reads that file after the drop-in. That is the person's choice. Fedora 45's file carries the
   line commented out.

## Alternatives rejected

- **Remove the package.** It is the most closed option. A person who runs Athanor in a VM
  would lose the addresses in the host's interface, consistent backups and time after the
  host resumes. They could not add the package back, because layering breaks `athanor-update`.
- **Mask the unit.** It is one line, but turning the agent on would then take an `unmask`
  and a start, and the udev rule would still decide when it runs.
- **Keep Fedora's default.** The agent would stay on with every RPC open, on every machine a
  QEMU host runs.

## Consequences

- An existing machine running in a VM loses the agent at the update that ships this. The
  image's default `/etc` no longer holds the device's wants link, and the person did not add
  it. The release notes of that release say so, and give the command of point 1.
- The acceptance moves `qemu-guest-agent.service` from `absent` to `disabled`
  (`scripts/devvm/acceptance/expectations.toml`). `system/tests/test_qemu_guest_agent.py`
  checks the preset line, the udev rule, the package's file list, the allow-list against the
  list of decision 2, and the policy rule that keeps the domain.
- The development VM offers no agent port. The acceptance therefore reads the unit's state,
  not a running agent. On 2026-11-20 `qemu-ga` 11.1.3 was started on the Fedora 45 S1 guest
  with this list on a Unix socket. It answered `guest-ping` and `guest-get-osinfo`, and refused
  `guest-exec`, `guest-file-open`, `guest-ssh-get-authorized-keys` and `guest-get-users`. The
  udev rule and the preset in a built image, on a VM that offers the port, are checked by
  hand.
- `doc_software.md` has no switch for the agent. Adding a row to its table of switches is a
  change to the specification, and is left to its next revision.
