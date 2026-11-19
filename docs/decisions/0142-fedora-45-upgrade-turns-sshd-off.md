---
id: ADR-0142
title: "The move to Fedora 45 turns sshd off on upgraded machines, as Fedora's desktop preset does"
date: 2026-11-19
status: accepted
issues: []
areas: [security, network]
---

# 0142. The move to Fedora 45 turns sshd off on upgraded machines, as Fedora's desktop preset does

## Context

[ADR-0103](0103-audit-5-decisions.md) D18, as [ADR-0119](0119-ssh-closes-in-the-public-zone-on-upgrade.md)
point 1 keeps it, says that an upgrade leaves `sshd` enabled where it was. On Fedora 43 that holds
by default: the image's own `/etc` enables `sshd` through Fedora's preset, and an upgrade does not
rerun the kickstart that turns it off on a new install (`system/tests/test_sshd_default.py`).

Fedora 45's `base-atomic` image ships `81-desktop.preset`, which disables `sshd`. Spike S1
(finding F2, 2026-11-19) saw the switch from a Fedora 43 image to a Fedora 45 one turn `sshd` off.
bootc's `/etc` merge takes the new image's default for every file the machine did not change. On
both machines read on 2026-11-19, the desktop and the laptop, the only link that enables `sshd`
is `/etc/systemd/system/multi-user.target.wants/sshd.service`, and the same link is in the
image's default `/etc` (`/usr/etc`). Neither machine has a local change that would keep it.

## Decision

1. The move to Fedora 45 follows Fedora's preset. On a machine where `sshd` is enabled only by
   the image's default `/etc`, `sshd` is off after the switch. A link that a person added to
   `/etc` and that the image's default does not hold stays, and so does the state of `sshd` it
   gives. This amends ADR-0103 D18 and ADR-0119 point 1 for `sshd`; ADR-0119's firewall points
   stand.
2. No package of Athanor ships a preset, link or unit that enables `sshd`. New installs keep it
   off (ADR-0089 item 2).
3. The release notes of the first release on Fedora 45 say that remote login is off after the
   update, and how to turn it on again from a console:
   `sudo systemctl enable --now sshd.service`, together with ADR-0119 point 3 on a network in
   the `public` zone. A machine that must stay reachable over `ssh` across the update adds, before
   it, a link the image's default does not hold:
   `sudo systemctl add-wants graphical.target sshd.service`.
4. The upgrade acceptance image and the development VM enable `sshd` in their own build
   (`scripts/devvm/acceptance/Containerfile`, `scripts/devvm/devvm.ks`); the product image does not.

## Alternatives rejected

- **An Athanor preset that enables `sshd`, ordered before `81-desktop.preset`.** It keeps D18 as
  written and costs one line, but it leaves remote login on, on almost every existing machine, by
  a choice Athanor makes against its base. The maintainer chose Fedora's default (2026-11-19):
  remote login is a choice a person makes, on an existing machine as on a new one.

## Consequences

- People who reach a machine over `ssh` lose that access at the update unless they add the link of
  point 3 first. Spike S1 checks that the link survives the switch.
- `system/tests/test_sshd_default.py` checks point 2 in place of the old promise that the state in
  `/etc` survives.
- The image check keeps `sshd.service` as an allowed listener (`forge/config/contacts.toml`): a
  machine where a person turned it on is still within the policy.
