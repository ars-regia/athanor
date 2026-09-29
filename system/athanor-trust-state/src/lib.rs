//! The update and trust state of an Athanor machine, as `athanor-update` publishes it in
//! `/run/athanor-update/state.json` (docs/architecture/doc_update_trust.md, UT7), and the
//! badge rule of the trust shield (docs/architecture/doc_shell.md, SH12).
//!
//! The greeter, the shield and the notifier all read the file through this crate, so the
//! rule exists once. The file is untrusted input for a reader: it is opened without
//! following a link, its owner is checked, its size is capped, every list in it is closed,
//! and [`display`] is the only way one of its strings should reach a widget.

use serde::{Deserialize, Serialize};
use std::io::Read as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::Path;

/// Where `athanor-update` publishes the state.
pub const STATE_PATH: &str = "/run/athanor-update/state.json";
/// The only schema this crate reads and writes.
pub const SCHEMA: u32 = 1;
/// A machine whose last successful check is older than this is not shown as verified.
pub const STALE_AFTER_SECS: i64 = 14 * 24 * 60 * 60;
const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_DISPLAY_CHARS: usize = 128;

/// One deployment of the system image. Times are seconds since the Unix epoch, UTC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub image: String,
    pub digest: String,
    pub version: String,
    pub build_time: i64,
}

/// Why the booted image is, or is not, verified. Only `Signature` means verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Signature,
    Media,
    NoSignature,
    KeyNotInPolicy,
    PolicyNotInForce,
    ReferenceOutOfScope,
}

/// The `verified` member of the file. The pair is redundant on purpose, so a reader can
/// refuse a file whose two halves disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verified {
    pub value: bool,
    pub reason: Reason,
}

impl From<Reason> for Verified {
    fn from(reason: Reason) -> Self {
        Self { value: reason == Reason::Signature, reason }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateState {
    None,
    Available,
    Downloaded,
    WillApplyAtNextShutdown,
    Refused,
    Held,
    OlderThanBooted,
}

/// The result of the last check. A code, never the text of an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    None,
    Network,
    Registry,
    Policy,
    Storage,
    Internal,
}

/// The signature policy the container tools resolve on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub path: String,
    pub sha256: String,
    pub shipped: bool,
}

/// The four readings of UT8, each absent when its source cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecureBoot {
    pub secure_boot: Option<u8>,
    pub setup_mode: Option<u8>,
    pub mok_sb_state: Option<u8>,
    pub lockdown: Option<String>,
}

impl SecureBoot {
    /// "On" means `SecureBoot` 1, `SetupMode` 0, shim validation not disabled (the MOK
    /// variable absent) and a kernel lockdown other than `none`.
    #[must_use]
    pub fn on(&self) -> bool {
        self.secure_boot == Some(1)
            && self.setup_mode == Some(0)
            && self.mok_sb_state.is_none()
            && self.lockdown.as_deref().is_some_and(|mode| mode != "none")
    }
}

/// Schema 1 of the state file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema: u32,
    pub booted: Deployment,
    pub downloaded: Option<Deployment>,
    pub previous: Option<Deployment>,
    pub verified: Verified,
    pub update: UpdateState,
    pub policy: Policy,
    pub secure_boot: SecureBoot,
    /// The newest build time this machine has ever booted; persisted under `/var/lib`.
    pub newest_booted_build_time: i64,
    /// When the registry last answered a check; persisted under `/var/lib`.
    pub last_successful_check: Option<i64>,
    pub last_error: ErrorCode,
    /// At most the registry's host name, and only beside a `network` or `registry` code.
    pub last_error_host: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The file does not exist: `athanor-update` has not run yet.
    Missing,
    /// It is a link, not a regular file, not owned by the expected user, writable by
    /// others, or larger than a state file can be.
    Untrusted,
    /// It is not schema 1, a list in it holds an unknown value, or its halves disagree.
    Malformed,
    Io(std::io::ErrorKind),
}

/// Parses the text of a state file.
///
/// # Errors
/// `Malformed` when the text is not schema 1 or contradicts itself.
pub fn parse(text: &str) -> Result<State, ReadError> {
    let state: State = serde_json::from_str(text).map_err(|_| ReadError::Malformed)?;
    let consistent = state.verified.value == (state.verified.reason == Reason::Signature);
    if state.schema != SCHEMA || !consistent {
        return Err(ReadError::Malformed);
    }
    Ok(state)
}

/// Reads the published state, which must be a regular file owned by root.
///
/// # Errors
/// See [`ReadError`].
pub fn read() -> Result<State, ReadError> {
    read_owned_by(Path::new(STATE_PATH), 0)
}

/// Reads `path`, refusing a link, a file not owned by `owner`, and one others can write.
///
/// # Errors
/// See [`ReadError`].
pub fn read_owned_by(path: &Path, owner: u32) -> Result<State, ReadError> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => ReadError::Missing,
            // ELOOP: the last component is a symbolic link.
            _ if err.raw_os_error() == Some(libc::ELOOP) => ReadError::Untrusted,
            kind => ReadError::Io(kind),
        })?;
    // The checks are made on the open descriptor, so they describe the bytes read below.
    let meta = file.metadata().map_err(|err| ReadError::Io(err.kind()))?;
    if !meta.is_file() || meta.uid() != owner || meta.mode() & 0o022 != 0 || meta.len() > MAX_STATE_BYTES {
        return Err(ReadError::Untrusted);
    }
    let mut text = String::new();
    file.by_ref().take(MAX_STATE_BYTES).read_to_string(&mut text).map_err(|_| ReadError::Malformed)?;
    parse(&text)
}

/// The three badges of SH12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    /// Verified, the newest version this machine has booted, checked within 14 days.
    Check,
    /// Anything else that is not a refusal.
    Attention,
    /// The last download was refused by the policy.
    Cross,
}

/// The badge for `state` at `now` (seconds since the epoch).
///
/// `older-than-booted` and `held` are not badge states: they are the normal answers on a
/// machine that went back, and that machine is already at `Attention` because it runs an
/// older version than one it has booted.
#[must_use]
pub fn badge(state: &State, now: i64) -> Badge {
    if state.update == UpdateState::Refused {
        return Badge::Cross;
    }
    let newest = state.booted.build_time >= state.newest_booted_build_time;
    let fresh = state.last_successful_check.is_some_and(|at| now.saturating_sub(at) <= STALE_AFTER_SECS);
    if state.verified.value && newest && fresh {
        Badge::Check
    } else {
        Badge::Attention
    }
}

/// A string of the state file made safe to show: control characters and bidirectional
/// controls removed, at most 128 characters. Set the result as plain text, never markup.
#[must_use]
pub fn display(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .take(MAX_DISPLAY_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    const DAY: i64 = 24 * 60 * 60;
    const NOW: i64 = 1_790_000_000;

    fn deployment(build_time: i64) -> Deployment {
        Deployment {
            image: "registry.example/athanor-system:stable".into(),
            digest: format!("sha256:{build_time:064x}"),
            version: "43.20260915.2".into(),
            build_time,
        }
    }

    fn verified_state() -> State {
        State {
            schema: SCHEMA,
            booted: deployment(1000),
            downloaded: None,
            previous: Some(deployment(900)),
            verified: Reason::Signature.into(),
            update: UpdateState::None,
            policy: Policy { path: "/etc/containers/policy.json".into(), sha256: "ab".repeat(32), shipped: true },
            secure_boot: SecureBoot { secure_boot: Some(1), setup_mode: Some(0), mok_sb_state: None, lockdown: Some("integrity".into()) },
            newest_booted_build_time: 1000,
            last_successful_check: Some(NOW - DAY),
            last_error: ErrorCode::None,
            last_error_host: None,
        }
    }

    #[test]
    fn verified_newest_and_fresh_is_the_check() {
        assert_eq!(badge(&verified_state(), NOW), Badge::Check);
    }

    #[test]
    fn every_reason_but_signature_is_attention() {
        for reason in [Reason::Media, Reason::NoSignature, Reason::KeyNotInPolicy, Reason::PolicyNotInForce, Reason::ReferenceOutOfScope] {
            let state = State { verified: reason.into(), ..verified_state() };
            assert_eq!(badge(&state, NOW), Badge::Attention, "{reason:?}");
        }
    }

    #[test]
    fn fourteen_days_is_the_last_fresh_day() {
        let at_limit = State { last_successful_check: Some(NOW - STALE_AFTER_SECS), ..verified_state() };
        assert_eq!(badge(&at_limit, NOW), Badge::Check);
        let past = State { last_successful_check: Some(NOW - STALE_AFTER_SECS - 1), ..verified_state() };
        assert_eq!(badge(&past, NOW), Badge::Attention);
        let never = State { last_successful_check: None, ..verified_state() };
        assert_eq!(badge(&never, NOW), Badge::Attention);
    }

    #[test]
    fn a_machine_that_went_back_is_attention_and_never_a_cross() {
        for update in [UpdateState::Held, UpdateState::OlderThanBooted] {
            let state = State { update, newest_booted_build_time: 2000, ..verified_state() };
            assert_eq!(badge(&state, NOW), Badge::Attention);
            // The update state alone does not move the badge.
            assert_eq!(badge(&State { update, ..verified_state() }, NOW), Badge::Check);
        }
    }

    #[test]
    fn a_refusal_is_the_cross_whatever_else_holds() {
        assert_eq!(badge(&State { update: UpdateState::Refused, ..verified_state() }, NOW), Badge::Cross);
    }

    #[test]
    fn secure_boot_never_moves_the_badge() {
        let off = SecureBoot { secure_boot: Some(0), setup_mode: Some(1), mok_sb_state: Some(1), lockdown: Some("none".into()) };
        assert!(!off.on());
        assert!(verified_state().secure_boot.on());
        assert_eq!(badge(&State { secure_boot: off, ..verified_state() }, NOW), Badge::Check);
    }

    #[test]
    fn the_lists_are_closed_and_the_schema_is_one() {
        let good = serde_json::to_string(&verified_state()).expect("serialize");
        assert_eq!(parse(&good), Ok(verified_state()));
        assert!(good.contains(r#""reason":"signature""#) && good.contains(r#""update":"none""#));
        for (from, to) in [
            (r#""reason":"signature""#, r#""reason":"trusted""#),
            (r#""update":"none""#, r#""update":"ready""#),
            (r#""last_error":"none""#, r#""last_error":"dns: lookup registry.example""#),
            (r#""schema":1"#, r#""schema":2"#),
            (r#""schema":1"#, r#""schema":1,"extra":true"#),
            // The two halves of `verified` disagree.
            (r#""reason":"signature""#, r#""reason":"media""#),
        ] {
            assert_eq!(parse(&good.replace(from, to)), Err(ReadError::Malformed), "{to}");
        }
    }

    #[test]
    fn will_apply_at_next_shutdown_is_spelled_as_the_spec_spells_it() {
        let text = serde_json::to_string(&UpdateState::WillApplyAtNextShutdown).expect("serialize");
        assert_eq!(text, r#""will-apply-at-next-shutdown""#);
        assert_eq!(serde_json::to_string(&UpdateState::OlderThanBooted).expect("serialize"), r#""older-than-booted""#);
        assert_eq!(serde_json::to_string(&Reason::ReferenceOutOfScope).expect("serialize"), r#""reference-out-of-scope""#);
    }

    fn scratch(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-trust-state-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn the_reader_refuses_a_link_a_foreign_owner_and_a_writable_file() {
        let dir = scratch("reader");
        let me = std::fs::metadata(&dir).expect("metadata").uid();
        let file = dir.join("state.json");
        std::fs::write(&file, serde_json::to_string(&verified_state()).expect("serialize")).expect("write");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");

        assert_eq!(read_owned_by(&file, me), Ok(verified_state()));
        assert_eq!(read_owned_by(&file, me + 1), Err(ReadError::Untrusted));
        assert_eq!(read_owned_by(&dir.join("absent.json"), me), Err(ReadError::Missing));

        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&file, &link).expect("symlink");
        assert_eq!(read_owned_by(&link, me), Err(ReadError::Untrusted));

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).expect("chmod");
        assert_eq!(read_owned_by(&file, me), Err(ReadError::Untrusted));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn display_strips_controls_and_bidi_and_truncates() {
        assert_eq!(display("43.2026\u{202E}0915\n.2\u{2066}"), "43.20260915.2");
        assert_eq!(display(&"x".repeat(500)).chars().count(), 128);
    }
}
