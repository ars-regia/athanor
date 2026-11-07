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
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::battery::{self, Level, Notice, Notices, SETTINGS_ACTION, SETTINGS_PAGE};

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
        .interface(SERVER)?
        .path(SERVER_PATH)?
        .build();
    let mut server = MessageStream::for_match_rule(rule, signals, None).await?;
    let mut upower = mirror::spawn(
        handle,
        system,
        UPOWER,
        Source::Fixed(vec![(DISPLAY_DEVICE, DEVICE_IFACE)]),
    );
    let mut notices = Notices::default();
    loop {
        tokio::select! {
            changed = upower.changed() => {
                changed.map_err(|_| "the mirror of UPower ended")?;
                let (level, device) = read(&upower);
                if let Some(step) = notices.update(level) {
                    if let Some(shown) = step.close {
                        // Asked of the server that raised it: the id is that server's.
                        let closed = call(query, &shown.server, SERVER_PATH, SERVER, "CloseNotification", &(shown.id,)).await;
                        if let Err(err) = closed {
                            tracing::warn!(%err, id = shown.id, "the battery notice was not closed");
                        }
                    }
                    if let Some(level) = step.raise {
                        let notice = battery::notice(level, device.as_ref(), catalog);
                        match raise(query, &notice).await {
                            Ok((server, id)) => notices.raised(server, id),
                            Err(err) => {
                                tracing::warn!(%err, ?level, "the battery notice was not shown; it is tried again at the next change");
                                notices.failed();
                            }
                        }
                    }
                }
            }
            message = server.next() => {
                let message = message.ok_or("the notification signals ended")??;
                signal(&message, &mut notices, query).await;
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
    let mut hints: HashMap<&str, Value<'_>> =
        HashMap::from([("urgency", Value::U8(notice.urgency))]);
    if notice.resident {
        hints.insert("resident", Value::Bool(true));
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
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let reply = timeout(
        CALL_TIMEOUT,
        query.call_method(Some(destination), path, Some(interface), method, body),
    )
    .await
    .map_err(|_| format!("{destination} did not answer {method} within 2 s"))??;
    Ok(reply)
}
