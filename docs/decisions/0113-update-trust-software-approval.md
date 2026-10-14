---
id: ADR-0113
title: "Approve update trust revision 4 and software revision 3, aligned to ADR-0103"
date: 2026-10-14
status: accepted
issues: [159]
areas: [update, security, apps]
---

# 0113. Approve update trust revision 4 and software revision 3, aligned to ADR-0103

## Context

[doc_update_trust.md](../architecture/doc_update_trust.md) carried the consent of 2026-09-19
to revision 2, and revision 4 of 2026-10-08 (ADR-0094) was never approved, so its text could
not be built ([ADR-0074](0074-agent-and-contributor-model.md) item 5). Two of its items
disagreed with [ADR-0103](0103-audit-5-decisions.md), which prevails until the text is
aligned:

- UT2 says the image key has no revocation. D5 amends it: before 1.0 an offline root signs a
  short-lived online signing key and a timestamp, clients pin the root, and the interim for
  the Machine Owner Key is a signed key set with a not-after date and revocation.
- UT6 gives `os.athanor.update.apply` the defaults `auth_admin_keep` / `auth_admin_keep` /
  `yes`, with no case for the sessions of other users. D22 asks an administrator when other
  sessions exist, allows the active session alone, and has `blocked()` fail closed; the
  service's inhibitor query answers "not blocked" today when it fails.

[doc_software.md](../architecture/doc_software.md) was approved at revision 2 on 2026-09-30.
Revision 3 of 2026-10-05 added decision 7, the default applications, and section 9
(2026-10-06) overrode four of its rows: Nautilus is the file manager of 1.0, GNOME Disks is
the disk utility, Firefox is the Flatpak ([ADR-0097](0097-owners-execution-firefox-processes.md)),
and Bazaar follows the Fedora 45 base ([A2-28](0065-bazaar-waits-for-fedora-45.md), D8). The
table of decision 7 still read as before section 9.

ADR-0112 (#417) lifts the freeze of ADR-0103 D3 on specification revisions.

## Decision

Decided by the maintainer on 2026-10-14.

1. **`doc_update_trust.md` revision 4 is approved, with UT2 and UT6 aligned to ADR-0103 D5
   and D22.**
   - UT2 keeps the single key and its out-of-band recovery until the key hierarchy exists,
     and states the hierarchy of D5: the offline root, the short-lived online key with a
     not-after date, revocation by the root, and the Machine Owner Key's interim key set.
   - UT6 adds a second polkit action, `os.athanor.update.apply-multi-session`,
     `auth_admin` for every subject and never kept. The service lists logind's sessions
     when `Apply()` arrives and checks that action while a `user` session of another uid is
     open, and `os.athanor.update.apply` otherwise; a failed count checks the stricter
     action. The alternative, the service refusing or escalating on its own, was weighed and
     not taken: the defaults of one polkit action are fixed and a rule cannot see the other
     sessions, so polkit asks for an administrator in this case only through an action of
     its own, and a refusal would leave an administrator no way to apply from the desktop.
     The second action is the pattern of logind's own `reboot` and
     `reboot-multiple-sessions`, whose defaults UT6 already follows.
   - The inhibitor check before `Apply()` and `GoBack()` fails closed: a failed or unreadable
     `ListInhibitors` counts as blocked, and the request is refused before anything is
     unlocked.
   - `doc_threat_model.md` TM9 notes the second action on the row of
     `os.athanor.update.apply`; the action gets its own row in the change that declares it,
     as TM9 requires.
2. **`doc_software.md` revision 3 is approved, with the table of decision 7 aligned to
   section 9.** The Files and Archives rows name Nautilus through 1.0, the Disks row GNOME
   Disks, the Web row the Firefox Flatpak, and the Applications row notes Bazaar from the
   Fedora 45 base. Section 9.4 records option (b) and Bazaar as decided, conditional on the
   Fedora 45 base. The SearXNG section of ADR-0103 D19 is not part of this revision; it comes
   as revision 4.

## Consequences

- Both documents may be implemented as approved. The `.policy` file, the service's session
  count and the fail-closed inhibitor query of D22 land in a later pull request, which adds
  the TM9 row of `os.athanor.update.apply-multi-session` and the acceptance case of UT6.
- The key hierarchy of D5 still waits for the LUKS2 key backup; its mechanism is specified by
  the revision that builds it.
- ADR-0103's Consequences name UT2, UT6 and TM9 among the texts to align; this change aligns
  them, and the specifications now carry D5 and D22.
