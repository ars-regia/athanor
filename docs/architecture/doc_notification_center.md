# Athanor notification center

Status: **revision 1, 2026-10-04, awaiting the maintainer's review.** The maintainer took its decisions in conversation on 2026-10-04: the center is a second panel of the control center's program (NC1), `athanor-shelld` holds every notification fact (NC2), the history survives a reboot for a period the user chooses, seven days by default (NC3), automatic do not disturb by schedule, fullscreen and screen sharing (NC6), per-application settings including sound and timeout (NC5), sound on for every application by default (NC7), markup, progress and inline reply (NC9, NC10), a month calendar without events (NC11), and the classification of every register entry (section 3). It is the specification `doc_shell_standard.md`, section 4, step 6 requires for notifications and the calendar.

## 1. Context

- **What binds this document.**
  - `doc_shell_standard.md`: the gate (ST2), the register (ST3), the thresholds of ST5 (a first complete frame within 100 ms, the memory budgets, the soak, recovery within 1 s with the unread notifications kept), scenarios (ST6), accessibility and languages (ST7), the aesthetic signature (ST8).
  - `doc_bar.md`: `athanor-shelld` and its private interface admitted by unit (BR1), modules that hide when their service is absent (BR3), notifications (BR4), one popover at a time (BR6).
  - `doc_control_center.md`: the program `athanor-control-center` and its interface `os.athanor.ControlCenter1` (CC2), the models of `athanor-services` on `zbus` (CC3), the do-not-disturb state this document completes (CC7), opening and closing (CC9), Super+N kept for this surface (CC9).
  - `doc_shell.md`: GTK4, one process per surface, logic in crates with no GTK type (SH4).
- **The register** (`shell-features.md`, Notifications and calendar) has 44 entries, F-notif-01 to F-notif-44, and F-bar-08 is the bar's unread indicator. Section 3 classifies each.
- **Facts verified on 2026-10-04** in `forge/specs/athanor-shelld/athanor-shelld-1.0.0`:
  - `athanor-shelld` advertises `actions`, `body`, `icon-static` and `persistence` (`src/notifications.rs:28`), keeps at most 100 notifications in memory (`src/store.rs:9`), and admits calls on `os.athanor.Notifications1` only from `athanor-bar.service` (`src/sender.rs`, checked in `src/notifications.rs`).
  - Nothing is written to disk but the do-not-disturb switch (`src/dnd.rs`); the crash file of ST5 is specified and not built.
  - The unit sets `MemoryHigh=48M` and `MemoryMax=192M` around a budget of 16 MB PSS at rest (`data/athanor-shelld.service`, after `doc_bar.md`, section 5, item 17).
  - The bar's clock opens a `gtk4::Calendar` in a popover (`forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/clock.rs:128`).
  - `athanor-compositor-client` binds `zcosmic_toplevel_info_v1` (`system/athanor-compositor-client/src/connection.rs:746`), which reports each window's fullscreen and activated states. Spike P4 found that cosmic-comp 1.8 withholds privileged globals only from clients inside a Wayland security context; a session service on the main socket receives them all.

## 2. Decisions

**NC1. A second panel of the control center's program.** The notification center is a panel of `athanor-control-center` (CC2), beside the control center's own. The two share the process, the resident hidden window, Landlock, the opening rules of CC9 and the accessibility plumbing; at most one of the two is open.

- **Opening.** `os.athanor.ControlCenter1` gains `ToggleNotifications()`. The bar's notifications button, a click on the bar's clock and Super+N call it. Super+N is written once per user at the first start with the custom-binding writer CC9 adds for Super+C, and never again.
- **`Open`** (CC9) is true while either panel is open, so the bar keeps its popups hidden in both cases.
- **The bar loses its notification list and its calendar popover;** both now live in this panel (NC11, NC12).
- **Building the list inside the bar was rejected,** because it keeps the list under the bar's 64 MB and BR6's one popover, and **a combined column with the control center was rejected,** because it redraws the panel CC4 approved.

**NC2. `athanor-shelld` holds every fact; the panels only show.** The history, the per-application rules, the do-not-disturb state and the sound live in the daemon. The bar, the center and Settings read them and request changes through the private interface (NC8). Notifications therefore keep arriving, being filtered, sounding and being saved while a panel is closed or has crashed.

- **Rejected:** history and rules in the center's process, because the daemon needs the rules anyway to decide popup, sound and do not disturb, which would leave two copies to drift; a separate history daemon, because it adds a process, a unit and an interface for nothing the daemon cannot do.

**NC3. The history.**

- **Where.** `$XDG_STATE_HOME/athanor/notifications.json`, mode 0600, written to a temporary file and renamed. Writes are coalesced to at most one every 2 seconds, plus one when the daemon stops. "Clear all" and the clearing of a group write at once.
- **What is kept.** Every notification except those with the `transient` hint. Each entry has:
  - its id;
  - its application identity (NC4);
  - its summary and body as received;
  - its actions;
  - its urgency;
  - its time;
  - its read state;
  - the unique bus name of its sender.

  An `image-data` image is never written; after a restart the row shows the application's icon. An `image-path` is kept only when it is an icon name. A reply typed by the user (NC9) is never kept.

- **How long.** Seven days and at most 500 entries by default; the oldest leaves first. The user chooses one day, seven days, thirty days or until cleared in Settings. The limit of 500 always holds.
- **Across a reboot.** The file is read whatever the boot, which replaces ST5's same-boot rule (section 4). New ids continue above the highest restored one. An action reaches its sender only while that sender's bus name is connected. After a restart the sender's name has gone, so the row shows its actions as unavailable, and a click on the row opens the application through its desktop entry when its identity was proven (NC4).
- **Failures.**
  - A file that does not parse is renamed `notifications.json.corrupt`, replacing any earlier one, and the daemon starts empty with one journal line at warning.
  - A failed write keeps the history in memory and retries at the next write, with one journal line until a write succeeds.

**NC4. Who sent it.** `athanor-shelld` takes the sender's application identity from its cgroup, which the sender cannot forge:

- the unit or scope name defined by systemd's desktop-environment convention, `app-[<launcher>-]<application id>[@<random>].service` or `app-[<launcher>-]<application id>-<random>.scope`;
- Flatpak's `app-flatpak-<application id>-<number>.scope`.

The `desktop-entry` hint and `app_name` are declared by the sender and may lie. A notification whose identity is proven is grouped, iconed and ruled by that identity. A notification without a proven identity goes to the group "Other applications", named by its `app_name` as plain text, and follows the shared rule of that group. A rule that grants a privilege (`bypass_dnd`) never applies to it.

**NC5. Per-application rules.** One file per application, `$XDG_CONFIG_HOME/athanor/notifications.d/<application id>.conf`, plus `other.conf` for the shared group, in the `key=value` lines of `dnd.rs`'s file, so no new format.

| Key           | Values                                           | Default |
| ------------- | ------------------------------------------------ | ------- |
| `allowed`     | `true`, `false`                                  | `true`  |
| `popups`      | `true` (shown and listed), `false` (listed only) | `true`  |
| `bypass_dnd`  | `true`, `false`                                  | `false` |
| `lock_screen` | `all`, `name` (application name only), `none`    | `name`  |
| `sound`       | `true`, `false`                                  | `true`  |
| `timeout`     | seconds, or `app` (the application's own)        | `app`   |

- **Global settings** live in `$XDG_CONFIG_HOME/athanor/notifications.conf`, in the same format:
  - `retention`: NC3;
  - `sound`: the global switch;
  - `popup_corner`: one of four corners, `bar` by default;
  - `private_popups`: popups show only the application's name;
  - `timeout_low` and `timeout_normal`: by urgency, 5 s by default;
  - the schedule and triggers of NC6.
- **Malformed files.** An unknown key is ignored with a journal line at warning. A value that does not parse takes that key's default. The other keys still apply.
- **`lock_screen`** is stored and served here. The session lock's specification applies it.
- **Writers.** The daemon writes these files when an admitted client asks (NC8): the center's "Mute this application", and Settings' page once Settings has its specification. The daemon re-reads a file that changes on disk.

**NC6. Do not disturb.** The state CC7 introduced, `{ on, until, schedule }`, is completed here. Do not disturb is in effect when any of these holds, and the daemon publishes the state together with its reason:

1. **manual:** the switch, until `until` when one is set (one hour, until 08:00, CC7);
2. **schedule:** a daily window in local time, which may cross midnight, on the chosen days of the week (every day by default);
3. **fullscreen:** a window that is fullscreen and activated on any output, read through `athanor-compositor-client`;
4. **screen sharing:** a screen-capture session open in the portal backend of `doc_portal.md`.

Rules of the state:

- **Each trigger can be turned off** in Settings.
- **A trigger the session cannot observe is published as unavailable,** with one journal line, and Settings shows it so. This covers fullscreen when cosmic-comp withholds the protocol, and screen sharing until `doc_portal.md` delivers its signal. It never appears active while doing nothing.
- **A manual action wins until the next automatic change.** Turned off at 23:00 inside a 22:00–07:00 window, do not disturb stays off until 07:00. Turned off during a fullscreen video, it stays off until the video ends.
- **The end of do not disturb** shows one summary popup, "N notifications while do not disturb was on", which opens the center (F-notif-19). It never replays the missed popups.
- **The clock.**
  - The state is evaluated again when the wall clock is set: a `timerfd` with `TFD_TIMER_CANCEL_ON_SET`.
  - It is evaluated again on resume from suspend: logind's `PrepareForSleep(false)`.
  - It is evaluated again when the time zone changes: timedated's `Timezone` property.
  - So `until` and the window hold across a journey or a night with the lid closed.

**NC7. Sound.** `athanor-shelld` plays the sound; it is on for every application by default and can be turned off per application (NC5) and globally.

- **Theme names, no files.** Names resolve through the freedesktop Sound Theme specification: `message-new-instant` for normal and low urgency, `dialog-warning` for critical. A `sound-name` hint is honoured when the theme resolves it. A `sound-file` hint is ignored, so a sandboxed application cannot make the daemon open a file of its choosing. `suppress-sound` is honoured.
- **Do not disturb** silences every sound except those of critical notifications and of proven applications with `bypass_dnd`. A critical notification still respects its application's `sound=false`.
- **Playback** goes to PipeWire as the user. How it plays is spike N1 (section 5). A failed playback is logged once and never delays the notification.
- **Capability.** The daemon advertises `sound` (NC9).

**NC8. The private interface grows.** `os.athanor.Notifications1` keeps BR1's admission by unit and its unicast signals, and admits per method:

| Methods                                                                        | `athanor-bar` |     `athanor-control-center`      | Settings |
| ------------------------------------------------------------------------------ | :-----------: | :-------------------------------: | :------: |
| `List`, `Close`, `InvokeAction`, `Reply`, `MarkRead`                           |      yes      |                yes                |    no    |
| `ClearAll`, `ClearGroup`, `History`                                            |      no       |                yes                |    no    |
| `DoNotDisturb`, `SetDoNotDisturb`, `SetDoNotDisturbUntil`                      |      yes      |                yes                |   yes    |
| `Rules(app)`, `SetRule(app, key, value)`, `Settings`, `SetSetting(key, value)` |      no       | yes (`allowed` and `popups` only) |   yes    |

- **Signals.** `added`, `replaced`, `closed`, `read`, `DoNotDisturbChanged(on, reason, until)` and `RulesChanged(app)` go to each admitted unit connected.
- **Settings' unit name** is fixed by Settings' specification. Until then Settings is not admitted, and the rules change only through the center's mute and the files.

**NC9. The public interface.**

- **Capabilities.** `athanor-shelld` advertises, in addition, `body-markup`, `body-hyperlinks` (NC10), `sound` (NC7) and `inline-reply`.
- **Progress.** The integer hint `value`, from 0 to 100, draws a progress bar in the popup and the row. A notification replaced with a new `value` updates in place, without a new popup or sound.
- **Inline reply** follows KDE's extension, which applications such as messaging clients already speak:
  - the capability `inline-reply`;
  - the action key `inline-reply`;
  - the optional hint `x-kde-reply-placeholder-text`;
  - the signal `NotificationReplied(id, text)` to the sender.

  The exact names are checked against plasma-workspace in the plan (section 5, N2). The text goes to the sender and nowhere else.

- **Job notifications** with pause and cancel (KDE's `JobView`) are excluded (section 3).

**NC10. Markup, safely.** The body is still untrusted input: control and bidirectional characters are stripped and its length is bounded, as BR4 states. What changes is markup:

- **Allowed tags.** The daemon parses the body and keeps only `<b>`, `<i>`, `<u>` and `<a href>`. Every other tag and every entity it does not know becomes literal text. The panels then build the Pango markup themselves from that parsed form, escaping all text, so the sender's string never reaches Pango as markup.
- **Links.** `href` is kept only for `https:`, `http:` and `mailto:` URIs; any other link keeps its text and loses its target. A link shows its address when hovered or focused, and opens only on an explicit click or Enter, through GTK's `UriLauncher`.
- **Images** inside the body (`<img>`) are dropped.

**NC11. The panel.**

- **Placement.**
  - Anchored at the bar's end edge, by the notifications button and the clock (start edge under right-to-left text), on the focused output, under the bar or above it when the bar is at the bottom.
  - 400 logical pixels wide, at most 80% of the output's height, scrolling.
  - It closes on an outside click, Escape and loss of focus (CC9).
- **Header.**
  - The do-not-disturb switch, with its state in words ("On until 07:00 · schedule").
  - A menu with one hour, until 08:00 and edit the schedule (to Settings).
  - "Clear all".
- **The list.**
  - Groups by application identity (NC4), the most recent group first.
  - A group with more than three rows collapses behind its count.
  - The group's menu holds "Mute this application", which writes `allowed=false` after a confirmation, "Show in the list only", which writes `popups=false`, and "Notification settings" (to Settings).
- **A row** carries:
  - its icon or image, summary and relative time;
  - the body in two lines, which expands;
  - a close button;
  - up to three action buttons and "More";
  - the reply field when the sender declared `inline-reply`;
  - a progress bar when it carries `value`.

  A click on the row invokes the `default` action. On a touch screen a horizontal swipe closes the row.

- **The calendar.** Below the list, a month calendar with today marked, previous and next month, and the first day of the week from the locale. Week numbers are off until the user turns them on. It shows no events.
- **States.**
  - With no notifications the list shows "No notifications".
  - With `athanor-shelld` absent the panel shows "Notifications unavailable" instead of a stale list, and reloads when the daemon returns (CC3).
- **Read state.** A notification becomes read when its row is shown in the open panel, or when its popup is clicked or one of its actions is used. The bar's button counts the unread notifications (F-bar-08).
- **Keyboard and screen reader** (ST7).
  - Tab and the arrow keys move between groups and rows. Enter invokes the default action, Delete closes the row, Escape closes the panel.
  - On opening, the panel is announced with its unread count ("Notification center, 3 unread").
  - Each row's accessible name joins application, summary, body and time.
- **Right-to-left** text mirrors the panel, as the bar.

**NC12. What the bar keeps and gains.** The bar keeps drawing the popups (BR4).

- **Popup placement.**
  - At the corner of `popup_corner`.
  - On the overlay layer, above fullscreen windows, when the fullscreen trigger is off (F-notif-07).
- **Popup content.**
  - With `private_popups` on, a popup shows only the application's name and icon.
  - Popups gain markup, progress and a "Reply" button.
- **Reply from a popup.** The popup surface still never takes the keyboard focus (BR4), so "Reply" opens the center with that row's reply field focused.
- **Rate limit.** An application that sends more than 20 notifications in 10 seconds loses popups and sound until it slows down. Its notifications still enter the history, with one journal line.
- **The bar no longer** shows its notification list popover or its calendar popover (NC1).

**NC13. Low battery.** `athanor-shelld` watches UPower's display device through the battery model of `athanor-services` (CC3). When `WarningLevel` becomes `low` or `critical` (by UPower's configuration 20% and 5% by default), it emits its own critical notification, once per level per discharge. The notification carries the remaining time and an action that opens the power page of the control center. On a machine without a battery the model is absent and nothing is watched (BR3).

**NC14. Limits.**

- **Memory.** `athanor-shelld` stays within its budget of 16 MB PSS at rest with a full history of 500 entries. The center counts within the control center's 64 MB (CC2, `doc_control_center.md`, section 4, item 5). The first measurement on the reference machine confirms or corrects these; a miss returns to the maintainer.
- **Size bounds.** BR4's bounds on strings and images stand. The history file is at most 500 entries of bounded fields, and never carries images.

**NC15. Tests.**

- **Without a display, in `athanor-shelld`.**
  - The decision table: identity proven or not, `allowed`, `transient`, each do-not-disturb source, critical, `bypass_dnd`, sound, timeout.
  - The window across midnight and the chosen days; the manual action winning until the next change; a set clock, a resume and a time-zone change.
  - The history's write, read back after a restart, coalescing, retention by age and by count, the corrupt file, a failed write.
  - The reply never written; the markup and link filter; the rate limit; the low-battery levels once per discharge.
- **On a private `dbus-daemon`,** as the daemon's tests do today:
  - the admission table of NC8, method by method;
  - signals reaching only admitted units;
  - `NotificationReplied` and `ActionInvoked` reaching the sender;
  - the history surviving a restart of the daemon.
- **Surface cases.** SH13's matrix of 12 cases for each of four scenes: the panel with a full list, the panel empty, the panel with the daemon absent, the popups with markup, progress and reply.
- **End to end in the dev VM, run on every update of cosmic-comp:**
  - `notify-send` produces a row;
  - a fullscreen window turns do not disturb on with the reason `fullscreen`;
  - Super+N opens the panel;
  - Orca's accessible tree carries the row names of NC11.
- **On the reference laptop:** every build is installed with the bench's deploy script and judged by the maintainer. The 24-hour soak of ST5 includes bursts of notifications.

**NC16. Construction.** Each step merges on its own; the panel stays disabled by default until the last, as CC14.

1. **The daemon, with no visible change:**
   - identity (NC4);
   - history (NC3);
   - rules (NC5);
   - the do-not-disturb state with the schedule and the fullscreen trigger (NC6);
   - the interface (NC8);
   - low battery (NC13).
2. **The panel:**
   - the center replacing the bar's list and calendar popovers;
   - Super+N, the clock and the button;
   - the read state and the bar's unread count.
3. **The richer notification:** markup and links (NC10), progress and inline reply (NC9), sound (NC7, after spike N1), the popup corner, private popups and the rate limit (NC12).
4. **Screen sharing,** when `doc_portal.md` delivers its signal.
5. **The gate of the standard:** measurement, scenarios, accessibility and languages, the aesthetic signature. Only then is the panel enabled.

## 3. The register

Classified by the maintainer on 2026-10-04.

- **`have` once this document is built:**
  - 01 to 04, 06, 08, 10, 11, 13 to 15 and 25: today, unchanged or extended;
  - 05 (popup corner), 07 (above fullscreen), 09 (inline reply), 12 (clear a group), 16 (durations), 17 (schedule, fullscreen, screen sharing), 19 (summary);
  - 20 (per-application settings, with Settings), 21 (per-urgency timeout), 23 (sound), 24 (markup), 28 (mute and swipe), 29 (keyboard navigation), 30 (private popups), 31 (Super+N), 33 (month calendar, moved into the panel), 34 (week numbers), 43 (low battery);
  - F-bar-08 (the unread count).
- **`excluded`, with the reason written into the register:**

| Entry | Reason                                                                                                                                  |
| ----- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 18    | Focus modes with allowed people need a contacts source Athanor does not have; `bypass_dnd` per application covers the part that matters |
| 22    | Rules and filters on notification text cost much for little                                                                             |
| 26    | Pause and cancel need KDE's job interface, which few applications outside KDE speak; the progress bar of NC9 remains                    |
| 27    | Thumbnails and dragging files from a notification would give the panel access to the user's files                                       |
| 32    | AI summaries of notifications                                                                                                           |
| 35–38 | Calendar events, accounts and reminders need a calendar service; the maintainer chose a calendar without events                         |
| 39    | World clocks                                                                                                                            |
| 40    | Weather needs the network and the location, a service of its own                                                                        |
| 41–42 | A widget board, to-do list, timers and focus sessions                                                                                   |
| 44    | The clipboard history belongs to the control center (CC10)                                                                              |

## 4. Changes to other documents

Applied with the approval of this document.

- **`shell-features.md`:** the statuses and exclusions of section 3, with their reasons and date.
- **`doc_bar.md`:**
  - BR1: `athanor-shelld` admits per method as NC8.
  - BR3: the clock no longer opens a calendar; a click opens the center.
  - BR4:
    - the capabilities of NC9;
    - "plain text only, never markup" replaced by the filter of NC10;
    - the list moves into the center;
    - the history of NC3 replaces the list of 100;
    - do not disturb is NC6;
    - the popups follow NC12.
- **`doc_shell_standard.md`, ST5,** "What `athanor-shelld` keeps across a crash": the file is NC3's history. It is read whatever the boot, and a file that does not parse is renamed rather than removed.
- **`doc_control_center.md`:**
  - CC2: the program has two panels, and `os.athanor.ControlCenter1` gains `ToggleNotifications()`.
  - CC7: the schedule and triggers are NC6.
  - CC9: `Open` covers both panels, and Super+N is written as Super+C is.

## 5. Open doubts

1. **N1. Sound playback.** Three candidates, chosen by a spike before step 3 of NC16 against the 16 MB budget and the rule of no new long-lived process:
   - libcanberra through its PulseAudio backend on PipeWire;
   - `pw-play` from `pipewire-utils`, one short process per sound;
   - a small decoder in the daemon writing to a PipeWire stream.
2. **N2. Inline reply names.** The capability, action key, hint and signal of NC9 are taken from KDE's implementation. The plan reads plasma-workspace's source and records the exact names before step 3.
3. **N3. The screen-sharing signal** comes from `doc_portal.md`, which does not exist yet. Until it does, the trigger is published as unavailable (NC6).
4. **N4. Memory.** The budgets of NC14 hold a history five times today's list. If the measurement exceeds them, the maintainer chooses between a smaller default limit and a larger budget.
5. **N5. Application identity.** The unit and scope names of NC4 are the conventions of systemd and Flatpak. The plan confirms them on the image for applications started by the launcher, the dock, XDG autostart and Flatpak, and lists those that end in "Other applications".
6. **N6. Popups above fullscreen.** That cosmic-comp 1.8 draws a layer-shell surface of the overlay layer above a fullscreen window is checked in the dev VM before step 3. If it does not, F-notif-07 returns to the maintainer.

## 6. Acceptance

On a fresh install in the dev VM and on the reference laptop:

1. The bar's button, a click on the clock and Super+N open the center, and its first complete frame arrives within 100 ms of the input at the 95th percentile (ST5).
2. A notification received before a reboot is in the center after it, with its actions shown as unavailable. One older than the retention period is not. One sent with `transient` never is.
3. `notifications.json` has mode 0600, holds no image and no reply text, and is emptied on disk by "Clear all".
4. A process outside `athanor-bar` and `athanor-control-center` is refused by `os.athanor.Notifications1`, and the bar is refused `SetRule` and `ClearAll`.
5. A notification from a process with no application unit, carrying another application's `desktop-entry`, lands in "Other applications" and does not pass do not disturb even when that application has `bypass_dnd=true`.
6. With a schedule of 22:00–07:00, do not disturb turns on at 22:00 and off at 07:00, also across a suspend over 07:00. Turned off at 23:00, it stays off until 07:00. At its end one summary popup appears.
7. A fullscreen video turns do not disturb on with the reason "fullscreen"; ending it turns it off.
8. A body with `<b>`, `<a href="https://…">` and `<a href="file:///…">` shows bold text and one link; the `file:` link is plain text; `<img>` is dropped.
9. A reply typed in the center reaches the sender through `NotificationReplied` and is absent from the history file.
10. A sound plays for a normal notification and stays silent under do not disturb, except a critical one; `sound=false` silences the application.
11. Of twenty-one notifications sent by one application within 10 seconds, the twenty-first shows no popup and plays no sound, and the history holds all twenty-one.
12. On the reference laptop, with the battery falling past UPower's low level, one critical notification appears once.
13. With `athanor-shelld` killed, the center shows "Notifications unavailable", and within 1 s of the daemon's return it shows the same list and unread count (ST5).
14. The four surface scenes pass SH13's 48 cases, and the surface passes the gate of `doc_shell_standard.md` (ST2) before the panel is enabled.
