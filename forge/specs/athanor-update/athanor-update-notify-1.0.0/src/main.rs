//! `athanor-update-notify`: the user side of updates until the shield exists
//! (docs/architecture/doc_update_trust.md, UT11). It reads the state file, sends
//! notifications, and calls `Apply()` and `GoBack()`. Nothing else.
mod notices;
mod sandbox;
mod text;

use futures_util::StreamExt as _;
use notices::{Notice, Notices, Request, Sent};
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, MessageStream};

const SERVER: &str = "org.freedesktop.Notifications";
const SERVER_PATH: &str = "/org/freedesktop/Notifications";
const UPDATE: &str = "os.athanor.Update1";
const UPDATE_PATH: &str = "/os/athanor/Update1";
// ponytail: the file changes a few times a day and is 1 kB; a poll is enough. Watch the
// directory with inotify if a surface ever needs the change within a second.
const POLL: Duration = Duration::from_secs(30);

async fn send(session: &Connection, notice: &Notice) -> zbus::Result<(u32, String)> {
    let t = text::texts(text::session_locale().as_deref());
    let (summary, body, actions) = match notice {
        Notice::Ready { .. } => (t.ready_summary, t.ready_body.to_owned(), vec![notices::ACTION_APPLY, t.restart, notices::ACTION_LATER, t.later]),
        Notice::Running { version, build_time, .. } => {
            let date = chrono::DateTime::from_timestamp(*build_time, 0).map(|time| time.format("%Y-%m-%d").to_string()).unwrap_or_default();
            // The version comes from a world-readable file: plain text, stripped, truncated.
            let body = t.running_body.replace("{version}", &athanor_trust_state::display(version)).replace("{date}", &date);
            (t.running_summary, body, vec![notices::ACTION_GO_BACK, t.go_back])
        }
    };
    // The owner is read before the call, so the id returned is tied to the server asked.
    let owner = zbus::fdo::DBusProxy::new(session).await?.get_name_owner(zbus::names::BusName::try_from(SERVER)?).await?.to_string();
    let hints: HashMap<&str, Value<'_>> = HashMap::from([("urgency", Value::U8(1)), ("resident", Value::Bool(true))]);
    let reply = session
        .call_method(Some(SERVER), SERVER_PATH, Some(SERVER), "Notify", &("Athanor", 0u32, "software-update-available-symbolic", summary, body.as_str(), actions, hints, 0i32))
        .await?;
    Ok((reply.body().deserialize::<u32>()?, owner))
}

async fn request(system: &Connection, request: Request) {
    let method = match request {
        Request::Apply => "Apply",
        Request::GoBack => "GoBack",
    };
    if let Err(err) = system.call_method(Some(UPDATE), UPDATE_PATH, Some(UPDATE), method, &()).await {
        tracing::warn!(%err, method, "the request was refused");
    }
}

async fn run(state_dir: &Path) -> zbus::Result<()> {
    let (session, system) = (Connection::session().await?, Connection::system().await?);
    let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(SERVER)?.member("ActionInvoked")?.path(SERVER_PATH)?.build();
    let mut invoked = MessageStream::for_match_rule(rule, &session, None).await?;
    let mut notices = Notices::default();
    let mut poll = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            _ = poll.tick() => {
                let Ok(state) = athanor_trust_state::read() else { continue };
                let seen = notices::seen_booted(state_dir);
                if let Some(notice) = notices.due(&state, seen.as_deref()) {
                    match send(&session, &notice).await {
                        Ok((id, server)) => { notices.sent.insert(id, Sent { server, notice }); }
                        Err(err) => tracing::warn!(%err, "the notification was not sent"),
                    }
                }
                if seen.as_deref() != Some(state.booted.digest.as_str()) {
                    if let Err(err) = notices::record_booted(state_dir, &state.booted.digest) {
                        tracing::warn!(%err, "the booted digest was not recorded");
                    }
                }
            }
            Some(Ok(message)) = invoked.next() => {
                let header = message.header();
                let (Some(sender), Ok((id, action))) = (header.sender(), message.body().deserialize::<(u32, String)>()) else { continue };
                if let Some(wanted) = notices.invoked(sender.as_str(), id, &action) {
                    request(&system, wanted).await;
                }
            }
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt().with_writer(std::io::stderr).without_time().init();
    // The sandbox comes first, while the process has one thread; the runtime is single-threaded too.
    let confined = sandbox::state_dir().and_then(|dir| {
        sandbox::ensure_single_threaded()?;
        sandbox::restrict(&[Path::new("/usr"), Path::new("/run/athanor-update")], &dir)?;
        Ok(dir)
    });
    let state_dir = match confined {
        Ok(dir) => dir,
        Err(err) => {
            tracing::error!(%err, "refusing to run unconfined");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(%err, "cannot start the runtime");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(&state_dir)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "the session or system bus is not reachable");
            ExitCode::FAILURE
        }
    }
}
