//! The trust shield and its sheet (doc_bar.md BR6, doc_shell.md SH12): which seal, which
//! rows, which actions, and what a refusal of `os.athanor.Update1` means. No words here: the
//! binary's `ui::shield` says each of these in our own translations, so "verified" never
//! comes from the file. Every string taken from the file passes `athanor_trust_state::display`.

use athanor_trust_state::{display, Badge, ErrorCode, ReadError, Reason, State, UpdateState};

pub const UPDATE_NAME: &str = "os.athanor.Update1";
pub const UPDATE_PATH: &str = "/os/athanor/Update1";
pub const UPDATE_INTERFACE: &str = "os.athanor.Update1";
pub const STATE_METHOD: &str = "State";
const ERROR_PREFIX: &str = "os.athanor.Update1.Error.";

#[must_use]
pub fn icon(badge: Badge) -> &'static str {
    match badge {
        Badge::Check => "athanor-seal-verified-symbolic",
        Badge::Attention => "athanor-seal-attention-symbolic",
        Badge::Cross => "athanor-seal-blocked-symbolic",
    }
}

/// SH12's badge; a file that cannot be read backs nothing, so it is Attention.
#[must_use]
pub fn badge(read: &Result<State, ReadError>, now: i64) -> Badge {
    read.as_ref().map_or(Badge::Attention, |state| athanor_trust_state::badge(state, now))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// No file yet: the first check has not run.
    Missing,
    /// Not owned by root, or a link: ignored (UT7).
    Untrusted,
    /// Anything else that is not a schema-1 state.
    Malformed,
    /// The update service did not answer, or the bus could not say who did.
    NoAnswer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rows {
    pub version: String,
    pub build_time: i64,
    pub reason: Reason,
    pub last_check: Option<i64>,
    pub update: UpdateState,
    /// After a failed check: the code, and at most the host name.
    pub error: Option<(ErrorCode, Option<String>)>,
    pub policy_in_force: bool,
    pub policy_shipped: bool,
    pub secure_boot_on: bool,
    pub restart_to_update: bool,
    pub go_back: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sheet {
    Unreadable(Unreadable),
    Read(Rows),
}

#[must_use]
pub fn sheet(read: &Result<State, ReadError>) -> Sheet {
    let state = match read {
        Ok(state) => state,
        Err(ReadError::Missing) => return Sheet::Unreadable(Unreadable::Missing),
        Err(ReadError::Untrusted) => return Sheet::Unreadable(Unreadable::Untrusted),
        Err(ReadError::Malformed) => return Sheet::Unreadable(Unreadable::Malformed),
        Err(ReadError::Io(_)) => return Sheet::Unreadable(Unreadable::NoAnswer),
    };
    Sheet::Read(Rows {
        version: display(&state.booted.version),
        build_time: state.booted.build_time,
        reason: state.verified.reason,
        last_check: state.last_successful_check,
        update: state.update,
        error: (state.last_error != ErrorCode::None)
            .then(|| (state.last_error, state.last_error_host.as_deref().map(display))),
        // UT7's closed list names the one reason that says the policy was not in force.
        policy_in_force: state.verified.reason != Reason::PolicyNotInForce,
        policy_shipped: state.policy.shipped,
        secure_boot_on: state.secure_boot.on(),
        restart_to_update: restart_to_update_offered(read),
        go_back: go_back_offered(read),
    })
}

/// UT6: `Apply()` refuses unless the state is `downloaded`, and never downloads.
#[must_use]
pub fn restart_to_update_offered(read: &Result<State, ReadError>) -> bool {
    read.as_ref().is_ok_and(|state| state.update == UpdateState::Downloaded && state.downloaded.is_some())
}

/// UT6: `GoBack()` targets the immediately previous deployment.
#[must_use]
pub fn go_back_offered(read: &Result<State, ReadError>) -> bool {
    read.as_ref().is_ok_and(|state| state.previous.is_some())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Apply,
    GoBack,
}

impl Request {
    #[must_use]
    pub fn method(self) -> &'static str {
        match self {
            Request::Apply => "Apply",
            Request::GoBack => "GoBack",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAuthorized,
    Busy,
    NothingDownloaded,
    NoPreviousVersion,
    Blocked,
    Failed,
    /// The service is not there, did not answer, or the system bus is not reachable.
    NoAnswer,
}

/// What a confirmation says when the state stopped offering `request` while it was open:
/// the request's own refusal, unless the service no longer answers, which proves nothing
/// about a download or a previous version.
#[must_use]
pub fn withdrawn(read: &Result<State, ReadError>, request: Request) -> Refusal {
    match (read, request) {
        (Err(ReadError::Io(_)), _) => Refusal::NoAnswer,
        (_, Request::Apply) => Refusal::NothingDownloaded,
        (_, Request::GoBack) => Refusal::NoPreviousVersion,
    }
}

/// The refusal a failed call stands for, from its D-Bus error name (`None` for a local
/// error). An `os.athanor.Update1.Error` this build does not know is a failure, not silence.
#[must_use]
pub fn refusal(remote_error: Option<&str>) -> Refusal {
    let Some(name) = remote_error else { return Refusal::NoAnswer };
    match name.strip_prefix(ERROR_PREFIX) {
        Some("NotAuthorized") => Refusal::NotAuthorized,
        Some("Busy") => Refusal::Busy,
        Some("NothingDownloaded") => Refusal::NothingDownloaded,
        Some("NoPreviousVersion") => Refusal::NoPreviousVersion,
        Some("Blocked") => Refusal::Blocked,
        Some(_) => Refusal::Failed,
        None => Refusal::NoAnswer,
    }
}

/// The state `os.athanor.Update1.State` answered: its JSON, or its D-Bus error name (`None`
/// for a local error), and the uid of the connection that sent the reply. Only root's
/// answer is trusted, whatever it says: the service reads the file as root, which is the
/// owner check this process cannot make from inside its user namespace.
///
/// # Errors
/// `Io` when the service did not answer or the bus could not name the sender's uid,
/// `Untrusted` when another user answered, and the service's own verdict otherwise.
pub fn state_reply(
    answer: Result<&str, Option<&str>>,
    sender_uid: Option<u32>,
) -> Result<State, ReadError> {
    let service_error = match answer {
        Err(None) => return Err(ReadError::Io(std::io::ErrorKind::NotConnected)),
        Err(Some(name)) => match name.strip_prefix(ERROR_PREFIX) {
            Some(error) => Some(error),
            // The bus answered for it: not activatable, no reply in time.
            None => return Err(ReadError::Io(std::io::ErrorKind::NotConnected)),
        },
        Ok(_) => None,
    };
    match sender_uid {
        Some(0) => {}
        Some(_) => return Err(ReadError::Untrusted),
        // Not trusted either way; but no one was shown to be another user.
        None => return Err(ReadError::Io(std::io::ErrorKind::NotConnected)),
    }
    match (answer, service_error) {
        (Ok(json), _) => athanor_trust_state::parse(json),
        (_, Some("NoState")) => Err(ReadError::Missing),
        (_, Some("Untrusted")) => Err(ReadError::Untrusted),
        _ => Err(ReadError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_trust_state::{Deployment, ErrorCode, Reason, UpdateState};

    const NOW: i64 = 1_789_725_600;

    fn verified() -> State {
        let text =
            include_str!("../../../athanor-update/athanor-update-1.0.0/tests/state-verified.json");
        athanor_trust_state::parse(text).expect("fixture")
    }

    fn downloaded() -> State {
        let mut state = verified();
        state.downloaded = Some(Deployment {
            digest: "sha256:2".into(),
            ..state.booted.clone()
        });
        state.previous = Some(state.booted.clone());
        state.update = UpdateState::Downloaded;
        state
    }

    #[test]
    fn each_badge_has_its_seal() {
        assert_eq!(icon(Badge::Check), "athanor-seal-verified-symbolic");
        assert_eq!(icon(Badge::Attention), "athanor-seal-attention-symbolic");
        assert_eq!(icon(Badge::Cross), "athanor-seal-blocked-symbolic");
    }

    #[test]
    fn an_unreadable_file_is_attention_and_says_why() {
        for (error, why) in [
            (ReadError::Missing, Unreadable::Missing),
            (ReadError::Untrusted, Unreadable::Untrusted),
            (ReadError::Malformed, Unreadable::Malformed),
            (
                ReadError::Io(std::io::ErrorKind::NotConnected),
                Unreadable::NoAnswer,
            ),
        ] {
            let read = Err(error);
            assert_eq!(badge(&read, NOW), Badge::Attention);
            assert_eq!(sheet(&read), Sheet::Unreadable(why));
            assert!(!restart_to_update_offered(&read) && !go_back_offered(&read));
        }
    }

    #[test]
    fn restart_to_update_needs_a_downloaded_deployment() {
        assert!(restart_to_update_offered(&Ok(downloaded())));
        let mut available = downloaded();
        available.update = UpdateState::Available;
        assert!(
            !restart_to_update_offered(&Ok(available)),
            "UT6: Apply() never downloads"
        );
        let mut no_deployment = downloaded();
        no_deployment.downloaded = None;
        assert!(!restart_to_update_offered(&Ok(no_deployment)));
        assert!(!restart_to_update_offered(&Ok(verified())));
    }

    #[test]
    fn go_back_needs_a_previous_deployment() {
        assert!(go_back_offered(&Ok(downloaded())));
        assert!(!go_back_offered(&Ok(verified())), "the fixture has none");
    }

    #[test]
    fn the_rows_carry_what_the_file_backs() {
        let Sheet::Read(rows) = sheet(&Ok(downloaded())) else {
            panic!("read")
        };
        assert_eq!(rows.version, "43.20260915.2");
        assert_eq!(rows.build_time, 1_789_466_400);
        assert_eq!(rows.reason, Reason::Signature);
        assert_eq!(rows.update, UpdateState::Downloaded);
        assert_eq!(rows.last_check, Some(1_789_900_000));
        assert_eq!(rows.error, None);
        assert!(rows.policy_in_force && rows.policy_shipped && rows.secure_boot_on);
        assert!(rows.restart_to_update && rows.go_back);
    }

    #[test]
    fn a_failed_check_shows_its_code_and_at_most_the_host() {
        let mut state = verified();
        state.last_error = ErrorCode::Registry;
        state.last_error_host = Some("registry.example".into());
        let Sheet::Read(rows) = sheet(&Ok(state)) else {
            panic!("read")
        };
        assert_eq!(
            rows.error,
            Some((ErrorCode::Registry, Some("registry.example".into())))
        );
    }

    #[test]
    fn a_permissive_policy_is_not_in_force() {
        let mut state = verified();
        state.verified.value = false;
        state.verified.reason = Reason::PolicyNotInForce;
        state.policy.shipped = false;
        let Sheet::Read(rows) = sheet(&Ok(state)) else {
            panic!("read")
        };
        assert!(!rows.policy_in_force && !rows.policy_shipped);
    }

    #[test]
    fn hostile_strings_are_displayed_plain_and_bounded() {
        let mut state = verified();
        state.booted.version = format!("43.\u{202E}evil\u{0007}{}", "9".repeat(300));
        state.last_error = ErrorCode::Network;
        state.last_error_host = Some("<b>host</b>\u{200F}.example".into());
        let Sheet::Read(rows) = sheet(&Ok(state)) else {
            panic!("read")
        };
        assert!(!rows.version.contains('\u{202E}') && !rows.version.contains('\u{0007}'));
        assert!(rows.version.chars().count() <= 128);
        assert_eq!(
            rows.error,
            Some((ErrorCode::Network, Some("<b>host</b>.example".into()))),
            "kept as text, bidi stripped"
        );
    }

    #[test]
    fn every_update1_error_has_its_refusal() {
        for (name, refusal_) in [
            (
                "os.athanor.Update1.Error.NotAuthorized",
                Refusal::NotAuthorized,
            ),
            ("os.athanor.Update1.Error.Busy", Refusal::Busy),
            (
                "os.athanor.Update1.Error.NothingDownloaded",
                Refusal::NothingDownloaded,
            ),
            (
                "os.athanor.Update1.Error.NoPreviousVersion",
                Refusal::NoPreviousVersion,
            ),
            ("os.athanor.Update1.Error.Blocked", Refusal::Blocked),
            ("os.athanor.Update1.Error.Failed", Refusal::Failed),
            (
                "org.freedesktop.DBus.Error.ServiceUnknown",
                Refusal::NoAnswer,
            ),
            ("org.freedesktop.DBus.Error.NoReply", Refusal::NoAnswer),
            ("os.athanor.Update1.Error.SomethingNew", Refusal::Failed),
        ] {
            assert_eq!(refusal(Some(name)), refusal_, "{name}");
        }
        assert_eq!(
            refusal(None),
            Refusal::NoAnswer,
            "a local error, such as no system bus"
        );
    }

    #[test]
    fn a_withdrawn_request_says_why_only_when_the_state_backs_it() {
        assert_eq!(withdrawn(&Ok(verified()), Request::Apply), Refusal::NothingDownloaded);
        assert_eq!(withdrawn(&Ok(verified()), Request::GoBack), Refusal::NoPreviousVersion);
        assert_eq!(withdrawn(&Err(ReadError::Missing), Request::Apply), Refusal::NothingDownloaded);
        let no_answer = Err(ReadError::Io(std::io::ErrorKind::NotConnected));
        assert_eq!(withdrawn(&no_answer, Request::Apply), Refusal::NoAnswer);
        assert_eq!(withdrawn(&no_answer, Request::GoBack), Refusal::NoAnswer);
    }

    #[test]
    fn requests_name_their_method() {
        assert_eq!(Request::Apply.method(), "Apply");
        assert_eq!(Request::GoBack.method(), "GoBack");
    }

    #[test]
    fn a_state_reply_is_trusted_only_from_root() {
        let json =
            include_str!("../../../athanor-update/athanor-update-1.0.0/tests/state-verified.json");
        assert_eq!(state_reply(Ok(json), Some(0)), Ok(verified()));
        assert_eq!(state_reply(Ok(json), Some(1000)), Err(ReadError::Untrusted), "another user owns the name");
        assert_eq!(
            state_reply(Ok(json), None),
            Err(ReadError::Io(std::io::ErrorKind::NotConnected)),
            "the bus could not say who answered: not trusted, and asked again"
        );
        assert_eq!(state_reply(Ok("{}"), Some(0)), Err(ReadError::Malformed));
    }

    #[test]
    fn every_state_error_has_its_row() {
        for (name, read) in [
            ("os.athanor.Update1.Error.NoState", Err(ReadError::Missing)),
            ("os.athanor.Update1.Error.Untrusted", Err(ReadError::Untrusted)),
            ("os.athanor.Update1.Error.Unreadable", Err(ReadError::Malformed)),
            ("os.athanor.Update1.Error.SomethingNew", Err(ReadError::Malformed)),
        ] {
            assert_eq!(state_reply(Err(Some(name)), Some(0)), read, "{name}");
            assert_eq!(state_reply(Err(Some(name)), Some(1000)), Err(ReadError::Untrusted), "{name} from another user");
        }
        for name in [Some("org.freedesktop.DBus.Error.ServiceUnknown"), Some("org.freedesktop.DBus.Error.NoReply"), None] {
            assert!(
                matches!(state_reply(Err(name), None), Err(ReadError::Io(_))),
                "{name:?}: the service did not answer"
            );
        }
    }
}
