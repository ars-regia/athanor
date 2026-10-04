//! Do not disturb: on by hand, by schedule and by automatic triggers (fullscreen, screen
//! sharing). The only state kept across sessions is the file `do-not-disturb` in the daemon's
//! state directory, `key=value` lines: `on=true`, `until=<unix seconds>` and
//! `off_override=<reasons>`. It never holds a notification. The old form, a bare `on`, reads as
//! `on=true`.
//!
//! The state is a pure function of what was observed and what was set by hand, so every
//! change of the clock, the settings or a trigger is handled by calling [`Dnd::evaluate`].

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use nix::fcntl::OFlag;
use tracing::warn;

use crate::rules::{write_atomic, Settings, Window};

const FILE: &str = "do-not-disturb";
const READ_LIMIT: u64 = 4096;
const CAP_S: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    Manual,
    Schedule,
    Fullscreen,
    ScreenSharing,
}

impl Reason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Manual => "manual",
            Reason::Schedule => "schedule",
            Reason::Fullscreen => "fullscreen",
            Reason::ScreenSharing => "screen-sharing",
        }
    }

    fn parse(name: &str) -> Option<Reason> {
        [
            Reason::Manual,
            Reason::Schedule,
            Reason::Fullscreen,
            Reason::ScreenSharing,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Unavailable,
    Inactive,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    /// Minutes since local midnight.
    pub minute: u16,
    /// 0 = Monday.
    pub weekday: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct Observed {
    pub now: i64,
    pub local: Local,
    pub fullscreen: Trigger,
    pub sharing: Trigger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub on: bool,
    pub reason: Option<Reason>,
    pub until: Option<i64>,
    pub unavailable: Vec<Reason>,
}

#[derive(Debug, Default)]
pub struct Dnd {
    /// `Some(until)` when switched on by hand, `until` being `None` for no end.
    manual_on: Option<Option<i64>>,
    /// Switched off by hand while these automatic sources were active; holds until the set
    /// of active sources differs.
    off_override: Option<BTreeSet<Reason>>,
    last_auto: BTreeSet<Reason>,
    last_on: bool,
    missed: u32,
}

/// Is `local` inside the window? The end is exclusive; a window that crosses midnight belongs
/// to the day it starts on, so its morning part is checked against the day before.
#[must_use]
pub fn in_window(window: &Window, local: Local) -> bool {
    let day = usize::from(local.weekday % 7);
    let (start, end) = (window.start_min, window.end_min);
    if start < end {
        window.days[day] && (start..end).contains(&local.minute)
    } else if start > end {
        if local.minute >= start {
            window.days[day]
        } else {
            local.minute < end && window.days[(day + 6) % 7]
        }
    } else {
        false
    }
}

fn active_sources(observed: &Observed, settings: &Settings) -> BTreeSet<Reason> {
    let mut active = BTreeSet::new();
    let scheduled = settings
        .schedule
        .as_ref()
        .is_some_and(|w| in_window(w, observed.local));
    if settings.trigger_schedule && scheduled {
        active.insert(Reason::Schedule);
    }
    if settings.trigger_fullscreen && observed.fullscreen == Trigger::Active {
        active.insert(Reason::Fullscreen);
    }
    if settings.trigger_screen_sharing && observed.sharing == Trigger::Active {
        active.insert(Reason::ScreenSharing);
    }
    active
}

impl Dnd {
    /// The state kept in `dir`. A missing file is off; so is one that is not a regular file,
    /// too large, not text or not understood (with a warning): a bad file never stops the
    /// daemon. A hand-set end that has passed is dropped and the file rewritten.
    pub fn load(dir: &Path) -> io::Result<Dnd> {
        let mut dnd = match read(&dir.join(FILE)) {
            Ok(text) => {
                let (dnd, odd) = Dnd::parse(&text);
                if odd {
                    warn!(
                        "do-not-disturb switch has lines that are not understood, they are ignored"
                    );
                }
                dnd
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Dnd::default(),
            Err(err) => {
                warn!(%err, "do-not-disturb switch unreadable, treating it as off");
                Dnd::default()
            }
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        if dnd.expire(now) {
            if let Err(err) = dnd.save(dir) {
                warn!(%err, "cannot rewrite the do-not-disturb switch");
            }
        }
        Ok(dnd)
    }

    /// The state in `text`, and whether any line was not understood. An `until` that does not
    /// parse makes the whole switch off: a hand-set end must never become no end.
    fn parse(text: &str) -> (Dnd, bool) {
        let (mut on, mut until, mut off_override) = (false, None, BTreeSet::new());
        let (mut odd, mut bad_end) = (false, false);
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            match line.split_once('=') {
                None if line == "on" => on = true,
                Some(("on", value)) => on = value.trim() == "true",
                Some(("until", value)) => match value.trim().parse() {
                    Ok(end) => until = Some(end),
                    Err(_) => bad_end = true,
                },
                Some(("off_override", value)) => {
                    off_override = value
                        .split(',')
                        .filter_map(|name| Reason::parse(name.trim()))
                        .collect();
                }
                _ => odd = true,
            }
        }
        odd |= bad_end;
        on &= !bad_end;
        let dnd = Dnd {
            manual_on: on.then_some(until),
            off_override: (!off_override.is_empty()).then_some(off_override),
            ..Dnd::default()
        };
        (dnd, odd)
    }

    /// The file's text: what [`Dnd::save`] writes, and what a caller compares to know that
    /// the state kept on disk is no longer the state held.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(until) = self.manual_on {
            out.push_str("on=true\n");
            if let Some(until) = until {
                out.push_str(&format!("until={until}\n"));
            }
        }
        if let Some(set) = &self.off_override {
            let names: Vec<_> = set.iter().map(|reason| reason.as_str()).collect();
            out.push_str(&format!("off_override={}\n", names.join(",")));
        }
        out
    }

    /// Keep the switch; nothing to keep removes the file.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let path = dir.join(FILE);
        let out = self.render();
        if out.is_empty() {
            return match fs::remove_file(path) {
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            };
        }
        write_atomic(&path, out.as_bytes())
    }

    /// Drop a hand-set end that has passed; true when something changed.
    fn expire(&mut self, now: i64) -> bool {
        if matches!(self.manual_on, Some(Some(until)) if until <= now) {
            self.manual_on = None;
            return true;
        }
        false
    }

    /// The switch by hand. Off holds against every source active right now, until that set
    /// changes; on holds until `until` (exclusive) or for good.
    pub fn set_manual(
        &mut self,
        on: bool,
        until: Option<i64>,
        observed: &Observed,
        settings: &Settings,
    ) {
        if on {
            self.manual_on = Some(until);
            self.off_override = None;
        } else {
            let active = active_sources(observed, settings);
            self.manual_on = None;
            self.off_override = (!active.is_empty()).then_some(active);
        }
    }

    /// The state now; `Some(missed)` when it has just turned off with notifications missed.
    pub fn evaluate(
        &mut self,
        observed: &Observed,
        settings: &Settings,
    ) -> (Effective, Option<u32>) {
        self.expire(observed.now);
        let active = active_sources(observed, settings);
        if self
            .off_override
            .as_ref()
            .is_some_and(|held| *held != active)
        {
            self.off_override = None;
        }
        let automatic = self.off_override.is_none();
        let reason = if self.manual_on.is_some() {
            Some(Reason::Manual)
        } else if automatic {
            active.iter().next().copied()
        } else {
            None
        };
        let on = reason.is_some();
        let mut unavailable = Vec::new();
        if settings.trigger_fullscreen && observed.fullscreen == Trigger::Unavailable {
            unavailable.push(Reason::Fullscreen);
        }
        if settings.trigger_screen_sharing && observed.sharing == Trigger::Unavailable {
            unavailable.push(Reason::ScreenSharing);
        }
        let summary = if on {
            if !self.last_on {
                self.missed = 0;
            }
            None
        } else if self.last_on {
            Some(std::mem::take(&mut self.missed)).filter(|&missed| missed > 0)
        } else {
            None
        };
        self.last_on = on;
        self.last_auto = active;
        let until = self.manual_on.flatten();
        (
            Effective {
                on,
                reason,
                until,
                unavailable,
            },
            summary,
        )
    }

    /// A popup was hidden by do not disturb.
    pub fn missed_one(&mut self) {
        self.missed = self.missed.saturating_add(1);
    }
}

/// Seconds until the next edge of the window or `until`, capped at one hour (a DST change is
/// then corrected at the next wake-up), never zero. `Local` has whole minutes only, so the
/// seconds already into the minute come from `now` (local offsets are whole minutes): the
/// wake lands on the edge, not up to a minute after it.
#[must_use]
pub fn next_wake(observed: &Observed, settings: &Settings, until: Option<i64>) -> u64 {
    let mut wake = CAP_S;
    if let (true, Some(window)) = (settings.trigger_schedule, &settings.schedule) {
        for edge in [window.start_min, window.end_min] {
            let ahead = (u64::from(edge) + 1440 - u64::from(observed.local.minute)) % 1440;
            let ahead = if ahead == 0 { 1440 } else { ahead } * 60;
            wake = wake.min(ahead.saturating_sub(observed.now.rem_euclid(60) as u64));
        }
    }
    if let Some(until) = until.filter(|&until| until > observed.now) {
        wake = wake.min((until - observed.now) as u64);
    }
    wake.max(1)
}

fn read(path: &Path) -> io::Result<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits())
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(READ_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > READ_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Settings, Window};

    fn at(day: u8, hh: u16, mm: u16, now: i64) -> Observed {
        Observed {
            now,
            local: Local {
                minute: hh * 60 + mm,
                weekday: day,
            },
            fullscreen: Trigger::Inactive,
            sharing: Trigger::Unavailable,
        }
    }

    fn night() -> Settings {
        Settings {
            schedule: Some(Window {
                start_min: 22 * 60,
                end_min: 7 * 60,
                days: [true; 7],
            }),
            ..Settings::default()
        }
    }

    #[test]
    fn the_window_crosses_midnight_on_its_days() {
        let w = Window {
            start_min: 22 * 60,
            end_min: 7 * 60,
            days: [true, false, false, false, false, false, false],
        };
        assert!(
            in_window(
                &w,
                Local {
                    minute: 23 * 60,
                    weekday: 0
                }
            ),
            "Monday night"
        );
        assert!(
            in_window(
                &w,
                Local {
                    minute: 6 * 60,
                    weekday: 1
                }
            ),
            "Tuesday morning belongs to Monday's window"
        );
        assert!(
            !in_window(
                &w,
                Local {
                    minute: 7 * 60,
                    weekday: 1
                }
            ),
            "the end is exclusive"
        );
        assert!(
            !in_window(
                &w,
                Local {
                    minute: 23 * 60,
                    weekday: 1
                }
            ),
            "not on Tuesday"
        );
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
        assert_eq!(
            dnd.evaluate(&at(1, 7, 1, 32_520), &settings).1,
            None,
            "one summary only"
        );
    }

    #[test]
    fn off_by_hand_holds_until_the_next_automatic_change() {
        let mut dnd = Dnd::default();
        let settings = night();
        dnd.evaluate(&at(0, 22, 0, 0), &settings);
        dnd.set_manual(false, None, &at(0, 23, 0, 3_600), &settings);
        assert!(
            !dnd.evaluate(&at(1, 3, 0, 18_000), &settings).0.on,
            "still off at 03:00"
        );
        assert!(!dnd.evaluate(&at(1, 7, 0, 32_400), &settings).0.on);
        assert!(
            dnd.evaluate(&at(1, 22, 0, 86_400), &settings).0.on,
            "the next night turns it on"
        );
    }

    #[test]
    fn on_by_hand_for_an_hour_survives_a_fullscreen_video() {
        let mut dnd = Dnd::default();
        let settings = Settings::default();
        dnd.set_manual(true, Some(3_600), &at(2, 12, 0, 0), &settings);
        let mut video = at(2, 12, 10, 600);
        video.fullscreen = Trigger::Active;
        assert_eq!(
            dnd.evaluate(&video, &settings).0.reason,
            Some(Reason::Manual)
        );
        assert!(
            dnd.evaluate(&at(2, 12, 20, 1_200), &settings).0.on,
            "the video ended, the hour holds"
        );
        assert!(
            !dnd.evaluate(&at(2, 13, 0, 3_600), &settings).0.on,
            "until is exclusive"
        );
    }

    #[test]
    fn a_clock_set_backwards_is_evaluated_again_without_a_second_summary() {
        let mut dnd = Dnd::default();
        let settings = night();
        dnd.evaluate(&at(0, 22, 30, 1_800), &settings);
        dnd.missed_one();
        assert_eq!(
            dnd.evaluate(&at(0, 21, 50, -600), &settings).1,
            Some(1),
            "the clock went back out of the window"
        );
        assert!(dnd.evaluate(&at(0, 22, 0, 0), &settings).0.on);
        assert_eq!(
            dnd.evaluate(&at(1, 7, 0, 32_400), &settings).1,
            None,
            "nothing missed the second time"
        );
    }

    #[test]
    fn an_unobservable_trigger_is_published_as_unavailable() {
        let mut dnd = Dnd::default();
        let mut observed = at(0, 12, 0, 0);
        observed.fullscreen = Trigger::Unavailable;
        let (state, _) = dnd.evaluate(&observed, &Settings::default());
        assert!(!state.on);
        assert_eq!(
            state.unavailable,
            [Reason::Fullscreen, Reason::ScreenSharing]
        );
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("athanor-shelld-dnd-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn the_switch_round_trips_and_nothing_to_keep_removes_the_file() {
        let dir = scratch("round");
        let settings = night();
        let mut dnd = Dnd::load(&dir).expect("absent file is off");
        assert!(!dnd.evaluate(&at(0, 12, 0, 0), &settings).0.on);
        dnd.set_manual(true, Some(i64::MAX), &at(0, 12, 0, 0), &settings);
        dnd.save(&dir).expect("save");
        let text = std::fs::read_to_string(dir.join("do-not-disturb")).expect("read");
        assert_eq!(text, format!("on=true\nuntil={}\n", i64::MAX));
        assert!(
            Dnd::load(&dir)
                .expect("load")
                .evaluate(&at(0, 12, 0, 0), &settings)
                .0
                .on
        );
        dnd.evaluate(&at(0, 23, 0, 0), &settings);
        dnd.set_manual(false, None, &at(0, 23, 0, 0), &settings);
        dnd.save(&dir).expect("save");
        let text = std::fs::read_to_string(dir.join("do-not-disturb")).expect("read");
        assert_eq!(text, "off_override=schedule\n");
        let mut back = Dnd::load(&dir).expect("load");
        assert!(
            !back.evaluate(&at(0, 23, 30, 0), &settings).0.on,
            "the override came back"
        );
        dnd.set_manual(false, None, &at(0, 12, 0, 0), &settings);
        dnd.save(&dir).expect("save");
        assert!(!dir.join("do-not-disturb").exists());
        dnd.save(&dir).expect("removing twice is fine");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_file_that_is_no_switch_is_off_and_never_blocks() {
        let dir = scratch("junk");
        let path = dir.join("do-not-disturb");
        nix::unistd::mkfifo(&path, nix::sys::stat::Mode::from_bits_truncate(0o600)).expect("fifo");
        let mut dnd = Dnd::load(&dir).expect("a FIFO is off, not a hang");
        assert!(!dnd.evaluate(&at(0, 12, 0, 0), &Settings::default()).0.on);
        std::fs::remove_file(&path).expect("rm");
        std::os::unix::fs::symlink("/etc/hostname", &path).expect("link");
        assert!(
            !Dnd::load(&dir)
                .expect("load")
                .evaluate(&at(0, 12, 0, 0), &Settings::default())
                .0
                .on
        );
        std::fs::remove_file(&path).expect("rm");
        std::fs::write(
            &path,
            "on=maybe\nuntil=soon\noff_override=nope\n\u{0}garbage",
        )
        .expect("junk");
        assert!(
            !Dnd::load(&dir)
                .expect("load")
                .evaluate(&at(0, 12, 0, 0), &Settings::default())
                .0
                .on
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_trigger_switched_off_in_the_settings_is_neither_active_nor_unavailable() {
        let mut dnd = Dnd::default();
        let settings = Settings {
            trigger_fullscreen: false,
            trigger_screen_sharing: false,
            ..Settings::default()
        };
        let mut observed = at(0, 12, 0, 0);
        observed.fullscreen = Trigger::Active;
        let (state, _) = dnd.evaluate(&observed, &settings);
        assert!(!state.on);
        assert!(state.unavailable.is_empty());
    }

    #[test]
    fn the_next_wake_is_never_zero_and_ignores_a_past_until() {
        let settings = Settings::default();
        assert_eq!(next_wake(&at(0, 12, 0, 100), &settings, Some(101)), 1);
        assert_eq!(
            next_wake(&at(0, 12, 0, 100), &settings, Some(100)),
            3_600,
            "already expired"
        );
        assert_eq!(next_wake(&at(0, 12, 0, 100), &settings, Some(50)), 3_600);
    }

    #[test]
    fn a_malformed_end_reads_as_off_and_an_unknown_line_is_reported() {
        let (dnd, odd) = Dnd::parse("on=true\nuntil=soon\n");
        assert!(dnd.manual_on.is_none(), "never on with no end by mistake");
        assert!(odd);
        let (dnd, odd) = Dnd::parse("on=true\nbogus\nmore bogus\n");
        assert!(dnd.manual_on.is_some() && odd);
        let (dnd, odd) = Dnd::parse("on\n");
        assert!(dnd.manual_on.is_some() && !odd);
        let (_, odd) = Dnd::parse("on=true\nuntil=5\noff_override=schedule\n\n");
        assert!(!odd);
    }

    #[test]
    fn the_wake_counts_the_seconds_already_into_the_minute() {
        // 21:59:59: the edge is one second away, not sixty.
        assert_eq!(next_wake(&at(0, 21, 59, 59), &night(), None), 1);
        assert_eq!(next_wake(&at(0, 21, 59, 60 * 7 + 30), &night(), None), 30);
        assert_eq!(next_wake(&at(0, 21, 30, -31), &night(), None), 1_800 - 29);
    }
}
