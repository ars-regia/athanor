//! What the logind tests share: a private `dbus-daemon`, a fake logind on it, and the main
//! loop's side of the logind thread, a calloop loop woken by the thread's ping.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use athanor_lock::logind::{self, Call, Report};
use calloop::EventLoop;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use zbus::OwnedObjectPath;

/// How long a test waits for anything.
pub const WAIT: Duration = Duration::from_secs(10);

const SESSION: &str = "/org/freedesktop/login1/session/c1";

/// A private bus; dropping it stops the daemon and removes its socket.
pub struct Bus {
    daemon: Child,
    pub address: String,
    dir: PathBuf,
}

impl Bus {
    pub fn start() -> Bus {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let dir = env::temp_dir().join(format!(
            "athanor-lock-{}-{}",
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
        assert!(!address.trim().is_empty(), "dbus-daemon printed no address");
        Bus {
            daemon,
            address: address.trim().to_owned(),
            dir,
        }
    }

    /// Ends the daemon, and with it every connection to it.
    #[allow(dead_code)] // each test binary compiles this module
    pub fn kill(&mut self) {
        self.daemon.kill().expect("kill dbus-daemon");
        self.daemon.wait().expect("reap dbus-daemon");
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        fs::remove_dir_all(&self.dir).ok();
    }
}

/// What the fake logind was asked.
#[derive(Debug, PartialEq, Eq)]
pub enum Record {
    Inhibit([String; 4]),
    SetLockedHint(bool),
    /// The last copy of an inhibitor's descriptor was closed.
    Released,
}

/// A signal for the fake logind to emit.
#[derive(Clone, Copy)]
#[allow(dead_code)] // each test binary compiles this module
pub enum Emit {
    Lock,
    Unlock,
    PrepareForSleep(bool),
}

/// The session the fake logind names as the user's `Display`.
pub struct Setup {
    /// The session id; empty for a user with no display session.
    pub id: &'static str,
    pub kind: &'static str,
    /// The session belongs to another uid than the test's.
    pub other_uid: bool,
    pub locked_hint: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            id: "c1",
            kind: "wayland",
            other_uid: false,
            locked_hint: false,
        }
    }
}

struct Manager {
    records: Sender<Record>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    async fn inhibit(
        &self,
        what: String,
        who: String,
        why: String,
        mode: String,
    ) -> zbus::fdo::Result<zbus::OwnedFd> {
        let (mine, theirs) = std::os::unix::net::UnixStream::pair()
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        mine.set_nonblocking(true).expect("nonblocking");
        let mut mine = tokio::net::UnixStream::from_std(mine).expect("tokio stream");
        self.records
            .send(Record::Inhibit([what, who, why, mode]))
            .expect("record");
        let records = self.records.clone();
        tokio::spawn(async move {
            // Nothing is ever written: the read ends when the lock closes its copy.
            let read = mine.read(&mut [0; 1]).await.expect("read");
            assert_eq!(read, 0);
            records.send(Record::Released).ok();
        });
        Ok(std::os::fd::OwnedFd::from(theirs).into())
    }
}

struct User {
    display: (String, OwnedObjectPath),
}

#[zbus::interface(name = "org.freedesktop.login1.User")]
impl User {
    #[zbus(property)]
    fn display(&self) -> (String, OwnedObjectPath) {
        self.display.clone()
    }
}

struct Session {
    kind: String,
    uid: u32,
    locked_hint: bool,
    records: Sender<Record>,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        self.kind.clone()
    }

    #[zbus(property)]
    fn user(&self) -> (u32, OwnedObjectPath) {
        (
            self.uid,
            OwnedObjectPath::try_from("/org/freedesktop/login1/user/_1").expect("path"),
        )
    }

    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.locked_hint
    }

    fn set_locked_hint(&mut self, locked: bool) {
        self.locked_hint = locked;
        self.records
            .send(Record::SetLockedHint(locked))
            .expect("record");
    }
}

/// A fake logind on a thread and a current-thread runtime of its own.
pub struct Fake {
    pub records: Receiver<Record>,
    emit: UnboundedSender<Emit>,
}

impl Fake {
    /// Returns once the fake owns `org.freedesktop.login1`.
    pub fn start(bus: &Bus, setup: Setup) -> Fake {
        let (records, recorded) = channel();
        let (emit, mut emitted) = unbounded_channel();
        let (ready, started) = channel();
        let address = bus.address.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                let connection = zbus::connection::Builder::address(address.as_str())
                    .build()
                    .await
                    .expect("fake logind's connection");
                let me = connection.unique_name().expect("unique name").clone();
                let mine = zbus::fdo::DBusProxy::new(&connection)
                    .await
                    .expect("bus proxy")
                    .get_connection_unix_user(me.as_ref().into())
                    .await
                    .expect("own uid");
                let server = connection.object_server();
                server
                    .at(
                        "/org/freedesktop/login1",
                        Manager {
                            records: records.clone(),
                        },
                    )
                    .await
                    .expect("manager");
                server
                    .at(
                        "/org/freedesktop/login1/user/self",
                        User {
                            display: (
                                setup.id.to_owned(),
                                OwnedObjectPath::try_from(if setup.id.is_empty() {
                                    "/"
                                } else {
                                    SESSION
                                })
                                .expect("path"),
                            ),
                        },
                    )
                    .await
                    .expect("user");
                server
                    .at(
                        SESSION,
                        Session {
                            kind: setup.kind.to_owned(),
                            uid: if setup.other_uid {
                                mine.wrapping_add(1)
                            } else {
                                mine
                            },
                            locked_hint: setup.locked_hint,
                            records,
                        },
                    )
                    .await
                    .expect("session");
                connection
                    .request_name("org.freedesktop.login1")
                    .await
                    .expect("name");
                ready.send(()).expect("ready");
                while let Some(next) = emitted.recv().await {
                    let sent = match next {
                        Emit::Lock => signal(&connection, SESSION, "Session", "Lock", &()).await,
                        Emit::Unlock => {
                            signal(&connection, SESSION, "Session", "Unlock", &()).await
                        }
                        Emit::PrepareForSleep(start) => {
                            signal(
                                &connection,
                                "/org/freedesktop/login1",
                                "Manager",
                                "PrepareForSleep",
                                &start,
                            )
                            .await
                        }
                    };
                    sent.expect("emit");
                }
            });
        });
        started.recv_timeout(WAIT).expect("the fake logind started");
        Fake {
            records: recorded,
            emit,
        }
    }

    #[allow(dead_code)] // each test binary compiles this module
    pub fn emit(&self, signal: Emit) {
        self.emit.send(signal).expect("the fake logind runs");
    }

    #[allow(dead_code)] // each test binary compiles this module
    pub fn record(&self) -> Record {
        self.records.recv_timeout(WAIT).expect("a record")
    }
}

async fn signal(
    connection: &zbus::Connection,
    path: &str,
    interface: &str,
    name: &str,
    body: &(impl zbus::export::serde::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<()> {
    connection
        .emit_signal(
            None::<zbus::names::BusName<'_>>,
            path,
            format!("org.freedesktop.login1.{interface}").as_str(),
            name,
            body,
        )
        .await
}

/// The main loop's side of the logind thread: a calloop loop with the thread's ping in it.
pub struct Lock {
    event_loop: EventLoop<'static, bool>,
    reports: Receiver<Report>,
    queue: VecDeque<Report>,
    pub calls: UnboundedSender<Call>,
}

impl Lock {
    pub fn start(bus: &Bus) -> Lock {
        let (ping, source) = calloop::ping::make_ping().expect("ping");
        let event_loop = EventLoop::try_new().expect("event loop");
        event_loop
            .handle()
            .insert_source(source, |_, _, woken: &mut bool| *woken = true)
            .expect("ping source");
        let (sender, reports) = channel();
        let calls = logind::spawn(&bus.address, sender, ping).expect("logind thread");
        Lock {
            event_loop,
            reports,
            queue: VecDeque::new(),
            calls,
        }
    }

    /// The next report, read only once the ping has woken the loop.
    pub fn next(&mut self) -> Report {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(report) = self.queue.pop_front() {
                return report;
            }
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("a report within the wait");
            let mut woken = false;
            self.event_loop
                .dispatch(Some(left), &mut woken)
                .expect("dispatch");
            if woken {
                self.queue.extend(self.reports.try_iter());
            }
        }
    }

    #[allow(dead_code)] // each test binary compiles this module
    pub fn call(&self, call: Call) {
        self.calls.send(call).expect("the logind thread runs");
    }
}
