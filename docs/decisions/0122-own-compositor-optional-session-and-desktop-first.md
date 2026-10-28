---
id: ADR-0122
title: "The own compositor ships as an optional session before 1.0, and takes the desktop first"
date: 2026-10-17
status: accepted
issues: []
areas: [shell, security, process]
---

# 0122. The own compositor ships as an optional session before 1.0, and takes the desktop first

## Context

ADR-0121 set the direction: a compositor of Athanor's own on Smithay, which replaces cosmic-comp in
release 1.0 once it passes the acceptance of `doc_own_stack.md` revision 2 (decision 5). Until
then it stays out of the image, and on the desktop its builds run only while the rig of release
0.9 is idle (decision 1).

On 2026-10-17, after `doc_own_stack.md` revision 1 was approved, the maintainer asked for the
compositor to reach their machines in the least time, with no target date. Two things stood in the
way: the compositor reached daily use only at 1.0, so its defects would surface late; and its
milestone, which needs the desktop's NVIDIA card, waited for the rig of release 0.9.

## Decision

The maintainer took these decisions on 2026-10-17 (`doc_own_stack.md` decisions 10 and 11):

1. **An optional session before the acceptance.** Once milestone C1's DRM backend has passed the
   console sessions (C1 plan, task 9), the image ships the compositor as a session the greeter
   offers beside cosmic-comp's, which stays the default until the acceptance (amended on
   2026-10-28: see Amendment). It ships only when:
   - privileged globals are granted by executable and unit and the check of C1 task 5 passes
     (`doc_own_stack.md` OS4), so that no confined client reaches layer shell, capture, input
     injection or the session lock;
   - its unit passes `verify.py services`, and the crate the panic budget and the persistence
     rules of `doc_threat_model.md` TM3;
   - the requirements of OS4 that milestone C1 does not cover, the trusted path for credential
     prompts (`doc_lock_and_prompts.md` LP13) and the lock path, are listed in the change that ships
     it, and the maintainer accepts them in that change;
   - the greeter's session choice, which is its login path, has an approved revision of
     `doc_session.md` behind it (ADR-0074) and the maintainer's go-ahead before its code is written
     (AGENTS.md, "Stop and ask before editing").
2. **The compositor takes the desktop first.** Milestone C1 runs on the desktop before release
   0.9's rig work, which moves to the reference laptop between the toolkit's runs. Release 0.9
   may land some days after 2026-10-21.

The alternatives weighed were the compositor outside the image until the acceptance, and its
builds only while 0.9's rig is idle; both put the compositor in daily use later and were not taken.

## Consequences

- This record amends ADR-0121 decision 1 in the rule for the desktop, and decision 5 in one point:
  the compositor is in the image as an optional session before release 1.0. Release 1.0 still
  waits for its acceptance. After the amendment the compositor becomes the default session
  before then, and release 1.0 takes cosmic-comp's session off the greeter.
- While cosmic-comp is the default, the patch register of `doc_compositor.md` CO3 applies to it
  unchanged, and `doc_shell.md` SH2 and `doc_compositor.md` CO4 hold as ADR-0121 says.
- The C1 plan gains task 9a, the optional session, with the conditions of decision 1.
- A defect of the optional session reaches only the machines whose user picks it at the greeter;
  the default session is unaffected. After the amendment, a defect of the own compositor
  reaches every machine whose user does not choose cosmic-comp's session.

## Amendment

On 2026-10-28 the maintainer amended decision 1 through `doc_own_stack.md` decision 28 (#810): the
own compositor becomes the default session before its acceptance holds in full, once the gaps of
`doc_session.md` SN16 that would reach the default session are closed and on the parts of OS19
that decision names. cosmic-comp's session stays at the greeter as the second choice until the
acceptance holds in full, and a defect of the own compositor then reaches every machine whose
user does not choose cosmic-comp's session.
