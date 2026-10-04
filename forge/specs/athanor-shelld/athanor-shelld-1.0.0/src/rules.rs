//! Per-application notification rules and the notification settings, small `key=value` files
//! under `$XDG_CONFIG_HOME/athanor/notifications/`: `notifications.conf`, `other.conf` (the
//! applications we cannot identify) and `apps/<id>.conf`. A line that does not parse costs
//! its key only: the default stands and the line is reported. Writes keep the file's other
//! lines and go through a temporary file and a rename.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc::Sender;
use tracing::warn;

use crate::hints::is_desktop_id;
use crate::identity::Identity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockScreen {
    All,
    Name,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeout {
    App,
    Seconds(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub allowed: bool,
    pub popups: bool,
    pub bypass_dnd: bool,
    pub lock_screen: LockScreen,
    pub sound: bool,
    pub timeout: Timeout,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            allowed: true,
            popups: true,
            bypass_dnd: false,
            lock_screen: LockScreen::Name,
            sound: true,
            timeout: Timeout::App,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    Days(u32),
    UntilCleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    Bar,
    TopStart,
    TopEnd,
    BottomStart,
    BottomEnd,
}

/// Minutes since midnight; `start_min > end_min` crosses midnight. `days` is Monday first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start_min: u16,
    pub end_min: u16,
    pub days: [bool; 7],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub retention: Retention,
    pub sound: bool,
    pub popup_corner: Corner,
    pub private_popups: bool,
    pub timeout_low_s: u32,
    pub timeout_normal_s: u32,
    pub schedule: Option<Window>,
    pub trigger_schedule: bool,
    pub trigger_fullscreen: bool,
    pub trigger_screen_sharing: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            retention: Retention::Days(7),
            sound: true,
            popup_corner: Corner::Bar,
            private_popups: false,
            timeout_low_s: 5,
            timeout_normal_s: 5,
            schedule: None,
            trigger_schedule: true,
            trigger_fullscreen: true,
            trigger_screen_sharing: true,
        }
    }
}

pub const RULE_KEYS: [&str; 6] = [
    "allowed",
    "popups",
    "bypass_dnd",
    "lock_screen",
    "sound",
    "timeout",
];
pub const SETTING_KEYS: [&str; 11] = [
    "retention",
    "sound",
    "popup_corner",
    "private_popups",
    "timeout_low",
    "timeout_normal",
    "schedule",
    "schedule_days",
    "trigger_schedule",
    "trigger_fullscreen",
    "trigger_screen_sharing",
];

const MAX_WARNINGS: usize = 16;

/// Keep the first warnings and one line for the rest: a junk file must not cost a journal flood.
fn note(warnings: &mut Vec<String>, message: String) {
    match warnings.len().cmp(&MAX_WARNINGS) {
        std::cmp::Ordering::Less => warnings.push(message),
        std::cmp::Ordering::Equal => warnings.push("further warnings suppressed".to_owned()),
        std::cmp::Ordering::Greater => {}
    }
}

fn boolean(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// A whole number of seconds, 1 to 3600.
fn seconds(value: &str) -> Option<u32> {
    value.parse().ok().filter(|s| (1..=3600).contains(s))
}

fn clock(value: &str) -> Option<u16> {
    let (hours, minutes) = value.split_once(':')?;
    let digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(hours) || !digits(minutes) {
        return None;
    }
    let (hours, minutes): (u16, u16) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

fn times(value: &str) -> Option<(u16, u16)> {
    let (start, end) = value.split_once('-')?;
    let (start, end) = (clock(start.trim())?, clock(end.trim())?);
    (start != end).then_some((start, end))
}

fn days(value: &str) -> Option<[bool; 7]> {
    const NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let mut chosen = [false; 7];
    for name in value.split(',') {
        chosen[NAMES.iter().position(|n| *n == name.trim())?] = true;
    }
    Some(chosen)
}

/// The `key=value` lines of a file, comments and blanks dropped; a line without `=` is
/// `Err(line)`.
fn lines(text: &str) -> impl Iterator<Item = Result<(&str, &str), &str>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .ok_or(l)
        })
}

pub fn parse_rule(text: &str) -> (Rule, Vec<String>) {
    let mut rule = Rule::default();
    let mut warnings = Vec::new();
    for line in lines(text) {
        let (key, value) = match line {
            Ok(pair) => pair,
            Err(bad) => {
                note(&mut warnings, format!("no `=` in `{bad}`"));
                continue;
            }
        };
        let ok = match key {
            "allowed" => boolean(value).map(|b| rule.allowed = b),
            "popups" => boolean(value).map(|b| rule.popups = b),
            "bypass_dnd" => boolean(value).map(|b| rule.bypass_dnd = b),
            "sound" => boolean(value).map(|b| rule.sound = b),
            "lock_screen" => match value {
                "all" => Some(LockScreen::All),
                "name" => Some(LockScreen::Name),
                "none" => Some(LockScreen::None),
                _ => None,
            }
            .map(|l| rule.lock_screen = l),
            "timeout" => match value {
                "app" => Some(Timeout::App),
                _ => seconds(value).map(Timeout::Seconds),
            }
            .map(|t| rule.timeout = t),
            _ => {
                note(&mut warnings, format!("unknown key `{key}`"));
                continue;
            }
        };
        if ok.is_none() {
            note(&mut warnings, format!("bad value `{value}` for `{key}`"));
        }
    }
    (rule, warnings)
}

pub fn parse_settings(text: &str) -> (Settings, Vec<String>) {
    let mut settings = Settings::default();
    let mut warnings = Vec::new();
    let (mut span, mut chosen_days) = (None, None);
    for line in lines(text) {
        let (key, value) = match line {
            Ok(pair) => pair,
            Err(bad) => {
                note(&mut warnings, format!("no `=` in `{bad}`"));
                continue;
            }
        };
        let ok = match key {
            "retention" => match value {
                "forever" => Some(Retention::UntilCleared),
                "1" | "7" | "30" => value.parse().ok().map(Retention::Days),
                _ => None,
            }
            .map(|r| settings.retention = r),
            "sound" => boolean(value).map(|b| settings.sound = b),
            "popup_corner" => match value {
                "bar" => Some(Corner::Bar),
                "top-start" => Some(Corner::TopStart),
                "top-end" => Some(Corner::TopEnd),
                "bottom-start" => Some(Corner::BottomStart),
                "bottom-end" => Some(Corner::BottomEnd),
                _ => None,
            }
            .map(|c| settings.popup_corner = c),
            "private_popups" => boolean(value).map(|b| settings.private_popups = b),
            "timeout_low" => seconds(value).map(|s| settings.timeout_low_s = s),
            "timeout_normal" => seconds(value).map(|s| settings.timeout_normal_s = s),
            "schedule" => times(value).map(|t| span = Some(t)),
            "schedule_days" => days(value).map(|d| chosen_days = Some(d)),
            "trigger_schedule" => boolean(value).map(|b| settings.trigger_schedule = b),
            "trigger_fullscreen" => boolean(value).map(|b| settings.trigger_fullscreen = b),
            "trigger_screen_sharing" => boolean(value).map(|b| settings.trigger_screen_sharing = b),
            _ => {
                note(&mut warnings, format!("unknown key `{key}`"));
                continue;
            }
        };
        if ok.is_none() {
            note(&mut warnings, format!("bad value `{value}` for `{key}`"));
        }
    }
    // Days alone do not make a window; a window without days is every day.
    settings.schedule = span.map(|(start_min, end_min)| Window {
        start_min,
        end_min,
        days: chosen_days.unwrap_or([true; 7]),
    });
    (settings, warnings)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Changed {
    /// The application id; `""` is `Other`.
    Rule(String),
    Settings,
}

#[derive(Debug)]
pub enum RuleError {
    BadApp,
    BadKey,
    BadValue,
    Io(io::Error),
}

impl From<io::Error> for RuleError {
    fn from(err: io::Error) -> Self {
        RuleError::Io(err)
    }
}

const SETTINGS_FILE: &str = "notifications.conf";
const OTHER_FILE: &str = "other.conf";
const APPS_DIR: &str = "apps";

pub struct Rules {
    dir: PathBuf,
    rules: HashMap<String, Rule>,
    settings: Option<Settings>,
}

impl Rules {
    #[must_use]
    pub fn new(dir: PathBuf) -> Rules {
        Rules {
            dir,
            rules: HashMap::new(),
            settings: None,
        }
    }

    fn rule_path(&self, app: &str) -> PathBuf {
        if app.is_empty() {
            self.dir.join(OTHER_FILE)
        } else {
            self.dir.join(APPS_DIR).join(format!("{app}.conf"))
        }
    }

    pub fn rule(&mut self, identity: &Identity) -> Rule {
        let key = identity.key();
        if let Some(rule) = self.rules.get(key) {
            return rule.clone();
        }
        let path = self.rule_path(key);
        let (rule, warnings) = parse_rule(&read(&path));
        report(&path, &warnings);
        self.rules.insert(key.to_owned(), rule.clone());
        rule
    }

    pub fn settings(&mut self) -> Settings {
        if let Some(settings) = &self.settings {
            return settings.clone();
        }
        let path = self.dir.join(SETTINGS_FILE);
        let (settings, warnings) = parse_settings(&read(&path));
        report(&path, &warnings);
        self.settings = Some(settings.clone());
        settings
    }

    pub fn set_rule(&mut self, app: &str, key: &str, value: &str) -> Result<(), RuleError> {
        if !app.is_empty() && !is_desktop_id(app) {
            return Err(RuleError::BadApp);
        }
        if !RULE_KEYS.contains(&key) {
            return Err(RuleError::BadKey);
        }
        if !one_line(value) || !parse_rule(&format!("{key}={value}")).1.is_empty() {
            return Err(RuleError::BadValue);
        }
        update(&self.rule_path(app), key, value)?;
        self.rules.remove(app);
        Ok(())
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), RuleError> {
        if !SETTING_KEYS.contains(&key) {
            return Err(RuleError::BadKey);
        }
        if !one_line(value) || !parse_settings(&format!("{key}={value}")).1.is_empty() {
            return Err(RuleError::BadValue);
        }
        update(&self.dir.join(SETTINGS_FILE), key, value)?;
        self.settings = None;
        Ok(())
    }

    /// `relative` is a path under the configuration directory, as `watch` sends it.
    pub fn invalidate(&mut self, relative: &Path) -> Option<Changed> {
        let mut parts = relative.iter().map(OsStr::to_str);
        let changed = match (parts.next()?, parts.next(), parts.next()) {
            (Some(SETTINGS_FILE), None, _) => Changed::Settings,
            (Some(OTHER_FILE), None, _) => Changed::Rule(String::new()),
            (Some(APPS_DIR), Some(Some(file)), None) => {
                let id = file.strip_suffix(".conf").filter(|id| is_desktop_id(id))?;
                Changed::Rule(id.to_owned())
            }
            _ => return None,
        };
        match &changed {
            Changed::Settings => self.settings = None,
            Changed::Rule(app) => {
                self.rules.remove(app);
            }
        }
        Some(changed)
    }
}

const READ_LIMIT: usize = 64 * 1024;

fn one_line(value: &str) -> bool {
    !value.contains(['\n', '\r'])
}

/// A missing file is every default; a file over the limit or not UTF-8 is an error.
fn read_text(path: &Path) -> io::Result<String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(err),
    };
    let mut bytes = Vec::new();
    file.take(READ_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > READ_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

/// For reading rules: any failure is every default, with one warning.
fn read(path: &Path) -> String {
    read_text(path).unwrap_or_else(|err| {
        warn!(path = %path.display(), %err, "notification rules unreadable, using defaults");
        String::new()
    })
}

fn report(path: &Path, warnings: &[String]) {
    for warning in warnings {
        warn!(path = %path.display(), "{warning}");
    }
}

/// Set `key` in the file at `path`, keeping its other lines, through a temporary file in the
/// same directory and a rename.
fn update(path: &Path, key: &str, value: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    fs::create_dir_all(parent)?;
    let new = format!("{key}={value}");
    let mut out = String::new();
    let mut done = false;
    let current = read_text(path)?;
    for line in current.lines() {
        if line.split_once('=').map(|(k, _)| k.trim()) == Some(key) {
            if done {
                continue; // a repeated key: the first line took the new value
            }
            out.push_str(&new);
            done = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !done {
        out.push_str(&new);
        out.push('\n');
    }
    write_atomic(path, out.as_bytes())
}

/// Write `contents` to `path` (mode 0600) through a temporary file in the same directory and a
/// rename; the parent must exist.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    // Unique, exclusive and never followed: a planted link cannot redirect the write. The name
    // does not end in `.conf`, so the watch ignores it. A link at the final name is replaced by
    // the rename, never written through.
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let tmp = parent.join(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(contents)
                .and_then(|()| file.sync_all())
                .and_then(|()| fs::rename(&tmp, path))
        });
    if written.is_err() {
        fs::remove_file(&tmp).ok(); // best effort: the write error is what matters
    }
    written
}

/// nix's `Inotify` hands out its descriptor as `AsFd` only; `AsyncFd` wants `AsRawFd`.
struct Queue(Inotify);

impl AsRawFd for Queue {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_fd().as_raw_fd()
    }
}

/// Send each changed `*.conf` under `dir` and `dir/apps`, relative to `dir`, until the
/// receiver is dropped.
pub async fn watch(dir: PathBuf, changed: Sender<PathBuf>) -> io::Result<()> {
    fs::create_dir_all(dir.join(APPS_DIR))?;
    let inotify = Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC)?;
    let flags =
        AddWatchFlags::IN_CLOSE_WRITE | AddWatchFlags::IN_MOVED_TO | AddWatchFlags::IN_DELETE;
    let root = inotify.add_watch(&dir, flags)?;
    let apps = inotify.add_watch(&dir.join(APPS_DIR), flags)?;
    let ready = AsyncFd::new(Queue(inotify))?;
    loop {
        let mut guard = ready.readable().await?;
        let events = match ready.get_ref().0.read_events() {
            Ok(events) => events,
            Err(Errno::EAGAIN) => {
                guard.clear_ready();
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        for event in events {
            let Some(name) = event.name.filter(|n| {
                n.to_str()
                    .is_some_and(|n| n.ends_with(".conf") && !n.starts_with('.'))
            }) else {
                continue;
            };
            let relative = if event.wd == root {
                PathBuf::from(name)
            } else if event.wd == apps {
                Path::new(APPS_DIR).join(name)
            } else {
                continue;
            };
            if changed.send(relative).await.is_err() {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_every_default() {
        assert_eq!(parse_rule(""), (Rule::default(), vec![]));
        assert_eq!(parse_settings(""), (Settings::default(), vec![]));
    }

    #[test]
    fn a_bad_line_costs_only_its_key() {
        let (rule, warnings) =
            parse_rule("allowed=false\npopups=maybe\ncolour=red\n# note\n\nsound = false\n");
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
        assert_eq!(
            parse_rule("lock_screen=none").0.lock_screen,
            LockScreen::None
        );
    }

    #[test]
    fn a_schedule_crosses_midnight_on_chosen_days() {
        let (s, w) =
            parse_settings("schedule=22:00-07:00\nschedule_days=mon,fri\nretention=forever\n");
        assert!(w.is_empty(), "{w:?}");
        let window = s.schedule.expect("window");
        assert_eq!((window.start_min, window.end_min), (22 * 60, 7 * 60));
        assert_eq!(window.days, [true, false, false, false, true, false, false]);
        assert_eq!(s.retention, Retention::UntilCleared);
        for bad in [
            "schedule=07:00-07:00",
            "schedule=25:00-07:00",
            "schedule=7-8",
            "schedule_days=",
        ] {
            assert_eq!(parse_settings(bad).1.len(), 1, "{bad}");
        }
    }

    #[test]
    fn set_rule_keeps_other_lines_and_refuses_paths() {
        let dir = std::env::temp_dir().join(format!("athanor-shelld-rules-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("apps")).expect("mkdir");
        std::fs::write(
            dir.join("apps/org.example.Chat.conf"),
            "# mine\nsound=false\n",
        )
        .expect("seed");
        let mut rules = Rules::new(dir.clone());
        rules
            .set_rule("org.example.Chat", "allowed", "false")
            .expect("set");
        let text = std::fs::read_to_string(dir.join("apps/org.example.Chat.conf")).expect("read");
        assert_eq!(text, "# mine\nsound=false\nallowed=false\n");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(dir.join("apps/org.example.Chat.conf"))
                .expect("meta")
                .permissions(),
        );
        assert_eq!(mode & 0o777, 0o600);
        for app in ["../../.bashrc", "a/b", ".hidden", "other.conf/../../x"] {
            assert!(
                matches!(
                    rules.set_rule(app, "allowed", "false"),
                    Err(RuleError::BadApp)
                ),
                "{app}"
            );
        }
        assert!(matches!(
            rules.set_rule("org.example.Chat", "colour", "red"),
            Err(RuleError::BadKey)
        ));
        assert!(matches!(
            rules.set_rule("org.example.Chat", "allowed", "maybe"),
            Err(RuleError::BadValue)
        ));
        assert_eq!(
            std::fs::read_dir(&dir).expect("ls").count(),
            1,
            "nothing written outside apps/"
        );
        rules.set_rule("", "popups", "false").expect("other");
        assert_eq!(
            std::fs::read_to_string(dir.join("other.conf")).expect("read"),
            "popups=false\n"
        );
        rules
            .set_rule("other", "popups", "true")
            .expect("an application named other");
        assert!(dir.join("apps/other.conf").exists());
        assert!(
            !rules.rule(&Identity::Other).popups,
            "Other's rule is not the application's"
        );
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
        let path = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("event")
            .expect("path");
        assert_eq!(path, Path::new("apps/org.example.Chat.conf"));
        assert_eq!(
            rules.invalidate(&path),
            Some(Changed::Rule("org.example.Chat".into()))
        );
        assert!(!rules.rule(&chat).allowed);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("athanor-shelld-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("apps")).expect("mkdir");
        dir
    }

    #[test]
    fn planted_links_are_never_written_through() {
        use std::os::unix::fs::symlink;
        let dir = scratch("links");
        let victim = dir.join("victim");
        fs::write(&victim, "precious\n").expect("victim");
        symlink(&victim, dir.join("apps/.org.example.Chat.conf.tmp")).expect("old temp name");
        symlink(&victim, dir.join("apps/org.example.Chat.conf")).expect("final name");
        let mut rules = Rules::new(dir.clone());
        rules
            .set_rule("org.example.Chat", "allowed", "false")
            .expect("set");
        assert_eq!(fs::read_to_string(&victim).expect("victim"), "precious\n");
        let target = dir.join("apps/org.example.Chat.conf");
        assert!(!fs::symlink_metadata(&target)
            .expect("meta")
            .file_type()
            .is_symlink());
        assert!(fs::read_to_string(&target)
            .expect("read")
            .contains("allowed=false\n"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn an_unreadable_file_is_refused_not_overwritten() {
        let dir = scratch("invalid");
        let file = dir.join("apps/org.example.Chat.conf");
        let bytes = b"# mine\nsound=false\n\xff";
        fs::write(&file, bytes).expect("seed");
        let mut rules = Rules::new(dir.clone());
        assert!(matches!(
            rules.set_rule("org.example.Chat", "allowed", "false"),
            Err(RuleError::Io(_))
        ));
        assert_eq!(fs::read(&file).expect("read"), bytes);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_file_over_the_limit_is_defaults_and_one_warning() {
        let dir = scratch("big");
        let file = dir.join("apps/org.example.Chat.conf");
        let text = format!("allowed=false\n{}", "# padding\n".repeat(8000));
        assert!(text.len() > READ_LIMIT);
        fs::write(&file, &text).expect("seed");
        assert!(read_text(&file).is_err());
        let mut rules = Rules::new(dir.clone());
        assert!(
            rules
                .rule(&Identity::App("org.example.Chat".into()))
                .allowed
        );
        assert!(matches!(
            rules.set_rule("org.example.Chat", "popups", "false"),
            Err(RuleError::Io(_))
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn warnings_are_capped() {
        let junk = "colour=red\n".repeat(10_000);
        assert!(parse_rule(&junk).1.len() <= 17);
        assert!(parse_settings(&junk).1.len() <= 17);
    }
}
