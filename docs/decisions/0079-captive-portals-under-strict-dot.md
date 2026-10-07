---
id: ADR-0079
title: "Captive portals under strict DNS over TLS"
date: 2026-10-06
status: accepted
issues: [168, 143]
areas: [network, security]
---

# 0079. Captive portals under strict DNS over TLS

## Context

A2-11 (ADR-0046) sets strict DNS over TLS to one resolver with the routing domain `~.`,
and leaves captive portals to NetworkManager. #168 records why that cannot work as
written: NetworkManager resolves its connectivity-check host through systemd-resolved, so
the lookup also goes only to the configured resolver over TLS. A portal that blocks port
853 before sign-in makes the check host unresolvable, NetworkManager reports `limited` or
`none` instead of `portal`, and no sign-in page is offered. The maintainer decided on
2026-10-06 to add a dedicated portal probe rather than weaken DNS over TLS.

The same failure applies to the sign-in page itself: every name the portal page loads
resolves through systemd-resolved, so a browser opened on the portal URL cannot load it
either. Upstream systemd has no mechanism for this case; it is tracked as an open
request in systemd/systemd#29869, and systemd/systemd#11240 shows the user-visible
failure since 2018.

## Decision

1. Strict DNS over TLS stays the only resolution path for the system and every user
   session. Nothing changes systemd-resolved's configuration, and there is no
   opportunistic fallback.
2. NetworkManager stays the source of connectivity state. The portal path starts only
   when a link is up and NetworkManager reports `limited` or `none` for it.
3. Detection is a dedicated system service. It asks that link's DHCP- or RA-supplied DNS
   server for one name, the connectivity-check host, and fetches the check URL bound to
   that link. When the network advertises a captive-portal URI (RFC 8910: DHCPv4 option
   114, DHCPv6 option 103, IPv6 RA option 37), the service uses the RFC 8908 API at that
   URI instead of inferring a portal from a redirect.
4. Sign-in happens in a confined, ephemeral browser context. Its name resolution goes
   only to the link's DNS servers, through its own resolver configuration and not through
   systemd-resolved. It has no access to user data, keeps no profile, and closes when
   NetworkManager reports full connectivity. The page is treated as hostile content.
5. The user reaches sign-in through a notification ("Sign in to the network") and the
   bar's network menu (F-cc-13). Nothing opens on its own.
6. The probe and the sign-in context are confined under the project's zero-trust rules:
   the probe has no network access beyond its one query and one fetch on that link.
7. This design is a replacement for an upstream mechanism that does not exist yet. If
   systemd-resolved gains a scoped captive-portal mode, Athanor moves to it and retires
   the probe.

## Consequences

Amends ADR-0046 only in its clause "captive portals via NetworkManager": NetworkManager
keeps the connectivity state, and detection and sign-in follow this record. The rest of
ADR-0046 is unchanged.

The component is specified before any code, as a section of the network specification
or a short document of its own (#168). Its closing criterion is the one in #168,
extended to the sign-in page: behind a simulated portal that blocks port 853, the
notification appears and the portal page loads and signs in; on a normal network no
process makes a plain-text DNS query (checked with a packet capture).
