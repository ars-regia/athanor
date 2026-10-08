---
id: ADR-0089
title: "Defaults that contact or listen are off until the person turns them on"
date: 2026-10-08
status: accepted
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
   running, so it is reachable only where the zone allows it. systemd-resolved's multicast DNS is
   off globally; NetworkManager turns it on per connection, for a network in the `home` zone.
3. `sshd.service` is disabled by preset. The developer mode of the Software application turns it
   on again (`doc_software.md`, decision 2).
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
- `doc_first_run.md` FR12 and `doc_settings.md` SE9 carry the resolver requirement; no code for it
  exists yet.
- Wiring `system/check-image-contacts.sh` into the image workflow is not done by this record.
