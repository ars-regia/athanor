//! What to tell the user, and which `ActionInvoked` signals to believe (UT11). No bus here:
//! `main.rs` sends what this module decides.
use athanor_trust_state::{State, UpdateState};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A downloaded digest waits for a restart.
    Ready { digest: String },
    /// A new deployment booted for the first time, and there is a way back.
    Running { digest: String, version: String, build_time: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Apply,
    GoBack,
}

pub const ACTION_APPLY: &str = "apply";
pub const ACTION_LATER: &str = "later";
pub const ACTION_GO_BACK: &str = "go-back";

/// A notification this process sent and still answers for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The unique name that owned `org.freedesktop.Notifications` when it was sent.
    pub server: String,
    pub notice: Notice,
}

#[derive(Default)]
pub struct Notices {
    /// Digests offered since this process started: one notice per digest and per session.
    offered: HashSet<String>,
    /// The downloaded digest last announced to this user, in any session: never offered again.
    announced: Option<String>,
    pub sent: HashMap<u32, Sent>,
}

impl Notices {
    /// `announced` is the record `announced()` reads: the digest a past session offered.
    #[must_use]
    pub fn new(announced: Option<String>) -> Self {
        Self { announced, ..Self::default() }
    }

    /// The notice `state` calls for, if any. `seen_booted` is the digest recorded by the
    /// last run; `None` means this user has never run the notifier, and a first session is
    /// not greeted with "the system was updated".
    pub fn due(&mut self, state: &State, seen_booted: Option<&str>) -> Option<Notice> {
        if seen_booted.is_some_and(|seen| seen != state.booted.digest) && state.previous.is_some() && self.offered.insert(format!("running {}", state.booted.digest)) {
            return Some(Notice::Running { digest: state.booted.digest.clone(), version: state.booted.version.clone(), build_time: state.booted.build_time });
        }
        let downloaded = state.downloaded.as_ref().filter(|_| state.update == UpdateState::Downloaded)?;
        if self.announced.as_deref() == Some(downloaded.digest.as_str()) {
            return None;
        }
        self.offered.insert(downloaded.digest.clone()).then(|| Notice::Ready { digest: downloaded.digest.clone() })
    }

    /// The request an `ActionInvoked(id, action)` from `sender` stands for. Any session
    /// process can emit that signal, and a forged one would summon the administrator prompt
    /// of `GoBack()` out of nowhere: only the server that took the notification, and only an
    /// id this process holds, are believed. The id is forgotten either way.
    pub fn invoked(&mut self, sender: &str, id: u32, action: &str) -> Option<Request> {
        if self.sent.get(&id)?.server != sender {
            return None;
        }
        match (self.sent.remove(&id)?.notice, action) {
            (Notice::Ready { .. }, ACTION_APPLY) => Some(Request::Apply),
            (Notice::Running { .. }, ACTION_GO_BACK) => Some(Request::GoBack),
            _ => None,
        }
    }
}

const SEEN_FILE: &str = "seen-booted";
const ANNOUNCED_FILE: &str = "announced";

fn recorded(state_dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(state_dir.join(file)).ok().map(|text| text.trim().to_owned())
}

fn record(state_dir: &Path, file: &str, digest: &str) -> std::io::Result<()> {
    let temporary = state_dir.join(format!(".{file}.{}", std::process::id()));
    std::fs::write(&temporary, format!("{digest}\n"))?;
    std::fs::rename(temporary, state_dir.join(file))
}

#[must_use]
pub fn seen_booted(state_dir: &Path) -> Option<String> {
    recorded(state_dir, SEEN_FILE)
}

/// # Errors
/// The record cannot be written.
pub fn record_booted(state_dir: &Path, digest: &str) -> std::io::Result<()> {
    record(state_dir, SEEN_FILE, digest)
}

/// The downloaded digest a past session announced (UT11).
#[must_use]
pub fn announced(state_dir: &Path) -> Option<String> {
    recorded(state_dir, ANNOUNCED_FILE)
}

/// # Errors
/// The record cannot be written.
pub fn record_announced(state_dir: &Path, digest: &str) -> std::io::Result<()> {
    record(state_dir, ANNOUNCED_FILE, digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(booted: &str, downloaded: Option<&str>, update: UpdateState) -> State {
        let text = include_str!("../../athanor-update-1.0.0/tests/state-verified.json");
        let mut state = athanor_trust_state::parse(text).expect("fixture");
        state.booted.digest = booted.into();
        state.previous = Some(state.booted.clone());
        state.downloaded = downloaded.map(|digest| athanor_trust_state::Deployment { digest: digest.into(), ..state.booted.clone() });
        state.update = update;
        state
    }

    #[test]
    fn one_notice_per_downloaded_digest_per_session() {
        let mut notices = Notices::default();
        let ready = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
        assert_eq!(notices.due(&ready, Some("sha256:a")), Some(Notice::Ready { digest: "sha256:b".into() }));
        assert_eq!(notices.due(&ready, Some("sha256:a")), None, "\"Later\" is not asked again in this session");
        // A new session with no announced record offers it; `an_announced_digest_is_never_offered_again` covers the record.
        assert!(Notices::default().due(&ready, Some("sha256:a")).is_some());
        let newer = state("sha256:a", Some("sha256:c"), UpdateState::Downloaded);
        assert_eq!(notices.due(&newer, Some("sha256:a")), Some(Notice::Ready { digest: "sha256:c".into() }));
    }

    #[test]
    fn an_announced_digest_is_never_offered_again() {
        let ready = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
        assert_eq!(Notices::new(Some("sha256:b".into())).due(&ready, Some("sha256:a")), None, "UT11: a new session does not ask again");
        assert_eq!(
            Notices::new(Some("sha256:b".into())).due(&state("sha256:a", Some("sha256:c"), UpdateState::Downloaded), Some("sha256:a")),
            Some(Notice::Ready { digest: "sha256:c".into() }),
            "a newer download is offered"
        );
        assert!(Notices::new(None).due(&ready, Some("sha256:a")).is_some(), "never announced: offered");
    }

    #[test]
    fn the_announced_record_round_trips() {
        let dir = std::env::temp_dir().join(format!("athanor-update-notify-announced-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert_eq!(announced(&dir), None);
        record_announced(&dir, "sha256:b").expect("write");
        assert_eq!(announced(&dir).as_deref(), Some("sha256:b"));
        assert_eq!(seen_booted(&dir), None, "the two records are separate files");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn an_unwritable_state_dir_keeps_one_offer_per_session() {
        let missing = std::env::temp_dir().join(format!("athanor-update-notify-missing-{}/nested", std::process::id()));
        assert!(record_announced(&missing, "sha256:b").is_err(), "the caller logs it; nothing panics");
        let mut notices = Notices::new(announced(&missing));
        let ready = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
        assert!(notices.due(&ready, Some("sha256:a")).is_some());
        assert_eq!(notices.due(&ready, Some("sha256:a")), None);
    }

    #[test]
    fn only_a_downloaded_update_is_announced() {
        for update in [UpdateState::Available, UpdateState::Held, UpdateState::OlderThanBooted, UpdateState::Refused, UpdateState::WillApplyAtNextShutdown] {
            assert_eq!(Notices::default().due(&state("sha256:a", Some("sha256:b"), update), Some("sha256:a")), None, "{update:?}");
        }
    }

    #[test]
    fn a_new_deployment_is_announced_once_and_never_on_a_first_session() {
        let mut notices = Notices::default();
        let booted = state("sha256:b", None, UpdateState::None);
        assert_eq!(notices.due(&booted, None), None, "first session of this user");
        assert!(matches!(notices.due(&booted, Some("sha256:a")), Some(Notice::Running { .. })));
        assert_eq!(notices.due(&booted, Some("sha256:a")), None);
        assert_eq!(notices.due(&booted, Some("sha256:b")), None);
    }

    #[test]
    fn a_forged_action_is_dropped() {
        let mut notices = Notices::default();
        notices.sent.insert(7, Sent { server: ":1.42".into(), notice: Notice::Running { digest: "sha256:b".into(), version: "43".into(), build_time: 0 } });
        assert_eq!(notices.invoked(":1.666", 7, ACTION_GO_BACK), None, "another sender");
        assert_eq!(notices.invoked(":1.42", 8, ACTION_GO_BACK), None, "an id this process does not hold");
        assert_eq!(notices.invoked(":1.42", 7, ACTION_APPLY), None, "an action that notification never offered");
        assert_eq!(notices.invoked(":1.42", 7, ACTION_GO_BACK), None, "the id was forgotten by the wrong action");
        notices.sent.insert(9, Sent { server: ":1.42".into(), notice: Notice::Ready { digest: "sha256:c".into() } });
        assert_eq!(notices.invoked(":1.42", 9, ACTION_APPLY), Some(Request::Apply));
        assert_eq!(notices.invoked(":1.42", 9, ACTION_APPLY), None, "once");
    }

    #[test]
    fn the_seen_record_round_trips() {
        let dir = std::env::temp_dir().join(format!("athanor-update-notify-seen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert_eq!(seen_booted(&dir), None);
        record_booted(&dir, "sha256:a").expect("write");
        assert_eq!(seen_booted(&dir).as_deref(), Some("sha256:a"));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
