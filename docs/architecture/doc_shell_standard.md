# Athanor shell standard

Status: **revision 2, approved by the maintainer on 2026-10-04.** The maintainer took its decisions in conversation on 2026-10-04: the standard comes before any new surface (ST2), the feature register is the union of every reference (ST3), the floor is a modest laptop (ST4), the thresholds of ST5, aesthetics judged by rules plus the maintainer's signature (ST8), and every measurement taken on one physical reference machine (ST9). Section 3 lists four spikes that open the plan; revision 1 replaced the container first proposed for the bench with the tooling of `scripts/devvm/`, which already does what the container was for; section 6 lists what is still open. Revision 2 follows the first run of the plan (the spikes, the bench, register version 1): `athanor-shelld` keeps the unread notifications across a crash, the maintainer's choice (ST5); the configuration the reference machine carries for the bench is listed (ST9); the bench is split between every merge and the gates (ST9); each requirement of ST7 names its check; sections 1 and 6 record what the run settled.

The document amends `doc_shell.md` (revision 5), SH1, and refers to `doc_bar.md` (revision 1), BR7 and section 5. Section 5 lists the changes.

## 1. Context

Stage 2 of `doc_shell.md` replaced cosmic-panel with our bar, our dock and `athanor-shelld` (PR #83). On 2026-10-04 the maintainer found the result below expectations, on the desktop and in daily use:

- **The dock's auto-hide is broken.** Started with `dock = "auto-hide"`, the dock disappears and does not come back when the pointer rests on the bottom edge. Switched from visible to auto-hide while running, it stays. When it tries to hide, the surface widens to the whole edge and the dock slides to its start instead of giving way to the strip. The state machine (`athanor_dock::autohide`) is correct on reading; the fault is in how the surface changes shape. The scenarios of ST6 reproduced it on the reference machine: under auto-hide the dock is not hidden and is drawn at the start of the bottom edge.
- **The bar is weak** in looks and in usefulness, measured against macOS, Windows and the better Linux shells.
- **There is no control center.** A benchmark of the same day found the bar's ten separate popovers where every major desktop offers one panel of toggles and sliders, and found that screen brightness and the power profile live in the battery module, which hides on a machine without a battery.

Running the bench found two more defects of the shipped shell:

- **An expired notification popup stayed on screen.** The soak found it; it is repaired.
- **Opening a popover of the bar while one of the dock is open, or the reverse, ends the other program.** cosmic-comp refuses the second grab (`NotTheTopmostPopup`) and raises a protocol error on the first client, which exits and is restarted by systemd (`docs/shell-bench/spikes.md`, findings outside the spikes).

The maintainer set the goal in these terms: the shell, bar, dock and everything else, is the interface of our environment, and in functionality, performance, stability, quality and aesthetics it must stand level with macOS, Windows and all of Linux.

Two gaps let the shipped state happen:

- **SH1's rule has no measure.** A surface replaces COSMIC's when it is "usable by an average user and better than COSMIC's". That compares against one competitor and names no number.
- **The tests look at pictures and configuration, not at behaviour.** The 180 surface cases of `doc_bar.md` BR9 render each scene once, still. No test moves a pointer, waits for a timer or watches a transition, so the auto-hide defect reached the maintainer's desktop.

## 2. Decisions

**ST1. Scope.** The standard covers every surface of the shell that a user sees or touches: the bar, the dock, the control center, notifications and the calendar, the launcher and the application library, the on-screen display, the session lock and the authentication dialogs, Settings, the workspace overview, and the greeter. Applications (Software, the file manager and the rest) are out of scope and will get a standard of their own. If `doc_compositor.md` is approved, window management joins this list (its CO2).

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
| Smoothness         | During every animation (opening, closing, the dock's reveal and hide, a workspace change), over at least 50 runs of each animation, at least **99%** of its frames are presented at their target refresh, and no frame is presented later than **two** refresh intervals after the previous one                                                                                                    |
| Start              | Bar and dock present their first frame within **1.5 s** of the start of the compositor's process at login                                                                                                                                                                                                                                             |
| Idle               | With no input and the screen on, the shell's processes together use less than **0.1%** of one CPU, averaged over 10 minutes                                                                                                                                                                                                                           |
| Memory             | The budgets of `doc_bar.md`, section 5, item 17, stand: `athanor-bar` 64 MB PSS, `athanor-dock` 48 MB, `athanor-shelld` 16 MB. Each new surface gets its budget in its own specification. No process grows by more than **10%** over the soak of the next row, measured from its size after the first 30 minutes, which the soak takes as warm-up                                                                                         |
| Stability          | A soak of **24 hours** on the reference machine, driven by a script that opens and closes every surface, sends bursts of notifications, switches theme, and suspends and resumes: no crash, no restart by systemd, and no journal line at priority `err` or above from a shell unit other than those listed, with a reason, in the bench's allow list |
| Recovery           | A shell process killed with `SIGKILL` presents its surface again within **1 s**, in the same state: favourites, layout, unread notifications and settings. A popover open at the time of the kill does not reopen. `athanor-shelld` is no exception: it keeps its unread notifications for this, as below                                                                                                                                    |

The numbers are reasoned proposals, not measurements: 100 ms is the classic limit under which an action reads as instantaneous, and 50 ms leaves room inside it. The first run of the bench (section 4, step 3) shows whether one is unrealistic on the floor; changing it is a revision (ST10). When this standard was approved one threshold failed by construction: the units of `athanor-bar`, `athanor-dock` and `athanor-shelld` waited `RestartSec=1s` before a restart, so recovery could not happen within 1 s; that was a defect of the units, repaired in section 4, step 3, not a reason to relax the threshold. *Amended 2026-10-06 (maintainer decision A2-20, #156):* the units now wait `RestartSec=100ms` (`athanor-bar.service:26`, `athanor-dock.service:24`, `athanor-shelld.service:24`). The restart settings of every session unit, their crash classes and the session's start order are owned by `doc_session.md` (SN4, SN6); a surface's unit takes its class's values from there, and recovery within 1 s is the requirement those values must meet.

**What `athanor-shelld` keeps across a crash.** The unread notifications survive a crash of the daemon, so that recovery holds for it as for the bar and the dock.

- At every change it writes them to `$XDG_STATE_HOME/athanor/notifications.json`, mode 0600, by a write to a temporary file and a rename: each with its id, its fields as the list shows them, its expiry, and the unique bus name of its sender.
- It removes the file when it stops cleanly, which is what a logout does, so only a crash leaves it behind.
- At start it reads the file only if it was written in the same boot (`/proc/sys/kernel/random/boot_id`), so a power loss does not carry old notifications into a new session. A file that does not parse is removed and the event logged at warning; the daemon starts empty.
- New ids continue above the highest restored one, so a client's later `CloseNotification` finds its notification. `ActionInvoked` reaches a sender that is still connected; one that has gone loses it, which the notification specification already allows.
- The read history is still lost on a crash; only the unread notifications are kept.

**Whole-session memory budget** (maintainer decision A2-20 (#158)). The per-program rows above bound each process; nothing bounded their sum. The specifications of this branch declare the following ceilings for the processes that are resident in a session, each a proposal that its own first measurement confirms or corrects:

| Process                          | Ceiling (PSS) | Declared in                                                   |
| -------------------------------- | ------------- | ------------------------------------------------------------- |
| `athanor-bar`                    | 64 MB         | `doc_bar.md`, section 5, item 17 (and the Memory row above)   |
| `athanor-dock`                   | 48 MB         | `doc_bar.md`, section 5, item 17                              |
| `athanor-shelld`                 | 16 MB         | `doc_bar.md`, section 5, item 17                              |
| `athanor-launcher`               | 80 MB         | `doc_launcher.md`, section 4, item 4                          |
| `athanor-library`                | 64 MB         | `doc_launcher.md`, section 4, item 4 (open until plan 3b)     |
| `athanor-osd`                    | 32 MB         | `doc_osd.md`, section 2 (Memory) and section 4, item 2 (OD13) |
| `athanor-overview`, hidden       | 64 MB         | `doc_overview.md`, section 4, item 4                          |
| `athanor-lock`, no surface       | 80 MB         | `doc_lock_and_prompts.md`, LP15                               |
| polkit agent                     | 64 MB         | `doc_lock_and_prompts.md`, LP15                               |
| `athanor-settings`, no window    | 40 MB         | `doc_settings.md`, SE5                                        |
| `athanor-a11y`                   | 16 MB         | `doc_accessibility.md`, section 4, item 8                     |
| `athanor-a11y-gate`              | 16 MB         | `doc_accessibility.md`, section 4, item 8                     |
| `athanor-osk` (proposal of this revision, ADR-0099; not in the sum) | not sized | `doc_accessibility.md` AX12; its budget is measured on the reference machine before the row joins the sum |
| **Sum of the declared ceilings** | **584 MB**    | 12 processes                                                  |

Two further ceilings are declared in specifications that are not yet merged into this branch, and are listed apart until they are: `athanor-control-center` 64 MB (`doc_control_center.md`, CC2 and section 4, item 5) and `athanor-clipd` 24 MB (same item), both on branch `control-center-spec`. With them the sum is 672 MB for 14 processes. **Pending** means the figures are not yet part of this document's sum; they join it when that specification merges.

The sum is an upper bound of the declared ceilings, not an expectation: processes sit below their ceilings at rest, and the shared pages that PSS divides between processes are counted once in the total. Processes that have no ceiling in any specification (the compositor and its panel, idle and portal daemons, the input method, the screen reader when it runs) add to it. The issue's "about 800 MB for about 19 processes" is the sum plus those, as an estimate.

The measurement is the sum of `Pss` over every process in the user's session slice (`user@<uid>.service` and the session scope), read from `/proc/<pid>/smaps_rollup`, taken after login on the reference VM once the session has settled, and compared with one whole-session budget. A CI job takes it: the ISO acceptance (`iso-acceptance.yml`) runs `scripts/session-memory/pss.py` in the guest once the desktop session is up, before Settings is opened, and uploads `memory.json` with the sum and a line per process. The budget lives in `scripts/session-memory/budget.json`; while that file carries no figure the job reports the sum and does not fail, and says so in its output. Once a figure is present the job fails when the sum exceeds it.

> **Decided (A2-31, #158):** the whole-session budget is measured first: its ceiling is the measured sum plus 15%, set on the reference VM. The earlier first figure of 900 MB PSS (the 672 MB of declared ceilings plus about 230 MB for the processes that declare none) is withdrawn as a ceiling. `budget.json` stays empty until the measurement exists.

**One process for the hidden GTK surfaces: evaluation** (maintainer decision A2-20 (#158)). The control center, the notification popups, the on-screen display and the overview are GTK surfaces that stay hidden most of the time, in separate processes. Merging them into one process would share one GTK and GLib baseline: the launcher measured about 16 MB of anonymous memory per GTK process (`doc_launcher.md`, section 6), so the saving is in the order of 15 MB for each process that disappears, 30 to 45 MB for three, an estimate and not a measurement.

| For a merge                                                            | Against a merge                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| One GTK baseline instead of three or four: 30 to 45 MB less (estimate) | One crash ends every surface in it; the recovery threshold (1 s, same state) then applies to all of them at once                                                                                                                                |
| One start and one D-Bus connection fewer per surface at login          | Confinement is per process: the control center has no filesystem write and no network, the overview needs the compositor's privileged protocols, the on-screen display neither; one process would carry the union of the privileges             |
| One place for the theme and the accessibility tree                     | The privileged protocols are granted per security context and per launch directory (`doc_bar.md`, BR2); a merged process widens one grant to every surface                                                                                      |
| Fewer units to order and to monitor                                    | A leak in one surface is no longer attributable by `smaps_rollup`, so the 10% growth rule of the Memory row loses its meaning for it; the cold-start races the surfaces already show are shared                                                  |

> **Proposal, awaiting the maintainer:** do not merge now. The saving is small against the whole-session budget (to be measured, A2-31), and the cost is paid in isolation, the property the zero-trust rules exist for. Revisit if the measured sum exceeds the budget, and then merge only the two surfaces with no privileged protocol and the same confinement (the on-screen display and the notification popups) before any other.

**ST6. Behaviour is specified by scenarios.**

- Every entry of the register has at least one scenario: an input made with a real pointer or keyboard, then the expected state and the expected screen, each with a time limit. The scenario names the register entry it covers.
- Scenarios live in the repository beside the bench (ST9).
- The dock's auto-hide is the first example, and the scenarios that would have caught its defect: under auto-hide, when the pointer leaves the dock, within 1.2 s only the strip is drawn; when the pointer rests on the edge, within 0.3 s the dock is drawn at its place, centred; after a context menu opens and closes and the pointer leaves, the dock hides again; switching from visible to auto-hide while running hides the dock exactly as starting in auto-hide does.

**ST7. Accessibility and languages are part of the gate.** Requirements of every surface, not extras (SH1), each with the check that verifies it:

- **Every control is reachable and operable from the keyboard alone, with a visible focus.** A scenario per surface moves the focus with Tab and the arrow keys over every control and activates each, reading the focus from AT-SPI and the focus ring from a screenshot.
- **Every control exposes a name and a role in the AT-SPI tree,** which is what Orca reads. The bench walks each surface's tree and fails a control with an empty name, a generic role, or extents outside the place where the control is drawn (the dock's extents fail this today, spike Q2).
- **No text is truncated in German; the layout mirrors under a right-to-left locale; scales 1.0, 1.25, 1.5 and 2.0 render without overlap or blur; the light, dark and high-contrast themes are complete.** The rig's surface cases (`doc_bar.md` BR9, `doc_shell.md` SH13) render scales 1.0 and 1.5, light and dark, English, German and a right-to-left pseudo-locale. At each gate the bench renders the rest on the reference machine: scales 1.25 and 2.0, and the high-contrast theme. Truncation is measured, not judged: under the timing variable of ST9 each program logs every label whose text GTK ellipsized, and an ellipsized label fails unless the surface's specification allows ellipsis there.
- **No control without a source** (SH1: no facades). The surface's specification names the source of each control, and its review checks it.

**ST8. Aesthetics: rules plus the maintainer's signature.**

- **A visual language specification,** `doc_visual_language.md`, comes before any new surface (section 4, step 4). The maintainer approves its direction once, on drafts placed beside the references.
- **It turns the direction into rules:** the spacing grid, the corner radii, the durations and curves of motion, the materials, the type scale, and contrast of at least 4.5:1 for text and 3:1 for controls and their states (WCAG 2.2 AA). The bench checks what can be measured: contrast and alignment to the grid on every screenshot, and the durations of motion from the frame timings.
- **At each gate,** the bench renders a board that places our surfaces beside the same surfaces of macOS, Windows 11 and GNOME. The maintainer reviews it and signs it; the signature is recorded with the results. Screenshots of the references are third-party works: they are kept outside the repository and the board links them locally.

**ST9. The bench runs on the reference machine.**

- **The machine** runs Athanor from the image as shipped, with no layered package. It is a test machine, not a workstation. It carries this configuration for the bench, recorded in `docs/shell-bench/machine.md`, and nothing else:
  - SSH with a key only;
  - a `sudo` rule without a password, limited to the commands the bench runs as root: the throwaway `uinput` devices, `bootc usr-overlay`, the installation of the change's packages, and the soak's suspend (`rtcwake -m no` to arm the wake alarm, then a suspend through logind);
  - automatic login of the bench's user, so that after the disk's passphrase the session starts with no second step;
  - COSMIC's screen-off and suspend set to never, and mains power.

  Any other change to the machine is a revision of this document (ST10).
- **The bench** lives in `scripts/shell-bench/`, with its scenarios, and reuses the tooling of `scripts/devvm/`, pointed at the reference machine instead of the VM: its helpers are streamed over SSH to the machine's own `python3` and run there, so nothing is installed on the machine. Input comes from a throwaway `uinput` pointer and keyboard, created as root through `sudo`, which the compositor reads like real devices through libinput (`scripts/devvm/pointer_click.py`); screenshots from `grim` against the session's compositor (`scripts/devvm/screenshot.sh`); the accessibility tree from AT-SPI, which is what Orca reads (`scripts/devvm/dock_press.py`); test notifications over the session bus. Suspend in the soak also goes through `sudo`.
- **Timing comes from inside our programs.** When an environment variable asks for it, each shell process logs the time of each input event and the presentation time of the frame that shows its effect, from GTK's frame clock, as lines of JSON. Without the variable the code path is inert. The bench sets it with `systemctl --user set-environment`, restarts the units, and unsets it when it ends. This is the only way to measure the time from a click to the screen without a camera; spike Q3 confirms that GTK4 gives a real presentation time.
- **A change not yet merged** reaches the machine as the packages its branch builds, installed on a transient overlay of `/usr` (`bootc usr-overlay`), which a reboot discards. A gate is always measured on a published image, never on an overlay. Spike Q4 confirms that packages on the overlay run under the image's integrity and SELinux policy.
- **Memory and CPU come from the kernel:** PSS from `smaps_rollup` and CPU time from `/proc/<pid>/stat`.
- **Results go into the repository,** under `docs/shell-bench/<date>-<commit>/`: the numbers as JSON, a report, and the aesthetic board with its signature (ST8).
- **When it runs.** The continuous integration gains nothing from this standard: the checks it runs today, the 180 surface cases and the memory measurement of `doc_bar.md` BR9, stay as they are. In exchange, the bench runs on the reference machine before every merge that touches a shell surface, not only before a release, so that a defect like the dock's is found before the merge rather than on a desktop. Before such a merge it runs the stages that need no one, on the change's packages over the published image: response, idle, memory, recovery and the scenarios. At a gate it runs everything, on the published image: also the start, which needs the maintainer to restart the machine, type the disk's passphrase and wait for the login, the 24-hour soak, the renders of ST7 and the aesthetic board of ST8. The maintainer chose this knowing that a defect is found later than a check on each pull request would find it.

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
3. **The measurement of today's bar and dock.** What fails becomes a list of defects, repaired first (ST2), starting with the dock's auto-hide and its scenarios (ST6), the restart delay of the three units (ST5), the notifications `athanor-shelld` keeps across a crash (ST5), and the popover that ends the other program (section 1).
4. **`doc_visual_language.md`:** drafts beside the references, the maintainer's approval, the rules of ST8.
5. **The control center:** its specification, built on the services the bar already holds, then its implementation and its gate.
6. **The other surfaces, in the order of the register:** notifications and the calendar revised, the launcher (built in PR #91, not enabled), the session lock and the authentication dialogs, the on-screen display, Settings, the overview. This keeps the order of SH1's later stages and inserts the control center before them.

## 5. Changes to other documents

- **`doc_shell.md`, SH1.** "The rule for replacing a surface: ours is usable by an average user and better than COSMIC's at the moment of the switch" becomes "ours passes `doc_shell_standard.md`". The rest of SH1 stands.
- **`doc_shell.md`, section 3.** The control center enters the stages before stage 3 continues (section 4, step 5).
- **`doc_bar.md`, BR7.** The dock's auto-hide gains the scenarios of ST6.
- **`doc_bar.md`, section 2, crashes.** "When the daemon restarts the history is lost" becomes "When the daemon restarts it restores the unread notifications (`doc_shell_standard.md`, ST5); the read history is lost".
- **`doc_bar.md`, section 5, item 17.** Its budgets are the budgets of ST5; nothing changes in it.
- **`doc_session.md`.** *Amended 2026-10-06 (maintainer decision A2-20, #156):* unit restart settings and the session's start and stop order are that document's (SN4 to SN6); this standard keeps the thresholds they must meet.

## 6. Open doubts

1. **The thresholds are not measured yet.** Section 4, step 3, gives the first numbers on the floor.
2. **The soak's allow list** starts empty. On the maintainer's desktop the bar logs, every hour, `the trust state is not trusted err=Missing`; a 30-minute soak on the reference machine logged no line at `err`, and the first 24-hour soak settles whether that line is a defect or an expected state.
3. **Where the popover defect lies.** cosmic-comp refuses a grab when the topmost popup belongs to another client and then ends that client; whether our programs should close their popover first or cosmic-comp should not end the client is settled by the repair, upstream if the fault is there.
4. **The machine's `sudo` rule is wider than ST9 allows.** It grants every command without a password; narrowing it to the list of ST9 is part of section 4, step 3.

Settled by the first run of the plan: the reference machine's graphics and panel (`docs/shell-bench/machine.md`: the unit carries the GeForce MX150, which drives no output; the panel runs at 60 Hz), and the four spikes of section 3, which passed without changing ST9 (`docs/shell-bench/spikes.md`).

## 7. Acceptance

The standard stands when:

1. the bench runs end to end on the reference machine, against today's bar and dock, and writes its first results under `docs/shell-bench/`;
2. register version 1 is approved and frozen;
3. the dock's auto-hide scenarios of ST6 pass on the reference machine after the repair;
4. the changes of section 5 are made in `doc_shell.md` and `doc_bar.md`.
