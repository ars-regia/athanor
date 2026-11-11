//! Shared polkit client for every Athanor daemon: the `Subject`/result wire types, the
//! `org.freedesktop.PolicyKit1.Authority` proxy, and the caller-authorization helpers.
//!
//! One copy on purpose. The per-daemon copies this replaces had drifted, and every one of
//! them built the subject from the daemon's *own* bus connection, i.e. from the message
//! bus's credentials instead of the caller's (see `ANALISI_2026-09-02.md` and
//! `scripts/verify.py polkit-subject`). Rules encoded here:
//!
//! - A daemon on the **system bus** passes the caller's unique name as a `system-bus-name`
//!   subject ([`check_polkit_auth_zbus`]). polkit resolves the name to a process itself,
//!   with no PID-reuse window on our side.
//! - A daemon on the **session bus** cannot do that: its callers' names do not exist on the
//!   system bus where polkit lives. It resolves the caller's PID/UID on its own bus
//!   ([`unix_process_subject`]) and sends a `unix-process` subject over a system-bus
//!   connection ([`check_subject`]).
//! - Nobody short-circuits on uid 0. Root callers are authorized by polkit like everyone else.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zbus::names::BusName;
use zbus::{OwnedVariant, Type, Variant};

/// polkit `Subject`, `(sa{sv})` on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PolkitSubject {
    /// `"system-bus-name"` or `"unix-process"`.
    pub kind: String,
    /// Kind-specific details: `name`, or `pid` / `start-time` / `uid`.
    pub details: HashMap<String, OwnedVariant>,
}

impl PolkitSubject {
    /// Subject for a caller identified by its unique name on the **system** bus.
    pub fn system_bus_name(name: impl Into<String>) -> Self {
        let name: String = name.into();
        let mut details = HashMap::new();
        insert(&mut details, "name", Variant::from(name));
        Self {
            kind: "system-bus-name".to_string(),
            details,
        }
    }

    /// Subject for a caller identified by PID. `start-time` is mandatory on the wire; `0`
    /// tells polkit to read it from `/proc` itself. `uid`, when the bus knows it, pins the
    /// owner polkit evaluates instead of letting polkit look it up later.
    pub fn unix_process(pid: u32, uid: Option<u32>) -> Self {
        let mut details = HashMap::new();
        insert(&mut details, "pid", Variant::from(pid));
        insert(&mut details, "start-time", Variant::from(0u64));
        if let Some(uid) = uid {
            insert(&mut details, "uid", Variant::from(uid as i32));
        }
        Self { kind: "unix-process".to_string(), details }
    }
}

/// `OwnedVariant::try_from` only fails for fd values, which never occur here.
fn insert(details: &mut HashMap<String, OwnedVariant>, key: &str, value: Variant<'_>) {
    if let Ok(owned) = OwnedVariant::try_from(value) {
        details.insert(key.to_string(), owned);
    }
}

/// What `CheckAuthorization` returns, `(bba{ss})` on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PolkitAuthorizationResult {
    pub is_authorized: bool,
    pub is_challenge: bool,
    pub details: HashMap<String, String>,
}

/// Proxy for `org.freedesktop.PolicyKit1.Authority` (system bus).
#[zbus::proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
pub trait PolicyKitAuthority {
    /// Asks polkit whether `subject` is authorized for `action_id`.
    fn check_authorization(
        &self,
        subject: &PolkitSubject,
        action_id: &str,
        details: &HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<PolkitAuthorizationResult>;
}

/// `CheckAuthorizationFlags.AllowUserInteraction`.
const ALLOW_USER_INTERACTION: u32 = 1;

/// Authorizes `sender` (the caller's unique name from the message header) for `action_id`.
/// For daemons whose `conn` is the **system bus**: the subject is the bus name itself.
///
/// # Errors
/// Proxy construction or the D-Bus call failed. A polkit "no" is `Ok(false)`, not an error.
pub async fn check_polkit_auth_zbus(
    conn: &zbus::Connection,
    sender: &str,
    action_id: &str,
    allow_user_interaction: bool,
) -> zbus::Result<bool> {
    check_subject(conn, &PolkitSubject::system_bus_name(sender), action_id, allow_user_interaction).await
}

/// Asks polkit, reached over `system_conn`, whether `subject` may perform `action_id`.
///
/// # Errors
/// Proxy construction or the D-Bus call failed. A polkit "no" is `Ok(false)`, not an error.
pub async fn check_subject(
    system_conn: &zbus::Connection,
    subject: &PolkitSubject,
    action_id: &str,
    allow_user_interaction: bool,
) -> zbus::Result<bool> {
    let proxy = PolicyKitAuthorityProxy::new(system_conn).await?;
    let flags = if allow_user_interaction { ALLOW_USER_INTERACTION } else { 0 };
    let result = proxy
        .check_authorization(subject, action_id, &HashMap::new(), flags, "")
        .await?;
    Ok(result.is_authorized)
}

/// Resolves `sender` (a unique name on `bus`, the connection the call arrived on) to a
/// `unix-process` subject through the bus driver's `GetConnectionCredentials`. For daemons
/// serving on the session bus, whose callers polkit cannot see by name.
///
/// # Errors
/// `sender` is not a valid bus name, the bus driver call failed (e.g. the caller already
/// disconnected), or the driver reported no PID for it.
pub async fn unix_process_subject(bus: &zbus::Connection, sender: &str) -> zbus::Result<PolkitSubject> {
    let name = BusName::try_from(sender)?;
    let creds = zbus::fdo::DBusProxy::new(bus).await?.get_connection_credentials(name).await?;
    let pid = creds
        .process_id()
        .ok_or_else(|| zbus::Error::Failure(format!("bus driver reports no PID for {sender}")))?;
    // ponytail: pid + start-time=0 leaves polkit a PID-reuse window between our lookup and its
    // /proc read. Pass creds.process_fd() as a `pidfd` detail once polkit >= 124 is the floor.
    Ok(PolkitSubject::unix_process(pid, creds.unix_user_id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail<'a>(s: &'a PolkitSubject, key: &str) -> Option<&'a Variant<'static>> {
        s.details.get(key).map(|v| &**v)
    }

    #[test]
    fn wire_signatures_match_polkit() {
        assert_eq!(PolkitSubject::SIGNATURE.to_string(), "(sa{sv})");
        assert_eq!(PolkitAuthorizationResult::SIGNATURE.to_string(), "(bba{ss})");
    }

    #[test]
    fn unix_process_carries_pid_start_time_and_uid_with_polkit_types() {
        let s = PolkitSubject::unix_process(4242, Some(1000));
        assert_eq!(s.kind, "unix-process");
        assert!(matches!(detail(&s, "pid"), Some(Variant::U32(4242))));
        assert!(matches!(detail(&s, "start-time"), Some(Variant::U64(0))));
        assert!(matches!(detail(&s, "uid"), Some(Variant::I32(1000))));
        assert!(detail(&PolkitSubject::unix_process(1, None), "uid").is_none());
    }

    #[test]
    fn system_bus_name_carries_the_name() {
        let s = PolkitSubject::system_bus_name(":1.42");
        assert_eq!(s.kind, "system-bus-name");
        assert!(matches!(detail(&s, "name"), Some(Variant::Str(n)) if n.as_str() == ":1.42"));
    }
}
