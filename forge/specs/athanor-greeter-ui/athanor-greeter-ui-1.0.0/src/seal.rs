//! The seal of SH12 in the greeter's top end corner (doc_shell.md, package 1d): the badge
//! the trust state file backs, read through `athanor-trust-state` so the rule exists once.
//!
//! athanor-greeter-client binds `/run/athanor-update` read-only into the sandbox, and only
//! after it checked, outside the sandbox, that the directory is root's and nobody else can
//! write it. Inside, the sandbox's user namespace maps only the greeter's own uid, so root's
//! file carries the overflow uid: that is the owner the reader requires there, and the
//! launcher's check is what makes it root's. Where this namespace has a root of its own (an
//! unconfined run, the test rig), the owner must be that root. A file that cannot be read
//! backs nothing: the seal shows the exclamation mark, never the check. The words are our
//! own translations; nothing from the file is shown.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use athanor_trust_state::{
    Badge, ReadError, Reason, State, UpdateState, STALE_AFTER_SECS, STATE_PATH,
};

use crate::i18n::tr;

/// The uid root's files carry here: 0 when uid 0 is mapped in this user namespace, the
/// kernel's overflow uid otherwise.
fn root_uid_here(uid_map: &str, overflow_uid: u32) -> u32 {
    let maps_zero = uid_map.lines().any(|line| {
        let mut fields = line.split_whitespace().map(str::parse::<u64>);
        matches!((fields.next(), fields.nth(1)), (Some(Ok(0)), Some(Ok(count))) if count > 0)
    });
    if maps_zero {
        0
    } else {
        overflow_uid
    }
}

/// The owner [`read`] requires. When `/proc` cannot say, root's uid 0, which a root file
/// seen from inside the sandbox never carries: the seal then shows the file as untrusted.
fn expected_owner() -> u32 {
    let proc_text = |path: &str| {
        std::fs::read_to_string(path)
            .map_err(|err| tracing::warn!(path, error = %err, "cannot read; the trust state is taken as untrusted"))
            .ok()
    };
    let Some(uid_map) = proc_text("/proc/self/uid_map") else {
        return 0;
    };
    let overflow =
        proc_text("/proc/sys/kernel/overflowuid").and_then(|text| text.trim().parse().ok());
    overflow.map_or(0, |overflow| root_uid_here(&uid_map, overflow))
}

/// The published state, as the seal may trust it.
pub fn read() -> Result<State, ReadError> {
    athanor_trust_state::read_owned_by(Path::new(STATE_PATH), expected_owner())
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// SH12's badge; a file that cannot be read backs nothing, so it is the exclamation mark.
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
        Err(ReadError::Malformed | ReadError::Io(_)) => {
            return tr("The trust state file could not be read")
        }
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
    const OVERFLOW: u32 = 65534;

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
    fn root_is_the_overflow_uid_only_where_the_namespace_has_no_root() {
        // bwrap's namespace: only the greeter's own uid.
        assert_eq!(
            root_uid_here("      977        977          1\n", OVERFLOW),
            OVERFLOW
        );
        // The initial namespace, and a rootless container whose root is a user outside.
        assert_eq!(
            root_uid_here("         0          0 4294967295\n", OVERFLOW),
            0
        );
        assert_eq!(root_uid_here("0 1000 1\n1 524288 65536\n", OVERFLOW), 0);
        // A range that maps nothing, or text that is not a map, maps no root.
        assert_eq!(root_uid_here("0 0 0\n", OVERFLOW), OVERFLOW);
        assert_eq!(root_uid_here("garbage\n", OVERFLOW), OVERFLOW);
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
    }
}
