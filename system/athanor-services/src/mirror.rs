//! A mirror of one D-Bus service's objects, kept current by its signals (doc_bar.md, BR3;
//! doc_control_center.md, CC3). The mirror follows the service's unique owner: a restart is
//! a new owner, a new generation and a fresh load, and a signal from any other sender is
//! ignored. It runs as one task on the models' runtime and publishes a [`Snapshot`] on a
//! `watch` channel, so a reader never waits on the service and never decodes a reply.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures_util::future::join_all;
use futures_util::StreamExt;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::time::{sleep_until, timeout, Instant};
use zbus::fdo::{self, DBusProxy};
use zbus::message::{Sequence, Type};
use zbus::names::BusName;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::props::{self, Change, Objects, Props, OBJECT_MANAGER, PROPERTIES};

/// How long a load waits for the service. A service that hangs is shown with no objects
/// rather than holding the mirror; the load is retried at its next owner change.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// Changes that arrive within one frame at 60 Hz reach the readers as one publish.
const COALESCE: Duration = Duration::from_millis(16);

/// What the mirror loads.
pub enum Source {
    /// `GetManagedObjects` at this path, then `InterfacesAdded`, `InterfacesRemoved` and
    /// `PropertiesChanged`.
    Managed(&'static str),
    /// `GetAll` of each (path, interface), then `PropertiesChanged`.
    Fixed(Vec<(&'static str, &'static str)>),
}

/// The mirrored service as its readers see it.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub objects: Objects,
    /// The service's unique name; `None` while no one owns its name.
    pub owner: Option<String>,
    /// Counts the owner's changes: a reply a reader requested under an older generation came
    /// from an instance that is gone.
    pub generation: u64,
}

/// Mirrors `name` on `connection` from `handle`'s runtime. The receiver starts with an empty
/// snapshot; the service appears in it when it owns its name, now or later.
pub fn spawn(
    handle: &Handle,
    connection: Connection,
    name: &'static str,
    source: Source,
) -> watch::Receiver<Snapshot> {
    let (tx, rx) = watch::channel(Snapshot::default());
    handle.spawn(async move {
        if let Err(err) = run(connection, name, source, &tx).await {
            tracing::error!(error = %err, name, "the mirror of the service stopped; it is shown as absent");
        }
        // Whatever ended the mirror, its objects are no longer kept current.
        tx.send_modify(|snapshot| {
            snapshot.objects.clear();
            snapshot.owner = None;
            snapshot.generation += 1;
        });
    });
    rx
}

async fn run(
    connection: Connection,
    name: &'static str,
    source: Source,
    tx: &watch::Sender<Snapshot>,
) -> zbus::Result<()> {
    // Subscribe before asking for the owner, so no change falls between the two.
    let dbus = DBusProxy::new(&connection).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, name)])
        .await?;
    // The bus delivers only the owner's signals; zbus cannot match a well-known sender on its
    // side, so a connection shared by several mirrors hands each of them every mirrored
    // service's signals, and `Mirror::signal` keeps the owner's.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(name)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &connection, None).await?;
    let mut mirror = Mirror {
        connection,
        name,
        source,
        tx,
        owner: None,
        load: None,
        buffered: Vec::new(),
        publish_at: None,
    };
    match dbus.get_name_owner(BusName::try_from(name)?).await {
        Ok(owner) => mirror.owner_changed(Some(owner.to_string())),
        Err(fdo::Error::NameHasNoOwner(_)) => {}
        Err(err) => return Err(err.into()),
    }
    loop {
        tokio::select! {
            change = owners.next() => {
                let Some(change) = change else { return Ok(()) };
                match change.args() {
                    Ok(args) => mirror.owner_changed(args.new_owner().as_ref().map(|owner| owner.to_string())),
                    Err(err) => tracing::warn!(error = %err, name, "NameOwnerChanged had an unexpected type"),
                }
            }
            message = signals.next() => {
                match message {
                    Some(Ok(message)) => mirror.signal(&message),
                    Some(Err(err)) => tracing::warn!(error = %err, name, "a signal could not be read"),
                    None => return Ok(()),
                }
            }
            events = loaded(&mut mirror.load) => mirror.loaded(events),
            () = until(mirror.publish_at) => mirror.publish(),
            () = tx.closed() => return Ok(()),
        }
    }
}

/// A load in flight: each reply with its receive position, or nothing for a call that failed.
type Load = Pin<Box<dyn Future<Output = Vec<(Sequence, Event)>> + Send>>;

/// What reaches the objects, in the order the connection received it.
enum Event {
    Objects(Objects),
    Interface {
        path: &'static str,
        interface: &'static str,
        props: Props,
    },
    Change(Change),
}

struct Mirror<'a> {
    connection: Connection,
    name: &'static str,
    source: Source,
    tx: &'a watch::Sender<Snapshot>,
    /// The owner the mirror follows, published or with its load in flight.
    owner: Option<String>,
    load: Option<Load>,
    /// The owner's signals received while its load is in flight, applied with the replies in
    /// the order the connection received them: a reply already holds every change signalled
    /// before it, and a change signalled after it applies on top.
    buffered: Vec<(Sequence, Change)>,
    /// When the changes applied since the last publish reach the readers.
    publish_at: Option<Instant>,
}

impl Mirror<'_> {
    /// Drops the objects of the previous owner at once, so no reader shows an instance that
    /// is gone, and loads the new owner's.
    fn owner_changed(&mut self, owner: Option<String>) {
        if owner == self.owner {
            return;
        }
        if owner.is_none() {
            tracing::info!(name = self.name, "the service left the bus");
        }
        self.load = owner
            .as_ref()
            .map(|owner| load(&self.connection, self.name, owner, &self.source));
        self.buffered.clear();
        self.publish_at = None;
        self.owner.clone_from(&owner);
        self.tx.send_modify(|snapshot| {
            snapshot.objects.clear();
            snapshot.owner = owner;
            snapshot.generation += 1;
        });
    }

    fn signal(&mut self, message: &Message) {
        let header = message.header();
        let sender = header.sender().map(|sender| sender.as_str());
        if sender.is_none() || sender != self.owner.as_deref() {
            return;
        }
        let Some(change) = props::change(message) else {
            return;
        };
        if self.load.is_some() {
            self.buffered.push((message.recv_position(), change));
            return;
        }
        // Applied now, published with the rest of the frame's changes.
        self.tx.send_if_modified(|snapshot| {
            props::apply(&mut snapshot.objects, change);
            false
        });
        self.publish_at
            .get_or_insert_with(|| Instant::now() + COALESCE);
    }

    fn loaded(&mut self, mut events: Vec<(Sequence, Event)>) {
        self.load = None;
        self.publish_at = None;
        events.extend(
            self.buffered
                .drain(..)
                .map(|(at, change)| (at, Event::Change(change))),
        );
        events.sort_by_key(|(at, _)| *at);
        self.tx.send_modify(|snapshot| {
            for (_, event) in events {
                match event {
                    Event::Objects(objects) => snapshot.objects = objects,
                    Event::Interface {
                        path,
                        interface,
                        props,
                    } => {
                        snapshot
                            .objects
                            .entry(path.to_owned())
                            .or_default()
                            .insert(interface.to_owned(), props);
                    }
                    Event::Change(change) => props::apply(&mut snapshot.objects, change),
                }
            }
        });
    }

    fn publish(&mut self) {
        self.publish_at = None;
        self.tx.send_modify(|_| ());
    }
}

/// Loads the objects from `owner`, the unique name, so every reply comes from the instance
/// this generation belongs to. The calls run together, each bounded by [`TIMEOUT`].
fn load(connection: &Connection, name: &'static str, owner: &str, source: &Source) -> Load {
    let requests: Vec<(&'static str, Option<&'static str>)> = match source {
        Source::Managed(root) => vec![(*root, None)],
        Source::Fixed(list) => list
            .iter()
            .map(|(path, interface)| (*path, Some(*interface)))
            .collect(),
    };
    let (connection, owner) = (connection.clone(), owner.to_owned());
    Box::pin(async move {
        let calls = requests
            .into_iter()
            .map(|(path, interface)| call(&connection, name, &owner, path, interface));
        join_all(calls).await.into_iter().flatten().collect()
    })
}

async fn call(
    connection: &Connection,
    name: &'static str,
    owner: &str,
    path: &'static str,
    interface: Option<&'static str>,
) -> Option<(Sequence, Event)> {
    let reply = match interface {
        None => {
            timeout(
                TIMEOUT,
                connection.call_method(
                    Some(owner),
                    path,
                    Some(OBJECT_MANAGER),
                    "GetManagedObjects",
                    &(),
                ),
            )
            .await
        }
        Some(interface) => {
            timeout(
                TIMEOUT,
                connection.call_method(
                    Some(owner),
                    path,
                    Some(PROPERTIES),
                    "GetAll",
                    &(interface,),
                ),
            )
            .await
        }
    };
    let reply = match reply {
        Ok(Ok(reply)) => reply,
        Ok(Err(err)) => {
            tracing::warn!(error = %err, name, path, "the service refused to load; it is shown with no objects until it restarts");
            return None;
        }
        Err(_) => {
            tracing::warn!(name, path, "the service did not answer within 5 s; it is shown with no objects until it restarts");
            return None;
        }
    };
    let event = match interface {
        None => props::managed_objects(&reply).map(Event::Objects),
        Some(interface) => props::get_all(&reply).map(|props| Event::Interface {
            path,
            interface,
            props,
        }),
    };
    if event.is_none() {
        tracing::error!(
            name,
            path,
            "the service answered its load with an unexpected type"
        );
    }
    Some((reply.recv_position(), event?))
}

async fn loaded(load: &mut Option<Load>) -> Vec<(Sequence, Event)> {
    match load {
        Some(load) => load.await,
        None => std::future::pending().await,
    }
}

async fn until(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::runtime::Handle;
    use zbus::zvariant;

    use super::*;
    use crate::props;
    use crate::testbus;

    const NAME: &str = "os.athanor.Test";

    fn level(snapshot: &Snapshot, path: &str) -> Option<u32> {
        props::get_u32(
            props::lookup(&snapshot.objects, path, testbus::INTERFACE)?,
            "Level",
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_objects_are_loaded_and_follow_signals() {
        let (_bus, server, client) = testbus::start(NAME).await;
        testbus::add_object(&server, "/root/a", "Alpha", 10).await;
        let mut rx = spawn(&Handle::current(), client, NAME, Source::Managed("/root"));
        let snap = testbus::wait_for(&mut rx, |s| s.objects.contains_key("/root/a")).await;
        assert_eq!(
            props::get_u32(&snap.objects["/root/a"]["os.athanor.Test1"], "Level"),
            Some(10)
        );
        testbus::set_level(&server, "/root/a", 42).await;
        let snap = testbus::wait_for(&mut rx, |s| level(s, "/root/a") == Some(42)).await;
        assert!(snap.generation >= 1);
        testbus::remove_object(&server, "/root/a").await;
        testbus::wait_for(&mut rx, |s| !s.objects.contains_key("/root/a")).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_property_of_the_wrong_type_is_absent_not_fatal() {
        let (_bus, server, client) = testbus::start(NAME).await;
        testbus::add_object_raw(
            &server,
            "/root/b",
            "Level",
            zvariant::Value::from("not a u32"),
        )
        .await;
        let mut rx = spawn(&Handle::current(), client, NAME, Source::Managed("/root"));
        let snap = testbus::wait_for(&mut rx, |s| s.objects.contains_key("/root/b")).await;
        assert_eq!(
            props::get_u32(&snap.objects["/root/b"]["os.athanor.Test1"], "Level"),
            None
        );
        // The mirror lives on: a later object of the right type still arrives.
        testbus::add_object(&server, "/root/c", "Gamma", 3).await;
        testbus::wait_for(&mut rx, |s| level(s, "/root/c") == Some(3)).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fixed_objects_are_loaded_with_get_all_and_follow_signals() {
        let (_bus, server, client) = testbus::start(NAME).await;
        testbus::add_object(&server, "/root/a", "Alpha", 10).await;
        let source = Source::Fixed(vec![("/root/a", testbus::INTERFACE)]);
        let mut rx = spawn(&Handle::current(), client, NAME, source);
        testbus::wait_for(&mut rx, |s| level(s, "/root/a") == Some(10)).await;
        testbus::set_level(&server, "/root/a", 11).await;
        testbus::wait_for(&mut rx, |s| level(s, "/root/a") == Some(11)).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_owner_leaves_and_returns() {
        let (bus, server, client) = testbus::start(NAME).await;
        testbus::add_object(&server, "/root/a", "Alpha", 1).await;
        let mut rx = spawn(&Handle::current(), client, NAME, Source::Managed("/root"));
        let first = testbus::wait_for(&mut rx, |s| s.objects.contains_key("/root/a")).await;
        assert!(first.owner.is_some());

        server.close().await.expect("close");
        let gone = testbus::wait_for(&mut rx, |s| s.owner.is_none()).await;
        assert!(gone.objects.is_empty());
        assert!(gone.generation > first.generation);

        let server = testbus::serve(&bus, NAME).await;
        testbus::add_object(&server, "/root/z", "Zeta", 2).await;
        let back = testbus::wait_for(&mut rx, |s| level(s, "/root/z") == Some(2)).await;
        assert!(back.owner.is_some());
        assert_ne!(back.owner, first.owner);
        assert!(back.generation > gone.generation);
        assert!(!back.objects.contains_key("/root/a"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_load_that_never_answers_does_not_stall() {
        let bus = testbus::TestBus::start();
        let stalled = testbus::serve_stalled(&bus, NAME).await;
        let other = testbus::serve(&bus, "os.athanor.Other").await;
        testbus::add_object(&other, "/root/a", "Alpha", 5).await;
        let client = bus.client().await;
        let started = Instant::now();
        let mut hung = spawn(
            &Handle::current(),
            client.clone(),
            NAME,
            Source::Managed("/root"),
        );
        let mut live = spawn(
            &Handle::current(),
            client,
            "os.athanor.Other",
            Source::Managed("/root"),
        );

        // The other mirror, on the same connection, publishes while the first load hangs.
        testbus::wait_for(&mut live, |s| level(s, "/root/a") == Some(5)).await;
        assert!(started.elapsed() < TIMEOUT, "{:?}", started.elapsed());
        let snap = testbus::wait_for(&mut hung, |s| s.owner.is_some()).await;
        assert!(snap.objects.is_empty());

        // Once the call has given up, the mirror follows the owner's signals again.
        tokio::time::sleep_until((started + TIMEOUT + Duration::from_millis(500)).into()).await;
        testbus::announce(&stalled, "/root/late", 9).await;
        testbus::wait_for(&mut hung, |s| level(s, "/root/late") == Some(9)).await;
        // The other mirror shares the connection and so receives that signal too, from a
        // sender that does not own its name.
        tokio::time::sleep(COALESCE * 4).await;
        assert!(!live.borrow().objects.contains_key("/root/late"));
    }
}
