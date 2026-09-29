//! The two notification interfaces (doc_bar.md BR1, BR4): the specification's, open to
//! every application, and the bar's, which answers athanor-bar.service only. Both live on
//! one connection and share one store.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::names::OwnedUniqueName;
use zbus::object_server::SignalEmitter;
use zbus::{interface, Connection};
use zvariant::{Signature, Type};

use crate::dnd;
use crate::hints::Hints;
use crate::icon;
use crate::sender::BarUnit;
use crate::server::{NOTIFICATIONS_PATH, PRIVATE_PATH};
use crate::store::{self, Content, Reason, Store, Urgency, Visual};
use crate::text;
use crate::wire::WireNotification;

pub const CAPABILITIES: [&str; 4] = ["actions", "body", "icon-static", "persistence"];
pub const MAX_ACTIONS: usize = 8;
pub const ACTION_KEY_BYTES: usize = 64;
pub const TOKEN_CHARS: usize = 256;

pub struct State {
    pub store: Store,
    started: Instant,
    state_dir: PathBuf,
    /// The unique name of the last caller `List` admitted (BR1): the bar's own connection,
    /// which the private interface's signals are unicast to. `None` — no bar has listed yet —
    /// means they go nowhere: this content is never for a wider audience than that.
    bar_destination: Option<OwnedUniqueName>,
}

impl State {
    pub fn new(store: Store, state_dir: PathBuf) -> State {
        State {
            store,
            started: Instant::now(),
            state_dir,
            bar_destination: None,
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

pub type Shared = Arc<Mutex<State>>;

/// A poisoned lock means a panic, and panic = "abort" means there is none: take the guard.
fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The arguments of `Notify`, made safe (BR4). The picture is, in the specification's order:
/// image-data, image-path, app_icon.
#[must_use]
pub fn content(
    app_name: &str,
    app_icon: &str,
    summary: &str,
    body: &str,
    actions: &[&str],
    hints: Hints,
    expire_timeout: i32,
) -> Content {
    let urgency = Urgency::from_hint(hints.urgency);
    let visual = match (
        hints.image,
        hints.image_path.or_else(|| icon::parse(app_icon)),
    ) {
        (Some(image), _) => Visual::Pixels(image),
        (None, Some(named)) => Visual::Icon(named),
        (None, None) => Visual::None,
    };
    Content {
        app_name: text::line(app_name, text::NAME_CHARS),
        summary: text::line(summary, text::SUMMARY_CHARS),
        body: text::lines(body, text::BODY_CHARS),
        actions: actions
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|pair| is_action_key(pair[0]))
            .take(MAX_ACTIONS)
            .map(|pair| (pair[0].to_owned(), text::line(pair[1], text::NAME_CHARS)))
            .collect(),
        urgency,
        transient: hints.transient,
        resident: hints.resident,
        desktop_entry: hints.desktop_entry,
        visual,
        timeout_ms: store::timeout_ms(expire_timeout, urgency),
    }
}

/// A key returns to the application unchanged, so a bad one is refused, not cleaned.
fn is_action_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= ACTION_KEY_BYTES && !key.chars().any(text::is_hidden)
}

/// `Notify`'s `actions`, bounded while decoding: a plain `Vec<&str>` would grow to hold every
/// element of whatever the caller sent before `content` ever gets to trim it to `MAX_ACTIONS`
/// pairs. At most `2 * MAX_ACTIONS` strings — a whole pair per key and value — are kept; the
/// rest are walked past, not stored.
struct Actions<'a>(Vec<&'a str>);

impl Type for Actions<'_> {
    const SIGNATURE: &'static Signature = <Vec<&str> as Type>::SIGNATURE;
}

impl<'de> Deserialize<'de> for Actions<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Actions<'de>, D::Error> {
        deserializer.deserialize_seq(ActionsVisitor)
    }
}

struct ActionsVisitor;

impl<'de> Visitor<'de> for ActionsVisitor {
    type Value = Actions<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of strings")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Actions<'de>, A::Error> {
        let cap = 2 * MAX_ACTIONS;
        let mut kept = Vec::new();
        while kept.len() < cap {
            match seq.next_element::<&str>()? {
                Some(item) => kept.push(item),
                None => return Ok(Actions(kept)),
            }
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Actions(kept))
    }
}

/// The bar's destination for the private interface's signals: the unique name of the last
/// caller `List` admitted, unicast so no other client sharing the bus ever sees this content
/// (`os.athanor.Notifications1`'s security boundary). `None` while no bar has listed yet —
/// nothing is sent, rather than broadcasting it.
async fn bar_emitter(
    state: &Shared,
    conn: &Connection,
) -> zbus::Result<Option<SignalEmitter<'static>>> {
    let Some(destination) = lock(state).bar_destination.clone() else {
        return Ok(None);
    };
    Ok(Some(
        SignalEmitter::new(conn, PRIVATE_PATH)?.set_destination(destination.into()),
    ))
}

/// Tells both sides a notification closed: applications listen on the specification's
/// object, the bar on its own, unicast. The two signals are sent independently — a failure
/// sending one must never skip the other — and each failure is logged on its own; the store
/// has already changed regardless.
async fn emit_closed(conn: &Connection, state: &Shared, id: u32, reason: Reason) {
    let public = async {
        Notifications::notification_closed(
            &SignalEmitter::new(conn, NOTIFICATIONS_PATH)?,
            id,
            reason as u32,
        )
        .await
    }
    .await;
    if let Err(err) = public {
        tracing::warn!(id, error = %err, "cannot tell applications a notification closed");
    }
    let private = async {
        match bar_emitter(state, conn).await? {
            Some(emitter) => Private::closed(&emitter, id, reason as u32).await,
            None => Ok(()),
        }
    }
    .await;
    if let Err(err) = private {
        tracing::warn!(id, error = %err, "cannot tell the bar a notification closed");
    }
}

pub struct Notifications {
    pub state: Shared,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<&'static str> {
        CAPABILITIES.to_vec()
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        (
            "athanor-shelld",
            "Athanor",
            env!("CARGO_PKG_VERSION"),
            "1.2",
        )
    }

    #[allow(clippy::too_many_arguments)] // the specification's signature
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Actions<'_>,
        hints: Hints,
        expire_timeout: i32,
        #[zbus(connection)] conn: &Connection,
    ) -> u32 {
        let content = content(
            app_name,
            app_icon,
            summary,
            body,
            &actions.0,
            hints,
            expire_timeout,
        );
        let (outcome, wire) = {
            let mut state = lock(&self.state);
            let now = state.now_ms();
            let outcome = state.store.notify(content, replaces_id, now);
            let wire = WireNotification::new(&outcome.notification, now, state.store.dnd());
            (outcome, wire)
        };
        for id in &outcome.evicted {
            emit_closed(conn, &self.state, *id, Reason::Expired).await;
        }
        let result = async {
            match bar_emitter(&self.state, conn).await? {
                Some(emitter) if outcome.replaced => Private::replaced(&emitter, &wire).await,
                Some(emitter) => Private::added(&emitter, &wire).await,
                None => Ok(()),
            }
        }
        .await;
        if let Err(err) = result {
            tracing::warn!(id = wire.id, error = %err, "cannot tell the bar about a notification");
        }
        wire.id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        let closed = lock(&self.state).store.close(id);
        if closed.is_none() {
            return Err(fdo::Error::InvalidArgs(format!("no notification {id}")));
        }
        emit_closed(conn, &self.state, id, Reason::Closed).await;
        Ok(())
    }

    #[zbus(signal)]
    pub async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn activation_token(
        emitter: &SignalEmitter<'_>,
        id: u32,
        activation_token: &str,
    ) -> zbus::Result<()>;
}

pub struct Private {
    pub state: Shared,
    pub bar: BarUnit,
}

impl Private {
    /// The caller's unique name, once admitted: `list` records it as the private signals'
    /// destination.
    async fn admit(&self, header: &Header<'_>, conn: &Connection) -> fdo::Result<OwnedUniqueName> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::AccessDenied("a call with no sender".into()))?;
        let credentials = DBusProxy::new(conn)
            .await?
            .get_connection_credentials(sender.clone().into())
            .await?;
        let pid = credentials.process_id().ok_or_else(|| {
            fdo::Error::AccessDenied("the bus gave no process id for the caller".into())
        })?;
        if self.bar.admits(pid) {
            Ok(sender.to_owned().into())
        } else {
            Err(fdo::Error::AccessDenied(format!(
                "only {} may call this interface",
                self.bar.unit()
            )))
        }
    }
}

#[interface(name = "os.athanor.Notifications1")]
impl Private {
    /// The do-not-disturb switch, and every notification held, oldest first. Admitting this
    /// call is also what makes the caller the private signals' destination (BR1): the bar
    /// lists on start and on restart, so this is where its unique name is (re)recorded.
    async fn list(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<(bool, Vec<WireNotification>)> {
        let sender = self.admit(&header, conn).await?;
        let mut state = lock(&self.state);
        state.bar_destination = Some(sender);
        let (now, dnd) = (state.now_ms(), state.store.dnd());
        Ok((
            dnd,
            state
                .store
                .iter()
                .map(|n| WireNotification::new(n, now, dnd))
                .collect(),
        ))
    }

    /// `reason` is 1 (the popup of a transient notification ended) or 2 (the user closed it).
    async fn close(
        &self,
        id: u32,
        reason: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn).await?;
        let reason = match reason {
            1 => Reason::Expired,
            2 => Reason::Dismissed,
            other => {
                return Err(fdo::Error::InvalidArgs(format!(
                    "reason {other} is neither 1 nor 2"
                )))
            }
        };
        if lock(&self.state).store.close(id).is_none() {
            return Err(fdo::Error::InvalidArgs(format!("no notification {id}")));
        }
        emit_closed(conn, &self.state, id, reason).await;
        Ok(())
    }

    /// The token comes from the bar's surface and the click's serial (BR4). It is sent before
    /// the action, so the application can raise its window with it.
    async fn invoke_action(
        &self,
        id: u32,
        action_key: &str,
        activation_token: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn).await?;
        let resident = {
            let state = lock(&self.state);
            let held = state
                .store
                .get(id)
                .ok_or_else(|| fdo::Error::InvalidArgs(format!("no notification {id}")))?;
            if !held
                .content
                .actions
                .iter()
                .any(|(key, _)| key == action_key)
            {
                return Err(fdo::Error::InvalidArgs(format!(
                    "notification {id} has no action {action_key:?}"
                )));
            }
            held.content.resident
        };
        let public = SignalEmitter::new(conn, NOTIFICATIONS_PATH)?;
        let token = text::line(activation_token, TOKEN_CHARS);
        if !token.is_empty() {
            Notifications::activation_token(&public, id, &token).await?;
        }
        Notifications::action_invoked(&public, id, action_key).await?;
        if !resident && lock(&self.state).store.close(id).is_some() {
            emit_closed(conn, &self.state, id, Reason::Dismissed).await;
        }
        Ok(())
    }

    async fn set_do_not_disturb(
        &self,
        on: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.admit(&header, conn).await?;
        let mut state = lock(&self.state);
        dnd::save(&state.state_dir, on)
            .map_err(|err| fdo::Error::IOError(format!("cannot keep the switch: {err}")))?;
        state.store.set_dnd(on);
        Ok(())
    }

    #[zbus(signal)]
    pub async fn added(
        emitter: &SignalEmitter<'_>,
        notification: &WireNotification,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn replaced(
        emitter: &SignalEmitter<'_>,
        notification: &WireNotification,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::Icon;

    #[test]
    fn bad_action_keys_are_dropped_and_labels_cleaned() {
        let long = "k".repeat(ACTION_KEY_BYTES + 1);
        let actions = [
            "default",
            "Open",
            "bad\u{202E}",
            "x",
            long.as_str(),
            "y",
            "ok",
            "La\u{0007}bel",
            "odd",
        ];
        let made = content("app", "", "s", "b", &actions, Hints::default(), -1);
        assert_eq!(
            made.actions,
            [
                ("default".into(), "Open".into()),
                ("ok".into(), "Label".into())
            ]
        );
    }

    #[test]
    fn the_picture_follows_the_specification_order() {
        let hints = Hints {
            image_path: Some(Icon::Name("from-hint".into())),
            ..Hints::default()
        };
        assert_eq!(
            content("a", "app-icon", "s", "b", &[], hints, -1).visual,
            Visual::Icon(Icon::Name("from-hint".into()))
        );
        assert_eq!(
            content("a", "app-icon", "s", "b", &[], Hints::default(), -1).visual,
            Visual::Icon(Icon::Name("app-icon".into()))
        );
        assert_eq!(
            content("a", "https://x/y.png", "s", "b", &[], Hints::default(), -1).visual,
            Visual::None
        );
    }

    #[test]
    fn a_hundred_thousand_actions_yield_at_most_max_actions_pairs() {
        use zvariant::serialized::Context;
        use zvariant::{to_bytes, LE};

        let many: Vec<String> = (0..100_000usize)
            .map(|n| {
                if n % 2 == 0 {
                    format!("k{n}")
                } else {
                    "v".to_owned()
                }
            })
            .collect();
        let encoded = to_bytes(Context::new_dbus(LE, 0), &many).expect("encode");
        let actions: Actions<'_> = encoded.deserialize().expect("decode").0;
        assert_eq!(actions.0.len(), 2 * MAX_ACTIONS);
        assert_eq!(&actions.0[..2], ["k0", "v"]);
        let made = content("app", "", "s", "b", &actions.0, Hints::default(), -1);
        assert_eq!(made.actions.len(), MAX_ACTIONS);
    }
}
