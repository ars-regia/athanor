//! What the RemoteDesktop portal decides (doc_portal.md PT15, amended on 2026-11-17): the device
//! types, the rule of the consent dialog's grant button, and the life of each session, from
//! `CreateSession` to its close, with the list `os.athanor.Portal1` publishes as
//! `InputSessions` (PT10).
//!
//! A session is granted once, by the user in the dialog of `Start`, and obtains one EIS file
//! descriptor at most, after the grant and only with the granted types. No consent is kept:
//! every session asks again.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The portal's device types (`AvailableDeviceTypes`).
pub const KEYBOARD: u32 = 1;
pub const POINTER: u32 = 2;
pub const TOUCHSCREEN: u32 = 4;
/// Every type the backend offers.
pub const AVAILABLE: u32 = KEYBOARD | POINTER | TOUCHSCREEN;

/// How long the grant button stays insensitive after the dialog maps (PT4).
pub const GRANT_DELAY: Duration = Duration::from_millis(500);

/// The device types `SelectDevices` asks for: its `types` option, all of them when absent. A
/// mask of 0 or with a bit the backend does not offer is refused.
pub fn requested(types: Option<u32>) -> Result<u32, String> {
    match types {
        None => Ok(AVAILABLE),
        Some(types) if types != 0 && types & !AVAILABLE == 0 => Ok(types),
        Some(types) => Err(format!(
            "device types {types:#x}: 1 keyboard, 2 pointer and 4 touch screen, at least one"
        )),
    }
}

/// What the dialog shows and what the user has ticked. Nothing is ticked when it maps, so
/// granting nothing is the refusal and a grant never carries 0.
#[derive(Debug, Clone)]
pub struct Consent {
    asked: u32,
    ticked: u32,
    mapped: Instant,
}

impl Consent {
    /// The dialog for the types `asked`, mapped at `mapped`.
    pub fn new(asked: u32, mapped: Instant) -> Consent {
        Consent {
            asked: asked & AVAILABLE,
            ticked: 0,
            mapped,
        }
    }

    /// The types the dialog lists, in the order it lists them.
    pub fn listed(&self) -> impl Iterator<Item = u32> + '_ {
        [KEYBOARD, POINTER, TOUCHSCREEN]
            .into_iter()
            .filter(|kind| self.asked & kind != 0)
    }

    /// The user ticks or unticks one type. A type the application did not ask for is ignored.
    pub fn tick(&mut self, kind: u32, on: bool) {
        let kind = kind & self.asked;
        if on {
            self.ticked |= kind;
        } else {
            self.ticked &= !kind;
        }
    }

    pub fn ticked(&self) -> u32 {
        self.ticked
    }

    /// Whether the grant button is sensitive at `now`: at least [`GRANT_DELAY`] after the map,
    /// and with a type ticked.
    pub fn grantable(&self, now: Instant) -> bool {
        self.ticked != 0 && now.saturating_duration_since(self.mapped) >= GRANT_DELAY
    }

    /// The grant button pressed at `now`: the ticked types, or `None` while it is insensitive.
    pub fn grant(&self, now: Instant) -> Option<u32> {
        self.grantable(now).then_some(self.ticked)
    }
}

/// How the dialog of `Start` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Granted, with these types, never 0.
    Granted(u32),
    /// Refused by the user, or closed by `Close`.
    Refused,
}

impl Answer {
    /// The portal's response: 0 granted, 1 refused or cancelled.
    pub fn response(self) -> u32 {
        match self {
            Answer::Granted(_) => 0,
            Answer::Refused => 1,
        }
    }
}

/// The name a dialog and the indicator give the application (PT4): its desktop file's `Name`,
/// else its id, else "An application" for a host application with no id.
pub fn display_name(app_id: &str, desktop_name: Option<&str>) -> String {
    match desktop_name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => name.to_owned(),
        None if !app_id.is_empty() => app_id.to_owned(),
        None => "An application".to_owned(),
    }
}

/// Where a session is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Created, its devices maybe selected: the types it asks for.
    Created(u32),
    /// `Start` shows the dialog.
    Asking(u32),
    /// Granted these types, no file descriptor asked for yet.
    Granted(u32),
    /// `ConnectToEIS` is asking the compositor.
    Connecting(u32),
    /// The compositor answered with a file descriptor: the context is open.
    Connected(u32),
    /// `ConnectToEIS` was refused: the session obtains no file descriptor.
    Spent,
}

#[derive(Debug, Clone)]
struct Session {
    app_id: String,
    name: String,
    stage: Stage,
}

/// The sessions of the backend, by the session's object path.
#[derive(Debug, Default)]
pub struct Sessions {
    sessions: BTreeMap<String, Session>,
}

/// Why a call on a session is refused.
pub type Refusal = String;

impl Sessions {
    pub fn new() -> Sessions {
        Sessions::default()
    }

    /// `CreateSession`: a new session for `app_id`, shown as `name`.
    pub fn create(&mut self, handle: &str, app_id: &str, name: &str) -> Result<(), Refusal> {
        if self.sessions.contains_key(handle) {
            return Err(format!("{handle} exists already"));
        }
        self.sessions.insert(
            handle.to_owned(),
            Session {
                app_id: app_id.to_owned(),
                name: name.to_owned(),
                stage: Stage::Created(AVAILABLE),
            },
        );
        Ok(())
    }

    fn of(&mut self, handle: &str, app_id: &str) -> Result<&mut Session, Refusal> {
        match self.sessions.get_mut(handle) {
            Some(session) if session.app_id == app_id => Ok(session),
            Some(_) => Err(format!("{handle} belongs to another application")),
            None => Err(format!("no session {handle}")),
        }
    }

    /// `SelectDevices`, before `Start`.
    pub fn select(&mut self, handle: &str, app_id: &str, types: u32) -> Result<(), Refusal> {
        let session = self.of(handle, app_id)?;
        match session.stage {
            Stage::Created(_) => {
                session.stage = Stage::Created(types);
                Ok(())
            }
            stage => Err(format!("{handle} cannot select devices once {stage:?}")),
        }
    }

    /// `Start` opens the dialog: the types it lists.
    pub fn ask(&mut self, handle: &str, app_id: &str) -> Result<u32, Refusal> {
        let session = self.of(handle, app_id)?;
        match session.stage {
            Stage::Created(types) => {
                session.stage = Stage::Asking(types);
                Ok(types)
            }
            stage => Err(format!("{handle} cannot start once {stage:?}")),
        }
    }

    /// The dialog's answer. A grant is kept only for types the session asked for; a refusal
    /// leaves the session spent. `false` when the session closed meanwhile.
    pub fn answer(&mut self, handle: &str, answer: Answer) -> bool {
        let Some(session) = self.sessions.get_mut(handle) else {
            return false;
        };
        let Stage::Asking(asked) = session.stage else {
            return false;
        };
        session.stage = match answer {
            Answer::Granted(types) if types != 0 && types & !asked == 0 => Stage::Granted(types),
            Answer::Granted(_) | Answer::Refused => Stage::Spent,
        };
        matches!(session.stage, Stage::Granted(_))
    }

    /// `ConnectToEIS`: the granted types, to ask the compositor for. Refused before the grant
    /// and for every call after the first.
    pub fn connect(&mut self, handle: &str, app_id: &str) -> Result<u32, Refusal> {
        let session = self.of(handle, app_id)?;
        match session.stage {
            Stage::Granted(types) => {
                session.stage = Stage::Connecting(types);
                Ok(types)
            }
            Stage::Connecting(_) | Stage::Connected(_) | Stage::Spent => Err(format!(
                "{handle} has asked for its file descriptor already"
            )),
            Stage::Created(_) | Stage::Asking(_) => Err(format!("{handle} is not granted")),
        }
    }

    /// The compositor's answer to `ConnectToEIS`: `true` for a file descriptor, `false` for a
    /// refusal. Returns whether the session is still open; when it closed meanwhile the caller
    /// closes the context it was just given.
    pub fn connected(&mut self, handle: &str, opened: bool) -> bool {
        let Some(session) = self.sessions.get_mut(handle) else {
            return false;
        };
        if let Stage::Connecting(types) = session.stage {
            session.stage = if opened {
                Stage::Connected(types)
            } else {
                Stage::Spent
            };
        }
        true
    }

    /// The session ends, for any reason: its stage, if it existed. A session that was
    /// connected or connecting has a context the caller closes with `CloseSession`.
    pub fn close(&mut self, handle: &str) -> Option<Stage> {
        self.sessions.remove(handle).map(|session| session.stage)
    }

    pub fn stage(&self, handle: &str) -> Option<Stage> {
        self.sessions.get(handle).map(|session| session.stage)
    }

    /// `InputSessions`: session id, application id and name of each session whose context is
    /// open, from the compositor's answer to its close.
    pub fn input_sessions(&self) -> Vec<(String, String, String)> {
        self.sessions
            .iter()
            .filter(|(_, session)| matches!(session.stage, Stage::Connected(_)))
            .map(|(handle, session)| (handle.clone(), session.app_id.clone(), session.name.clone()))
            .collect()
    }
}

/// Whether a session in `stage` holds, or may be about to hold, a context in the compositor.
pub fn holds_context(stage: Stage) -> bool {
    matches!(stage, Stage::Connecting(_) | Stage::Connected(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "/org/freedesktop/portal/desktop/session/1_1/a";
    const APP: &str = "org.example.Remote";

    #[test]
    fn requested_types_default_to_all_and_refuse_zero_or_unknown_bits() {
        assert_eq!(requested(None), Ok(AVAILABLE));
        assert_eq!(requested(Some(KEYBOARD | POINTER)), Ok(3));
        for bad in [0, 8, 9, 0xffff_ffff] {
            assert!(requested(Some(bad)).is_err(), "{bad:#x}");
        }
    }

    #[test]
    fn the_grant_waits_half_a_second_and_for_a_ticked_type() {
        let mapped = Instant::now();
        let mut consent = Consent::new(KEYBOARD | POINTER, mapped);
        let later = mapped + GRANT_DELAY;
        assert!(!consent.grantable(later), "nothing ticked");
        assert_eq!(consent.grant(later), None);
        consent.tick(KEYBOARD, true);
        assert!(!consent.grantable(mapped + GRANT_DELAY - Duration::from_millis(1)));
        assert_eq!(consent.grant(mapped), None, "too early");
        assert_eq!(consent.grant(later), Some(KEYBOARD));
        consent.tick(KEYBOARD, false);
        assert_eq!(consent.grant(later), None, "unticked again");
    }

    #[test]
    fn the_dialog_lists_and_grants_only_the_types_asked_for() {
        let mapped = Instant::now();
        let mut consent = Consent::new(KEYBOARD | TOUCHSCREEN, mapped);
        assert_eq!(
            consent.listed().collect::<Vec<_>>(),
            [KEYBOARD, TOUCHSCREEN]
        );
        consent.tick(POINTER, true);
        assert_eq!(consent.ticked(), 0, "a type not asked for cannot be ticked");
        consent.tick(TOUCHSCREEN, true);
        assert_eq!(consent.grant(mapped + GRANT_DELAY), Some(TOUCHSCREEN));
    }

    #[test]
    fn a_refusal_answers_one_and_a_grant_zero() {
        assert_eq!(Answer::Refused.response(), 1);
        assert_eq!(Answer::Granted(KEYBOARD).response(), 0);
    }

    #[test]
    fn the_name_falls_back_to_the_id_then_to_an_application() {
        assert_eq!(display_name(APP, Some("Remote")), "Remote");
        assert_eq!(display_name(APP, Some("  ")), APP);
        assert_eq!(display_name(APP, None), APP);
        assert_eq!(display_name("", None), "An application");
    }

    fn started(sessions: &mut Sessions, answer: Answer) -> bool {
        sessions.create(H, APP, "Remote").expect("create");
        sessions.select(H, APP, KEYBOARD | POINTER).expect("select");
        assert_eq!(sessions.ask(H, APP), Ok(KEYBOARD | POINTER));
        sessions.answer(H, answer)
    }

    #[test]
    fn no_file_descriptor_is_asked_for_before_the_grant() {
        let mut sessions = Sessions::new();
        sessions.create(H, APP, "Remote").expect("create");
        assert!(sessions.connect(H, APP).is_err(), "created");
        sessions.ask(H, APP).expect("ask");
        assert!(sessions.connect(H, APP).is_err(), "asking");
    }

    #[test]
    fn a_refused_session_obtains_no_file_descriptor() {
        let mut sessions = Sessions::new();
        assert!(!started(&mut sessions, Answer::Refused));
        assert!(sessions.connect(H, APP).is_err());
        assert!(sessions.input_sessions().is_empty());
    }

    #[test]
    fn a_grant_beyond_what_was_asked_is_no_grant() {
        let mut sessions = Sessions::new();
        assert!(!started(&mut sessions, Answer::Granted(TOUCHSCREEN)));
        assert!(
            !Sessions::new().answer(H, Answer::Granted(KEYBOARD)),
            "no session"
        );
        let mut zero = Sessions::new();
        assert!(!started(&mut zero, Answer::Granted(0)));
    }

    #[test]
    fn the_granted_types_are_connected_once() {
        let mut sessions = Sessions::new();
        assert!(started(&mut sessions, Answer::Granted(POINTER)));
        assert_eq!(sessions.connect(H, APP), Ok(POINTER));
        assert!(
            sessions.connect(H, APP).is_err(),
            "a second call while the first asks"
        );
        assert!(sessions.connected(H, true));
        assert!(
            sessions.connect(H, APP).is_err(),
            "a second call once connected"
        );
        assert!(sessions.connect(H, "org.example.Other").is_err());
    }

    #[test]
    fn a_compositor_refusal_spends_the_session() {
        let mut sessions = Sessions::new();
        assert!(started(&mut sessions, Answer::Granted(KEYBOARD)));
        sessions.connect(H, APP).expect("connect");
        assert!(sessions.connected(H, false));
        assert_eq!(sessions.stage(H), Some(Stage::Spent));
        assert!(sessions.connect(H, APP).is_err());
        assert!(sessions.input_sessions().is_empty());
    }

    #[test]
    fn input_sessions_list_a_session_from_its_context_to_its_close() {
        let mut sessions = Sessions::new();
        assert!(started(&mut sessions, Answer::Granted(KEYBOARD)));
        sessions.connect(H, APP).expect("connect");
        assert!(
            sessions.input_sessions().is_empty(),
            "the compositor has not answered"
        );
        sessions.connected(H, true);
        assert_eq!(
            sessions.input_sessions(),
            [(H.to_owned(), APP.to_owned(), "Remote".to_owned())]
        );
        let closed = sessions.close(H).expect("closed");
        assert!(holds_context(closed));
        assert!(sessions.input_sessions().is_empty());
    }

    #[test]
    fn a_session_closed_while_connecting_is_reported_gone() {
        let mut sessions = Sessions::new();
        assert!(started(&mut sessions, Answer::Granted(KEYBOARD)));
        sessions.connect(H, APP).expect("connect");
        assert!(holds_context(sessions.close(H).expect("closed")));
        assert!(
            !sessions.connected(H, true),
            "the caller must close the new context"
        );
    }

    #[test]
    fn a_session_is_granted_anew_each_time() {
        let mut sessions = Sessions::new();
        assert!(started(&mut sessions, Answer::Granted(KEYBOARD)));
        sessions.close(H);
        sessions.create(H, APP, "Remote").expect("create again");
        assert_eq!(sessions.stage(H), Some(Stage::Created(AVAILABLE)));
        assert!(sessions.connect(H, APP).is_err(), "no grant is kept");
    }

    #[test]
    fn devices_cannot_be_selected_once_started_and_start_runs_once() {
        let mut sessions = Sessions::new();
        sessions.create(H, APP, "Remote").expect("create");
        sessions.ask(H, APP).expect("ask");
        assert!(sessions.select(H, APP, KEYBOARD).is_err());
        assert!(sessions.ask(H, APP).is_err());
        assert!(
            sessions.create(H, APP, "Remote").is_err(),
            "the handle exists"
        );
    }
}
