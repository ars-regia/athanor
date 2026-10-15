---
id: ADR-0116
title: "Give the public firewall zone back to the image where the installer wrote it"
date: 2026-10-15
status: accepted
issues: [312]
areas: [security, network]
---

# 0116. Give the public firewall zone back to the image where the installer wrote it

## Context

[ADR-0089](0089-defaults-that-contact-or-listen.md) decision 2 ships firewalld's `public` zone
without `ssh` and `mdns`, and its consequences say that a machine with no local copy of
`/etc/firewalld/zones/public.xml` takes the new zone at the next upgrade, with no migration.

Every install has a local copy. When a kickstart has no `firewall` line, Anaconda still runs
`firewall-offline-cmd --enabled --service=ssh`. The shipped kickstarts had none until
`firewall --use-system-defaults` (#479). That command writes `public.xml` with `ssh` added and
keeps the file it replaced as `public.xml.old`:

- installs older than the image's own zone carry Fedora's zone with `ssh`: `ssh`, `mdns` and
  `dhcpv6-client`;
- newer installs carry the image's zone with `ssh` added.

The file is local configuration, so the three-way merge of `/etc` keeps it through every
upgrade, and the image's zone never applies. The dev VM, installed on 2026-09-18, has both
files, and `ostree admin config-diff` lists `public.xml` as modified. Decision 2 so reaches
only machines installed from an ISO that carries #479. The maintainer chose a one-time
migration over correcting the text of ADR-0089 alone (2026-10-15).

## Decision

1. `athanor-base-config` ships `athanor-firewall-zone-migrate.service`, a oneshot ordered
   before `firewalld.service`. It replaces `/etc/firewalld/zones/public.xml` with the image's
   zone, `/usr/etc/firewalld/zones/public.xml`, and removes `public.xml.old`, when all of
   these hold:
   - `public.xml.old` exists;
   - the zone is not newer than the installer's log, `/var/log/anaconda/anaconda.log`.
     firewalld also leaves a `public.xml.old` at every permanent change, so the `.old` alone
     does not show who wrote the zone. Nothing changes the zone during the installation,
     and the `/etc` merge keeps the file's time;
   - the zone holds nothing but `short`, `description`, `forward` and the services `ssh`,
     `mdns` and `dhcpv6-client`, with no attribute on the zone or on its elements.
2. Any other zone is left alone, and the unit's journal says why. A missing local zone needs
   nothing.
3. It runs once. Its marker, `/var/lib/athanor/firewall-public-zone.done`, is written whatever
   it found, so a zone edited later is never touched.

This amends the consequences of ADR-0089 where they say that no migration is shipped.

## Alternatives rejected

- **Correct the text of ADR-0089 only.** Decision 2 would never reach an existing install.
- **Delete the local file.** The merge keeps a deletion as a local change, and firewalld then
  reads Fedora's zone in `/usr/lib/firewalld/zones`, which opens `ssh` and `mdns`.
- **Keep `ssh` where `sshd` is enabled.** Fedora's preset enables `sshd` and existing installs
  keep it, so `ssh` would stay open almost everywhere, which undoes decision 2 (ADR-0089 says
  the same).

## Consequences

- At the first boot of the image that carries the unit, a machine reached over `ssh` on a
  network in the `public` zone becomes unreachable over `ssh`, and stops announcing itself
  over `mdns`. The `home` zone keeps both.
- Recovery, from the console or from a network in another zone:
  - `firewall-cmd --permanent --zone=home --change-interface=<interface>`;
  - or set the zone of the connection: `nmcli connection modify <name> connection.zone home`;
  - or `firewall-cmd --permanent --zone=public --add-service=ssh`.

  Doing one of the first two before the upgrade avoids the lockout. The third does nothing
  before the migration, because the installer's zone already holds `ssh`.
- After the replacement the file equals the image's, so the merge treats it as unmodified
  and later images' zones apply to it.
- The dev VM is reached over `ssh` through QEMU's user network, from `10.0.2.2`.
  `scripts/devvm/devvm.ks` binds that address to the `home` zone, and
  `scripts/devvm/acceptance/expectations.toml` declares the `home` zone next to `public`.
  A dev VM base created before needs the same binding once (`scripts/devvm/README.md`).
