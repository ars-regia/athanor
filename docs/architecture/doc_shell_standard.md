# Athanor shell standard

Status: **revision 1, 2026-10-04, awaiting the maintainer's approval.** The maintainer took its decisions in conversation on 2026-10-04: the standard comes before any new surface (ST2), the feature register is the union of every reference (ST3), the floor is a modest laptop (ST4), the thresholds of ST5, aesthetics judged by rules plus the maintainer's signature (ST8), and every measurement taken on one physical reference machine (ST9). Section 3 lists four spikes that open the plan; revision 1 replaced the container first proposed for the bench with the tooling of `scripts/devvm/`, which already does what the container was for; section 6 lists what is still open.

The document amends `doc_shell.md` (revision 5), SH1, and refers to `doc_bar.md` (revision 1), BR7 and section 5. Section 5 lists the changes.

## 1. Context

Stage 2 of `doc_shell.md` replaced cosmic-panel with our bar, our dock and `athanor-shelld` (PR #83). On 2026-10-04 the maintainer found the result below expectations, on the desktop and in daily use:

- **The dock's auto-hide is broken.** Started with `dock = "auto-hide"`, the dock disappears and does not come back when the pointer rests on the bottom edge. Switched from visible to auto-hide while running, it stays. When it tries to hide, the surface widens to the whole edge and the dock slides to its start instead of giving way to the strip. The state machine (`athanor_dock::autohide`) is correct on reading; the fault is in how the surface changes shape, and has not been reproduced yet.
- **The bar is weak** in looks and in usefulness, measured against macOS, Windows and the better Linux shells.
- **There is no control center.** A benchmark of the same day found the bar's ten separate popovers where every major desktop offers one panel of toggles and sliders, and found that screen brightness and the power profile live in the battery module, which hides on a machine without a battery.

The maintainer set the goal in these terms: the shell, bar, dock and everything else, is the interface of our environment, and in functionality, performance, stability, quality and aesthetics it must stand level with macOS, Windows and all of Linux.

Two gaps let the shipped state happen:

- **SH1's rule has no measure.** A surface replaces COSMIC's when it is "usable by an average user and better than COSMIC's". That compares against one competitor and names no number.
- **The tests look at pictures and configuration, not at behaviour.** The 180 surface cases of `doc_bar.md` BR9 render each scene once, still. No test moves a pointer, waits for a timer or watches a transition, so the auto-hide defect reached the maintainer's desktop.

## 2. Decisions

**ST1. Scope.** The standard covers every surface of the shell that a user sees or touches: the bar, the dock, the control center, notifications and the calendar, the launcher and the application library, the on-screen display, the session lock and the authentication dialogs, Settings, the workspace overview, and the greeter. Applications (Software, the file manager and the rest) are out of scope and will get a standard of their own.

**ST2. The standard is a gate, defects first.**

- A surface is enabled by default only when it has every entry of the register that names it (ST3) and passes ST5 to ST8 on the reference machine (ST4), and the maintainer has signed its aesthetic review (ST8).
- A surface already enabled by default that fails the standard is repaired before any other shell work. Defects come first, missing features after: the bar and the dock are measured as soon as the bench works (section 4, step 3), and what they fail becomes the next work.
- A failing measurement is never relaxed inside the change that fails it. A threshold changes only by a revision of this document approved by the maintainer (ST10).

**ST3. The feature register.** A new file, `docs/architecture/shell-features.md`, lists what "level" means in functionality.

- **Sources: the union of every reference.** An entry is required when any of these offers it in a default installation: macOS (current release), Windows 11 (current release), GNOME (current release), KDE Plasma (current release), COSMIC (the release cosmic-comp comes from), DankMaterialShell, Noctalia, Caelestia and end-4's illogical-impulse. Each entry names its sources, with a link or a version.
- **Each entry names the surface it belongs to,** and the surface's gate requires every entry of its own.
- **The register is versioned.** Version 1 is written in section 4, step 2, and frozen when the maintainer approves it. A feature a reference adds later goes into the next version and does not move a gate already under way.
- **Exclusions are written decisions.** An entry is left out only by the maintainer, with the reason in the register. Expected reasons: it contradicts the zero-trust model (for example, a clipboard history any application can read), or it exists only on a compositor we do not run (end-4's and Caelestia's Hyprland-only features).

**ST4. The reference machine is a modest laptop.** The thresholds hold on the floor, so that they hold everywhere: an x86-64-v3 CPU five to six years old (an 8th-generation Intel Core or a Ryzen 2000 mobile part), integrated graphics, 8 GB of memory, a 1080p panel at 60 Hz, at scale 1.0 and 1.5. The machine is the maintainer's Xiaomi Mi Notebook Pro 15.6": an Intel Core i7-8550U (8th generation, four cores, x86-64-v3), 8 GB of memory, Intel UHD Graphics 620, a 15.6" 1080p panel. This model is commonly sold with a discrete GeForce MX150 as well; if this unit has one, the bench leaves it idle and the integrated GPU drives the panel, because the floor is integrated graphics. The machine runs the default image, not the `-nvidia` variant. The maintainer's desktop remains a second verification target and never sets a threshold (the rule that Athanor is designed for every machine, not for the maintainer's).

**ST5. Thresholds.** Every number is measured on the reference machine: on the image as shipped for a gate, and for a change not yet merged on that image with the change's packages over it (ST9). A percentile is taken over at least 50 repetitions of the action.

| Dimension          | Threshold                                                                                                                                                                                                                                                                                                                                             |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Response, in place | A control that changes in place (a toggle, a slider, a button's pressed state) shows the change on screen within **50 ms** of the input event, at the 95th percentile                                                                                                                                                                                 |
| Response, opening  | A popover, a panel or a window of the shell (the control center, the calendar, the launcher, the notification list) presents its first complete frame within **100 ms** of the input event, at the 95th percentile. The launcher updates its results within 100 ms of each key                                                                        |
| Smoothness         | During every animation (opening, closing, the dock's reveal and hide, a workspace change), at least **99%** of frames are presented at their target refresh, and no frame is presented later than **two** refresh intervals after the previous one                                                                                                    |
| Start              | Bar and dock present their first frame within **1.5 s** of the start of the compositor's process at login                                                                                                                                                                                                                                             |
| Idle               | With no input and the screen on, the shell's processes together use less than **0.1%** of one CPU, averaged over 10 minutes                                                                                                                                                                                                                           |
| Memory             | The budgets of `doc_bar.md`, section 5, item 17, stand: `athanor-bar` 64 MB PSS, `athanor-dock` 48 MB, `athanor-shelld` 16 MB. Each new surface gets its budget in its own specification. No process grows by more than **10%** over the soak of the next row                                                                                         |
| Stability          | A soak of **24 hours** on the reference machine, driven by a script that opens and closes every surface, sends bursts of notifications, switches theme, and suspends and resumes: no crash, no restart by systemd, and no journal line at priority `err` or above from a shell unit other than those listed, with a reason, in the bench's allow list |
| Recovery           | A shell process killed with `SIGKILL` presents its surface again within **1 s**, in the same state: favourites, layout, unread notifications and settings. A popover open at the time of the kill does not reopen                                                                                                                                     |

The numbers are reasoned proposals, not measurements: 100 ms is the classic limit under which an action reads as instantaneous, and 50 ms leaves room inside it. The first run of the bench (section 4, step 3) shows whether one is unrealistic on the floor; changing it is a revision (ST10). One threshold already fails by construction: the units of `athanor-bar`, `athanor-dock` and `athanor-shelld` wait `RestartSec=1s` before a restart, so recovery cannot happen within 1 s; that is a defect of the units, repaired in section 4, step 3, not a reason to relax the threshold.

**ST6. Behaviour is specified by scenarios.**

- Every entry of the register has at least one scenario: an input made with a real pointer or keyboard, then the expected state and the expected screen, each with a time limit. The scenario names the register entry it covers.
- Scenarios live in the repository beside the bench (ST9).
- The dock's auto-hide is the first example, and the scenarios that would have caught its defect: under auto-hide, when the pointer leaves the dock, within 1.2 s only the strip is drawn; when the pointer rests on the edge, within 0.3 s the dock is drawn at its place, centred; after a context menu opens and closes and the pointer leaves, the dock hides again; switching from visible to auto-hide while running hides the dock exactly as starting in auto-hide does.

**ST7. Accessibility and languages are part of the gate.** Requirements of every surface, not extras (SH1):

- Every control is reachable and operable from the keyboard alone, with a visible focus.
- Every control exposes a name and a role in the AT-SPI tree, which is what Orca reads.
- No text is truncated in German; the layout mirrors under a right-to-left locale; scales 1.0, 1.25, 1.5 and 2.0 render without overlap or blur.
- The light, dark and high-contrast themes are complete.
- No control without a source (SH1: no facades).

**ST8. Aesthetics: rules plus the maintainer's signature.**

- **A visual language specification,** `doc_visual_language.md`, comes before any new surface (section 4, step 4). The maintainer approves its direction once, on drafts placed beside the references.
- **It turns the direction into rules:** the spacing grid, the corner radii, the durations and curves of motion, the materials, the type scale, and contrast of at least 4.5:1 for text and 3:1 for controls and their states (WCAG 2.2 AA). The bench checks what can be measured: contrast and alignment to the grid on every screenshot, and the durations of motion from the frame timings.
- **At each gate,** the bench renders a board that places our surfaces beside the same surfaces of macOS, Windows 11 and GNOME. The maintainer reviews it and signs it; the signature is recorded with the results. Screenshots of the references are third-party works: they are kept outside the repository and the board links them locally.

**ST9. The bench runs on the reference machine.**

- **The machine** runs Athanor from the image as shipped, with no layered package and no persistent change for the bench. It is reached over SSH with a key only. It is a test machine, not a workstation.
- **The bench** lives in `scripts/shell-bench/`, with its scenarios, and reuses the tooling of `scripts/devvm/`, pointed at the reference machine instead of the VM: its helpers are streamed over SSH to the machine's own `python3` and run there, so nothing is installed on the machine. Input comes from a throwaway `uinput` pointer and keyboard, created as root through `sudo`, which the compositor reads like real devices through libinput (`scripts/devvm/pointer_click.py`); screenshots from `grim` against the session's compositor (`scripts/devvm/screenshot.sh`); the accessibility tree from AT-SPI, which is what Orca reads (`scripts/devvm/dock_press.py`); test notifications over the session bus. Suspend in the soak also goes through `sudo`.
- **Timing comes from inside our programs.** When an environment variable asks for it, each shell process logs the time of each input event and the presentation time of the frame that shows its effect, from GTK's frame clock, as lines of JSON. Without the variable the code path is inert. The bench sets it with `systemctl --user set-environment`, restarts the units, and unsets it when it ends. This is the only way to measure the time from a click to the screen without a camera; spike Q3 confirms that GTK4 gives a real presentation time.
- **A change not yet merged** reaches the machine as the packages its branch builds, installed on a transient overlay of `/usr` (`bootc usr-overlay`), which a reboot discards. A gate is always measured on a published image, never on an overlay. Spike Q4 confirms that packages on the overlay run under the image's integrity and SELinux policy.
- **Memory and CPU come from the kernel:** PSS from `smaps_rollup` and CPU time from `/proc/<pid>/stat`.
- **Results go into the repository,** under `docs/shell-bench/<date>-<commit>/`: the numbers as JSON, a report, and the aesthetic board with its signature (ST8).
- **When it runs.** The continuous integration gains nothing from this standard: the checks it runs today, the 180 surface cases and the memory measurement of `doc_bar.md` BR9, stay as they are. In exchange, the bench runs on the reference machine before every merge that touches a shell surface, not only before a release, so that a defect like the dock's is found before the merge rather than on a desktop. The maintainer chose this knowing that a defect is found later than a check on each pull request would find it.

**ST10. Changing the standard.** Thresholds, the reference machine, the list of references and the allow list of the soak change only by a revision of this document approved by the maintainer. The register changes by a new version (ST3).

## 3. Spikes

They are the first tasks of the plan, and each gives an answer, not code we keep. A spike that fails stops the plan: ST9 is revised and the maintainer approves the revision before the plan continues.

| #   | Spike                                                                                                                                                                                              | Settles                                        |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| Q1 | Whether `grim`, run as the session user against the session's compositor socket, captures the screen on the reference machine: it does in the VM (`scripts/devvm/screenshot.sh`) and was refused on the maintainer's desktop on 2026-10-04 ("compositor doesn't support the screen capture protocol"); if refused, which capture protocol cosmic-comp offers there | how the bench takes screenshots                |
| Q2 | Whether a throwaway `uinput` pointer and keyboard, created as root over SSH as `scripts/devvm` does in the VM, move cosmic-comp's pointer and type into a surface on the reference machine's own libinput stack, beside its touchpad and keyboard | how the bench gives input |
| Q3  | Whether GTK4's frame clock, under cosmic-comp, gives each frame a presentation time taken from the compositor's feedback rather than an estimate, and how it relates to an input event's timestamp | how the bench measures response and smoothness |
| Q4  | Whether packages of the shell built from a branch, installed with `bootc usr-overlay` on the reference machine, run under the image's integrity policy and SELinux labels, restart cleanly as user units, and are gone after a reboot | how a change not yet merged is measured |

## 4. Order of work

1. **The spikes and the skeleton of the bench:** the connection to the reference machine, input, capture, and the timing instrumentation in `athanor-bar` and `athanor-dock`.
2. **Register version 1:** research over every reference, with sources; the maintainer approves and freezes it.
3. **The measurement of today's bar and dock.** What fails becomes a list of defects, repaired first (ST2), starting with the dock's auto-hide and its scenarios (ST6), and the restart delay of the three units (ST5).
4. **`doc_visual_language.md`:** drafts beside the references, the maintainer's approval, the rules of ST8.
5. **The control center:** its specification, built on the services the bar already holds, then its implementation and its gate.
6. **The other surfaces, in the order of the register:** notifications and the calendar revised, the launcher (built in PR #91, not enabled), the session lock and the authentication dialogs, the on-screen display, Settings, the overview. This keeps the order of SH1's later stages and inserts the control center before them.

## 5. Changes to other documents

- **`doc_shell.md`, SH1.** "The rule for replacing a surface: ours is usable by an average user and better than COSMIC's at the moment of the switch" becomes "ours passes `doc_shell_standard.md`". The rest of SH1 stands.
- **`doc_shell.md`, section 3.** The control center enters the stages before stage 3 continues (section 4, step 5).
- **`doc_bar.md`, BR7.** The dock's auto-hide gains the scenarios of ST6.
- **`doc_bar.md`, section 5, item 17.** Its budgets are the budgets of ST5; nothing changes in it.

## 6. Open doubts

1. **The reference machine's graphics and panel are not verified yet.** ST4 records the model from the maintainer; whether this unit carries the GeForce MX150, and the panel's refresh rate, are read on the machine (`lspci`, the panel's EDID) when it is set up, before section 4, step 1, is accepted.
2. **The thresholds are not measured yet.** Section 4, step 3, gives the first numbers on the floor.
3. **The spikes of section 3** may change ST9.
4. **The soak's allow list** starts empty. On the maintainer's desktop the bar logs, every hour, `the trust state is not trusted err=Missing`; whether that is a defect or an expected state is settled when the bench first runs.

## 7. Acceptance

The standard stands when:

1. the bench runs end to end on the reference machine, against today's bar and dock, and writes its first results under `docs/shell-bench/`;
2. register version 1 is approved and frozen;
3. the dock's auto-hide scenarios of ST6 pass on the reference machine after the repair;
4. the changes of section 5 are made in `doc_shell.md` and `doc_bar.md`.
