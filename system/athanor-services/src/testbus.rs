//! A private `dbus-daemon` per test, and a service on it whose objects a test adds, changes
//! and removes. The models are tested against a real bus, since a mirror follows a name's
//! owner through the bus's `NameOwnerChanged`, which a peer-to-peer connection does not have.
//! Public under the `testbus` feature for the tests of the crates that build on these models.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, fs, process};

use tokio::sync::watch;
use zbus::connection::Builder;
use zbus::fdo::ObjectManager;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::Connection;

use crate::mirror::Snapshot;
use crate::notifications::wire::WireNotification;
use crate::props::Objects;

/// The interface of the test service's objects.
pub const INTERFACE: &str = "os.athanor.Test1";
/// Where the test service serves `org.freedesktop.DBus.ObjectManager`.
pub const ROOT: &str = "/root";
/// How long [`wait_for`] waits: longer than a mirror's call timeout, so a test can watch one
/// expire.
pub const WAIT: Duration = Duration::from_secs(10);

/// A private session bus; dropping it stops the daemon and removes its socket.
pub struct TestBus {
    daemon: Child,
    pub address: String,
    dir: PathBuf,
}

impl TestBus {
    pub fn start() -> TestBus {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let dir = env::temp_dir().join(format!(
            "athanor-services-{}-{}",
            process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        // A configuration of its own: `--session` reads the host's, which on a system running
        // dbus-broker has no <listen> element, and dbus-daemon refuses to start without one.
        let config = dir.join("bus.conf");
        fs::write(
            &config,
            format!(
                "<busconfig><type>session</type><listen>unix:path={}</listen>\
                 <policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/>\
                 </policy></busconfig>",
                dir.join("bus").display()
            ),
        )
        .expect("bus.conf");
        let mut daemon = Command::new("dbus-daemon")
            .args(["--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--config-file={}", config.display()))
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon on PATH");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("stdout"))
            .read_line(&mut address)
            .expect("address");
        TestBus {
            daemon,
            address: address.trim().to_owned(),
            dir,
        }
    }

    pub fn builder(&self) -> Builder<'static> {
        Builder::address(self.address.as_str()).expect("address")
    }

    pub async fn client(&self) -> Connection {
        connect("client", self.builder().build()).await
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        fs::remove_dir_all(&self.dir).ok();
    }
}

/// A bus, a server that owns `name` with an object manager at [`ROOT`], and a client.
pub async fn start(name: &str) -> (TestBus, Connection, Connection) {
    let bus = TestBus::start();
    let server = serve(&bus, name).await;
    let client = bus.client().await;
    (bus, server, client)
}

/// A new connection that owns `name` and serves an object manager at [`ROOT`].
pub async fn serve(bus: &TestBus, name: &str) -> Connection {
    let server = bus
        .builder()
        .serve_at(ROOT, ObjectManager)
        .expect("object manager")
        .name(name.to_owned())
        .expect("name")
        .build();
    connect("server", server).await
}

/// A new connection that owns `name` and never answers `GetManagedObjects` at [`ROOT`].
pub async fn serve_stalled(bus: &TestBus, name: &str) -> Connection {
    let server = bus
        .builder()
        .serve_at(ROOT, Stalled)
        .expect("stalled object manager")
        .name(name.to_owned())
        .expect("name")
        .build();
    connect("server", server).await
}

/// A connection, or a failed test within [`WAIT`] rather than a test that hangs.
async fn connect(
    what: &str,
    connection: impl std::future::Future<Output = zbus::Result<Connection>>,
) -> Connection {
    tokio::time::timeout(WAIT, connection)
        .await
        .unwrap_or_else(|_| panic!("the {what} did not connect in time"))
        .unwrap_or_else(|err| panic!("the {what} did not connect: {err}"))
}

struct Stalled;

#[zbus::interface(name = "org.freedesktop.DBus.ObjectManager")]
impl Stalled {
    async fn get_managed_objects(
        &self,
    ) -> HashMap<OwnedObjectPath, BTreeMap<String, BTreeMap<String, OwnedValue>>> {
        std::future::pending().await
    }
}

/// An object of the test service. Its properties are values, so a test can give one the
/// wrong type.
struct TestObject {
    name: OwnedValue,
    level: OwnedValue,
}

#[zbus::interface(name = "os.athanor.Test1")]
impl TestObject {
    #[zbus(property)]
    fn name(&self) -> zbus::fdo::Result<OwnedValue> {
        Ok(self.name.try_clone().map_err(zbus::Error::from)?)
    }

    #[zbus(property)]
    fn level(&self) -> zbus::fdo::Result<OwnedValue> {
        Ok(self.level.try_clone().map_err(zbus::Error::from)?)
    }
}

fn owned(value: Value<'_>) -> OwnedValue {
    value.try_to_owned().expect("owned value")
}

/// Adds an object with `Name: s` and `Level: u`; the object manager announces it.
pub async fn add_object(server: &Connection, path: &str, name: &str, level: u32) {
    let object = TestObject {
        name: owned(Value::from(name)),
        level: owned(Value::from(level)),
    };
    assert!(server
        .object_server()
        .at(path.to_owned(), object)
        .await
        .expect("add"));
}

/// Adds an object whose `property` (`Name` or `Level`) holds `value`, of whatever type.
pub async fn add_object_raw(server: &Connection, path: &str, property: &str, value: Value<'_>) {
    let mut object = TestObject {
        name: owned(Value::from("")),
        level: owned(Value::from(0u32)),
    };
    match property {
        "Name" => object.name = owned(value),
        "Level" => object.level = owned(value),
        other => panic!("the test object has no property {other}"),
    }
    assert!(server
        .object_server()
        .at(path.to_owned(), object)
        .await
        .expect("add"));
}

/// Sets `Level` and emits `PropertiesChanged`.
pub async fn set_level(server: &Connection, path: &str, level: u32) {
    let object = server
        .object_server()
        .interface::<_, TestObject>(path.to_owned())
        .await
        .expect("object");
    object.get_mut().await.level = owned(Value::from(level));
    object
        .get()
        .await
        .level_changed(object.signal_emitter())
        .await
        .expect("PropertiesChanged");
}

/// Removes the object; the object manager announces it.
pub async fn remove_object(server: &Connection, path: &str) {
    assert!(server
        .object_server()
        .remove::<TestObject, _>(path.to_owned())
        .await
        .expect("remove"));
}

/// Emits `InterfacesAdded` for an object the server does not serve, as a service whose
/// `GetManagedObjects` hangs still announces its objects.
pub async fn announce(server: &Connection, path: &str, level: u32) {
    let emitter = SignalEmitter::new(server, ROOT).expect("emitter");
    let props = BTreeMap::from([("Level".to_owned(), owned(Value::from(level)))]);
    let interfaces = BTreeMap::from([(INTERFACE.to_owned(), props)]);
    emitter
        .emit(
            "org.freedesktop.DBus.ObjectManager",
            "InterfacesAdded",
            &(ObjectPath::try_from(path).expect("path"), interfaces),
        )
        .await
        .expect("InterfacesAdded");
}

/// Waits up to [`WAIT`] for a snapshot that satisfies `predicate`, and returns a copy of it,
/// so the test holds no borrow of the channel while the mirror publishes.
pub async fn wait_for(
    rx: &mut watch::Receiver<Snapshot>,
    mut predicate: impl FnMut(&Snapshot) -> bool,
) -> Snapshot {
    let snapshot = tokio::time::timeout(WAIT, rx.wait_for(|s| predicate(s)))
        .await
        .expect("the mirror reached the expected state in time")
        .expect("the mirror is running");
    Snapshot {
        objects: copy(&snapshot.objects),
        owner: snapshot.owner.clone(),
        generation: snapshot.generation,
    }
}

fn copy(objects: &Objects) -> Objects {
    objects
        .iter()
        .map(|(path, interfaces)| {
            let interfaces = interfaces
                .iter()
                .map(|(name, props)| {
                    let props = props
                        .iter()
                        .map(|(k, v)| (k.clone(), v.try_clone().expect("clone")))
                        .collect();
                    (name.clone(), props)
                })
                .collect();
            (path.clone(), interfaces)
        })
        .collect()
}

/// What a test sets and reads of the fake notification daemon.
#[derive(Default)]
pub struct FakeState {
    /// What `History` answers, oldest first.
    pub history: Vec<WireNotification>,
    /// How long `History` holds its answer back, after it has read `history`.
    pub delay: Duration,
    /// What `DoNotDisturb` answers: (on, reason, until, unavailable).
    pub dnd: (bool, String, i64, Vec<String>),
    /// Every call but `History` and `DoNotDisturb`, as `Method(args)`.
    pub calls: Vec<String>,
    /// Whether the commands fail.
    pub refuse: bool,
}

/// A fake `os.athanor.Notifications1`, served at the daemon's path by a connection that
/// owns `org.freedesktop.Notifications`.
pub struct FakeNotifications {
    pub connection: Connection,
    pub state: Arc<Mutex<FakeState>>,
}

struct FakePrivate(Arc<Mutex<FakeState>>);

impl FakePrivate {
    fn record(&self, call: String) -> zbus::fdo::Result<()> {
        let mut state = self.0.lock().expect("fake state");
        state.calls.push(call);
        if state.refuse {
            return Err(zbus::fdo::Error::Failed("refused".into()));
        }
        Ok(())
    }
}

#[zbus::interface(name = "os.athanor.Notifications1")]
impl FakePrivate {
    async fn history(&self) -> Vec<WireNotification> {
        let (history, delay) = {
            let state = self.0.lock().expect("fake state");
            (state.history.clone(), state.delay)
        };
        tokio::time::sleep(delay).await;
        history
    }

    async fn do_not_disturb(&self) -> (bool, String, i64, Vec<String>) {
        self.0.lock().expect("fake state").dnd.clone()
    }

    fn close(&self, id: u32, reason: u32) -> zbus::fdo::Result<()> {
        self.record(format!("Close({id},{reason})"))
    }

    fn invoke_action(&self, id: u32, key: &str, token: &str) -> zbus::fdo::Result<()> {
        self.record(format!("InvokeAction({id},{key},{token})"))
    }

    fn reply(&self, id: u32, text: &str) -> zbus::fdo::Result<()> {
        self.record(format!("Reply({id},{text})"))
    }

    fn mark_read(&self, ids: Vec<u32>) -> zbus::fdo::Result<()> {
        self.record(format!("MarkRead({ids:?})"))
    }

    fn clear_all(&self) -> zbus::fdo::Result<()> {
        self.record("ClearAll".into())
    }

    fn clear_group(&self, app: &str) -> zbus::fdo::Result<()> {
        self.record(format!("ClearGroup({app})"))
    }

    fn set_do_not_disturb(&self, on: bool) -> zbus::fdo::Result<()> {
        self.record(format!("SetDoNotDisturb({on})"))
    }

    fn set_do_not_disturb_until(&self, on: bool, until: i64) -> zbus::fdo::Result<()> {
        self.record(format!("SetDoNotDisturbUntil({on},{until})"))
    }

    fn set_rule(&self, app: &str, key: &str, value: &str) -> zbus::fdo::Result<()> {
        self.record(format!("SetRule({app},{key},{value})"))
    }
}

/// A notification with the given id and summary, and nothing else.
pub fn notification(id: u32, summary: &str) -> WireNotification {
    WireNotification {
        id,
        app_id: String::new(),
        app_name: String::new(),
        summary: summary.to_owned(),
        body: String::new(),
        body_spans: Vec::new(),
        actions: Vec::new(),
        actions_available: false,
        urgency: 1,
        transient: false,
        resident: false,
        read: false,
        time: 0,
        desktop_entry: String::new(),
        icon_name: String::new(),
        icon_file: String::new(),
        image_width: 0,
        image_height: 0,
        image_rgba: Vec::new(),
        timeout_ms: 0,
        popup_ms_left: 0,
        popup: false,
        value: -1,
        reply: false,
        reply_placeholder: String::new(),
    }
}

/// A new connection that owns `org.freedesktop.Notifications` and serves the private
/// interface with `history`.
pub async fn serve_notifications(
    bus: &TestBus,
    history: Vec<WireNotification>,
) -> FakeNotifications {
    let state = Arc::new(Mutex::new(FakeState {
        history,
        ..FakeState::default()
    }));
    let server = bus
        .builder()
        .serve_at(NOTIFICATIONS_PATH, FakePrivate(state.clone()))
        .expect("fake notifications")
        .name("org.freedesktop.Notifications")
        .expect("name")
        .build();
    FakeNotifications {
        connection: connect("notification daemon", server).await,
        state,
    }
}

/// Emits the signal `member` of the private interface from `server`; `body` is its arguments
/// as a tuple.
pub async fn emit_private(
    server: &Connection,
    member: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) {
    SignalEmitter::new(server, NOTIFICATIONS_PATH)
        .expect("emitter")
        .emit("os.athanor.Notifications1", member, body)
        .await
        .expect("signal");
}

/// The path of the private interface.
pub const NOTIFICATIONS_PATH: &str = "/os/athanor/Notifications1";
