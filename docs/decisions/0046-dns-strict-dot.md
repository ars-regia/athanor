---
id: A2-11
title: "DNS model: strict DNS over TLS"
date: 2026-10-05
status: amended by ADR-0079
issues: [143]
areas: [network, security]
---

# 0046. DNS model: strict DNS over TLS

## Context

Recorded in the maintainer decision log, section 'Audit 2 decisions (2026-10-05, maintainer)'. Source: audit2/SYNTHESIS.md.

## Decision

DNS model (b): strict DoT to a chosen resolver (Quad9 default, configurable), routing domain ~., DNSSEC allow-downgrade, captive portals via NetworkManager. #143 to be revised accordingly.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.

Amended 2026-10-08 by the maintainer: Quad9 with strict DNS over TLS stays the default. The resolver the decision calls "configurable" is chosen at first run and in Settings, Network, among Quad9, Cloudflare, the network's own DNS (from DHCP) and a custom resolver (`doc_first_run.md` FR12, `doc_settings.md` SE9); no implementation exists yet. The default is listed as a contact in `forge/config/contacts.toml` ([ADR-0089](0089-defaults-that-contact-or-listen.md)).
