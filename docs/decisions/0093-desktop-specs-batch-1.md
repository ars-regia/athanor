---
id: ADR-0093
title: "Desktop specifications, review batch 1, approved"
date: 2026-10-08
status: amended by ADR-0103
issues: [151, 156, 161]
areas: [shell, session, security]
---

# 0093. Desktop specifications, review batch 1, approved

## Context

The first batch of the 1.0 specification review covers the desktop that replaces COSMIC, in
order of dependency: compositor, session, session daemons, lock and prompts, on-screen display,
Settings, portal, shell and launcher. Each specification was reviewed with its open questions and
a recommendation for each. On 2026-10-08 the maintainer approved all nine "with the indicated
changes" and accepted every recommendation, including the question added on PR #1441
(ADR-0092).

## Decision

1. `doc_compositor.md` revision 2: the trusted-path patch held as ADR-0092 says; patch 1 proposed
   upstream before the next rebase, patch 2 not proposed; the rebase cost accepted before it is
   measured, the first rebase reopening CO3 if it exceeds the budget.
2. `doc_session.md` revision 2: `PartOf=graphical-session.target` as the one binding, within
   A2-20, so LP2's `athanor-session.target` is reversed (SN3); the guards' back-off and its reset
   (SN6); no lock on a guard's failure; the notice and the crash report (SN8).
3. `doc_session_daemons.md` revision 2, decisions 10 to 14: Software `confined` (ADR-0077 point 2);
   SD8 and SD9 approved before their spikes; the residual risk of the writable
   `org.athanor.desktop.*` schemas accepted for 1.0, as `doc_threat_model.md` TM3 states; restricted callers identified by cgroup and by executable under `/usr`;
   the order of SD22 confirmed.
4. `doc_lock_and_prompts.md` revision 2, decisions D13 to D16: the session's keyboard layout named
   and switchable on every trusted surface and restored on unlock; the keyring prompter deferred
   to the Fedora 45 base (ADR-0085); the indicator for every window state proposed upstream, the
   gap stated as residual risk meanwhile. Memory figures are estimates under ADR-0068.
5. `doc_osd.md` revision 2, decisions M7 to M10: an upstream D-Bus action type, not a resident key
   grabber, if a key costs more than 30 ms; the `Ask` methods callable by any session peer, an
   accepted consequence stated in OD4; memory under ADR-0068; the power-key inhibitor only if
   spike O7 shows nothing holds it.
6. `doc_settings.md` revision 2, decisions 9 to 12: the resolver choice applied by a confined
   system service under its own polkit action (`auth_admin_keep`); what Settings
   starts runs in four units installed with the image, started by name through the filtered
   bus and never as transient units (revised the same day after the review of PR #327); steps 2 to 4
   wait for a polkit agent; the report moves to `doc_report_problem.md`.
7. `doc_portal.md` revision 2, decisions 8 to 10: the "why not upstream" paragraph and the owner
   (A2-14); the six construction steps approved together; the accepted risk of one backend process
   holding the main socket and reading `$HOME`.
8. `doc_shell.md` revision 5 approved as written, confirming that of COSMIC only cosmic-comp stays.
9. `doc_launcher.md` revision 2: Alt+Tab belongs to `doc_overview.md`; applications start through
   `Opener` until the broker of SD22 step 6.

## Consequences

The nine specifications carry the decisions in their sections 6 and their status lines; this
record is the index. `doc_report_problem.md` revision 1 is approved with them. The dconf residual
risk is already stated in `doc_threat_model.md` TM3. Open follow-ups named by the
specifications: the A2-14
maintainer list for each replacement component, and spike S4 of `doc_languages.md` for the
greeter's keyboard layout.
