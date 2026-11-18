//! The bus side of the battery notices (`doc_session_daemons.md` SD14): UPower's display
//! device through the battery model's mirror, the notification server and the control
//! center. Signals are read on one session connection and every call this loop awaits goes
//! out on another that subscribes to nothing, so a full signal queue never holds a reply
//! (#1035).

use std::collections::HashMap;
use std::error::Error;
use std::time::Duration;

use athanor_i18n::Catalog;
use athanor_services::battery::{self as model, DEVICE_IFACE, DISPLAY_DEVICE, UPOWER};
use athanor_services::mirror::{self, Snapshot, Source};
use athanor_services::props;
use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::time::timeout;
use zbus::message::Type;
use zbus::names::BusName;
use zbus::Variant;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::battery::{self, Level, Notice, Notices, Step, SETTINGS_ACTION, SETTINGS_PAGE};

const SERVER: &str = "org.freedesktop.Notifications";
const SERVER_PATH: &str = "/org/freedesktop/Notifications";
const CONTROL_CENTER: &str = "os.athanor.ControlCenter1";
const CONTROL_CENTER_PATH: &str = "/os/athanor/ControlCenter1";
/// How long a call waits for its answer, as the query connections of #1043.
const CALL_TIMEOUT: Duration = Duration::from_secs(2);

type Failure = Box<dyn Error>;

/// Follows the display device until its mirror or the signal stream ends, which is an
/// error: the unit restarts the daemon, and the restart reads the battery state again (SD18).
pub async fn run(
    handle: &Handle,
    system: Connection,
    signals: &Connection,
    query: &Connection,
    catalog: &Catalog,
) -> Result<(), Failure> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(SERVER)
        .path(SERVER_PATH)
        .build()?;
    let mut server = MessageStream::for_match_rule(rule, signals, None).await?;
    // A failed raise stays owed until the level changes or a server appears, and a server that
    // appears gets the current level's notice again (SD14, maintainer decision, 2026-11-07).
    let owner = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .member("NameOwnerChanged")
        .arg(0, SERVER)
        .build()?;
    let mut owners = MessageStream::for_match_rule(owner, signals, None).await?;
    let mut upower = mirror::spawn(
        handle,
        system,
        UPOWER,
        Source::Fixed(vec![(DISPLAY_DEVICE, DEVICE_IFACE)]),
    );
    let mut notices = Notices::default();
    loop {
        let step = tokio::select! {
            changed = upower.changed() => {
                changed.map_err(|_| "the mirror of UPower ended")?;
                notices.update(read(&upower).0)
            }
            message = owners.next() => {
                let message = message.ok_or("the name owner signals ended")??;
                owner_step(&message, &mut notices)
            }
            message = server.next() => {
                let message = message.ok_or("the notification signals ended")??;
                signal(&message, &mut notices, query).await;
                None
            }
        };
        if let Some(step) = step {
            apply(step, &mut notices, &upower, query, catalog).await;
        }
    }
}

/// Closes the notice shown and raises the new one, with the device's charge for its text.
async fn apply(
    step: Step,
    notices: &mut Notices,
    upower: &watch::Receiver<Snapshot>,
    query: &Connection,
    catalog: &Catalog,
) {
    if let Some(shown) = step.close {
        // Asked of the server that raised it: the id is that server's.
        let closed = call(
            query,
            &shown.server,
            SERVER_PATH,
            SERVER,
            "CloseNotification",
            &(shown.id,),
        )
        .await;
        if let Err(err) = closed {
            tracing::warn!(%err, id = shown.id, "the battery notice was not closed");
        }
    }
    if let Some(level) = step.raise {
        let device = read(upower).1;
        let notice = battery::notice(level, device.as_ref(), catalog);
        match raise(query, &notice).await {
            Ok((server, id)) => notices.raised(server, id),
            Err(err) => {
                tracing::warn!(%err, ?level, "the battery notice was not shown; it is raised again at the next change or when a server appears");
                notices.failed();
            }
        }
    }
}

/// The level the display device asks for, and its charge for the text.
fn read(upower: &watch::Receiver<Snapshot>) -> (Option<Level>, Option<model::Battery>) {
    let snapshot = upower.borrow();
    match props::lookup(&snapshot.objects, DISPLAY_DEVICE, DEVICE_IFACE) {
        Some(device) => (battery::wanted(device), model::battery(device)),
        None => (None, None),
    }
}

/// The notification server's new owner, `Some(None)` when it left, from a NameOwnerChanged
/// signal; `None` unless the bus itself sent it about the server. zbus applies a match rule's
/// sender on the client side and lets any peer through for a well-known name, so a peer of the
/// same user could otherwise forge a unicast signal and spoof a server change.
fn owner_change(message: &Message) -> Option<Option<String>> {
    let header = message.header();
    if header.sender().map(|sender| sender.as_str()) != Some("org.freedesktop.DBus") {
        return None;
    }
    let (name, _, new) = message
        .body()
        .deserialize::<(String, String, String)>()
        .ok()?;
    (name == SERVER).then(|| Some(new).filter(|new| !new.is_empty()))
}

/// The step a NameOwnerChanged signal asks for; a forged one is ignored with a warning.
fn owner_step(message: &Message, notices: &mut Notices) -> Option<Step> {
    match owner_change(message) {
        Some(new) => notices.server(new.as_deref()),
        None => {
            tracing::warn!(
                sender = ?message.header().sender(),
                "a NameOwnerChanged signal not sent by the bus about the server is ignored"
            );
            None
        }
    }
}

async fn signal(message: &Message, notices: &mut Notices, query: &Connection) {
    let header = message.header();
    let (Some(sender), Some(member)) = (header.sender(), header.member()) else {
        return;
    };
    match member.as_str() {
        "ActionInvoked" => {
            let Ok((id, action)) = message.body().deserialize::<(u32, String)>() else {
                return;
            };
            if action == SETTINGS_ACTION && notices.is_shown(sender.as_str(), id) {
                let shown = call(
                    query,
                    CONTROL_CENTER,
                    CONTROL_CENTER_PATH,
                    CONTROL_CENTER,
                    "Show",
                    &(SETTINGS_PAGE,),
                )
                .await;
                if let Err(err) = shown {
                    tracing::warn!(%err, "the control center's battery page did not open");
                }
            }
        }
        "NotificationClosed" => {
            if let Ok((id, _reason)) = message.body().deserialize::<(u32, u32)>() {
                notices.closed(sender.as_str(), id);
            }
        }
        _ => {}
    }
}

/// Raises `notice`; returns the server's unique name, read before the call so the id is tied
/// to the server asked, and the id.
async fn raise(query: &Connection, notice: &Notice) -> Result<(String, u32), Failure> {
    let dbus = zbus::fdo::DBusProxy::new(query).await?;
    let owner = timeout(
        CALL_TIMEOUT,
        dbus.get_name_owner(BusName::try_from(SERVER)?),
    )
    .await
    .map_err(|_| "the bus did not answer within 2 s")??
    .to_string();
    let actions: Vec<&str> = notice
        .actions
        .iter()
        .flat_map(|(key, label)| [*key, label.as_str()])
        .collect();
    let mut hints: HashMap<&str, Variant<'_>> =
        HashMap::from([("urgency", Variant::U8(notice.urgency))]);
    if notice.resident {
        hints.insert("resident", Variant::Bool(true));
    }
    let body = (
        "Athanor",
        0u32,
        notice.icon,
        notice.summary.as_str(),
        notice.body.as_str(),
        actions,
        hints,
        -1i32,
    );
    // Sent to the owner just read, so the id belongs to it.
    let reply = call(query, owner.as_str(), SERVER_PATH, SERVER, "Notify", &body).await?;
    Ok((owner, reply.body().deserialize::<u32>()?))
}

async fn call<B>(
    query: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> Result<Message, Failure>
where
    B: serde::Serialize + zbus::DynamicType,
{
    let reply = timeout(
        CALL_TIMEOUT,
        query.call_method(Some(destination), path, Some(interface), method, body),
    )
    .await
    .map_err(|_| format!("{destination} did not answer {method} within 2 s"))??;
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_changed(sender: &str, name: &str, new: &str) -> Message {
        Message::signal(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
        )
        .sender(sender)
        .build(&(name, "", new))
        .expect("message")
    }

    #[test]
    fn only_the_bus_speaks_for_the_servers_owner() {
        let bus = "org.freedesktop.DBus";
        assert_eq!(
            owner_change(&owner_changed(bus, SERVER, ":1.7")),
            Some(Some(":1.7".into()))
        );
        assert_eq!(owner_change(&owner_changed(bus, SERVER, "")), Some(None));
        assert_eq!(
            owner_change(&owner_changed(bus, "org.example.Other", ":1.7")),
            None
        );
        // A peer's unicast NameOwnerChanged, forged, raises nothing and keeps the notice.
        let mut notices = Notices::default();
        notices.update(Some(Level::Critical));
        notices.raised(":1.7".into(), 1);
        for forged in [":1.42", ":1.7"] {
            let forged = owner_changed(forged, SERVER, ":1.43");
            assert_eq!(owner_step(&forged, &mut notices), None);
        }
        assert!(notices.is_shown(":1.7", 1), "the shown notice is kept");
        // The bus's own signal moves it.
        let moved = owner_step(&owner_changed(bus, SERVER, ":1.43"), &mut notices);
        assert_eq!(moved.and_then(|step| step.raise), Some(Level::Critical));
    }
}
