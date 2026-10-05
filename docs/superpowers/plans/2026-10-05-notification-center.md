# Notification center — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the notification center of `doc_notification_center.md`: every notification fact in `athanor-shelld`, a second panel in `athanor-control-center`, the richer notification, and the standard's gate, in the construction steps 1, 2, 3 and 5 of NC16.

**Architecture:** `athanor-shelld` gains pure modules — identity, rules, history, do not disturb, policy, markup, low battery, rate limit — each tested without a bus, wired into its two D-Bus interfaces and tested again on a private `dbus-daemon`. The wire type moves into `athanor-services`, where a toolkit-free model feeds the GTK panel of `athanor-control-center`. The bar keeps the popups, loses its list and calendar popovers, and reports the fullscreen state it already sees through `athanor-compositor-client`.

**Tech Stack:** Rust 2021; workspace `zbus 5.18` (tokio), `tokio 1.53`, `serde`/`serde_json`, `nix 0.29` (features `time`, `inotify`), `libc`, `gtk4 0.11` (`v4_18`), `gtk4-layer-shell 0.8`; Python e2e under `forge/test/shell/`.

**Spec:** `docs/architecture/doc_notification_center.md` (revision 1, approved 2026-10-04). Also binding: `doc_bar.md` (BR1, BR3, BR4), `doc_control_center.md` (CC2, CC3, CC7, CC9), `doc_shell_standard.md` (ST2, ST5, ST7, ST8), `doc_shell.md` (SH4, SH13). Built on the control-center plan, `docs/superpowers/plans/2026-10-04-control-center.md` (branch `control-center-spec`).

## Spec amendments this plan proposes

The code read on 2026-10-05 contradicts three details of the approved spec. Each is applied to `doc_notification_center.md` in Task 0, only after the maintainer approves this plan.

1. **Paths, for Landlock.** `athanor-shelld` writes beneath one state directory today (`sandbox::restrict(read, write)`, `system/athanor-unit/src/sandbox.rs:31`), `$XDG_STATE_HOME/athanor/shelld`. Writing `$XDG_STATE_HOME/athanor/notifications.json` by a rename, and `$XDG_CONFIG_HOME/athanor/notifications.conf`, would need write access to the whole `athanor` directories, which hold the other components' state and configuration. The plan uses:
   - history: `$XDG_STATE_HOME/athanor/shelld/notifications.json`;
   - settings: `$XDG_CONFIG_HOME/athanor/notifications/notifications.conf`;
   - rules: `$XDG_CONFIG_HOME/athanor/notifications/apps/<application id>.conf`, and `other.conf` beside `notifications.conf` for applications without a proven identity, so that an application whose id is `other` cannot take their place.
2. **The fullscreen trigger is observed by the bar.** `athanor-compositor-client` connects through a `gdk::Display` (`src/connection.rs:135`), so linking it would bring GTK into the headless daemon (SH4) and give it a Wayland connection it does not have today. The bar already holds that client: it reports `ReportFullscreen(b available, b active)` to the daemon, a method of NC8 admitted for `athanor-bar` only. While no bar is connected the trigger is unavailable, as NC6 already says for a trigger the session cannot observe.
3. **Opening the center on a row.** The summary popup (NC6) and the popup's "Reply" (NC12) open the center, which `ToggleNotifications()` cannot do without closing an open one. They use CC2's `Show(page)` with the page ids `notifications` and `notifications:<id>`, the second focusing that row's reply field. No new method.

Two readings of the spec, stated so the reviewer can check them:

- **Manual do not disturb.** A manual "on" (with or without `until`) holds until `until` or until the switch is turned off; automatic changes do not end it. A manual "off" while an automatic source is active holds until the set of active automatic sources changes. This is NC6's "a manual action wins until the next automatic change" applied to "off", the only case where it decides anything; applied to "on", a fullscreen video ending would cancel "one hour".
- **`List` and `History`.** `List` returns the unread notifications (the bar's popups and its count); `History` returns all of them.

## Global Constraints

- English in code, comments, commit messages and documents; enterprise tone; no attribution to any assistant anywhere.
- `athanor-shelld` and `athanor-services` depend on no `glib`, `gio` or `gtk4` crate (SH4): `cargo tree -p athanor-shelld -e normal --prefix none | grep -cE '^(glib|gio|gtk4) v'` prints `0`.
- `panic = "abort"` holds in every profile: a malformed file, reply or hint is a default, an absent field or a refusal, never a panic.
- Every string from a sender is untrusted (BR4): truncated by `athanor_unit::text`, control and bidirectional characters stripped; the body keeps only NC10's markup, never reaching Pango as the sender wrote it.
- History file mode 0600, written to a temporary file and renamed; never an image, never a reply text (NC3).
- `os.athanor.Notifications1` admits per method by the caller's unit, read from the bus credentials and `/proc/<pid>/cgroup` (NC8). No method is admitted for a caller outside `athanor-bar.service` and `athanor-control-center.service`.
- `bypass_dnd` never applies to a notification without a proven identity (NC4).
- Memory: `athanor-shelld` 16 MB PSS at rest with 500 entries; the control center, both panels, 64 MB (NC14). A miss returns to the maintainer (N4).
- No `|| true`, no `continue-on-error`, no placeholder in a security path, no `chmod 777`. No change to polkit, the Gatekeeper or attestation.
- The panel stays disabled by preset until Task 18 passes the gate (NC16).
- Build with `-j 4` and `CARGO_TARGET_DIR=/var/tmp/athanor-target-cc`; at most two agents at once; never a build in `/tmp`.
- Every build that changes what the maintainer sees is installed on the reference laptop with `python3 scripts/shell-bench/deploy.py --host athanor-ref <crates>` and judged by the maintainer; never while `soak.py` or `bench.py` runs.
- Commits: `type(scope): imperative summary`, as `git log -10` shows.

## Review Focus

1. **`SetRule` and `Rules` with an application id such as `../../.bashrc` or `a/b`:** refused with `InvalidArgs`, no file created outside `apps/`. Test in Task 2.
2. **A disk that refuses the write (read-only, full):** the history stays in memory, `Notify` answers at once, one warning line until a write succeeds, the next write retries. Test in Task 3.
3. **The wall clock set backwards inside a schedule window (NTP correction, manual set):** the state is evaluated again, no second summary popup, no do not disturb stuck on. Test in Task 4.
4. **A sender that disconnects between `Notify` and the identity lookup (its pid gone):** the notification is kept under "Other applications", never refused, never a panic. Test in Task 1.
5. **A history file from another version (an unknown field, a missing optional field):** read with defaults, not renamed `.corrupt`. Test in Task 3.

---

## Order, branches and parallelism

The implementation branch is `notification-center`, from `control-center` at the commit that holds the control-center plan's Task 1 (`athanor-services`). Each step of NC16 ends in its own pull request to `iso-v0`, opened only with the maintainer's consent.

| Task                                         | Depends on             | Group |
| -------------------------------------------- | ---------------------- | ----- |
| 0 Spec amendments                            | approval of this plan  | A     |
| 1 Identity                                   | —                      | A     |
| 2 Rules and settings                         | —                      | A     |
| 3 History                                    | 1, 2                   | B     |
| 4 Do not disturb                             | 2                      | B     |
| 5 Policy, interface and wire                 | 1–4, CC plan Task 1    | C     |
| 6 Fullscreen reported by the bar             | 5                      | D     |
| 7 Low battery                                | 5, CC plan Task 2      | D     |
| 8 Step 1 on the laptop                       | 5–7                    | E     |
| 9 Two panels in the control center           | 5, CC plan Task 11     | F     |
| 10 Notifications model in `athanor-services` | 5                      | F     |
| 11 The notification panel                    | 9, 10                  | G     |
| 12 The bar hands over list and calendar      | 11                     | H     |
| 13 Markup and links                          | 12                     | I     |
| 14 Progress and inline reply                 | 12                     | I     |
| 15 Spike N1: sound playback                  | —                      | any   |
| 16 Sound                                     | 15 and its decision, 5 | J     |
| 17 Popups: corner, overlay, private, rate    | 12                     | J     |
| 18 The gate of the standard                  | 13, 14, 16, 17         | K     |

This plan replaces the control-center plan's Task 10 (do not disturb with an end time, admission of the control center): Tasks 4 and 5 produce what that plan's Task 13 consumes, `DoNotDisturb`, `SetDoNotDisturbUntil` and `DoNotDisturbChanged`, with NC8's signature `(b on, s reason, x until)`. Task 0 records that in the control-center plan.

Tasks in one group share no file and may run as two agents; across groups they run in order. Every test that starts `dbus-daemon` runs outside the command sandbox. Test command for the daemon throughout:

```bash
PATH=$HOME/.nix-profile/bin:$PATH CARGO_TARGET_DIR=/var/tmp/athanor-target-cc cargo test -j 4 -p athanor-shelld
```

Written below as `cargo test -p athanor-shelld`, with the same environment.

---

### Task 0: Spec amendments

**Files:**

- Modify: `docs/architecture/doc_notification_center.md` (NC3 "Where", NC5 file paths, NC6 item 3, NC8 table and signals, NC12 "Reply from a popup", section 6 item 3 path), `docs/architecture/doc_shell_standard.md` (ST5 path), `docs/architecture/doc_bar.md` (BR4 do-not-disturb path sentence if it names one), and on branch `control-center-spec`, `docs/superpowers/plans/2026-10-04-control-center.md` (Task 10 heading gains "Superseded by `docs/superpowers/plans/2026-10-05-notification-center.md`, Tasks 4 and 5").

- [ ] **Step 1:** Write the three amendments of "Spec amendments this plan proposes" into the documents with a Python script in the scratchpad (the formatter rewrites whole `.md` files on Edit), each replacement asserted to match once.
- [ ] **Step 2:** Run `git diff --numstat` — Expected: only the lines the amendments touch; no file with deletions far above its additions.
- [ ] **Step 3:** Run `python3 scripts/verify.py docs` — Expected: no new failure against BASE.
- [ ] **Step 4:** Commit `docs(shell): place the notification files for Landlock and let the bar report fullscreen`.

---

## Step 1 — the daemon, with no visible change

### Task 1: Identity

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/identity.rs`
- Modify: `src/lib.rs` (`pub mod identity;`), `src/sender.rs` (make `is_slice_or_user_manager` `pub(crate)`), `src/hints.rs` (make `is_desktop_id` `pub(crate)`)

**Interfaces:**

- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)] pub enum Identity { App(String), Other }`
  - `pub fn from_cgroup(cgroup: &str) -> Identity`
  - `pub fn of_pid(proc_root: &Path, pid: u32) -> Identity` — a read error is `Other`, logged at debug.
  - `impl Identity { pub fn key(&self) -> &str }` — the application id, or `""` for `Other`, as on the bus and the wire.

Rules of `from_cgroup`, from systemd's desktop-environment convention, where `-` inside a name part is escaped `\x2d`, so splitting on `-` is unambiguous:

- Take the unified line `0::<path>`; a path that does not start with `/` is `Other`. Walk the components; the identity is read from the **first** component that starts with `app-` and ends with `.service` or `.scope`, and at least one component precedes it, every one of which satisfies `sender::is_slice_or_user_manager`. A process in a sub-cgroup of that unit is still that application.
- `.service`: strip `app-` and `.service`, then cut at the first `@`. Split on `-`: one part is the id; two parts are launcher and id.
- `.scope`: strip `app-` and `.scope`. Split on `-`: two parts are id and random; three are launcher, id and random. Flatpak's `app-flatpak-<id>-<number>.scope` is the three-part form.
- Unescape `\x2d` to `-` in the id. The id must pass `hints::is_desktop_id`. Anything else is `Other`.

- [ ] **Step 0: Confirm the names on the image (N5).** On athanor-ref (`ssh athanor-ref`), start one application from the launcher, one from the dock, one by XDG autostart and one Flatpak, and read `cat /proc/<pid>/cgroup` for each. Record the four last components in the commit message of Step 5 and the applications that would land in "Other applications". Expected: the forms above. A form outside them returns to the maintainer before Step 3.
- [ ] **Step 1: Write the failing tests** in `src/identity.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const APPS: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/";

    fn app(id: &str) -> Identity {
        Identity::App(id.to_owned())
    }

    #[test]
    fn units_and_scopes_name_the_application() {
        for (last, expected) in [
            ("app-org.gnome.Calculator@1a2b.service", app("org.gnome.Calculator")),
            ("app-cosmic-firefox@1a.service", app("firefox")),
            ("app-firefox-1234.scope", app("firefox")),
            ("app-cosmic-firefox-1234.scope", app("firefox")),
            ("app-flatpak-org.mozilla.firefox-98765.scope", app("org.mozilla.firefox")),
            ("app-gnome-code\\x2doss-77.scope", app("code-oss")),
        ] {
            assert_eq!(from_cgroup(&format!("{APPS}{last}\n")), expected, "{last}");
        }
    }

    #[test]
    fn a_sub_cgroup_of_an_application_is_that_application() {
        let cgroup = format!("{APPS}app-org.gnome.Calculator@1a2b.service/worker\n");
        assert_eq!(from_cgroup(&cgroup), app("org.gnome.Calculator"));
    }

    #[test]
    fn everything_else_is_other() {
        for cgroup in [
            format!("{APPS}athanor-bar.service\n"),
            "0::/user.slice/user-1000.slice/session-2.scope\n".to_owned(),
            format!("{APPS}app-a-b-c-d.scope\n"),
            format!("{APPS}app-.service\n"),
            format!("{APPS}app-..@1.service\n"),
            format!("{APPS}app-foo.service.d\n"),
            format!("{APPS}some.scope/app-org.gnome.Calculator@1.service\n"),
            "0::app-org.gnome.Calculator@1.service\n".to_owned(),
            String::new(),
        ] {
            assert_eq!(from_cgroup(&cgroup), Identity::Other, "{cgroup}");
        }
    }

    #[test]
    fn a_process_that_has_gone_is_other() {
        let root = std::env::temp_dir().join(format!("athanor-shelld-identity-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("mkdir");
        assert_eq!(of_pid(&root, 4_000_000), Identity::Other);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
```

- [ ] **Step 2:** Run `cargo test -p athanor-shelld identity` — Expected: FAIL to compile (`from_cgroup`, `of_pid`, `Identity` not defined).
- [ ] **Step 3:** Implement `identity.rs` as the rules above say; `Cargo.toml` already has `serde`.
- [ ] **Step 4:** Run `cargo test -p athanor-shelld` — Expected: PASS, every earlier test included.
- [ ] **Step 5:** Commit `feat(shelld): take a notification's application from its cgroup`, with Step 0's four names in the body.

---

### Task 2: Rules and settings

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/rules.rs`
- Modify: `src/lib.rs`, `Cargo.toml` (`nix = { workspace = true, features = ["inotify"] }`)

**Interfaces:**

- Consumes: Task 1 `Identity::key`, `hints::is_desktop_id`.
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockScreen { All, Name, None }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeout { App, Seconds(u32) }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule { pub allowed: bool, pub popups: bool, pub bypass_dnd: bool,
                  pub lock_screen: LockScreen, pub sound: bool, pub timeout: Timeout }
impl Default for Rule // true, true, false, Name, true, App

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention { Days(u32), UntilCleared } // 1, 7, 30 days or `forever`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner { Bar, TopStart, TopEnd, BottomStart, BottomEnd }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window { pub start_min: u16, pub end_min: u16, pub days: [bool; 7] } // Monday first
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings { pub retention: Retention, pub sound: bool, pub popup_corner: Corner,
                      pub private_popups: bool, pub timeout_low_s: u32, pub timeout_normal_s: u32,
                      pub schedule: Option<Window>, pub trigger_schedule: bool,
                      pub trigger_fullscreen: bool, pub trigger_screen_sharing: bool }
impl Default for Settings // Days(7), true, Bar, false, 5, 5, None, true, true, true

pub fn parse_rule(text: &str) -> (Rule, Vec<String>)          // warnings, one per bad line
pub fn parse_settings(text: &str) -> (Settings, Vec<String>)
pub const RULE_KEYS: [&str; 6] = ["allowed", "popups", "bypass_dnd", "lock_screen", "sound", "timeout"];
pub const SETTING_KEYS: [&str; 11] = ["retention", "sound", "popup_corner", "private_popups",
    "timeout_low", "timeout_normal", "schedule", "schedule_days", "trigger_schedule",
    "trigger_fullscreen", "trigger_screen_sharing"];
pub struct Rules { dir: PathBuf /* $XDG_CONFIG_HOME/athanor/notifications */, .. }
impl Rules {
    pub fn new(dir: PathBuf) -> Rules;
    pub fn rule(&mut self, identity: &Identity) -> Rule;   // cached, re-read when Task 2's watch says so
    pub fn settings(&mut self) -> Settings;
    pub fn set_rule(&mut self, app: &str, key: &str, value: &str) -> Result<(), RuleError>;
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), RuleError>;
    /// `relative` is a path under the configuration directory, as `watch` sends it.
    pub fn invalidate(&mut self, relative: &Path) -> Option<Changed>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum Changed { Rule(String /* "" = Other */), Settings }
#[derive(Debug)]
pub enum RuleError { BadApp, BadKey, BadValue, Io(std::io::Error) }
pub async fn watch(dir: PathBuf, changed: tokio::sync::mpsc::Sender<PathBuf>) -> std::io::Result<()>;
```

File format: `key=value` lines; blank lines and lines starting with `#` ignored; whitespace around key and value trimmed. Values: booleans `true`/`false`; `lock_screen` `all`/`name`/`none`; `timeout` a whole number of seconds 1–3600 or `app`; `retention` `1`, `7`, `30` or `forever`; `popup_corner` `bar`, `top-start`, `top-end`, `bottom-start`, `bottom-end`; `schedule` `HH:MM-HH:MM` (start may be after end: the window crosses midnight; equal start and end is invalid); `schedule_days` a comma list of `mon`…`sun`, at least one. `bypass_dnd` in `other.conf` is parsed and ignored by the policy (Task 5). Writes go to a temporary file in the same directory and a rename; the other lines of the file are kept in their order; mode 0600. `set_rule("", …)` writes `other.conf`; any other `app` that fails `is_desktop_id` is refused, so no path leaves `apps/`. `set_setting` writes `notifications.conf`. `watch` uses `nix::sys::inotify` on the configuration directory and on its `apps/` (`IN_CLOSE_WRITE | IN_MOVED_TO | IN_DELETE`) through `tokio::io::unix::AsyncFd`, and sends each changed path relative to the configuration directory: `notifications.conf`, `other.conf` or `apps/<id>.conf`. Each file is read when first needed and cached until `invalidate`.

- [ ] **Step 1: Write the failing tests** in `src/rules.rs`:

```rust
#[test]
fn an_empty_file_is_every_default() {
    assert_eq!(parse_rule(""), (Rule::default(), vec![]));
    assert_eq!(parse_settings(""), (Settings::default(), vec![]));
}

#[test]
fn a_bad_line_costs_only_its_key() {
    let (rule, warnings) = parse_rule("allowed=false\npopups=maybe\ncolour=red\n# note\n\nsound = false\n");
    assert!(!rule.allowed);
    assert!(rule.popups, "a value that does not parse takes the default");
    assert!(!rule.sound, "whitespace is trimmed");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn timeouts_and_lock_screen_parse() {
    assert_eq!(parse_rule("timeout=12").0.timeout, Timeout::Seconds(12));
    assert_eq!(parse_rule("timeout=app").0.timeout, Timeout::App);
    assert_eq!(parse_rule("timeout=0").0.timeout, Timeout::App);
    assert_eq!(parse_rule("timeout=99999").0.timeout, Timeout::App);
    assert_eq!(parse_rule("lock_screen=none").0.lock_screen, LockScreen::None);
}

#[test]
fn a_schedule_crosses_midnight_on_chosen_days() {
    let (s, w) = parse_settings("schedule=22:00-07:00\nschedule_days=mon,fri\nretention=forever\n");
    assert!(w.is_empty(), "{w:?}");
    let window = s.schedule.expect("window");
    assert_eq!((window.start_min, window.end_min), (22 * 60, 7 * 60));
    assert_eq!(window.days, [true, false, false, false, true, false, false]);
    assert_eq!(s.retention, Retention::UntilCleared);
    for bad in ["schedule=07:00-07:00", "schedule=25:00-07:00", "schedule=7-8", "schedule_days="] {
        assert_eq!(parse_settings(bad).1.len(), 1, "{bad}");
    }
}

#[test]
fn set_rule_keeps_other_lines_and_refuses_paths() {
    let dir = std::env::temp_dir().join(format!("athanor-shelld-rules-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("apps")).expect("mkdir");
    std::fs::write(dir.join("apps/org.example.Chat.conf"), "# mine\nsound=false\n").expect("seed");
    let mut rules = Rules::new(dir.clone());
    rules.set_rule("org.example.Chat", "allowed", "false").expect("set");
    let text = std::fs::read_to_string(dir.join("apps/org.example.Chat.conf")).expect("read");
    assert_eq!(text, "# mine\nsound=false\nallowed=false\n");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(dir.join("apps/org.example.Chat.conf")).expect("meta").permissions());
    assert_eq!(mode & 0o777, 0o600);
    for app in ["../../.bashrc", "a/b", ".hidden", "other.conf/../../x"] {
        assert!(matches!(rules.set_rule(app, "allowed", "false"), Err(RuleError::BadApp)), "{app}");
    }
    assert!(matches!(rules.set_rule("org.example.Chat", "colour", "red"), Err(RuleError::BadKey)));
    assert!(matches!(rules.set_rule("org.example.Chat", "allowed", "maybe"), Err(RuleError::BadValue)));
    assert_eq!(std::fs::read_dir(&dir).expect("ls").count(), 1, "nothing written outside apps/");
    rules.set_rule("", "popups", "false").expect("other");
    assert_eq!(std::fs::read_to_string(dir.join("other.conf")).expect("read"), "popups=false\n");
    rules.set_rule("other", "popups", "true").expect("an application named other");
    assert!(dir.join("apps/other.conf").exists());
    assert!(!rules.rule(&Identity::Other).popups, "Other's rule is not the application's");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test(flavor = "current_thread")]
async fn a_file_changed_on_disk_is_read_again() {
    let dir = std::env::temp_dir().join(format!("athanor-shelld-watch-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("apps")).expect("mkdir");
    let mut rules = Rules::new(dir.clone());
    let chat = Identity::App("org.example.Chat".into());
    assert!(rules.rule(&chat).allowed);
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(watch(dir.clone(), tx));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(dir.join("apps/org.example.Chat.conf"), "allowed=false\n").expect("write");
    let path = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await.expect("event").expect("path");
    assert_eq!(path, Path::new("apps/org.example.Chat.conf"));
    assert_eq!(rules.invalidate(&path), Some(Changed::Rule("org.example.Chat".into())));
    assert!(!rules.rule(&chat).allowed);
    std::fs::remove_dir_all(dir).expect("cleanup");
}
```

- [ ] **Step 2:** Run `cargo test -p athanor-shelld rules` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement `rules.rs`.
- [ ] **Step 4:** Run `cargo test -p athanor-shelld` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(shelld): read and write the per-application notification rules`.

---

### Task 3: History

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/history.rs`
- Modify: `src/store.rs` (`CAPACITY` 500; `Notification` gains fields; `Store::restore`, `prune`, `mark_read`, `clear_all`, `clear_group`), `src/lib.rs`

**Interfaces:**

- Consumes: Task 1 `Identity`; Task 2 `Retention`.
- Produces:

```rust
// store.rs
pub const CAPACITY: usize = 500;
pub struct Notification { pub id: u32, pub arrived_ms: u64, pub time: i64 /* unix seconds */,
    pub identity: Identity, pub sender: String /* unique bus name, "" when gone */,
    pub read: bool, pub content: Content }
impl Store {
    pub fn notify(&mut self, content: Content, replaces_id: u32, now_ms: u64, time: i64,
                  identity: Identity, sender: String) -> Outcome;
    pub fn restore(&mut self, restored: Vec<Notification>); // ids continue above the highest
    pub fn prune(&mut self, now: i64, retention: Retention) -> Vec<u32>;
    pub fn mark_read(&mut self, ids: &[u32]) -> Vec<u32>;  // the ids that changed
    pub fn clear_all(&mut self) -> Vec<u32>;
    pub fn clear_group(&mut self, identity: &Identity) -> Vec<u32>;
    pub fn sender_gone(&mut self, name: &str) -> Vec<u32>; // clears `sender`, returns the ids
}
// history.rs
pub fn load(path: &Path, bus_id: &str) -> Vec<Notification>;
pub fn save(path: &Path, store: &Store, bus_id: &str) -> std::io::Result<()>;
pub struct Coalescer { .. }
impl Coalescer {
    pub fn new() -> Coalescer;
    /// A change at `now`: `Some(delay)` to wait before writing, `None` when a write is already due.
    pub fn changed(&mut self, now: Instant) -> Option<Duration>;
    pub fn written(&mut self, now: Instant, ok: bool) -> bool; // true when a warning must be logged
}
```

The file: `{"version":1,"bus_id":"…","notifications":[…]}`, each entry with `id`, `time`, `identity`, `sender`, `read`, `app_name`, `summary`, `body`, `actions`, `urgency`, `resident`, `desktop_entry`, `icon_name`. Unknown fields are ignored and optional ones default (`#[serde(default)]`), so another version reads. Never written: entries with `transient`, `Visual::Pixels`, `Visual::Icon(Icon::File)` (only an icon name is kept), reply text (Task 14 never puts it in the store). `bus_id` is `org.freedesktop.DBus.GetId`: on load, when it differs from the current bus, every `sender` is emptied, because unique names restart with a new bus and `:1.42` would name another program. Restored entries get `arrived_ms = 0`, so `popup_ms_left` is 0: no popup replays. A file that does not parse is renamed `notifications.json.corrupt` (replacing any earlier one) with one warning; a missing file is an empty history. Writes: at most one every 2 s (`Coalescer`), at once for clear-all and clear-group and on stop; a failed write keeps the store and logs once until a write succeeds.

- [ ] **Step 1: Write the failing tests** in `src/history.rs`:

```rust
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("athanor-shelld-history-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn held(store: &mut Store, summary: &str, transient: bool, time: i64) -> u32 {
    let mut content = crate::store::tests::content(summary, Urgency::Normal, 5_000);
    content.transient = transient;
    store.notify(content, 0, 0, time, Identity::App("org.example.Chat".into()), ":1.7".into()).notification.id
}

#[test]
fn a_saved_history_reads_back_without_transients_or_images() {
    let dir = temp("roundtrip");
    let path = dir.join("notifications.json");
    let mut store = Store::new(false);
    held(&mut store, "kept", false, 100);
    held(&mut store, "transient", true, 101);
    let mut pixels = crate::store::tests::content("picture", Urgency::Normal, 5_000);
    pixels.visual = Visual::Pixels(crate::image::tests::one_pixel());
    store.notify(pixels, 0, 0, 102, Identity::Other, ":1.8".into());
    save(&path, &store, "bus-a").expect("save");
    let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let back = load(&path, "bus-a");
    assert_eq!(back.iter().map(|n| n.content.summary.as_str()).collect::<Vec<_>>(), ["kept", "picture"]);
    assert_eq!(back[1].content.visual, Visual::None);
    assert_eq!(back[0].sender, ":1.7", "same bus: the sender is kept");
    assert!(load(&path, "bus-b").iter().all(|n| n.sender.is_empty()), "another bus: senders emptied");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn ids_continue_above_the_highest_restored() {
    let mut store = Store::new(false);
    store.restore(vec![crate::store::tests::notification(41), crate::store::tests::notification(7)]);
    let next = held(&mut store, "new", false, 0);
    assert_eq!(next, 42);
}

#[test]
fn retention_by_age_and_by_count() {
    let mut store = Store::new(false);
    let day = 86_400;
    held(&mut store, "old", false, 0);
    held(&mut store, "recent", false, 7 * day);
    assert_eq!(store.prune(8 * day, Retention::Days(7)).len(), 1);
    assert_eq!(store.prune(800 * day, Retention::UntilCleared).len(), 0);
    for n in 0..CAPACITY + 3 {
        held(&mut store, &format!("n{n}"), false, 8 * day);
    }
    assert_eq!(store.iter().count(), CAPACITY);
}

#[test]
fn a_corrupt_file_is_renamed_and_the_history_starts_empty() {
    let dir = temp("corrupt");
    let path = dir.join("notifications.json");
    std::fs::write(&path, "{not json").expect("write");
    assert!(load(&path, "bus").is_empty());
    assert!(!path.exists());
    assert_eq!(std::fs::read_to_string(dir.join("notifications.json.corrupt")).expect("kept"), "{not json");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn another_version_reads_with_defaults() {
    let dir = temp("version");
    let path = dir.join("notifications.json");
    std::fs::write(&path, r#"{"version":2,"bus_id":"b","future":1,"notifications":[{"id":3,"time":5,"identity":"Other","summary":"s","later":true}]}"#).expect("write");
    let back = load(&path, "b");
    assert_eq!(back.len(), 1);
    assert_eq!((back[0].id, back[0].content.summary.as_str(), back[0].read), (3, "s", false));
    assert!(!dir.join("notifications.json.corrupt").exists());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_failed_write_keeps_the_store_and_warns_once() {
    let dir = temp("readonly");
    let mut store = Store::new(false);
    held(&mut store, "kept", false, 0);
    let path = dir.join("missing-dir/notifications.json");
    assert!(save(&path, &store, "bus").is_err());
    assert_eq!(store.iter().count(), 1);
    let mut coalescer = Coalescer::new();
    let now = Instant::now();
    assert!(coalescer.written(now, false), "the first failure warns");
    assert!(!coalescer.written(now, false), "the second does not");
    assert!(!coalescer.written(now, true));
    assert!(coalescer.written(now, false), "after a success, a new failure warns again");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn writes_are_coalesced_to_one_every_two_seconds() {
    let mut coalescer = Coalescer::new();
    let start = Instant::now();
    assert_eq!(coalescer.changed(start), None, "the first change writes at once");
    coalescer.written(start, true);
    assert_eq!(coalescer.changed(start + Duration::from_millis(500)), Some(Duration::from_millis(1500)));
    assert_eq!(coalescer.changed(start + Duration::from_millis(900)), Some(Duration::from_millis(1100)));
    assert_eq!(coalescer.changed(start + Duration::from_secs(3)), None);
}
```

`store::tests` already has `pub(crate) fn content(summary: &str, urgency: Urgency, timeout_ms: u32) -> Content`; it gains `pub(crate) fn notification(id: u32) -> Notification` (identity `Other`, sender `""`, time 0). `image`'s `mod tests` becomes `pub(crate) mod tests` and gains `pub(crate) fn one_pixel() -> Image`. `Store::new` keeps its `dnd: bool` argument until Task 5 removes it.

- [ ] **Step 2:** Run `cargo test -p athanor-shelld history` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement `history.rs` and the store changes; adapt the store's existing tests to the new `notify` arguments (same assertions).
- [ ] **Step 4:** Run `cargo test -p athanor-shelld` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(shelld): keep the notification history across restarts and reboots`.

---

### Task 4: Do not disturb

**Files:**

- Modify: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/dnd.rs` (rewrite; the file keeps its name `do-not-disturb`)
- Create: `src/clock.rs`
- Modify: `Cargo.toml` (`nix` features `["inotify", "time"]`, `libc = { workspace = true }`)

**Interfaces:**

- Consumes: Task 2 `Window`, `Settings`.
- Produces:

```rust
// dnd.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason { Manual, Schedule, Fullscreen, ScreenSharing }
impl Reason { pub fn as_str(self) -> &'static str } // "manual", "schedule", "fullscreen", "screen-sharing"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger { Unavailable, Inactive, Active }
#[derive(Debug, Clone, Copy)]
pub struct Local { pub minute: u16 /* since midnight */, pub weekday: u8 /* 0 = Monday */ }
pub struct Observed { pub now: i64, pub local: Local, pub fullscreen: Trigger, pub sharing: Trigger }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective { pub on: bool, pub reason: Option<Reason>, pub until: Option<i64>,
                       pub unavailable: Vec<Reason> }
#[derive(Debug, Default)]
pub struct Dnd { manual_on: Option<Option<i64>>, off_override: Option<BTreeSet<Reason>>,
                 last_auto: BTreeSet<Reason>, last_on: bool, missed: u32 }
impl Dnd {
    pub fn load(dir: &Path) -> io::Result<Dnd>;
    pub fn save(&self, dir: &Path) -> io::Result<()>;
    pub fn set_manual(&mut self, on: bool, until: Option<i64>, observed: &Observed, settings: &Settings);
    /// The state now; `Some(missed)` when it has just turned off with notifications missed.
    pub fn evaluate(&mut self, observed: &Observed, settings: &Settings) -> (Effective, Option<u32>);
    pub fn missed_one(&mut self); // a popup was hidden by do not disturb
}
pub fn in_window(window: &Window, local: Local) -> bool;
/// Seconds until the next edge of the window or `until`, capped at one hour (a DST change
/// is then corrected at the next wake-up).
pub fn next_wake(observed: &Observed, settings: &Settings, until: Option<i64>) -> u64;
// clock.rs
pub fn local_now() -> (i64, Local); // libc::tzset() then localtime_r: reads /etc/localtime again
pub async fn changes(tx: tokio::sync::mpsc::Sender<()>, wake: tokio::sync::watch::Receiver<u64>);
```

The file `do-not-disturb` in the state directory holds `key=value` lines: `on=true`, `until=<unix seconds>`, `off_override=schedule,fullscreen`. Today's content `on\n` reads as `on=true` (the switch of `doc_bar.md`). An `until` in the past reads as off and the file is rewritten. Do not disturb is in effect when `manual_on` holds (and its `until`, if any, is ahead), or when an automatic source is active and `off_override` is absent. `off_override` is set by `set_manual(false)` to the active automatic set, and cleared by `evaluate` as soon as that set differs. The schedule counts only when `trigger_schedule`; fullscreen only when `trigger_fullscreen`, and so on; a source the session cannot observe is listed in `unavailable`. The reason is `Manual` when the switch holds, else the first active source in the order schedule, fullscreen, screen sharing.

`clock::changes` sends `()` on each of: a `timerfd` on `CLOCK_REALTIME` armed `TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET` at the next wake (an `ECANCELED` read means the clock was set); logind's `PrepareForSleep(false)` on the system bus; timedated's `PropertiesChanged` for `Timezone`. Each re-runs `evaluate`. A system bus that is absent logs once and the timerfd alone remains.

- [ ] **Step 1: Write the failing tests** in `src/dnd.rs`:

```rust
fn at(day: u8, hh: u16, mm: u16, now: i64) -> Observed {
    Observed { now, local: Local { minute: hh * 60 + mm, weekday: day },
               fullscreen: Trigger::Inactive, sharing: Trigger::Unavailable }
}

fn night() -> Settings {
    Settings { schedule: Some(Window { start_min: 22 * 60, end_min: 7 * 60, days: [true; 7] }),
               ..Settings::default() }
}

#[test]
fn the_window_crosses_midnight_on_its_days() {
    let w = Window { start_min: 22 * 60, end_min: 7 * 60, days: [true, false, false, false, false, false, false] };
    assert!(in_window(&w, Local { minute: 23 * 60, weekday: 0 }), "Monday night");
    assert!(in_window(&w, Local { minute: 6 * 60, weekday: 1 }), "Tuesday morning belongs to Monday's window");
    assert!(!in_window(&w, Local { minute: 7 * 60, weekday: 1 }), "the end is exclusive");
    assert!(!in_window(&w, Local { minute: 23 * 60, weekday: 1 }), "not on Tuesday");
}

#[test]
fn the_schedule_turns_on_and_off_with_one_summary() {
    let mut dnd = Dnd::default();
    let settings = night();
    assert!(!dnd.evaluate(&at(0, 21, 59, 0), &settings).0.on);
    let (on, _) = dnd.evaluate(&at(0, 22, 0, 60), &settings);
    assert_eq!((on.on, on.reason), (true, Some(Reason::Schedule)));
    dnd.missed_one();
    dnd.missed_one();
    let (off, summary) = dnd.evaluate(&at(1, 7, 0, 32_460), &settings);
    assert!(!off.on);
    assert_eq!(summary, Some(2));
    assert_eq!(dnd.evaluate(&at(1, 7, 1, 32_520), &settings).1, None, "one summary only");
}

#[test]
fn off_by_hand_holds_until_the_next_automatic_change() {
    let mut dnd = Dnd::default();
    let settings = night();
    dnd.evaluate(&at(0, 22, 0, 0), &settings);
    dnd.set_manual(false, None, &at(0, 23, 0, 3_600), &settings);
    assert!(!dnd.evaluate(&at(1, 3, 0, 18_000), &settings).0.on, "still off at 03:00");
    assert!(!dnd.evaluate(&at(1, 7, 0, 32_400), &settings).0.on);
    assert!(dnd.evaluate(&at(1, 22, 0, 86_400), &settings).0.on, "the next night turns it on");
}

#[test]
fn on_by_hand_for_an_hour_survives_a_fullscreen_video() {
    let mut dnd = Dnd::default();
    let settings = Settings::default();
    dnd.set_manual(true, Some(3_600), &at(2, 12, 0, 0), &settings);
    let mut video = at(2, 12, 10, 600);
    video.fullscreen = Trigger::Active;
    assert_eq!(dnd.evaluate(&video, &settings).0.reason, Some(Reason::Manual));
    assert!(dnd.evaluate(&at(2, 12, 20, 1_200), &settings).0.on, "the video ended, the hour holds");
    assert!(!dnd.evaluate(&at(2, 13, 0, 3_600), &settings).0.on, "until is exclusive");
}

#[test]
fn a_clock_set_backwards_is_evaluated_again_without_a_second_summary() {
    let mut dnd = Dnd::default();
    let settings = night();
    dnd.evaluate(&at(0, 22, 30, 1_800), &settings);
    dnd.missed_one();
    assert_eq!(dnd.evaluate(&at(0, 21, 50, -600), &settings).1, Some(1), "the clock went back out of the window");
    assert!(dnd.evaluate(&at(0, 22, 0, 0), &settings).0.on);
    assert_eq!(dnd.evaluate(&at(1, 7, 0, 32_400), &settings).1, None, "nothing missed the second time");
}

#[test]
fn an_unobservable_trigger_is_published_as_unavailable() {
    let mut dnd = Dnd::default();
    let mut observed = at(0, 12, 0, 0);
    observed.fullscreen = Trigger::Unavailable;
    let (state, _) = dnd.evaluate(&observed, &Settings::default());
    assert!(!state.on);
    assert_eq!(state.unavailable, [Reason::Fullscreen, Reason::ScreenSharing]);
}

#[test]
fn the_switch_file_reads_the_old_form_and_drops_a_past_until() {
    let dir = std::env::temp_dir().join(format!("athanor-shelld-dnd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("do-not-disturb"), "on\n").expect("legacy");
    let mut dnd = Dnd::load(&dir).expect("load");
    assert!(dnd.evaluate(&at(0, 12, 0, 0), &Settings::default()).0.on);
    std::fs::write(dir.join("do-not-disturb"), "on=true\nuntil=10\n").expect("past");
    let mut dnd = Dnd::load(&dir).expect("load");
    assert!(!dnd.evaluate(&at(0, 12, 0, 20), &Settings::default()).0.on);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn the_next_wake_is_the_next_edge_capped_at_an_hour() {
    let settings = night();
    assert_eq!(next_wake(&at(0, 21, 30, 0), &settings, None), 1_800);
    assert_eq!(next_wake(&at(0, 12, 0, 0), &settings, None), 3_600);
    assert_eq!(next_wake(&at(0, 12, 0, 0), &settings, Some(600)), 600);
}
```

These replace `the_switch_round_trips_and_off_twice_is_fine`, whose "absent file means off" still holds. In `src/clock.rs`, with `chrono` as a dev-dependency only:

```rust
#[test]
fn local_time_agrees_with_chrono() {
    use chrono::{Datelike, Timelike};
    let reading = |t: chrono::DateTime<chrono::Local>| Local {
        minute: (t.hour() * 60 + t.minute()) as u16,
        weekday: t.weekday().num_days_from_monday() as u8,
    };
    let before = reading(chrono::Local::now());
    let (now, ours) = local_now();
    let after = reading(chrono::Local::now());
    assert!(
        [(before.minute, before.weekday), (after.minute, after.weekday)].contains(&(ours.minute, ours.weekday)),
        "a minute boundary may fall between the readings, never outside them"
    );
    assert!((now - chrono::Local::now().timestamp()).abs() <= 1);
}
```

- [ ] **Step 2:** Run `cargo test -p athanor-shelld dnd clock` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement `dnd.rs` and `clock.rs`.
- [ ] **Step 4:** Run `cargo test -p athanor-shelld` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(shelld): turn do not disturb on by hand, by schedule and by trigger`.

---

### Task 5: Policy, interface and wire

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/policy.rs`, `system/athanor-services/src/notifications/mod.rs`, `system/athanor-services/src/notifications/wire.rs`
- Modify (shelld): `src/wire.rs` (becomes `pub use athanor_services::notifications::wire::*;` plus `fn from_notification`), `src/sender.rs` (`BarUnit` → `Admitted`), `src/notifications.rs`, `src/server.rs`, `src/main.rs` (paths, Landlock), `src/dnd.rs` (wiring only), `Cargo.toml` (+`athanor-services`, +`athanor-i18n`), `data/athanor-shelld.service` (`ConfigurationDirectory` is not used: the directory is created by the daemon), `tests/common/mod.rs`, `tests/notifications.rs`
- Modify (unit): `system/athanor-unit/src/sandbox.rs` (`restrict(read, write: &[&Path])`)
- Modify (bar): `forge/specs/athanor-bar/athanor-bar-1.0.0/src/notices.rs` (the mirror of the wire), `src/ui/notifications.rs` (`List`, `DoNotDisturb`, the new signals)

**Interfaces:**

- Consumes: Tasks 1–4.
- Produces:

```rust
// policy.rs
pub struct Facts<'a> { pub identity: &'a Identity, pub rule: &'a Rule, pub settings: &'a Settings,
    pub dnd_on: bool, pub urgency: Urgency, pub transient: bool, pub expire_timeout: i32,
    pub suppress_sound: bool, pub rate_limited: bool }
#[derive(Debug, PartialEq, Eq)]
pub struct Decision { pub keep: bool, pub list: bool, pub popup: bool, pub sound: bool, pub timeout_ms: u32 }
pub fn decide(facts: &Facts) -> Decision;
// sender.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller { Bar, ControlCenter }
pub fn admits(caller: Caller, method: &str) -> bool;  // the table below
pub struct Admitted { .. } // a boxed Fn(&str /* unique name */, u32 /* pid */) -> Option<Caller> + Send + Sync
impl Admitted {
    pub fn from_proc_root(proc_root: impl Into<PathBuf>) -> Admitted; // the production rule, by the pid's cgroup
    pub fn from_fn(f: impl Fn(&str, u32) -> Option<Caller> + Send + Sync + 'static) -> Admitted; // tests
    pub fn caller(&self, unique_name: &str, pid: u32) -> Option<Caller>;
}
// server.rs
pub struct Config { pub state_dir: PathBuf, pub config_dir: PathBuf, pub proc_root: PathBuf,
                    pub admitted: Admitted }
```

Rules of `decide`: the rule is the application's for `App`, `other.conf`'s for `Other`, and `bypass_dnd` counts only for `App`. `allowed = false`: nothing kept, listed, shown or played. Otherwise `keep = !transient`; `list = true`; `popup = rule.popups && !rate_limited && (!dnd_on || critical || bypass)`; `sound = settings.sound && rule.sound && !suppress_sound && !rate_limited && (!dnd_on || critical || bypass)`; `timeout_ms` is 0 for critical, else the rule's seconds, else the application's `expire_timeout` when it is above 0, else 0 when it is 0, else `timeout_low_s` or `timeout_normal_s` by urgency.

The private interface `os.athanor.Notifications1` (NC8), every method checked with `admits(caller, method)`:

| Method                                    | Signature                     | Bar | Control center           |
| ----------------------------------------- | ----------------------------- | --- | ------------------------ |
| `List`                                    | `() → a(wire)` unread         | yes | yes                      |
| `History`                                 | `() → a(wire)` all            | no  | yes                      |
| `Close`                                   | `(u id, u reason) → ()`       | yes | yes                      |
| `InvokeAction`                            | `(u id, s key, s token) → ()` | yes | yes                      |
| `Reply`                                   | `(u id, s text) → ()`         | yes | yes (body in Task 14)    |
| `MarkRead`                                | `(au ids) → ()`               | yes | yes                      |
| `ClearAll`                                | `() → ()`                     | no  | yes                      |
| `ClearGroup`                              | `(s app) → ()` `""` = Other   | no  | yes                      |
| `DoNotDisturb`                            | `() → (b, s, x, as)`          | yes | yes                      |
| `SetDoNotDisturb`, `SetDoNotDisturbUntil` | `(b)`, `(b, x)`               | yes | yes                      |
| `Rules`                                   | `(s app) → a{ss}`             | no  | yes                      |
| `SetRule`                                 | `(s app, s key, s value)`     | no  | `allowed`, `popups` only |
| `Settings`                                | `() → a{ss}`                  | yes | yes                      |
| `SetSetting`                              | `(s key, s value)`            | no  | no (Settings, later)     |
| `ReportFullscreen`                        | `(b available, b active)`     | yes | no                       |

`DoNotDisturb` returns `(on, reason, until, unavailable)`, `reason` `""` when off, `until` 0 when none. The first admitted call of a unit records its unique name as that unit's destination; the bus's `NameOwnerChanged` forgets it. Signals `Added(wire)`, `Replaced(wire)`, `Closed(u, u)`, `Read(au)`, `DoNotDisturbChanged(b on, s reason, x until)`, `RulesChanged(s app)`, `SettingsChanged()` are emitted once per recorded destination, unicast, never broadcast. `ReportFullscreen` feeds Task 4's `Observed::fullscreen`; the bar's destination vanishing sets it to `Unavailable`.

The wire, `athanor_services::notifications::wire::WireNotification` (zvariant `Type`), fields in this order: `id u`, `app_id s` (`""` = Other), `app_name s`, `summary s`, `body s` (plain text), `body_spans a(sus)` (text, style bits b=1 i=2 u=4, href `""` = none; Task 5 sends one plain span), `actions a(ss)`, `actions_available b`, `urgency y`, `transient b`, `resident b`, `read b`, `time x`, `desktop_entry s`, `icon_name s`, `icon_file s`, `image_width u`, `image_height u`, `image_rgba ay`, `timeout_ms u`, `popup_ms_left u`, `popup b`, `value i` (-1 = none), `reply b`, `reply_placeholder s`. The bar's `notices.rs` mirrors it field by field, and both crates keep the test `the_fields_keep_their_order` over the same table of values.

Notify, in order: decode (`content`), identity (Task 1, through `GetConnectionCredentials` of the sender), rule and settings (Task 2), do not disturb (Task 4), `decide`; when `allowed = false` return a fresh id and keep nothing; else store with the decision, mark the coalescer, emit to the destinations. A popup hidden by do not disturb calls `Dnd::missed_one`. When `evaluate` returns `Some(n)` the daemon sends itself a transient notification "{n} notifications while do not disturb was on" (`athanor_i18n::tr`, plural form) with a `default` action whose invocation calls `os.athanor.ControlCenter1.Show("notifications")` on the session bus. On start: `load` the history with the bus's `GetId`, `prune`, then serve; on `SIGTERM` the history is written before exit.

The store's `dnd` field, `Store::dnd`, `Store::set_dnd` and the `dnd` argument of `Store::new` and `popup_ms_left` go: do not disturb lives in `dnd::Dnd` and reaches the store as `Decision::popup`.

Paths (amendment 1): state `$XDG_STATE_HOME/athanor/shelld/` (history and switch), config `$XDG_CONFIG_HOME/athanor/notifications/` with `apps/`, both created before Landlock, both writable; `sandbox::restrict` takes `write: &[&Path]`. Run `codegraph_impact` on `restrict` first: its only caller outside `athanor-update-notify` (which has its own copy) is `athanor-shelld`.

- [ ] **Step 1: Write the failing tests.** In `src/policy.rs`, the decision table:

```rust
fn facts<'a>(identity: &'a Identity, rule: &'a Rule, settings: &'a Settings) -> Facts<'a> {
    Facts { identity, rule, settings, dnd_on: false, urgency: Urgency::Normal, transient: false,
            expire_timeout: -1, suppress_sound: false, rate_limited: false }
}

#[test]
fn the_decision_table() {
    let app = Identity::App("org.example.Chat".into());
    let other = Identity::Other;
    let settings = Settings::default();
    let plain = Rule::default();
    let bypass = Rule { bypass_dnd: true, ..Rule::default() };
    let muted = Rule { allowed: false, ..Rule::default() };
    let quiet = Rule { sound: false, ..Rule::default() };
    let listed = Rule { popups: false, ..Rule::default() };

    let d = decide(&facts(&app, &plain, &settings));
    assert_eq!(d, Decision { keep: true, list: true, popup: true, sound: true, timeout_ms: 5_000 });
    assert_eq!(decide(&facts(&app, &muted, &settings)),
               Decision { keep: false, list: false, popup: false, sound: false, timeout_ms: 0 });
    assert!(!decide(&facts(&app, &listed, &settings)).popup);
    assert!(!decide(&Facts { transient: true, ..facts(&app, &plain, &settings) }).keep);

    let dnd = |identity, rule| decide(&Facts { dnd_on: true, ..facts(identity, rule, &settings) });
    assert!(!dnd(&app, &plain).popup && !dnd(&app, &plain).sound);
    assert!(dnd(&app, &bypass).popup && dnd(&app, &bypass).sound);
    assert!(!dnd(&other, &bypass).popup, "bypass never applies without a proven identity");
    let critical = |rule| decide(&Facts { dnd_on: true, urgency: Urgency::Critical, ..facts(&app, rule, &settings) });
    assert!(critical(&plain).popup && critical(&plain).sound);
    assert!(!critical(&quiet).sound, "critical still respects sound=false");
    assert_eq!(critical(&plain).timeout_ms, 0);
    assert!(!decide(&Facts { suppress_sound: true, ..facts(&app, &plain, &settings) }).sound);
    let silent = Settings { sound: false, ..Settings::default() };
    assert!(!decide(&facts(&app, &plain, &silent)).sound);
}

#[test]
fn the_timeout_order_is_rule_then_application_then_urgency() {
    let app = Identity::App("a".into());
    let settings = Settings { timeout_low_s: 3, timeout_normal_s: 8, ..Settings::default() };
    let ruled = Rule { timeout: Timeout::Seconds(12), ..Rule::default() };
    let plain = Rule::default();
    assert_eq!(decide(&Facts { expire_timeout: 2_000, ..facts(&app, &ruled, &settings) }).timeout_ms, 12_000);
    assert_eq!(decide(&Facts { expire_timeout: 2_000, ..facts(&app, &plain, &settings) }).timeout_ms, 2_000);
    assert_eq!(decide(&Facts { expire_timeout: 0, ..facts(&app, &plain, &settings) }).timeout_ms, 0);
    assert_eq!(decide(&facts(&app, &plain, &settings)).timeout_ms, 8_000);
    assert_eq!(decide(&Facts { urgency: Urgency::Low, ..facts(&app, &plain, &settings) }).timeout_ms, 3_000);
}
```

In `src/sender.rs`: `caller` returns `Some(Bar)` for `…/app.slice/athanor-bar.service`, `Some(ControlCenter)` for `…/app.slice/athanor-control-center.service`, `None` for `app-athanor-foo@0123.service`, for a child cgroup of either unit and for a missing pid (the existing cases, kept). `admits` over the whole table above, one assertion per cell.

Every connection of a test belongs to the test process, so one pid cannot stand for two units. The test daemon therefore admits by unique name, and the production daemon by the pid's cgroup; identity (Task 1) still reads the fake `/proc`. In `tests/common/mod.rs`, beside `Bus::daemon`, which keeps its pid-based admission:

```rust
use athanor_shelld::sender::{Admitted, Caller};

/// A daemon that admits `callers` by unique name and sees every sender in `cgroup`.
pub async fn daemon_for(&self, cgroup: &str, callers: &[(&Connection, Caller)]) -> Connection {
    let proc_root = fake_proc(&self.dir, cgroup);
    let names: HashMap<String, Caller> = callers
        .iter()
        .map(|(conn, caller)| (conn.unique_name().expect("unique name").to_string(), *caller))
        .collect();
    let (state_dir, config_dir) = (self.dir.join("state"), self.dir.join("config"));
    for dir in [&state_dir, &config_dir] {
        fs::create_dir_all(dir).expect("mkdir");
    }
    server::start(
        self.builder(),
        Config {
            state_dir,
            config_dir,
            proc_root,
            admitted: Admitted::from_fn(move |name, _pid| names.get(name).copied()),
        },
    )
    .await
    .expect("daemon")
}

/// Waits up to 5 s for `path` to contain `needle`.
pub async fn wait_for_file(path: &Path, needle: &str) {
    for _ in 0..100 {
        if fs::read_to_string(path).is_ok_and(|text| text.contains(needle)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("{} never contained {needle}", path.display());
}
```

In `tests/notifications.rs`, beside the existing `public`, `private` and `notify` helpers:

```rust
/// The daemon, then the bar, the control center and an application, connected before it.
async fn units(bus: &Bus, cgroup: &str) -> (Connection, Connection, Connection, Connection) {
    let (bar, center, app) = (bus.client().await, bus.client().await, bus.client().await);
    let daemon = bus
        .daemon_for(cgroup, &[(&bar, Caller::Bar), (&center, Caller::ControlCenter)])
        .await;
    (daemon, bar, center, app)
}

async fn admitted<B>(conn: &Connection, method: &str, body: &B) -> bool
where
    B: serde::Serialize + zvariant::DynamicType,
{
    match private(conn).await.call_method(method, body).await {
        Ok(_) => true,
        Err(err) => !err.to_string().contains("AccessDenied"),
    }
}

#[tokio::test]
async fn each_method_admits_the_units_of_the_table() {
    let bus = Bus::start("admit");
    let (_daemon, bar, center, app) = units(&bus, APP_CGROUP).await;
    macro_rules! row {
        ($method:literal, $body:expr, $bar:literal, $center:literal) => {
            assert_eq!(admitted(&bar, $method, &$body).await, $bar, "bar {}", $method);
            assert_eq!(admitted(&center, $method, &$body).await, $center, "center {}", $method);
            assert!(!admitted(&app, $method, &$body).await, "application {}", $method);
        };
    }
    row!("List", (), true, true);
    row!("History", (), false, true);
    row!("Close", (1u32, 2u32), true, true);
    row!("InvokeAction", (1u32, "default", ""), true, true);
    row!("Reply", (1u32, "hi"), true, true);
    row!("MarkRead", (vec![1u32],), true, true);
    row!("ClearAll", (), false, true);
    row!("ClearGroup", ("",), false, true);
    row!("DoNotDisturb", (), true, true);
    row!("SetDoNotDisturb", (false,), true, true);
    row!("SetDoNotDisturbUntil", (false, 0i64), true, true);
    row!("Rules", ("",), false, true);
    row!("SetRule", ("", "popups", "true"), false, true);
    row!("Settings", (), true, true);
    row!("SetSetting", ("sound", "true"), false, false);
    row!("ReportFullscreen", (true, false), true, false);
}

#[tokio::test]
async fn signals_reach_only_admitted_units() {
    let bus = Bus::start("unicast");
    let (_daemon, bar, center, app) = units(&bus, APP_CGROUP).await;
    let mut admitted_streams = Vec::new();
    for conn in [&bar, &center] {
        let proxy = private(conn).await;
        admitted_streams.push(proxy.receive_signal("Added").await.expect("subscribe"));
        let _: Vec<WireNotification> = proxy.call("List", &()).await.expect("List");
    }
    let mut to_app = private(&app).await.receive_signal("Added").await.expect("subscribe");
    let id = notify(&public(&app).await, 0, "s", "", &[], HashMap::new()).await;
    for stream in &mut admitted_streams {
        let (wire,): (WireNotification,) =
            stream.next().await.expect("signal").body().deserialize().expect("wire");
        assert_eq!(wire.id, id);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(300), to_app.next()).await.is_err(),
        "a unicast signal never reaches an unadmitted connection"
    );
}

#[tokio::test]
async fn the_control_center_may_mute_but_not_grant_bypass() {
    let bus = Bus::start("mute");
    let (_daemon, bar, center, _app) = units(&bus, APP_CGROUP).await;
    let bar_proxy = private(&bar).await;
    let mut changed = bar_proxy.receive_signal("RulesChanged").await.expect("subscribe");
    let _: Vec<WireNotification> = bar_proxy.call("List", &()).await.expect("List");
    let center_proxy = private(&center).await;
    center_proxy
        .call::<_, _, ()>("SetRule", &("org.example.Chat", "allowed", "false"))
        .await
        .expect("mute");
    let (app,): (String,) = changed.next().await.expect("signal").body().deserialize().expect("app");
    assert_eq!(app, "org.example.Chat");
    let err = center_proxy
        .call::<_, _, ()>("SetRule", &("org.example.Chat", "bypass_dnd", "true"))
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    let rules: HashMap<String, String> =
        center_proxy.call("Rules", &("org.example.Chat",)).await.expect("Rules");
    assert_eq!(rules.get("allowed").map(String::as_str), Some("false"));
    assert_eq!(rules.get("bypass_dnd").map(String::as_str), Some("false"));
}

#[tokio::test]
async fn the_history_survives_a_restart_of_the_daemon() {
    let bus = Bus::start("restart");
    let (_daemon, _bar, center, app) = units(&bus, APP_CGROUP).await;
    let sender = public(&app).await;
    let kept = notify(&sender, 0, "kept", "", &["default", "Open"], HashMap::new()).await;
    notify(&sender, 0, "gone", "", &[], HashMap::from([("transient", Value::from(true))])).await;
    private(&center)
        .await
        .call::<_, _, ()>("MarkRead", &(vec![kept],))
        .await
        .expect("MarkRead");
    let file = bus.dir.join("state/notifications.json");
    common::wait_for_file(&file, "\"read\":true").await;

    // A second bus and daemon on a copy of the file: a restart, and a new bus as after a reboot.
    let second = Bus::start("restart-second");
    fs::create_dir_all(second.dir.join("state")).expect("mkdir");
    fs::copy(&file, second.dir.join("state/notifications.json")).expect("copy");
    let (_daemon2, _bar2, center2, app2) = units(&second, APP_CGROUP).await;
    let history: Vec<WireNotification> =
        private(&center2).await.call("History", &()).await.expect("History");
    let seen: Vec<_> = history
        .iter()
        .map(|n| (n.id, n.summary.as_str(), n.read, n.actions_available))
        .collect();
    assert_eq!(seen, [(kept, "kept", true, false)], "on another bus the actions are unavailable");
    let next = notify(&public(&app2).await, 0, "next", "", &[], HashMap::new()).await;
    assert!(next > kept);
}

#[tokio::test]
async fn a_spoofed_desktop_entry_lands_in_other_and_does_not_pass_dnd() {
    let bus = Bus::start("spoof");
    fs::create_dir_all(bus.dir.join("config/apps")).expect("mkdir");
    fs::write(bus.dir.join("config/apps/org.example.Chat.conf"), "bypass_dnd=true\n").expect("rule");
    let (_daemon, bar, _center, app) = units(&bus, SESSION_CGROUP).await;
    let bar_proxy = private(&bar).await;
    let mut added = bar_proxy.receive_signal("Added").await.expect("subscribe");
    let _: Vec<WireNotification> = bar_proxy.call("List", &()).await.expect("List");
    bar_proxy.call::<_, _, ()>("SetDoNotDisturb", &(true,)).await.expect("dnd");
    let hints = HashMap::from([("desktop-entry", Value::from("org.example.Chat"))]);
    notify(&public(&app).await, 0, "s", "", &[], hints).await;
    let (wire,): (WireNotification,) =
        added.next().await.expect("signal").body().deserialize().expect("wire");
    assert_eq!(
        (wire.app_id.as_str(), wire.desktop_entry.as_str(), wire.popup),
        ("", "org.example.Chat", false)
    );
}

#[tokio::test]
async fn do_not_disturb_changed_reaches_both_units_with_its_reason() {
    let bus = Bus::start("dnd-changed");
    let (_daemon, bar, center, _app) = units(&bus, APP_CGROUP).await;
    let mut streams = Vec::new();
    for conn in [&bar, &center] {
        let proxy = private(conn).await;
        streams.push(proxy.receive_signal("DoNotDisturbChanged").await.expect("subscribe"));
        let _: Vec<WireNotification> = proxy.call("List", &()).await.expect("List");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    private(&center)
        .await
        .call::<_, _, ()>("SetDoNotDisturbUntil", &(true, now + 3600))
        .await
        .expect("set");
    for stream in &mut streams {
        let got: (bool, String, i64) =
            stream.next().await.expect("signal").body().deserialize().expect("args");
        assert_eq!(got, (true, "manual".to_owned(), now + 3600));
    }
}
```

`SESSION_CGROUP` in `tests/common/mod.rs` is `/user.slice/user-1000.slice/session-2.scope`, a sender in no application unit. The existing tests change with the interface in the same commit: `List` answers `a(wire)` (the do-not-disturb state moves to `DoNotDisturb`), `the_list_keeps_the_newest_hundred_and_says_so` becomes five hundred, and the capability list grows only in Tasks 13, 14 and 16.

- [ ] **Step 2:** Run `cargo test -p athanor-shelld` and `cargo test -p athanor-bar notices` — Expected: FAIL to compile.
- [ ] **Step 3:** Implement: `policy.rs`; the wire in `athanor-services`; `Admitted` and `admits`; the interface; Notify's pipeline; the paths and Landlock; the bar's mirror and calls (`List` on start and restart, `DoNotDisturb` for the switch, `DoNotDisturbChanged` instead of reading the switch from `List`; the bar's popover keeps listing what `List` returns, so nothing the user sees changes).
- [ ] **Step 4:** Run `cargo test -p athanor-shelld -p athanor-services -p athanor-bar -p athanor-unit` — Expected: PASS. Run `cargo tree -p athanor-shelld -e normal --prefix none | grep -cE '^(glib|gio|gtk4) v'` — Expected: `0`.
- [ ] **Step 5:** Run `forge/test/shell/shelld_e2e.py` and `notifications_e2e.py` in the rig as their headers say (`scene.sh`) — Expected: every line PASS; a check that asserted the old `List` signature is updated in the same commit.
- [ ] **Step 6:** Commit `feat(shelld): decide each notification by its rules and serve the center's interface`.

---

### Task 6: Fullscreen reported by the bar

**Files:**

- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/notifications.rs` (or a new `src/ui/fullscreen.rs` if the module passes 1 000 lines), `src/lib.rs` (pure helper)

**Interfaces:**

- Consumes: `athanor_compositor_client::{Client, Event, Window}` with `Window::state.fullscreen` and `state.activated` (`system/athanor-compositor-client/src/model.rs:17-20`); Task 5 `ReportFullscreen`.
- Produces: `pub fn fullscreen_active(windows: &[Window]) -> bool` in the bar's library (any window fullscreen and activated).

On every window event the bar computes `fullscreen_active` and calls `ReportFullscreen(true, active)` when the value changes and once after each `List`. When the client has no toplevel-info global (a compositor that withholds it), it calls `ReportFullscreen(false, false)` once.

- [ ] **Step 1:** Test `fullscreen_active` in the bar's library: none, one fullscreen not activated, one activated not fullscreen, one both → only the last is true.
- [ ] **Step 2:** Run `cargo test -p athanor-bar fullscreen` — Expected: FAIL.
- [ ] **Step 3:** Implement the helper and the calls.
- [ ] **Step 4:** Run `cargo test -p athanor-bar` — Expected: PASS. In the rig, `notifications_e2e.py` gains one check: a fullscreen window in the scene, then `DoNotDisturb()` answers `(true, "fullscreen", 0, …)`; leaving fullscreen answers `false`.
- [ ] **Step 5:** Commit `feat(bar): tell athanor-shelld when a fullscreen window has the focus`.

---

### Task 7: Low battery

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/battery.rs`
- Modify: `system/athanor-services/src/battery.rs` (`Battery` gains `warning_level: Option<u32>`, UPower's `WarningLevel`), shelld `src/server.rs` (spawn the model on the system bus)

**Interfaces:**

- Consumes: the control-center plan's Task 2 `athanor_services::battery::spawn(handle, buses, backlight_dir) -> (watch::Receiver<BatteryState>, mpsc::Sender<BatteryCommand>)`.
- Produces: `pub struct LowBattery { .. }`, `impl LowBattery { pub fn new() -> Self; pub fn observe(&mut self, battery: Option<&Battery>) -> Option<Level> }`, `pub enum Level { Low, Critical }`.

UPower's `WarningLevel`: 3 = low, 4 = critical, 5 = action; 1 = none, 2 = discharging. `observe` returns `Low` once when the level becomes 3, `Critical` once when it becomes 4 or 5, and re-arms both when the battery charges (`Charge` not discharging). `None` battery: nothing. The daemon then sends itself a critical notification: summary "Battery low" or "Battery critically low", body the remaining time when UPower gives one, action `power` whose invocation calls `os.athanor.ControlCenter1.Show("power")`.

- [ ] **Step 1:** Tests in `src/battery.rs`: 2→3→3→4→4 yields `Low`, then `Critical`, once each; charging then 3 again yields `Low` again; a `None` battery yields nothing. In `athanor-services`, the `testbus` UPower device with `WarningLevel = 3` publishes `warning_level == Some(3)`.
- [ ] **Step 2:** Run `cargo test -p athanor-shelld battery` and `cargo test -p athanor-services battery` — Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run both — Expected: PASS; the toolkit check of Task 5 still prints `0`.
- [ ] **Step 5:** Commit `feat(shelld): warn once per discharge when the battery runs low`.

---

### Task 8: Step 1 on the laptop

- [ ] **Step 1:** `python3 scripts/shell-bench/deploy.py --host athanor-ref athanor-shelld athanor-bar` — Expected: both units active, no warning in `journalctl --user -u athanor-shelld -b`.
- [ ] **Step 2:** On the laptop: `notify-send` twice, `systemctl --user restart athanor-shelld`, then read `~/.local/state/athanor/shelld/notifications.json` with `stat -c %a` (Expected: `600`) and `python3 -m json.tool` (Expected: both entries, no image). Reboot the laptop with the maintainer's consent, then `deploy.py` again (the overlay ends with the boot) and check the file is read: `journalctl --user -u athanor-shelld -b | grep -c restored` — Expected: `1`.
- [ ] **Step 3:** The maintainer uses the laptop for a day with the bar unchanged: Expected no visible change (NC16 step 1).
- [ ] **Step 4:** Ask the maintainer before pushing `notification-center` and opening the pull request for step 1 to `iso-v0`, with the e2e and unit results in its body.

---

## Step 2 — the panel

### Task 9: Two panels in the control center

**Files:**

- Modify: `forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/bus.rs` (`ToggleNotifications`, page ids), `src/surface.rs` (a `gtk4::Stack` with `controls` and `notifications`), `src/main.rs` (Super+N), `src/panel.rs` (the stack child)

**Interfaces:**

- Consumes: the control-center plan's Task 11: `os.athanor.ControlCenter1` with `Show(s page)`, `Toggle()`, property `Open: b`; `athanor_compositor_client::shortcuts::set_custom_binding_once(modifiers, key, command, marker)`.
- Produces: `ToggleNotifications()`; `Show("notifications")`, `Show("notifications:<id>")` (Task 14 focuses the reply field); `pub enum Page { Controls(String), Notifications(Option<u32>) }` and `pub fn parse_page(id: &str) -> Option<Page>`; `Open` true while either panel is shown; Super+N bound once with the marker `state/athanor/control-center/super-n-bound` to `busctl --user call os.athanor.ControlCenter1 /os/athanor/ControlCenter1 os.athanor.ControlCenter1 ToggleNotifications`.

Rules: `Toggle()` while the notification panel is open switches to the control panel, and the converse; a second call of the same toggle closes. At most one panel is visible (NC1). The notification panel is anchored at the bar's end edge (the start edge in right-to-left), 400 logical pixels wide, at most 80% of the output's height (NC11).

- [ ] **Step 1:** Tests: `parse_page` for `"notifications"`, `"notifications:42"`, `"notifications:x"` (None), `"network"`; a pure `toggle(current: Option<Panel>, asked: Panel) -> Option<Panel>` over the four cases; the panel height `min(content, 0.8 × output)`; the binding writer test of CC Task 11 repeated for Super+N.
- [ ] **Step 2:** Run `cargo test -p athanor-control-center` — Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p athanor-control-center` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(control-center): host the notification center beside the controls`.

---

### Task 10: Notifications model in `athanor-services`

**Files:**

- Modify: `system/athanor-services/src/notifications/mod.rs`, `src/testbus.rs` (a fake `os.athanor.Notifications1`)

**Interfaces:**

- Consumes: Task 5's wire and interface; the control-center plan's Task 1 `Buses`, `Runtime`.
- Produces:

```rust
pub struct Dnd { pub on: bool, pub reason: String, pub until: i64, pub unavailable: Vec<String> }
pub struct NotificationsState { pub available: bool, pub entries: Vec<WireNotification> /* newest last */,
                                pub dnd: Dnd, pub generation: u64 }
pub enum NotificationsCommand { Close(u32), Invoke { id: u32, key: String, token: String },
    Reply { id: u32, text: String }, MarkRead(Vec<u32>), ClearAll, ClearGroup(String),
    SetDnd { on: bool, until: Option<i64> }, Mute(String), ListOnly(String) }
pub fn spawn(handle: &Handle, buses: Buses) -> (watch::Receiver<NotificationsState>, mpsc::Sender<NotificationsCommand>);
```

The model calls `History` and `DoNotDisturb` when `org.freedesktop.Notifications` gets an owner, applies `Added`, `Replaced`, `Closed`, `Read`, `DoNotDisturbChanged` from that owner only, and publishes `available = false` with no entries when the owner leaves (NC11 "Notifications unavailable"). Signals that arrive while `History` is in flight are applied after it, in order. Calls carry the 5 s timeout of `mirror::TIMEOUT`; a refused command logs at warning and the state is not changed.

- [ ] **Step 1:** Tests on `testbus`: the fake answers `History` with two entries → state has both; `Added` → three; the fake's connection dropped → `available == false`, no entries; a new fake → reloaded; `Closed` arriving during a slow `History` (the fake delays its answer 200 ms) is applied after it.
- [ ] **Step 2:** Run `cargo test -p athanor-services notifications` — Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run it — Expected: PASS; the toolkit check prints `0` for `athanor-services`.
- [ ] **Step 5:** Commit `feat(services): follow athanor-shelld's notifications for the center`.

---

### Task 11: The notification panel

**Files:**

- Create: `forge/specs/athanor-control-center/athanor-control-center-1.0.0/src/notifications/mod.rs`, `group.rs` (pure), `text.rs` (pure), `row.rs`, `header.rs`, `calendar.rs`

**Interfaces:**

- Consumes: Task 9's stack and `Page`; Task 10's model; `athanor_compositor_client::Opener` to open an application through its desktop entry (BR2, the launch path every shell surface uses).
- Produces:

```rust
// group.rs
pub struct Group { pub app_id: String, pub newest: i64, pub rows: Vec<u32> /* newest first */ }
pub fn groups(entries: &[WireNotification]) -> Vec<Group>;   // newest group first
pub fn shown_rows(group: &Group, expanded: bool) -> &[u32];  // all when expanded or ≤ 3, else the newest one
// text.rs
pub fn relative_time(now: i64, then: i64) -> String;          // "now", "5 min", "2 h", "Yesterday", a date
pub fn dnd_words(dnd: &Dnd, local_until: Option<String>) -> String; // "On until 07:00 · schedule", "Off"
pub fn accessible_name(app: &str, summary: &str, body: &str, time: &str) -> String;
pub fn announcement(unread: usize) -> String;                 // "Notification center, 3 unread"
```

Contents (NC11): the header with the do-not-disturb switch and its words, the menu (one hour, until 08:00 computed as in CC plan Task 13, "Edit schedule" launching `cosmic-settings` until Settings exists) and "Clear all"; the list grouped by `app_id` (`""` = "Other applications", named by the row's `app_name`), each group with "Mute this application" (a confirmation dialog, then `Mute`), "Show in the list only" (`ListOnly`) and "Notification settings" (same target as "Edit schedule"); rows with icon or image, summary, relative time, body in two lines that expands, close, up to three actions and "More", the default action on click; actions shown insensitive when `actions_available` is false, and a click then opens the application through its desktop entry when `app_id` is set; a horizontal `GestureSwipe` on touch closes the row; "No notifications"; "Notifications unavailable" while `available` is false; below the list, `gtk4::Calendar` with `show-week-numbers` from a setting (off by default), opened on today each time the panel opens. Read state: when the panel is shown and on each change while shown, `MarkRead` with the ids of `shown_rows` of every group. Keyboard: Tab and arrows between groups and rows; Enter invokes `default`; Delete closes; Escape closes the panel. Screen reader: the panel announced with `announcement` on opening; each row's accessible label is `accessible_name`. Right-to-left mirrors with the widget direction, as the bar.

- [ ] **Step 1:** Tests for `group.rs` (three apps interleaved → three groups newest first; a group of four collapses to one shown row and expands to four; `""` groups as one) and `text.rs` (each `relative_time` band at its edges; `dnd_words` for manual with until, schedule, fullscreen, off; `announcement(0)` and `(3)`).
- [ ] **Step 2:** Run `cargo test -p athanor-control-center notifications` — Expected: FAIL.
- [ ] **Step 3:** Implement the pure modules, then the widgets.
- [ ] **Step 4:** Run `cargo test -p athanor-control-center` — Expected: PASS. Deploy `athanor-shelld athanor-bar athanor-control-center`; open the panel with `busctl --user call … ToggleNotifications` on the laptop; the maintainer judges it.
- [ ] **Step 5:** Commit `feat(control-center): show the notification history, do not disturb and the calendar`.

---

### Task 12: The bar hands over its list and calendar

**Files:**

- Modify: `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/notifications.rs` (delete the list popover, `fill`, `Inner`; the button calls `ToggleNotifications`; a badge with the unread count), `src/ui/clock.rs` (delete the calendar popover; a click calls `ToggleNotifications`), `src/ui/popups.rs` (a click on a popup marks it read)

**Interfaces:**

- Consumes: Task 5's `List` (unread), `Read`, `MarkRead`; Task 9's `ToggleNotifications`.
- Produces: the bar's button shows the unread count (F-bar-08) and its accessible name says it ("Notifications, 3 unread").

- [ ] **Step 1:** Tests in the bar's library: the badge text for 0 (hidden), 1–99, 100 and more ("99+"); the accessible name for 0, 1 and 3.
- [ ] **Step 2:** Run `cargo test -p athanor-bar` — Expected: FAIL.
- [ ] **Step 3:** Implement; delete the dead code, not comment it out.
- [ ] **Step 4:** Run `cargo test -p athanor-bar` — Expected: PASS. In the rig, `notifications_e2e.py`: the list popover checks become "the button calls `ToggleNotifications`" (a fake `os.athanor.ControlCenter1` in the scene records the call), "the clock calls `ToggleNotifications`", "the badge shows 2 after two `Notify`, 0 after `MarkRead`".
- [ ] **Step 5:** Deploy `athanor-shelld athanor-bar athanor-control-center`; the maintainer opens the center with the button, the clock and Super+N and judges it.
- [ ] **Step 6:** Commit `feat(bar): open the notification center from the button and the clock`. Ask the maintainer before pushing and opening the pull request for step 2.

---

## Step 3 — the richer notification

### Task 13: Markup and links

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/markup.rs`
- Modify: shelld `src/notifications.rs` (`CAPABILITIES` gains `body-markup`, `body-hyperlinks`; `body_spans` from `markup::parse`), `system/athanor-services/src/notifications/wire.rs` (`pub fn pango_markup(spans: &[(String, u32, String)]) -> String`), the bar's `src/ui/popups.rs` and `src/ui/notifications.rs` card, the center's `src/notifications/row.rs`

**Interfaces:**

- Produces: `pub fn parse(body: &str) -> (String /* plain */, Vec<(String, u32, String)>)`; `pango_markup` escapes every text with `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` and wraps it in `<b>`, `<i>`, `<u>` and `<a href="…">` from the style bits and href it was given, never from the sender's text.

`parse` keeps `<b>`, `<i>`, `<u>` and `<a href="…">` (nesting allowed; a closing tag closes that tag and every tag opened inside it; tags still open at the end are closed); an `href` is kept only when it starts with `https:`, `http:` or `mailto:` (case-insensitive scheme), else the link keeps its text and loses its target; `<img …>` and `<img …/>` vanish with their attributes; any other tag stays as literal text; the entities `&amp; &lt; &gt; &quot; &apos;` and numeric `&#…;` decode, any other `&…;` stays literal; decoded text then passes `athanor_unit::text`'s stripping of control and bidirectional characters, so `&#x202E;` cannot reorder the line. The plain form joins the span texts. The label's `activate-link` handler opens the URI with `gtk4::UriLauncher` on an explicit click or Enter only, and the link's tooltip shows the address.

- [ ] **Step 1:** Tests in `markup.rs`:

```rust
#[test]
fn only_the_allowed_tags_survive() {
    let (plain, spans) = parse("<b>Bold</b> <a href=\"https://x.org/a\">link</a> <a href=\"file:///etc/passwd\">file</a><img src=\"x\"/> <script>x</script> &amp; &nbsp;");
    assert_eq!(plain, "Bold link file <script>x</script> & &nbsp;");
    assert_eq!(spans[0], ("Bold".into(), 1, String::new()));
    assert!(spans.contains(&("link".into(), 0, "https://x.org/a".into())));
    assert!(spans.contains(&("file".into(), 0, String::new())), "a file: link loses its target");
}

#[test]
fn schemes_are_checked_whatever_their_case() {
    assert_eq!(parse("<a href=\"JavaScript:alert(1)\">x</a>").1, [("x".into(), 0, String::new())]);
    assert_eq!(parse("<a href=\"MAILTO:a@b\">m</a>").1, [("m".into(), 0, "MAILTO:a@b".into())]);
}

#[test]
fn an_entity_cannot_bring_back_a_stripped_character() {
    assert_eq!(parse("a&#x202E;b&#7;c&#65;").0, "abcA");
}

#[test]
fn nested_and_unclosed_tags_are_closed() {
    let (_, spans) = parse("<b>a<i>b</b>c");
    assert_eq!(spans, [("a".into(), 1, String::new()), ("b".into(), 3, String::new()), ("c".into(), 0, String::new())]);
}
```

And in `wire.rs`: `pango_markup(&[("<x> & \"y\"".into(), 1, "https://a/?b=1&c=2".into())])` equals `<a href="https://a/?b=1&amp;c=2"><b>&lt;x&gt; &amp; &quot;y&quot;</b></a>`.

- [ ] **Step 2:** Run `cargo test -p athanor-shelld markup` and `cargo test -p athanor-services wire` — Expected: FAIL.
- [ ] **Step 3:** Implement; both surfaces set the label's markup only from `pango_markup`.
- [ ] **Step 4:** Run `cargo test -p athanor-shelld -p athanor-services -p athanor-bar -p athanor-control-center` — Expected: PASS. Acceptance item 8 checked in the rig with `notify-send 'm' '<b>B</b> <a href="https://x.org">x</a> <a href="file:///etc">f</a><img src="/x"/>'` through AT-SPI: the text is "B x f" and one link.
- [ ] **Step 5:** Commit `feat(notifications): show bold, italic, underline and safe links in the body`.

---

### Task 14: Progress and inline reply

**Files:**

- Modify: shelld `src/hints.rs` (`value`, `x-kde-reply-placeholder-text`, `x-kde-reply-submit-button-text`, `x-kde-reply-submit-button-icon-name`, `sound-name`, `suppress-sound`), `src/notifications.rs` (`Reply`, the `NotificationReplied` signal, `CAPABILITIES` gains `inline-reply`), `src/store.rs` (`Content` gains `value: Option<u8>`, `reply: Option<Reply { placeholder, submit_text, submit_icon }>`), the bar's popups ("Reply" button calling `Show("notifications:<id>")`, a progress bar), the center's rows (progress bar, reply entry, focus by `Page::Notifications(Some(id))`)

**Interfaces:**

- KDE's names, read on 2026-10-05 in plasma-workspace `libnotificationmanager/server_p.cpp` and `notification.cpp` (N2, resolved): capability `inline-reply`; the action key `inline-reply`, whose label is the reply button's text; hints `x-kde-reply-placeholder-text`, `x-kde-reply-submit-button-text`, `x-kde-reply-submit-button-icon-name`; the signal `NotificationReplied(u id, s text)` on `org.freedesktop.Notifications`, emitted as a targeted signal to the notification's sender (`QDBusMessage::createTargetedSignal`).
- Produces: `Reply(u id, s text)` emits `NotificationReplied` to the sender's unique name only, closes the notification as dismissed unless it is resident, and never writes the text to the store or the history; it fails with `InvalidArgs` when the notification declared no `inline-reply` action or its sender has gone. `value` outside 0–100 is ignored. A `Replaced` that changes only `value` neither pops up again nor plays a sound (`popup = false`, `sound = false` for that update).

- [ ] **Step 1:** Tests: `hints` decodes the four new hints, a `value` of 150 is `None`; on the private bus, `Reply` from the control center delivers `NotificationReplied(id, "hi")` to the sending connection and not to a third connection with a match rule, then the history file contains no `"hi"`; a replace with a new `value` keeps the popup state and plays nothing (assert on the decision recorded in the `Replaced` wire, `popup == false`).
- [ ] **Step 2:** Run `cargo test -p athanor-shelld` — Expected: FAIL.
- [ ] **Step 3:** Implement; the reply entry in the center sends `Reply`, and the popup's "Reply" opens `notifications:<id>` because a popup never takes the keyboard (BR4).
- [ ] **Step 4:** Run `cargo test -p athanor-shelld -p athanor-bar -p athanor-control-center` — Expected: PASS.
- [ ] **Step 5:** Commit `feat(notifications): show progress and answer inline replies`.

---

### Task 15: Spike N1 — sound playback

**Files:** `/var/tmp/sound-spike/` only (throwaway), then `docs/superpowers/plans/2026-10-05-notification-center.md` (the decision recorded under this task).

- [ ] **Step 1:** On the laptop, with nothing else measured, play `/usr/share/sounds/freedesktop/stereo/message-new-instant.oga` 100 times, 1 s apart, through each candidate: (a) `libcanberra` through its PulseAudio backend, called by FFI from a 20-line Rust test binary; (b) `pw-play`; (c) a decoder (`lewton`) writing to a PipeWire stream (`pipewire` crate). Record for each: added PSS of the daemon (`smem` or `/proc/<pid>/smaps_rollup`), latency from call to the stream's first buffer (`pw-top` or the stream's state change), CPU over the 100 plays, new long-lived processes (Expected: none for all three), new dependencies.
- [ ] **Step 2:** Write the table into this task with a recommendation and give it to the maintainer, who chooses. Task 16 starts only after that choice.
- [ ] **Step 3:** Commit `docs(shell): record the sound playback spike and the maintainer's choice`.

**Spike result (2026-10-05, reference laptop, Fedora 43 image, PipeWire 1.4.11 with pipewire-pulse, idle, `message-new-instant.oga` 1.03 s, 100 plays 1 s apart, stream volume -30 dB, default sink untouched).** Each candidate ran as one process that stays alive across the 100 plays. "Added PSS" is `Pss` from `/proc/<pid>/smaps_rollup` after the last play minus before the first play, with the player already initialised (context created, PipeWire thread connected); the figure in brackets is against process start. Latency is the time from the call to the stream node reaching `Running` in the PipeWire registry, observed by a separate monitor process on the same monotonic clock. Raw numbers: `/var/tmp/sound-spike/raw/` on the desktop, report in `.superpowers/sdd/2026-10-05-notification-center/task-15-report.md`.

| | (a) libcanberra, pulse backend, FFI | (b) `pw-play` child process | (c) `pipewire` crate + `lewton` |
|---|---|---|---|
| Added PSS of the daemon | +836 KB (+2207 KB); +525 KB creep between play 1 and 100 | +16 KB; child is 3.2 MB PSS for about 1.1 s per play, then gone | +881 KB (+1603 KB); +340 KB creep between play 1 and 100 |
| Latency to first buffer, median (p95, first play) | 20.3 ms (21.7, 87.8) | 24.9 ms (28.2, 29.3) | 22.2 ms (26.3, 88.2); 26.3 ms median in-process, of which decoding 10 ms |
| CPU in the daemon process over 100 plays | 1.18 s | 0.09 s, plus 2.70 s in the 100 children | 2.80 s |
| CPU added in pipewire / pipewire-pulse / wireplumber | 1.23 s / 1.28 s / 3.57 s | 1.00 s / 0.10 s / 3.11 s | 3.84 s / 0.30 s / 2.00 s |
| New long-lived processes | none; 1 extra thread (libpulse mainloop) | none; one short-lived child per play | none; 3 extra threads (PipeWire loop and data loop) |
| New dependencies | libcanberra (+ libpulse, libvorbisfile, libtdb, libltdl), all already in the image; no crate (about 20 lines of FFI) | none: `pw-play` (pipewire-utils) is in the image; `std::process` | crates `pipewire` 0.9, `libspa`, `libspa-sys`, `pipewire-sys`, `lewton`, `ogg` plus transitive (about 25 at run time; `libc`, `nix`, `thiserror`, `bitflags` are probably in the workspace already); build needs `pipewire-devel` and clang (bindgen); `libpipewire-0.3` is in the image |

**Recommendation: (b) `pw-play`.** It costs the daemon nothing resident (+16 KB against +0.8 to +0.9 MB), adds no crate, no C binding and no `unsafe`, and its 25 ms latency is imperceptible for a notification sound; the transient 3.2 MB and 27 ms CPU per play are paid only when a sound plays and are returned at once. It has the least code to keep (a `Command` and its exit status, so a failure is logged once from the status), and it is the only candidate whose cost does not grow with the number of plays inside the daemon. Choose (a) only if the daemon should look sounds up by event name through libcanberra (theme lookup, caching) and accept a libpulse thread, +0.8 MB and a +0.5 MB creep per 100 plays. Choose (c) only if a child per sound is refused; it has the highest CPU (decoding every time, a stream per play, the highest server-side load) and the largest dependency surface.

**Caveats.** (1) The creep of (a) and (c) was seen over 100 plays only and was not followed to a plateau. (2) Per-play CPU of (b) is the children's CPU; its 3.2 MB is the whole `pw-play` process, much of it shared libraries. (3) The first play after idle costs about 88 ms for (a) and (c) (the sink wakes); for (b) the first play is 29 ms (no such outlier seen). (4) Task 16 must let the daemon's sandbox exec `pw-play` and reach the PipeWire socket if (b) is chosen. (5) Sound is resolved to a path in all three runs; libcanberra would also accept an event id, which was not measured. (6) The `Running` state is a proxy for the first buffer: the stream's own first-buffer callback in (c) arrives within a few ms of it.

**Maintainer's choice (2026-10-05): (b) `pw-play`.** Task 16 spawns `pw-play` per sound; the daemon's sandbox allows executing it and reaching the PipeWire socket.

---

### Task 16: Sound

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/sound.rs`
- Modify: shelld `src/notifications.rs` (play when `Decision::sound`), `CAPABILITIES` gains `sound`, `data/athanor-shelld.service` if the chosen playback needs a socket or an executable not yet allowed

**Interfaces:**

- Produces: `pub fn resolve(data_dirs: &[PathBuf], theme: &str, name: &str) -> Option<PathBuf>` (freedesktop Sound Theme: `<dir>/sounds/<theme>/stereo/<name>.{oga,ogg,wav}`, then the theme's `Inherits=` from `index.theme`, then `freedesktop`; a name with `/` or starting with `.` is refused); `pub fn name_for<'a>(urgency: Urgency, sound_name: Option<&'a str>) -> &'a str` (`message-new-instant`, `dialog-warning` for critical, the hint when it resolves); `pub fn play(path: &Path)` with the body Task 15 chose, never blocking `Notify` (spawned on the runtime), a failure logged once.

- [ ] **Step 1:** Tests for `resolve` (a fake theme tree with inheritance; `../x` refused; `.wav` found when `.oga` is absent) and `name_for`. The `sound-file` hint is never read: a test that a `Notify` with `sound-file=/etc/shadow` resolves to the theme's sound.
- [ ] **Step 2:** Run `cargo test -p athanor-shelld sound` — Expected: FAIL.
- [ ] **Step 3:** Implement with the chosen playback.
- [ ] **Step 4:** Run `cargo test -p athanor-shelld` — Expected: PASS. Deploy; the maintainer hears a normal notification, silence under do not disturb, a critical one through it, and silence for an application set to `sound=false` (acceptance item 10).
- [ ] **Step 5:** Commit `feat(shelld): play the theme's sound for a notification`.

---

### Task 17: Popups — corner, overlay, private, rate limit

**Files:**

- Create: `forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/rate.rs`
- Modify: shelld `src/notifications.rs` (`rate_limited` into `Facts`), the bar's `src/ui/popups.rs` (corner, layer, private content), `src/ui/notifications.rs` (calls `Settings` after each `List` and on `SettingsChanged`, both from Task 5, and keeps `popup_corner`, `private_popups` and `trigger_fullscreen`)

**Interfaces:**

- Produces: `pub struct RateLimit`; `impl RateLimit { pub fn admit(&mut self, key: &str, now_ms: u64) -> bool }` — false for the 21st and later notification of one key within 10 s, one warning line per burst; keys are the application id, or `app_name` for `Other`.

- [ ] **Step 0: Check N6 in the dev VM.** A layer-shell surface on `Layer::Overlay` above a fullscreen `mpv` window on cosmic-comp 1.8: take a screenshot with the scene tools. Expected: the popup is visible. If not, stop and return F-notif-07 to the maintainer; the rest of the task proceeds without the overlay.
- [ ] **Step 1:** Tests: `RateLimit` admits 20 in 10 s, refuses the 21st, admits again after the window slides; two keys are independent. On the private bus: 21 `Notify` in a burst → the 21st `Added` has `popup == false`, `History` holds 21 (acceptance item 11). In the bar: a pure `corner_edges(corner, panel_edge, rtl) -> (Edge, Edge)` for the five corners, and `private_card(app_name) -> (String, String)` showing only the name.
- [ ] **Step 2:** Run `cargo test -p athanor-shelld rate` and `cargo test -p athanor-bar popups` — Expected: FAIL.
- [ ] **Step 3:** Implement; the popup window uses `Layer::Overlay` when `trigger_fullscreen` is false, `Layer::Top` otherwise.
- [ ] **Step 4:** Run `cargo test -p athanor-shelld -p athanor-bar` — Expected: PASS. Deploy; the maintainer judges the four corners and private popups.
- [ ] **Step 5:** Commit `feat(notifications): place popups at the chosen corner, keep them private and slow down floods`. Ask the maintainer before pushing and opening the pull request for step 3.

---

## Step 5 — the gate of the standard

### Task 18: The gate

**Files:**

- Modify: `forge/test/shell/cases.py` (surface `notification-center`: the four scenes of NC15), `forge/test/shell/notifications_e2e.py`, `forge/test/shell/atspi_check.py` (row names), `docs/shell-bench/` (the results), the control center's preset only after the maintainer's decision

- [ ] **Step 1: Surface cases.** Four scenes — the panel with 500 entries, the panel empty, the panel with `athanor-shelld` stopped, popups with markup, progress and reply — each over SH13's 12 cases from `cases.py notification-center`. Expected: 48 goldens, each judged once by the maintainer before it becomes a golden.
- [ ] **Step 2: End to end in the dev VM** (NC15): `notify-send` produces a row; a fullscreen window gives `(true, "fullscreen", …)`; Super+N opens the panel; Orca's tree (`atspi_check.py`) carries the row names of `accessible_name`. Add the run to the workflow that runs on each cosmic-comp update.
- [ ] **Step 3: Measurement on the laptop** with `scripts/shell-bench/bench.py` and nothing else running: the first complete frame of the panel within 100 ms of the input at p95, for the button, the clock and Super+N (acceptance item 1); `athanor-shelld` PSS at rest with 500 entries ≤ 16 MB; the control center with both panels ≤ 64 MB. A miss goes to the maintainer (N4) before anything else.
- [ ] **Step 4: Languages and signature.** English, German and the right-to-left pseudo-language through `cases.py`; the aesthetic signature of ST8 judged by the maintainer on the laptop.
- [ ] **Step 5: Recovery.** Kill `athanor-shelld`: the panel shows "Notifications unavailable"; within 1 s of its return the same list and unread count (acceptance item 13).
- [ ] **Step 6: Soak.** The 24-hour soak of ST5 with bursts of notifications, run by `soak.py` with the laptop untouched.
- [ ] **Step 7:** With every step green and the maintainer's decision, enable the panel in the preset as the control-center plan's Task 14 says, and commit `build(control-center): enable the notification center`.

---

## Acceptance for this plan

Section 6 of the spec, item by item:

| Item | Where it is proved                                                                         |
| ---- | ------------------------------------------------------------------------------------------ |
| 1    | Task 18 Step 3                                                                             |
| 2    | Task 3 tests, Task 5 restart test, Task 8 Step 2                                           |
| 3    | Task 3 tests (mode, no image), Task 14 (no reply text), Task 5 (`ClearAll` writes at once) |
| 4    | Task 5 admission tests                                                                     |
| 5    | Task 5 `a_spoofed_desktop_entry_lands_in_other_and_does_not_pass_dnd`                      |
| 6    | Task 4 tests; on the laptop across a suspend in Task 18 Step 6                             |
| 7    | Task 6 rig check, Task 18 Step 2                                                           |
| 8    | Task 13                                                                                    |
| 9    | Task 14                                                                                    |
| 10   | Task 5 decision table, Task 16 on the laptop                                               |
| 11   | Task 17                                                                                    |
| 12   | Task 7 tests; on the laptop when the battery falls past 20%                                |
| 13   | Task 18 Step 5                                                                             |
| 14   | Task 18 Steps 1–4                                                                          |

And: `cargo test -p athanor-shelld -p athanor-services -p athanor-bar -p athanor-control-center -p athanor-unit` passes; the toolkit check prints `0` for `athanor-shelld` and `athanor-services`; `python3 scripts/verify.py` shows no new failure against BASE.
