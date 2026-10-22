//! What to tell the user, and which `ActionInvoked` signals to believe (UT11). No bus here:
//! `main.rs` sends what this module decides.
use athanor_trust_state::{postponed_until, State, UpdateState};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A downloaded digest waits for a restart.
    Ready { digest: String },
    /// A new deployment booted for the first time, and there is a way back.
    Running { digest: String, version: String, build_time: i64 },
    /// A security digest applies at the next shutdown (UT13). `until` is the expiry of the
    /// lineage's postpone once it is used; before that the notice offers the postpone.
    Security { digest: String, until: Option<i64> },
    /// A security digest applies at the next shutdown and the lineage's postpone is spent
    /// (a newer digest after the expiry): the same text, no offer, no expiry line.
    SecurityApplies { digest: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Apply,
    GoBack,
    Postpone,
}

pub const ACTION_APPLY: &str = "apply";
pub const ACTION_LATER: &str = "later";
pub const ACTION_GO_BACK: &str = "go-back";
pub const ACTION_POSTPONE: &str = "postpone";

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
        let staged = state.downloaded.as_ref()?;
        let notice = |digest: String| match (state.update, &state.postpone) {
            // The service unlocked it: nothing restarts by itself. One postpone is offered while
            // the lineage's is unused, none once it is spent. A signed copy (no lineage) is not
            // a security notice.
            (UpdateState::WillApplyAtNextShutdown, Some(postpone)) if !postpone.postponed => Some(Notice::Security { digest, until: None }),
            (UpdateState::WillApplyAtNextShutdown, Some(_)) => Some(Notice::SecurityApplies { digest }),
            // Re-locked by the postpone: no second one is offered, and the opt-out still gets the UT11 notice.
            (UpdateState::Downloaded, Some(postpone)) if postpone.postponed && state.automatic_security_updates => Some(Notice::Security { digest, until: postponed_until(state) }),
            (UpdateState::Downloaded, _) => Some(Notice::Ready { digest }),
            _ => None,
        };
        if self.announced.as_deref() == Some(staged.digest.as_str()) {
            return None;
        }
        let notice = notice(staged.digest.clone())?;
        self.offered.insert(staged.digest.clone()).then_some(notice)
    }

    /// A notice that could not be sent, typically before the session's notification server
    /// has started: it is due again at the next poll instead of being lost.
    pub fn withdraw(&mut self, notice: &Notice) {
        self.offered.remove(&match notice {
            Notice::Ready { digest } | Notice::Security { digest, .. } | Notice::SecurityApplies { digest } => digest.clone(),
            Notice::Running { digest, .. } => format!("running {digest}"),
        });
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
            (Notice::Security { until: None, .. }, ACTION_POSTPONE) => Some(Request::Postpone),
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

    fn security(state: &mut State, postponed: bool) {
        state.postpone = Some(athanor_trust_state::Postpone { anchor: None, first_notice_at: 1_790_000_000, boots_remaining: athanor_trust_state::POSTPONE_BOOTS, postponed });
    }

    #[test]
    fn a_security_digest_is_announced_once_with_postpone() {
        let mut staged = state("sha256:a", Some("sha256:b"), UpdateState::WillApplyAtNextShutdown);
        security(&mut staged, false);
        let mut notices = Notices::default();
        let notice = notices.due(&staged, Some("sha256:a")).expect("a security digest");
        assert_eq!(notice, Notice::Security { digest: "sha256:b".into(), until: None });
        assert_eq!(crate::text::actions(&notice, crate::text::texts(None)), vec![ACTION_POSTPONE, "Postpone"], "no open-ended Later");
        assert_eq!(notices.due(&staged, Some("sha256:a")), None, "once per session");
        assert_eq!(Notices::new(Some("sha256:b".into())).due(&staged, Some("sha256:a")), None, "and never in a later one");
    }

    #[test]
    fn a_newer_digest_after_an_expired_postpone_is_announced_without_an_offer() {
        let mut staged = state("sha256:a", Some("sha256:s2"), UpdateState::WillApplyAtNextShutdown);
        security(&mut staged, true);
        let mut notices = Notices::new(Some("sha256:s1".into()));
        let notice = notices.due(&staged, Some("sha256:a")).expect("S2 installs at the next shutdown");
        assert_eq!(notice, Notice::SecurityApplies { digest: "sha256:s2".into() });
        let t = crate::text::texts(None);
        assert!(crate::text::actions(&notice, t).is_empty(), "no postpone offer");
        assert_eq!(notices.invoked(":1.1", 1, ACTION_POSTPONE), None);
        assert_eq!(notices.due(&staged, Some("sha256:a")), None, "once per digest");
    }

    #[test]
    fn a_postponed_digest_offers_no_postpone() {
        let mut locked = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
        security(&mut locked, true);
        let notice = Notices::default().due(&locked, Some("sha256:a")).expect("a postponed digest");
        assert_eq!(notice, Notice::Security { digest: "sha256:b".into(), until: Some(1_790_604_800) });
        assert!(crate::text::actions(&notice, crate::text::texts(None)).is_empty());
    }

    #[test]
    fn with_the_opt_out_on_the_notice_still_arrives() {
        let mut locked = state("sha256:a", Some("sha256:b"), UpdateState::Downloaded);
        security(&mut locked, false);
        locked.automatic_security_updates = false;
        let notice = Notices::default().due(&locked, Some("sha256:a")).expect("the UT11 notice");
        assert_eq!(notice, Notice::Ready { digest: "sha256:b".into() });
        assert_eq!(crate::text::actions(&notice, crate::text::texts(None)), vec![ACTION_APPLY, "Restart to update", ACTION_LATER, "Later"]);
    }

    #[test]
    fn a_signed_copy_is_not_announced_as_security() {
        let mut staged = state("sha256:a", Some("sha256:b"), UpdateState::WillApplyAtNextShutdown);
        staged.postpone = None;
        assert_eq!(Notices::default().due(&staged, Some("sha256:a")), None);
    }

    #[test]
    fn postpone_is_invoked_only_from_the_notification_owner() {
        let mut notices = Notices::default();
        let notice = Notice::Security { digest: "sha256:b".into(), until: None };
        notices.sent.insert(5, Sent { server: ":1.42".into(), notice: notice.clone() });
        assert_eq!(notices.invoked(":1.666", 5, ACTION_POSTPONE), None, "another sender");
        notices.sent.insert(5, Sent { server: ":1.42".into(), notice: notice.clone() });
        assert_eq!(notices.invoked(":1.42", 6, ACTION_POSTPONE), None, "another id");
        assert_eq!(notices.invoked(":1.42", 5, ACTION_POSTPONE), Some(Request::Postpone));
        notices.sent.insert(5, Sent { server: ":1.42".into(), notice: Notice::Security { digest: "sha256:b".into(), until: Some(1) } });
        assert_eq!(notices.invoked(":1.42", 5, ACTION_POSTPONE), None, "a postponed notice offers none");
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
    fn a_withdrawn_notice_is_due_again() {
        let mut notices = Notices::default();
        let booted = state("sha256:b", None, UpdateState::None);
        let running = notices.due(&booted, Some("sha256:a")).expect("a new deployment");
        notices.withdraw(&running);
        assert_eq!(notices.due(&booted, Some("sha256:a")), Some(running));
        let downloaded = state("sha256:a", Some("sha256:c"), UpdateState::Downloaded);
        let ready = notices.due(&downloaded, Some("sha256:a")).expect("a downloaded update");
        notices.withdraw(&ready);
        assert_eq!(notices.due(&downloaded, Some("sha256:a")), Some(ready));
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
