# Athanor compositor

Status: **revision 2, approved by the maintainer on 2026-10-08.** This revision settles the trusted-path patch on cosmic-comp PR #1441 (ADR-0041), adds the upstream policy to CO3, completes the list of patches to match the register, records the upstream plan for patches 1 and 2, and accepts the rebase cost as measured by the first rebase.

This document answers one question the maintainer raised on 2026-10-04: should Athanor have a
compositor of its own, one that takes in what niri, Hyprland, MangoWC and the others offer? The
base decision already exists and is not restated here: `doc_shell.md`, **SH2** (cosmic-comp stays,
we do not write a compositor, the dependency has an exit) and **SH3** (of COSMIC only cosmic-comp
stays). This document adds what SH2 leaves open: how window-management features enter, how the
patch set is kept, and one more condition for a fork.

## 1. Context

- **The cost of a compositor is the hardware matrix, not the first version** (SH2): outputs plugged
  and unplugged, fractional scale, VRR and HDR, input methods, tablets and touch, XWayland, the
  NVIDIA driver, accessibility, and the protocols every client expects. The defects met so far sit
  there: cosmic-comp dropping a client that destroys a mapped layer surface, the Vulkan loader race
  of cosmic-workspaces on NVIDIA.
- **Athanor has already changed compositor once.** The session ran on niri until 2026-09-09, with
  `ermete-niri` and `athanor-niri-ipc`; the greeter and the session moved to cosmic-comp that week.
- **The compositor is a security boundary.** It decides which client may capture the screen, inject
  input, read the clipboard or use a privileged protocol. Spike P4 (`doc_shell.md`, section 3)
  found that cosmic-comp 1.8 grants its privileged globals by an unauthenticated engine string,
  `com.system76.CosmicPanel`.
- **The forge already builds cosmic-comp** (`forge/specs/cosmic-comp`): 1.8.0 from the upstream
  archive, with two patches: focus reconciled when a layer surface changes, and the commit hash
  taken from `GIT_HASH`.

## 2. Decisions

**CO1. No compositor that unites the others.** niri (a scrolling column layout), Hyprland (dynamic
tiling, animations, plugins) and MangoWC (dwl's tags) are three models of window management, not
three lists of features. A compositor offering all of them carries the complexity of each and the
maintenance of all three, and gives the user a choice to make before the desktop is usable.
Athanor is for everyone: it ships good defaults, not every model.

**CO2. Window-management features enter through the register, one entry at a time.** Tiling,
scrolling layouts, window rules, gestures and the like are judged like any other feature of the
shell: an entry in `shell-features.md` with its sources, approved or excluded by the maintainer
(`doc_shell_standard.md`, ST3). Version 1 of the register has no surface for them; version 2
proposes an eleventh surface, **Window management**. An approved entry is met, in order of
preference, by cosmic-comp as it is, by its configuration through `athanor-compositor-client`
(SH2), by a patch proposed upstream, or by a patch of our own (CO3).

**CO3. The patch set stays small, and each patch has an exit.**

- A patch lives in `forge/specs/cosmic-comp/SOURCES`, in `git format-patch` form, and the
  changelog of the spec names why it exists.
- Each patch that changes behaviour is proposed upstream first. Its changelog line carries the
  link to the upstream pull request or issue, or says why it is not proposed.
- The patches the specifications of 2026-10-05 add, each proposed upstream first unless said otherwise:
  - the three-finger swipe that opens the overview (`doc_overview.md` decision 4);
  - a reduced-motion switch for the workspace slide, mirrored from `enable-animations`, which only turns the slide on and off and leaves its 200 ms duration alone (`doc_overview.md` decision 5; `doc_accessibility.md` AX9);
  - the trusted path for credential prompts, built on cosmic-comp PR #1441 (ADR-0041, `doc_lock_and_prompts.md` D10, LP13); see "The trusted path and PR #1441" below;
  - the keyboard monitor that answers Orca alone, bound to a cgroup (`doc_accessibility.md` AX4, patch 6 of the register);
  - sticky, slow and bounce keys in the input path (`doc_accessibility.md` AX11, patch 7), proposed upstream before any code lands in the package;
  - a third for the overview, a neutral layer namespace on the overlay or top layer, only if spike S3 of `doc_overview.md` fails (decision 1).
- Each cosmic-comp release is rebased in its own pull request. A patch that upstream merged is
  dropped in that pull request.
- **The trusted path and PR #1441** (maintainer decision of 2026-10-08, ADR-0041). The pull request is a third party's, open and unreviewed upstream since May 2025, so the trusted path does not wait for it. We carry its code as our own patch (register, row 5) in the cosmic-comp package. The package build fails if the patch stops applying to an update, so the cost shows at every update of cosmic-comp instead of being hidden. In parallel we take part in the upstream discussion of #1441 and propose the reveal key there. When #1441 or an equivalent is merged upstream, the patch is dropped in the rebase pull request.
- **Upstream first, never dependent on upstream.** Every component of ours works if upstream never accepts what we send. A small fix goes upstream as soon as it exists; a large feature starts with an upstream issue or discussion before the code. A patch is carried in the package from the day it is needed, whatever upstream answers.
- Security policy (which client gets which privileged protocol, a trusted path for credential
  prompts) is the first reason for a patch, as SH2 states. Window management comes second, and
  only for an approved register entry.

- **The register of patches** (maintainer decision A2-20 (#158)). Every patch the specifications add or carry, with its owner. "Not yet proposed" means no upstream link exists; sizes are estimates, except for the two carried patches, which are measured on the files. A patch whose exit condition is met is dropped in the rebase pull request that finds it.

  | #   | Patch                                                                 | Owner (spec, requirement)                                                  | Upstream                                                                           | Size (lines)  | Exit condition                                                                                       |
  | --- | --------------------------------------------------------------------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ------------- | ---------------------------------------------------------------------------------------------------- |
  | 1   | `0001`, layer-surface keyboard focus follows `keyboard_interactivity` | `doc_launcher.md`, LA8; `doc_overview.md`; `doc_accessibility.md`          | Proposed upstream before the next rebase; the changelog then carries the link      | 43 (measured) | Upstream reconciles focus when a layer surface changes its interactivity                             |
  | 2   | `0002`, `build.rs` honours `GIT_HASH`                                 | `forge/specs/cosmic-comp` changelog                                        | Not proposed: specific to our build from an archive; the changelog says so         | 6 (measured)  | Upstream builds from a source archive without git                                                    |
  | 3   | Vertical three-finger swipe opens and closes the overview             | `doc_overview.md`, decision 4 (OV13)                                       | Not yet proposed; proposed first (CO3)                                             | 40 to 80      | Upstream implements the TODO at `src/input/mod.rs:1195`                                              |
  | 4   | Reduced-motion switch for the workspace slide                         | `doc_overview.md`, decision 5 (OV13); `doc_accessibility.md`, AX9          | Not yet proposed; proposed first (CO3)                                             | 30 to 60      | Upstream adds a reduced-motion setting                                                               |
  | 5   | Trusted path for credential prompts                                   | `doc_lock_and_prompts.md`, LP13 (D10, ADR-0041)                            | Based on cosmic-comp PR #1441 (third party, open since May 2025); reveal key proposed upstream | 300 to 600; to be reviewed against the code of #1441 | #1441 (or an equivalent) merged upstream             |
  | 6   | The keyboard monitor answers Orca alone (cgroup check)                | `doc_accessibility.md`, AX4 (decision 6)                                   | Not yet proposed; cosmic-comp PR #2763 (configurable allow-lists) is where it fits | 60 to 120     | Upstream merges allow-lists that can bind the name to a cgroup                                       |
  | 7   | Sticky, slow and bounce keys in the input path                        | `doc_accessibility.md`, AX11 (decision 4), after spike A3                  | Not yet proposed; **proposed upstream first**, before any code lands in the package | 300 to 600    | Upstream ships keyboard aids; the Settings rows (`doc_settings.md`) appear then                      |
  | 8   | Conditional: drop the input method's grab while the session is locked | `doc_languages.md`, LN12, only if spike S2 shows the defect on 1.8.0       | cosmic-comp#2702 (the defect, open since 2026-08-06); patch not yet proposed       | 40 to 80      | Upstream fixes cosmic-comp#2702                                                                      |
  | 9   | Conditional: count the special layer namespace only on overlay or top | `doc_overview.md`, decision 1, only if spike S3 fails                      | Not yet proposed; proposed first (CO3)                                             | 20 to 40      | The overview meets ST5 smoothness with its neutral namespace, or upstream changes the namespace rule |
  | 10  | Conditional: no capture of window content while the session is locked | `doc_lock_and_prompts.md`, spike L1 (as LP13), only if L1 shows the defect | Not yet proposed; a defect report first                                            | 20 to 60      | Upstream withholds window content from capture while locked                                          |

  Of the ten, two are carried today, five are firm and three depend on a spike (8, 9, 10); the issue's "about nine" counts the new ones. The sizes are budgets for review, not measurements; a patch above its estimate is a reason to reopen its decision. A second budget is the rebase cost: a scheduled job tries the set on the next cosmic-comp tag (`scripts/cosmic-comp-rebase/`, workflow `cosmic-comp-rebase.yml`) and reports which patches apply. The firm and carried patches add up to roughly 800 to 1500 lines in the input and security paths of the compositor, more than "small" suggests; that is why each rebase is its own pull request and why a patch above its estimate reopens its decision. The line numbers and versions cited here are read once and age at every rebase: they are estimates.
- **Order for AX11.** The keyboard aids are proposed upstream first: the proposal (patch 7) is filed and its answer awaited before the patch is merged into `forge/specs/cosmic-comp`. This records the order only; nothing has been filed.

**CO4. One more condition for a fork.** To the conditions of SH2 (cosmic-comp abandoned or
relicensed, a security requirement declined and the patch set no longer maintainable, more
maintainers) this document adds: **a window-management model the maintainer approves in the
register, which cosmic-comp cannot host and upstream declines.** A fork starts from cosmic-comp
(or from niri, both on Smithay), never from zero.

## 3. Open questions

- **Rebase cost.** No cosmic-comp release has yet been rebased with the patch set in place. The maintainer approved CO3 on 2026-10-08 before the measurement: the first rebase measures the cost and reopens CO3 if it exceeds the budget.
- **Upstream status of the two patches** (settled 2026-10-08). Patch 1 is proposed upstream before the next rebase, as it is a real focus defect, and its changelog line then carries the link. Patch 2 is not proposed, being specific to our build from an archive, and its changelog line says why, so CO3 holds to the letter.
