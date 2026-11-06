---
id: ADR-0135
title: "A faster road to 1.0: ten open pull requests, a scope freeze, and the own compositor's full acceptance in 1.1"
date: 2026-11-05
status: amended by ADR-0138
issues: [835, 877]
areas: [process, docs, shell, security]
---

# 0135. A faster road to 1.0: ten open pull requests, a scope freeze, and the own compositor's full acceptance in 1.1

**Accepted** by the maintainer on 2026-11-05, by selectable options answered that day.

## Context

The maintainer asked on 2026-11-05 to shorten the road to release 1.0. The measured pace of the
repository on that date:

| Window to 2026-11-05 | Pull requests merged | Issues closed | Issues opened |
| --- | --- | --- | --- |
| 7 days | 108 | 58 | 52 |
| 14 days | 221 | 70 | 93 |

On 2026-11-05, 80 issues were open: 14 in milestone 0.10, 25 in 1.0, 3 in 0.9, 23 in "After 1.0" and 15 in no
milestone. Merges keep pace with the work, but over fourteen days the issues opened outnumber
those closed by 23, so the milestones before 1.0 grow while they are worked.

Three rules hold the pace:

- `AGENTS.md` allows at most five open pull requests. [ADR-0112](0112-pace-to-0-9.md) item 2
  suspended the limit until the signed 0.9 tag, which has since been pushed, and recorded that
  the recommendation then was a limit of ten. The limit of five is in force again.
- No rule decides the milestone of a newly found issue. `docs/operations/release-1.0.md`
  section 5 lets only a decision record change the rows of 1.0, but an issue placed in 0.10 or
  1.0 adds work to that release without changing a row.
- Release 1.0 waits for the full acceptance of the own compositor, now `athanor-sulphur`
  (Sulphur, [ADR-0134](0134-alchemical-names-for-the-pillars.md)): OS19 of `doc_own_stack.md`
  revision 2 in full ([ADR-0121](0121-own-compositor-and-toolkit.md) decision 5;
  [ADR-0123](0123-rebase-release-0-10-rows-7-and-25.md) item 5), and for the overview and the
  window switcher on Alt+Tab, with cosmic-comp's session leaving the greeter and
  cosmic-workspaces retired in 1.0 ([ADR-0132](0132-own-compositor-default-before-full-acceptance.md);
  `release-1.0.md` revision 7). ADR-0132 already lets Sulphur become the default session before
  then, under the conditions of `doc_own_stack.md` decision 28, with cosmic-comp's session at
  the greeter as the second choice; the greeter checks Sulphur's session by default today wherever its package is
  installed (`athanor-greeter-ui`, `src/auth.rs`, `Session::Own`).

Fedora 43 reaches its end of life on 2026-12-02. Release 0.10 "base", the Fedora rebase, ships
before that date, and release 0.11 "anchor" follows it ([ADR-0123](0123-rebase-release-0-10-rows-7-and-25.md),
[ADR-0128](0128-row-7-and-key-2-in-1-0.md)).

## Decision

Decided by the maintainer on 2026-11-05.

1. **The open pull request limit is ten.** `AGENTS.md` allows at most ten open pull requests in
   place of five. The rest of the rule holds: with ten open, finish before starting, a new pull
   request waiting until one is merged or closed; reviewers' findings on open pull requests are
   closed before new work begins; and a branch without an open pull request is merged, archived
   as a tag `archive/<branch>`, or deleted. This amends ADR-0112 item 2, in the limit that returned
   with the signed 0.9 tag.
2. **The scope of 0.10 and 1.0 is frozen.** From 2026-11-05, a newly found issue goes to the
   milestone "After 1.0" unless one of these holds:
   - (a) it blocks a row of `release-1.0.md` section 2, or the exit evidence of a release;
   - (b) it is a security defect or a regression;
   - (c) the maintainer places it explicitly.

   An issue that meets (a), (b) or (c) is placed in a release before or at 1.0 as before. Issues
   already in milestones 0.10 and 1.0 stay where they are.
3. **Parts of the own compositor move to 1.1.** Release 1.0 ships with Sulphur as the default
   session and cosmic-comp's session at the greeter as the second choice, as ADR-0132 already
   allows before 1.0. Sulphur is not the only session of 1.0.
   - **Moved to 1.1:**
     - the full acceptance of `doc_own_stack.md` revision 2, OS19 items 1 to 5 as written,
       beyond what decision 28 asks for the switch: among them OS4 and OS5 tested in the fixture
       of OS6 with the trusted path (`doc_session.md` SN16, gap 2), the keyboard monitor that
       answers Orca alone checked in the default session (gap 3), and the hour-long console run
       per machine of item 4;
     - the overview, `athanor-overview`, and the window switcher on Alt+Tab, which revision 7 of
       `release-1.0.md` added to 1.0;
     - the retirement of cosmic-workspaces, which revision 7 moved to 1.0; it stays in
       cosmic-comp's session, as [ADR-0103](0103-audit-5-decisions.md) D13 placed it;
     - with them, cosmic-comp's session leaving the greeter, which decision 28 ties to OS19 in
       full and to the overview and the window switcher.
   - **What 1.0 still requires of Sulphur** is what ADR-0132 requires for it to be the default
     session: the conditions of `doc_own_stack.md` decision 28, as amended, holding on the 1.0
     build. They are, in decision 28's words where it gives them:
     - the gaps of `doc_session.md` SN16 that would reach a user who signs in without choosing
       closed: the lock path (gap 1), with cosmic-greeter as the locker and cosmic-idle as the
       idle daemon under their supervisor; a polkit agent, cosmic-osd with its line; X11
       applications through xwayland-satellite (gap 6); and the key bindings of OS13 (gap 7), but
       the keys of the overview, the magnifier, the application library and the clipboard
       history that decision 28 names;
     - OS19 item 1 for every difference that hides, moves or changes a control or a text, but
       the controls decision 28 hides and the move their absence causes, and for every
       difference of colour in a high-contrast variant or in the contrast of a text; any other
       difference recorded in `athanor-differences.toml` with its cause and its issue;
     - OS19 item 2 in full, but the requirements of OS5 that come with the overview
       (`athanor_workspace_manager_v1` of decision 32), the magnifier and the colour filters
       having come with #864;
     - OS19 item 3, but the transcripts' lines of the controls decision 28 hides;
     - OS19 items 4 and 5 on one console run of at least fifteen minutes per machine, the
       laptop's keeping its suspend and resume and its output unplugged and plugged back, every
       run recording frame times that meet item 5's thresholds;
     - revision 4 of `doc_session.md`, with its hand-off to cosmic-comp's session when a session
       fails before its target is active;
     - every way of opening the overview hidden or unbound, and the Launcher buttons opening
       `athanor-launcher` under Sulphur.

     This record adds no criterion of its own.
   - cosmic-comp keeps patches 1 and 2 for as long as its session ships
     ([ADR-0115](0115-zero-nine-in-one-week.md), ADR-0103 D14).

The maintainer decided items 4 to 6 on 2026-11-05, on the draft of this record.

4. **The gaps of SN16 accepted at the switch stay accepted for 1.0.** A gap of
   `doc_session.md` SN16 that the maintainer accepted by number in the pull request that made
   Sulphur the default, as SN16 provides, stays accepted for 1.0 and closes with the full
   acceptance in 1.1.
5. **The work moved by decision 3 waits in milestone "After 1.0"**, the repository having no
   1.1 milestone. Of the open issues of milestone 1.0, only #835 (the workspaces module and
   button with `athanor-overview`) and #877 (Super+Alt+S reaching Orca under Sulphur, gap 3) move
   there. The golden differences of OS19 item 1 that decision 28 lets record and fix after the
   switch (#736, #740, #744, #918) stay in 1.0.
6. **The approved specifications follow in the pull request of this record**, whose approval
   approves their text: `doc_own_stack.md` becomes revision 3, and the notes of `doc_shell.md`
   SH2 and `doc_compositor.md` CO4 are amended.

## Alternatives rejected

- **Anticipate the steps only a person can do.** The maintainer's approvals, the hardware runs
  at the console and the key ceremony could be brought forward and batched, so that the road
  waits less on them. Offered on 2026-11-05 and not chosen.
- **Keep the limit of five open pull requests.** At about fifteen merges a day, five open pull
  requests leave agents waiting on review between merges; ADR-0112 had already recommended ten.
- **No limit,** as between ADR-0112 and the 0.9 tag. More pull requests wait on the
  maintainer's review at once, and reviewers' findings pile up across them.
- **Freeze nothing.** The milestones of 0.10 and 1.0 keep growing as the work finds issues, by
  23 over the fourteen days measured.
- **Keep the full acceptance, the overview and the window switcher in 1.0** (ADR-0132 as
  written). 1.0 keeps waiting for work that the default session already ships without, behind
  the fallback to cosmic-comp's session.
- **Sulphur as the only session of 1.0, without the full acceptance.** It removes the way back
  on hardware that the fifteen-minute runs did not cover.

## Consequences

- `AGENTS.md`, "At most ten open pull requests", applies decision 1.
- `docs/operations/release-1.0.md` becomes revision 8. Section 1, rows 12 and 13 of section 2,
  section 3, section 4, the rows of 0.9 and 1.0 in section 4.1, the bullets on rows 12 and 13 in
  section 4.2 and section 4.3 follow decision 3, and section 4.1 states the rule of decision 2.
  Row numbers keep their revision 2 value. Nothing here moves release 0.10 or 0.11: 0.10 still
  ships before Fedora 43's end of life on 2026-12-02, and decision 2 keeps in it whatever blocks
  its exit.
- The trusted path for credential prompts and the keyboard monitor that answers Orca alone in
  the default session, which ADR-0123 item 5 confirmed in 1.0, are in 1.1, where ADR-0115 had
  placed them as cosmic-comp patches 5 and 6. Those patches stay unwritten. Until 1.1 neither
  session has the trusted path's border, as `doc_session.md` SN16 gap 2 records.
- 1.0 has no overview and no window switcher on Alt+Tab under Sulphur: windows switch through
  the dock and the bar, and by keyboard through Ctrl+Alt+Tab to the dock, as decision 28 and
  ADR-0132's consequences describe. cosmic-workspaces keeps running in cosmic-comp's session.
- While cosmic-comp's session ships, the patch register of `doc_compositor.md` CO3 applies to it
  unchanged, and `doc_shell.md` SH2 and `doc_compositor.md` CO4 hold, as ADR-0121 and
  [ADR-0122](0122-own-compositor-optional-session-and-desktop-first.md) say.
- `doc_own_stack.md` becomes revision 3 (decision 6): its status line and list of changes, OS5's
  list, the lead-in of OS19, section 6 step 4, decision 28 (its opening and its paragraph on the
  overview) and the risks of section 8 say that the own compositor replaces cosmic-comp in 1.1
  and that 1.0 ships it as the default session. The notes of `doc_shell.md` SH2 and
  `doc_compositor.md` CO4, which ADR-0121 placed, say the same, and their status lines record the
  amendment. Every other sentence of those documents keeps its words.
- After this record merges, #835 and #877 move from milestone 1.0 to "After 1.0"; #736, #740,
  #744 and #918 stay in 1.0 (decision 5). Moving them is a maintainer step.
- Amended: ADR-0112 item 2 (the limit); ADR-0121 decision 5 (the acceptance 1.0 waits for) and
  its point on cosmic-workspaces; ADR-0122's consequence that release 1.0 takes cosmic-comp's
  session off the greeter; ADR-0123 item 5 (the trusted path and the keyboard monitor in 1.0);
  ADR-0132 decision 5 and its consequences (1.0 waits for the full acceptance, the overview and
  the window switcher, and retires cosmic-workspaces). ADR-0103 D13 reads as written again for
  cosmic-workspaces. Their texts keep their words; their status lines and the index name this
  record. Amended documents: `AGENTS.md`, `docs/operations/release-1.0.md` (revision 8),
  `doc_own_stack.md` (revision 3), `doc_shell.md` (SH2's note) and `doc_compositor.md` (CO4's
  note).
