//! The seal of SH12 in the greeter's top end corner (doc_shell.md, package 1d): the badge
//! the trust state backs, decided through `athanor-trust-state` so the rule exists once.
//!
//! The state comes from `os.athanor.Update1.State`, as the bar's does (doc_update_trust.md,
//! UT7), not from the file: inside the greeter's sandbox root is the overflow uid, so only
//! the root service can check the file's owner. athanor-greeter-client's filter lets that
//! one method through. The reply is trusted only when the bus, outside every namespace,
//! says root sent it. No answer, another sender, an error or a reply that is not a state
//! backs nothing: the seal shows the exclamation mark, never the check. The words are our
//! own translations; nothing from the state is shown.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use athanor_trust_state::{
    state_reply, Badge, ReadError, Reason, State, UpdateState, STALE_AFTER_SECS, STATE_METHOD,
    UPDATE_INTERFACE, UPDATE_NAME, UPDATE_PATH,
};

use crate::i18n::tr;

/// How long the service and the bus get to answer, as the bar gives them.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// The state, asked of the update service on the system bus; the call starts the service.
pub async fn ask() -> Result<State, ReadError> {
    match tokio::time::timeout(ANSWER_WITHIN, ask_once()).await {
        Ok(read) => read,
        Err(_) => {
            tracing::warn!("the update service did not answer State in time");
            state_reply(Err(None), None)
        }
    }
}

async fn ask_once() -> Result<State, ReadError> {
    let no_answer = state_reply(Err(None), None);
    let connection = match zbus::Connection::system().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(error = %err, "no system bus; the trust state cannot be asked");
            return no_answer;
        }
    };
    let reply = connection
        .call_method(
            Some(UPDATE_NAME),
            UPDATE_PATH,
            Some(UPDATE_INTERFACE),
            STATE_METHOD,
            &(),
        )
        .await;
    let (message, answer) = match &reply {
        // A body that is not one string is not a state: it parses as nothing.
        Ok(message) => (
            message,
            Ok(message.body().deserialize::<String>().unwrap_or_default()),
        ),
        Err(zbus::Error::MethodError(name, _, message)) => (message, Err(name.as_str())),
        Err(err) => {
            tracing::warn!(error = %err, "the update service did not answer State");
            return no_answer;
        }
    };
    let sender_uid = match message.header().sender() {
        Some(sender) => uid_of(&connection, sender).await,
        None => None,
    };
    let read = match &answer {
        Ok(json) => state_reply(Ok(json), sender_uid),
        Err(name) => state_reply(Err(Some(name)), sender_uid),
    };
    if let Err(err) = &read {
        tracing::warn!(?err, ?sender_uid, "the trust state is not trusted");
    }
    read
}

/// The uid the bus daemon recorded for the connection that sent the reply: the sender is
/// set by the bus, so this is who answered, not whoever owns the name by now.
async fn uid_of(
    connection: &zbus::Connection,
    sender: &zbus::names::UniqueName<'_>,
) -> Option<u32> {
    let uid = async {
        zbus::fdo::DBusProxy::new(connection)
            .await?
            .get_connection_unix_user(sender.clone().into())
            .await
    };
    match uid.await {
        Ok(uid) => Some(uid),
        Err(err) => {
            tracing::warn!(error = %err, %sender, "the bus did not say who answered State");
            None
        }
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// SH12's badge; a state that was not trusted backs nothing, so it is the exclamation mark.
pub fn badge(read: &Result<State, ReadError>, now: i64) -> Badge {
    read.as_ref().map_or(Badge::Attention, |state| {
        athanor_trust_state::badge(state, now)
    })
}

pub fn icon(badge: Badge) -> &'static str {
    match badge {
        Badge::Check => "athanor-seal-verified-symbolic",
        Badge::Attention => "athanor-seal-attention-symbolic",
        Badge::Cross => "athanor-seal-blocked-symbolic",
    }
}

/// The seal's words and accessible name: the sheet header of the bar's shield (doc_bar.md,
/// BR6), which claims only what the badge backs.
pub fn header(badge: Badge) -> String {
    match badge {
        Badge::Check => tr("System image verified"),
        Badge::Attention => tr("Not verified yet"),
        Badge::Cross => tr("Update refused"),
    }
}

/// The tooltip: the one reason that decided the badge.
pub fn detail(read: &Result<State, ReadError>, now: i64) -> String {
    let state = match read {
        Ok(state) => state,
        Err(ReadError::Missing) => return tr("No trust state yet: the first check has not run"),
        Err(ReadError::Untrusted) => {
            return tr("The trust state file is not owned by the system and was ignored")
        }
        Err(ReadError::Malformed) => return tr("The trust state file could not be read"),
        Err(ReadError::Io(_)) => return tr("The update service did not answer"),
    };
    if state.update == UpdateState::Refused {
        return tr("The last update was refused by the policy");
    }
    match state.verified.reason {
        Reason::Signature => {}
        Reason::Media => return tr("Not verified: installed from media"),
        Reason::NoSignature => return tr("Not verified: no signature"),
        Reason::KeyNotInPolicy => return tr("Not verified: its key is not in the policy"),
        Reason::PolicyNotInForce => return tr("Not verified: fetched under a permissive policy"),
        Reason::ReferenceOutOfScope => {
            return tr("Not verified: the image is outside the policy's scope")
        }
        Reason::LocalChanges => return tr("Not verified: the system was changed on this machine"),
        Reason::ChannelAbsent => {
            return tr("Not verified: waiting for the stable channel to be published")
        }
        Reason::OriginNotEnforcing => {
            return tr("Not verified: this machine does not verify its updates")
        }
        Reason::PinnedBuild => {
            return tr("Signed, but pinned to one build: it receives no updates")
        }
    }
    if state.booted.build_time < state.newest_booted_build_time {
        return tr("This version is older than one this machine has run");
    }
    if !state
        .last_successful_check
        .is_some_and(|at| now.saturating_sub(at) <= STALE_AFTER_SECS)
    {
        return tr("No check for updates has succeeded for 14 days");
    }
    tr("Signed with a key of the policy in force")
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_trust_state::{Deployment, ErrorCode, Policy, SecureBoot, SCHEMA};

    const NOW: i64 = 1_789_725_600;

    fn verified() -> State {
        let booted = Deployment {
            image: "registry.example/athanor-system:stable".into(),
            digest: format!("sha256:{}", "1".repeat(64)),
            version: "43.20260915.2".into(),
            build_time: NOW - 3 * 86_400,
        };
        State {
            schema: SCHEMA,
            booted: booted.clone(),
            downloaded: None,
            previous: None,
            verified: Reason::Signature.into(),
            update: UpdateState::None,
            policy: Policy {
                path: "/etc/containers/policy.json".into(),
                sha256: "ab".repeat(32),
                shipped: true,
            },
            secure_boot: SecureBoot {
                secure_boot: Some(1),
                setup_mode: Some(0),
                mok_sb_state: None,
                lockdown: Some("integrity".into()),
            },
            newest_booted_build_time: booted.build_time,
            last_successful_check: Some(NOW - 30 * 3600),
            last_error: ErrorCode::None,
            last_error_host: None,
        }
    }

    #[test]
    fn a_state_that_cannot_be_read_never_shows_the_check() {
        for error in [
            ReadError::Missing,
            ReadError::Untrusted,
            ReadError::Malformed,
            ReadError::Io(std::io::ErrorKind::PermissionDenied),
        ] {
            assert_eq!(badge(&Err(error), NOW), Badge::Attention);
        }
        assert_eq!(badge(&Ok(verified()), NOW), Badge::Check);
    }

    #[test]
    fn each_badge_has_its_own_icon_and_words() {
        let badges = [Badge::Check, Badge::Attention, Badge::Cross];
        for (index, one) in badges.iter().enumerate() {
            for other in &badges[index + 1..] {
                assert_ne!(icon(*one), icon(*other));
                assert_ne!(header(*one), header(*other));
            }
        }
    }

    #[test]
    fn the_detail_names_what_decided_the_badge() {
        let detail_of = |state: State| detail(&Ok(state), NOW);
        assert_eq!(
            detail_of(verified()),
            "Signed with a key of the policy in force"
        );
        assert_eq!(
            detail_of(State {
                update: UpdateState::Refused,
                verified: Reason::Media.into(),
                ..verified()
            }),
            "The last update was refused by the policy"
        );
        assert_eq!(
            detail_of(State {
                verified: Reason::Media.into(),
                ..verified()
            }),
            "Not verified: installed from media"
        );
        assert_eq!(
            detail_of(State {
                newest_booted_build_time: NOW,
                ..verified()
            }),
            "This version is older than one this machine has run"
        );
        assert_eq!(
            detail_of(State {
                last_successful_check: Some(NOW - STALE_AFTER_SECS - 1),
                ..verified()
            }),
            "No check for updates has succeeded for 14 days"
        );
        assert_eq!(
            detail(&Err(ReadError::Untrusted), NOW),
            "The trust state file is not owned by the system and was ignored"
        );
        assert_eq!(
            detail(&Err(ReadError::Io(std::io::ErrorKind::NotConnected)), NOW),
            "The update service did not answer"
        );
    }
}
