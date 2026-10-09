---
id: ADR-0089
title: "Defaults that contact or listen are off until the person turns them on"
date: 2026-10-08
status: amended by ADR-0103
issues: []
areas: [security, network]
---

# 0089. Defaults that contact or listen are off until the person turns them on

## Context

A review of what the image sends or accepts without being asked (2026-10-08) found five
defaults that Fedora enables and that no Athanor document chose: the `rpm-ostree-countme.timer`
install counter, the daily `unbound-anchor.timer` for an `unbound` that is not installed,
`mdns` and `ssh` in the default firewall zone, `sshd.service`, and the telemetry and sponsored
content defaults of the Firefox RPM. ADR-0056 says Athanor ships no telemetry, and
`doc_first_run.md` FR12 promises a list of what the computer contacts by itself.

## Decision

Decided by the maintainer on 2026-10-08.

1. `rpm-ostree-countme.timer` is disabled by preset and masked in the image. `unbound-anchor.timer`
   is disabled by preset.
2. The `public` zone of firewalld, the default zone, allows neither `mdns` nor `ssh`. Both stay in
   the `home` zone, which the person assigns to a network of their own. `avahi-daemon` keeps
   running and browses, so printers and services are still found, but it never publishes: the image
   sets `disable-publishing=yes` in `avahi-daemon.conf` (`system/Containerfile`), so the machine
   does not announce itself on any network, and inbound mDNS is closed on `public` by the zone.
   systemd-resolved's multicast DNS stays off; `.local` names resolve through avahi and nss-mdns
   (`mdns4_minimal` in `nsswitch.conf`, IPv4 only).
3. `sshd.service` is off on new installs only: the install kickstarts (`system/athanor-install.ks`,
   `system/disk_config/iso.toml`) run `services --disabled=sshd`, and the image keeps Fedora's
   preset, so that an existing installation keeps its state through the `/etc` merge. The
   `remote-login` switch of the Software application turns it on (`doc_software.md`, decision 2).
   With `ssh` out of the `public` zone, that switch must also add the `ssh` service to the zone of
   the active connection; this is a gap of the specification and no code exists for it.
4. Until Firefox moves to Flatpak, the image ships `/usr/lib64/firefox/distribution/policies.json`
   with telemetry, studies, the default-browser agent, Pocket and sponsored content turned off.
5. What the image contacts by itself is a machine-readable list, `forge/config/contacts.toml`.
   `python3 scripts/verify.py contacts` compares it with the presets of the repository, and
   `forge/scripts/check_image_contacts.py` with the units of a built image.
6. Quad9 with strict DNS over TLS stays the default resolver (ADR-0046) and is listed as a contact.
   First run and Settings are to offer the choice of resolver (`doc_first_run.md` FR12,
   `doc_settings.md` SE9).

## Consequences

- Applied by `athanor-base-config` (preset, firewalld zone), `athanor-system-tweaks` (resolved),
  `athanor-desktop-ui` (Firefox policy).
- **Existing installs.** `/etc/firewalld/zones/public.xml` is configuration: a machine that has no
  local copy of it takes the new zone at the next upgrade, and loses `ssh` and `mdns` in `public`.
  Because `sshd` stays enabled there, a remote machine whose network is in the `public` zone
  becomes unreachable over ssh after the upgrade. Recovery, from the console or another zone:
  `firewall-cmd --permanent --zone=home --change-interface=<interface>` (or set the zone of the
  connection with `nmcli connection modify <name> connection.zone home`), or
  `firewall-cmd --permanent --zone=public --add-service=ssh`. No migration is shipped: opening
  `ssh` again in `public` for a machine that has `sshd` enabled would undo decision 2.
- The image check `forge/scripts/check_image_contacts.py` sees timers and the listed units in the
  `*.wants` and `*.requires` directories of the system and user unit paths, with masks given by a
  symlink to `/dev/null` or an empty file. It does not follow `Wants=` inside another unit.
  `scripts/verify.py contacts` reads the repository presets and explicit enables only; what Fedora's
  `90-default.preset` enables is covered by the image check in CI alone. `sshd.service` is a
  `listener` in `contacts.toml`: enabled in the image, off on new installs.
- `doc_first_run.md` FR12 and `doc_settings.md` SE9 carry the resolver requirement; no code for it
  exists yet.
- `call-system-image.yml` runs `system/check-image-contacts.sh` on the built system image, next to the RPM check.
