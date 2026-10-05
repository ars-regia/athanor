//! What the shell's programs share about notifications: the wire type of
//! `os.athanor.Notifications1`, which athanor-shelld sends and the bar and the control center
//! read (doc_notification_center.md, NC8).

pub mod wire;

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep_until, timeout, Instant};
use zbus::fdo::{self, DBusProxy};
use zbus::message::Type;
use zbus::names::BusName;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::mirror::TIMEOUT;
use crate::runtime::{backoff, Bus, Buses};
use wire::WireNotification;

/// The name the notification daemon owns; its owner is the one the model follows.
pub const NAME: &str = "org.freedesktop.Notifications";
/// Where the daemon serves its private interface.
pub const PATH: &str = "/os/athanor/Notifications1";
pub const INTERFACE: &str = "os.athanor.Notifications1";
/// The daemon holds at most this many; a reader holds no more however many it is told of.
const MAX_ENTRIES: usize = 500;
/// Commands waiting for the one in flight: one more is refused rather than queued.
const MAX_WAITING: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dnd {
    pub on: bool,
    /// `""` when off.
    pub reason: String,
    /// Unix seconds; 0 for no end.
    pub until: i64,
    /// The triggers the session cannot observe.
    pub unavailable: Vec<String>,
}

#[derive(Clone, Default, PartialEq)]
pub struct NotificationsState {
    /// The daemon owns its name and its history has been read.
    pub available: bool,
    /// Newest last.
    pub entries: Vec<WireNotification>,
    pub dnd: Dnd,
    /// Counts the owner's changes.
    pub generation: u64,
    /// The first answer arrived, or the daemon is known to be absent.
    pub settled: bool,
    /// Counts the commands the daemon refused or did not answer, so that a reader that sees
    /// it change knows its last action did not complete.
    pub refused: u32,
}

/// The notifications' text is the person's: it stays out of the logs.
impl fmt::Debug for NotificationsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotificationsState")
            .field("available", &self.available)
            .field("entries", &self.entries.len())
            .field("dnd", &self.dnd)
            .field("generation", &self.generation)
            .field("settled", &self.settled)
            .field("refused", &self.refused)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum NotificationsCommand {
    /// The person closed the notification.
    Close(u32),
    Invoke {
        id: u32,
        key: String,
        token: String,
    },
    Reply {
        id: u32,
        text: String,
    },
    MarkRead(Vec<u32>),
    ClearAll,
    /// By application id; `""` for Other.
    ClearGroup(String),
    SetDnd {
        on: bool,
        until: Option<i64>,
    },
    /// No notification of this application at all.
    Mute(String),
    /// Notifications of this application go to the list, with no popup.
    ListOnly(String),
}

/// A reply's text and an activation token are the person's and a credential: neither is logged.
impl fmt::Debug for NotificationsCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Close(id) => write!(f, "Close({id})"),
            Self::Invoke { id, .. } => write!(f, "Invoke({id})"),
            Self::Reply { id, .. } => write!(f, "Reply({id})"),
            Self::MarkRead(ids) => write!(f, "MarkRead({ids:?})"),
            Self::ClearAll => write!(f, "ClearAll"),
            Self::ClearGroup(_) => write!(f, "ClearGroup"),
            Self::SetDnd { on, until } => write!(f, "SetDnd({on}, {until:?})"),
            Self::Mute(_) => write!(f, "Mute"),
            Self::ListOnly(_) => write!(f, "ListOnly"),
        }
    }
}

/// A signal of the daemon's private interface.
enum Signal {
    Added(WireNotification),
    Replaced(WireNotification),
    Closed(u32),
    Read(Vec<u32>),
    Dnd {
        on: bool,
        reason: String,
        until: i64,
    },
}

impl Signal {
    fn parse(message: &Message) -> Option<Signal> {
        let header = message.header();
        let body = message.body();
        let signal = match header.member()?.as_str() {
            "Added" => Signal::Added(body.deserialize().ok()?),
            "Replaced" => Signal::Replaced(body.deserialize().ok()?),
            "Closed" => Signal::Closed(body.deserialize::<(u32, u32)>().ok()?.0),
            "Read" => Signal::Read(body.deserialize().ok()?),
            "DoNotDisturbChanged" => {
                let (on, reason, until) = body.deserialize().ok()?;
                Signal::Dnd { on, reason, until }
            }
            _ => return None,
        };
        Some(signal)
    }

    /// Every signal sets or removes by id, and one that the history already holds lands where
    /// the daemon put it, so a replay in order gives the daemon's order.
    fn apply(self, state: &mut NotificationsState) {
        match self {
            // The daemon moves a replaced notification to the end, as the newest.
            Signal::Added(note) | Signal::Replaced(note) => {
                state.entries.retain(|held| held.id != note.id);
                state.entries.push(note);
                let extra = state.entries.len().saturating_sub(MAX_ENTRIES);
                state.entries.drain(..extra);
            }
            Signal::Closed(id) => state.entries.retain(|held| held.id != id),
            Signal::Read(ids) => state
                .entries
                .iter_mut()
                .filter(|held| ids.contains(&held.id))
                .for_each(|held| held.read = true),
            Signal::Dnd { on, reason, until } => {
                state.dnd.on = on;
                state.dnd.reason = reason;
                state.dnd.until = until;
            }
        }
    }
}

/// What a call in flight comes back with.
enum Done {
    Load(zbus::Result<(Vec<WireNotification>, Dnd)>),
    Command(zbus::Result<()>),
}

type Running = Pin<Box<dyn Future<Output = Done> + Send>>;

/// Starts the model on `handle`'s runtime, on the session bus. Commands wait in an unbounded
/// queue and run one at a time; a send fails only when the model has ended. Without the
/// session bus the state is settled and unavailable, and the model ends.
pub fn spawn(
    handle: &Handle,
    buses: Buses,
) -> (
    watch::Receiver<NotificationsState>,
    mpsc::UnboundedSender<NotificationsCommand>,
) {
    let (state, rx) = watch::channel(NotificationsState::default());
    let (commands, command_rx) = mpsc::unbounded_channel();
    handle.spawn(async move {
        let state = ExitGuard(state);
        let connection = match buses.connection(Bus::Session).await {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!(error = %err, "no session bus; the notification center is hidden");
                return;
            }
        };
        if let Err(err) = run(connection, &state.0, command_rx).await {
            tracing::error!(error = %err, "the notifications model stopped; it is shown as unavailable");
        }
    });
    (rx, commands)
}

/// However the model ends, its readers are left with a settled, empty, unavailable state
/// rather than a history nobody keeps current.
struct ExitGuard(watch::Sender<NotificationsState>);

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.0.send_modify(|state| {
            *state = NotificationsState {
                generation: state.generation.wrapping_add(1),
                settled: true,
                refused: state.refused,
                ..NotificationsState::default()
            };
        });
    }
}

async fn run(
    connection: Connection,
    published: &watch::Sender<NotificationsState>,
    mut commands: mpsc::UnboundedReceiver<NotificationsCommand>,
) -> zbus::Result<()> {
    // Subscribe before asking for the owner, so no change falls between the two. The bus
    // delivers the owner's signals unicast once it has been called, or broadcast to a match;
    // the sender is checked against the owner's unique name below.
    let dbus = DBusProxy::new(&connection).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(INTERFACE)?
        .path(PATH)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &connection, None).await?;
    let mut view = NotificationsState::default();
    let mut owner: Option<String> = None;
    // The owner's signals received while its load is in flight, applied after it in order:
    // the load may or may not hold them, and each applies the same either way.
    let mut buffered: Option<Vec<Signal>> = None;
    let mut waiting: VecDeque<NotificationsCommand> = VecDeque::new();
    let mut running: Option<Running> = None;
    // The load is tried again while the owner stays, after `backoff(attempt)`.
    let mut attempt = 0u32;
    let mut retry_at: Option<Instant> = None;
    match dbus.get_name_owner(BusName::try_from(NAME)?).await {
        Ok(name) => owner = Some(name.to_string()),
        Err(fdo::Error::NameHasNoOwner(_)) => view.settled = true,
        Err(err) => return Err(err.into()),
    }
    if let Some(name) = &owner {
        buffered = Some(Vec::new());
        running = Some(load(&connection, name));
    }
    published.send_replace(view.clone());
    loop {
        tokio::select! {
            change = owners.next() => {
                let Some(change) = change else { return Ok(()) };
                let args = match change.args() {
                    Ok(args) => args,
                    Err(err) => {
                        tracing::warn!(error = %err, "NameOwnerChanged had an unexpected type");
                        continue;
                    }
                };
                let new = args.new_owner().as_ref().map(ToString::to_string);
                if new != owner {
                    // The old instance's entries and calls go at once.
                    view = NotificationsState {
                        generation: view.generation.wrapping_add(1),
                        settled: new.is_none(),
                        refused: view.refused,
                        ..NotificationsState::default()
                    };
                    waiting.clear();
                    attempt = 0;
                    retry_at = None;
                    buffered = new.as_ref().map(|_| Vec::new());
                    running = new.as_ref().map(|name| load(&connection, name));
                    owner = new;
                }
            }
            message = signals.next() => {
                let message = match message {
                    Some(Ok(message)) => message,
                    Some(Err(err)) => {
                        tracing::warn!(error = %err, "a signal could not be read");
                        continue;
                    }
                    None => return Ok(()),
                };
                let header = message.header();
                let from = header.sender().map(|sender| sender.as_str());
                if from.is_none() || from != owner.as_deref() {
                    continue;
                }
                let Some(signal) = Signal::parse(&message) else { continue };
                match &mut buffered {
                    Some(held) => held.push(signal),
                    None if view.available => signal.apply(&mut view),
                    None => {}
                }
            }
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()) };
                if view.available && waiting.len() < MAX_WAITING {
                    waiting.push_back(command);
                } else {
                    tracing::warn!(?command, "the notification daemon is not there to take a command");
                    view.refused = view.refused.wrapping_add(1);
                }
            }
            () = until(retry_at) => {
                retry_at = None;
                if let (Some(name), None) = (&owner, &running) {
                    buffered = Some(Vec::new());
                    running = Some(load(&connection, name));
                }
            }
            done = async {
                match running.as_mut() {
                    Some(call) => call.await,
                    None => std::future::pending().await,
                }
            }, if running.is_some() => {
                running = None;
                match done {
                    Done::Load(Ok((entries, dnd))) => {
                        let extra = entries.len().saturating_sub(MAX_ENTRIES);
                        view.entries = entries.into_iter().skip(extra).collect();
                        view.dnd = dnd;
                        view.available = true;
                        view.settled = true;
                        attempt = 0;
                        for signal in buffered.take().unwrap_or_default() {
                            signal.apply(&mut view);
                        }
                    }
                    Done::Load(Err(err)) => {
                        buffered = None;
                        view.settled = true;
                        if admission_refused(&err) {
                            // The daemon will not admit this process: asking again changes nothing.
                            tracing::warn!(error = %err, "the notification daemon does not admit this process; it is shown as unavailable");
                        } else {
                            tracing::warn!(error = %err, "the notification daemon did not load; trying again");
                            retry_at = Some(Instant::now() + backoff(attempt));
                            attempt = attempt.saturating_add(1);
                        }
                    }
                    Done::Command(Err(err)) => {
                        tracing::warn!(error = %err, "the notification daemon refused or did not answer");
                        view.refused = view.refused.wrapping_add(1);
                    }
                    Done::Command(Ok(())) => {}
                }
            }
        }
        if running.is_none() {
            if let (Some(name), Some(command)) = (&owner, waiting.pop_front()) {
                running = Some(Box::pin(command_call(
                    connection.clone(),
                    name.clone(),
                    command,
                )));
            }
        }
        published.send_if_modified(|state| {
            let changed = *state != view;
            if changed {
                state.clone_from(&view);
            }
            changed
        });
    }
}

/// Whether the daemon refused to admit the caller, as it does a process that is not the bar
/// or the control center.
fn admission_refused(err: &zbus::Error) -> bool {
    match err {
        zbus::Error::MethodError(name, ..) => {
            name.as_str() == "org.freedesktop.DBus.Error.AccessDenied"
        }
        zbus::Error::FDO(err) => matches!(**err, fdo::Error::AccessDenied(_)),
        _ => false,
    }
}

async fn until(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// The history, then do not disturb, from `owner` (the unique name, so every reply comes from
/// the instance this generation belongs to).
fn load(connection: &Connection, owner: &str) -> Running {
    let (connection, owner) = (connection.clone(), owner.to_owned());
    Box::pin(async move {
        let loaded = async {
            let reply = call(&connection, &owner, "History", &()).await?;
            let entries = reply.body().deserialize::<Vec<WireNotification>>()?;
            let reply = call(&connection, &owner, "DoNotDisturb", &()).await?;
            let (on, reason, until, unavailable) = reply
                .body()
                .deserialize::<(bool, String, i64, Vec<String>)>()?;
            let dnd = Dnd {
                on,
                reason,
                until,
                unavailable,
            };
            Ok((entries, dnd))
        };
        Done::Load(loaded.await)
    })
}

async fn call(
    connection: &Connection,
    owner: &str,
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<Message> {
    timeout(
        TIMEOUT,
        connection.call_method(Some(owner), PATH, Some(INTERFACE), method, body),
    )
    .await
    .unwrap_or_else(|_| Err(zbus::Error::Failure("no answer in time".into())))
}

async fn command_call(
    connection: Connection,
    owner: String,
    command: NotificationsCommand,
) -> Done {
    let (c, o) = (&connection, owner.as_str());
    let sent = match command {
        // 2: the user closed it.
        NotificationsCommand::Close(id) => call(c, o, "Close", &(id, 2u32)).await,
        NotificationsCommand::Invoke { id, key, token } => {
            call(c, o, "InvokeAction", &(id, key, token)).await
        }
        NotificationsCommand::Reply { id, text } => call(c, o, "Reply", &(id, text)).await,
        NotificationsCommand::MarkRead(ids) => call(c, o, "MarkRead", &(ids,)).await,
        NotificationsCommand::ClearAll => call(c, o, "ClearAll", &()).await,
        NotificationsCommand::ClearGroup(app) => call(c, o, "ClearGroup", &(app,)).await,
        NotificationsCommand::SetDnd { on, until: None } => {
            call(c, o, "SetDoNotDisturb", &(on,)).await
        }
        NotificationsCommand::SetDnd {
            on,
            until: Some(until),
        } => call(c, o, "SetDoNotDisturbUntil", &(on, until)).await,
        // The control center may set `allowed` and `popups`, and no other rule.
        NotificationsCommand::Mute(app) => call(c, o, "SetRule", &(app, "allowed", "false")).await,
        NotificationsCommand::ListOnly(app) => {
            call(c, o, "SetRule", &(app, "popups", "false")).await
        }
    };
    Done::Command(sent.map(|_| ()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::runtime::backoff;
    use crate::testbus::{self, FakeNotifications, TestBus};

    struct Rig {
        bus: TestBus,
        fake: FakeNotifications,
        states: watch::Receiver<NotificationsState>,
        commands: mpsc::UnboundedSender<NotificationsCommand>,
    }

    async fn rig(history: Vec<WireNotification>) -> Rig {
        let bus = TestBus::start();
        let fake = testbus::serve_notifications(&bus, history).await;
        let client = bus.client().await;
        let (states, commands) = spawn(&Handle::current(), Buses::with(client));
        Rig {
            bus,
            fake,
            states,
            commands,
        }
    }

    async fn wait_for(
        rx: &mut watch::Receiver<NotificationsState>,
        predicate: impl FnMut(&NotificationsState) -> bool,
    ) -> NotificationsState {
        tokio::time::timeout(testbus::WAIT, rx.wait_for(predicate))
            .await
            .expect("the model reached the expected state in time")
            .expect("the model is running")
            .clone()
    }

    fn ids(state: &NotificationsState) -> Vec<u32> {
        state.entries.iter().map(|held| held.id).collect()
    }

    fn two() -> Vec<WireNotification> {
        vec![
            testbus::notification(1, "one"),
            testbus::notification(2, "two"),
        ]
    }

    async fn added(fake: &FakeNotifications, id: u32) {
        testbus::emit_private(fake, "Added", &(testbus::notification(id, "new"),)).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_history_and_do_not_disturb_are_loaded_and_signals_follow() {
        let mut rig = rig(two()).await;
        let state = wait_for(&mut rig.states, |s| s.available).await;
        assert_eq!(ids(&state), [1, 2]);
        assert!(state.settled);

        added(&rig.fake, 3).await;
        wait_for(&mut rig.states, |s| ids(s) == [1, 2, 3]).await;

        let mut replaced = testbus::notification(2, "changed");
        replaced.read = true;
        testbus::emit_private(&rig.fake, "Replaced", &(replaced,)).await;
        let state = wait_for(&mut rig.states, |s| {
            s.entries.last().is_some_and(|e| e.summary == "changed")
        })
        .await;
        assert_eq!(ids(&state), [1, 3, 2]);

        testbus::emit_private(&rig.fake, "Read", &(vec![1u32, 3],)).await;
        let state = wait_for(&mut rig.states, |s| s.entries[0].read).await;
        assert!(state.entries[1].read);

        testbus::emit_private(&rig.fake, "Closed", &(1u32, 2u32)).await;
        wait_for(&mut rig.states, |s| ids(s) == [3, 2]).await;

        testbus::emit_private(
            &rig.fake,
            "DoNotDisturbChanged",
            &(true, "fullscreen", 77i64),
        )
        .await;
        let state = wait_for(&mut rig.states, |s| s.dnd.on).await;
        assert_eq!(
            (state.dnd.reason.as_str(), state.dnd.until),
            ("fullscreen", 77)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn do_not_disturb_is_read_with_the_history() {
        let bus = TestBus::start();
        let fake = testbus::serve_notifications(&bus, two()).await;
        fake.state.lock().expect("state").dnd = (true, "manual".into(), 5, vec!["a".into()]);
        let (mut states, _commands) = spawn(&Handle::current(), Buses::with(bus.client().await));
        let state = wait_for(&mut states, |s| s.available).await;
        assert_eq!(
            state.dnd,
            Dnd {
                on: true,
                reason: "manual".into(),
                until: 5,
                unavailable: vec!["a".into()]
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_owner_leaves_and_a_new_one_is_loaded() {
        let mut rig = rig(two()).await;
        let first = wait_for(&mut rig.states, |s| s.available).await;
        rig.fake.connection.close().await.expect("close");
        let gone = wait_for(&mut rig.states, |s| !s.available).await;
        assert!(gone.entries.is_empty() && gone.settled);
        assert!(gone.generation > first.generation);

        let _second =
            testbus::serve_notifications(&rig.bus, vec![testbus::notification(9, "z")]).await;
        let back = wait_for(&mut rig.states, |s| s.available).await;
        assert_eq!(ids(&back), [9]);
        assert!(back.generation > gone.generation);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_no_daemon_the_state_is_settled_and_unavailable() {
        let bus = TestBus::start();
        let (mut states, _commands) = spawn(&Handle::current(), Buses::with(bus.client().await));
        let state = wait_for(&mut states, |s| s.settled).await;
        assert!(!state.available && state.entries.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_close_that_arrives_during_a_slow_history_is_applied_after_it() {
        let bus = TestBus::start();
        let fake = testbus::serve_notifications(&bus, two()).await;
        fake.state.lock().expect("state").delay = Duration::from_millis(200);
        let (mut states, _commands) = spawn(&Handle::current(), Buses::with(bus.client().await));
        // The fake read its history before the signal and answers 200 ms after it.
        tokio::time::sleep(Duration::from_millis(80)).await;
        testbus::emit_private(&fake, "Closed", &(1u32, 2u32)).await;
        added(&fake, 3).await;
        let state = wait_for(&mut states, |s| s.available).await;
        assert_eq!(ids(&state), [2, 3]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_signal_from_another_sender_is_ignored() {
        let mut rig = rig(two()).await;
        wait_for(&mut rig.states, |s| s.available).await;
        let other = rig.bus.client().await;
        testbus::emit_broadcast(&other, "Added", &(testbus::notification(7, "new"),)).await;
        // The daemon's own signal after it: once that is applied, the other has been seen.
        added(&rig.fake, 3).await;
        let state = wait_for(&mut rig.states, |s| s.entries.len() == 3).await;
        assert_eq!(ids(&state), [1, 2, 3]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn commands_call_the_daemon_and_a_refused_one_is_counted() {
        let mut rig = rig(two()).await;
        wait_for(&mut rig.states, |s| s.available).await;
        let send = |command| rig.commands.send(command).expect("model runs");
        send(NotificationsCommand::Close(1));
        send(NotificationsCommand::Invoke {
            id: 2,
            key: "k".into(),
            token: "t".into(),
        });
        send(NotificationsCommand::Reply {
            id: 2,
            text: "hi".into(),
        });
        send(NotificationsCommand::MarkRead(vec![1, 2]));
        send(NotificationsCommand::ClearAll);
        send(NotificationsCommand::ClearGroup("a.b".into()));
        send(NotificationsCommand::SetDnd {
            on: true,
            until: None,
        });
        send(NotificationsCommand::SetDnd {
            on: false,
            until: Some(9),
        });
        send(NotificationsCommand::Mute("a.b".into()));
        send(NotificationsCommand::ListOnly("a.b".into()));
        let fake = rig.fake.state.clone();
        tokio::time::timeout(testbus::WAIT, async {
            while fake.lock().expect("state").calls.len() < 10 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the calls arrived");
        assert_eq!(
            fake.lock().expect("state").calls,
            [
                "Close(1,2)",
                "InvokeAction(2,k,t)",
                "Reply(2,hi)",
                "MarkRead([1, 2])",
                "ClearAll",
                "ClearGroup(a.b)",
                "SetDoNotDisturb(true)",
                "SetDoNotDisturbUntil(false,9)",
                "SetRule(a.b,allowed,false)",
                "SetRule(a.b,popups,false)",
            ]
        );
        assert_eq!(rig.states.borrow().refused, 0);

        fake.lock().expect("state").refuse = true;
        send(NotificationsCommand::ClearAll);
        let state = wait_for(&mut rig.states, |s| s.refused == 1).await;
        assert_eq!(state.entries.len(), 2);
    }

    #[test]
    fn debug_output_leaves_out_the_text() {
        let state = NotificationsState {
            entries: vec![testbus::notification(1, "secret summary")],
            ..NotificationsState::default()
        };
        assert!(!format!("{state:?}").contains("secret"));
        let reply = NotificationsCommand::Reply {
            id: 1,
            text: "secret reply".into(),
        };
        assert!(!format!("{reply:?}").contains("secret"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_replace_replayed_after_a_stale_history_keeps_the_newest_last() {
        let bus = TestBus::start();
        let fake = testbus::serve_notifications(&bus, two()).await;
        fake.state.lock().expect("state").delay = Duration::from_millis(200);
        let (mut states, _commands) = spawn(&Handle::current(), Buses::with(bus.client().await));
        tokio::time::sleep(Duration::from_millis(80)).await;
        // The history ends [1, 2]; the daemon replaced 1, then added 3.
        testbus::emit_private(&fake, "Replaced", &(testbus::notification(1, "again"),)).await;
        added(&fake, 3).await;
        let state = wait_for(&mut states, |s| s.available).await;
        assert_eq!(ids(&state), [2, 1, 3]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_failed_load_is_tried_again() {
        let bus = TestBus::start();
        let fake = testbus::serve_notifications(&bus, two()).await;
        fake.state.lock().expect("state").fail_history = 1;
        let (mut states, _commands) = spawn(&Handle::current(), Buses::with(bus.client().await));
        let failed = wait_for(&mut states, |s| s.settled).await;
        assert!(!failed.available);
        let state = wait_for(&mut states, |s| s.available).await;
        assert_eq!(ids(&state), [1, 2]);
        assert_eq!(fake.state.lock().expect("state").history_calls, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refusal_of_admission_is_not_tried_again() {
        let bus = TestBus::start();
        let fake = testbus::serve_notifications(&bus, two()).await;
        fake.state.lock().expect("state").deny_history = true;
        let (mut states, _commands) = spawn(&Handle::current(), Buses::with(bus.client().await));
        wait_for(&mut states, |s| s.settled).await;
        tokio::time::sleep(backoff(0) + Duration::from_millis(700)).await;
        assert_eq!(fake.state.lock().expect("state").history_calls, 1);
        assert!(!states.borrow().available);
    }
}
