# Athanor compositor

Status: **revision 1, 2026-10-04, awaiting the maintainer's approval.**

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
  - the trusted path for credential prompts, our own patch with no upstream proposal by the maintainer's decision (`doc_lock_and_prompts.md` D10, LP13);
  - a third for the overview, a neutral layer namespace on the overlay or top layer, only if spike S3 of `doc_overview.md` fails (decision 1).
- Each cosmic-comp release is rebased in its own pull request. A patch that upstream merged is
  dropped in that pull request.
- Security policy (which client gets which privileged protocol, a trusted path for credential
  prompts) is the first reason for a patch, as SH2 states. Window management comes second, and
  only for an approved register entry.

**CO4. One more condition for a fork.** To the conditions of SH2 (cosmic-comp abandoned or
relicensed, a security requirement declined and the patch set no longer maintainable, more
maintainers) this document adds: **a window-management model the maintainer approves in the
register, which cosmic-comp cannot host and upstream declines.** A fork starts from cosmic-comp
(or from niri, both on Smithay), never from zero.

## 3. Open questions

- **Rebase cost.** No cosmic-comp release has yet been rebased with the patch set in place: the
  first one measures the cost CO3 assumes.
- **Upstream status of the two patches.** Neither changelog line names an upstream link yet (CO3
  requires one or a reason).
