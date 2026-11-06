---
id: ADR-0138
title: "Athanor's own polkit agent, lock, window switcher, session daemons and output management are placed in 1.0, written in Sal"
date: 2026-11-06
status: accepted
issues: []
areas: [shell, security, process, docs]
---

# 0138. Athanor's own polkit agent, lock, window switcher, session daemons and output management are placed in 1.0, written in Sal

**Accepted** by the maintainer on 2026-11-06, in conversation, as explicit placements under
[ADR-0135](0135-faster-road-to-1-0.md) decision 2 (c).

## Context

[ADR-0103](0103-audit-5-decisions.md) D13 shipped the session lock and the polkit agent in 1.0 as
Fedora components, and kept cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings until 1.1.
`docs/operations/release-1.0.md` row 12 follows it, and its section 3 lists Athanor's own lock, agent
and SystemPrompter (`doc_lock_and_prompts.md` LP18), the window switcher on Alt+Tab (moved to 1.1 by
ADR-0135 decision 3) and the retirement of cosmic-idle, cosmic-bg and cosmic-settings as "after 1.0".
`doc_launcher.md` LA12 ([ADR-0061](0061-desktop-minor-decisions.md), A2-24) keeps cosmic-launcher,
cosmic-app-library and pop-launcher in the image because cosmic-launcher also serves Alt+Tab.
[ADR-0124](0124-visual-language-revision-3.md) already built Athanor's own toolkit, Sal, and froze
the GTK shell: it "gets nothing new until it is removed".

On 2026-11-06 the maintainer said: "dobbiamo sostituire tutto quello che non è proprietario",
"dobbiamo andare in stato proprietario" and "stiamo abbandonando gtk, va scritto in sal".

## Decision

Decided by the maintainer on 2026-11-06. Each is a placement under ADR-0135 decision 2 (c).

1. **Athanor's own polkit agent and session lock are in 1.0** (`doc_lock_and_prompts.md` LP18). They
   replace the Fedora components that row 12 planned, with no Fedora interim. The keyring prompter
   exchange stays deferred to the Fedora 45 base (`doc_lock_and_prompts.md` D15,
   [ADR-0085](0085-keyring-prompter-secret-exchange-source.md), A2-31 / [ADR-0068](0068-session-budget-and-prompter-timing.md)).
2. **Athanor's own window switcher on Alt+Tab is in 1.0** (`doc_overview.md`). Then one change makes
   `athanor-launcher` the default and removes cosmic-launcher and pop-launcher, as LA12, A2-24 and
   ADR-0061 require. The switcher is a Sal client, `athanor-switcher`, that serves Sulphur's session and
   cosmic-comp's; in 1.0 it shows every window of every workspace, by icon and title, on Alt+Tab and
   Alt+Shift+Tab. Athanor's own application library, `athanor-library` (`doc_launcher.md` LA1, LA7), is
   in 1.0 too, and cosmic-app-library leaves in the same change as cosmic-launcher and pop-launcher.
   The launcher itself, `athanor-launcher`, today a GTK4 program, is rewritten in Sal in 1.0, chosen
   by the maintainer on 2026-11-06 over keeping it in GTK4 until 1.1; the key that opens it when
   sticky keys are on is decided later (`doc_launcher.md`).
   The overview, `athanor-overview`, and cosmic-workspaces' retirement stay in 1.1 (ADR-0135).
3. **`athanor-idle` and `athanor-wallpaper` (`doc_session_daemons.md`) are in 1.0, and so is the
   replacement of every role of cosmic-settings-daemon.** This brings forward their owners per SD2:
   `athanor-osd` (`doc_osd.md`), the accessibility service (`doc_accessibility.md`, `athanor-a11y`) and
   the session service (`athanor-sessiond`), and also the keyboard layout switch and xkb seed of
   `athanor-shelld` (`doc_languages.md` LN9) and the theme service `athanor-style`
   (`doc_visual_language.md` VL4), placed by the maintainer on 2026-11-06 because without them
   Super+Space, the layout seed and the theme would regress when the daemon leaves. External-monitor
   brightness over DDC/CI, which cosmic-settings-daemon 1.11 sets when no backlight exists, moves to
   `athanor-osd` through the brightness model of `athanor-services` (`doc_session_daemons.md`).
   cosmic-idle, cosmic-bg and cosmic-settings-daemon leave
   in 1.0. This amends ADR-0103 D13 for those three. cosmic-settings and cosmic-workspaces are not
   placed by this record.
4. **Sulphur's wlr-output-management is completed in 1.0:** modes, position, enable and adaptive
   sync. Today only scale and transform apply.
5. **LP18 step 4 and step 10 are in 1.0 with the lock:** the greeter moves onto `athanor-auth`, and the
   lock-aware `athanor-usbguard-policy` replaces `athanor-usbguard-hook`. They were in 1.1 only because the
   lock was. Decided by the maintainer on 2026-11-06 by the option "Both in 1.0".
6. **New surfaces are written in Sal, not GTK4:** the lock, the agent's dialog, the switcher, the
   launcher, the application library, the OSD and the wallpaper. Daemons with no surface, such as `athanor-idle`, use wayland-client directly.

## Alternatives rejected

- **Keep the Fedora components in 1.0** (ADR-0103 D13, ADR-0135 as written). The image would ship
  1.0 with COSMIC programs that the maintainer's direction is to replace.
- **Write the new surfaces in GTK4.** The GTK shell is frozen (ADR-0124) and is being abandoned.

## Consequences

- 1.0 gets later. The six items are new work on the path to 1.0 and no date is promised.
- The limit of ten open pull requests of ADR-0135 decision 1 still applies, as does the scope freeze
  for anything not placed here.
- Authentication work stays stop-and-ask: the agent and the lock are authentication paths, and each
  is implemented only after the maintainer approves the implementation plan (`doc_lock_and_prompts.md`
  LP8; `AGENTS.md`, "Stop and ask before editing").
- Each step is judged on the reference laptop, as the specifications say.
- The specifications that name GTK for these surfaces get their own revisions before the code, under
  ADR-0074. This record rewrites none of them:
  - `doc_lock_and_prompts.md` LP2, LP14, LP15 and LP17 (the lock and the agent);
  - `doc_osd.md` (the OSD);
  - `doc_session_daemons.md` SD6: the wallpaper service is specified as a headless `wl_shm` client,
    and the decision to write it in Sal changes that (SD1 names no GTK and is not in the list);
  - `doc_overview.md`, which specifies no window switcher yet: its GTK4 and gtk4-layer-shell lines
    (about lines 92, 94, 119, 270 and 386) concern the overview, which stays in 1.1, and the switcher
    is specified there in Sal.
  - `doc_launcher.md`, which binds the launcher and the application library to GTK4 (SH4, about
    lines 7, 24 and 26); `athanor-launcher` and `athanor-library` are written in Sal.
- The specifications whose placements this record changes, and which their next revisions must amend:
  - `doc_own_stack.md`: the status line (line 3) and the 1.1 placement of the window switcher (about
    lines 423, 666, 778, 917 to 920, 940 and 1205) and of the application library after the switch
    (about lines 931 to 935); decision 28 and gaps 1 and 2, which name
    cosmic-greeter as the locker, cosmic-idle as the idle daemon and cosmic-osd as the polkit agent
    as conditions of the 1.0 switch (about lines 310 to 316, 780 to 798, 905 and 960). Lines 63, 656
    and 686, and the sentences that open at 420 and 656 up to their switcher clauses at 423 and 666,
    concern the compositor's switch and cosmic-workspaces, which stay in 1.1, and do not conflict;
  - `doc_session.md` SN15 and SN16 (about lines 176 to 216), which name cosmic-greeter's locker
    scope;
  - `doc_lock_and_prompts.md` about line 290 ("LP18 step 4 for 1.1").
- The 1.0 exit column of `release-1.0.md` section 4.1 inherits the conditions of decision 28, which
  change for the lock, the idle daemon and the agent when `doc_own_stack.md` is amended.
- `docs/operations/release-1.0.md` is amended in this pull request: row 12, section 1, section 3 and
  the notes of section 4.2 on rows 9, 12, 13 and 19, and the greeter row of section 2.
- Amended: ADR-0103 D13 (for cosmic-idle, cosmic-bg, cosmic-settings-daemon and the lock and agent),
  ADR-0135 decision 3 (the window switcher). Their texts keep their words; their status lines and the
  index name this record.
